// SPDX-License-Identifier: AGPL-3.0-or-later
// Codec and client of the formatting family of the ATED v2 rich-editor protocol (F0): FormatState,
// ApplyCharFormat, ApplyParaFormat, ListStyles, ApplyStyle, PreviewStyle and the replies FormatState
// and StyleList. Mirrors crates/core-wasm/src/rich_protocol/format.rs; contract, byte layouts, decoded
// shapes and rules: docs/us-240-format-protocol.md. web/rich-editor-protocol.mjs only holds the hooks
// (opcode tables, dispatch), so US-240, US-243 and US-250 never touch the codec: they write
// web/format-char.mjs, web/format-para.mjs and web/styles-pane.mjs on top of `createFormatClient`.
//
// Decoded shapes (BigInt for revisions, numbers for everything else):
//   commands  {type:"formatState"}
//             {type:"applyCharFormat", baseRevision: 9n, scope:"wordOrTyping", edits:[{attr:"size", op:"set", value:24}]}
//             {type:"applyParaFormat", baseRevision: 9n, edits:[{attr:"alignment", op:"set", value:"center"}]}
//             {type:"listStyles", filter:"paragraph", usedOnly:true}
//             {type:"applyStyle", baseRevision: 9n, target:"paragraph", id:"Heading1"}
//             {type:"previewStyle", target:"paragraph", show:true, id:"Heading1"}
//   replies   {type:"formatState", revision, collapsed, readOnly, partial, typing, plain, chars:{...}, paragraph:{...}}
//             {type:"styleList", revision, styles:[{id, basedOn, target, builtIn, used, current, isDefault, headingLevel, sample}]}
// A state field is {state:"value", value} | {state:"mixed"} | {state:"unset"}.

export const NO_REVISION_GUARD = 0xffffffffffffffffn;
export const FORMAT_LIMITS = Object.freeze({
  maxEdits: 16, maxNameBytes: 255, maxStyleList: 4096, maxFontList: 512, maxFormatMarks: 16384, maxPagesPerMarks: 16, minHalfPoints: 2, maxHalfPoints: 3276, maxTwips: 31680,
  maxLoadFonts: 8, maxFontFileBytes: 1024 * 1024, minBorderEighths: 2, maxBorderEighths: 96, maxBorderSpace: 31, maxStateRuns: 20000,
});

export const FormatOpcode = Object.freeze({
  FormatState: 0x0150, ApplyCharFormat: 0x0151, ApplyParaFormat: 0x0152, ListStyles: 0x0153, ApplyStyle: 0x0154, PreviewStyle: 0x0155,
  ListFonts: 0x0156, FormatMarks: 0x0157,
  // US-240 (T240-F): font files fetched on demand, top of the reserved range 0x0158 to 0x016F.
  LoadFonts: 0x016F,
});
export const FormatReplyOpcode = Object.freeze({ FormatState: 0x8130, StyleList: 0x8131, FontList: 0x8132, FormatMarks: 0x8133 });
export const FormatErrorCode = Object.freeze({ RICH_FORMAT_NOT_IMPLEMENTED: 60, RICH_UNKNOWN_STYLE: 61, RICH_FORMAT_PLAIN_TEXT: 62, RICH_DUPLICATE_COMMAND: 63 });

export const CHAR_ATTRS = Object.freeze({ font: 1, size: 2, bold: 3, italic: 4, underline: 5, strike: 6, verticalAlign: 7, color: 8, highlight: 9 });
export const PARA_ATTRS = Object.freeze({
  alignment: 1, indentLeft: 2, indentRight: 3, firstLine: 4, spaceBefore: 5, spaceAfter: 6, lineSpacing: 7, shading: 8, borders: 9,
});
export const EDIT_OPS = Object.freeze({ set: 1, clear: 2, toggle: 3, stepUp: 4, stepDown: 5, stepUpPoint: 6, stepDownPoint: 7 });
export const UNDERLINES = Object.freeze(["none", "single", "double", "thick", "dotted", "dashed", "wave"]);
export const VERTICAL_ALIGNS = Object.freeze(["baseline", "superscript", "subscript"]);
export const ALIGNMENTS = Object.freeze(["left", "center", "right", "justify"]);
export const LINE_RULES = Object.freeze(["auto", "exact", "atLeast"]);
export const BORDER_STYLES = Object.freeze(["single", "double", "dotted", "dashed"]);
export const BORDER_SIDES = Object.freeze(["top", "left", "bottom", "right", "between", "bar"]);
export const FORMAT_SCOPES = Object.freeze(["wordOrTyping", "selectionOnly"]);
export const STYLE_FILTERS = Object.freeze(["all", "paragraph", "character"]);
export const STYLE_TARGETS = Object.freeze(["paragraph", "character"]);
/** Formatting marks, in wire order. */
export const MARK_KINDS = Object.freeze(["paragraphEnd", "space", "tab", "noBreakSpace", "lineBreak", "pageBreak", "columnBreak"]);

