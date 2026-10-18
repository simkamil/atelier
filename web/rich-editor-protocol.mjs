// SPDX-License-Identifier: AGPL-3.0-or-later
// Reference codec of the ATED v2 rich-editor envelope (US-203). Mirrors
// crates/core-wasm/src/rich_protocol.rs; specification in
// docs/us-203-rich-editor-protocol.md. Envelope and message types only: the
// page cache and editor integration belong to US-203 part 2 (R4).
// Positions, revisions and byte lengths that can exceed 2^32 are BigInt.
// Decoders do not copy: `data`, page `display` are views (subarray) into the
// frame passed in, which the caller must not mutate while using them.

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
});
export const NO_CARET = 0xffffffff;

export const RichOpcode = Object.freeze({
  Open: 0x0101, Close: 0x0102, InsertText: 0x0103, ReplaceRange: 0x0104, Delete: 0x0105,
  Move: 0x0106, SetSelection: 0x0107, HitTest: 0x0108, Undo: 0x0109, Redo: 0x010a,
  TextWindow: 0x010b, Pages: 0x010c, MarkSaved: 0x010d, Layout: 0x010e, AccessibleWindow: 0x010f,
  // US-218 (R3b)
  Save: 0x0110, InsertImage: 0x0111, MakeEditableCopy: 0x0112,
});
export const RichReplyOpcode = Object.freeze({
  Error: 0x8000, State: 0x8100, TextWindow: 0x8101, Pages: 0x8102, HitResult: 0x8103, Closed: 0x8104,
  AccessibleWindow: 0x8105,
  SaveData: 0x8106,
});

export const RichErrorCode = Object.freeze(Object.assign(Object.create(null), {
  RICH_INVALID_ENVELOPE: 1, RICH_UNSUPPORTED_VERSION: 2, RICH_UNKNOWN_OPCODE: 3, RICH_INVALID_BODY: 4,
  RICH_INVALID_DOCUMENT_ID: 5, RICH_UNKNOWN_DOCUMENT: 6, RICH_POSITION_OUT_OF_RANGE: 7,
  RICH_POSITION_NOT_BOUNDARY: 8, RICH_INVALID_RANGE: 9, RICH_INVALID_UTF8: 10, RICH_INVALID_TEXT: 11,
  RICH_FRAME_TOO_LARGE: 12, RICH_WINDOW_TOO_LARGE: 13, RICH_PAGE_RANGE: 14, RICH_TOO_MANY_PAGES: 15,
  RICH_STALE_REVISION: 16, RICH_OPEN_FAILED: 17, RICH_INTERNAL: 18, RICH_LIMIT: 19, RICH_NOT_READY: 20,
  // US-218 (R3b)
  RICH_READ_ONLY: 21, RICH_SAVE_FAILED: 22, RICH_IMAGE_INVALID: 23, RICH_REVISIONS_UNSUPPORTED: 24,
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
    case "accessibleWindow": w.u64(u64(command.start)); w.u64(u64(command.end)); opcode = RichOpcode.AccessibleWindow; break;
    case "save": w.u8(enumIndex(SAVE_FORMATS, command.format), 0, 0, 0); writeStamp(w, command); opcode = RichOpcode.Save; break;
    case "insertImage":
      if (!(command.bytes instanceof Uint8Array)) fail("RICH_INVALID_BODY");
      writeText16(w, command.alt ?? ""); w.raw(command.bytes); opcode = RichOpcode.InsertImage; break;
    case "makeEditableCopy": writeStamp(w, command); opcode = RichOpcode.MakeEditableCopy; break;
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
    pageCount, caret, invalidated, pages, window,
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
        response = { type: "pages", revision, pageCount, pages: readPages(r, pageCount) };
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
    case "pages": w.u64(u64(response.revision)); w.u32(u32(response.pageCount)); writePages(w, response.pages); opcode = RichReplyOpcode.Pages; break;
    case "hitResult": w.u64(u64(response.revision)); w.u32(u32(response.page)); w.u64(u64(response.position)); w.u8(response.upstream ? 1 : 0, 0, 0, 0); opcode = RichReplyOpcode.HitResult; break;
    case "closed": opcode = RichReplyOpcode.Closed; break;
    case "accessibleWindow": w.u64(u64(response.revision)); writeAccessible(w, response.items); opcode = RichReplyOpcode.AccessibleWindow; break;
    case "saveData": writeSaveData(w, response); opcode = RichReplyOpcode.SaveData; break;
    default: return fail("RICH_UNKNOWN_OPCODE");
  }
  const out = frame(opcode, docId, requestId, w.bytes());
  decodeRichReply(out);
  return out;
}
