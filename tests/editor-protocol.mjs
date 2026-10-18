// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { decodeEditorResponse, EditorOpcode, encodeEditorCommand, EditorProtocolError } from "../web/editor-protocol.mjs";
import { createNativeHost } from "../web/native-host.mjs";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";

function expectCode(callback, code) {
  assert.throws(callback, error => error instanceof EditorProtocolError && error.code === code);
}
function a4Page() {
  const bytes = new Uint8Array(44);
  bytes.set([65, 84, 68, 76]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true);
  view.setUint16(6, 0, true);
  view.setFloat64(8, 0, true); view.setFloat64(16, 0, true);
  view.setFloat64(24, 595.276, true); view.setFloat64(32, 841.89, true);
  view.setUint32(40, 0, true);
  return bytes;
}
function glyphAtlas() {
  const bytes = new Uint8Array(44 + 22 + 3 * 17 + 1);
  bytes.set(a4Page());
  const view = new DataView(bytes.buffer);
  view.setUint32(40, 1, true);
  let at = 44;
  bytes[at++] = 1; // FillPath
  bytes[at++] = 0; // NonZero
  view.setFloat32(at + 12, 1, true); at += 16; // Opaque black
  view.setUint32(at, 4, true); at += 4;
  bytes[at++] = 0; // Outer contour
  at += 16;
  bytes[at++] = 1; at += 16;
  bytes[at++] = 1; at += 16;
  bytes[at++] = 4;
  return bytes;
}
function glyphRunPage(glyphIds = [0], fontId = 0n) {
  const bytes = new Uint8Array(73 + glyphIds.length * 36);
  bytes.set(a4Page());
  const view = new DataView(bytes.buffer);
  view.setUint32(40, 1, true);
  bytes[44] = 3; // DrawGlyphRun
  view.setBigUint64(45, fontId, true);
  view.setFloat32(65, 1, true);
  view.setUint32(69, glyphIds.length, true);
  glyphIds.forEach((glyphId, index) => view.setUint32(73 + index * 36, glyphId, true));
  return bytes;
}
function previewV2Payload({ atlas = glyphAtlas(), page = glyphRunPage(), text = "x" } = {}) {
  const textBytes = new TextEncoder().encode(text);
  const bytes = new Uint8Array(20 + textBytes.length + 4 + atlas.length + 4 + page.length);
  bytes.set([65, 84, 82, 80]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 2, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, textBytes.length, true);
  view.setUint32(16, 0, true);
  let at = 20;
  bytes.set(textBytes, at); at += textBytes.length;
  view.setUint32(at, atlas.length, true); at += 4;
  bytes.set(atlas, at); at += atlas.length;
  view.setUint32(at, page.length, true); at += 4;
  bytes.set(page, at);
  return bytes;
}
function previewV1Payload({ page = glyphRunPage(), text = "x" } = {}) {
  const textBytes = new TextEncoder().encode(text);
  const bytes = new Uint8Array(20 + textBytes.length + 4 + page.length);
  bytes.set([65, 84, 82, 80]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, textBytes.length, true);
  view.setUint32(16, 0, true);
  let at = 20;
  bytes.set(textBytes, at); at += textBytes.length;
  view.setUint32(at, page.length, true); at += 4;
  bytes.set(page, at);
  return bytes;
}
function statePayload(textBytes = new TextEncoder().encode("hello"), { anchor = 0, focus = 0 } = {}) {
  const page = a4Page();
  const result = new Uint8Array(64 + textBytes.length + 4 + page.length + 36);
  result.set([65, 84, 69, 83]);
  const view = new DataView(result.buffer);
  view.setUint16(4, 1, true); view.setUint16(6, 0x0d, true);
  view.setBigUint64(8, 42n, true);
  view.setUint32(16, anchor, true); view.setUint32(20, focus, true);
  view.setUint32(24, textBytes.length, true); view.setUint32(28, 1, true);
  view.setUint32(32, 0, true);
  view.setFloat64(36, 10, true); view.setFloat64(44, 20, true); view.setFloat64(52, 14, true);
  view.setUint32(60, 1, true);
  let offset = 64;
  result.set(textBytes, offset); offset += textBytes.length;
  view.setUint32(offset, page.length, true); offset += 4;
  result.set(page, offset); offset += page.length;
  view.setUint32(offset, 0, true); view.setFloat64(offset + 4, 10, true);
  view.setFloat64(offset + 12, 20, true); view.setFloat64(offset + 20, 30, true);
  view.setFloat64(offset + 28, 14, true);
  return result;
}