/** Word's sizes for Ctrl+Maj+> and Ctrl+Maj+<, in half-points (mirror of `SIZE_STEPS`; the engine computes the steps). */
export const SIZE_STEPS = Object.freeze([16, 18, 20, 21, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 72, 96, 144]);

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const MAX_U64 = (1n << 64n) - 1n;

const nameOf = (table, value, fail) => { const name = table[value]; if (name === undefined) fail("RICH_INVALID_BODY"); return name; };
const indexOf = (table, name) => { const at = table.indexOf(name); if (at < 0) throw new TypeError(`unknown name ${name}`); return at; };
const keyOf = (table, value) => { const entry = Object.entries(table).find(([, v]) => v === value); return entry?.[0]; };

function guard(r) { return r.u64(); }
function writeGuard(w, value) {
  if (typeof value !== "bigint" || value < 0n || value > MAX_U64) throw new TypeError("revision guard must be a BigInt");
  w.u64(value);
}

function writeCommandId(w, value) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) throw new TypeError("command identifier");
  w.u32(value);
}

function readName(r, fail, allowEmpty) {
  const len = r.u16();
  if (len > FORMAT_LIMITS.maxNameBytes) fail("RICH_INVALID_BODY");
  let text;
  try { text = decoder.decode(r.take(len)); } catch (error) {
    if (error?.name === "RichProtocolError") throw error;
    return fail("RICH_INVALID_UTF8");
  }
  if (!allowEmpty && text.length === 0) fail("RICH_INVALID_BODY");
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(text)) fail("RICH_INVALID_BODY");
  return text;
}
function writeName(w, value) {
  if (typeof value !== "string") throw new TypeError("a name is a string");
  const bytes = encoder.encode(value);
  if (bytes.length > FORMAT_LIMITS.maxNameBytes) throw new TypeError("name too long");
  w.u16(bytes.length); w.raw(bytes);
}
function readRgb(r) { return { r: r.u8(), g: r.u8(), b: r.u8() }; }
function writeRgb(w, c) {
  for (const k of ["r", "g", "b"]) if (!Number.isInteger(c?.[k]) || c[k] < 0 || c[k] > 255) throw new TypeError("colour component");
  w.u8(c.r, c.g, c.b);
}
function readChoice(r, count, fail) { const v = r.u8(); if (v >= count) fail("RICH_INVALID_BODY"); return v; }
function i32(r) { return r.u32() | 0; }
function signedTwips(r, fail) { const v = i32(r); if (v < -FORMAT_LIMITS.maxTwips || v > FORMAT_LIMITS.maxTwips) fail("RICH_INVALID_BODY"); return v; }
function lengthTwips(r, fail) { const v = r.u32(); if (v > FORMAT_LIMITS.maxTwips) fail("RICH_INVALID_BODY"); return v; }
function writeSigned(w, v) {
  if (!Number.isInteger(v) || v < -FORMAT_LIMITS.maxTwips || v > FORMAT_LIMITS.maxTwips) throw new TypeError("twips");
  w.u32(v >>> 0);
}
function writeLength(w, v) {
  if (!Number.isInteger(v) || v < 0 || v > FORMAT_LIMITS.maxTwips) throw new TypeError("twips");
  w.u32(v);
}

// ------------------------------------------------------------------ character edits

/** Whether an operation on a character attribute carries a value (`undefined` when the pair is not allowed). */
function charValueFollows(attr, op) {
  if (op === "set") return true;
  if (op === "clear") return false;
  if (op === "toggle") return attr === "bold" || attr === "italic" || attr === "strike" ? false : (attr === "underline" || attr === "verticalAlign" ? true : undefined);
  if (op === "stepUp" || op === "stepDown" || op === "stepUpPoint" || op === "stepDownPoint") return attr === "size" ? false : undefined;
  return undefined;
}

function readCharValue(r, attr, op, fail) {
  switch (attr) {
    case "font": return readName(r, fail, false);
    case "size": {
      const v = r.u16();
      if (v < FORMAT_LIMITS.minHalfPoints || v > FORMAT_LIMITS.maxHalfPoints) fail("RICH_INVALID_BODY");
      return v;
    }
    case "bold": case "italic": case "strike": return r.flag();
    case "underline": case "verticalAlign": {
      const table = attr === "underline" ? UNDERLINES : VERTICAL_ALIGNS;
      const v = readChoice(r, table.length, fail);
      if (op === "toggle" && v === 0) fail("RICH_INVALID_BODY");
      return table[v];
    }
    default: return readRgb(r);
  }
}

