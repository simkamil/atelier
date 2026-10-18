// SPDX-License-Identifier: AGPL-3.0-or-later
// US-218 (R3b): open, create and save in the rich editor, against the real
// WASM engine in ATED v2 frames. `node tests/rich-editor-save.mjs [engine.wasm]`
// (default: the wasm-release build of `python tools/package-engine.py`).
//
// Part A checks the codec of the new messages against the frozen vectors of
// tests/rich-editor-save-vectors.txt (written by the Rust tests). Part B runs
// the scenarios of the story: text files saved back byte for byte (BOM,
// encodings, line breaks), DOCX round trip over the corpus without a write
// notice, revisions and comments (read-only, accepted state, editable copy),
// new documents, pictures, limits.
//
// The DOCX corpus of crates/fmt-ooxml/tests is exported to target/us218-corpus
// by `cargo test -p atelier-fmt-ooxml --test docx_editor_corpus`; without it
// only the documents of tests/ are replayed and the run says so.
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import {
  decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, RICH_LIMITS, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const hex = (b) => Buffer.from(b).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));

// ---- Part A: vectors --------------------------------------------------------
{
  const rows = readFileSync(here("rich-editor-save-vectors.txt"), "utf8")
    .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
  const counts = { request: 0, reply: 0, "bad-request": 0, "bad-reply": 0 };
  const stampText = (c) => `now_ms=${c.nowMs} user=${hex(new TextEncoder().encode(c.user))} version=${hex(new TextEncoder().encode(c.version))}`;
  const note = (n) => `${n.code.toString(16).padStart(4, "0")}:${hex(new TextEncoder().encode(n.text))}`;
  for (const [kind, name, frameHex, expected] of rows) {
    counts[kind] += 1;
    const bytes = fromHex(frameHex);
    if (kind === "request") {
      const d = decodeRichRequest(bytes);
      const c = d.command;
      const tail = {
        save: () => `Save format=${["", "txt", "docx", "docm", "dotx"].indexOf(c.format)} ${stampText(c)}`,
        insertImage: () => `InsertImage alt=${hex(new TextEncoder().encode(c.alt))} bytes=${hex(c.bytes)}`,
        makeEditableCopy: () => `MakeEditableCopy ${stampText(c)}`,
      }[c.type]();
      assert.equal(`doc=${d.docId} req=${d.requestId} ${tail}`, expected, name);
      assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    } else if (kind === "reply") {
      const d = decodeRichReply(bytes);
      const r = d.response;
      const tail = r.type === "saveData"
        ? `SaveData revision=${r.revision} bytes=${hex(r.bytes)} notices=${r.notices.length ? r.notices.map(note).join(",") : "none"}`
        : `Error code=${r.code}`;
      assert.equal(`doc=${d.docId} req=${d.requestId} ${tail}`, expected, name);
      assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
    } else {
      const decode = kind === "bad-request" ? decodeRichRequest : decodeRichReply;
      assert.throws(() => decode(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
    }
  }
  assert.deepEqual(counts, { request: 7, reply: 6, "bad-request": 18, "bad-reply": 8 });
  // Truncation of every valid frame is a typed error.
  for (const [kind, , frameHex] of rows.filter((r) => r[0] === "request" || r[0] === "reply")) {
    const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
    const bytes = fromHex(frameHex);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decode(bytes.subarray(0, cut)), RichProtocolError);
  }
  // The flag bits of State.
  const state = { type: "state", revision: 1n, anchor: 0n, focus: 0n, docLen: 0n, focusUpstream: false, modified: false, canUndo: false, canRedo: false, layoutComplete: true, pageCount: 0, atlas: { epoch: 1, first: 0, entries: [] }, caret: null, invalidated: [], pages: [], window: null };
  for (const [readOnly, hasRevisions] of [[false, false], [true, false], [false, true], [true, true]]) {
    const back = decodeRichReply(encodeRichReply({ docId: 1, requestId: 1, response: { ...state, readOnly, hasRevisions } })).response;
    assert.deepEqual([back.readOnly, back.hasRevisions], [readOnly, hasRevisions]);
  }
  assert.equal(RICH_LIMITS.maxSaveBytes, 32 * 1024 * 1024 - 20 - 14 - 64 * 259);
  console.log(`save protocol: ${rows.length} vectors verified`);
}

// ---- Part B: the real engine ------------------------------------------------
const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const wasm = await readFile(wasmPath);
const instance = new WebAssembly.Instance(await WebAssembly.compile(wasm));
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
const state = (command) => {
  const r = send(command);
  assert.equal(r.type, "state", JSON.stringify(r.code ?? r.type));
  return r;
};
const errorOf = (command, doc = 1) => {
  const r = send(command, doc);
  assert.equal(r.type, "error", `expected an error, got ${r.type}`);
  return r.code;
};
const open = (kind, profile, data = new Uint8Array()) => state({ type: "open", kind, profile, data });
const openTxt = (bytes) => open("txt", "txt", bytes);
const openDocx = (bytes) => open("docx", "document", bytes);
const text = () => {
  const r = send({ type: "textWindow", start: 0n, len: 262144 });
  assert.equal(r.type, "textWindow");
  return r.text;
};
const select = (anchor, focus = anchor) => state({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false });
const insert = (t) => state({ type: "insertText", text: t });
const STAMP = { nowMs: 1_700_000_000_000n, user: "Alice", version: "1.2" };
const save = (format) => {
  const r = send({ type: "save", format, ...STAMP });
  assert.equal(r.type, "saveData", JSON.stringify(r.code ?? r.type));
  return { bytes: r.bytes.slice(), notices: r.notices, revision: r.revision };
};
const codes = (notices) => notices.map((n) => n.code);
const utf8 = (s) => new TextEncoder().encode(s);

let checks = 0;
const check = (name, run) => { run(); checks += 1; };

// ---- Text files ---------------------------------------------------------------
const CP1252 = new Map([["é", 0xe9], ["è", 0xe8], ["à", 0xe0], ["€", 0x80]]);
function fileBytes(textLf, eol, encoding, bom) {
  const t = textLf.replaceAll("\n", eol);
  if (encoding === "utf8") return Buffer.concat([Buffer.from(bom ? [0xef, 0xbb, 0xbf] : []), Buffer.from(t, "utf8")]);
  if (encoding === "utf16le") return Buffer.concat([Buffer.from(bom ? [0xff, 0xfe] : []), Buffer.from(t, "utf16le")]);
  if (encoding === "utf16be") {
    const le = Buffer.from(t, "utf16le");
    for (let i = 0; i + 1 < le.length; i += 2) { const a = le[i]; le[i] = le[i + 1]; le[i + 1] = a; }
    return Buffer.concat([Buffer.from(bom ? [0xfe, 0xff] : []), le]);
  }
  return Buffer.from([...t].map((c) => CP1252.get(c) ?? c.charCodeAt(0)));
}
const CORPUS_TEXT = "Premier été, à 5€\n\nTroisième ligne\tavec tabulation\nfin";
const textCases = [];
for (const [eolName, eol] of [["lf", "\n"], ["crlf", "\r\n"], ["cr", "\r"]]) {
  for (const [encoding, bom] of [["utf8", false], ["utf8", true], ["utf16le", true], ["utf16be", true], ["cp1252", false]]) {
    for (const finalBreak of [false, true]) {
      textCases.push({ name: `${encoding}-bom${bom}-${eolName}-final${finalBreak}`, encoding, bom, eol, finalBreak });
    }
  }
}

check("text files: unmodified, then edited, saved byte for byte", () => {
  for (const { name, encoding, bom, eol, finalBreak } of textCases) {
    const original = fileBytes(CORPUS_TEXT + (finalBreak ? "\n" : ""), eol, encoding, bom);
    openTxt(original);
    const saved = save("txt");
    assert.equal(hex(saved.bytes), hex(original), `${name}: unmodified`);
    assert.deepEqual(saved.notices, [], name);
    // The client confirms the save: the document is no longer modified.
    assert.equal(state({ type: "markSaved", revision: saved.revision }).modified, false, name);
    select(7);
    insert("!");
    const end = BigInt(utf8(text()).length);
    select(end);
    state({ type: "delete", forward: false, unit: "grapheme" });
    let expected = CORPUS_TEXT.slice(0, 7) + "!" + CORPUS_TEXT.slice(7) + (finalBreak ? "\n" : "");
    expected = expected.slice(0, -1);
    const edited = save("txt");
    assert.equal(hex(edited.bytes), hex(fileBytes(expected, eol, encoding, bom)), `${name}: edited`);
    assert.deepEqual(edited.notices, [], name);
  }
});

check("text files: mixed breaks, unmappable characters, new document", () => {
  const mixed = utf8("a\r\nb\r\nc\nd\re");
  openTxt(mixed);
  let s = save("txt");
  assert.equal(hex(s.bytes), hex(mixed), "each line keeps its own break");
  assert.deepEqual(s.notices, []);
  select(1);
  insert("!");
  assert.equal(Buffer.from(save("txt").bytes).toString(), "a!\r\nb\r\nc\nd\re");
  openTxt(Uint8Array.from([0x61, 0xe9, 0x0a]));
  select(0);
  insert("中");
  s = save("txt");
  assert.deepEqual([...s.bytes], [0x3f, 0x61, 0xe9, 0x0a]);
  assert.deepEqual(codes(s.notices), [0x0210]);
  open("blank", "document");
  insert("Bonjour\nété");
  s = save("txt");
  assert.equal(Buffer.from(s.bytes).toString("utf8"), "Bonjour\r\nété", "UTF-8 without BOM, CRLF");
  assert.equal(s.bytes[0], 0x42);
  open("blank", "txt");
  assert.equal(save("txt").bytes.length, 0);
});

// ---- DOCX ---------------------------------------------------------------------
const docxFiles = [];
const addDir = (dir, prefix) => {
  if (!existsSync(dir)) return false;
  for (const name of readdirSync(dir).sort()) {
    if (name.endsWith(".docx")) docxFiles.push({ name: `${prefix}${name}`, bytes: new Uint8Array(readFileSync(new URL(name, dir))) });
  }
  return true;
};
addDir(here("docx-reference/"), "tests/docx-reference/");
addDir(here("docx-reference/libreoffice/"), "tests/docx-reference/libreoffice/");
const corpusDir = here("../target/us218-corpus/");
const hasCorpus = addDir(corpusDir, "crates/fmt-ooxml corpus: ");
if (!hasCorpus) console.warn("warning: target/us218-corpus is missing (run the docx_editor_corpus test): the fmt-ooxml corpus was not replayed");
const fixtures = new Set(["revisions.docx", "comments.docx"]);

check("docx: every corpus file saves back without a notice, unmodified and edited", () => {
  let replayed = 0;
  for (const { name, bytes } of docxFiles) {
    const base = name.split("/").pop();
    if (fixtures.has(base)) continue;
    const opened = openDocx(bytes);
    const before = text();
    const saved = save("docx");
    assert.deepEqual(saved.notices, [], `${name}: unmodified`);
    const again = openDocx(saved.bytes);
    assert.equal(again.readOnly, opened.readOnly, name);
    assert.equal(text(), before, `${name}: text after the round trip`);
    if (!opened.readOnly) {
      select(0);
      insert("Ajout ");
      const edited = save("docx");
      // A plain insertion in a paragraph that has a skeleton is not a loss.
      assert.deepEqual(edited.notices, [], `${name}: edited`);
      openDocx(edited.bytes);
      assert.ok(text().startsWith("Ajout "), `${name}: the edit is in the file`);
    }
    replayed += 1;
  }
  assert.ok(replayed >= (hasCorpus ? 6 : 3), `replayed ${replayed} documents`);
});

check("text files: repository files and edge cases", () => {
  const u16 = (t, bom) => Buffer.concat([Buffer.from(bom ? [0xff, 0xfe] : []), Buffer.from(t, "utf16le")]);
  const real = ["mixed-utf8.txt", "repetitive.txt"].map((n) => [n, new Uint8Array(readFileSync(new URL(`../crates/compress/tests/corpus/${n}`, import.meta.url)))]);
  const cases = [
    ...real,
    ["empty", new Uint8Array()],
    ["bom only", Uint8Array.from([0xef, 0xbb, 0xbf])],
    ["utf16 bom only", Uint8Array.from([0xff, 0xfe])],
    ["utf16 without bom", u16("Bonjour le monde, ligne un\nligne deux\n", false)],
    ["outside the BMP, utf8", utf8("pile \u{1f600} face \u{1d11e}\n")],
    ["outside the BMP, utf16", u16("pile \u{1f600} face \u{1d11e}\n", true)],
    ["only breaks", utf8("\n\n\r\n\r")],
  ];
  for (const [name, bytes] of cases) {
    openTxt(bytes);
    const saved = save("txt");
    assert.equal(hex(saved.bytes), hex(bytes), name);
    assert.deepEqual(saved.notices, [], name);
  }
  for (const [name, bytes] of real) {
    openTxt(bytes);
    select(0);
    insert("!");
    assert.equal(hex(save("txt").bytes), hex(Buffer.concat([Buffer.from("!"), bytes])), name);
  }
});

check("properties: the client's time, user and version are stamped, not the engine's", () => {
  // A time out of range is refused; the document is untouched.
  open("blank", "document");
  insert("abc");
  assert.equal(errorOf({ type: "save", format: "docx", nowMs: 0xffffffffffffffffn, user: "", version: "" }), "RICH_INVALID_BODY");
  assert.equal(text(), "abc");
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "save", format: "docx", nowMs: 1n, user: "a".repeat(256), version: "" } }), (e) => e.code === "RICH_INVALID_TEXT");
  // Same document, two clocks: the files differ, so the time comes from the request.
  const one = save("docx").bytes;
  const other = send({ type: "save", format: "docx", nowMs: 1_800_000_000_000n, user: "Bob", version: "9" }).bytes.slice();
  assert.notEqual(hex(one), hex(other));
});

