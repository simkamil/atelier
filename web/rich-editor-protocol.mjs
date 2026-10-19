// SPDX-License-Identifier: AGPL-3.0-or-later
// Reference codec of the ATED v2 rich-editor envelope (US-203). Mirrors
// crates/core-wasm/src/rich_protocol.rs; specification in
// docs/us-203-rich-editor-protocol.md. Envelope and message types only: the
// page cache and editor integration belong to US-203 part 2 (R4).
// Positions, revisions and byte lengths that can exceed 2^32 are BigInt.
// Decoders do not copy: `data`, page `display` are views (subarray) into the
// frame passed in, which the caller must not mutate while using them.

import {
  FormatErrorCode, FormatOpcode, FormatReplyOpcode, readFormatCommand, readFormatReply, writeFormatCommand, writeFormatReply,
} from "./rich-editor-format.mjs";

export const RICH_VERSION = 2;
export const HEADER_BYTES = 20;

export const RICH_LIMITS = Object.freeze({
  maxFrameBytes: 32 * 1024 * 1024,
  maxOpenBytes: 32 * 1024 * 1024 - HEADER_BYTES - 8,
  maxTextBytes: 1024 * 1024,
  maxWindowBytes: 256 * 1024,
  maxPagesPerResponse: 16,
  maxPageDisplayBytes: 16 * 1024 * 1024,
  maxResponsePageBytes: 8 * 1024 * 1024,
  minPageDisplayBytes: 44,
  maxAtlasEntries: 4096,
  maxAtlasEntryBytes: 64 * 1024,
  maxAtlasResponseBytes: 4 * 1024 * 1024,
  maxInvalidations: 1024,
  maxPages: 1 << 20,
  maxLayoutBudget: 4096,
  defaultLayoutBudget: 16,
  maxAccessibleItems: 256,
  // US-218 (R3b): pictures and saved files.
  maxImageBytes: 8 * 1024 * 1024,
  maxAltBytes: 512,
  maxNotices: 64,
  maxNoticeBytes: 255,
  maxStampChars: 255,
  maxSaveBytes: 32 * 1024 * 1024 - HEADER_BYTES - (8 + 4 + 2) - 64 * (4 + 255),
  // R7b-1: selection rectangles and picture bitmaps.
  maxSelectionRects: 4096,
  maxBitmapSide: 1600,
  // R7b-2: search, replace, statistics, properties.
  maxQueryBytes: 1024,
  maxSearchItems: 1000,
  searchCountCap: 10000,
  maxExcerptBytes: 4096,
  maxHighlightRanges: 1000,
  maxHighlightRects: 8192,
  maxPropertyBytes: 64 * 1024,
  maxReplaceMatches: 5000,
});
export const NO_CARET = 0xffffffff;

export const RichOpcode = Object.freeze({
  Open: 0x0101, Close: 0x0102, InsertText: 0x0103, ReplaceRange: 0x0104, Delete: 0x0105,
  Move: 0x0106, SetSelection: 0x0107, HitTest: 0x0108, Undo: 0x0109, Redo: 0x010a,
  TextWindow: 0x010b, Pages: 0x010c, MarkSaved: 0x010d, Layout: 0x010e, AccessibleWindow: 0x010f,
  // US-218 (R3b)
  Save: 0x0110, InsertImage: 0x0111, MakeEditableCopy: 0x0112,
  // R2b
  Resync: 0x0118,
  // R7b-1
  SelectionRects: 0x0120, ImageBitmap: 0x0121, SelectUnit: 0x0122,
  // R7c: text drag and drop.
  MoveText: 0x0140,
  // R7b-2: search, replace, statistics, properties.
  Search: 0x0130, Statistics: 0x0131, SearchRects: 0x0132, Replace: 0x0133, ReplaceAll: 0x0134,
  PropertiesRead: 0x0135, PropertiesApply: 0x0136,
  // F0: formatting of characters, paragraphs and styles (codec in web/rich-editor-format.mjs).
  ...FormatOpcode,
});
export const RichReplyOpcode = Object.freeze({
  Error: 0x8000, State: 0x8100, TextWindow: 0x8101, Pages: 0x8102, HitResult: 0x8103, Closed: 0x8104,
  AccessibleWindow: 0x8105,
  SaveData: 0x8106,
  SelectionRects: 0x8110, ImageBitmap: 0x8111,
  SearchResult: 0x8120, Statistics: 0x8121, SearchRects: 0x8122, Replaced: 0x8123, Properties: 0x8125, PropertiesApplied: 0x8126,
  ...FormatReplyOpcode,
});

export const RichErrorCode = Object.freeze(Object.assign(Object.create(null), {
  RICH_INVALID_ENVELOPE: 1, RICH_UNSUPPORTED_VERSION: 2, RICH_UNKNOWN_OPCODE: 3, RICH_INVALID_BODY: 4,
  RICH_INVALID_DOCUMENT_ID: 5, RICH_UNKNOWN_DOCUMENT: 6, RICH_POSITION_OUT_OF_RANGE: 7,
  RICH_POSITION_NOT_BOUNDARY: 8, RICH_INVALID_RANGE: 9, RICH_INVALID_UTF8: 10, RICH_INVALID_TEXT: 11,
  RICH_FRAME_TOO_LARGE: 12, RICH_WINDOW_TOO_LARGE: 13, RICH_PAGE_RANGE: 14, RICH_TOO_MANY_PAGES: 15,
  RICH_STALE_REVISION: 16, RICH_OPEN_FAILED: 17, RICH_INTERNAL: 18, RICH_LIMIT: 19, RICH_NOT_READY: 20,
  // US-218 (R3b)
  RICH_READ_ONLY: 21, RICH_SAVE_FAILED: 22, RICH_IMAGE_INVALID: 23, RICH_REVISIONS_UNSUPPORTED: 24,
  // R7b-1
  RICH_UNKNOWN_OBJECT: 30,
  // R7b-2 (codes 40 to 49)
  RICH_EMPTY_QUERY: 40, RICH_STALE_MATCH: 41, RICH_QUERY_TOO_LARGE: 42, RICH_PROPERTIES_FAILED: 43, RICH_TOO_MANY_MATCHES: 44,
  // R7c (codes 50 to 59)
  RICH_MOVE_UNSUPPORTED: 50,
  // F0 (codes 60 to 69)
  ...FormatErrorCode,
}));
const ERROR_NAMES = new Map(Object.entries(RichErrorCode).map(([name, code]) => [code, name]));

const OPEN_KINDS = ["blank", "txt", "docx"];
const PROFILES = ["document", "txt"];
export const RICH_DIRECTIONS = Object.freeze([
  "left", "right", "up", "down", "home", "end", "documentStart", "documentEnd",
  "pageUp", "pageDown", "wordLeft", "wordRight", "paragraphUp", "paragraphDown",
]);
const UNITS = ["grapheme", "word"];
const HIT_MODES = ["probe", "setCaret", "extend"];
/** Units of `selectUnit`, by wire value. */
export const SELECTION_UNITS = Object.freeze(["word", "paragraph"]);
/** Formats of `save`, by wire value (1..4). */
export const SAVE_FORMATS = Object.freeze([null, "txt", "docx", "docm", "dotx"]);

const MAGIC = [0x41, 0x54, 0x45, 0x44];
const MAX_U64 = (1n << 64n) - 1n;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** `code` is a stable name (RICH_*); `context` is {docId, requestId} when the header was readable. */
export class RichProtocolError extends Error {
  constructor(code, context = null) {
    super(code);
    this.name = "RichProtocolError";
    this.code = code;
    this.context = context;
  }
}

function fail(code, context = null) { throw new RichProtocolError(code, context); }

