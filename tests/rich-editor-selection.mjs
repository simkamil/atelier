// SPDX-License-Identifier: AGPL-3.0-or-later
// R7b-1: selection highlight, drawn pictures and word or paragraph selection of the rich editor.
// `node tests/rich-editor-selection.mjs [engine.wasm]` (default: the wasm-release build of
// `python tools/package-engine.py`).
//
// A: the codec of the new messages against tests/rich-editor-selection-vectors.txt (written by the
//    Rust tests of rich_protocol/sel_tests.rs).
// B: the client against an engine double: stale revisions, cancellation, one request per State,
//    the viewport, the pictures it asks for.
// C: the bitmap cache of the pictures, bounded in bytes.
// D: the real engine through the codec: rectangles over several pages, a picture in a page and
//    its bitmap, word and paragraph selection.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { decodeDisplayList } from "../web/display-list.mjs";
import { createRichImageCache } from "../web/rich-image-cache.mjs";
import { createRichEditorClient } from "../web/rich-editor-client.mjs";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import {
  decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, RICH_LIMITS, RichErrorCode, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const hex = (b) => Buffer.from(b).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));
const bits = (v) => { const b = Buffer.alloc(8); b.writeDoubleLE(v); return b.readBigUInt64LE().toString(16).padStart(16, "0"); };
const turn = () => new Promise((resolve) => setImmediate(resolve));
const abortError = () => new DOMException("The operation was aborted.", "AbortError");
let checks = 0;
const check = async (name, run) => { try { await run(); checks += 1; } catch (error) { console.error(`not ok - ${name}`); throw error; } };