function writeCharValue(w, attr, value) {
  switch (attr) {
    case "font": writeName(w, value); if (value === "") throw new TypeError("empty font"); break;
    case "size":
      if (!Number.isInteger(value) || value < FORMAT_LIMITS.minHalfPoints || value > FORMAT_LIMITS.maxHalfPoints) throw new TypeError("size");
      w.u16(value); break;
    case "bold": case "italic": case "strike":
      if (typeof value !== "boolean") throw new TypeError("flag");
      w.u8(value ? 1 : 0); break;
    case "underline": w.u8(indexOf(UNDERLINES, value)); break;
    case "verticalAlign": w.u8(indexOf(VERTICAL_ALIGNS, value)); break;
    default: writeRgb(w, value);
  }
}

function readEdits(r, count, fail, attrs, followsFn, readValue) {
  if (count === 0 || count > FORMAT_LIMITS.maxEdits) fail("RICH_INVALID_BODY");
  const edits = [];
  for (let i = 0; i < count; i += 1) {
    const attr = keyOf(attrs, r.u8());
    const op = keyOf(EDIT_OPS, r.u8());
    if (attr === undefined || op === undefined) fail("RICH_INVALID_BODY");
    if (edits.some((e) => e.attr === attr)) fail("RICH_INVALID_BODY");
    const follows = followsFn(attr, op);
    if (follows === undefined) fail("RICH_INVALID_BODY");
    const edit = { attr, op };
    if (follows) edit.value = readValue(r, attr, op, fail);
    edits.push(edit);
  }
  return edits;
}

function writeEdits(w, edits, attrs, followsFn, writeValue) {
  if (!Array.isArray(edits) || edits.length === 0 || edits.length > FORMAT_LIMITS.maxEdits) throw new TypeError("edit count");
  for (const edit of edits) {
    const attr = attrs[edit?.attr]; const op = EDIT_OPS[edit?.op];
    if (attr === undefined || op === undefined) throw new TypeError("unknown attribute or operation");
    const follows = followsFn(edit.attr, edit.op);
    if (follows === undefined) throw new TypeError("operation not allowed on this attribute");
    if (follows !== (edit.value !== undefined)) throw new TypeError("value presence");
    w.u8(attr, op);
    if (follows) writeValue(w, edit.attr, edit.value);
  }
}

// ------------------------------------------------------------------ paragraph edits

function paraValueFollows(attr, op) {
  if (op === "set") return true;
  if (op === "clear") return false;
  if (op === "toggle") return attr === "spaceBefore" || attr === "spaceAfter" ? false : undefined;
  if (op === "stepUp" || op === "stepDown") return attr === "indentLeft" ? false : undefined;
  return undefined;
}

function readLine(r, fail) {
  const style = readChoice(r, BORDER_STYLES.length, fail);
  const eighths = r.u16();
  if (eighths < FORMAT_LIMITS.minBorderEighths || eighths > FORMAT_LIMITS.maxBorderEighths) fail("RICH_INVALID_BODY");
  const space = r.u8();
  if (space > FORMAT_LIMITS.maxBorderSpace) fail("RICH_INVALID_BODY");
  const mode = r.u8();
  let color = null;
  if (mode === 0) r.zero(3); else if (mode === 1) color = readRgb(r); else fail("RICH_INVALID_BODY");
  return { style: BORDER_STYLES[style], eighths, space, color };
}
function writeLine(w, line) {
  const eighths = line?.eighths; const space = line?.space;
  if (!Number.isInteger(eighths) || eighths < FORMAT_LIMITS.minBorderEighths || eighths > FORMAT_LIMITS.maxBorderEighths) throw new TypeError("border width");
  if (!Number.isInteger(space) || space < 0 || space > FORMAT_LIMITS.maxBorderSpace) throw new TypeError("border space");
  w.u8(indexOf(BORDER_STYLES, line.style)); w.u16(eighths); w.u8(space);
  if (line.color === null || line.color === undefined) w.u8(0, 0, 0, 0); else { w.u8(1); writeRgb(w, line.color); }
}
function readBorderEdit(r, fail) {
  const kind = r.u8();
  if (kind === 0) { r.zero(8); return "keep"; }
  if (kind === 1) { r.zero(8); return "none"; }
  if (kind === 2) return readLine(r, fail);
  return fail("RICH_INVALID_BODY");
}
function writeBorderEdit(w, edit) {
  if (edit === "keep") { w.raw(new Uint8Array(9)); return; }
  if (edit === "none") { w.u8(1); w.raw(new Uint8Array(8)); return; }
  w.u8(2); writeLine(w, edit);
}