check("docx: new document saved as docx, docm and dotx", () => {
  open("blank", "document");
  insert("Titre\nCorps");
  for (const format of ["docx", "docm", "dotx"]) {
    const saved = save(format);
    assert.deepEqual(saved.notices, [], format);
    assert.equal(saved.bytes[0], 0x50, "PK");
    const reopened = openDocx(saved.bytes);
    assert.equal(reopened.readOnly, false);
    assert.equal(text(), "Titre\nCorps", format);
  }
});

const fixture = (name) => new Uint8Array(readFileSync(here(`docx-reference/${name}`)));
const EDITS = [
  { type: "insertText", text: "x" },
  { type: "replaceRange", start: 0n, end: 0n, text: "x" },
  { type: "delete", forward: true, unit: "grapheme" },
  { type: "undo" },
  { type: "redo" },
  { type: "insertImage", alt: "", bytes: Uint8Array.from([1, 2, 3]) },
];

check("revisions and comments: read-only, accepted state, editable copy", () => {
  const cases = [
    ["revisions.docx", "Début inséré fin.\n\ndéplacé\ngras\ncentré\nsuite"],
    ["comments.docx", "Avant commenté après.\nFin."],
  ];
  for (const [name, expected] of cases) {
    const s = openDocx(fixture(name));
    assert.equal(s.readOnly, true, name);
    assert.equal(s.hasRevisions, true, name);
    assert.equal(text(), expected, `${name}: all changes accepted, no mark of deleted text`);
    for (const command of EDITS) assert.equal(errorOf(command), "RICH_READ_ONLY", `${name} ${command.type}`);
    assert.equal(text(), expected, `${name}: untouched`);
    const moved = state({ type: "move", direction: "right", extend: false });
    assert.equal(moved.readOnly, true);
    assert.equal(moved.modified, false);
    // What the user would save is the file as opened.
    assert.deepEqual(save("docx").notices, [], name);
    const r = send({ type: "makeEditableCopy", ...STAMP });
    assert.equal(r.type, "saveData", name);
    assert.ok(r.notices.length > 0, `${name}: the copy comes with notices`);
    assert.ok(r.notices.every((n) => n.code >= 0x0300 && n.code < 0x0400), name);
    const copy = r.bytes.slice();
    const editable = openDocx(copy);
    assert.equal(editable.readOnly, false, name);
    assert.equal(editable.hasRevisions, false, name);
    assert.equal(text(), expected, `${name}: the copy shows the accepted text`);
    select(0);
    insert("Z");
    assert.equal(text(), `Z${expected}`);
    assert.deepEqual(save("docx").notices, [], `${name}: the copy saves cleanly`);
    // The copy has no revision and no comment left: it opens editable again.
    assert.equal(openDocx(save("docx").bytes).readOnly, false);
  }
  // The libreoffice document holds real tracked changes and comments.
  const rich = docxFiles.find((f) => f.name.endsWith("libreoffice/rich.docx"));
  assert.equal(openDocx(rich.bytes).readOnly, true);
  assert.equal(errorOf(EDITS[0]), "RICH_READ_ONLY");
  // A document without any is editable.
  assert.equal(openDocx(fixture("basic.docx")).readOnly, false);
});

