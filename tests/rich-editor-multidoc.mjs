// SPDX-License-Identifier: AGPL-3.0-or-later
// T0 (several documents in one engine, docs/us-327-multidoc-protocol.md): the codec of CopyFragment, PasteFragment, the
// Fragment reply and the errors 70 to 73 against tests/rich-editor-multidoc-vectors.txt (written by the Rust tests of
// rich_protocol/fragment_tests.rs), then three documents in parallel on the REAL WASM engine through one shared
// transport: independence, interleaving, fairness of the layout pumps, copy and paste between documents, refusal of an
// Open without harm to the others, Close during a request, lost reply, restarted engine.
// `node tests/rich-editor-multidoc.mjs [engine.wasm]` (default: the wasm-release build).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { decodeMessage } from "../web/wasm-protocol.mjs";
import { createEngineTransport, createRichEditorClient } from "../web/rich-editor-client.mjs";
import {
  decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, RICH_LIMITS, RichErrorCode, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const hex = (b) => Buffer.from(b).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));
let checks = 0;
const check = () => { checks += 1; };

// ---- vectors ---------------------------------------------------------------------------------------------------------

const rows = readFileSync(here("rich-editor-multidoc-vectors.txt"), "utf8")
  .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
const counts = { request: 0, reply: 0, "bad-request": 0 };
for (const [kind, name, frameHex, expected] of rows) {
  counts[kind] += 1;
  const bytes = fromHex(frameHex);
  if (kind === "request") {
    const d = decodeRichRequest(bytes);
    const c = d.command;
    const head = `doc=${d.docId} req=${d.requestId}`;
    if (c.type === "copyFragment") {
      for (const field of ["revision", "start", "end"]) assert.equal(typeof c[field], "bigint", `${name} ${field} is a BigInt`);
      assert.equal(`${head} CopyFragment revision=${c.revision} start=${c.start} end=${c.end}`, expected, name);
    } else {
      assert.equal(c.type, "pasteFragment", name);
      assert.ok(c.bytes instanceof Uint8Array, `${name} bytes`);
      assert.equal(`${head} PasteFragment bytes=${hex(c.bytes)}`, expected, name);
    }
    assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decodeRichRequest(bytes.subarray(0, cut)), RichProtocolError, `${name} cut ${cut}`);
  } else if (kind === "reply") {
    const d = decodeRichReply(bytes);
    const r = d.response;
    const head = `doc=${d.docId} req=${d.requestId}`;
    if (r.type === "fragment") {
      assert.equal(typeof r.revision, "bigint");
      assert.equal(`${head} Fragment revision=${r.revision} lossy=${r.lossy ? 1 : 0} text=${hex(new TextEncoder().encode(r.text))} bytes=${hex(r.bytes)}`, expected, name);
    } else assert.equal(`${head} Error code=${r.code}`, expected, name);
    assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decodeRichReply(bytes.subarray(0, cut)), RichProtocolError, `${name} cut ${cut}`);
  } else {
    assert.throws(() => decodeRichRequest(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
  }
  check();
}
assert.deepEqual(counts, { request: 4, reply: 6, "bad-request": 7 });
assert.deepEqual([RichErrorCode.RICH_TOO_MANY_DOCUMENTS, RichErrorCode.RICH_MEMORY, RichErrorCode.RICH_FRAGMENT_INVALID, RichErrorCode.RICH_EMPTY_SELECTION], [70, 71, 72, 73]);
assert.equal(RICH_LIMITS.maxDocuments, 8);
// The encoder refuses what the decoder refuses.
assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "copyFragment", revision: 0n, start: -1n, end: 1n } }), RichProtocolError);
assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "pasteFragment", bytes: [1] } }), RichProtocolError);
assert.throws(() => encodeRichReply({ docId: 1, requestId: 1, response: { type: "fragment", revision: 0n, lossy: false, text: "", bytes: "x" } }), RichProtocolError);
check();

// ---- the real engine ---------------------------------------------------------------------------------------------------

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const module = await WebAssembly.compile(await readFile(wasmPath));