function readParaValue(r, attr, _op, fail) {
  switch (attr) {
    case "alignment": return ALIGNMENTS[readChoice(r, ALIGNMENTS.length, fail)];
    case "indentLeft": case "indentRight": case "firstLine": return signedTwips(r, fail);
    case "spaceBefore": case "spaceAfter": return lengthTwips(r, fail);
    case "lineSpacing": {
      const rule = readChoice(r, LINE_RULES.length, fail);
      const value = r.u32();
      const ok = rule === 0 ? value >= 15 && value <= 2400 : value >= 1 && value <= FORMAT_LIMITS.maxTwips;
      if (!ok) fail("RICH_INVALID_BODY");
      return { rule: LINE_RULES[rule], value };
    }
    case "shading": return readRgb(r);
    default: {
      const sides = {};
      for (const side of BORDER_SIDES) sides[side] = readBorderEdit(r, fail);
      return sides;
    }
  }
}
function writeParaValue(w, attr, value) {
  switch (attr) {
    case "alignment": w.u8(indexOf(ALIGNMENTS, value)); break;
    case "indentLeft": case "indentRight": case "firstLine": writeSigned(w, value); break;
    case "spaceBefore": case "spaceAfter": writeLength(w, value); break;
    case "lineSpacing": {
      const rule = indexOf(LINE_RULES, value?.rule);
      const v = value.value;
      const ok = Number.isInteger(v) && (rule === 0 ? v >= 15 && v <= 2400 : v >= 1 && v <= FORMAT_LIMITS.maxTwips);
      if (!ok) throw new TypeError("line spacing value");
      w.u8(rule); w.u32(v); break;
    }
    case "shading": writeRgb(w, value); break;
    default:
      for (const side of BORDER_SIDES) writeBorderEdit(w, value?.[side] ?? (() => { throw new TypeError("border side"); })());
  }
}

// ------------------------------------------------------------------ commands

/** Decodes a formatting command; `ctx.fail(code)` throws the protocol error. */
export function readFormatCommand(opcode, r, ctx) {
  const { fail } = ctx;
  switch (opcode) {
    case FormatOpcode.FormatState: return { type: "formatState" };
    case FormatOpcode.ApplyCharFormat: {
      const baseRevision = guard(r);
      const commandId = r.u32();
      const scope = FORMAT_SCOPES[r.u8()];
      if (scope === undefined) fail("RICH_INVALID_BODY");
      r.zero(1);
      const edits = readEdits(r, r.u16(), fail, CHAR_ATTRS, charValueFollows, readCharValue);
      return { type: "applyCharFormat", baseRevision, commandId, scope, edits };
    }
    case FormatOpcode.ApplyParaFormat: {
      const baseRevision = guard(r);
      const commandId = r.u32();
      const count = r.u16();
      r.zero(2);
      const edits = readEdits(r, count, fail, PARA_ATTRS, paraValueFollows, readParaValue);
      return { type: "applyParaFormat", baseRevision, commandId, edits };
    }
    case FormatOpcode.ListStyles: {
      const filter = nameOf(STYLE_FILTERS, r.u8(), fail);
      const usedOnly = r.flag();
      r.zero(2);
      return { type: "listStyles", filter, usedOnly };
    }
    case FormatOpcode.ApplyStyle: {
      const baseRevision = guard(r);
      const commandId = r.u32();
      const target = nameOf(STYLE_TARGETS, r.u8(), fail);
      r.zero(3);
      const id = readName(r, fail, target === "character");
      return { type: "applyStyle", baseRevision, commandId, target, id };
    }
    case FormatOpcode.ListFonts: return { type: "listFonts" };
    case FormatOpcode.LoadFonts: {
      const count = r.u8();
      if (count === 0 || count > FORMAT_LIMITS.maxLoadFonts) fail("RICH_INVALID_BODY");
      const fonts = [];
      for (let i = 0; i < count; i += 1) {
        const length = r.u32();
        if (length === 0 || length > FORMAT_LIMITS.maxFontFileBytes) fail("RICH_INVALID_BODY");
        fonts.push(r.take(length).slice());
      }
      return { type: "loadFonts", fonts };
    }
    case FormatOpcode.FormatMarks: {
      const first = r.u32();
      const count = r.u32();
      if (count === 0) fail("RICH_INVALID_BODY");
      if (count > FORMAT_LIMITS.maxPagesPerMarks) fail("RICH_TOO_MANY_PAGES");
      if (first >= 1 << 20) fail("RICH_PAGE_RANGE");
      return { type: "formatMarks", first, count };
    }
    case FormatOpcode.PreviewStyle: {
      const target = nameOf(STYLE_TARGETS, r.u8(), fail);
      const action = r.u8();
      if (action > 1) fail("RICH_INVALID_BODY");
      r.zero(2);
      const show = action === 0;
      const id = readName(r, fail, !show);
      if (!show && id !== "") fail("RICH_INVALID_BODY");
      return { type: "previewStyle", target, show, id };
    }
    default: return fail("RICH_UNKNOWN_OPCODE");
  }
}

