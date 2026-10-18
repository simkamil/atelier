// SPDX-License-Identifier: AGPL-3.0-or-later
// Cross-checks the JS ATED v2 codec against the frozen vectors that the Rust
// tests of crates/core-wasm/src/rich_protocol.rs generate and verify.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  RICH_DIRECTIONS, RICH_LIMITS, RichErrorCode, RichProtocolError, decodeRichReply, decodeRichRequest,
  encodeRichReply, encodeRichRequest,
} from "../web/rich-editor-protocol.mjs";

const hex = (bytes) => Buffer.from(bytes).toString("hex");
const fromHex = (value) => Uint8Array.from(Buffer.from(value, "hex"));
const textHex = (text) => hex(new TextEncoder().encode(text));
const bits = (value) => { const b = Buffer.alloc(8); b.writeDoubleLE(value); return b.readBigUInt64LE().toString(16).padStart(16, "0"); };
const flag = (value) => (value ? 1 : 0);
const KINDS = { blank: 0, txt: 1, docx: 2 };
const PROFILES = { document: 0, txt: 1 };
const UNITS = { grapheme: 0, word: 1 };
const MODES = { probe: 0, setCaret: 1, extend: 2 };

function describeRequest({ docId, requestId, command: c }) {
  const head = `doc=${docId} req=${requestId}`;
  const tail = {
    open: () => `Open kind=${KINDS[c.kind]} profile=${PROFILES[c.profile]} data=${hex(c.data)}`,
    close: () => "Close",
    insertText: () => `InsertText text=${textHex(c.text)}`,
    replaceRange: () => `ReplaceRange start=${c.start} end=${c.end} text=${textHex(c.text)}`,
    delete: () => `Delete forward=${flag(c.forward)} unit=${UNITS[c.unit]}`,
    move: () => `Move direction=${RICH_DIRECTIONS.indexOf(c.direction)} extend=${flag(c.extend)}`,
    setSelection: () => `SetSelection anchor=${c.anchor} focus=${c.focus} upstream=${flag(c.upstream)}`,
    layout: () => `Layout budget=${c.budget}`,
    hitTest: () => `HitTest page=${c.page} mode=${MODES[c.mode]} x=${bits(c.x)} y=${bits(c.y)}`,
    undo: () => "Undo",
    redo: () => "Redo",
    textWindow: () => `TextWindow start=${c.start} len=${c.len}`,
    pages: () => `Pages first=${c.first} count=${c.count}`,
    markSaved: () => `MarkSaved revision=${c.revision}`,
    accessibleWindow: () => `AccessibleWindow start=${c.start} end=${c.end}`,
  }[c.type]();
  return `${head} ${tail}`;
}

const INLINE_KINDS = { object: 0, lineBreak: 1, pageBreak: 2, symbol: 3 };
const ROLES = { paragraph: 0, heading: 1, listItem: 2, tableCell: 3, opaque: 4 };
const describeItems = (items) => (items.length
  ? items.map((i) => `${i.paraStart}-${i.paraEnd}:${ROLES[i.role]}:${i.level}:${textHex(i.label)}:${textHex(i.text)}:${
    i.inlines.length ? i.inlines.map((o) => `${o.offset}/${INLINE_KINDS[o.kind]}/${textHex(o.value)}`).join("+") : "none"}`).join(";")
  : "none");

const describePages = (pages) => (pages.length ? pages.map((p) => `${p.index}:${p.display.length}`).join(",") : "none");