class Reader {
  constructor(bytes) { this.bytes = bytes; this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); this.at = 0; }
  need(n) { if (this.at + n > this.bytes.length) fail("RICH_INVALID_BODY"); }
  take(n) { this.need(n); const out = this.bytes.subarray(this.at, this.at + n); this.at += n; return out; }
  u8() { this.need(1); return this.bytes[this.at++]; }
  u16() { this.need(2); const v = this.view.getUint16(this.at, true); this.at += 2; return v; }
  u32() { this.need(4); const v = this.view.getUint32(this.at, true); this.at += 4; return v; }
  u64() { this.need(8); const v = this.view.getBigUint64(this.at, true); this.at += 8; return v; }
  f64() { this.need(8); const v = this.view.getFloat64(this.at, true); this.at += 8; if (!Number.isFinite(v)) fail("RICH_INVALID_BODY"); return v; }
  zero(n) { if (this.take(n).some((b) => b !== 0)) fail("RICH_INVALID_BODY"); }
  flag() { const v = this.u8(); if (v > 1) fail("RICH_INVALID_BODY"); return v === 1; }
  finish() { if (this.at !== this.bytes.length) fail("RICH_INVALID_BODY"); }
  text(max, tooBig) {
    const len = this.u32();
    if (len > max) fail(tooBig);
    try { return decoder.decode(this.take(len)); } catch (error) {
      if (error instanceof RichProtocolError) throw error;
      return fail("RICH_INVALID_UTF8");
    }
  }
}

class Writer {
  constructor() { this.parts = []; this.length = 0; }
  raw(bytes) { this.parts.push(bytes); this.length += bytes.length; }
  num(size, write) { const b = new Uint8Array(size); write(new DataView(b.buffer)); this.raw(b); }
  u8(...v) { this.raw(Uint8Array.from(v)); }
  u16(v) { this.num(2, (d) => d.setUint16(0, v, true)); }
  u32(v) { this.num(4, (d) => d.setUint32(0, v, true)); }
  u64(v) { this.num(8, (d) => d.setBigUint64(0, v, true)); }
  f64(v) { this.num(8, (d) => d.setFloat64(0, v, true)); }
  text(value) { const b = encoder.encode(value); this.u32(b.length); this.raw(b); }
  bytes() { const out = new Uint8Array(this.length); let at = 0; for (const p of this.parts) { out.set(p, at); at += p.length; } return out; }
}

function u64(value) {
  if (typeof value !== "bigint" || value < 0n || value > MAX_U64) fail("RICH_INVALID_BODY");
  return value;
}
function u32(value) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) fail("RICH_INVALID_BODY");
  return value;
}

/** No U+FFFC, no \r, no control character except \n and \t; lone surrogates are invalid UTF-8. */
export function validateEditText(text) {
  for (let i = 0; i < text.length; i += 1) {
    const c = text.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = text.charCodeAt(i + 1);
      if (!(d >= 0xdc00 && d <= 0xdfff)) fail("RICH_INVALID_UTF8");
      i += 1;
    } else if (c >= 0xdc00 && c <= 0xdfff) fail("RICH_INVALID_UTF8");
    else if (c === 0xfffc || (c < 0x20 && c !== 0x0a && c !== 0x09)) fail("RICH_INVALID_TEXT");
  }
}

function enumIndex(list, value) {
  const at = list.indexOf(value);
  if (at < 0) fail("RICH_INVALID_BODY");
  return at;
}

function frame(opcode, docId, requestId, body) {
  if (HEADER_BYTES + body.length > RICH_LIMITS.maxFrameBytes) fail("RICH_FRAME_TOO_LARGE");
  const out = new Uint8Array(HEADER_BYTES + body.length);
  const view = new DataView(out.buffer);
  out.set(MAGIC);
  view.setUint16(4, RICH_VERSION, true);
  view.setUint16(6, opcode, true);
  view.setUint32(8, u32(docId), true);
  view.setUint32(12, u32(requestId), true);
  view.setUint32(16, body.length, true);
  out.set(body, HEADER_BYTES);
  return out;
}

function split(input) {
  if (!(input instanceof Uint8Array)) fail("RICH_INVALID_ENVELOPE");
  const bytes = input;
  if (bytes.length < 6 || MAGIC.some((m, i) => bytes[i] !== m)) fail("RICH_INVALID_ENVELOPE");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint16(4, true) !== RICH_VERSION) fail("RICH_UNSUPPORTED_VERSION");
  if (bytes.length < HEADER_BYTES) fail("RICH_INVALID_ENVELOPE");
  const context = { docId: view.getUint32(8, true), requestId: view.getUint32(12, true) };
  const bodyLen = view.getUint32(16, true);
  if (bodyLen > RICH_LIMITS.maxFrameBytes - HEADER_BYTES) fail("RICH_FRAME_TOO_LARGE", context);
  if (bytes.length !== HEADER_BYTES + bodyLen) fail("RICH_INVALID_ENVELOPE", context);
  return { opcode: view.getUint16(6, true), context, body: bytes.subarray(HEADER_BYTES) };
}