/** Encodes a formatting command body into `w`; returns the opcode. Malformed input throws (the caller types it). */
export function writeFormatCommand(w, command) {
  switch (command.type) {
    case "formatState": return FormatOpcode.FormatState;
    case "applyCharFormat":
      writeGuard(w, command.baseRevision); writeCommandId(w, command.commandId);
      w.u8(indexOf(FORMAT_SCOPES, command.scope), 0);
      w.u16(command.edits.length);
      writeEdits(w, command.edits, CHAR_ATTRS, charValueFollows, writeCharValue);
      return FormatOpcode.ApplyCharFormat;
    case "applyParaFormat":
      writeGuard(w, command.baseRevision); writeCommandId(w, command.commandId);
      w.u16(command.edits.length); w.u8(0, 0);
      writeEdits(w, command.edits, PARA_ATTRS, paraValueFollows, writeParaValue);
      return FormatOpcode.ApplyParaFormat;
    case "listStyles":
      w.u8(indexOf(STYLE_FILTERS, command.filter), command.usedOnly ? 1 : 0, 0, 0);
      return FormatOpcode.ListStyles;
    case "applyStyle":
      writeGuard(w, command.baseRevision); writeCommandId(w, command.commandId);
      w.u8(indexOf(STYLE_TARGETS, command.target), 0, 0, 0);
      writeName(w, command.id);
      return FormatOpcode.ApplyStyle;
    case "listFonts": return FormatOpcode.ListFonts;
    case "loadFonts": {
      const fonts = command.fonts;
      if (!Array.isArray(fonts) || fonts.length === 0 || fonts.length > FORMAT_LIMITS.maxLoadFonts) throw new TypeError("fonts");
      w.u8(fonts.length);
      for (const font of fonts) {
        if (!(font instanceof Uint8Array) || font.length === 0 || font.length > FORMAT_LIMITS.maxFontFileBytes) throw new TypeError("font file");
        w.u32(font.length); w.raw(font);
      }
      return FormatOpcode.LoadFonts;
    }
    case "formatMarks":
      for (const k of ["first", "count"]) if (!Number.isInteger(command[k]) || command[k] < 0 || command[k] > 0xffffffff) throw new TypeError(k);
      w.u32(command.first); w.u32(command.count);
      return FormatOpcode.FormatMarks;
    case "previewStyle":
      w.u8(indexOf(STYLE_TARGETS, command.target), command.show ? 0 : 1, 0, 0);
      writeName(w, command.id);
      return FormatOpcode.PreviewStyle;
    default: throw new TypeError("not a formatting command");
  }
}

export const FORMAT_COMMAND_TYPES = Object.freeze(["formatState", "applyCharFormat", "applyParaFormat", "listStyles", "applyStyle", "previewStyle", "listFonts", "loadFonts", "formatMarks"]);
export const FORMAT_REPLY_TYPES = Object.freeze(["formatState", "styleList", "fontList", "formatMarks"]);
/** Reply type the client expects for each command (`State` for the commands that change the document). */
export const FORMAT_REPLY_TYPE = Object.freeze({
  formatState: "formatState", applyCharFormat: "state", applyParaFormat: "state", listStyles: "styleList", applyStyle: "state", previewStyle: "state",
  listFonts: "fontList", formatMarks: "formatMarks", loadFonts: "state",
});

// ------------------------------------------------------------------ replies

const tri = (r, read, fail) => {
  const s = r.u8();
  if (s === 0) return { state: "mixed" };
  if (s === 1) return { state: "value", value: read(r, fail) };
  if (s === 2) return { state: "unset" };
  return fail("RICH_INVALID_BODY");
};
const writeTri = (w, field, write) => {
  if (field?.state === "mixed") { w.u8(0); return; }
  if (field?.state === "unset") { w.u8(2); return; }
  if (field?.state !== "value") throw new TypeError("state field");
  w.u8(1); write(w, field.value);
};
const flag = (r) => r.flag();
const writeFlag = (w, v) => { if (typeof v !== "boolean") throw new TypeError("flag"); w.u8(v ? 1 : 0); };

function readSize(r, fail) {
  const v = r.u16();
  if (v < FORMAT_LIMITS.minHalfPoints || v > FORMAT_LIMITS.maxHalfPoints) fail("RICH_INVALID_BODY");
  return v;
}

function readCharState(r, fail) {
  return {
    font: tri(r, (rr) => ({ requested: readName(rr, fail, false), used: readName(rr, fail, false), substituted: rr.flag() }), fail),
    size: tri(r, readSize, fail),
    bold: tri(r, flag, fail),
    italic: tri(r, flag, fail),
    underline: tri(r, (rr) => UNDERLINES[readChoice(rr, UNDERLINES.length, fail)], fail),
    strike: tri(r, flag, fail),
    verticalAlign: tri(r, (rr) => VERTICAL_ALIGNS[readChoice(rr, VERTICAL_ALIGNS.length, fail)], fail),
    color: tri(r, readRgb, fail),
    highlight: tri(r, readRgb, fail),
    style: tri(r, (rr) => readName(rr, fail, false), fail),
  };
}
function writeCharState(w, s) {
  writeTri(w, s.font, (ww, f) => { writeName(ww, f.requested); writeName(ww, f.used); writeFlag(ww, f.substituted); });
  writeTri(w, s.size, (ww, v) => { if (!Number.isInteger(v) || v < 2 || v > 3276) throw new TypeError("size"); ww.u16(v); });
  writeTri(w, s.bold, writeFlag);
  writeTri(w, s.italic, writeFlag);
  writeTri(w, s.underline, (ww, v) => ww.u8(indexOf(UNDERLINES, v)));
  writeTri(w, s.strike, writeFlag);
  writeTri(w, s.verticalAlign, (ww, v) => ww.u8(indexOf(VERTICAL_ALIGNS, v)));
  writeTri(w, s.color, writeRgb);
  writeTri(w, s.highlight, writeRgb);
  writeTri(w, s.style, writeName);
}