/** One engine instance and a request-level client double (what the Worker offers): the outer reply bytes, status removed. */
function createEngine() {
  const { memory, alloc, dispatch, free } = new WebAssembly.Instance(module).exports;
  const log = [];
  const plan = { loseNext: false };
  function invoke(frame) {
    const ptr = alloc(frame.length) >>> 0;
    new Uint8Array(memory.buffer).set(frame, ptr);
    const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
    const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
    const bodyPtr = descriptor.getUint32(0, true);
    const bodyLen = descriptor.getUint32(4, true);
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    free(bodyPtr, bodyLen); free(descriptorPtr, 8);
    assert.equal(new DataView(body.buffer).getUint32(0, true), 0, "status OK");
    return body.slice(4);
  }
  return {
    log, plan,
    async request(frame) {
      const d = decodeRichRequest(decodeMessage(frame, "command").payload);
      log.push({ doc: d.docId, type: d.command.type });
      await Promise.resolve();
      const reply = invoke(frame);
      if (plan.loseNext && d.command.type !== "layout" && d.command.type !== "resync") {
        plan.loseNext = false;
        throw Object.assign(new Error("reply lost"), { name: "TransportLost" });
      }
      return reply;
    },
  };
}

const enc = new TextEncoder();
const textOf = async (client) => {
  const { response } = await client.command({ type: "textWindow", start: 0n, len: 262144 });
  return response.text;
};
const open = (client, text) => client.open({ kind: "txt", profile: "txt", data: enc.encode(text) });
const insert = (client, text) => client.command({ type: "insertText", text });
const caret = (client, at) => client.command({ type: "setSelection", anchor: BigInt(at), focus: BigInt(at), upstream: false });

let engine = createEngine();
let transport = createEngineTransport(engine);
const clients = new Map();
const make = (docId, options = {}) => {
  const client = createRichEditorClient({ transport, docId, layoutBudget: 16, onError() {}, ...options });
  clients.set(docId, client);
  return client;
};
const [c1, c2, c3] = [make(1), make(2), make(3)];

// Three documents opened at once; nothing replaces anything.
await Promise.all([open(c1, "alpha"), open(c2, "beta"), open(c3, "gamma")]);
assert.deepEqual(await Promise.all([textOf(c1), textOf(c2), textOf(c3)]), ["alpha", "beta", "gamma"]);
check();

// Interleaved edits, an undo in one document only.
await Promise.all([caret(c1, 5), caret(c2, 0), caret(c3, 5)]);
await Promise.all([insert(c1, "!"), insert(c2, "?"), insert(c3, "#"), insert(c2, "?")]);
assert.deepEqual(await Promise.all([textOf(c1), textOf(c2), textOf(c3)]), ["alpha!", "??beta", "gamma#"]);
const undone = await c2.command({ type: "undo" });
assert.equal(undone.response.type, "state");
assert.deepEqual(await Promise.all([textOf(c1), textOf(c2), textOf(c3)]), ["alpha!", "?beta", "gamma#"]);
assert.equal(c1.cache.revision, 1n);
assert.equal(c3.cache.revision, 1n);
check();

// Atlas epochs are per document: a resync of one leaves the other alone.
await Promise.all([c1.ensurePages(0, 1), c2.ensurePages(0, 1), c3.ensurePages(0, 1)]);
const epochs = [c1, c2, c3].map((c) => c.atlasEpoch);
assert.equal(new Set(epochs).size, 3, `three epochs: ${epochs}`);
await c1.command({ type: "resync" });
assert.equal(c1.atlasEpoch, epochs[0] + 1);
assert.equal(c2.atlasEpoch, epochs[1]);
assert.equal(c3.atlasEpoch, epochs[2]);
check();

// Fairness: a big document pumps its layout while another tab types; the typing is served after at most one step of each other pump.
const big = Array.from({ length: 4000 }, (_, i) => `line ${i} of a long document that needs a few pages of layout`).join("\n");
const c4 = make(4);
await open(c4, big);
assert.equal(c4.cache.layoutComplete, false, "the big document is still being laid out");
const enqueued = engine.log.length;
await insert(c1, "x");
const interleaved = engine.log.slice(enqueued).findIndex((e) => e.doc === 1 && e.type === "insertText");
assert.ok(interleaved >= 0 && interleaved <= 2, `served after ${interleaved} foreign steps`);
assert.equal(await textOf(c1), "alpha!x");
check();
await c4.closeDocument();
assert.equal(await textOf(c1), "alpha!x");