function parseCommand(opcode, body) {
  const r = new Reader(body);
  let command;
  switch (opcode) {
    case RichOpcode.Open: {
      const kind = OPEN_KINDS[r.u8()];
      const profile = PROFILES[r.u8()];
      if (kind === undefined || profile === undefined) fail("RICH_INVALID_BODY");
      r.zero(2);
      const len = r.u32();
      if (len > RICH_LIMITS.maxOpenBytes) fail("RICH_FRAME_TOO_LARGE");
      const data = r.take(len);
      const consistent = kind === "blank" ? data.length === 0 : kind === "txt" ? profile === "txt" : profile === "document";
      if (!consistent) fail("RICH_INVALID_BODY");
      command = { type: "open", kind, profile, data };
      break;
    }
    case RichOpcode.Close: command = { type: "close" }; break;
    case RichOpcode.Undo: command = { type: "undo" }; break;
    case RichOpcode.Redo: command = { type: "redo" }; break;
    case RichOpcode.InsertText: {
      const text = r.text(RICH_LIMITS.maxTextBytes, "RICH_FRAME_TOO_LARGE");
      if (text.length === 0) fail("RICH_INVALID_BODY");
      validateEditText(text);
      command = { type: "insertText", text };
      break;
    }
    case RichOpcode.ReplaceRange: {
      const start = r.u64();
      const end = r.u64();
      if (start > end) fail("RICH_INVALID_RANGE");
      const text = r.text(RICH_LIMITS.maxTextBytes, "RICH_FRAME_TOO_LARGE");
      validateEditText(text);
      command = { type: "replaceRange", start, end, text };
      break;
    }
    case RichOpcode.Delete: {
      const forward = r.flag();
      const unit = UNITS[r.u8()];
      if (unit === undefined) fail("RICH_INVALID_BODY");
      r.zero(2);
      command = { type: "delete", forward, unit };
      break;
    }
    case RichOpcode.Move: {
      const direction = RICH_DIRECTIONS[r.u8()];
      if (direction === undefined) fail("RICH_INVALID_BODY");
      const extend = r.flag();
      r.zero(2);
      command = { type: "move", direction, extend };
      break;
    }
    case RichOpcode.SetSelection: {
      const anchor = r.u64();
      const focus = r.u64();
      const upstream = r.flag();
      r.zero(3);
      command = { type: "setSelection", anchor, focus, upstream };
      break;
    }
    case RichOpcode.HitTest: {
      const page = r.u32();
      const mode = HIT_MODES[r.u8()];
      if (mode === undefined) fail("RICH_INVALID_BODY");
      r.zero(3);
      if (page >= RICH_LIMITS.maxPages) fail("RICH_PAGE_RANGE");
      command = { type: "hitTest", page, mode, x: r.f64(), y: r.f64() };
      break;
    }
    case RichOpcode.TextWindow: {
      const start = r.u64();
      const len = r.u32();
      if (len === 0) fail("RICH_INVALID_BODY");
      if (len > RICH_LIMITS.maxWindowBytes) fail("RICH_WINDOW_TOO_LARGE");
      command = { type: "textWindow", start, len };
      break;
    }
    case RichOpcode.Pages: {
      const first = r.u32();
      const count = r.u32();
      if (count === 0) fail("RICH_INVALID_BODY");
      if (count > RICH_LIMITS.maxPagesPerResponse) fail("RICH_TOO_MANY_PAGES");
      if (first >= RICH_LIMITS.maxPages) fail("RICH_PAGE_RANGE");
      command = { type: "pages", first, count };
      break;
    }
    case RichOpcode.MarkSaved: command = { type: "markSaved", revision: r.u64() }; break;
    case RichOpcode.Layout: {
      const budget = r.u32();
      if (budget < 1 || budget > RICH_LIMITS.maxLayoutBudget) fail("RICH_INVALID_BODY");
      command = { type: "layout", budget };
      break;
    }
    case RichOpcode.Resync: command = { type: "resync" }; break;
    case RichOpcode.AccessibleWindow: {
      const start = r.u64();
      const end = r.u64();
      if (start > end) fail("RICH_INVALID_RANGE");
      if (end - start > BigInt(RICH_LIMITS.maxWindowBytes)) fail("RICH_WINDOW_TOO_LARGE");
      command = { type: "accessibleWindow", start, end };
      break;
    }
    case RichOpcode.Save: {
      const format = SAVE_FORMATS[r.u8()];
      if (!format) fail("RICH_INVALID_BODY");
      r.zero(3);
      command = { type: "save", format, ...readStamp(r) };
      break;
    }
    case RichOpcode.InsertImage: {
      const alt = text16(r);
      if (encoder.encode(alt).length > RICH_LIMITS.maxAltBytes || /[\u0000-\u001f\u007f\ufffe\uffff]/.test(alt)) fail("RICH_INVALID_TEXT");
      const len = r.bytes.length - r.at;
      if (len === 0) fail("RICH_INVALID_BODY");
      if (len > RICH_LIMITS.maxImageBytes) fail("RICH_LIMIT");
      command = { type: "insertImage", alt, bytes: r.take(len) };
      break;
    }
    case RichOpcode.MakeEditableCopy: command = { type: "makeEditableCopy", ...readStamp(r) }; break;
    case RichOpcode.SelectionRects: {
      const first = r.u32();
      const count = r.u32();
      if (count === 0) fail("RICH_INVALID_BODY");
      if (count > RICH_LIMITS.maxPagesPerResponse) fail("RICH_TOO_MANY_PAGES");
      if (first >= RICH_LIMITS.maxPages) fail("RICH_PAGE_RANGE");
      command = { type: "selectionRects", first, count };
      break;
    }
    case RichOpcode.ImageBitmap: command = { type: "imageBitmap", objectId: r.u64() }; break;
    case RichOpcode.SelectUnit: {
      const position = r.u64();
      const unit = SELECTION_UNITS[r.u8()];
      if (unit === undefined) fail("RICH_INVALID_BODY");
      const extend = r.flag();
      r.zero(2);
      command = { type: "selectUnit", position, unit, extend };
      break;
    }
    case RichOpcode.MoveText: {
      const start = r.u64();
      const end = r.u64();
      const target = r.u64();
      const copy = r.flag();
      r.zero(3);
      if (start > end) fail("RICH_INVALID_RANGE");
      command = { type: "moveText", start, end, target, copy };
      break;
    }
    case RichOpcode.Search: case RichOpcode.Statistics: case RichOpcode.SearchRects: case RichOpcode.Replace:
    case RichOpcode.ReplaceAll: case RichOpcode.PropertiesRead: case RichOpcode.PropertiesApply:
      command = readFindCommand(opcode, r); break;
    case RichOpcode.FormatState: case RichOpcode.ApplyCharFormat: case RichOpcode.ApplyParaFormat:
    case RichOpcode.ListStyles: case RichOpcode.ApplyStyle: case RichOpcode.PreviewStyle: case RichOpcode.ListFonts: case RichOpcode.FormatMarks:
    case RichOpcode.LoadFonts:
      command = readFormatCommand(opcode, r, { fail }); break;
    default: return fail("RICH_UNKNOWN_OPCODE");
  }
  r.finish();
  return command;
}

function withContext(context, work) {
  try { return work(); } catch (error) {
    if (error instanceof RichProtocolError && !error.context) throw new RichProtocolError(error.code, context);
    throw error;
  }
}

/** @returns {{docId:number, requestId:number, command:object}} */
export function decodeRichRequest(bytes) {
  const { opcode, context, body } = split(bytes);
  return withContext(context, () => {
    if (context.docId === 0) fail("RICH_INVALID_DOCUMENT_ID");
    return { ...context, command: parseCommand(opcode, body) };
  });
}

// Malformed caller input (wrong types, missing fields) becomes a typed protocol error.
function typed(work) {
  try { return work(); } catch (error) {
    if (error instanceof RichProtocolError) throw error;
    return fail("RICH_INVALID_BODY");
  }
}

export function encodeRichRequest(request) {
  return typed(() => encodeRequestUnchecked(request ?? {}));
}

export function encodeRichReply(reply) {
  return typed(() => encodeReplyUnchecked(reply ?? {}));
}

function encodeRequestUnchecked({ docId, requestId, command }) {
  const w = new Writer();
  let opcode;
  switch (command?.type) {
    case "open":
      w.u8(enumIndex(OPEN_KINDS, command.kind), enumIndex(PROFILES, command.profile), 0, 0);
      if (!(command.data instanceof Uint8Array)) fail("RICH_INVALID_BODY");
      w.u32(command.data.length); w.raw(command.data);
      opcode = RichOpcode.Open; break;
    case "close": opcode = RichOpcode.Close; break;
    case "undo": opcode = RichOpcode.Undo; break;
    case "redo": opcode = RichOpcode.Redo; break;
    case "insertText": validateEditText(command.text); w.text(command.text); opcode = RichOpcode.InsertText; break;
    case "replaceRange": w.u64(u64(command.start)); w.u64(u64(command.end)); validateEditText(command.text); w.text(command.text); opcode = RichOpcode.ReplaceRange; break;
    case "delete": w.u8(command.forward ? 1 : 0, enumIndex(UNITS, command.unit), 0, 0); opcode = RichOpcode.Delete; break;
    case "move": w.u8(enumIndex(RICH_DIRECTIONS, command.direction), command.extend ? 1 : 0, 0, 0); opcode = RichOpcode.Move; break;
    case "setSelection": w.u64(u64(command.anchor)); w.u64(u64(command.focus)); w.u8(command.upstream ? 1 : 0, 0, 0, 0); opcode = RichOpcode.SetSelection; break;
    case "hitTest": w.u32(u32(command.page)); w.u8(enumIndex(HIT_MODES, command.mode), 0, 0, 0); w.f64(command.x); w.f64(command.y); opcode = RichOpcode.HitTest; break;
    case "textWindow": w.u64(u64(command.start)); w.u32(u32(command.len)); opcode = RichOpcode.TextWindow; break;
    case "pages": w.u32(u32(command.first)); w.u32(u32(command.count)); opcode = RichOpcode.Pages; break;
    case "markSaved": w.u64(u64(command.revision)); opcode = RichOpcode.MarkSaved; break;
    case "layout": w.u32(u32(command.budget)); opcode = RichOpcode.Layout; break;
    case "resync": opcode = RichOpcode.Resync; break;
    case "accessibleWindow": w.u64(u64(command.start)); w.u64(u64(command.end)); opcode = RichOpcode.AccessibleWindow; break;
    case "save": w.u8(enumIndex(SAVE_FORMATS, command.format), 0, 0, 0); writeStamp(w, command); opcode = RichOpcode.Save; break;
    case "insertImage":
      if (!(command.bytes instanceof Uint8Array)) fail("RICH_INVALID_BODY");
      writeText16(w, command.alt ?? ""); w.raw(command.bytes); opcode = RichOpcode.InsertImage; break;
    case "makeEditableCopy": writeStamp(w, command); opcode = RichOpcode.MakeEditableCopy; break;
    case "selectionRects": w.u32(u32(command.first)); w.u32(u32(command.count)); opcode = RichOpcode.SelectionRects; break;
    case "imageBitmap": w.u64(u64(command.objectId)); opcode = RichOpcode.ImageBitmap; break;
    case "selectUnit": w.u64(u64(command.position)); w.u8(enumIndex(SELECTION_UNITS, command.unit), command.extend ? 1 : 0, 0, 0); opcode = RichOpcode.SelectUnit; break;
    case "moveText":
      w.u64(u64(command.start)); w.u64(u64(command.end)); w.u64(u64(command.target)); w.u8(command.copy ? 1 : 0, 0, 0, 0);
      opcode = RichOpcode.MoveText; break;
    case "search": case "statistics": case "searchRects": case "replace": case "replaceAll":
    case "propertiesRead": case "propertiesApply":
      opcode = writeFindCommand(w, command); break;
    case "formatState": case "applyCharFormat": case "applyParaFormat": case "listStyles": case "applyStyle": case "previewStyle":
    case "listFonts": case "formatMarks": case "loadFonts":
      opcode = writeFormatCommand(w, command); break;
    default: return fail("RICH_UNKNOWN_OPCODE");
  }
  const out = frame(opcode, docId, requestId, w.bytes());
  decodeRichRequest(out);
  return out;
}