function readParaState(r, fail) {
  const out = {
    alignment: tri(r, (rr) => ALIGNMENTS[readChoice(rr, ALIGNMENTS.length, fail)], fail),
    indentLeft: tri(r, signedTwips, fail),
    indentRight: tri(r, signedTwips, fail),
    firstLine: tri(r, signedTwips, fail),
    spaceBefore: tri(r, lengthTwips, fail),
    spaceAfter: tri(r, lengthTwips, fail),
    lineSpacing: tri(r, (rr) => { const rule = readChoice(rr, LINE_RULES.length, fail); return { rule: LINE_RULES[rule], value: rr.u32() }; }, fail),
    shading: tri(r, readRgb, fail),
    borders: {},
  };
  for (const side of BORDER_SIDES) out.borders[side] = tri(r, readLine, fail);
  out.style = tri(r, (rr) => readName(rr, fail, false), fail);
  return out;
}
function writeParaState(w, s) {
  writeTri(w, s.alignment, (ww, v) => ww.u8(indexOf(ALIGNMENTS, v)));
  writeTri(w, s.indentLeft, writeSigned);
  writeTri(w, s.indentRight, writeSigned);
  writeTri(w, s.firstLine, writeSigned);
  writeTri(w, s.spaceBefore, writeLength);
  writeTri(w, s.spaceAfter, writeLength);
  writeTri(w, s.lineSpacing, (ww, v) => { ww.u8(indexOf(LINE_RULES, v.rule)); ww.u32(v.value); });
  writeTri(w, s.shading, writeRgb);
  for (const side of BORDER_SIDES) writeTri(w, s.borders[side], writeLine);
  writeTri(w, s.style, writeName);
}

function readStyleEntry(r, fail) {
  const id = readName(r, fail, false);
  const name = readName(r, fail, true);
  const basedOn = readName(r, fail, true);
  const target = nameOf(STYLE_TARGETS, r.u8(), fail);
  const flags = r.u8();
  if (flags & ~0x1f) fail("RICH_INVALID_BODY");
  const headingLevel = r.u8();
  if (headingLevel > 9) fail("RICH_INVALID_BODY");
  r.zero(1);
  const font = readName(r, fail, true);
  const halfPoints = r.u16();
  if (halfPoints !== 0 && (halfPoints < FORMAT_LIMITS.minHalfPoints || halfPoints > FORMAT_LIMITS.maxHalfPoints)) fail("RICH_INVALID_BODY");
  const charFlags = r.u8();
  if (charFlags & ~0x07) fail("RICH_INVALID_BODY");
  const underline = UNDERLINES[readChoice(r, UNDERLINES.length, fail)];
  const mode = r.u8();
  let color = null;
  if (mode === 0) r.zero(3); else if (mode === 1) color = readRgb(r); else fail("RICH_INVALID_BODY");
  const alignment = ALIGNMENTS[readChoice(r, ALIGNMENTS.length, fail)];
  return {
    id, name, basedOn, target, builtIn: (flags & 1) !== 0, used: (flags & 2) !== 0, current: (flags & 4) !== 0, isDefault: (flags & 8) !== 0, quick: (flags & 16) !== 0, headingLevel,
    sample: { font, halfPoints, bold: (charFlags & 1) !== 0, italic: (charFlags & 2) !== 0, strike: (charFlags & 4) !== 0, underline, color, alignment },
  };
}
function writeStyleEntry(w, s) {
  writeName(w, s.id); writeName(w, s.name ?? ""); writeName(w, s.basedOn ?? "");
  w.u8(indexOf(STYLE_TARGETS, s.target), (s.builtIn ? 1 : 0) | (s.used ? 2 : 0) | (s.current ? 4 : 0) | (s.isDefault ? 8 : 0) | (s.quick ? 16 : 0), s.headingLevel, 0);
  const sample = s.sample;
  writeName(w, sample.font);
  w.u16(sample.halfPoints);
  w.u8((sample.bold ? 1 : 0) | (sample.italic ? 2 : 0) | (sample.strike ? 4 : 0), indexOf(UNDERLINES, sample.underline));
  if (sample.color === null) w.u8(0, 0, 0, 0); else { w.u8(1); writeRgb(w, sample.color); }
  w.u8(indexOf(ALIGNMENTS, sample.alignment));
}

