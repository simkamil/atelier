// SPDX-License-Identifier: AGPL-3.0-or-later
// R7b-2: search, replace, statistics and properties of the rich editor against the real WASM engine, in ATED v2 frames.
// `node tests/rich-editor-find.mjs [engine.wasm]` (default: the wasm-release build of `python tools/package-engine.py`).
//
// Part A checks the codec against the frozen vectors of tests/rich-editor-find-vectors.txt (written by the Rust tests);
// part B runs the scenarios of the contract (docs/us-340-search.md, "Rich editor protocol") through the engine:
// every search option, flat positions through objects-free text, replacement as one undo step, statistics of the document
// and of the selection, highlight rectangles, properties saved in the DOCX, read-only documents, refusals and their codes.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { decodeRichReply, encodeRichRequest, PROPERTY_FIELDS } from "../web/rich-editor-protocol.mjs";
import { verifyFindVectors } from "./rich-editor-find-vectors.mjs";

console.log(`find protocol: ${verifyFindVectors()} vectors verified`);

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const instance = new WebAssembly.Instance(await WebAssembly.compile(await readFile(wasmPath)));
const { memory, alloc, dispatch, free } = instance.exports;
let outerId = 1n;
let innerId = 1;

function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  assert.notEqual(ptr, 0, "input allocation");
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  assert.notEqual(descriptorPtr, 0, "response descriptor");
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const bodyPtr = descriptor.getUint32(0, true);
  const bodyLen = descriptor.getUint32(4, true);
  try {
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    return { status: new DataView(body.buffer).getUint32(0, true), frame: body.slice(4) };
  } finally {
    assert.equal(free(bodyPtr, bodyLen), 0);
    assert.equal(free(descriptorPtr, 8), 0);
  }
}

function send(command, doc = 1) {
  const payload = encodeRichRequest({ docId: doc, requestId: innerId++, command });
  const result = invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload }));
  assert.equal(result.status, 0);
  const message = decodeMessage(result.frame, "event");
  assert.equal(message.type, MessageType.EditorResult);
  const reply = decodeRichReply(message.payload);
  assert.equal(reply.requestId, innerId - 1);
  return reply.response;
}
const expect = (command, type) => {
  const r = send(command);
  assert.equal(r.type, type, `${command.type}: ${r.code ?? r.type}`);
  return r;
};
const errorOf = (command) => expect(command, "error").code;
const state = (command) => expect(command, "state");
const utf8 = (s) => new TextEncoder().encode(s);
const openTxt = (t) => state({ type: "open", kind: "txt", profile: "txt", data: utf8(t) });
const text = () => expect({ type: "textWindow", start: 0n, len: 262144 }, "textWindow").text;
const select = (a, f = a) => state({ type: "setSelection", anchor: BigInt(a), focus: BigInt(f), upstream: false });
const revision = () => state({ type: "layout", budget: 1 }).revision;
const search = (query, flags = {}) => expect({ type: "search", revision: revision(), flags, query }, "searchResult");
const spans = (r) => r.items.map((i) => [Number(i.start), Number(i.end)]);
const replaceAll = (query, replacement, flags = {}) => expect({ type: "replaceAll", revision: revision(), flags, query, replacement }, "replaced");
const stats = () => expect({ type: "statistics" }, "statistics");
const properties = () => expect({ type: "propertiesRead" }, "properties").snapshot;
const apply = (changes) => expect({ type: "propertiesApply", changes }, "propertiesApplied");
const STAMP = { nowMs: 1_700_000_000_000n, user: "Alice", version: "1.2" };
let checks = 0;
const check = (name, run) => { run(); checks += 1; };

