// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { decodeEditorResponse, encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";

const bytes = await readFile(process.argv[2] ?? "data/engine/atelier_core_wasm.wasm");
const module = await WebAssembly.compile(bytes);
const { memory, alloc, dispatch, free } = new WebAssembly.Instance(module).exports;
let requestId = 1n;
function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  assert.notEqual(ptr, 0, "input allocation");
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  assert.notEqual(descriptorPtr, 0, "response descriptor");
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const bodyPtr = descriptor.getUint32(0, true);
  const bodyLen = descriptor.getUint32(4, true);
  try {
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    return { status: new DataView(body.buffer).getUint32(0, true), frame: body.slice(4) };
  } finally {
    assert.equal(free(bodyPtr, bodyLen), 0);
    assert.equal(free(descriptorPtr, 8), 0);
  }
}
function frame(opcode, values = {}) {
  return encodeMessage({ type: MessageType.Editor, requestId: requestId++, payload: encodeEditorCommand(opcode, values) });
}
function command(opcode, values) {
  const result = invoke(frame(opcode, values));
  assert.equal(result.status, 0);
  const message = decodeMessage(result.frame, "event");
  assert.equal(message.requestId, requestId - 1n);
  return decodeEditorResponse(message.type, message.payload);
}
function expectFailure(payload, code, name) {
  const result = invoke(encodeMessage({ type: MessageType.Editor, requestId: requestId++, payload }));
  assert.equal(result.status, 0, name);
  const message = decodeMessage(result.frame, "event");
  const failure = decodeEditorResponse(message.type, message.payload);
  assert.equal(failure.kind, "failed", name);
  assert.equal(failure.code, code, name);
}

const probeCommand = encodeEditorCommand(EditorOpcode.ProbePoint, { page: 2, x: 12.5, y: 34.25 });
assert.equal(probeCommand.length, 28, "ProbePoint uses a 20-byte body");
assert.equal(new DataView(probeCommand.buffer).getUint16(6, true), 15);
assert.equal(new DataView(probeCommand.buffer).getUint32(8, true), 2);
assert.equal(new DataView(probeCommand.buffer).getFloat64(12, true), 12.5);
assert.equal(new DataView(probeCommand.buffer).getFloat64(20, true), 34.25);

const moveCommand = encodeEditorCommand(EditorOpcode.DropText, {
  revision: 7n, nowMs: 8n, sourceAnchor: 9, sourceFocus: 10, target: 11, kind: "move", text: "",
});
assert.equal(moveCommand.length, 40, "DropText uses a 32-byte prefix");
assert.equal(moveCommand[36], 0, "move kind");
assert.deepEqual([...moveCommand.subarray(37)], [0, 0, 0], "reserved bytes are zero");
const externalCommand = encodeEditorCommand(EditorOpcode.DropText, {
  revision: 7n, nowMs: 8n, sourceAnchor: 0, sourceFocus: 0, target: 11, kind: "external", text: "é",
});
assert.equal(externalCommand.length, 42, "external text is UTF-8 encoded after the prefix");
assert.equal(externalCommand[36], 2, "external kind");
assert.deepEqual([...externalCommand.subarray(40)], [0xc3, 0xa9]);

const replaceCommand = encodeEditorCommand(EditorOpcode.ReplaceRange, {
  revision: 12n, nowMs: 13n, start: 14, end: 15, text: "\u00a0:",
});
assert.equal(replaceCommand.length, 35, "ReplaceRange uses a 24-byte prefix plus UTF-8 text");
assert.equal(new DataView(replaceCommand.buffer).getBigUint64(8, true), 12n);
assert.equal(new DataView(replaceCommand.buffer).getUint32(24, true), 14);
assert.equal(new DataView(replaceCommand.buffer).getUint32(28, true), 15);
assert.deepEqual([...replaceCommand.subarray(32)], [0xc2, 0xa0, 0x3a]);

