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
console.log("Editor real WASM ABI: response-budget rollback, cleanup, UTF-8/BOM/CRLF and malformed-envelope rejection passed.");