function readPages(r, pageCount) {
  const n = r.u32();
  if (n > RICH_LIMITS.maxPagesPerResponse) fail("RICH_TOO_MANY_PAGES");
  const pages = [];
  let total = 0;
  for (let i = 0; i < n; i += 1) {
    const index = r.u32();
    const len = r.u32();
    if (index >= pageCount || (pages.length && pages[pages.length - 1].index >= index)) fail("RICH_PAGE_RANGE");
    if (len < RICH_LIMITS.minPageDisplayBytes || len > RICH_LIMITS.maxPageDisplayBytes) fail("RICH_INVALID_BODY");
    if (pages.length > 0 && total + len > RICH_LIMITS.maxResponsePageBytes) fail("RICH_FRAME_TOO_LARGE");
    total += len;
    const display = r.take(len);
    if (String.fromCharCode(...display.subarray(0, 4)) !== "ATDL") fail("RICH_INVALID_BODY");
    pages.push({ index, display });
  }
  return pages;
}

// Glyph atlas section of State and Pages replies (just before the pages):
// epoch u32, first u32, count u32, then count x (len u32 + ATDL of one contour).
// The entries are views into the frame, like the page display lists.
function readAtlas(r) {
  const epoch = r.u32(); const first = r.u32(); const count = r.u32();
  if (first + count > RICH_LIMITS.maxAtlasEntries) fail("RICH_INVALID_BODY");
  const entries = [];
  let total = 0;
  for (let i = 0; i < count; i += 1) {
    const len = r.u32();
    if (len < RICH_LIMITS.minPageDisplayBytes || len > RICH_LIMITS.maxAtlasEntryBytes) fail("RICH_INVALID_BODY");
    total += len;
    if (total > RICH_LIMITS.maxAtlasResponseBytes) fail("RICH_FRAME_TOO_LARGE");
    const entry = r.take(len);
    if (String.fromCharCode(...entry.subarray(0, 4)) !== "ATDL") fail("RICH_INVALID_BODY");
    entries.push(entry);
  }
  return { epoch, first, entries };
}

function writeAtlas(w, atlas) {
  if (!atlas || !Array.isArray(atlas.entries)) fail("RICH_INVALID_BODY");
  const epoch = u32(atlas.epoch); const first = u32(atlas.first);
  let total = 0;
  for (const entry of atlas.entries) {
    if (!(entry instanceof Uint8Array) || entry.length < RICH_LIMITS.minPageDisplayBytes || entry.length > RICH_LIMITS.maxAtlasEntryBytes) fail("RICH_INVALID_BODY");
    total += entry.length;
  }
  if (first + atlas.entries.length > RICH_LIMITS.maxAtlasEntries || total > RICH_LIMITS.maxAtlasResponseBytes) fail("RICH_INVALID_BODY");
  w.u32(epoch); w.u32(first); w.u32(atlas.entries.length);
  for (const entry of atlas.entries) { w.u32(entry.length); w.raw(entry); }
}

function writePages(w, pages) {
  w.u32(pages.length);
  let total = 0;
  for (const [n, page] of pages.entries()) {
    if (!(page.display instanceof Uint8Array)) fail("RICH_INVALID_BODY");
    if (page.display.length > RICH_LIMITS.maxPageDisplayBytes || (n > 0 && total + page.display.length > RICH_LIMITS.maxResponsePageBytes)) fail("RICH_FRAME_TOO_LARGE");
    total += page.display.length;
    w.u32(page.index); w.u32(page.display.length); w.raw(page.display);
  }
}

function parseState(r) {
  const revision = r.u64(); const anchor = r.u64(); const focus = r.u64(); const docLen = r.u64();
  const flags = r.u32();
  if ((flags & ~0xff) !== 0 || anchor > docLen || focus > docLen) fail("RICH_INVALID_BODY");
  const pageCount = r.u32();
  if (pageCount > RICH_LIMITS.maxPages) fail("RICH_TOO_MANY_PAGES");
  const caretPage = r.u32();
  const x = r.f64(); const y = r.f64(); const height = r.f64();
  let caret = null;
  if (caretPage === NO_CARET) {
    if (!Object.is(x, 0) || !Object.is(y, 0) || !Object.is(height, 0)) fail("RICH_INVALID_BODY");
  } else {
    if (caretPage >= pageCount || height < 0) fail("RICH_INVALID_BODY");
    caret = { page: caretPage, x, y, height };
  }
  const n = r.u32();
  if (n > RICH_LIMITS.maxInvalidations) fail("RICH_INVALID_BODY");
  const invalidated = [];
  for (let i = 0; i < n; i += 1) {
    const first = r.u32(); const end = r.u32();
    if (first >= end || end > RICH_LIMITS.maxPages || (invalidated.length && invalidated[invalidated.length - 1][1] >= first)) fail("RICH_PAGE_RANGE");
    invalidated.push([first, end]);
  }
  const atlas = readAtlas(r);
  const pages = readPages(r, pageCount);
  let window = null;
  if (flags & 0x10) {
    const start = r.u64();
    const text = r.text(RICH_LIMITS.maxWindowBytes, "RICH_WINDOW_TOO_LARGE");
    if (start + BigInt(encoder.encode(text).length) > docLen) fail("RICH_POSITION_OUT_OF_RANGE");
    window = { start, text };
  }
  return {
    revision, anchor, focus, focusUpstream: (flags & 0x20) !== 0, docLen,
    readOnly: (flags & 0x40) !== 0, hasRevisions: (flags & 0x80) !== 0,
    modified: (flags & 1) !== 0, canUndo: (flags & 2) !== 0, canRedo: (flags & 4) !== 0, layoutComplete: (flags & 8) !== 0,
    pageCount, caret, invalidated, atlas, pages, window,
  };
}

