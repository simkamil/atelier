// SPDX-License-Identifier: AGPL-3.0-or-later

export const MessageType = Object.freeze({
  Ping: 0x0001,
  LoadText: 0x0010,
  ApplyOps: 0x0011,
  Render: 0x0012,
  Cancel: 0x0013,
  Editor: 0x0020,
  Pong: 0x8001,
  Completed: 0x8010,
  Failed: 0x8011,
  Progress: 0x8012,
  Cancelled: 0x8013,
  EditorResult: 0x8020,
});

export const HEADER_BYTES = 24;
export const MAX_MESSAGE_BYTES = 64 * 1024 * 1024 - 4;
const MAGIC = [0x41, 0x54, 0x4c, 0x4d];
const MAX_U64 = (1n << 64n) - 1n;
const TYPES = new Map(Object.values(MessageType).map((value) => [value, value]));
const COMMANDS = new Set([MessageType.Ping, MessageType.LoadText, MessageType.ApplyOps, MessageType.Render, MessageType.Cancel, MessageType.Editor]);
const EMPTY_TYPES = new Set([MessageType.Ping, MessageType.Pong, MessageType.Cancel, MessageType.Cancelled]);

export class ProtocolError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "ProtocolError";
    this.code = code;
  }
}

function fail(code, message) {
  throw new ProtocolError(code, message);
}

function checkType(type) {
  if (typeof type !== "number" || !Number.isInteger(type) || !TYPES.has(type)) {
    fail("INVALID_TYPE", "type must be one of the ten v1 MessageType constants.");
  }
}

function checkId(requestId) {
  if (typeof requestId !== "bigint") {
    fail("INVALID_REQUEST_ID", "requestId must be a BigInt; Number is never accepted.");
  }
  if (requestId < 1n || requestId > MAX_U64) {
    fail("INVALID_REQUEST_ID", "requestId must be between 1 and 2^64-1.");
  }
}

function checkPayload(payload) {
  if (!(payload instanceof Uint8Array)) {
    fail("INVALID_ARGUMENT", "payload must be a Uint8Array.");
  }
  if (payload.byteLength > MAX_MESSAGE_BYTES - HEADER_BYTES) {
    fail("MESSAGE_TOO_LARGE", `payload exceeds ${MAX_MESSAGE_BYTES - HEADER_BYTES} bytes.`);
  }
}

function checkEmptyPayload(type, length) {
  if (EMPTY_TYPES.has(type) && length !== 0) {
    fail("PAYLOAD_NOT_EMPTY", "Ping, Pong, Cancel, and Cancelled require an empty payload.");
  }
}

function checkDirection(direction) {
  if (direction !== "command" && direction !== "event") {
    fail("INVALID_DIRECTION", "direction must be 'command' or 'event'.");
  }
}

export function encodeMessage(input = undefined) {
  if (input === null || (input !== undefined && typeof input !== "object")) {
    fail("INVALID_ARGUMENT", "encodeMessage expects a message object.");
  }
  const { type, requestId, payload } = input ?? {};
  checkType(type);
  checkId(requestId);
  checkPayload(payload);
  checkEmptyPayload(type, payload.byteLength);
  const total = HEADER_BYTES + payload.byteLength;
  if (!Number.isSafeInteger(total) || total > MAX_MESSAGE_BYTES || payload.byteLength > 0xffffffff) {
    fail("MESSAGE_TOO_LARGE", "encoded message exceeds the protocol size limit.");
  }
  const bytes = new Uint8Array(total);
  bytes.set(MAGIC, 0);
  const view = new DataView(bytes.buffer);
  view.setUint16(4, 1, true);
  view.setUint16(6, type, true);
  view.setUint16(8, 0, true);
  view.setUint16(10, 0, true);
  view.setBigUint64(12, requestId, true);
  view.setUint32(20, payload.byteLength, true);
  bytes.set(payload, HEADER_BYTES);
  return bytes;
}

