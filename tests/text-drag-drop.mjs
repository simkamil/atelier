// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { attachTextDragDrop } from "../web/text-drag-drop.mjs";

class Target {
  listeners = new Map();
  addEventListener(type, listener, options = false) {
    const values = this.listeners.get(type) ?? [];
    values.push({ listener, capture: options === true || Boolean(options?.capture) });
    this.listeners.set(type, values);
  }
  removeEventListener(type, listener, options = false) {
    const capture = options === true || Boolean(options?.capture);
    this.listeners.set(type, (this.listeners.get(type) ?? []).filter(item => item.listener !== listener || item.capture !== capture));
  }
  emit(type, values = {}) {
    let stopped = false;
    const event = { type, preventDefault() { this.defaultPrevented = true; },
      stopImmediatePropagation() { stopped = true; this.immediateStopped = true; }, ...values };
    const listeners = this.listeners.get(type) ?? [];
    for (const item of [...listeners.filter(value => value.capture), ...listeners.filter(value => !value.capture)]) {
      if (!stopped) item.listener(event);
    }
    return event;
  }
}

const tick = () => new Promise(resolve => setTimeout(resolve, 0));
async function settle() { for (let i = 0; i < 10; i += 1) await tick(); }

function fixture({ enabled = () => true, selection = { anchor: 5, focus: 1 }, text = "a🙂bc",
  pages = [{ minX: 10, minY: 20, width: 100, height: 100 }], zoom = 1, pageGap = 16,
  canvasRect = { left: 100, top: 100, width: 200, height: 200 },
  scrollRect = { left: 100, top: 100, width: 100, height: 100 },
  scrollValues = { clientWidth: 100, clientHeight: 100, scrollWidth: 100, scrollHeight: 100, scrollLeft: 0, scrollTop: 0 },
  probePoint = async () => ({ offset: 6, revision: 3n, caret: { page: 0, x: 20, y: 30, height: 12 } }) } = {}) {
  const canvas = new Target();
  const scrollport = new Target();
  const captured = new Set();
  canvas.getBoundingClientRect = () => canvasRect;
  canvas.setPointerCapture = id => captured.add(id);
  canvas.releasePointerCapture = id => captured.delete(id);
  canvas.hasPointerCapture = id => captured.has(id);
  scrollport.getBoundingClientRect = () => scrollRect;
  Object.assign(scrollport, scrollValues);
  const calls = { probes: [], drops: [], clicks: [], markers: [], errors: [], focuses: 0, bubbleDown: 0 };
  canvas.addEventListener("pointerdown", () => { calls.bubbleDown += 1; });
  const controller = attachTextDragDrop({ canvas, scrollport,
    getGeometry: () => ({ pages, zoom, pageGap }),
    getSelectionSnapshot: () => ({ text, ...selection, revision: 3n }),
    isPointInSelection: () => true,
    probePoint: async point => { calls.probes.push(point); return probePoint(point); },
    onDrop: async value => calls.drops.push(value), onClick: value => calls.clicks.push(value),
    onDragState: value => calls.markers.push(value), isEnabled: enabled,
    focus: () => { calls.focuses += 1; }, onError: error => calls.errors.push(error) });
  return { canvas, scrollport, captured, calls, controller };
}

function down(canvas, x = 110, y = 120, extra = {}) {
  return canvas.emit("pointerdown", { button: 0, pointerId: 4, clientX: x, clientY: y, detail: 1, ...extra });
}
function move(canvas, x, y = 120) { return canvas.emit("pointermove", { pointerId: 4, clientX: x, clientY: y }); }
function up(canvas, x, y = 120, extra = {}) { return canvas.emit("pointerup", { pointerId: 4, clientX: x, clientY: y, ...extra }); }