const loaded = encodeEditorCommand(EditorOpcode.Load, { bytes: Uint8Array.of(0xef, 0xbb, 0xbf, 0x61) });
assert.equal(new TextDecoder().decode(loaded.subarray(0, 4)), "ATED");
assert.equal(new DataView(loaded.buffer).getUint16(6, true), EditorOpcode.Load);
assert.deepEqual([...loaded.subarray(8)], [0xef, 0xbb, 0xbf, 0x61]);
assert.equal(encodeEditorCommand(EditorOpcode.InsertText, { nowMs: 7n, text: "é" }).length, 18);
expectCode(() => encodeEditorCommand(EditorOpcode.InsertText, { nowMs: Number.MAX_SAFE_INTEGER + 1, text: "x" }), "INVALID_ARGUMENT");
const maxU64Command = encodeEditorCommand(EditorOpcode.InsertText, { nowMs: (1n << 64n) - 1n, text: "x" });
assert.equal(new DataView(maxU64Command.buffer).getBigUint64(8, true), (1n << 64n) - 1n, "BigInt u64 maximum encodes exactly");
assert.deepEqual([...encodeEditorCommand(EditorOpcode.Move, { direction: "pageDown", extend: true }).subarray(8)], [9, 1, 0, 0]);
assert.equal(encodeEditorCommand(EditorOpcode.HitTest, { page: 2, x: 10.5, y: 20, extend: false }).length, 29);
assert.deepEqual([...encodeEditorCommand(EditorOpcode.Move, { direction: "wordLeft", extend: true }).subarray(8)], [10, 1, 0, 0]);
assert.deepEqual([...encodeEditorCommand(EditorOpcode.Move, { direction: "paragraphDown" }).subarray(8)], [13, 0, 0, 0]);
assert.equal(encodeEditorCommand(EditorOpcode.DeleteWordForward, { nowMs: 42n }).length, 16);
const pointSelection = encodeEditorCommand(EditorOpcode.SelectPoint, { page: 1, x: 30.25, y: 70.5,
  unit: "word", extend: true, anchor: 3, anchorEnd: 8 });
const pointView = new DataView(pointSelection.buffer);
assert.equal(pointSelection.length, 40);
assert.deepEqual([pointView.getUint32(8, true), pointView.getFloat64(12, true), pointView.getFloat64(20, true)], [1, 30.25, 70.5]);
assert.deepEqual([...pointSelection.subarray(28, 32)], [1, 1, 0, 0]);
assert.deepEqual([pointView.getUint32(32, true), pointView.getUint32(36, true)], [3, 8]);
expectCode(() => encodeEditorCommand(EditorOpcode.SelectPoint, { page: 0, x: NaN, y: 0, unit: "word" }), "INVALID_ARGUMENT");
expectCode(() => encodeEditorCommand(EditorOpcode.SelectPoint, { page: 0, x: 0, y: 0, unit: "sentence" }), "INVALID_ARGUMENT");
expectCode(() => encodeEditorCommand(EditorOpcode.Move, { direction: "sideways" }), "INVALID_ARGUMENT");
expectCode(() => encodeEditorCommand(EditorOpcode.HitTest, { page: 0, x: Infinity, y: 0 }), "INVALID_ARGUMENT");

const doubleBom = Uint8Array.of(0xef, 0xbb, 0xbf, 0x78);
const decoded = decodeEditorResponse(MessageType.EditorResult, statePayload(doubleBom, { anchor: 3, focus: 4 }));
assert.equal(decoded.kind, "state");
assert.equal(decoded.text, "\uFEFFx", "second BOM remains document content");
assert.equal(decoded.revision, 42n);
assert.deepEqual([decoded.anchor, decoded.focus], [3, 4]);
assert.deepEqual([decoded.canUndo, decoded.canRedo, decoded.dirty, decoded.bom], [true, false, true, true]);
assert.equal(decoded.pages.length, 1);
assert.equal(decoded.selectionRects[0].width, 30);

