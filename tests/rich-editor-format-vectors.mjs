// SPDX-License-Identifier: AGPL-3.0-or-later
// F0: the frozen vectors of the formatting messages of ATED v2 (FormatState, ApplyCharFormat, ApplyParaFormat,
// ListStyles, ApplyStyle, PreviewStyle and the replies FormatState and StyleList), written by the Rust tests
// (crates/core-wasm/src/rich_protocol/format_tests.rs, ATELIER_REGEN_VECTORS=1) and checked here against the JS
// codec: every frame decodes to the described fields, re-encodes to the same bytes, refusals carry the same stable
// code, every truncation is a typed error. Run by tests/rich-editor-format.mjs.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, RichProtocolError } from "../web/rich-editor-protocol.mjs";
import {
  ALIGNMENTS, BORDER_SIDES, BORDER_STYLES, CHAR_ATTRS, EDIT_OPS, FORMAT_LIMITS, FORMAT_SCOPES, LINE_RULES, NO_REVISION_GUARD, PARA_ATTRS,
  MARK_KINDS, STYLE_FILTERS, STYLE_TARGETS, UNDERLINES, VERTICAL_ALIGNS,
} from "../web/rich-editor-format.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const encoder = new TextEncoder();
const hex = (bytes) => Buffer.from(bytes).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));
const text = (s) => hex(encoder.encode(s));
const rgb = (c) => [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");
const guard = (g) => (g === NO_REVISION_GUARD ? "none" : String(g));
const line = (l) => `${BORDER_STYLES.indexOf(l.style)}.${l.eighths}.${l.space}.${l.color === null ? "auto" : rgb(l.color)}`;
const bool = (v) => (v ? 1 : 0);

function charValue(attr, value) {
  if (value === undefined) return "-";
  switch (attr) {
    case "font": return text(value);
    case "size": return String(value);
    case "bold": case "italic": case "strike": return String(bool(value));
    case "underline": return String(UNDERLINES.indexOf(value));
    case "verticalAlign": return String(VERTICAL_ALIGNS.indexOf(value));
    default: return rgb(value);
  }
}

function paraValue(attr, value) {
  if (value === undefined) return "-";
  switch (attr) {
    case "alignment": return String(ALIGNMENTS.indexOf(value));
    case "lineSpacing": return `${LINE_RULES.indexOf(value.rule)}/${value.value}`;
    case "shading": return rgb(value);
    case "borders": return BORDER_SIDES.map((side) => (typeof value[side] === "string" ? value[side] : line(value[side]))).join(",");
    default: return String(value);
  }
}

function describeRequest(c) {
  switch (c.type) {
    case "formatState": return "FormatState";
    case "applyCharFormat":
      return `ApplyCharFormat guard=${guard(c.baseRevision)} scope=${FORMAT_SCOPES.indexOf(c.scope)} edits=${c.edits.map((e) => `${CHAR_ATTRS[e.attr]}:${EDIT_OPS[e.op]}:${charValue(e.attr, e.value)}`).join(";")}`;
    case "applyParaFormat":
      return `ApplyParaFormat guard=${guard(c.baseRevision)} edits=${c.edits.map((e) => `${PARA_ATTRS[e.attr]}:${EDIT_OPS[e.op]}:${paraValue(e.attr, e.value)}`).join(";")}`;
    case "listStyles": return `ListStyles filter=${STYLE_FILTERS.indexOf(c.filter)} used=${bool(c.usedOnly)}`;
    case "applyStyle": return `ApplyStyle guard=${guard(c.baseRevision)} target=${STYLE_TARGETS.indexOf(c.target)} id=${text(c.id)}`;
    case "listFonts": return "ListFonts";
    case "formatMarks": return `FormatMarks first=${c.first} count=${c.count}`;
    case "previewStyle": return `PreviewStyle target=${STYLE_TARGETS.indexOf(c.target)} show=${bool(c.show)} id=${text(c.id)}`;
    default: throw new Error(`not a formatting command: ${c.type}`);
  }
}

const tri = (t, show) => (t.state === "mixed" ? "m" : t.state === "unset" ? "u" : `v${show(t.value)}`);

function describeReply(r) {
  switch (r.type) {
    case "error": return `Error code=${r.code}`;
    case "formatState": {
      const c = r.chars; const p = r.paragraph;
      return `FormatState revision=${r.revision} anchor=${r.anchor} focus=${r.focus} flags=${bool(r.collapsed) | (bool(r.readOnly) << 1) | (bool(r.partial) << 2) | (bool(r.typing) << 3) | (bool(r.plain) << 4)} `
        + `font=${tri(c.font, (f) => `${text(f.requested)}/${text(f.used)}/${bool(f.substituted)}`)} size=${tri(c.size, String)} bold=${tri(c.bold, (v) => bool(v))} italic=${tri(c.italic, (v) => bool(v))} `
        + `underline=${tri(c.underline, (v) => UNDERLINES.indexOf(v))} strike=${tri(c.strike, (v) => bool(v))} valign=${tri(c.verticalAlign, (v) => VERTICAL_ALIGNS.indexOf(v))} `
        + `color=${tri(c.color, rgb)} highlight=${tri(c.highlight, rgb)} cstyle=${tri(c.style, text)} | `
        + `align=${tri(p.alignment, (v) => ALIGNMENTS.indexOf(v))} left=${tri(p.indentLeft, String)} right=${tri(p.indentRight, String)} first=${tri(p.firstLine, String)} `
        + `before=${tri(p.spaceBefore, String)} after=${tri(p.spaceAfter, String)} line=${tri(p.lineSpacing, (v) => `${LINE_RULES.indexOf(v.rule)}/${v.value}`)} shading=${tri(p.shading, rgb)} `
        + `borders=${BORDER_SIDES.map((side) => tri(p.borders[side], line)).join(",")} pstyle=${tri(p.style, text)}`;
    }
    case "styleList":
      return `StyleList revision=${r.revision} styles=${r.styles.length === 0 ? "none" : r.styles.map((s) => [
        text(s.id), text(s.name), text(s.basedOn), STYLE_TARGETS.indexOf(s.target), bool(s.builtIn) | (bool(s.used) << 1) | (bool(s.current) << 2) | (bool(s.isDefault) << 3) | (bool(s.quick) << 4), s.headingLevel,
        text(s.sample.font), s.sample.halfPoints, bool(s.sample.bold) | (bool(s.sample.italic) << 1) | (bool(s.sample.strike) << 2), UNDERLINES.indexOf(s.sample.underline),
        s.sample.color === null ? "auto" : rgb(s.sample.color), ALIGNMENTS.indexOf(s.sample.alignment),
      ].join("/")).join(";")}`;
    case "fontList":
      return `FontList revision=${r.revision} theme=${text(r.themeMajor)}/${text(r.themeMinor)} families=${r.families.length === 0 ? "none" : r.families.map((f) => `${text(f.name)}/${bool(f.bold) | (bool(f.italic) << 1) | (bool(f.used) << 2) | (bool(f.substituted) << 3)}/${text(f.substitute)}`).join(";")}`;
    case "formatMarks":
      return `FormatMarks revision=${r.revision} marks=${r.marks.length === 0 ? "none" : r.marks.map((m) => `${m.page}:${MARK_KINDS.indexOf(m.kind)}:${m.x}:${m.y}:${m.width}`).join(";")}`;
    default: throw new Error(`not a formatting reply: ${r.type}`);
  }
}

/** Checks every vector of tests/rich-editor-format-vectors.txt; returns the number of rows. */
export function verifyFormatVectors() {
  const rows = readFileSync(here("rich-editor-format-vectors.txt"), "utf8")
    .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
  const counted = { request: 0, reply: 0, "bad-request": 0, "bad-reply": 0 };
  for (const [kind, name, frameHex, expected] of rows) {
    counted[kind] += 1;
    const bytes = fromHex(frameHex);
    if (kind === "request") {
      const d = decodeRichRequest(bytes);
      assert.equal(`doc=${d.docId} req=${d.requestId} ${describeRequest(d.command)}`, expected, name);
      assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    } else if (kind === "reply") {
      const d = decodeRichReply(bytes);
      assert.equal(`doc=${d.docId} req=${d.requestId} ${describeReply(d.response)}`, expected, name);
      assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
    } else {
      const decode = kind === "bad-request" ? decodeRichRequest : decodeRichReply;
      assert.throws(() => decode(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
    }
  }
  assert.ok(counted.request >= 15 && counted.reply >= 8 && counted["bad-request"] >= 54 && counted["bad-reply"] >= 18, JSON.stringify(counted));
  for (const [kind, , frameHex] of rows.filter((r) => r[0] === "request" || r[0] === "reply")) {
    const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
    const bytes = fromHex(frameHex);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decode(bytes.subarray(0, cut)), RichProtocolError);
  }
  // Limits written by the encoders and refused by them (never a frame the decoder would reject).
  const request = (command) => encodeRichRequest({ docId: 1, requestId: 1, command });
  const refused = (command, code = "RICH_INVALID_BODY") => assert.throws(() => request(command), (e) => e instanceof RichProtocolError && e.code === code, String(command.type));
  const char = (edits, extra = {}) => ({ type: "applyCharFormat", baseRevision: 0n, scope: "wordOrTyping", edits, ...extra });
  const bold = { attr: "bold", op: "toggle" };
  assert.ok(request(char([bold])));
  refused(char([]));
  refused(char([bold, bold]));
  refused(char([{ ...bold, value: true }]));
  refused(char([{ attr: "bold", op: "set" }]));
  refused(char([{ attr: "bold", op: "set", value: 1 }]));
  refused(char([{ attr: "size", op: "set", value: 1 }]));
  refused(char([{ attr: "size", op: "set", value: FORMAT_LIMITS.maxHalfPoints + 1 }]));
  refused(char([{ attr: "size", op: "set", value: 11.5 }]));
  refused(char([{ attr: "font", op: "toggle" }]));
  refused(char([{ attr: "font", op: "set", value: "" }]));
  refused(char([{ attr: "font", op: "set", value: "a".repeat(FORMAT_LIMITS.maxNameBytes + 1) }]));
  refused(char([{ attr: "underline", op: "toggle", value: "none" }]));
  refused(char([{ attr: "color", op: "set", value: { r: 256, g: 0, b: 0 } }]));
  refused(char([{ attr: "nope", op: "set", value: 1 }]));
  refused(char([bold], { scope: "everything" }));
  refused(char([bold], { baseRevision: 0 }));
  refused({ type: "applyParaFormat", baseRevision: 0n, edits: [{ attr: "indentLeft", op: "set", value: FORMAT_LIMITS.maxTwips + 1 }] });
  refused({ type: "applyParaFormat", baseRevision: 0n, edits: [{ attr: "spaceAfter", op: "set", value: -1 }] });
  refused({ type: "applyParaFormat", baseRevision: 0n, edits: [{ attr: "lineSpacing", op: "set", value: { rule: "auto", value: 14 } }] });
  refused({ type: "applyParaFormat", baseRevision: 0n, edits: [{ attr: "borders", op: "set", value: { top: "none" } }] });
  refused({ type: "applyStyle", baseRevision: 0n, target: "paragraph", id: "" });
  assert.ok(request({ type: "applyStyle", baseRevision: 0n, target: "character", id: "" }));
  refused({ type: "previewStyle", target: "paragraph", show: false, id: "x" });
  refused({ type: "listStyles", filter: "tables", usedOnly: false });
  const manyStyles = Array.from({ length: FORMAT_LIMITS.maxStyleList + 1 }, (_, i) => ({
    id: `S${i}`, name: "", basedOn: "", target: "paragraph", builtIn: false, used: false, current: false, isDefault: false, headingLevel: 0,
    sample: { font: "", halfPoints: 0, bold: false, italic: false, strike: false, underline: "none", color: null, alignment: "left" },
  }));
  assert.throws(() => encodeRichReply({ docId: 1, requestId: 1, response: { type: "styleList", revision: 0n, styles: manyStyles } }), RichProtocolError);
  return rows.length;
}
