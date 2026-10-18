// SPDX-License-Identifier: AGPL-3.0-or-later
// Cross-checks the JS ATED v2 codec against the frozen vectors that the Rust
// tests of crates/core-wasm/src/rich_protocol.rs generate and verify.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  RICH_DIRECTIONS, RICH_LIMITS, RichProtocolError, decodeRichReply, decodeRichRequest,
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
    setSelection: () => `SetSelection anchor=${c.anchor} focus=${c.focus}`,
    hitTest: () => `HitTest page=${c.page} mode=${MODES[c.mode]} x=${bits(c.x)} y=${bits(c.y)}`,
    undo: () => "Undo",
    redo: () => "Redo",
    textWindow: () => `TextWindow start=${c.start} len=${c.len}`,
    pages: () => `Pages first=${c.first} count=${c.count}`,
    markSaved: () => `MarkSaved revision=${c.revision}`,
  }[c.type]();
  return `${head} ${tail}`;
}

const describePages = (pages) => (pages.length ? pages.map((p) => `${p.index}:${p.display.length}`).join(",") : "none");

function describeReply({ docId, requestId, response: s }) {
  const head = `doc=${docId} req=${requestId}`;
  const tail = {
    error: () => `Error code=${s.code}`,
    closed: () => "Closed",
    hitResult: () => `HitResult revision=${s.revision} page=${s.page} position=${s.position}`,
    textWindow: () => `TextWindow revision=${s.revision} start=${s.start} doc_len=${s.docLen} text=${textHex(s.text)}`,
    pages: () => `Pages revision=${s.revision} page_count=${s.pageCount} pages=${describePages(s.pages)}`,
    state: () => {
      const caret = s.caret ? `${s.caret.page}:${bits(s.caret.x)}:${bits(s.caret.y)}:${bits(s.caret.height)}` : "none";
      const inval = s.invalidated.length ? s.invalidated.map(([a, b]) => `${a}-${b}`).join(",") : "none";
      const window = s.window ? `${s.window.start}:${textHex(s.window.text)}` : "none";
      return `State revision=${s.revision} anchor=${s.anchor} focus=${s.focus} doc_len=${s.docLen} modified=${flag(s.modified)} can_undo=${flag(s.canUndo)} can_redo=${flag(s.canRedo)} layout_complete=${flag(s.layoutComplete)} page_count=${s.pageCount} caret=${caret} invalidated=${inval} pages=${describePages(s.pages)} window=${window}`;
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
assert.deepEqual(counts, { request: 21, reply: 10, "bad-request": 33, "bad-reply": 12 });

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

console.log(`rich editor protocol: ${rows.length} vectors verified`);