check("search options", () => {
  openTxt("Hello hello HELLO hellos\nrésumé resume\na b a b");
  const r = search("hello");
  assert.deepEqual(spans(r), [[0, 5], [6, 11], [12, 17], [18, 23]]);
  assert.equal(r.total, 4);
  assert.equal(r.truncated || r.totalCapped, false);
  assert.deepEqual(spans(search("hello", { matchCase: true })), [[6, 11], [18, 23]]);
  assert.deepEqual(spans(search("hello", { wholeWord: true })), [[0, 5], [6, 11], [12, 17]]);
  assert.deepEqual(spans(search("resume")), [[34, 40]]);
  assert.deepEqual(spans(search("resume", { ignoreAccents: true })), [[25, 33], [34, 40]]);
  assert.equal(search("a b").items.length, 2);
  assert.equal(search("a b", { matchNbsp: true }).items.length, 1);
  assert.equal(search("").items.length, 0);
  const item = search("hello").items[1];
  assert.equal(item.excerpt.text.slice(item.excerpt.start, item.excerpt.end), "hello");
});

check("flat positions read back through the text window", () => {
  openTxt("un café \u{1F642} café\nsecond café");
  const t = utf8(text());
  const found = search("café").items;
  assert.equal(found.length, 3);
  for (const item of found) assert.equal(new TextDecoder().decode(t.subarray(Number(item.start), Number(item.end))), "café");
});

check("more occurrences than the list holds", () => {
  openTxt("a ".repeat(1105));
  const r = search("a");
  assert.equal(r.items.length, 1000);
  assert.equal(r.total, 1105);
  assert.equal(r.truncated, true);
  assert.equal(r.totalCapped, false);
  const all = replaceAll("a", "aa");
  assert.equal(all.count, 1105);
  assert.equal(text(), "aa ".repeat(1105));
  assert.equal(all.state.revision, 1n);
});

check("replace all is one undo step", () => {
  openTxt("foo bar foo\nfoo");
  const done = replaceAll("foo", "qux\nz");
  assert.equal(done.count, 3);
  assert.equal(text(), "qux\nz bar qux\nz\nqux\nz");
  assert.equal(done.state.modified && done.state.canUndo, true);
  const undone = state({ type: "undo" });
  assert.equal(text(), "foo bar foo\nfoo");
  assert.equal(undone.revision, 2n);
  assert.equal(undone.canUndo, false);
  assert.equal(state({ type: "redo" }).revision, 3n);
  assert.equal(text(), "qux\nz bar qux\nz\nqux\nz");
  assert.equal(replaceAll("absent", "x").count, 0);
  assert.equal(replaceAll("bar", "bar").state.revision, 3n, "an occurrence that already reads like its replacement changes nothing");
});

check("replace the current occurrence", () => {
  openTxt("cat dog cat");
  const rev = revision();
  const replace = (r, start, end) => ({ type: "replace", revision: r, flags: {}, start: BigInt(start), end: BigInt(end), query: "cat", replacement: "bird" });
  assert.equal(errorOf(replace(rev, 8, 11)), "RICH_STALE_MATCH");
  select(8, 11);
  assert.equal(errorOf(replace(rev + 3n, 8, 11)), "RICH_STALE_REVISION");
  select(4, 7);
  assert.equal(errorOf(replace(rev, 4, 7)), "RICH_STALE_MATCH");
  select(8, 11);
  const done = expect(replace(rev, 8, 11), "replaced");
  assert.equal(done.count, 1);
  assert.deepEqual([done.state.anchor, done.state.focus], [12n, 12n]);
  assert.equal(text(), "cat dog bird");
});

check("refusals carry their stable codes", () => {
  openTxt("abc");
  const rev = revision();
  const request = { docId: 1, requestId: 1 };
  assert.throws(() => encodeRichRequest({ ...request, command: { type: "replaceAll", revision: rev, query: "", replacement: "x" } }), { code: "RICH_EMPTY_QUERY" });
  assert.equal(errorOf({ type: "search", revision: rev + 1n, flags: {}, query: "a" }), "RICH_STALE_REVISION");
  assert.equal(errorOf({ type: "searchRects", revision: rev, firstPage: 40, pageCount: 1, ranges: [{ start: 0n, end: 1n }] }), "RICH_PAGE_RANGE");
  assert.equal(errorOf({ type: "searchRects", revision: rev, firstPage: 0, pageCount: 1, ranges: [{ start: 0n, end: 99n }] }), "RICH_POSITION_OUT_OF_RANGE");
  assert.equal(errorOf({ type: "replaceAll", revision: rev + 1n, flags: {}, query: "a", replacement: "b" }), "RICH_STALE_REVISION");
});