const exportedBytes = Uint8Array.of(0xef, 0xbb, 0xbf, 0x78, 0x0d, 0x0a);
const exportPayload = new Uint8Array(16 + exportedBytes.length);
exportPayload.set([65, 84, 69, 88]);
const exportView = new DataView(exportPayload.buffer);
exportView.setUint16(4, 1, true); exportView.setUint16(6, 0, true); exportView.setBigUint64(8, 9n, true);
exportPayload.set(exportedBytes, 16);
const exported = decodeEditorResponse(MessageType.EditorResult, exportPayload);
assert.equal(exported.kind, "export");
assert.deepEqual(exported.bytes, exportedBytes, "ATEX preserves exact file bytes");
assert.equal(exported.revision, 9n);
expectCode(() => decodeEditorResponse(MessageType.Failed, new Uint8Array(65)), "INVALID_RESPONSE");
expectCode(() => decodeEditorResponse(MessageType.Failed, new TextEncoder().encode("stack trace")), "INVALID_RESPONSE");
expectCode(() => decodeEditorResponse(MessageType.EditorResult, statePayload(new TextEncoder().encode("é"), { anchor: 1, focus: 1 })), "INVALID_RESPONSE");
const trailing = Uint8Array.from(statePayload());
const withTrailing = new Uint8Array(trailing.length + 1); withTrailing.set(trailing); withTrailing.at(-1);
expectCode(() => decodeEditorResponse(MessageType.EditorResult, withTrailing), "TRAILING_BYTES");

const previewV2 = previewV2Payload();
const decodedPreviewV2 = decodeEditorResponse(MessageType.EditorResult, previewV2);
assert.equal(decodedPreviewV2.kind, "preview");
assert.equal(decodedPreviewV2.pages.length, 1);
assert.ok(decodedPreviewV2.glyphAtlas instanceof Uint8Array);
const decodedPreviewV1 = decodeEditorResponse(MessageType.EditorResult, previewV1Payload());
assert.equal(decodedPreviewV1.kind, "preview", "legacy v1 previews remain accepted without an atlas");
assert.equal("glyphAtlas" in decodedPreviewV1, false);
const outlineFreePreview = decodeEditorResponse(MessageType.EditorResult,
  previewV2Payload({ atlas: a4Page(), page: a4Page(), text: "   " }));
assert.equal(outlineFreePreview.glyphAtlas.length, 44, "v2 accepts an empty atlas for text with no drawable outlines");
const atlasLengthOffset = 21; // 20-byte header, then the one-byte text.
let malformedPreview = previewV2.slice();
new DataView(malformedPreview.buffer).setUint32(atlasLengthOffset, glyphAtlas().length + 1, true);
expectCode(() => decodeEditorResponse(MessageType.EditorResult, malformedPreview), "INVALID_RESPONSE");
malformedPreview = previewV2.slice();
new DataView(malformedPreview.buffer).setUint32(atlasLengthOffset, 0, true);
expectCode(() => decodeEditorResponse(MessageType.EditorResult, malformedPreview), "INVALID_RESPONSE");
expectCode(() => decodeEditorResponse(MessageType.EditorResult,
  previewV2Payload({ page: glyphRunPage([1]) })), "INVALID_RESPONSE");
expectCode(() => decodeEditorResponse(MessageType.EditorResult,
  previewV2Payload({ page: glyphRunPage([0], 1n) })), "INVALID_RESPONSE");
const invalidAtlas = glyphAtlas(); invalidAtlas[44] = 3;
expectCode(() => decodeEditorResponse(MessageType.EditorResult,
  previewV2Payload({ atlas: invalidAtlas, page: a4Page() })), "INVALID_RESPONSE");
malformedPreview = previewV2.slice();
const pageLengthOffset = atlasLengthOffset + 4 + glyphAtlas().length;
new DataView(malformedPreview.buffer).setUint32(pageLengthOffset, 26 * 1024 * 1024, true);
expectCode(() => decodeEditorResponse(MessageType.EditorResult, malformedPreview), "INVALID_RESPONSE");

const editorFrame = encodeMessage({ type: MessageType.Editor, requestId: 10n, payload: encodeEditorCommand(EditorOpcode.State) });
assert.equal(decodeMessage(editorFrame, "command").type, MessageType.Editor);
const eventFrame = encodeMessage({ type: MessageType.EditorResult, requestId: 10n, payload: statePayload() });
assert.equal(decodeMessage(eventFrame, "event").requestId, 10n);

