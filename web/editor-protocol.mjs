// SPDX-License-Identifier: AGPL-3.0-or-later
import { decodeDisplayList } from "./display-list.mjs";

export const EditorOpcode = Object.freeze({
  State: 0, Load: 1, InsertText: 2, DeleteBackward: 3, DeleteForward: 4,
  Undo: 5, Redo: 6, SetSelection: 7, Move: 8, HitTest: 9, Export: 10, MarkSaved: 11,
  DeleteWordBackward: 12, DeleteWordForward: 13, SelectPoint: 14,
  ProbePoint: 15, DropText: 16, ReplaceRange: 17,
});

export const EDITOR_LIMITS = Object.freeze({
  maxFileBytes: 131075,
  maxTextBytes: 128 * 1024,
  maxStateBytes: 32 * 1024 * 1024,
  maxPages: 1024,
});

const MAX_U64 = (1n << 64n) - 1n;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const EDITOR_MAGIC = [0x41, 0x54, 0x45, 0x44];
const STATE_MAGIC = [0x41, 0x54, 0x45, 0x53];
const EXPORT_MAGIC = [0x41, 0x54, 0x45, 0x58];
const DIRECTIONS = new Map([
  ["left", 0], ["right", 1], ["up", 2], ["down", 3], ["home", 4],
  ["end", 5], ["documentStart", 6], ["documentEnd", 7],
  ["pageUp", 8], ["pageDown", 9],
  ["wordLeft", 10], ["wordRight", 11], ["paragraphUp", 12], ["paragraphDown", 13],
]);

export class EditorProtocolError extends Error {
  constructor(code, message) { super(message); this.name = "EditorProtocolError"; this.code = code; }
}

function fail(code, message) { throw new EditorProtocolError(code, message); }
function isBytes(value) { return value instanceof Uint8Array; }
function u64(value, name) {
  let n;
  if (typeof value === "number" && (!Number.isSafeInteger(value) || value < 0)) {
    fail("INVALID_ARGUMENT", `${name} must be a safe unsigned integer or BigInt.`);
  }
  try { n = typeof value === "bigint" ? value : BigInt(value); } catch { fail("INVALID_ARGUMENT", `${name} must be an unsigned integer.`); }
  if (n < 0n || n > MAX_U64) fail("INVALID_ARGUMENT", `${name} is out of range.`);
  return n;
}
function u32(value, name) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) fail("INVALID_ARGUMENT", `${name} is out of range.`);
  return value;
}
function finite(value, name) {
  if (!Number.isFinite(value)) fail("INVALID_ARGUMENT", `${name} must be finite.`);
  return value;
}
function ascii(bytes) {
  for (const byte of bytes) if (byte > 0x7f) fail("INVALID_RESPONSE", "Expected ASCII bytes.");
  return String.fromCharCode(...bytes);
}
function requireMagic(view, bytes, magic, code) {
  if (bytes.length < magic.length) fail(code, "Truncated editor payload.");
  for (let i = 0; i < magic.length; i += 1) if (view.getUint8(i) !== magic[i]) fail(code, "Invalid editor payload magic.");
}
function boolByte(value, name) {
  if (typeof value !== "boolean") fail("INVALID_ARGUMENT", `${name} must be boolean.`);
  return value ? 1 : 0;
}