function describeReply({ docId, requestId, response: s }) {
  const head = `doc=${docId} req=${requestId}`;
  const tail = {
    error: () => `Error code=${s.code}`,
    closed: () => "Closed",
    accessibleWindow: () => `AccessibleWindow revision=${s.revision} items=${describeItems(s.items)}`,
    hitResult: () => `HitResult revision=${s.revision} page=${s.page} position=${s.position} upstream=${flag(s.upstream)}`,
    textWindow: () => `TextWindow revision=${s.revision} start=${s.start} doc_len=${s.docLen} text=${textHex(s.text)}`,
    pages: () => `Pages revision=${s.revision} page_count=${s.pageCount} pages=${describePages(s.pages)}`,
    state: () => {
      const caret = s.caret ? `${s.caret.page}:${bits(s.caret.x)}:${bits(s.caret.y)}:${bits(s.caret.height)}` : "none";
      const inval = s.invalidated.length ? s.invalidated.map(([a, b]) => `${a}-${b}`).join(",") : "none";
      const window = s.window ? `${s.window.start}:${textHex(s.window.text)}` : "none";
      return `State revision=${s.revision} anchor=${s.anchor} focus=${s.focus} focus_upstream=${flag(s.focusUpstream)} doc_len=${s.docLen} modified=${flag(s.modified)} can_undo=${flag(s.canUndo)} can_redo=${flag(s.canRedo)} layout_complete=${flag(s.layoutComplete)} page_count=${s.pageCount} caret=${caret} invalidated=${inval} pages=${describePages(s.pages)} window=${window}`;
    },
  }[s.type]();
  return `${head} ${tail}`;
}

function expectCode(callback, code, context) {
  assert.throws(callback, (error) => {
    assert.ok(error instanceof RichProtocolError, String(error));
    assert.equal(error.code, code);
    assert.equal(error.context !== null, context);
    return true;
  });
}

const rows = readFileSync(new URL("./rich-editor-protocol-vectors.txt", import.meta.url), "utf8")
  .split(/\r?\n/).filter((line) => line && !line.startsWith("#")).map((line) => line.split("\t"));
const counts = { request: 0, reply: 0, "bad-request": 0, "bad-reply": 0 };

for (const [kind, name, frameHex, expected] of rows) {
  const bytes = fromHex(frameHex);
  counts[kind] += 1;
  if (kind === "request") {
    const decoded = decodeRichRequest(bytes);
    assert.equal(describeRequest(decoded), expected, name);
    assert.equal(hex(encodeRichRequest(decoded)), frameHex, `${name} re-encode`);
  } else if (kind === "reply") {
    const decoded = decodeRichReply(bytes);
    assert.equal(describeReply(decoded), expected, name);
    assert.equal(hex(encodeRichReply(decoded)), frameHex, `${name} re-encode`);
  } else {
    const [code, note] = expected.split(" ");
    const decode = kind === "bad-request" ? decodeRichRequest : decodeRichReply;
    expectCode(() => decode(bytes), code, note !== "nocontext");
  }
}
assert.deepEqual(counts, { request: 27, reply: 12, "bad-request": 41, "bad-reply": 27 });

// Truncation: every strict prefix of a valid frame is rejected, never thrown as a foreign error.
for (const [kind, , frameHex] of rows.filter((row) => row[0] === "request" || row[0] === "reply")) {
  const bytes = fromHex(frameHex);
  const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
  for (let cut = 0; cut < bytes.length; cut += 1) {
    assert.throws(() => decode(bytes.subarray(0, cut)), RichProtocolError);
  }
}

// Encoder limits match the specification.
const open = (n) => ({ docId: 1, requestId: 1, command: { type: "open", kind: "docx", profile: "document", data: new Uint8Array(n) } });
assert.equal(encodeRichRequest(open(RICH_LIMITS.maxOpenBytes)).length, RICH_LIMITS.maxFrameBytes);
expectCode(() => encodeRichRequest(open(RICH_LIMITS.maxOpenBytes + 1)), "RICH_FRAME_TOO_LARGE", false);
expectCode(() => encodeRichRequest({ docId: 0, requestId: 1, command: { type: "undo" } }), "RICH_INVALID_DOCUMENT_ID", true);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "insertText", text: "a￼" } }), "RICH_INVALID_TEXT", false);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "insertText", text: "a\ud800" } }), "RICH_INVALID_UTF8", false);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "replaceRange", start: 3, end: 4n, text: "" } }), "RICH_INVALID_BODY", false);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "replaceRange", start: 5n, end: 4n, text: "" } }), "RICH_INVALID_RANGE", true);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "nope" } }), "RICH_UNKNOWN_OPCODE", false);
expectCode(() => decodeRichRequest("not bytes"), "RICH_INVALID_ENVELOPE", false);