check("statistics of the document and of the selection", () => {
  openTxt("one two three\nfour");
  select(0, 7);
  const r = stats();
  assert.deepEqual([r.revision, r.anchor, r.focus, r.layoutComplete], [0n, 0n, 7n, true]);
  assert.deepEqual(r.document, { words: 4, characters: 15, charactersWithSpaces: 17, paragraphs: 2, lines: 2, pages: 1 });
  assert.deepEqual(r.selection, { words: 2, characters: 6, charactersWithSpaces: 7, paragraphs: 1, lines: 1, pages: 1 });
  select(4, 16);
  assert.deepEqual([stats().selection.paragraphs, stats().selection.lines], [2, 2]);
  select(3);
  assert.deepEqual(stats().selection, { words: 0, characters: 0, charactersWithSpaces: 0, paragraphs: 0, lines: 0, pages: 0 });
});

check("highlight rectangles of the occurrences of a page", () => {
  openTxt("alpha beta\ngamma beta\nbeta");
  const r = search("beta");
  const ranges = r.items.map((i) => ({ start: i.start, end: i.end }));
  const rects = expect({ type: "searchRects", revision: r.revision, firstPage: 0, pageCount: 1, ranges }, "searchRects").rects;
  assert.ok(rects.length >= 3);
  assert.ok(rects.every((x) => x.page === 0 && x.width > 0 && x.height > 0 && x.item < 3));
  const y = (item) => rects.find((x) => x.item === item).y;
  assert.ok(y(0) < y(1) && y(1) < y(2));
});

check("properties: read, apply, too long, modified, saved in the DOCX", () => {
  state({ type: "open", kind: "blank", profile: "document", data: new Uint8Array() });
  const before = properties();
  assert.equal(before.editable.title, "");
  assert.deepEqual(PROPERTY_FIELDS.map((n) => before.limits[n]), [255, 255, 255, 255, 255, 255, 1024, 4096, 2048, 255, 35]);
  const applied = apply({ title: "Mon titre", keywords: "a, b" });
  assert.deepEqual([applied.status, applied.changed, applied.state.modified, applied.state.revision], ["applied", 2, true, 0n]);
  assert.equal(properties().editable.title, "Mon titre");
  assert.equal(apply({ title: "Mon titre" }).changed, 0);
  const tooLong = apply({ subject: "x".repeat(300) });
  assert.deepEqual([tooLong.status, tooLong.field, tooLong.max, tooLong.length], ["tooLong", "subject", 255, 300]);
  assert.equal(properties().editable.subject, "");
  const saved = expect({ type: "save", format: "docx", ...STAMP }, "saveData");
  assert.equal(state({ type: "markSaved", revision: saved.revision }).modified, false);
  const opened = state({ type: "open", kind: "docx", profile: "document", data: saved.bytes });
  assert.equal(opened.modified, false);
  const back = properties();
  assert.equal(back.editable.title, "Mon titre");
  assert.equal(back.editable.keywords, "a, b");
  assert.equal(back.lastModifiedBy, "Alice");
  assert.equal(back.application, "Atelier");
  assert.equal(apply({ title: "" }).changed, 1);
  assert.equal(properties().editable.title, "");
});

check("a read-only document is searched but neither replaced nor given properties", () => {
  const bytes = readFileSync(new URL("./docx-reference/revisions.docx", import.meta.url));
  const opened = state({ type: "open", kind: "docx", profile: "document", data: bytes });
  assert.equal(opened.readOnly, true);
  const found = search("fin");
  assert.ok(found.items.length >= 1);
  assert.equal(errorOf({ type: "replaceAll", revision: revision(), flags: {}, query: "fin", replacement: "x" }), "RICH_READ_ONLY");
  assert.equal(errorOf({ type: "propertiesApply", changes: { title: "t" } }), "RICH_READ_ONLY");
  assert.equal(stats().document.words > 0, true);
  assert.equal(properties().limits.title, 255);
});

console.log(`find engine: ${checks} scenarios passed`);
