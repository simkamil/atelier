// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { createRichPreview, RichPreviewError } from "../web/rich-preview.mjs";

function a4Page() {
  const bytes = new Uint8Array(44);
  const view = new DataView(bytes.buffer);
  bytes.set([0x41, 0x54, 0x44, 0x4c]); // ATDL
  view.setUint16(4, 1, true);
  view.setFloat64(8, 0, true);
  view.setFloat64(16, 0, true);
  view.setFloat64(24, 595.276, true);
  view.setFloat64(32, 841.89, true);
  view.setUint32(40, 0, true);
  return bytes;
}

class Element {
  constructor() {
    this.style = {};
    this.listeners = new Map();
    this.hidden = true;
    this.tabIndex = -1;
    this.clientWidth = 600;
    this.clientHeight = 700;
    this.scrollTop = 0;
    this.scrollLeft = 0;
    this.textContent = "";
    this.focusCount = 0;
  }
  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? new Set();
    listeners.add(listener);
    this.listeners.set(type, listeners);
  }
  removeEventListener(type, listener) { this.listeners.get(type)?.delete(listener); }
  dispatch(type, event) { for (const listener of this.listeners.get(type) ?? []) listener(event); }
  focus() { this.focusCount += 1; }
}

function canvasElement() {
  const canvas = new Element();
  canvas.width = 0;
  canvas.height = 0;
  canvas.getContext = () => canvas.context;
  canvas.fillRectCalls = [];
  canvas.context = {
    setTransform() {}, clearRect() {}, beginPath() {}, rect() {}, clip() {},
    save() {}, restore() {}, fillRect(...args) { canvas.fillRectCalls.push(args); },
  };
  return canvas;
}

const savedGlobalAddEventListener = globalThis.addEventListener;
const savedGlobalRemoveEventListener = globalThis.removeEventListener;
const globalListeners = new Map();
globalThis.addEventListener = (type, listener) => {
  const listeners = globalListeners.get(type) ?? new Set();
  listeners.add(listener);
  globalListeners.set(type, listeners);
};
globalThis.removeEventListener = (type, listener) => globalListeners.get(type)?.delete(listener);
function dispatchGlobal(type) { for (const listener of globalListeners.get(type) ?? []) listener(); }

const canvas = canvasElement();
canvas.parentElement = new Element();
const scrollport = new Element();
const spacer = new Element();
const mirror = new Element();
const trigger = new Element();
let closeCount = 0;
const errors = [];
const preview = createRichPreview({
  canvas, scrollport, spacer, textRepresentation: mirror,
  onClose: () => { closeCount += 1; },
  onError: error => errors.push(error),
});

assert.throws(() => preview.open(), error => error instanceof RichPreviewError && error.code === "RICH_PREVIEW_NO_DOCUMENT");
const text = "Titre <script>alert(1)</script>\nDeuxième paragraphe";
const page = a4Page();
const document = { pages: [page, page.slice()], text, noticeCount: 2 };
scrollport.clientHeight = 0; // Simulate document installation while its workspace is hidden.
assert.deepEqual(preview.setDocument(document), { pageCount: 2, noticeCount: 2 });
assert.equal(mirror.textContent, text, "accessible mirror receives literal text content");
assert.equal(scrollport.scrollTop, 0);
assert.equal(spacer.style.height, "1715.78px", "hidden workspace starts with a zero-height viewport");

scrollport.clientHeight = 700;
scrollport.clientWidth = 320;
preview.open({ returnFocusTo: trigger });
assert.equal(scrollport.hidden, false);
assert.equal(scrollport.tabIndex, 0, "preview scrollport is keyboard focusable");
assert.equal(scrollport.focusCount, 1);
assert.equal(spacer.style.height, "1015.78px", "opening recalculates against the visible height");
assert.equal(canvas.style.position, "sticky");
assert.equal(canvas.style.left, "0px");
assert.equal(canvas.style.width, "320px", "canvas spans the narrow viewport");
assert.equal(spacer.style.width, "595.276px", "content width preserves the full A4 page");
assert.equal(canvas.parentElement.style.minWidth, "595.276px", "page stack can contain the full sticky page width");
scrollport.scrollLeft = 140;
scrollport.dispatch("scroll", {});
assert.equal(canvas.fillRectCalls.at(-1)[0], -140, "horizontal scroll is applied once by the renderer");
scrollport.clientHeight = 800;
dispatchGlobal("resize");
assert.equal(spacer.style.height, "915.78px", "resize recalculates against the new viewport height");
const escape = { key: "Escape", prevented: false, preventDefault() { this.prevented = true; } };
scrollport.dispatch("keydown", escape);
assert.equal(escape.prevented, true);
assert.equal(scrollport.hidden, true);
assert.equal(trigger.focusCount, 1, "Escape restores focus to the opener");
assert.equal(closeCount, 1);

// A malformed later page must leave the last valid document visible and mirrored.
const invalidPage = page.slice();
invalidPage[0] = 0;
assert.throws(() => preview.setDocument({ pages: [page, invalidPage], text: "corrupted", noticeCount: 0 }),
  error => error instanceof RichPreviewError && error.code === "RICH_PREVIEW_INVALID_ATDL");
assert.equal(errors.length, 1);
assert.equal(mirror.textContent, text);
assert.equal(spacer.style.height, "915.78px");
assert.throws(() => preview.setDocument({
  pages: [{ page: { bounds: { width: 595.276, height: 841.89 } }, commands: [] }],
  text: "malformed decoded object", noticeCount: 0,
}), error => error instanceof RichPreviewError && error.code === "RICH_PREVIEW_INVALID_ATDL");
assert.equal(errors.length, 2);
assert.equal(mirror.textContent, text, "decoded objects cannot bypass byte limits or validation");

assert.throws(() => preview.setDocument({ pages: [page], text: "x".repeat(128 * 1024 + 1), noticeCount: 0 }),
  error => error instanceof RichPreviewError && error.code === "RICH_PREVIEW_LIMIT");
assert.throws(() => preview.setZoom(2.1), RangeError);
preview.setZoom(1.25);
assert.equal(spacer.style.height, "1336.725px");
assert.ok(Math.abs(Number.parseFloat(spacer.style.width) - 744.095) < 0.001, "zoom updates the scrollable A4 width");
assert.ok(Math.abs(Number.parseFloat(canvas.parentElement.style.minWidth) - 744.095) < 0.001,
  "zoom updates the sticky canvas containing width");

preview.open({ returnFocusTo: trigger });
preview.dispose();
assert.equal(scrollport.hidden, true);
assert.equal(trigger.focusCount, 2, "disposing an open view restores focus");
assert.equal(closeCount, 2);
assert.equal(scrollport.listeners.get("scroll")?.size ?? 0, 0);
assert.throws(() => preview.open(), error => error instanceof RichPreviewError && error.code === "RICH_PREVIEW_CLOSED");
if (savedGlobalAddEventListener === undefined) delete globalThis.addEventListener;
else globalThis.addEventListener = savedGlobalAddEventListener;
if (savedGlobalRemoveEventListener === undefined) delete globalThis.removeEventListener;
else globalThis.removeEventListener = savedGlobalRemoveEventListener;

console.log(JSON.stringify({ status: "passed", checks: [
  "A4 multi-page layout and accessible literal-text mirror",
  "keyboard focus, Escape close and focus restoration",
  "atomic rejection of malformed ATDL, decoded objects and oversized text",
  "spacer recalculation from hidden install through open and resize, including narrow horizontal scrolling",
  "zoom bounds, disposal and listener cleanup",
] }));