// A reply never decodes as a request and the reverse.
const [, , undoFrame] = rows.find((row) => row[1] === "undo");
expectCode(() => decodeRichReply(fromHex(undoFrame)), "RICH_UNKNOWN_OPCODE", true);

// Bit flips: every single-byte corruption either decodes or raises a typed error.
for (const [kind, , frameHex] of rows.filter((row) => row[0] === "request" || row[0] === "reply")) {
  const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
  const original = fromHex(frameHex);
  for (let at = 0; at < Math.min(original.length, 160); at += 1) {
    const mutated = Uint8Array.from(original);
    mutated[at] ^= 0xff;
    try { decode(mutated); } catch (error) { assert.ok(error instanceof RichProtocolError, `byte ${at}: ${error}`); }
  }
}

// Decoders alias the frame (no copy): page displays are views.
{
  const pagesRow = rows.find((row) => row[1] === "pages" && row[0] === "reply");
  const bytes = fromHex(pagesRow[2]);
  const decoded = decodeRichReply(bytes);
  assert.equal(decoded.response.pages[0].display.buffer, bytes.buffer);
}

// Replies at the limits: 16 pages, 256 KiB window, 1024 invalidations.
const atdl = (fill) => { const p = new Uint8Array(RICH_LIMITS.minPageDisplayBytes); p.set([0x41, 0x54, 0x44, 0x4c, 1]); p[43] = fill; return p; };
const limitState = () => ({
  type: "state", revision: 1n, anchor: 0n, focus: 0n, docLen: BigInt(RICH_LIMITS.maxWindowBytes),
  focusUpstream: false, modified: false, canUndo: false, canRedo: false, layoutComplete: true, pageCount: 40, caret: null,
  invalidated: Array.from({ length: RICH_LIMITS.maxInvalidations }, (_, i) => [i * 2, i * 2 + 1]),
  pages: Array.from({ length: RICH_LIMITS.maxPagesPerResponse }, (_, i) => ({ index: i * 2, display: atdl(i) })),
  window: { start: 0n, text: "a".repeat(RICH_LIMITS.maxWindowBytes) },
});
{
  const at = { docId: 1, requestId: 1, response: limitState() };
  const decoded = decodeRichReply(encodeRichReply(at));
  assert.equal(decoded.response.pages.length, 16);
  assert.equal(decoded.response.invalidated.length, 1024);
  assert.equal(decoded.response.window.text.length, RICH_LIMITS.maxWindowBytes);
  const over = (edit, code) => {
    const response = limitState();
    edit(response);
    expectCode(() => encodeRichReply({ docId: 1, requestId: 1, response }), code, true);
  };
  over((r) => r.pages.push({ index: 33, display: atdl(0) }), "RICH_TOO_MANY_PAGES");
  over((r) => { r.invalidated.push([5000, 5001]); }, "RICH_INVALID_BODY");
  over((r) => { r.window.text += "a"; r.docLen += 1n; }, "RICH_WINDOW_TOO_LARGE");
  over((r) => { r.invalidated = [[1, 2], [2, 3]]; }, "RICH_PAGE_RANGE");
}

// Typed errors for malformed caller input, no inherited keys as codes.
expectCode(() => encodeRichRequest(null), "RICH_UNKNOWN_OPCODE", false);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "insertText" } }), "RICH_INVALID_BODY", false);
expectCode(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "open", kind: "blank", profile: "document", data: "x" } }), "RICH_INVALID_BODY", false);
expectCode(() => encodeRichReply({ docId: 1, requestId: 1, response: { type: "error", code: "constructor" } }), "RICH_INVALID_BODY", false);
expectCode(() => encodeRichReply({ docId: 1, requestId: 1, response: { type: "state" } }), "RICH_INVALID_BODY", false);
assert.equal(RichErrorCode.constructor, undefined);

console.log(`rich editor protocol: ${rows.length} vectors verified`);
