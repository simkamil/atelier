// SPDX-License-Identifier: AGPL-3.0-or-later
// US-270 : logique d'entrée des images sans navigateur (collage, sélecteur, lecture bornée, cache, messages).
import assert from "node:assert/strict";
import { attachImagePaste, createImageCache, imageAltText, createImagePicker, imageMessageKey, insertImageFiles, readImageFile } from "../web/image-insert.mjs";
import { EditorOpcode, encodeEditorCommand } from "../web/editor-protocol.mjs";

const fakeFile = (size, type = "image/png") => ({ size, type, arrayBuffer: async () => new Uint8Array(size).fill(1).buffer });

// Codes d'erreur -> clés du catalogue.
assert.equal(imageMessageKey("EDITOR_IMAGE_UNSUPPORTED"), "image-error-unsupported");
assert.equal(imageMessageKey("EDITOR_IMAGE_CORRUPT"), "image-error-corrupt");
assert.equal(imageMessageKey("EDITOR_IMAGE_TOO_LARGE"), "image-error-too-large");
assert.equal(imageMessageKey("EDITOR_IMAGE_LIMIT"), "image-error-limit");
assert.equal(imageMessageKey("EDITOR_BUSY"), null);
assert.equal(imageMessageKey("toString"), null);

// Texte alternatif : nom de fichier utile, sinon le texte localisé.
assert.equal(imageAltText({ name: "plan du site.png" }, "Image 3"), "plan du site");
assert.equal(imageAltText({ name: "photo.2024.jpeg" }, "Image 3"), "photo.2024");
for (const name of ["image.png", "Image 12.PNG", "blob", "", ".png"]) assert.equal(imageAltText({ name }, "Image 3"), "Image 3", name);
assert.equal(imageAltText(undefined, "Image 1"), "Image 1");
const alted = encodeEditorCommand(EditorOpcode.InsertImage, { bytes: Uint8Array.of(9), alt: "é".repeat(400), nowMs: 0 });
assert.equal(new DataView(alted.buffer).getUint16(16, true), 512, "alternative text is cut to 512 bytes on a character boundary");

// Lecture bornée.
assert.equal((await readImageFile(fakeFile(10))).byteLength, 10);
await assert.rejects(readImageFile(fakeFile(8 * 1024 * 1024 + 1)), { code: "EDITOR_IMAGE_TOO_LARGE" });
await assert.rejects(readImageFile(fakeFile(0)), { code: "EDITOR_IMAGE_CORRUPT" });
await assert.rejects(readImageFile({ size: 5, arrayBuffer: async () => { throw new Error("x"); } }), { code: "EDITOR_IMAGE_READ" });
await assert.rejects(readImageFile(null), { code: "EDITOR_IMAGE_READ" });

// Insertion en série : une erreur n'arrête pas les suivants.
const sent = []; const errors = [];
const result = await insertImageFiles([fakeFile(3), fakeFile(9 * 1024 * 1024), fakeFile(4)], {
  insertBytes: async bytes => { sent.push(bytes.byteLength); }, onError: e => errors.push(e.code) });
assert.deepEqual(result, { inserted: 2, failed: 1 });
assert.deepEqual(sent, [3, 4]);
assert.deepEqual(errors, ["EDITOR_IMAGE_TOO_LARGE"]);

// Protocole : bornes de la commande.
const payload = encodeEditorCommand(EditorOpcode.InsertImage, { bytes: Uint8Array.of(1, 2, 3), nowMs: 5n });
assert.equal(payload.byteLength, 8 + 12 + 3);
assert.equal(new DataView(payload.buffer).getUint16(6, true), 28);
assert.throws(() => encodeEditorCommand(EditorOpcode.InsertImage, { bytes: new Uint8Array(0), nowMs: 0 }), { code: "INVALID_ARGUMENT" });
assert.throws(() => encodeEditorCommand(EditorOpcode.InsertImage, { bytes: new Uint8Array(8 * 1024 * 1024 + 1), nowMs: 0 }), { code: "MESSAGE_TOO_LARGE" });
assert.equal(new DataView(encodeEditorCommand(EditorOpcode.ImageBitmap, { id: 7 }).buffer).getUint16(6, true), 29);
assert.throws(() => encodeEditorCommand(26, {}), { code: "INVALID_ARGUMENT" });
assert.throws(() => encodeEditorCommand(31, {}), { code: "INVALID_ARGUMENT" });