{
  for (const modifier of ["shiftKey", "altKey", "metaKey"]) {
    const f = fixture();
    const event = down(f.canvas, 110, 120, { [modifier]: true });
    assert.equal(event.defaultPrevented, undefined, `${modifier} stays available to mouse selection`);
    assert.equal(event.immediateStopped, undefined);
    assert.equal(f.calls.bubbleDown, 1);
    assert.equal(f.calls.probes.length, 0);
    assert.equal(f.captured.size, 0);
    f.controller.dispose();
  }
}

{
  const f = fixture();
  const start = down(f.canvas);
  assert.equal(start.defaultPrevented, true);
  assert.equal(start.immediateStopped, true, "capture listener prevents the selection controller from taking over");
  assert.equal(f.calls.bubbleDown, 0);
  move(f.canvas, 120);
  up(f.canvas, 120);
  await settle();
  assert.equal(f.calls.drops.length, 1, JSON.stringify({ errors: f.calls.errors.map(error => error.message), probes: f.calls.probes }));
  assert.deepEqual(f.calls.drops[0], { kind: "move", revision: 3n, sourceAnchor: 5, sourceFocus: 1, target: 6, text: "a🙂bc" });
  assert.equal(f.calls.probes.length, 2, "a running move probe is followed by one exact release probe");
  assert.equal(f.captured.size, 0);
  assert.deepEqual(f.calls.markers.at(-1), null);
  assert.equal(f.calls.focuses, 1);
  f.controller.dispose();
}

{
  const f = fixture();
  down(f.canvas);
  move(f.canvas, 120);
  up(f.canvas, 120, 120, { ctrlKey: true });
  await settle();
  assert.equal(f.calls.drops[0].kind, "copy", "Ctrl is sampled on release");
  f.controller.dispose();
}

{
  const f = fixture({ probePoint: async () => ({ offset: 5, revision: 3n,
    caret: { page: 0, x: 15, y: 30, height: 12 } }) });
  down(f.canvas);
  move(f.canvas, 120);
  up(f.canvas, 120, 120, { ctrlKey: true });
  await settle();
  assert.equal(f.calls.drops.length, 1, "copying into the source selection is allowed");
  assert.equal(f.calls.drops[0].kind, "copy");
  f.controller.dispose();
}

{
  const f = fixture({ probePoint: async () => ({ offset: 5, revision: 3n,
    caret: { page: 0, x: 15, y: 30, height: 12 } }) });
  down(f.canvas);
  move(f.canvas, 120);
  up(f.canvas, 120);
  await settle();
  assert.equal(f.calls.drops.length, 0, "a target at the source edge is a no-op");
  assert.equal(f.calls.errors.length, 0);
  f.controller.dispose();
}

{
  const f = fixture({ probePoint: async () => ({ offset: 5, revision: 3n,
    caret: { page: 0, x: 12, y: 30, height: 12 } }) });
  down(f.canvas);
  up(f.canvas, 111);
  await settle();
  assert.equal(f.calls.drops.length, 0, "sub-four-pixel press is a click, not a drag");
  assert.equal(f.calls.clicks.length, 1);
  assert.equal(f.calls.clicks[0].offset, 5);
  assert.equal(f.captured.size, 0);
  f.controller.dispose();
}

