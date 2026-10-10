// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { encodeMessage, decodeMessage, MessageType, HEADER_BYTES, MAX_MESSAGE_BYTES, ProtocolError } from "../web/wasm-protocol.mjs";

const payload = new Uint8Array(MAX_MESSAGE_BYTES - HEADER_BYTES).fill(0x5a);
const frame = encodeMessage({ type: MessageType.ApplyOps, requestId: 1n << 63n, payload });
assert.equal(frame.byteLength, MAX_MESSAGE_BYTES);
const decoded = decodeMessage(frame, "command");
assert.equal(decoded.requestId, 1n << 63n);
assert.equal(decoded.payload.byteLength, payload.byteLength);
assert.ok(decoded.payload.every((value) => value === 0x5a));
frame.fill(0, HEADER_BYTES);
assert.equal(decoded.payload[0], 0x5a);
assert.equal(decoded.payload.at(-1), 0x5a);
assert.throws(() => decodeMessage(new Uint8Array(MAX_MESSAGE_BYTES + 1), "command"),
  (error) => error instanceof ProtocolError && error.code === "MESSAGE_TOO_LARGE");
console.log(`Protocol exact maximum ${MAX_MESSAGE_BYTES} bytes accepted; one byte over rejected.`);