// Collage.
function fakeDocument() {
  const listeners = new Map();
  return { listeners, addEventListener: (t, l) => listeners.set(t, l), removeEventListener: t => listeners.delete(t),
    fire(event) { listeners.get("paste")?.(event); } };
}
function pasteEvent(target, types, files) {
  return { target, clipboardData: { types, files }, defaultPrevented: false, stopped: false,
    preventDefault() { this.defaultPrevented = true; }, stopImmediatePropagation() { this.stopped = true; } };
}
{
  const doc = fakeDocument(); const target = {}; const got = [];
  const paste = attachImagePaste({ target, doc, onFiles: files => got.push(files) });
  const image = { type: "image/png" };
  let e = pasteEvent(target, ["Files"], [image]); doc.fire(e);
  assert.ok(e.defaultPrevented && e.stopped && got.length === 1);
  e = pasteEvent(target, ["text/plain", "Files"], [image]); doc.fire(e);
  assert.ok(!e.defaultPrevented && got.length === 1, "text keeps priority");
  e = pasteEvent({}, ["Files"], [image]); doc.fire(e);
  assert.ok(!e.defaultPrevented && got.length === 1, "other targets are ignored");
  e = pasteEvent(target, ["Files"], [{ type: "application/pdf" }]); doc.fire(e);
  assert.ok(!e.defaultPrevented && got.length === 1, "non-image files are left alone");
  paste.dispose(); assert.equal(doc.listeners.size, 0);
  const disabled = attachImagePaste({ target, doc, onFiles: files => got.push(files), isEnabled: () => false });
  e = pasteEvent(target, ["Files"], [image]); doc.fire(e);
  assert.ok(e.defaultPrevented && got.length === 1, "a disabled editor swallows the image paste without inserting");
  disabled.dispose();
}

// Sélecteur.
{
  const handlers = {}; let clicks = 0;
  const input = { setAttribute() {}, addEventListener: (t, h) => { handlers[t] = h; }, removeEventListener() {}, remove() {}, click() { clicks += 1; }, files: [], value: "x" };
  const doc = { createElement: () => input, body: { append() {} } };
  const picker = createImagePicker({ doc });
  assert.equal(input.type, "file"); assert.equal(input.multiple, true); assert.equal(input.hidden, true);
  const first = picker.open();
  assert.deepEqual(await picker.open(), [], "a second open while pending is a no-op");
  input.files = [fakeFile(1)]; handlers.change();
  assert.equal((await first).length, 1); assert.equal(clicks, 1); assert.equal(input.value, "");
  const cancelled = picker.open(); handlers.cancel();
  assert.deepEqual(await cancelled, []);
}

// Cache.
{
  globalThis.ImageData ??= class { constructor(data, width, height) { this.data = data; this.width = width; this.height = height; } };
  const closed = [];
  globalThis.createImageBitmap = async data => ({ width: data.width, close() { closed.push(data.width); } });
  const cache = createImageCache({ maxBytes: 100 });
  const fetched = [];
  const fetchBitmap = async id => { fetched.push(id); return { kind: "imageBitmap", id, width: 5, height: 5, rgba: new Uint8Array(100) }; };
  await cache.prepare(new Set([0]), fetchBitmap);
  await cache.prepare(new Set([0]), fetchBitmap);
  assert.deepEqual(fetched, [0]);
  assert.ok(cache.get(0n) !== null && cache.get(5) === null);
  await cache.prepare(new Set([1]), fetchBitmap);
  assert.equal(cache.get(0), null, "unreferenced bitmaps are evicted over the memory bound");
  assert.ok(cache.get(1) !== null && closed.length === 1);
  await assert.rejects(cache.prepare(new Set([2]), async () => ({ kind: "imageBitmap", id: 9 })), { code: "EDITOR_INVALID_RESPONSE" });
  cache.clear(); assert.equal(cache.size, 0);
}
console.log("Image insertion: bounded reads, error keys, protocol bounds, paste priority, picker, bitmap cache.");