/** Encode an ATED v1 command payload as defined by the shared editor contract. */
export function encodeEditorCommand(opcode, values = {}) {
  if (!Number.isInteger(opcode) || opcode < 0 || opcode > 17 || values === null || typeof values !== "object") {
    fail("INVALID_ARGUMENT", "Invalid editor opcode or arguments.");
  }
  let body;
  if (opcode === EditorOpcode.Load) {
    if (!isBytes(values.bytes) || values.bytes.byteLength > EDITOR_LIMITS.maxFileBytes) fail("MESSAGE_TOO_LARGE", "Document bytes exceed the editor limit.");
    body = Uint8Array.from(values.bytes);
  } else if (opcode === EditorOpcode.InsertText) {
    if (typeof values.text !== "string") fail("INVALID_ARGUMENT", "Inserted text must be a string.");
    body = encoder.encode(values.text);
    if (body.byteLength > EDITOR_LIMITS.maxTextBytes) fail("MESSAGE_TOO_LARGE", "Inserted text exceeds the editor limit.");
    const prefix = new Uint8Array(8);
    new DataView(prefix.buffer).setBigUint64(0, u64(values.nowMs, "nowMs"), true);
    body = concat(prefix, body);
  } else if ([EditorOpcode.DeleteBackward, EditorOpcode.DeleteForward, EditorOpcode.DeleteWordBackward, EditorOpcode.DeleteWordForward].includes(opcode)) {
    body = new Uint8Array(8);
    new DataView(body.buffer).setBigUint64(0, u64(values.nowMs, "nowMs"), true);
  } else if (opcode === EditorOpcode.SetSelection) {
    body = new Uint8Array(8);
    const view = new DataView(body.buffer);
    view.setUint32(0, u32(values.anchor, "anchor"), true);
    view.setUint32(4, u32(values.focus, "focus"), true);
  } else if (opcode === EditorOpcode.Move) {
    const direction = typeof values.direction === "number" ? values.direction : DIRECTIONS.get(values.direction);
    if (!Number.isInteger(direction) || direction < 0 || direction > 13) fail("INVALID_ARGUMENT", "Invalid movement direction.");
    body = new Uint8Array([direction, boolByte(values.extend ?? false, "extend"), 0, 0]);
  } else if (opcode === EditorOpcode.ProbePoint) {
    body = new Uint8Array(20);
    const view = new DataView(body.buffer);
    view.setUint32(0, u32(values.page, "page"), true);
    view.setFloat64(4, finite(values.x, "x"), true);
    view.setFloat64(12, finite(values.y, "y"), true);
  } else if (opcode === EditorOpcode.DropText || opcode === EditorOpcode.ReplaceRange) {
    const prefix = opcode === EditorOpcode.DropText ? 32 : 24;
    if (typeof values.text !== "string") fail("INVALID_ARGUMENT", "Edit text must be a string.");
    const text = encoder.encode(values.text);
    if (text.byteLength > EDITOR_LIMITS.maxTextBytes) fail("MESSAGE_TOO_LARGE", "Edit text exceeds the editor limit.");
    body = new Uint8Array(prefix + text.byteLength);
    const view = new DataView(body.buffer);
    view.setBigUint64(0, u64(values.revision, "revision"), true);
    view.setBigUint64(8, u64(values.nowMs, "nowMs"), true);
    if (opcode === EditorOpcode.DropText) {
      const kind = ["move", "copy", "external"].indexOf(values.kind);
      if (kind < 0 || (kind !== 2 && text.byteLength !== 0)) fail("INVALID_ARGUMENT", "Invalid text drop.");
      view.setUint32(16, u32(values.sourceAnchor, "sourceAnchor"), true);
      view.setUint32(20, u32(values.sourceFocus, "sourceFocus"), true);
      view.setUint32(24, u32(values.target, "target"), true);
      view.setUint8(28, kind);
    } else {
      view.setUint32(16, u32(values.start, "start"), true);
      view.setUint32(20, u32(values.end, "end"), true);
      if (values.start > values.end) fail("INVALID_ARGUMENT", "Reversed replacement range.");
    }
    body.set(text, prefix);
  } else if (opcode === EditorOpcode.HitTest) {
    body = new Uint8Array(21);
    const view = new DataView(body.buffer);
    view.setUint32(0, u32(values.page, "page"), true);
    view.setFloat64(4, finite(values.x, "x"), true);
    view.setFloat64(12, finite(values.y, "y"), true);
    view.setUint8(20, boolByte(values.extend ?? false, "extend"));
  } else if (opcode === EditorOpcode.SelectPoint) {
    const unit = typeof values.unit === "number" ? values.unit : ["character", "word", "paragraph"].indexOf(values.unit);
    if (!Number.isInteger(unit) || unit < 0 || unit > 2) fail("INVALID_ARGUMENT", "Invalid selection unit.");
    body = new Uint8Array(32);
    const view = new DataView(body.buffer);
    view.setUint32(0, u32(values.page, "page"), true);
    view.setFloat64(4, finite(values.x, "x"), true);
    view.setFloat64(12, finite(values.y, "y"), true);
    view.setUint8(20, unit);
    view.setUint8(21, boolByte(values.extend ?? false, "extend"));
    view.setUint32(24, u32(values.anchor ?? 0, "anchor"), true);
    view.setUint32(28, u32(values.anchorEnd ?? 0, "anchorEnd"), true);
  } else if (opcode === EditorOpcode.MarkSaved) {
    body = new Uint8Array(8);
    new DataView(body.buffer).setBigUint64(0, u64(values.revision, "revision"), true);
  } else if ([EditorOpcode.State, EditorOpcode.Undo, EditorOpcode.Redo, EditorOpcode.Export].includes(opcode)) {
    body = new Uint8Array();
  } else fail("INVALID_ARGUMENT", "Unknown editor opcode.");

  const payload = new Uint8Array(8 + body.byteLength);
  payload.set(EDITOR_MAGIC);
  const view = new DataView(payload.buffer);
  view.setUint16(4, 1, true);
  view.setUint16(6, opcode, true);
  payload.set(body, 8);
  return payload;
}