class FakeWebView {
  listeners = new Set();
  posted = [];
  addEventListener(_type, listener) { this.listeners.add(listener); }
  removeEventListener(_type, listener) { this.listeners.delete(listener); }
  postMessage(value) { this.posted.push(value); }
  reply(value) { for (const listener of this.listeners) listener({ data: value }); }
}
const fake = new FakeWebView();
const host = createNativeHost({ webview: fake, timeoutMs: 1000 });
const opening = host.open();
const openRequest = fake.posted.at(-1).split("|");
const openId = openRequest[2];
fake.reply(`us120-file|1|${openId}|ok|${Buffer.from(exportedBytes).toString("base64")}`);
const opened = await opening;
assert.equal(opened.kind, "ok");
assert.deepEqual(opened.bytes, exportedBytes);

const savedBytes = Uint8Array.of(1, 2, 3);
const saving = host.save(savedBytes);
const saveRequest = fake.posted.at(-1).split("|");
assert.equal(Buffer.from(saveRequest[4], "base64").toString("hex"), "010203");
fake.reply(`us120-file|1|${saveRequest[2]}|cancelled|`);
assert.deepEqual(await saving, { kind: "cancelled" });

const dirty = host.notifyDirty(true);
const dirtyRequest = fake.posted.at(-1).split("|");
assert.equal(dirtyRequest[4], "1");
assert.deepEqual(await dirty, { kind: "ok" });
await Promise.all(Array.from({ length: 64 }, (_, index) => host.notifyDirty(index % 2 === 0)));
const afterDirtyStorm = host.open();
const stormOpen = fake.posted.at(-1).split("|");
fake.reply(`us120-file|1|${stormOpen[2]}|cancelled|`);
assert.deepEqual(await afterDirtyStorm, { kind: "cancelled" }, "coalesced dirty notifications do not occupy file request slots");

// US-115 : réclamation des documents d'ouverture et notification native.
let notices = 0;
const stopNotices = host.onLaunch(() => { notices += 1; });
fake.reply("us115-launch|1");
fake.reply("us115-launch|2");
fake.reply("us120-file|1|1|launch|");
assert.equal(notices, 1, "only the exact launch notice reaches subscribers");
const emptyClaim = host.takeLaunch();
const emptyRequest = fake.posted.at(-1).split("|");
assert.deepEqual(emptyRequest.slice(0, 2).concat(emptyRequest[3], emptyRequest[4]), ["us120-file", "1", "launch", ""]);
fake.reply(`us120-file|1|${emptyRequest[2]}|ok|`);
assert.deepEqual(await emptyClaim, { kind: "ok" });
const claim = host.takeLaunch();
const claimId = fake.posted.at(-1).split("|")[2];
fake.reply(`us120-file|1|${claimId}|ok|txt;3;${Buffer.from("héllo").toString("base64")}`);
const claimed = await claim;
assert.equal(claimed.kind, "ok");
assert.equal(claimed.document.type, "txt");
assert.equal(claimed.document.skipped, 3);
assert.equal(Buffer.from(claimed.document.bytes).toString("utf8"), "héllo");
for (const bad of ["txt;3", "pdf;0;AA==", "txt;100;AA==", "txt;x;AA==", "txt;0;A", "txt;0;AA==;x", "txt;0;Zh=="]) {
  const pendingClaim = host.takeLaunch();
  const badId = fake.posted.at(-1).split("|")[2];
  fake.reply(`us120-file|1|${badId}|ok|${bad}`);
  assert.deepEqual(await pendingClaim, { kind: "error", key: "HOST_INVALID_RESPONSE" }, bad);
}
const tooBig = host.takeLaunch();
const tooBigId = fake.posted.at(-1).split("|")[2];
fake.reply(`us120-file|1|${tooBigId}|ok|txt;0;${Buffer.alloc(131076).toString("base64")}`);
assert.deepEqual(await tooBig, { kind: "error", key: "HOST_FILE_TOO_LARGE" });
const refused = host.takeLaunch();
const refusedId = fake.posted.at(-1).split("|")[2];
fake.reply(`us120-file|1|${refusedId}|error|files.error.unsupported_type`);
assert.deepEqual(await refused, { kind: "error", key: "files.error.unsupported_type" });
stopNotices();
fake.reply("us115-launch|1");
assert.equal(notices, 1, "unsubscribed callbacks are not called");
host.close();
assert.equal(fake.listeners.size, 0);

console.log("editor protocol/host checks passed: strict ATED/ATES/ATEX bounds, double BOM, correlation, host open/save/cancel/dirty");