export function readFormatReply(opcode, r, ctx) {
  const { fail } = ctx;
  if (opcode === FormatReplyOpcode.FormatState) {
    const revision = r.u64();
    const anchor = r.u64();
    const focus = r.u64();
    const lastCommandId = r.u32();
    const flags = r.u8();
    if (flags & ~0x1f) fail("RICH_INVALID_BODY");
    r.zero(3);
    const chars = readCharState(r, fail);
    const paragraph = readParaState(r, fail);
    return { type: "formatState", revision, anchor, focus, lastCommandId, collapsed: (flags & 1) !== 0, readOnly: (flags & 2) !== 0, partial: (flags & 4) !== 0, typing: (flags & 8) !== 0, plain: (flags & 16) !== 0, chars, paragraph };
  }
  if (opcode === FormatReplyOpcode.StyleList) {
    const revision = r.u64();
    const count = r.u32();
    if (count > FORMAT_LIMITS.maxStyleList) fail("RICH_INVALID_BODY");
    const styles = [];
    for (let i = 0; i < count; i += 1) styles.push(readStyleEntry(r, fail));
    return { type: "styleList", revision, styles };
  }
  if (opcode === FormatReplyOpcode.FontList) {
    const revision = r.u64();
    const themeMajor = readName(r, fail, true);
    const themeMinor = readName(r, fail, true);
    const count = r.u32();
    if (count > FORMAT_LIMITS.maxFontList) fail("RICH_INVALID_BODY");
    const families = [];
    for (let i = 0; i < count; i += 1) {
      const name = readName(r, fail, false);
      const flags = r.u8();
      if (flags & ~0x0f) fail("RICH_INVALID_BODY");
      const substitute = readName(r, fail, true);
      if (((flags & 8) !== 0) === (substitute === "")) fail("RICH_INVALID_BODY");
      families.push({ name, bold: (flags & 1) !== 0, italic: (flags & 2) !== 0, used: (flags & 4) !== 0, substituted: (flags & 8) !== 0, substitute });
    }
    return { type: "fontList", revision, themeMajor, themeMinor, families };
  }
  if (opcode === FormatReplyOpcode.FormatMarks) {
    const revision = r.u64();
    const count = r.u32();
    if (count > FORMAT_LIMITS.maxFormatMarks) fail("RICH_INVALID_BODY");
    const marks = [];
    for (let i = 0; i < count; i += 1) {
      const page = r.u32();
      if (page >= 1 << 20) fail("RICH_PAGE_RANGE");
      const kind = MARK_KINDS[r.u8()];
      if (kind === undefined) fail("RICH_INVALID_BODY");
      r.zero(3);
      const x = r.f64(); const y = r.f64(); const width = r.f64();
      if (width < 0) fail("RICH_INVALID_BODY");
      marks.push({ page, kind, x, y, width });
    }
    return { type: "formatMarks", revision, marks };
  }
  return fail("RICH_UNKNOWN_OPCODE");
}

export function writeFormatReply(w, response) {
  if (response.type === "formatState") {
    w.u64(response.revision); w.u64(response.anchor); w.u64(response.focus); writeCommandId(w, response.lastCommandId);
    w.u8((response.collapsed ? 1 : 0) | (response.readOnly ? 2 : 0) | (response.partial ? 4 : 0) | (response.typing ? 8 : 0) | (response.plain ? 16 : 0), 0, 0, 0);
    writeCharState(w, response.chars);
    writeParaState(w, response.paragraph);
    return FormatReplyOpcode.FormatState;
  }
  if (response.type === "styleList") {
    if (response.styles.length > FORMAT_LIMITS.maxStyleList) throw new TypeError("too many styles");
    w.u64(response.revision); w.u32(response.styles.length);
    for (const s of response.styles) writeStyleEntry(w, s);
    return FormatReplyOpcode.StyleList;
  }
  if (response.type === "fontList") {
    if (response.families.length > FORMAT_LIMITS.maxFontList) throw new TypeError("too many fonts");
    w.u64(response.revision); writeName(w, response.themeMajor); writeName(w, response.themeMinor); w.u32(response.families.length);
    for (const f of response.families) {
      writeName(w, f.name);
      w.u8((f.bold ? 1 : 0) | (f.italic ? 2 : 0) | (f.used ? 4 : 0) | (f.substituted ? 8 : 0));
      writeName(w, f.substitute ?? "");
    }
    return FormatReplyOpcode.FontList;
  }
  if (response.type === "formatMarks") {
    if (response.marks.length > FORMAT_LIMITS.maxFormatMarks) throw new TypeError("too many marks");
    w.u64(response.revision); w.u32(response.marks.length);
    for (const m of response.marks) {
      if (!Number.isInteger(m.page) || m.page < 0) throw new TypeError("page");
      w.u32(m.page); w.u8(indexOf(MARK_KINDS, m.kind), 0, 0, 0); w.f64(m.x); w.f64(m.y); w.f64(m.width);
    }
    return FormatReplyOpcode.FormatMarks;
  }
  throw new TypeError("not a formatting reply");
}