function writeState(w, s) {
  for (const v of [s.revision, s.anchor, s.focus, s.docLen]) w.u64(u64(v));
  w.u32((s.modified ? 1 : 0) | (s.canUndo ? 2 : 0) | (s.canRedo ? 4 : 0) | (s.layoutComplete ? 8 : 0) | (s.window ? 16 : 0) | (s.focusUpstream ? 32 : 0) | (s.readOnly ? 64 : 0) | (s.hasRevisions ? 128 : 0));
  w.u32(u32(s.pageCount));
  const c = s.caret ?? { page: NO_CARET, x: 0, y: 0, height: 0 };
  w.u32(c.page); w.f64(c.x); w.f64(c.y); w.f64(c.height);
  w.u32(s.invalidated.length);
  for (const [first, end] of s.invalidated) { w.u32(u32(first)); w.u32(u32(end)); }
  writeAtlas(w, s.atlas);
  writePages(w, s.pages);
  if (s.window) { w.u64(u64(s.window.start)); w.text(s.window.text); }
}

const OBJECT_BYTES = [0xef, 0xbf, 0xbc];
const INLINE_KINDS = ["object", "lineBreak", "pageBreak", "symbol"];
const ACCESSIBLE_ROLES = ["paragraph", "heading", "listItem", "tableCell", "opaque"];

function text16(r) {
  const len = r.u16();
  try { return decoder.decode(r.take(len)); } catch (error) {
    if (error instanceof RichProtocolError) throw error;
    return fail("RICH_INVALID_UTF8");
  }
}

function writeText16(w, value) {
  const b = encoder.encode(value);
  if (b.length > 0xffff) fail("RICH_INVALID_BODY");
  w.u16(b.length); w.raw(b);
}

/**
 * What the client supplies for the document properties (the engine has no clock):
 * `nowMs` (BigInt, milliseconds since the epoch), `user` and `version` (at most 255 characters each).
 */
function readStamp(r) {
  const nowMs = r.u64();
  const user = text16(r);
  const version = text16(r);
  if ([...user].length > RICH_LIMITS.maxStampChars || [...version].length > RICH_LIMITS.maxStampChars) fail("RICH_INVALID_TEXT");
  return { nowMs, user, version };
}

function writeStamp(w, { nowMs = 0n, user = "", version = "" }) {
  w.u64(u64(nowMs)); writeText16(w, user); writeText16(w, version);
}

/** `SaveData` (US-218): {revision (BigInt), bytes, notices: [{code, text}]}. `bytes` is a view into the frame. */
function readSaveData(r) {
  const revision = r.u64();
  const len = r.u32();
  if (len > RICH_LIMITS.maxSaveBytes) fail("RICH_LIMIT");
  const bytes = r.take(len);
  const count = r.u16();
  if (count > RICH_LIMITS.maxNotices) fail("RICH_INVALID_BODY");
  const notices = [];
  for (let i = 0; i < count; i += 1) {
    const code = r.u16();
    const text = text16(r);
    if (encoder.encode(text).length > RICH_LIMITS.maxNoticeBytes) fail("RICH_INVALID_BODY");
    notices.push({ code, text });
  }
  return { type: "saveData", revision, bytes, notices };
}

function writeSaveData(w, response) {
  if (!(response.bytes instanceof Uint8Array)) fail("RICH_INVALID_BODY");
  if (response.bytes.length > RICH_LIMITS.maxSaveBytes) fail("RICH_LIMIT");
  if (!Array.isArray(response.notices) || response.notices.length > RICH_LIMITS.maxNotices) fail("RICH_INVALID_BODY");
  w.u64(u64(response.revision)); w.u32(response.bytes.length); w.raw(response.bytes);
  w.u16(response.notices.length);
  for (const n of response.notices) {
    if (encoder.encode(n.text).length > RICH_LIMITS.maxNoticeBytes) fail("RICH_INVALID_BODY");
    w.u16(n.code); writeText16(w, n.text);
  }
}

/** `SelectionRects` (R7b-1): {revision (BigInt), rects: [{page, x, y, width, height}]} in points of the page. */
function readSelectionRects(r) {
  const revision = r.u64();
  const count = r.u32();
  if (count > RICH_LIMITS.maxSelectionRects) fail("RICH_INVALID_BODY");
  const rects = [];
  for (let i = 0; i < count; i += 1) {
    const page = r.u32();
    const x = r.f64(); const y = r.f64(); const width = r.f64(); const height = r.f64();
    if (page >= RICH_LIMITS.maxPages || width < 0 || height < 0) fail("RICH_INVALID_BODY");
    rects.push({ page, x, y, width, height });
  }
  return { type: "selectionRects", revision, rects };
}

function writeSelectionRects(w, response) {
  if (!Array.isArray(response.rects) || response.rects.length > RICH_LIMITS.maxSelectionRects) fail("RICH_INVALID_BODY");
  w.u64(u64(response.revision)); w.u32(response.rects.length);
  for (const rect of response.rects) {
    w.u32(u32(rect.page)); w.f64(rect.x); w.f64(rect.y); w.f64(rect.width); w.f64(rect.height);
  }
}

function checkBitmap(width, height, length) {
  const side = (v) => Number.isInteger(v) && v >= 1 && v <= RICH_LIMITS.maxBitmapSide;
  if (!side(width) || !side(height) || length !== width * height * 4) fail("RICH_INVALID_BODY");
}

/** `ImageBitmap` (R7b-1): {objectId (BigInt), width, height, rgba (Uint8Array, a view into the frame)}. */
function readImageBitmap(r) {
  const objectId = r.u64();
  const width = r.u32(); const height = r.u32();
  const len = r.u32();
  checkBitmap(width, height, len);
  return { type: "imageBitmap", objectId, width, height, rgba: r.take(len) };
}

function writeImageBitmap(w, response) {
  if (!(response.rgba instanceof Uint8Array)) fail("RICH_INVALID_BODY");
  checkBitmap(response.width, response.height, response.rgba.length);
  w.u64(u64(response.objectId)); w.u32(response.width); w.u32(response.height); w.u32(response.rgba.length); w.raw(response.rgba);
}

/** Items: {paraStart, paraEnd (BigInt), role, level, label, text, inlines: [{offset (BigInt), kind, value}]}, one inline per U+FFFC. */
function readAccessible(r) {
  const count = r.u32();
  if (count > RICH_LIMITS.maxAccessibleItems) fail("RICH_INVALID_BODY");
  const items = [];
  for (let i = 0; i < count; i += 1) {
    const paraStart = r.u64(); const paraEnd = r.u64();
    const roleCode = r.u8(); const level = r.u8();
    r.zero(2);
    const role = ACCESSIBLE_ROLES[roleCode];
    const levelOk = roleCode === 1 ? level >= 1 && level <= 9 : roleCode === 2 ? level <= 8 : level === 0;
    if (role === undefined || !levelOk) fail("RICH_INVALID_BODY");
    const label = text16(r);
    const len = r.u32();
    const raw = r.take(len);
    let text;
    try { text = decoder.decode(raw); } catch { return fail("RICH_INVALID_UTF8"); }
    const previous = items[items.length - 1];
    if (paraStart > paraEnd || paraEnd - paraStart !== BigInt(len) || (previous && paraStart <= previous.paraEnd)) fail("RICH_INVALID_BODY");
    const n = r.u16();
    const inlines = [];
    let floor = 0;
    for (let k = 0; k < n; k += 1) {
      const offset = r.u64();
      const kindCode = r.u8();
      r.zero(1);
      const value = text16(r);
      const kind = INLINE_KINDS[kindCode];
      if (kind === undefined || offset > BigInt(len)) fail("RICH_INVALID_BODY");
      const at = Number(offset);
      if (at < floor || at + 3 > len || OBJECT_BYTES.some((b, j) => raw[at + j] !== b)) fail("RICH_INVALID_BODY");
      floor = at + 3;
      inlines.push({ offset, kind, value });
    }
    let marks = 0;
    for (let at = 0; at + 2 < len; at += 1) {
      if (raw[at] === 0xef && raw[at + 1] === 0xbf && raw[at + 2] === 0xbc) { marks += 1; at += 2; }
    }
    if (marks !== n) fail("RICH_INVALID_BODY");
    items.push({ paraStart, paraEnd, role, level, label, text, inlines });
  }
  return items;
}

