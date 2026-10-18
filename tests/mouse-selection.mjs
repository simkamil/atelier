// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { attachMouseSelection } from "../web/mouse-selection.mjs";
import { selectEngine } from "./engine-adapter.mjs";

selectEngine("mouse-selection", "node tests/mouse-selection.mjs [--engine v1|v2]");

class Target {
  listeners = new Map();
  addEventListener(type, listener) {
    const values = this.listeners.get(type) ?? [];
    values.push(listener);
    this.listeners.set(type, values);
  }
  removeEventListener(type, listener) {
    this.listeners.set(type, (this.listeners.get(type) ?? []).filter(value => value !== listener));
  }
  emit(type, values = {}) {
    const event = { type, preventDefault() { this.defaultPrevented = true; }, ...values };
    for (const listener of this.listeners.get(type) ?? []) listener(event);
    return event;
  }
}

function fixture({ pages = [{ minX: 10, minY: 20, width: 100, height: 100 }], zoom = 1,
  pageGap = 16, selection = { anchor: 0, focus: 0 }, selectPoint = async () => ({ anchor: 4, focus: 4 }),
  enabled = () => true } = {}) {
  const canvas = new Target();
  const scrollport = new Target();
  const selected = [];
  const errors = [];
  const focuses = [];
  const captured = new Set();
  canvas.getBoundingClientRect = () => ({ left: 100, top: 100, width: 100, height: 100 });
  canvas.setPointerCapture = id => captured.add(id);
  canvas.releasePointerCapture = id => captured.delete(id);
  canvas.hasPointerCapture = id => captured.has(id);
  scrollport.getBoundingClientRect = () => ({ left: 100, top: 100, width: 100, height: 100 });
  Object.assign(scrollport, { clientWidth: 100, clientHeight: 100, scrollWidth: 100, scrollHeight: 100,
    scrollLeft: 0, scrollTop: 0 });
  const controller = attachMouseSelection({ canvas, scrollport,
    getGeometry: () => ({ pages, zoom, pageGap }), getSelection: () => selection,
    selectPoint: async values => { selected.push(values); return selectPoint(values); },
    focus: () => focuses.push(true), isEnabled: enabled, onError: error => errors.push(error) });
  return { canvas, scrollport, selected, errors, focuses, captured, controller };
}

const click = (target, x, y, extra = {}) => target.emit("pointerdown", {
  button: 0, pointerId: 7, clientX: x, clientY: y, detail: 1, ...extra,
});
const move = (target, x, y) => target.emit("pointermove", { pointerId: 7, clientX: x, clientY: y });
const up = (target, x, y) => target.emit("pointerup", { pointerId: 7, clientX: x, clientY: y });
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
async function settle() { for (let i = 0; i < 8; i += 1) await tick(); }

{
  const f = fixture();
  click(f.canvas, 125, 140);
  move(f.canvas, 145, 160);
  up(f.canvas, 900, 900); // Capture keeps receiving the final point outside the Canvas.
  await settle();
  assert.equal(f.selected.length, 2, "down and coalesced final point reach the engine");
  assert.deepEqual(f.selected[0], { page: 0, x: 35, y: 60, unit: "character", extend: false, anchor: 0, anchorEnd: 0 });
  assert.equal(f.selected[1].extend, true);
  assert.deepEqual([f.selected[1].x, f.selected[1].y], [110, 120], "outside point clamps to page edge");
  assert.equal(f.captured.size, 0, "pointer capture releases after pointerup");
  assert.equal(f.focuses.length, 1, "focus restores after final selection");
  f.controller.dispose();
}

{
  const f = fixture({ selection: { anchor: 30, focus: 10 } });
  click(f.canvas, 120, 120, { shiftKey: true });
  move(f.canvas, 130, 130);
  up(f.canvas, 130, 130);
  await settle();
  assert.equal(f.selected[0].extend, true);
  assert.equal(f.selected[0].anchor, 30, "Shift retains the directed anchor");
  assert.equal(f.selected[0].anchorEnd, 30, "character selection uses a single anchor");
  assert.equal(f.selected[1].anchor, 30, "drag does not replace the Shift anchor with the latest focus");
  f.controller.dispose();
}

{
  const f = fixture({ pages: [
    { minX: 10, minY: 20, width: 100, height: 100 },
    { minX: 10, minY: 120, width: 100, height: 100 },
  ], zoom: 2, pageGap: 20 });
  click(f.canvas, 150, 130, { detail: 2 });
  up(f.canvas, 150, 330);
  await settle();
  assert.equal(f.selected[0].unit, "word", "double click selects a word");
  assert.deepEqual([f.selected[0].x, f.selected[0].y], [35, 35], "zoom maps CSS pixels to page points");
  assert.equal(f.selected[1].page, 1, "drag crosses page gaps to the nearest second page");
  assert.equal(f.selected[1].y, 125);
  f.controller.dispose();
}