// ---- A: vectors ------------------------------------------------------------------------------
{
  const rows = readFileSync(here("rich-editor-selection-vectors.txt"), "utf8")
    .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
  const counts = { request: 0, reply: 0, "bad-request": 0, "bad-reply": 0 };
  for (const [kind, name, frameHex, expected] of rows) {
    counts[kind] += 1;
    const bytes = fromHex(frameHex);
    if (kind === "request") {
      const d = decodeRichRequest(bytes);
      const c = d.command;
      const tail = {
        selectionRects: () => `SelectionRects first=${c.first} count=${c.count}`,
        imageBitmap: () => `ImageBitmap object=${c.objectId}`,
        selectUnit: () => `SelectUnit position=${c.position} unit=${["word", "paragraph"].indexOf(c.unit)} extend=${c.extend ? 1 : 0}`,
      }[c.type]();
      assert.equal(`doc=${d.docId} req=${d.requestId} ${tail}`, expected, name);
      assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    } else if (kind === "reply") {
      const d = decodeRichReply(bytes);
      const r = d.response;
      const tail = r.type === "selectionRects"
        ? `SelectionRects revision=${r.revision} rects=${r.rects.length ? r.rects.map((q) => `${q.page}:${bits(q.x)}:${bits(q.y)}:${bits(q.width)}:${bits(q.height)}`).join(",") : "none"}`
        : r.type === "imageBitmap"
          ? `ImageBitmap object=${r.objectId} width=${r.width} height=${r.height} rgba=${hex(r.rgba)}`
          : `Error code=${r.code}`;
      assert.equal(`doc=${d.docId} req=${d.requestId} ${tail}`, expected, name);
      assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
    } else {
      const decode = kind === "bad-request" ? decodeRichRequest : decodeRichReply;
      assert.throws(() => decode(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
    }
  }
  assert.deepEqual(counts, { request: 5, reply: 4, "bad-request": 11, "bad-reply": 13 });
  for (const [kind, , frameHex] of rows.filter((r) => r[0] === "request" || r[0] === "reply")) {
    const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
    const bytes = fromHex(frameHex);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decode(bytes.subarray(0, cut)), RichProtocolError);
  }
  // The decoded shapes are the documented ones (concrete JS types).
  const decoded = decodeRichReply(fromHex(rows.find((r) => r[0] === "reply" && r[1] === "selection-rects")[2])).response;
  assert.equal(typeof decoded.revision, "bigint");
  assert.deepEqual(Object.keys(decoded.rects[0]), ["page", "x", "y", "width", "height"]);
  assert.ok(decoded.rects.every((q) => [q.page, q.x, q.y, q.width, q.height].every((v) => typeof v === "number")));
  const bitmap = decodeRichReply(fromHex(rows.find((r) => r[1] === "image-bitmap" && r[0] === "reply")[2])).response;
  assert.equal(typeof bitmap.objectId, "bigint");
  assert.ok(bitmap.rgba instanceof Uint8Array && bitmap.rgba.length === bitmap.width * bitmap.height * 4);
  assert.equal(RichErrorCode.RICH_UNKNOWN_OBJECT, 30);
  assert.equal(RICH_LIMITS.maxSelectionRects, 4096);
  // The encoder refuses what the decoder refuses.
  const reply = (response) => () => encodeRichReply({ docId: 1, requestId: 1, response });
  assert.throws(reply({ type: "selectionRects", revision: 1n, rects: Array.from({ length: 4097 }, () => ({ page: 0, x: 0, y: 0, width: 1, height: 1 })) }), RichProtocolError);
  assert.throws(reply({ type: "selectionRects", revision: 1n, rects: [{ page: 0, x: 0, y: 0, width: -1, height: 1 }] }), RichProtocolError);
  assert.throws(reply({ type: "selectionRects", revision: 1n, rects: [{ page: 0, x: NaN, y: 0, width: 1, height: 1 }] }), RichProtocolError);
  assert.throws(reply({ type: "imageBitmap", objectId: 1n, width: 2, height: 2, rgba: new Uint8Array(15) }), RichProtocolError);
  assert.throws(reply({ type: "imageBitmap", objectId: 1n, width: 1601, height: 1, rgba: new Uint8Array(1601 * 4) }), RichProtocolError);
  console.log(`selection protocol: ${rows.length} vectors verified`);
}

// ---- B: the client against an engine double ----------------------------------------------------
const atdl = () => { const b = new Uint8Array(44); b.set([0x41, 0x54, 0x44, 0x4c]); b[4] = 1; return b; };

/** Engine double: 3 pages, a selection and a revision the test moves, rectangles held on demand. */
function makeDouble() {
  const e = { rev: 1n, anchor: 0n, focus: 0n, open: false, rectRequests: [], bitmapRequests: [], holdRects: null, rectRevision: null, unknown: new Set() };
  e.state = () => ({
    type: "state", revision: e.rev, anchor: e.anchor, focus: e.focus, focusUpstream: false, docLen: 1000n, modified: false,
    canUndo: false, canRedo: false, layoutComplete: true, pageCount: 3, caret: null, invalidated: [],
    atlas: { epoch: 1, first: 0, entries: [] }, pages: [], window: null, readOnly: false, hasRevisions: false,
  });
  e.handle = (request) => {
    const c = request.command;
    switch (c.type) {
      case "open": e.open = true; return { ...e.state(), pages: [0, 1, 2].map((index) => ({ index, display: atdl() })) };
      case "pages": return { type: "pages", revision: e.rev, pageCount: 3, atlas: { epoch: 1, first: 0, entries: [] }, pages: Array.from({ length: c.count }, (_, i) => ({ index: c.first + i, display: atdl() })) };
      case "setSelection": e.anchor = c.anchor; e.focus = c.focus; return e.state();
      case "insertText": e.rev += 1n; e.anchor = e.focus = e.focus + BigInt(c.text.length); return e.state();
      case "selectionRects":
        e.rectRequests.push({ first: c.first, count: c.count, revision: e.rev });
        return { type: "selectionRects", revision: e.rectRevision ?? e.rev, rects: e.anchor === e.focus ? [] : [{ page: c.first, x: 72, y: Number(e.rev), width: 100, height: 14 }] };
      case "imageBitmap": {
        e.bitmapRequests.push(c.objectId);
        if (e.unknown.has(c.objectId)) return { type: "error", code: "RICH_UNKNOWN_OBJECT" };
        return { type: "imageBitmap", objectId: c.objectId, width: 2, height: 1, rgba: Uint8Array.of(1, 2, 3, 255, 4, 5, 6, 255) };
      }
      default: return e.state();
    }
  };
  return e;
}

function setupClient(engine, options = {}) {
  const log = { signals: [], published: [], bitmaps: [], errors: [] };
  const gate = { hold: false, release: null, held: null };
  const transport = {
    async request(frame, { signal } = {}) {
      if (signal?.aborted) throw abortError();
      const req = decodeRichRequest(frame);
      if (req.command.type === "selectionRects") log.signals.push(signal);
      const bytes = encodeRichReply({ docId: 1, requestId: req.requestId, response: engine.handle(req) });
      if (req.command.type === "selectionRects" && gate.hold) {
        gate.held = req;
        await new Promise((resolve, reject) => {
          gate.release = resolve;
          signal?.addEventListener("abort", () => reject(abortError()), { once: true });
        });
      }
      return bytes;
    },
  };
  const held = new Map();
  const client = createRichEditorClient({
    transport, docId: 1, yieldTurn: turn, onError: (error) => log.errors.push(error),
    onSelectionRects: (highlight) => log.published.push(highlight),
    hasImage: (id) => held.has(id),
    onImageBitmap: (bitmap) => { held.set(bitmap.objectId, bitmap); log.bitmaps.push(bitmap); },
    ...options,
  });
  return { client, transport, log, gate, held };
}

const OPEN = { kind: "blank", profile: "document", data: new Uint8Array(0) };
const select = (client, anchor, focus) => client.command({ type: "setSelection", anchor, focus, upstream: false });

await check("an empty selection asks nothing and publishes nothing", async () => {
  const engine = makeDouble();
  const { client, log } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(0, 2);
  await select(client, 5n, 5n);
  await client.idle();
  assert.equal(engine.rectRequests.length, 0);
  assert.deepEqual(log.published, []);
  client.dispose();
});

await check("a non-empty selection is asked once, for the visible pages, and published with its revision", async () => {
  const engine = makeDouble();
  const { client, log } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(1, 3);
  await select(client, 2n, 9n);
  await client.idle();
  assert.deepEqual(engine.rectRequests, [{ first: 1, count: 2, revision: 1n }]);
  assert.equal(log.published.length, 1);
  assert.equal(log.published[0].revision, 1n);
  assert.deepEqual(log.published[0].rects, [{ page: 1, x: 72, y: 1, width: 100, height: 14 }]);
  await turn(); await turn();
  assert.equal(engine.rectRequests.length, 1, "nothing more is sent while nothing changes");
  client.dispose();
});

await check("the highlight is emptied as soon as the selection is, with no request", async () => {
  const engine = makeDouble();
  const { client, log } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(0, 1);
  await select(client, 2n, 9n);
  await client.idle();
  assert.equal(log.published.length, 1);
  await select(client, 9n, 9n);
  assert.equal(log.published.length, 2, "published before the next turn");
  assert.deepEqual(log.published[1].rects, []);
  await client.idle();
  assert.equal(engine.rectRequests.length, 1);
  await select(client, 9n, 9n);
  await client.idle();
  assert.equal(log.published.length, 2, "an empty highlight is not published twice");
  client.dispose();
});

await check("a request in flight is cancelled by the next State and its late reply is never shown", async () => {
  const engine = makeDouble();
  const { client, log, gate } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(0, 1);
  gate.hold = true;
  await select(client, 0n, 4n);
  while (!gate.held) await turn();
  const first = log.signals[0];
  assert.equal(first.aborted, false);
  gate.hold = false;
  // The next State (an edit: revision 2, selection collapsed then extended) cancels the request.
  const edit = client.command({ type: "insertText", text: "xyz" });
  await edit;
  assert.equal(first.aborted, true, "the pending request was cancelled");
  await select(client, 0n, 3n);
  await client.idle();
  assert.deepEqual(log.published.map((h) => h.revision), [2n], "only the highlight of the newest State");
  assert.equal(engine.rectRequests.at(-1).revision, 2n);
  assert.equal(log.errors.length, 0, "a cancellation is not an error");
  client.dispose();
});

await check("a reply for another revision than the current State is dropped", async () => {
  const engine = makeDouble();
  const { client, log } = setupClient(engine);
  engine.rectRevision = 7n; // the engine answers for a revision the client does not hold
  await client.open(OPEN);
  client.setVisibleRange(0, 1);
  await select(client, 0n, 4n);
  await client.idle();
  assert.equal(engine.rectRequests.length, 1);
  assert.deepEqual(log.published, []);
  assert.equal(log.errors.length, 0);
  client.dispose();
});

await check("scrolling to other pages asks again; closing and opening forget the highlight", async () => {
  const engine = makeDouble();
  const { client, log } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(0, 1);
  await select(client, 0n, 4n);
  await client.idle();
  client.setVisibleRange(2, 3);
  await client.idle();
  assert.deepEqual(engine.rectRequests.map((r) => r.first), [0, 2]);
  assert.equal(log.published.length, 2);
  await client.open(OPEN);
  assert.deepEqual(log.published.at(-1), { revision: null, rects: [] }, "a new document starts without highlight");
  client.dispose();
});

await check("pictures asked for by the pages are fetched once, one reply at a time, as copies", async () => {
  const engine = makeDouble();
  engine.unknown.add(8n);
  const { client, log, held } = setupClient(engine);
  await client.open(OPEN);
  client.setVisibleRange(0, 1);
  client.setWantedImages([5n, 8n, 6n]);
  await client.idle();
  assert.deepEqual(engine.bitmapRequests, [5n, 8n, 6n]);
  assert.deepEqual([...held.keys()], [5n, 6n]);
  assert.deepEqual([...log.bitmaps[0].rgba], [1, 2, 3, 255, 4, 5, 6, 255]);
  assert.deepEqual(log.errors, [], "a picture that is gone is not an error");
  client.setWantedImages([5n, 6n]);
  await client.idle();
  assert.equal(engine.bitmapRequests.length, 3, "held pictures are not asked again");
  assert.throws(() => client.setWantedImages([5]), TypeError);
  client.dispose();
});

// ---- C: the bitmap cache ------------------------------------------------------------------------
await check("the picture cache is bounded in bytes, keeps what the pages need and drops late results after clear", async () => {
  const closed = [];
  const draw = async ({ rgba, tag }) => ({ tag, bytes: rgba.length, close() { closed.push(tag); } });
  const cache = createRichImageCache({ maxBytes: 1000, draw });
  const bitmap = (tag, bytes = 400) => ({ width: 10, height: bytes / 40, rgba: new Uint8Array(bytes), tag });
  await cache.add(1n, bitmap("a"));
  await cache.add(2n, bitmap("b"));
  assert.equal(cache.bytes, 800);
  cache.get(1n); // 2 is now the least recently used
  await cache.add(3n, bitmap("c"));
  assert.equal(cache.has(2n), false, "the least recently used picture goes");
  assert.deepEqual(closed, ["b"]);
  assert.ok(cache.has(1n) && cache.has(3n) && cache.bytes <= 1000);
  // A picture the shown pages draw is never evicted, even over the bound.
  cache.retain([1n, 3n]);
  await cache.add(4n, bitmap("d"));
  assert.ok(cache.has(1n) && cache.has(3n) && cache.has(4n), "the new picture stays (nothing else may go)");
  cache.retain([4n]);
  await cache.add(5n, bitmap("e"));
  assert.ok(cache.has(4n) && cache.has(5n) && !cache.has(1n) && !cache.has(3n));
  assert.ok(cache.bytes <= 1000, `${cache.bytes}`);
  // Replacing a picture releases the old one.
  await cache.add(5n, bitmap("f"));
  assert.ok(closed.includes("e"));
  // A result that arrives after clear() is released, not stored.
  let finish;
  const slow = createRichImageCache({ maxBytes: 1000, draw: (b) => new Promise((resolve) => { finish = () => resolve({ tag: b.tag, close() { closed.push(`late-${b.tag}`); } }); }) });
  const pending = slow.add(9n, bitmap("z"));
  slow.clear();
  finish();
  assert.equal(await pending, false);
  assert.equal(slow.size, 0);
  assert.ok(closed.includes("late-z"));
  cache.clear();
  assert.equal(cache.size, 0);
  assert.equal(cache.bytes, 0);
  await assert.rejects(() => cache.add(1, bitmap("n")), TypeError);
});

// ---- D: the real engine ---------------------------------------------------------------------------
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

function send(command) {
  const payload = encodeRichRequest({ docId: 1, requestId: innerId++, command });
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
const errorOf = (command) => {
  const r = send(command);
  assert.equal(r.type, "error", `expected an error, got ${r.type}`);
  return r.code;
};
const blank = () => state({ type: "open", kind: "blank", profile: "document", data: new Uint8Array() });
const select2 = (anchor, focus = anchor) => state({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false });
const unit = (position, kind, extend = false) => state({ type: "selectUnit", position: BigInt(position), unit: kind, extend });
const complete = () => { for (;;) { const s = state({ type: "layout", budget: 4096 }); if (s.layoutComplete) return s; } };
const pageList = (index) => {
  const r = send({ type: "pages", first: index, count: 1 });
  assert.equal(r.type, "pages");
  return decodeDisplayList(r.pages[0].display);
};

await check("engine: rectangles over several pages, empty selection, bad pages", () => {
  blank();
  state({ type: "insertText", text: Array.from({ length: 150 }, (_, n) => `Ligne ${n} du document`).join("\n") });
  const done = complete();
  assert.ok(done.pageCount >= 3, `${done.pageCount} pages`);
  select2(0, Number(done.docLen));
  const r = send({ type: "selectionRects", first: 0, count: 16 });
  assert.equal(r.type, "selectionRects");
  assert.equal(typeof r.revision, "bigint");
  assert.deepEqual([...new Set(r.rects.map((q) => q.page))], Array.from({ length: done.pageCount }, (_, i) => i));
  assert.ok(r.rects.length >= 150 && r.rects.every((q) => q.width > 0 && q.height > 0));
  select2(7);
  assert.deepEqual(send({ type: "selectionRects", first: 0, count: 1 }).rects, []);
  assert.equal(errorOf({ type: "selectionRects", first: done.pageCount, count: 1 }), "RICH_PAGE_RANGE");
});

const png = new Uint8Array(readFileSync(here("../crates/image/tests/fixtures/png/browser-200x100-red.png")));

await check("engine: a picture is drawn in its page and its bitmap is the picture", () => {
  blank();
  state({ type: "insertText", text: "avant apres" });
  select2(6);
  const inserted = state({ type: "insertImage", alt: "Rouge", bytes: png });
  assert.ok(inserted.pages.some((p) => p.index === 0), "the page is sent again");
  const draws = pageList(0).commands.filter((c) => c.type === "drawImage");
  assert.equal(draws.length, 1);
  const { imageId, bounds } = draws[0];
  assert.equal(typeof imageId, "bigint");
  // 200 x 100 px at 96 dpi: 150 x 75 pt.
  assert.ok(Math.abs(bounds.width - 150) < 0.02 && Math.abs(bounds.height - 75) < 0.02, `${bounds.width} x ${bounds.height}`);
  const bitmap = send({ type: "imageBitmap", objectId: imageId });
  assert.equal(bitmap.type, "imageBitmap");
  assert.equal(bitmap.objectId, imageId);
  assert.deepEqual([bitmap.width, bitmap.height, bitmap.rgba.length], [200, 100, 200 * 100 * 4]);
  assert.deepEqual([...bitmap.rgba.subarray(0, 4)], [255, 0, 0, 255], "red");
  assert.equal(errorOf({ type: "imageBitmap", objectId: imageId + 1000n }), "RICH_UNKNOWN_OBJECT");
  state({ type: "undo" });
  assert.equal(errorOf({ type: "imageBitmap", objectId: imageId }), "RICH_UNKNOWN_OBJECT");
});

await check("engine: double click selects a word, triple click a paragraph, extension grows by units", () => {
  blank();
  state({ type: "insertText", text: "un deux trois\nquatre cinq" });
  let s = unit(4, "word");
  assert.deepEqual([s.anchor, s.focus], [3n, 7n]);
  s = unit(10, "word", true);
  assert.deepEqual([s.anchor, s.focus], [3n, 13n]);
  s = unit(0, "word", true);
  assert.deepEqual([s.anchor, s.focus], [7n, 0n]);
  s = unit(16, "paragraph");
  assert.deepEqual([s.anchor, s.focus], [14n, 25n], "the last paragraph has no break to take");
  s = unit(2, "paragraph");
  assert.deepEqual([s.anchor, s.focus], [0n, 14n], "the break ends the paragraph");
  assert.equal(errorOf({ type: "selectUnit", position: 26n, unit: "word", extend: false }), "RICH_POSITION_OUT_OF_RANGE");
  assert.equal(select2(3).anchor, 3n);
});

console.log(`rich-editor-selection: ${checks} checks passed`);
