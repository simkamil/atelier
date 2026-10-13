// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { decodeEditorResponse, EditorOpcode, encodeEditorCommand, EditorProtocolError } from "../web/editor-protocol.mjs";

function expectCode(callback, code) {
  assert.throws(callback, error => error instanceof EditorProtocolError && error.code === code);
}
function statePayload(text = "text") {
  const encoded = new TextEncoder().encode(text), page = new Uint8Array(44);
  page.set([65, 84, 68, 76]);
  const pv = new DataView(page.buffer);
  pv.setUint16(4, 1, true); pv.setFloat64(24, 595.276, true); pv.setFloat64(32, 841.89, true);
  const state = new Uint8Array(64 + encoded.length + 4 + page.length);
  state.set([65, 84, 69, 83]);
  const view = new DataView(state.buffer);
  view.setUint16(4, 1, true); view.setBigUint64(8, 3n, true);
  view.setUint32(24, encoded.length, true); view.setUint32(28, 1, true); view.setFloat64(52, 14, true);
  state.set(encoded, 64); view.setUint32(64 + encoded.length, page.length, true); state.set(page, 68 + encoded.length);
  return state;
}
function response(count, state = statePayload()) {
  const bytes = new Uint8Array(16 + state.length);
  bytes.set([65, 84, 81, 82]);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true); view.setUint32(8, count, true); view.setUint32(12, state.length, true); bytes.set(state, 16);
  return bytes;
}

const one = encodeEditorCommand(EditorOpcode.Replace, {
  revision: 12n, nowMs: 9n, query: "é", replacement: "e\u0301", start: 4, end: 6,
  options: { matchCase: true, wholeWord: true },
});
assert.equal(new DataView(one.buffer).getUint16(6, true), 21);
assert.equal(one.length, 8 + 36 + 2 + 3);
assert.deepEqual([...one.subarray(8 + 16, 8 + 20)], [3, 0, 0, 0]);
assert.deepEqual([new DataView(one.buffer).getUint32(36, true), new DataView(one.buffer).getUint32(40, true)], [4, 6]);

const all = encodeEditorCommand(EditorOpcode.ReplaceAll, { revision: 2, nowMs: 3, query: "a", replacement: "aa" });
assert.equal(new DataView(all.buffer).getUint16(6, true), 22);
assert.equal(all.length, 8 + 28 + 2 + 1);
expectCode(() => encodeEditorCommand(EditorOpcode.ReplaceAll, { revision: 0, nowMs: 0, query: "", replacement: "x" }), "INVALID_ARGUMENT");
expectCode(() => encodeEditorCommand(EditorOpcode.ReplaceAll, { revision: 0, nowMs: 0, query: "a", replacement: "x", options: { regex: true } }), "INVALID_ARGUMENT");

const decoded = decodeEditorResponse(0x8020, response(128, statePayload("changed")));
assert.equal(decoded.kind, "replacement");
assert.equal(decoded.count, 128);
assert.equal(decoded.state.kind, "state");
assert.equal(decoded.state.text, "changed");
expectCode(() => decodeEditorResponse(0x8020, response(128 * 1024 + 1)), "INVALID_RESPONSE");
expectCode(() => decodeEditorResponse(0x8020, response(1, new Uint8Array([65, 84, 69, 88]))), "INVALID_RESPONSE");
const reserved = response(1);
new DataView(reserved.buffer).setUint16(6, 1, true);
expectCode(() => decodeEditorResponse(0x8020, reserved), "INVALID_RESPONSE");
const trailing = new Uint8Array(response(1).length + 1); trailing.set(response(1));
expectCode(() => decodeEditorResponse(0x8020, trailing), "INVALID_RESPONSE");

console.log("replacement protocol checks passed: ATED replacement frames and strict ATQR nested-state validation");