// ------------------------------------------------------------------ client

const isRefusal = (error) => typeof error?.code === "string" && error.code.startsWith("RICH_") && error.code !== "RICH_FRAME_TOO_LARGE";

/**
 * Typed helpers over a `createRichEditorClient` client. Every changing command carries the revision guard of the
 * moment it is sent (read inside the queue, so a keystroke queued before it is taken into account), which makes
 * a repeat after a lost reply safe: an applied command is refused as stale, never applied twice.
 *
 *   formatState()                      -> the decoded `formatState` reply (never changes the document, never forces a resync)
 *   listStyles({filter, usedOnly})     -> `{revision, styles}`
 *   applyCharFormat({scope, edits})    -> `{outcome: "applied", response, change}`
 *   applyParaFormat({edits})           -> idem
 *   applyStyle({target, id})           -> idem
 *   previewStyle({target, id})         -> `{response, change}` (a State: the pages show the preview); cancelPreview({target})
 *
 * A changing command whose exchange failed other than by a `RICH_*` refusal (transport, abort, bad frame) has an
 * unknown outcome: the helper asks `formatState()` (the client resynchronises first) and compares revisions, then
 * rejects with `error.outcome` set to `"applied"` or `"not-applied"`; `retry` is not automatic, the caller decides.
 */
export function createFormatClient(client) {
  const guardNow = () => {
    const revision = client.cache?.revision;
    if (typeof revision !== "bigint") throw new TypeError("the client holds no revision: open a document first");
    return revision;
  };
  // Identifiers 1 to 2^32-1, one per command; 0 means "none" on the wire.
  let nextId = 1;
  const takeId = () => { const id = nextId; nextId = nextId === 0xffffffff ? 1 : nextId + 1; return id; };

  /**
   * A changing command. `retryOf` (the `error.commandId` of an earlier attempt that came out `"not-applied"`) is not
   * needed: every attempt takes a new identifier; an attempt whose outcome is unknown is settled by the engine, which
   * reports the last identifier it accepted (`lastCommandId`), never by comparing revisions.
   */
  // One changing command in flight per client, its resolution included: a second one queued behind a command whose
  // reply was lost would overwrite `lastCommandId` before the read-back and turn "applied" into "not-applied".
  let chain = Promise.resolve();
  function changing(command, signal) {
    const run = chain.then(() => changingNow(command, signal));
    chain = run.catch(() => {});
    return run;
  }

  async function changingNow(command, signal) {
    const commandId = takeId();
    let sent = false;
    try {
      // The guard is read when the command's turn comes, so a keystroke queued before it is taken into account.
      const result = await client.command(() => { sent = true; return { ...command, baseRevision: guardNow(), commandId }; }, { signal });
      return { outcome: "applied", commandId, ...result };
    } catch (error) {
      if (isRefusal(error) || error?.code === "CLIENT_CLOSED") throw error;
      // Unknown outcome: the resynchronisation of the client runs before this exchange.
      let outcome = sent ? null : "not-applied";
      if (sent) {
        try {
          // No caller signal: it may be the very one that cancelled the command.
          const state = await client.command({ type: "formatState" });
          outcome = state.response.lastCommandId === commandId ? "applied" : "not-applied";
        } catch { /* the document cannot be read either: outcome stays unknown */ }
      }
      error.outcome = outcome;
      error.commandId = commandId;
      throw error;
    }
  }

  return {
    async formatState({ signal } = {}) { return (await client.command({ type: "formatState" }, { signal })).response; },
    async listStyles({ filter = "all", usedOnly = false, signal } = {}) {
      const { response } = await client.command({ type: "listStyles", filter, usedOnly }, { signal });
      return { revision: response.revision, styles: response.styles };
    },
    async listFonts({ signal } = {}) { return (await client.command({ type: "listFonts" }, { signal })).response; },
    /** Font files (one face each) for the engine, which never carries them (US-240, T240-F); answered by a page state. */
    loadFonts({ fonts, signal }) { return client.command({ type: "loadFonts", fonts }, { signal }); },
    async formatMarks({ first, count, signal }) { return (await client.command({ type: "formatMarks", first, count }, { signal })).response; },
    applyCharFormat({ scope = "wordOrTyping", edits, signal }) { return changing({ type: "applyCharFormat", scope, edits }, signal); },
    applyParaFormat({ edits, signal }) { return changing({ type: "applyParaFormat", edits }, signal); },
    applyStyle({ target, id, signal }) { return changing({ type: "applyStyle", target, id }, signal); },
    previewStyle({ target, id, signal }) { return client.command({ type: "previewStyle", target, show: true, id }, { signal }); },
    cancelPreview({ target = "paragraph", signal } = {}) { return client.command({ type: "previewStyle", target, show: false, id: "" }, { signal }); },
  };
}
