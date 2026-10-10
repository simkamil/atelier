// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { createWasmEngine } from "../web/wasm-loader.mjs";
import {
  HEADER_BYTES,
  MAX_MESSAGE_BYTES,
  MessageType,
  ProtocolError,
  decodeMessage,
  encodeMessage,
} from "../web/wasm-protocol.mjs";

const [oracleArgument, wasmArgument] = process.argv.slice(2);
assert.ok(oracleArgument, "usage: node tests/wasm-protocol.mjs PATH_TO_PROTOCOL_ORACLE [PATH_TO_WASM]");
const oracle = resolve(oracleArgument);
const wasmPath = wasmArgument && resolve(wasmArgument);
const MAX_U64 = (1n << 64n) - 1n;

function hex(bytes) {
  return Buffer.from(bytes).toString("hex");
}

function fromHex(value) {
  return Uint8Array.from(Buffer.from(value, "hex"));
}

function runOracle(...args) {
  const result = spawnSync(oracle, args, { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  if (result.error) throw result.error;
  return result;
}

function rustEncode(type, requestId, payload) {
  const result = runOracle("encode", type.toString(16), requestId.toString(), hex(payload));
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function rustDecode(bytes, direction) {
  return runOracle("decode", direction, hex(bytes));
}

function assertProtocolError(callback, code) {
  assert.throws(callback, (error) => error instanceof ProtocolError && error.code === code);
}

const golden = [
  {
    type: MessageType.Ping,
    direction: "command",
    requestId: 1n,
    payload: new Uint8Array(),
    frame: "41544c4d0100010000000000010000000000000000000000",
  },
  {
    type: MessageType.LoadText,
    direction: "command",
    requestId: 2n,
    payload: Uint8Array.of(0, 0xff),
    frame: "41544c4d010010000000000002000000000000000200000000ff",
  },
  { type: MessageType.ApplyOps, direction: "command", requestId: 3n, payload: Uint8Array.of(0, 0xff, 3), frame: "41544c4d010011000000000003000000000000000300000000ff03" },
  { type: MessageType.Render, direction: "command", requestId: 4n, payload: Uint8Array.of(0, 0xff, 4), frame: "41544c4d010012000000000004000000000000000300000000ff04" },
  { type: MessageType.Cancel, direction: "command", requestId: 5n, payload: new Uint8Array(), frame: "41544c4d0100130000000000050000000000000000000000" },
  { type: MessageType.Pong, direction: "event", requestId: 6n, payload: new Uint8Array(), frame: "41544c4d0100018000000000060000000000000000000000" },
  { type: MessageType.Completed, direction: "event", requestId: 7n, payload: Uint8Array.of(0, 0xff, 7), frame: "41544c4d010010800000000007000000000000000300000000ff07" },
  { type: MessageType.Failed, direction: "event", requestId: 8n, payload: Uint8Array.of(0, 0xff, 8), frame: "41544c4d010011800000000008000000000000000300000000ff08" },
  { type: MessageType.Progress, direction: "event", requestId: 9n, payload: Uint8Array.of(0, 0xff, 9), frame: "41544c4d010012800000000009000000000000000300000000ff09" },
  { type: MessageType.Cancelled, direction: "event", requestId: 10n, payload: new Uint8Array(), frame: "41544c4d01001380000000000a0000000000000000000000" },
];
for (const fixture of golden) {
  const encoded = encodeMessage(fixture);
  assert.equal(hex(encoded), fixture.frame, "explicit golden frame");
  assert.equal(rustEncode(fixture.type, fixture.requestId, fixture.payload), fixture.frame, "Rust golden encoding");
  assert.deepEqual(decodeMessage(fromHex(fixture.frame), fixture.direction), {
    type: fixture.type,
    requestId: fixture.requestId,
    payload: fixture.payload,
  });
  const decoded = rustDecode(fromHex(fixture.frame), fixture.direction);
  assert.equal(decoded.status, 0, decoded.stderr);
  const fixturePayloadHex = hex(fixture.payload);
  assert.equal(decoded.stdout.trim(), `${fixture.type.toString(16).padStart(4, "0")} ${fixture.requestId}${fixturePayloadHex ? ` ${fixturePayloadHex}` : ""}`);
}

const types = [
  [MessageType.Ping, "command"],
  [MessageType.LoadText, "command"],
  [MessageType.ApplyOps, "command"],
  [MessageType.Render, "command"],
  [MessageType.Cancel, "command"],
  [MessageType.Pong, "event"],
  [MessageType.Completed, "event"],
  [MessageType.Failed, "event"],
  [MessageType.Progress, "event"],
  [MessageType.Cancelled, "event"],
];
for (let index = 0; index < types.length; index += 1) {
  const [type, direction] = types[index];
  const requestId = index === 0 ? 9007199254740993n : index === 1 ? MAX_U64 : BigInt(index + 1);
  const payload = [MessageType.Ping, MessageType.Pong, MessageType.Cancel, MessageType.Cancelled].includes(type)
    ? new Uint8Array()
    : Uint8Array.of(0, 0xff, 0xc3, 0xa9, index);
  const jsFrame = encodeMessage({ type, requestId, payload });
  const rustFrame = fromHex(rustEncode(type, requestId, payload));
  assert.equal(hex(rustFrame), hex(jsFrame), `${type.toString(16)} JS->Rust encoding`);
  const rustDecoded = rustDecode(rustFrame, direction);
  assert.equal(rustDecoded.status, 0, rustDecoded.stderr);
  const payloadHex = hex(payload);
  assert.equal(rustDecoded.stdout.trim(), `${type.toString(16).padStart(4, "0")} ${requestId}${payloadHex ? ` ${payloadHex}` : ""}`);
  assert.deepEqual(decodeMessage(jsFrame, direction), { type, requestId, payload }, `${type.toString(16)} JS decode`);
  assert.deepEqual(decodeMessage(rustFrame, direction), { type, requestId, payload }, `${type.toString(16)} Rust->JS decode`);
}

// A subview must use its own byteOffset, and both encode/decode own their bytes.
const backing = Uint8Array.of(77, 0, 255, 12, 88);
const subview = backing.subarray(1, 4);
const encodedSubview = encodeMessage({ type: MessageType.LoadText, requestId: 17n, payload: subview });
subview.fill(1);
assert.deepEqual(decodeMessage(encodedSubview, "command").payload, Uint8Array.of(0, 255, 12));
const frameBacking = Uint8Array.of(90, ...encodedSubview, 91);
const decodedSubview = decodeMessage(frameBacking.subarray(1, frameBacking.length - 1), "command");
frameBacking.fill(0);
assert.deepEqual(decodedSubview.payload, Uint8Array.of(0, 255, 12));
const bufferFrame = Buffer.concat([Buffer.of(77), Buffer.from(encodedSubview), Buffer.of(88)]);
const decodedBufferSubview = decodeMessage(bufferFrame.subarray(1, bufferFrame.length - 1), "command");
assert.equal(decodedBufferSubview.payload.constructor, Uint8Array, "decoded Buffer payload is a plain Uint8Array");
bufferFrame.fill(0);
assert.deepEqual(decodedBufferSubview.payload, Uint8Array.of(0, 255, 12), "Buffer-backed subview payload is an independent plain Uint8Array copy");

const sharedMemory = new SharedArrayBuffer(encodedSubview.byteLength);
const sharedFrame = new Uint8Array(sharedMemory);
sharedFrame.set(encodedSubview);
const originalGetUint32 = DataView.prototype.getUint32;
let sharedPayloadMutated = false;
DataView.prototype.getUint32 = function controlledGetUint32(offset, littleEndian) {
  const value = originalGetUint32.call(this, offset, littleEndian);
  if (!sharedPayloadMutated && offset === 20) {
    sharedFrame[HEADER_BYTES] = 99;
    sharedPayloadMutated = true;
  }
  return value;
};
let decodedSharedFrame;
try {
  decodedSharedFrame = decodeMessage(sharedFrame, "command");
} finally {
  DataView.prototype.getUint32 = originalGetUint32;
}
assert.equal(sharedPayloadMutated, true, "controlled header read mutated the original shared payload");
assert.deepEqual(decodedSharedFrame.payload, Uint8Array.of(0, 255, 12), "payload comes from the owned frame snapshot");

// Every prefix, including partial headers, is refused by both implementations.
const prefixFrame = encodeMessage({ type: MessageType.LoadText, requestId: 3n, payload: Uint8Array.of(7, 8) });
for (let length = 0; length < prefixFrame.byteLength; length += 1) {
  assertProtocolError(() => decodeMessage(prefixFrame.subarray(0, length), "command"), "TRUNCATED");
  assert.notEqual(rustDecode(prefixFrame.subarray(0, length), "command").status, 0, `Rust rejects prefix ${length}`);
}

const invalidCases = [
  ["INVALID_MAGIC", (frame) => { frame[0] ^= 1; }],
  ["UNSUPPORTED_VERSION", (frame) => { frame[4] = 2; }],
  ["INVALID_TYPE", (frame) => { frame[6] = 2; frame[7] = 0; }],
  ["INVALID_FLAGS", (frame) => { frame[8] = 1; }],
  ["NONZERO_RESERVED", (frame) => { frame[10] = 1; }],
  ["INVALID_REQUEST_ID", (frame) => { frame.fill(0, 12, 20); }],
  ["TRAILING_BYTES", (frame) => { frame.push(0); }],
];
for (const [code, corrupt] of invalidCases) {
  const frame = [...encodeMessage({ type: MessageType.LoadText, requestId: 4n, payload: new Uint8Array() })];
  corrupt(frame);
  assertProtocolError(() => decodeMessage(Uint8Array.from(frame), "command"), code);
  assert.notEqual(rustDecode(Uint8Array.from(frame), "command").status, 0, `Rust rejects ${code}`);
}
const wrongDirection = encodeMessage({ type: MessageType.Ping, requestId: 2n, payload: new Uint8Array() });
assertProtocolError(() => decodeMessage(wrongDirection, "event"), "WRONG_DIRECTION");
assert.notEqual(rustDecode(wrongDirection, "event").status, 0);
const controlPayload = encodeMessage({ type: MessageType.LoadText, requestId: 4n, payload: Uint8Array.of(0) });
controlPayload[6] = MessageType.Ping & 0xff;
controlPayload[7] = MessageType.Ping >> 8;
assertProtocolError(() => decodeMessage(controlPayload, "command"), "PAYLOAD_NOT_EMPTY");
assert.notEqual(rustDecode(controlPayload, "command").status, 0);

const wrongDeclaredLength = encodeMessage({ type: MessageType.LoadText, requestId: 1n, payload: Uint8Array.of(5) });
wrongDeclaredLength[20] = 2;
assertProtocolError(() => decodeMessage(wrongDeclaredLength, "command"), "TRUNCATED");
assert.notEqual(rustDecode(wrongDeclaredLength, "command").status, 0);

assertProtocolError(() => encodeMessage({ type: MessageType.Ping, requestId: 1, payload: new Uint8Array() }), "INVALID_REQUEST_ID");
for (const requestId of [0n, MAX_U64 + 1n]) {
  assertProtocolError(() => encodeMessage({ type: MessageType.Ping, requestId, payload: new Uint8Array() }), "INVALID_REQUEST_ID");
}
assertProtocolError(() => encodeMessage({ type: 99, requestId: 1n, payload: new Uint8Array() }), "INVALID_TYPE");
assertProtocolError(() => encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: [] }), "INVALID_ARGUMENT");
assertProtocolError(() => decodeMessage([], "command"), "INVALID_ARGUMENT");
assertProtocolError(() => decodeMessage(new Uint8Array(), "other"), "INVALID_DIRECTION");
assertProtocolError(() => encodeMessage({ type: MessageType.Cancel, requestId: 1n, payload: Uint8Array.of(1) }), "PAYLOAD_NOT_EMPTY");
assertProtocolError(() => encodeMessage({ type: MessageType.LoadText, requestId: 1n, payload: new Uint8Array(MAX_MESSAGE_BYTES - HEADER_BYTES + 1) }), "MESSAGE_TOO_LARGE");
assertProtocolError(() => encodeMessage(null), "INVALID_ARGUMENT");
const oversizedDeclaration = encodeMessage({ type: MessageType.LoadText, requestId: 1n, payload: new Uint8Array() });
new DataView(oversizedDeclaration.buffer).setUint32(20, 0xffffffff, true);
assertProtocolError(() => decodeMessage(oversizedDeclaration, "command"), "MESSAGE_TOO_LARGE");
assert.equal(HEADER_BYTES, 24);
const nonAsciiHex = runOracle("encode", "0010", "1", "1é");
assert.notEqual(nonAsciiHex.status, 0);
assert.match(nonAsciiHex.stderr, /ASCII/);

if (wasmPath) {
  const { readFile } = await import("node:fs/promises");
  const engine = await createWasmEngine(await readFile(wasmPath));
  for (const fixture of golden) {
    const transported = engine.dispatch(encodeMessage(fixture));
    assert.deepEqual(
      decodeMessage(transported, fixture.direction),
      { type: fixture.type, requestId: fixture.requestId, payload: fixture.payload },
      "real ABI dispatch transports a protocol frame by exact echo",
    );
  }
}

console.log(`US-091 protocol checks passed (${types.length} Rust/JS cross-codec cases${wasmPath ? ", ABI transport" : ""}).`);