const original = new TextEncoder().encode("\ufeffseed\r\né");
const loaded = command(EditorOpcode.Load, { bytes: original });
const baseline = command(EditorOpcode.Export);
assert.deepEqual(baseline.bytes, original);
assert.equal(loaded.dirty, false);

// Keep the public ABI allocation registry within 4 KiB of its 128 MiB ceiling.
// The input fits, but a successful render response cannot. A rejected response
// must release its input and leave the candidate document unpublished.
const held = [];
try {
  for (const size of [64 * 1024 * 1024, 64 * 1024 * 1024 - 4096]) {
    const ptr = alloc(size) >>> 0;
    assert.notEqual(ptr, 0, "pressure allocation");
    held.push([ptr, size]);
  }
  const rejected = invoke(frame(EditorOpcode.Load, { bytes: new TextEncoder().encode("x".repeat(1024)) }));
  assert.equal(rejected.status, 2, "ABI limit status");
  assert.equal(rejected.frame.length, 0);
} finally {
  for (const [ptr, size] of held) assert.equal(free(ptr, size), 0);
}
const after = command(EditorOpcode.Export);
assert.equal(after.revision, baseline.revision);
assert.deepEqual(after.bytes, baseline.bytes);
const state = command(EditorOpcode.State);
assert.equal(state.dirty, false);
assert.equal(state.canUndo, false);
assert.equal(state.anchor, loaded.anchor);
assert.equal(state.focus, loaded.focus);

const malformed = frame(EditorOpcode.Load, { bytes: new TextEncoder().encode("discard") });
malformed[8] ^= 1; // corrupt the ATLM payload length, retaining the Editor tag
const result = invoke(malformed);
assert.equal(result.status, 0);
const failure = decodeMessage(result.frame, "event");
assert.equal(decodeEditorResponse(failure.type, failure.payload).code, "EDITOR_INVALID_ENVELOPE");
assert.deepEqual(command(EditorOpcode.Export).bytes, original);

// Mutate the wire after encoding: JS validation must not mask the WASM guards.
const selectPoint = () => encodeEditorCommand(EditorOpcode.SelectPoint, {
  page: 0, x: 72, y: 72, unit: "character", extend: true, anchor: 0, anchorEnd: 0,
});
const invalidPoints = [
  ["unknown page", p => new DataView(p.buffer).setUint32(8, 99, true), "EDITOR_INVALID_PAGE"],
  ["NaN coordinate", p => new DataView(p.buffer).setFloat64(12, NaN, true), "EDITOR_INVALID_COMMAND"],
  ["infinite coordinate", p => new DataView(p.buffer).setFloat64(20, Infinity, true), "EDITOR_INVALID_COMMAND"],
  ["unknown unit", p => { p[28] = 3; }, "EDITOR_INVALID_COMMAND"],
  ["invalid boolean", p => { p[29] = 2; }, "EDITOR_INVALID_COMMAND"],
  ["reserved field", p => { p[30] = 1; }, "EDITOR_INVALID_COMMAND"],
  ["unequal character anchors", p => new DataView(p.buffer).setUint32(36, 4, true), "EDITOR_INVALID_COMMAND"],
  ["mid UTF-8 anchor", p => { const v = new DataView(p.buffer); v.setUint32(32, 7, true); v.setUint32(36, 7, true); }, "EDITOR_INVALID_SELECTION"],
  ["past-end anchor", p => { const v = new DataView(p.buffer); v.setUint32(32, 999, true); v.setUint32(36, 999, true); }, "EDITOR_INVALID_SELECTION"],
  ["truncated body", p => p.slice(0, -1), "EDITOR_INVALID_COMMAND"],
  ["trailing byte", p => { const out = new Uint8Array(p.length + 1); out.set(p); return out; }, "EDITOR_INVALID_COMMAND"],
];
const stableState = command(EditorOpcode.State);
const stableExport = command(EditorOpcode.Export);
for (const [name, mutate, expected] of invalidPoints) {
  const payload = selectPoint();
  const replacement = mutate(payload);
  const rejected = invoke(encodeMessage({ type: MessageType.Editor, requestId: requestId++,
    payload: replacement instanceof Uint8Array ? replacement : payload }));
  assert.equal(rejected.status, 0, name);
  const message = decodeMessage(rejected.frame, "event");
  const failure = decodeEditorResponse(message.type, message.payload);
  assert.equal(failure.code, expected, name);
  assert.deepEqual(command(EditorOpcode.State), stableState, `${name}: state unchanged`);
  assert.deepEqual(command(EditorOpcode.Export), stableExport, `${name}: bytes/revision unchanged`);
}