function concat(left, right) {
  const result = new Uint8Array(left.byteLength + right.byteLength);
  result.set(left); result.set(right, left.byteLength); return result;
}

/** Decode strict EditorResult/Failed payloads. All returned byte arrays are owned copies. */
export function decodeEditorResponse(type, payload) {
  if (!isBytes(payload) || payload.byteLength > EDITOR_LIMITS.maxStateBytes) fail("INVALID_RESPONSE", "Invalid or oversized editor response.");
  if (type === 0x8011) {
    if (payload.byteLength > 64) fail("INVALID_RESPONSE", "Editor failure code exceeds its bound.");
    const code = ascii(payload);
    if (!/^EDITOR_[A-Z0-9_]{1,56}$/.test(code)) fail("INVALID_RESPONSE", "Invalid editor failure code.");
    return { kind: "failed", code };
  }
  if (type !== 0x8020) fail("INVALID_RESPONSE", "Unexpected editor response type.");
  if (payload.byteLength < 8) fail("TRUNCATED", "Truncated editor response.");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (payload.length >= 4 && ascii(payload.subarray(0, 4)) === "ATPT") {
    if (payload.length !== 48 || view.getUint16(4, true) !== 1 || view.getUint16(6, true) !== 0) {
      fail("INVALID_RESPONSE", "Invalid point probe response.");
    }
    const offset = view.getUint32(16, true), page = view.getUint32(20, true);
    const x = view.getFloat64(24, true), y = view.getFloat64(32, true), height = view.getFloat64(40, true);
    if (offset > EDITOR_LIMITS.maxTextBytes || page >= EDITOR_LIMITS.maxPages || ![x,y,height].every(Number.isFinite)
        || x < 0 || y < 0 || height <= 0 || x > 595.296 || y + height > 841.91) {
      fail("INVALID_RESPONSE", "Invalid point probe geometry.");
    }
    return { kind: "point", revision: view.getBigUint64(8, true), offset, caret: { page, x, y, height } };
  }
  if (payload.byteLength >= 16 && EXPORT_MAGIC.every((byte, i) => view.getUint8(i) === byte)) {
    if (view.getUint16(4, true) !== 1 || view.getUint16(6, true) !== 0 || payload.byteLength > 16 + EDITOR_LIMITS.maxFileBytes) {
      fail("INVALID_RESPONSE", "Invalid or oversized editor export.");
    }
    return { kind: "export", revision: view.getBigUint64(8, true), bytes: payload.slice(16) };
  }
  requireMagic(view, payload, STATE_MAGIC, "INVALID_RESPONSE");
  if (view.getUint16(4, true) !== 1) fail("UNSUPPORTED_VERSION", "Unsupported editor state version.");
  const flags = view.getUint16(6, true);
  if ((flags & ~0x0f) !== 0) fail("INVALID_RESPONSE", "Unknown editor state flags.");
  if (payload.byteLength < 64) fail("TRUNCATED", "Truncated editor state header.");
  const revision = view.getBigUint64(8, true);
  const anchor = view.getUint32(16, true);
  const focus = view.getUint32(20, true);
  const textLength = view.getUint32(24, true);
  const pageCount = view.getUint32(28, true);
  const caretPage = view.getUint32(32, true);
  const caretX = view.getFloat64(36, true);
  const caretY = view.getFloat64(44, true);
  const caretHeight = view.getFloat64(52, true);
  const rectCount = view.getUint32(60, true);
  if (textLength > EDITOR_LIMITS.maxTextBytes || pageCount === 0 || pageCount > EDITOR_LIMITS.maxPages
      || !Number.isFinite(caretX) || !Number.isFinite(caretY) || !Number.isFinite(caretHeight) || caretHeight <= 0
      || caretPage >= pageCount || caretX < 0 || caretY < 0) {
    fail("INVALID_RESPONSE", "Editor state fields exceed their bounds.");
  }
  let offset = 64;
  if (textLength > payload.byteLength - offset) fail("TRUNCATED", "Truncated editor text.");
  const textBytes = payload.subarray(offset, offset + textLength);
  let text;
  try { text = decoder.decode(textBytes); } catch { fail("INVALID_RESPONSE", "Editor text is not valid UTF-8."); }
  offset += textLength;
  if (anchor > textLength || focus > textLength) fail("INVALID_RESPONSE", "Editor selection is outside text.");
  // JavaScript TextEncoder gives the byte offset for a scalar boundary. Reject offsets inside a UTF-8 sequence.
  const boundaries = new Set([0, textLength]);
  let byteOffset = 0;
  for (const scalar of text) { byteOffset += encoder.encode(scalar).byteLength; boundaries.add(byteOffset); }
  if (!boundaries.has(anchor) || !boundaries.has(focus)) fail("INVALID_RESPONSE", "Editor selection splits a UTF-8 scalar.");
  const pages = [];
  const decodedPages = [];
  for (let page = 0; page < pageCount; page += 1) {
    if (offset + 4 > payload.byteLength) fail("TRUNCATED", "Truncated editor page length.");
    const length = view.getUint32(offset, true); offset += 4;
    if (length > payload.byteLength - offset) fail("TRUNCATED", "Truncated ATDL page.");
    const bytes = payload.slice(offset, offset + length);
    try { decodedPages.push(decodeDisplayList(bytes)); }
    catch { fail("INVALID_RESPONSE", "Editor returned an invalid ATDL page."); }
    pages.push(bytes); offset += length;
  }
  const caretBounds = decodedPages[caretPage].page.bounds;
  if (caretX < caretBounds.min.x || caretX > caretBounds.max.x || caretY < caretBounds.min.y || caretY > caretBounds.max.y
      || caretHeight > caretBounds.height) fail("INVALID_RESPONSE", "Editor caret is outside its page.");
  if (rectCount > Math.floor((payload.byteLength - offset) / 36)) fail("TRUNCATED", "Truncated editor selection rectangles.");
  const selectionRects = [];
  for (let i = 0; i < rectCount; i += 1) {
    const page = view.getUint32(offset, true);
    const x = view.getFloat64(offset + 4, true);
    const y = view.getFloat64(offset + 12, true);
    const width = view.getFloat64(offset + 20, true);
    const height = view.getFloat64(offset + 28, true);
    offset += 36;
    const bounds = decodedPages[page]?.page.bounds;
    if (page >= pageCount || ![x, y, width, height].every(Number.isFinite) || width < 0 || height < 0
        || !bounds || x < bounds.min.x || y < bounds.min.y || x + width > bounds.max.x || y + height > bounds.max.y) {
      fail("INVALID_RESPONSE", "Invalid editor selection rectangle.");
    }
    selectionRects.push({ page, x, y, width, height });
  }
  if (offset !== payload.byteLength) fail("TRAILING_BYTES", "Editor state has trailing bytes.");
  return {
    kind: "state", revision, flags,
    canUndo: Boolean(flags & 1), canRedo: Boolean(flags & 2), dirty: Boolean(flags & 4), bom: Boolean(flags & 8),
    anchor, focus, text, textBytes: textBytes.slice(), pages, caret: { page: caretPage, x: caretX, y: caretY, height: caretHeight }, selectionRects,
  };
}