{
  let enabled = false;
  const f = fixture({ enabled: () => enabled });
  assert.equal(f.canvas.listeners.get("drop")?.length, 1, "external drop handler is attached");
  const event = f.canvas.emit("drop", { clientX: 110, clientY: 120,
    dataTransfer: { types: ["text/plain", "text/html"], getData: type => type === "text/plain" ? "line\n🙂" : "<b>x</b>" } });
  await settle();
  assert.equal(event.defaultPrevented, true, "plain-text drops never trigger browser navigation while busy");
  assert.equal(f.calls.drops.length, 0);
  enabled = true;
  const transfer = { active: true, types: ["text/plain"], getData() {
    if (!this.active) throw new Error("DataTransfer read after drop dispatch");
    return "line\n🙂";
  } };
  const dragOverEvent = f.canvas.emit("dragover", { clientX: 110, clientY: 120, dataTransfer: transfer });
  assert.equal(dragOverEvent.defaultPrevented, true);
  await settle();
  assert.deepEqual(f.calls.markers.at(-1), { page: 0, x: 20, y: 30, height: 12 }, "external dragover displays the insertion marker");
  f.canvas.emit("drop", { clientX: 110, clientY: 120, dataTransfer: transfer });
  transfer.active = false;
  await settle();
  assert.equal(f.calls.drops.length, 1);
  assert.deepEqual(f.calls.drops[0], { kind: "external", revision: 3n, sourceAnchor: 5, sourceFocus: 1, target: 6, text: "line\n🙂" });
  assert.equal(f.calls.focuses, 1, "successful external insertion restores hidden input focus");
  assert.equal(f.calls.markers.at(-1), null, "drop clears the insertion marker");
  f.canvas.emit("dragover", { clientX: 110, clientY: 120, dataTransfer: { types: ["text/plain"] } });
  await settle();
  f.canvas.emit("dragleave");
  assert.equal(f.calls.markers.at(-1), null, "dragleave clears the insertion marker");
  f.controller.dispose();
}

{
  const f = fixture();
  for (const types of [["text/html"], ["Files"]]) {
    const transfer = { types, getData() { throw new Error("non-plain data must be ignored"); } };
    const over = f.canvas.emit("dragover", { clientX: 110, clientY: 120, dataTransfer: transfer });
    const dropped = f.canvas.emit("drop", { clientX: 110, clientY: 120, dataTransfer: transfer });
    assert.equal(over.defaultPrevented, true, `${types[0]} dragover is suppressed`);
    assert.equal(dropped.defaultPrevented, true, `${types[0]} drop is suppressed`);
  }
  await settle();
  assert.equal(f.calls.drops.length, 0, "HTML and file payloads are ignored");
  assert.equal(f.calls.probes.length, 0);
  f.controller.dispose();
}

{
  const points = [];
  const f = fixture({
    pages: [{ minX: 10, minY: 20, width: 200, height: 300 }, { minX: 5, minY: 400, width: 100, height: 200 }],
    zoom: 1.5, pageGap: 20,
    canvasRect: { left: 100, top: 50, width: 400, height: 600 },
    scrollRect: { left: 100, top: 50, width: 100, height: 100 },
    scrollValues: { clientWidth: 100, clientHeight: 100, scrollWidth: 500, scrollHeight: 1000, scrollLeft: 30, scrollTop: 80 },
    probePoint: async point => { points.push(point); return { offset: 6, revision: 3n,
      caret: { page: point.page, x: point.x, y: point.y, height: 12 } }; },
  });
  f.canvas.emit("dragover", { clientX: 125, clientY: 140, dataTransfer: { types: ["text/plain"] } });
  await settle();
  assert.equal(points[0].page, 0);
  assert.ok(Math.abs(points[0].x - 46.666666666666664) < 1e-9 && Math.abs(points[0].y - 133.33333333333334) < 1e-9,
    "page-one coordinates include zoom and horizontal/vertical scroll");
  f.canvas.emit("dragover", { clientX: 250, clientY: 445, dataTransfer: { types: ["text/plain"] } });
  await settle();
  assert.equal(points[1].page, 1);
  assert.ok(Math.abs(points[1].x - 105) < 1e-9 && Math.abs(points[1].y - 403.3333333333333) < 1e-9,
    "page-two coordinates include inter-page gap, zoom and scroll");
  f.controller.dispose();
}