function writeAccessible(w, items) {
  if (!Array.isArray(items) || items.length > RICH_LIMITS.maxAccessibleItems) fail("RICH_INVALID_BODY");
  w.u32(items.length);
  for (const item of items) {
    w.u64(u64(item.paraStart)); w.u64(u64(item.paraEnd));
    w.u8(enumIndex(ACCESSIBLE_ROLES, item.role), item.level, 0, 0);
    writeText16(w, item.label);
    w.text(item.text);
    if (item.inlines.length > 0xffff) fail("RICH_INVALID_BODY");
    w.u16(item.inlines.length);
    for (const inline of item.inlines) { w.u64(u64(inline.offset)); w.u8(enumIndex(INLINE_KINDS, inline.kind), 0); writeText16(w, inline.value); }
  }
}

// ---- R7b-2: search, replace, statistics, properties -----------------------------------------------------
// Contract: docs/us-340-search.md ("Rich editor protocol"). Positions and revisions are BigInt, counts are numbers.

/** Editable properties, in wire order (the names of the properties dialog). */
export const PROPERTY_FIELDS = Object.freeze(["title", "subject", "author", "manager", "company", "category", "keywords", "comments",
  "hyperlinkBase", "contentStatus", "language"]);
const SEARCH_FLAGS = Object.freeze(["matchCase", "wholeWord", "ignoreAccents", "matchNbsp"]);
const STAT_FIELDS = Object.freeze(["words", "characters", "charactersWithSpaces", "paragraphs", "lines", "pages"]);
const PROPERTY_STATS = Object.freeze(["pages", "words", "characters", "charactersWithSpaces", "lines", "paragraphs"]);

function readSearchFlags(r) {
  const byte = r.u8();
  if ((byte & ~0x0f) !== 0) fail("RICH_INVALID_BODY");
  return Object.fromEntries(SEARCH_FLAGS.map((name, bit) => [name, (byte & (1 << bit)) !== 0]));
}

function searchFlagsByte(flags) {
  if (flags === undefined) return 0;
  if (flags === null || typeof flags !== "object") fail("RICH_INVALID_BODY");
  for (const key of Object.keys(flags)) if (!SEARCH_FLAGS.includes(key)) fail("RICH_INVALID_BODY");
  let byte = 0;
  SEARCH_FLAGS.forEach((name, bit) => {
    if (flags[name] !== undefined && typeof flags[name] !== "boolean") fail("RICH_INVALID_BODY");
    if (flags[name]) byte |= 1 << bit;
  });
  return byte;
}

function wellFormed(text) {
  if (typeof text !== "string") fail("RICH_INVALID_BODY");
  for (let i = 0; i < text.length; i += 1) {
    const c = text.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = text.charCodeAt(i + 1);
      if (!(d >= 0xdc00 && d <= 0xdfff)) fail("RICH_INVALID_UTF8");
      i += 1;
    } else if (c >= 0xdc00 && c <= 0xdfff) fail("RICH_INVALID_UTF8");
  }
}

function decodeUtf8(bytes) {
  try { return decoder.decode(bytes); } catch { return fail("RICH_INVALID_UTF8"); }
}

function readQuery(r, emptyAllowed) {
  const len = r.u32();
  if (len > RICH_LIMITS.maxQueryBytes) fail("RICH_QUERY_TOO_LARGE");
  const text = decodeUtf8(r.take(len));
  if (text.length === 0 && !emptyAllowed) fail("RICH_EMPTY_QUERY");
  return text;
}

function writeQuery(w, query) {
  wellFormed(query);
  const bytes = encoder.encode(query);
  if (bytes.length > RICH_LIMITS.maxQueryBytes) fail("RICH_QUERY_TOO_LARGE");
  w.u32(bytes.length); w.raw(bytes);
}

function readFindCommand(opcode, r) {
  switch (opcode) {
    case RichOpcode.Search: {
      const revision = r.u64();
      const flags = readSearchFlags(r);
      r.zero(3);
      return { type: "search", revision, flags, query: readQuery(r, true) };
    }
    case RichOpcode.Statistics: return { type: "statistics" };
    case RichOpcode.SearchRects: {
      const revision = r.u64();
      const firstPage = r.u32(); const pageCount = r.u32();
      if (pageCount === 0) fail("RICH_INVALID_BODY");
      if (pageCount > RICH_LIMITS.maxPagesPerResponse) fail("RICH_TOO_MANY_PAGES");
      if (firstPage >= RICH_LIMITS.maxPages) fail("RICH_PAGE_RANGE");
      const count = r.u32();
      if (count === 0 || count > RICH_LIMITS.maxHighlightRanges) fail("RICH_INVALID_BODY");
      const ranges = [];
      for (let i = 0; i < count; i += 1) {
        const start = r.u64(); const end = r.u64();
        if (start > end) fail("RICH_INVALID_RANGE");
        ranges.push({ start, end });
      }
      return { type: "searchRects", revision, firstPage, pageCount, ranges };
    }
    case RichOpcode.Replace: case RichOpcode.ReplaceAll: {
      const current = opcode === RichOpcode.Replace;
      const revision = r.u64();
      const flags = readSearchFlags(r);
      r.zero(3);
      let start; let end;
      if (current) {
        start = r.u64(); end = r.u64();
        if (start > end) fail("RICH_INVALID_RANGE");
      }
      const query = readQuery(r, false);
      if (query.includes("\ufffc")) fail("RICH_INVALID_TEXT");
      const replacement = r.text(RICH_LIMITS.maxTextBytes, "RICH_FRAME_TOO_LARGE");
      validateEditText(replacement);
      return current ? { type: "replace", revision, flags, start, end, query, replacement }
        : { type: "replaceAll", revision, flags, query, replacement };
    }
    case RichOpcode.PropertiesRead: return { type: "propertiesRead" };
    default: {
      const changes = {};
      for (const name of PROPERTY_FIELDS) {
        const present = r.flag();
        const len = r.u32();
        if (len > RICH_LIMITS.maxPropertyBytes) fail("RICH_FRAME_TOO_LARGE");
        const raw = r.take(len);
        if (present) changes[name] = decodeUtf8(raw);
        else if (len !== 0) fail("RICH_INVALID_BODY");
      }
      return { type: "propertiesApply", changes };
    }
  }
}

function writeFindCommand(w, command) {
  switch (command.type) {
    case "search":
      w.u64(u64(command.revision)); w.u8(searchFlagsByte(command.flags), 0, 0, 0); writeQuery(w, command.query);
      return RichOpcode.Search;
    case "statistics": return RichOpcode.Statistics;
    case "searchRects":
      w.u64(u64(command.revision)); w.u32(u32(command.firstPage)); w.u32(u32(command.pageCount));
      if (!Array.isArray(command.ranges)) fail("RICH_INVALID_BODY");
      w.u32(command.ranges.length);
      for (const range of command.ranges) { w.u64(u64(range.start)); w.u64(u64(range.end)); }
      return RichOpcode.SearchRects;
    case "replace": case "replaceAll": {
      const current = command.type === "replace";
      w.u64(u64(command.revision)); w.u8(searchFlagsByte(command.flags), 0, 0, 0);
      if (current) { w.u64(u64(command.start)); w.u64(u64(command.end)); }
      writeQuery(w, command.query);
      validateEditText(command.replacement); w.text(command.replacement);
      return current ? RichOpcode.Replace : RichOpcode.ReplaceAll;
    }
    case "propertiesRead": return RichOpcode.PropertiesRead;
    default: {
      const changes = command.changes ?? {};
      for (const name of Object.keys(changes)) if (!PROPERTY_FIELDS.includes(name)) fail("RICH_INVALID_BODY");
      for (const name of PROPERTY_FIELDS) {
        const value = changes[name];
        if (value !== undefined) wellFormed(value);
        const bytes = encoder.encode(value ?? "");
        w.u8(value === undefined ? 0 : 1); w.u32(bytes.length); w.raw(bytes);
      }
      return RichOpcode.PropertiesApply;
    }
  }
}