{
  const f = fixture();
  click(f.canvas, 130, 130, { detail: 3 });
  up(f.canvas, 140, 140);
  await settle();
  assert.equal(f.selected[0].unit, "paragraph", "triple click selects a paragraph");
  assert.equal(f.selected[1].unit, "paragraph", "paragraph drag extends in paragraph units");
  assert.ok(f.selected[1].anchor <= f.selected[1].anchorEnd, "paragraph base range is normalized");
  f.controller.dispose();
}

{
  let resolveInitial;
  const f = fixture({ selectPoint: values => values.extend
    ? Promise.resolve({ anchor: 4, focus: 40 })
    : new Promise(resolve => { resolveInitial = resolve; }) });
  click(f.canvas, 120, 120);
  move(f.canvas, 130, 130);
  up(f.canvas, 140, 140);
  f.captured.delete(7); // Simulate the browser's implicit capture release on pointerup.
  f.canvas.emit("lostpointercapture", { pointerId: 7 });
  assert.equal(f.selected.length, 1, "only one engine request is active while the down is pending");
  resolveInitial({ anchor: 4, focus: 4 });
  await settle();
  assert.equal(f.selected.length, 2, "pending movement is coalesced and the pointerup point wins");
  assert.deepEqual([f.selected[1].x, f.selected[1].y], [50, 60]);
  assert.equal(f.selected[1].anchor, 4);
  f.controller.dispose();
}

{
  let enabled = true;
  let resolveInitial;
  const f = fixture({ enabled: () => enabled,
    selectPoint: () => new Promise(resolve => { resolveInitial = resolve; }) });
  click(f.canvas, 120, 120);
  move(f.canvas, 140, 140);
  enabled = false;
  resolveInitial({ anchor: 4, focus: 4 });
  await settle();
  assert.equal(f.selected.length, 1, "disabled state discards queued movement after the pending command resolves");
  assert.equal(f.captured.size, 0, "disabled state releases capture");
  assert.equal(f.focuses.length, 0, "disabled state cancels instead of completing the drag");
  f.controller.dispose();
}

{
  let shouldThrow = false;
  let resolveInitial;
  const failure = new Error("enabled callback failed");
  const f = fixture({ enabled: () => {
    if (shouldThrow) throw failure;
    return true;
  }, selectPoint: () => new Promise(resolve => { resolveInitial = resolve; }) });
  click(f.canvas, 120, 120);
  move(f.canvas, 140, 140);
  shouldThrow = true;
  resolveInitial({ anchor: 4, focus: 4 });
  await settle();
  assert.equal(f.selected.length, 1, "enabled callback failure cancels queued movement");
  assert.equal(f.captured.size, 0, "enabled callback failure releases capture");
  assert.equal(f.errors.length, 1, "enabled callback failure is reported once");
  assert.equal(f.errors[0], failure);
  f.controller.dispose();
}

{
  let resolve;
  const f = fixture({ selectPoint: () => new Promise(done => { resolve = done; }) });
  click(f.canvas, 120, 120);
  f.controller.cancel();
  resolve({ anchor: 1, focus: 1 });
  await settle();
  assert.equal(f.selected.length, 1, "cancel invalidates the pending drag");
  assert.equal(f.focuses.length, 0, "cancel does not restore focus through a stale completion");
  assert.equal(f.captured.size, 0);
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
    const f = fixture();
    f.scrollport.scrollHeight = 300;
    click(f.canvas, 150, 195);
    let spins = 0;
    while (frames.size && spins++ < 30) {
      const [frame, callback] = frames.entries().next().value;
      frames.delete(frame);
      callback();
    }
    assert.ok(f.scrollport.scrollTop <= 200 && f.scrollport.scrollTop > 0, "autoscroll is clamped to scroll bounds");
    assert.equal(frames.size, 0, "RAF stops at the scroll boundary");
    f.controller.cancel();
    assert.equal(frames.size, 0, "cancel leaves no queued frame");
    f.controller.dispose();
  } finally {
    if (originalRaf === undefined) delete globalThis.requestAnimationFrame;
    else globalThis.requestAnimationFrame = originalRaf;
    if (originalCaf === undefined) delete globalThis.cancelAnimationFrame;
    else globalThis.cancelAnimationFrame = originalCaf;
  }
}

console.log("Mouse selection: capture, Shift anchor, word selection, async pointerup, cancel/disable/error handling, page/zoom mapping and bounded autoscroll passed.");