{
  command(EditorOpcode.Load, { bytes: new TextEncoder().encode("abc") });
  command(EditorOpcode.SetSelection, { anchor: 3, focus: 2 });
  command(EditorOpcode.InsertText, { text: "!", nowMs: 1n });
  command(EditorOpcode.SetSelection, { anchor: 2, focus: 1 });
  const beforeProbe = command(EditorOpcode.State);
  const probe = command(EditorOpcode.ProbePoint, { page: 0, x: 54, y: 65 });
  assert.equal(probe.kind, "point");
  assert.equal(probe.revision, beforeProbe.revision);
  const afterProbe = command(EditorOpcode.State);
  assert.deepEqual(afterProbe, beforeProbe, "ProbePoint preserves revision, selection, and history flags");
  const undone = command(EditorOpcode.Undo);
  assert.equal(undone.text, "abc", "Undo after ProbePoint still reaches the prior edit");
  assert.equal(undone.anchor, 3);
  assert.equal(undone.focus, 2);
}

{
  const originalText = "\ufeffAé\r\nBC";
  command(EditorOpcode.Load, { bytes: new TextEncoder().encode(originalText) });
  command(EditorOpcode.SetSelection, { anchor: 6, focus: 5 });
  const originalState = command(EditorOpcode.State);
  const originalExport = command(EditorOpcode.Export);
  const moved = command(EditorOpcode.DropText, {
    revision: originalState.revision, nowMs: 2n, sourceAnchor: 6, sourceFocus: 5,
    target: 0, kind: "move", text: "",
  });
  assert.equal(moved.text, "BAé\r\nC");
  const exportAfterMove = command(EditorOpcode.Export);
  assert.deepEqual(exportAfterMove.bytes, new TextEncoder().encode("\ufeffBAé\r\nC"));
  const undoMove = command(EditorOpcode.Undo);
  assert.equal(undoMove.text, "Aé\r\nBC");
  assert.equal(undoMove.anchor, 6);
  assert.equal(undoMove.focus, 5);
  const redoMove = command(EditorOpcode.Redo);
  assert.equal(redoMove.text, "BAé\r\nC");
  assert.notDeepEqual(command(EditorOpcode.Export).bytes, originalExport.bytes);
}

{
  command(EditorOpcode.Load, { bytes: new TextEncoder().encode("mot") });
  command(EditorOpcode.SetSelection, { anchor: 3, focus: 3 });
  const raw = command(EditorOpcode.InsertText, { text: ":", nowMs: 10n });
  assert.equal(raw.text, "mot:");
  const spaced = command(EditorOpcode.ReplaceRange, {
    revision: raw.revision, nowMs: 11n, start: 3, end: 4, text: "\u00a0:",
  });
  assert.equal(spaced.text, "mot\u00a0:");
  assert.equal(command(EditorOpcode.Undo).text, "mot:", "one Undo restores the raw punctuation");
  assert.equal(command(EditorOpcode.Redo).text, "mot\u00a0:", "one Redo reapplies spacing");
}