function readCounts(r) {
  const counts = Object.fromEntries(STAT_FIELDS.map((name) => [name, r.u32()]));
  if (counts.pages > RICH_LIMITS.maxPages) fail("RICH_TOO_MANY_PAGES");
  return counts;
}

function writeCounts(w, counts) {
  if (counts === null || typeof counts !== "object") fail("RICH_INVALID_BODY");
  for (const name of STAT_FIELDS) w.u32(u32(counts[name]));
}

function readPropertyText(r) { return r.text(RICH_LIMITS.maxPropertyBytes, "RICH_FRAME_TOO_LARGE"); }

function readOptionalText(r) {
  const present = r.flag();
  const text = readPropertyText(r);
  if (!present && text.length > 0) fail("RICH_INVALID_BODY");
  return present ? text : null;
}

function readOptionalNumber(r) {
  const present = r.flag();
  const value = r.u64();
  if (!present && value !== 0n) fail("RICH_INVALID_BODY");
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) fail("RICH_INVALID_BODY");
  return present ? Number(value) : null;
}

function writeOptionalText(w, value) {
  if (value !== null && value !== undefined) wellFormed(value);
  const bytes = encoder.encode(value ?? "");
  if (bytes.length > RICH_LIMITS.maxPropertyBytes) fail("RICH_FRAME_TOO_LARGE");
  w.u8(value === null || value === undefined ? 0 : 1); w.u32(bytes.length); w.raw(bytes);
}

function writeOptionalNumber(w, value) {
  w.u8(value === null || value === undefined ? 0 : 1);
  w.u64(value === null || value === undefined ? 0n : BigInt(value));
}

/** Snapshot of the properties: the shape of the properties dialog (without `kind`). */
function readSnapshot(r) {
  const editable = {}; const limits = {};
  for (const name of PROPERTY_FIELDS) editable[name] = readPropertyText(r);
  for (const name of PROPERTY_FIELDS) limits[name] = r.u32();
  const dates = [];
  for (let i = 0; i < 3; i += 1) {
    const kind = r.u8();
    const text = readPropertyText(r);
    if (kind === 0 && text.length === 0) dates.push(null);
    else if (kind === 1 || kind === 2) dates.push({ valid: kind === 1, text });
    else fail("RICH_INVALID_BODY");
  }
  const [lastModifiedBy, revision, application, appVersion, template] = [0, 1, 2, 3, 4].map(() => readOptionalText(r));
  const totalTime = readOptionalNumber(r);
  const statistics = Object.fromEntries(PROPERTY_STATS.map((name) => [name, readOptionalNumber(r)]));
  const noticeCount = r.u32();
  return { editable, limits, created: dates[0], modified: dates[1], lastPrinted: dates[2], lastModifiedBy, revision, application, appVersion, template, totalTime, statistics, noticeCount };
}

function writeSnapshot(w, s) {
  for (const name of PROPERTY_FIELDS) {
    wellFormed(s.editable[name]);
    const bytes = encoder.encode(s.editable[name]);
    if (bytes.length > RICH_LIMITS.maxPropertyBytes) fail("RICH_FRAME_TOO_LARGE");
    w.u32(bytes.length); w.raw(bytes);
  }
  for (const name of PROPERTY_FIELDS) w.u32(u32(s.limits[name]));
  for (const date of [s.created, s.modified, s.lastPrinted]) {
    if (date === null || date === undefined) { w.u8(0); w.u32(0); continue; }
    wellFormed(date.text);
    const bytes = encoder.encode(date.text);
    w.u8(date.valid ? 1 : 2); w.u32(bytes.length); w.raw(bytes);
  }
  for (const value of [s.lastModifiedBy, s.revision, s.application, s.appVersion, s.template]) writeOptionalText(w, value);
  writeOptionalNumber(w, s.totalTime);
  for (const name of PROPERTY_STATS) writeOptionalNumber(w, s.statistics[name]);
  w.u32(u32(s.noticeCount));
}

function readFindReply(opcode, r) {
  switch (opcode) {
    case RichReplyOpcode.SearchResult: {
      const revision = r.u64(); const total = r.u32();
      const flags = r.u8();
      r.zero(3);
      if ((flags & ~3) !== 0) fail("RICH_INVALID_BODY");
      const count = r.u32();
      if (count > RICH_LIMITS.maxSearchItems || count > total) fail("RICH_INVALID_BODY");
      const items = [];
      for (let i = 0; i < count; i += 1) {
        const start = r.u64(); const end = r.u64();
        const len = r.u32(); const from = r.u32(); const to = r.u32();
        const eflags = r.u8();
        r.zero(3);
        if (len > RICH_LIMITS.maxExcerptBytes || (eflags & ~3) !== 0) fail("RICH_INVALID_BODY");
        const raw = r.take(len);
        const text = decodeUtf8(raw);
        const boundary = (at) => at === len || (raw[at] & 0xc0) !== 0x80;
        const previous = items[items.length - 1];
        if (start >= end || !(from < to && to <= len && boundary(from) && boundary(to)) || (previous && start < previous.end)) fail("RICH_INVALID_BODY");
        items.push({ start, end, excerpt: { text, start: from, end: to, truncatedBefore: (eflags & 1) !== 0, truncatedAfter: (eflags & 2) !== 0 } });
      }
      return { type: "searchResult", revision, total, truncated: (flags & 1) !== 0, totalCapped: (flags & 2) !== 0, items };
    }
    case RichReplyOpcode.Statistics: {
      const revision = r.u64(); const anchor = r.u64(); const focus = r.u64();
      const layoutComplete = r.flag();
      r.zero(3);
      return { type: "statistics", revision, anchor, focus, layoutComplete, document: readCounts(r), selection: readCounts(r) };
    }
    case RichReplyOpcode.SearchRects: {
      const revision = r.u64(); const count = r.u32();
      if (count > RICH_LIMITS.maxHighlightRects) fail("RICH_INVALID_BODY");
      const rects = [];
      for (let i = 0; i < count; i += 1) {
        const rect = { item: r.u32(), page: r.u32(), x: r.f64(), y: r.f64(), width: r.f64(), height: r.f64() };
        const previous = rects[rects.length - 1];
        if (rect.page >= RICH_LIMITS.maxPages || rect.width < 0 || rect.height < 0 || (previous && rect.item < previous.item)) fail("RICH_INVALID_BODY");
        rects.push(rect);
      }
      return { type: "searchRects", revision, rects };
    }
    case RichReplyOpcode.Replaced: {
      const count = r.u32();
      return { type: "replaced", count, state: { type: "state", ...parseState(r) } };
    }
    case RichReplyOpcode.Properties: return { type: "properties", revision: r.u64(), snapshot: readSnapshot(r) };
    default: {
      const status = r.u8();
      if (status > 1) fail("RICH_INVALID_BODY");
      r.zero(1);
      const changed = r.u16(); const field = r.u8();
      r.zero(3);
      const max = r.u32(); const length = r.u32();
      const consistent = status === 0 ? field === 0 && max === 0 && length === 0
        : field < PROPERTY_FIELDS.length && length > max && changed === 0;
      if (!consistent || changed > PROPERTY_FIELDS.length) fail("RICH_INVALID_BODY");
      return { type: "propertiesApplied", status: status === 0 ? "applied" : "tooLong", changed,
        field: status === 0 ? null : PROPERTY_FIELDS[field], max, length, state: { type: "state", ...parseState(r) } };
    }
  }
}