// ---- Pictures -------------------------------------------------------------------
const png = new Uint8Array(readFileSync(new URL("../crates/image/tests/fixtures/png/browser-200x100-red.png", import.meta.url)));
const jpeg = new Uint8Array(readFileSync(new URL("../crates/image/tests/fixtures/jpeg/baseline-420.jpg", import.meta.url)));
const mediaNames = (docx) => {
  const names = [];
  const view = new DataView(docx.buffer, docx.byteOffset, docx.byteLength);
  // Central directory entries: signature 0x02014b50, name length at +28, name at +46.
  for (let i = 0; i + 46 <= docx.length; i += 1) {
    if (view.getUint32(i, true) === 0x02014b50) {
      const len = view.getUint16(i + 28, true);
      names.push(Buffer.from(docx.subarray(i + 46, i + 46 + len)).toString());
    }
  }
  return names.filter((n) => n.startsWith("word/media/"));
};

check("pictures: inserted as objects, written to word/media, dropped from text with a notice", () => {
  open("blank", "document");
  insert("ab");
  select(1);
  const s = state({ type: "insertImage", alt: "Un carré rouge", bytes: png });
  assert.equal(s.docLen, 5n);
  assert.deepEqual([s.anchor, s.focus], [4n, 4n]);
  assert.equal(s.modified, true);
  assert.equal(text(), "a￼b");
  state({ type: "insertImage", alt: "", bytes: jpeg });
  assert.equal(text(), "a￼￼b");
  const saved = save("docx");
  assert.deepEqual(saved.notices, []);
  const media = mediaNames(saved.bytes);
  assert.equal(media.length, 2, JSON.stringify(media));
  assert.ok(media.some((n) => n.endsWith(".png")) && media.some((n) => /\.jpe?g$/.test(n)), JSON.stringify(media));
  const t = save("txt");
  assert.equal(Buffer.from(t.bytes).toString(), "ab");
  assert.ok(t.notices.some((n) => n.code === 0x0204), "objects are reported as lost in the text save");
  assert.equal(state({ type: "undo" }).docLen, 5n);
  assert.equal(state({ type: "redo" }).docLen, 8n);
  // The picture is read back from the file as an object.
  openDocx(saved.bytes);
  assert.equal(text().split("￼").length - 1, 2);
});

check("pictures: refusals leave the document alone", () => {
  open("blank", "document");
  insert("abc");
  assert.equal(errorOf({ type: "insertImage", alt: "", bytes: Uint8Array.from([1, 2, 3, 4]) }), "RICH_IMAGE_INVALID");
  assert.equal(errorOf({ type: "insertImage", alt: "", bytes: png.subarray(0, 40) }), "RICH_IMAGE_INVALID");
  assert.equal(text(), "abc");
  // An image over 8 MiB is refused by the codec, before the engine.
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "insertImage", alt: "", bytes: new Uint8Array(RICH_LIMITS.maxImageBytes + 1).fill(1) } }), (e) => e.code === "RICH_LIMIT");
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "insertImage", alt: "a".repeat(513), bytes: png } }), (e) => e.code === "RICH_INVALID_TEXT");
  assert.equal(errorOf({ type: "save", format: "txt" }, 2), "RICH_UNKNOWN_DOCUMENT");
});


console.log(`rich editor save: ${checks} scenario groups passed (${docxFiles.length} DOCX files, ${textCases.length} text files)`);