{
  command(EditorOpcode.Load, { bytes: new TextEncoder().encode("abc") });
  command(EditorOpcode.SetSelection, { anchor: 1, focus: 2 });
  const stableState = command(EditorOpcode.State);
  const stableExport = command(EditorOpcode.Export);
  const assertStillStable = name => {
    assert.deepEqual(command(EditorOpcode.State), stableState, `${name}: revision/selection/history unchanged`);
    assert.deepEqual(command(EditorOpcode.Export), stableExport, `${name}: exported bytes unchanged`);
  };

  expectFailure(encodeEditorCommand(EditorOpcode.ReplaceRange, {
    revision: stableState.revision + 1n, nowMs: 1n, start: 0, end: 1, text: "x",
  }), "EDITOR_STALE_EDIT", "stale replacement revision");
  assertStillStable("stale replacement revision");

  for (const [name, byte, value] of [["invalid drop kind", 36, 3], ["nonzero drop reserved byte", 37, 1]]) {
    const payload = encodeEditorCommand(EditorOpcode.DropText, {
      revision: stableState.revision, nowMs: 1n, sourceAnchor: 1, sourceFocus: 2,
      target: 0, kind: "move", text: "",
    });
    payload[byte] = value;
    expectFailure(payload, "EDITOR_INVALID_COMMAND", name);
    assertStillStable(name);
  }

  const invalidUtf8 = encodeEditorCommand(EditorOpcode.ReplaceRange, {
    revision: stableState.revision, nowMs: 1n, start: 0, end: 1, text: "x",
  });
  invalidUtf8[32] = 0xff;
  expectFailure(invalidUtf8, "EDITOR_INVALID_UTF8", "invalid replacement UTF-8");
  assertStillStable("invalid replacement UTF-8");

  expectFailure(encodeEditorCommand(EditorOpcode.ReplaceRange, {
    revision: stableState.revision, nowMs: 1n, start: 99, end: 100, text: "x",
  }), "EDITOR_INVALID_SELECTION", "invalid replacement offset");
  assertStillStable("invalid replacement offset");

  const truncated = encodeEditorCommand(EditorOpcode.ReplaceRange, {
    revision: stableState.revision, nowMs: 1n, start: 0, end: 0, text: "",
  }).slice(0, -1);
  expectFailure(truncated, "EDITOR_INVALID_COMMAND", "truncated replacement payload");
  assertStillStable("truncated replacement payload");
}

{
  const malformedAtpt = new Uint8Array(48);
  malformedAtpt.set(new TextEncoder().encode("ATPT"));
  const view = new DataView(malformedAtpt.buffer);
  view.setUint16(4, 1, true);
  view.setFloat64(24, -1, true);
  view.setFloat64(32, 0, true);
  view.setFloat64(40, 14, true);
  assert.throws(
    () => decodeEditorResponse(0x8020, malformedAtpt),
    error => error.code === "INVALID_RESPONSE",
    "malformed ATPT geometry is rejected",
  );
}

const beforeDocx = command(EditorOpcode.State);
const docx = new Uint8Array(await readFile("tests/docx-reference/basic.docx"));
const preview = command(EditorOpcode.PreviewDocx, { bytes: docx });
assert.equal(preview.kind, "preview");
assert(preview.text.startsWith("Référence de titre"));
assert.equal(preview.pages.length, 1);
assert.deepEqual(command(EditorOpcode.State), beforeDocx, "real WASM preview leaves every TXT state field unchanged");
expectFailure(encodeEditorCommand(EditorOpcode.PreviewDocx, { bytes: new Uint8Array([1,2,3]) }), "EDITOR_PREVIEW_FAILED", "malformed DOCX rejected");
assert.deepEqual(command(EditorOpcode.State), beforeDocx, "failed preview preserves complete TXT state");
console.log(`Editor real WASM ABI: rollback, cleanup, UTF-8/BOM/CRLF, ProbePoint/DropText/ReplaceRange, DOCX preview and ${invalidPoints.length} atomic SelectPoint rejections passed.`);