function writeFindReply(w, response) {
  switch (response.type) {
    case "searchResult": {
      if (!Array.isArray(response.items) || response.items.length > RICH_LIMITS.maxSearchItems) fail("RICH_INVALID_BODY");
      w.u64(u64(response.revision)); w.u32(u32(response.total));
      w.u8((response.truncated ? 1 : 0) | (response.totalCapped ? 2 : 0), 0, 0, 0);
      w.u32(response.items.length);
      for (const item of response.items) {
        const e = item.excerpt;
        wellFormed(e.text);
        const bytes = encoder.encode(e.text);
        if (bytes.length > RICH_LIMITS.maxExcerptBytes) fail("RICH_INVALID_BODY");
        w.u64(u64(item.start)); w.u64(u64(item.end)); w.u32(bytes.length); w.u32(u32(e.start)); w.u32(u32(e.end));
        w.u8((e.truncatedBefore ? 1 : 0) | (e.truncatedAfter ? 2 : 0), 0, 0, 0); w.raw(bytes);
      }
      return RichReplyOpcode.SearchResult;
    }
    case "statistics":
      w.u64(u64(response.revision)); w.u64(u64(response.anchor)); w.u64(u64(response.focus));
      w.u8(response.layoutComplete ? 1 : 0, 0, 0, 0);
      writeCounts(w, response.document); writeCounts(w, response.selection);
      return RichReplyOpcode.Statistics;
    case "searchRects":
      if (!Array.isArray(response.rects) || response.rects.length > RICH_LIMITS.maxHighlightRects) fail("RICH_INVALID_BODY");
      w.u64(u64(response.revision)); w.u32(response.rects.length);
      for (const rect of response.rects) { w.u32(u32(rect.item)); w.u32(u32(rect.page)); w.f64(rect.x); w.f64(rect.y); w.f64(rect.width); w.f64(rect.height); }
      return RichReplyOpcode.SearchRects;
    case "replaced": w.u32(u32(response.count)); writeState(w, response.state); return RichReplyOpcode.Replaced;
    case "properties": w.u64(u64(response.revision)); writeSnapshot(w, response.snapshot); return RichReplyOpcode.Properties;
    default: {
      const tooLong = response.status === "tooLong";
      w.u8(tooLong ? 1 : 0, 0); w.u16(u32(response.changed));
      w.u8(tooLong ? enumIndex(PROPERTY_FIELDS, response.field) : 0, 0, 0, 0);
      w.u32(u32(response.max)); w.u32(u32(response.length));
      writeState(w, response.state);
      return RichReplyOpcode.PropertiesApplied;
    }
  }
}

/** @returns {{docId:number, requestId:number, response:object}} */
export function decodeRichReply(bytes) {
  const { opcode, context, body } = split(bytes);
  return withContext(context, () => {
    if (context.docId === 0) fail("RICH_INVALID_DOCUMENT_ID");
    const r = new Reader(body);
    let response;
    switch (opcode) {
      case RichReplyOpcode.Error: {
        const code = ERROR_NAMES.get(r.u16());
        if (!code) fail("RICH_INVALID_BODY");
        r.zero(2);
        response = { type: "error", code };
        break;
      }
      case RichReplyOpcode.State: response = { type: "state", ...parseState(r) }; break;
      case RichReplyOpcode.TextWindow: {
        const revision = r.u64(); const start = r.u64(); const docLen = r.u64();
        const text = r.text(RICH_LIMITS.maxWindowBytes, "RICH_WINDOW_TOO_LARGE");
        if (start + BigInt(encoder.encode(text).length) > docLen) fail("RICH_POSITION_OUT_OF_RANGE");
        response = { type: "textWindow", revision, start, docLen, text };
        break;
      }
      case RichReplyOpcode.Pages: {
        const revision = r.u64(); const pageCount = r.u32();
        if (pageCount > RICH_LIMITS.maxPages) fail("RICH_TOO_MANY_PAGES");
        const atlas = readAtlas(r);
        response = { type: "pages", revision, pageCount, atlas, pages: readPages(r, pageCount) };
        break;
      }
      case RichReplyOpcode.HitResult: {
        const revision = r.u64(); const page = r.u32(); const position = r.u64();
        const upstream = r.flag();
        r.zero(3);
        response = { type: "hitResult", revision, page, position, upstream };
        break;
      }
      case RichReplyOpcode.Closed: response = { type: "closed" }; break;
      case RichReplyOpcode.AccessibleWindow: {
        const revision = r.u64();
        response = { type: "accessibleWindow", revision, items: readAccessible(r) };
        break;
      }
      case RichReplyOpcode.SaveData: response = readSaveData(r); break;
      case RichReplyOpcode.SelectionRects: response = readSelectionRects(r); break;
      case RichReplyOpcode.ImageBitmap: response = readImageBitmap(r); break;
      case RichReplyOpcode.SearchResult: case RichReplyOpcode.Statistics: case RichReplyOpcode.SearchRects:
      case RichReplyOpcode.Replaced: case RichReplyOpcode.Properties: case RichReplyOpcode.PropertiesApplied:
        response = readFindReply(opcode, r); break;
      case RichReplyOpcode.FormatState: case RichReplyOpcode.StyleList: case RichReplyOpcode.FontList: case RichReplyOpcode.FormatMarks:
        response = readFormatReply(opcode, r, { fail }); break;
      default: return fail("RICH_UNKNOWN_OPCODE");
    }
    r.finish();
    return { ...context, response };
  });
}

function encodeReplyUnchecked({ docId, requestId, response }) {
  const w = new Writer();
  let opcode;
  switch (response?.type) {
    case "error": {
      const code = typeof response.code === "string" ? RichErrorCode[response.code] : undefined;
      if (!code) fail("RICH_INVALID_BODY");
      w.u16(code); w.u16(0); opcode = RichReplyOpcode.Error; break;
    }
    case "state": writeState(w, response); opcode = RichReplyOpcode.State; break;
    case "textWindow": w.u64(u64(response.revision)); w.u64(u64(response.start)); w.u64(u64(response.docLen)); w.text(response.text); opcode = RichReplyOpcode.TextWindow; break;
    case "pages": w.u64(u64(response.revision)); w.u32(u32(response.pageCount)); writeAtlas(w, response.atlas); writePages(w, response.pages); opcode = RichReplyOpcode.Pages; break;
    case "hitResult": w.u64(u64(response.revision)); w.u32(u32(response.page)); w.u64(u64(response.position)); w.u8(response.upstream ? 1 : 0, 0, 0, 0); opcode = RichReplyOpcode.HitResult; break;
    case "closed": opcode = RichReplyOpcode.Closed; break;
    case "accessibleWindow": w.u64(u64(response.revision)); writeAccessible(w, response.items); opcode = RichReplyOpcode.AccessibleWindow; break;
    case "saveData": writeSaveData(w, response); opcode = RichReplyOpcode.SaveData; break;
    case "selectionRects": writeSelectionRects(w, response); opcode = RichReplyOpcode.SelectionRects; break;
    case "imageBitmap": writeImageBitmap(w, response); opcode = RichReplyOpcode.ImageBitmap; break;
    case "searchResult": case "statistics": case "searchRects": case "replaced": case "properties": case "propertiesApplied":
      opcode = writeFindReply(w, response); break;
    case "formatState": case "styleList": case "fontList": case "formatMarks": opcode = writeFormatReply(w, response); break;
    default: return fail("RICH_UNKNOWN_OPCODE");
  }
  const out = frame(opcode, docId, requestId, w.bytes());
  decodeRichReply(out);
  return out;
}