export function decodeMessage(bytes, direction) {
  if (!(bytes instanceof Uint8Array)) {
    fail("INVALID_ARGUMENT", "bytes must be a Uint8Array.");
  }
  checkDirection(direction);
  if (bytes.byteLength > MAX_MESSAGE_BYTES) {
    fail("MESSAGE_TOO_LARGE", `message exceeds ${MAX_MESSAGE_BYTES} bytes.`);
  }
  if (bytes.byteLength < HEADER_BYTES) {
    fail("TRUNCATED", "message is shorter than the 24-byte header.");
  }
  // Snapshot shared or otherwise mutable input once; validate and extract only
  // from this owned frame so the payload cannot change after header checking.
  const frame = Uint8Array.from(bytes);
  const view = new DataView(frame.buffer);
  for (let index = 0; index < MAGIC.length; index += 1) {
    if (view.getUint8(index) !== MAGIC[index]) fail("INVALID_MAGIC", "message magic must be ATLM.");
  }
  if (view.getUint16(4, true) !== 1) fail("UNSUPPORTED_VERSION", "only protocol version 1 is supported.");
  const type = view.getUint16(6, true);
  checkType(type);
  if ((direction === "command") !== COMMANDS.has(type)) {
    fail("WRONG_DIRECTION", `${type.toString(16)} is invalid in the ${direction} direction.`);
  }
  if (view.getUint16(8, true) !== 0) fail("INVALID_FLAGS", "v1 flags must be zero.");
  if (view.getUint16(10, true) !== 0) fail("NONZERO_RESERVED", "v1 reserved field must be zero.");
  const requestId = view.getBigUint64(12, true);
  checkId(requestId);
  const payloadLength = view.getUint32(20, true);
  const expected = HEADER_BYTES + payloadLength;
  if (!Number.isSafeInteger(expected) || expected > MAX_MESSAGE_BYTES) {
    fail("MESSAGE_TOO_LARGE", "declared message size exceeds the protocol limit.");
  }
  if (frame.byteLength < expected) fail("TRUNCATED", "payload is shorter than its declared length.");
  if (frame.byteLength > expected) fail("TRAILING_BYTES", "message contains bytes beyond its declared payload.");
  checkEmptyPayload(type, payloadLength);
  return { type, requestId, payload: Uint8Array.from(frame.subarray(HEADER_BYTES, expected)) };
}

/** Cooperative job kinds of the staged Editor opcodes (see crates/core-wasm/src/staged.rs). */
export const StagedKind = Object.freeze({ OpenDocx: 1, ExportDocx: 2 });
export const STAGED_PROGRESS_TOTAL = 1000;
const STAGED_OPCODE = Object.freeze({ Begin: 23, Step: 24, Cancel: 25 });

function stagedFrame(requestId, opcode, body) {
  const payload = new Uint8Array(8 + body.byteLength);
  const view = new DataView(payload.buffer);
  payload.set([0x41, 0x54, 0x45, 0x44], 0);
  view.setUint16(4, 1, true);
  view.setUint16(6, opcode, true);
  payload.set(body, 8);
  return encodeMessage({ type: MessageType.Editor, requestId, payload });
}

/** Returns the staged opcode (23..25) of a decoded Editor command, or 0. */
export function stagedOpcodeOf(decoded) {
  const { payload } = decoded;
  if (decoded.type !== MessageType.Editor || payload.byteLength < 8
      || payload[0] !== 0x41 || payload[1] !== 0x54 || payload[2] !== 0x45 || payload[3] !== 0x44) return 0;
  const opcode = new DataView(payload.buffer, payload.byteOffset).getUint16(6, true);
  return Object.values(STAGED_OPCODE).includes(opcode) ? opcode : 0;
}

/** Begin an abortable open (preview) or export of a DOCX; send the frame with `client.request`. */
export function encodeStagedBegin(requestId, kind, docxBytes) {
  if (kind !== StagedKind.OpenDocx && kind !== StagedKind.ExportDocx) fail("INVALID_ARGUMENT", "unknown staged job kind.");
  if (!(docxBytes instanceof Uint8Array)) fail("INVALID_ARGUMENT", "docxBytes must be a Uint8Array.");
  const body = new Uint8Array(4 + docxBytes.byteLength);
  new DataView(body.buffer).setUint16(0, kind, true);
  body.set(docxBytes, 4);
  return stagedFrame(requestId, STAGED_OPCODE.Begin, body);
}

function jobFrame(requestId, opcode, jobId) {
  const body = new Uint8Array(8);
  new DataView(body.buffer).setBigUint64(0, jobId, true);
  return stagedFrame(requestId, opcode, body);
}
export const encodeStagedStep = (requestId, jobId) => jobFrame(requestId, STAGED_OPCODE.Step, jobId);
export const encodeStagedCancel = (requestId, jobId) => jobFrame(requestId, STAGED_OPCODE.Cancel, jobId);

/** Decodes an `ATJB` progress payload; `undefined` when the payload is a final result. */
export function decodeStagedProgress(payload) {
  if (payload.byteLength !== 24 || payload[0] !== 0x41 || payload[1] !== 0x54 || payload[2] !== 0x4a || payload[3] !== 0x42) return undefined;
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (view.getUint16(4, true) !== 1) fail("UNSUPPORTED_VERSION", "unsupported staged progress version.");
  const state = view.getUint16(6, true);
  const done = view.getUint32(8, true);
  const total = view.getUint32(12, true);
  if (state > 1 || total !== STAGED_PROGRESS_TOTAL || done > total) fail("INVALID_RESPONSE", "malformed staged progress.");
  return { state, done, total, jobId: view.getBigUint64(16, true) };
}

/** Decodes the final `ATSV` payload of an export job. */
export function decodeStagedExport(payload) {
  if (payload.byteLength < 12 || payload[0] !== 0x41 || payload[1] !== 0x54 || payload[2] !== 0x53 || payload[3] !== 0x56) fail("INVALID_RESPONSE", "not an export result.");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (view.getUint16(4, true) !== 1) fail("UNSUPPORTED_VERSION", "unsupported export result version.");
  return { noticeCount: view.getUint32(8, true), bytes: Uint8Array.from(payload.subarray(12)) };
}