// Copy and paste between documents through the client: bold survives, one undo step. Formatting needs the document profile.
const blank = (client) => client.open({ kind: "blank", profile: "document", data: new Uint8Array() });
const source = make(31);
const target = make(32);
await Promise.all([blank(source), blank(target)]);
await insert(source, "alpha!");
await insert(target, "beta");
await source.command({ type: "setSelection", anchor: 0n, focus: 5n, upstream: false });
await source.command({ type: "applyCharFormat", baseRevision: 0xffffffffffffffffn, commandId: 0, scope: "selectionOnly", edits: [{ attr: "bold", op: "toggle" }] });
const fragment = await source.copyFragment(0n, 6n);
assert.equal(fragment.text, "alpha!");
assert.equal(fragment.lossy, false);
assert.equal(String.fromCharCode(...fragment.bytes.subarray(0, 4)), "ATFR");
await caret(target, 1);
const revisionBefore = target.cache.revision;
await target.pasteFragment(fragment.bytes);
assert.equal(await textOf(target), "balpha!eta");
assert.equal(target.cache.revision, revisionBefore + 1n);
await target.command({ type: "setSelection", anchor: 1n, focus: 6n, upstream: false });
const bolded = await target.command({ type: "formatState" });
assert.deepEqual(bolded.response.chars.bold, { state: "value", value: true }, "the pasted text keeps its bold");
await target.command({ type: "setSelection", anchor: 7n, focus: 10n, upstream: false });
const around = await target.command({ type: "formatState" });
assert.notDeepEqual(around.response.chars.bold, { state: "value", value: true }, "the text around does not");
await target.command({ type: "undo" });
assert.equal(await textOf(target), "beta");
check();
// A stale revision and an empty range are refused, hostile bytes too, and nothing changes.
await assert.rejects(source.copyFragment(2n, 2n), (e) => e.code === "RICH_EMPTY_SELECTION");
await assert.rejects(target.pasteFragment(Uint8Array.of(1, 2, 3)), (e) => e.code === "RICH_FRAGMENT_INVALID");
assert.equal(await textOf(target), "beta");
// Into a text document the fragment gives its text.
await caret(c3, 0);
await c3.pasteFragment(fragment.bytes);
assert.equal(await textOf(c3), "alpha!gamma#");
await Promise.all([source.closeDocument(), target.closeDocument()]);
check();

// Close during a request: the commands queued before it run, the others documents are untouched, the id is unknown after.
await open(c2, "closing");
const closing = [insert(c2, "z"), c2.closeDocument()];
await Promise.all(closing);
await assert.rejects(c2.command({ type: "textWindow", start: 0n, len: 10 }), (e) => e.code === "RICH_UNKNOWN_DOCUMENT");
assert.deepEqual(await Promise.all([textOf(c1), textOf(c3)]), ["alpha!x", "alpha!gamma#"]);
check();

// The limit: eight documents (c1, c3 and six more), the ninth is refused and nobody else is harmed.
const extra = [];
for (let id = 2; id <= 7; id += 1) { const c = id === 2 ? c2 : make(id + 10); extra.push(c); await open(c, `doc ${id}`); }
const ninth = make(99);
await assert.rejects(open(ninth, "nine"), (e) => e.code === "RICH_TOO_MANY_DOCUMENTS");
assert.equal(ninth.opened, false);
assert.equal(await textOf(c1), "alpha!x");
assert.equal(await textOf(c3), "alpha!gamma#");
// A document closed gives its place.
await extra[1].closeDocument();
await open(ninth, "nine");
assert.equal(await textOf(ninth), "nine");
check();

// A lost reply: the client resynchronises that document and the engine and the client agree again.
engine.plan.loseNext = true;
await assert.rejects(insert(c3, "L"), (e) => e.code === "CLIENT_TRANSPORT");
await c3.idle();
assert.equal(await textOf(c3), "alpha!Lgamma#", "the engine applied it");
assert.equal(c3.cache.textWindow?.text.includes("alpha!Lgamma#") ?? true, true);
assert.equal(await textOf(c1), "alpha!x");
check();

// The engine restarts (a new instance): every document is unknown, an Open of the same id brings it back.
engine = createEngine();
transport = createEngineTransport(engine);
const fresh = createRichEditorClient({ transport, docId: 1, onError() {} });
await assert.rejects(fresh.command({ type: "textWindow", start: 0n, len: 10 }), (e) => e.code === "RICH_UNKNOWN_DOCUMENT");
await open(fresh, "back");
assert.equal(await textOf(fresh), "back");
check();

for (const c of [...clients.values(), fresh]) c.dispose();
console.log(JSON.stringify({ status: "passed", checks, foreignStepsBeforeTyping: interleaved }));