{
  const originalRaf = globalThis.requestAnimationFrame;
  const originalCaf = globalThis.cancelAnimationFrame;
  let id = 0;
  const frames = new Map();
  globalThis.requestAnimationFrame = callback => { frames.set(++id, callback); return id; };
  globalThis.cancelAnimationFrame = frame => frames.delete(frame);
  try {
    const options = { scrollValues: { clientWidth: 100, clientHeight: 100, scrollWidth: 100, scrollHeight: 220, scrollLeft: 0, scrollTop: 0 } };
    const f = fixture(options);
    down(f.canvas, 120, 120);
    move(f.canvas, 125, 195);
    let spins = 0;
    while (frames.size && spins++ < 30) {
      const [frame, callback] = frames.entries().next().value;
      frames.delete(frame);
      callback();
    }
    assert.equal(f.scrollport.scrollTop, 120, "auto-scroll stops at the content boundary");
    assert.equal(frames.size, 0, "RAF stops scheduling at the edge");
    f.controller.cancel();
    f.controller.dispose();

    const pending = fixture(options);
    down(pending.canvas, 120, 120);
    move(pending.canvas, 125, 195);
    assert.equal(frames.size, 1, "edge drag schedules one frame");
    pending.controller.cancel();
    assert.equal(frames.size, 0, "cancel removes the pending auto-scroll frame");
    pending.controller.dispose();
  } finally {
    if (originalRaf === undefined) delete globalThis.requestAnimationFrame;
    else globalThis.requestAnimationFrame = originalRaf;
    if (originalCaf === undefined) delete globalThis.cancelAnimationFrame;
    else globalThis.cancelAnimationFrame = originalCaf;
  }
}

{
  let resolve;
  const f = fixture({ probePoint: () => new Promise(done => { resolve = done; }) });
  down(f.canvas);
  move(f.canvas, 120);
  await settle();
  f.canvas.emit("pointercancel", { pointerId: 4 });
  resolve({ offset: 6, revision: 3n, caret: { page: 0, x: 20, y: 30, height: 12 } });
  await settle();
  assert.equal(f.calls.drops.length, 0);
  assert.deepEqual(f.calls.markers, [null], "a canceled stale probe cannot show its marker");
  assert.equal(f.captured.size, 0);
  f.controller.dispose();
}

{
  let resolve;
  const transfer = { types: ["text/plain"], getData: () => "outside" };
  const f = fixture({ probePoint: () => new Promise(done => { resolve = done; }) });
  f.canvas.emit("drop", { clientX: 110, clientY: 120, dataTransfer: transfer });
  await settle();
  f.controller.cancel(); // Also covers Escape's cancellation path.
  resolve({ offset: 6, revision: 3n, caret: { page: 0, x: 20, y: 30, height: 12 } });
  await settle();
  assert.equal(f.calls.drops.length, 0, "cancel invalidates a pending external drop probe");
  f.controller.dispose();
}

{
  const f = fixture({ probePoint: async () => ({ offset: 2, revision: 4n,
    caret: { page: 0, x: 12, y: 30, height: 12 } }) });
  down(f.canvas);
  move(f.canvas, 120);
  up(f.canvas, 120);
  await settle();
  assert.equal(f.calls.drops.length, 0, "stale probe result cannot commit a drop");
  assert.equal(f.calls.markers.at(-1), null);
  assert.equal(f.calls.errors[0]?.message, "EDITOR_STALE_EDIT");
  f.controller.dispose();
}

{
  const f = fixture({ probePoint: async () => ({ offset: 2, revision: 3n,
    caret: { page: 0, x: Number.NaN, y: 30, height: 12 } }) });
  down(f.canvas);
  move(f.canvas, 120);
  up(f.canvas, 120);
  await settle();
  assert.equal(f.calls.drops.length, 0, "malformed probe output cannot commit");
  assert.equal(f.captured.size, 0);
  assert.equal(f.calls.markers.at(-1), null);
  f.controller.dispose();
}

{
  const f = fixture();
  down(f.canvas);
  f.controller.cancel();
  assert.equal(f.captured.size, 0);
  assert.deepEqual(f.calls.markers, [null]);
  f.controller.dispose();
  assert.equal(f.canvas.listeners.get("pointerdown").length, 1, "dispose removes its listener and leaves unrelated handlers intact");
}

console.log("Text drag/drop: move/copy, click, inside no-op, external text/plain, stale probes, malformed results and cleanup passed.");
