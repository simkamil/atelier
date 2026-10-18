// SPDX-License-Identifier: AGPL-3.0-or-later
// Scenarios of the ATED v2 client (web/rich-editor-client.mjs) against a simulated engine and
// transport. The simulated engine answers with frames built by the R0 codec.
import assert from "node:assert/strict";
import { getEventListeners } from "node:events";
import { PROPERTY_FIELDS, decodeRichRequest, encodeRichReply } from "../web/rich-editor-protocol.mjs";
import { EditorPageCache, RichClientError, createRichEditorClient } from "../web/rich-editor-client.mjs";

globalThis.Path2D ??= class { moveTo() {} lineTo() {} quadraticCurveTo() {} bezierCurveTo() {} closePath() {} };
const atdl = (tag) => { const b = new Uint8Array(44); b.set([0x41, 0x54, 0x44, 0x4c]); b[4] = tag & 0xff; b[5] = (tag >> 8) & 0xff; return b; };
const tagOf = (bytes) => bytes[4] | (bytes[5] << 8);
const abortError = () => new DOMException("The operation was aborted.", "AbortError");
const turn = () => new Promise((resolve) => setImmediate(resolve));
const OPEN = { kind: "blank", profile: "document", data: new Uint8Array(0) };

// One glyph atlas entry: ATDL, a black nonzero FillPath of a triangle of size `size`.
function atlasEntry(size) {
  const out = new Uint8Array(44 + 22 + 3 * 17 + 1);
  const view = new DataView(out.buffer);
  out.set([0x41, 0x54, 0x44, 0x4c, 1, 0, 0, 0]);
  view.setFloat64(24, 595.276, true); view.setFloat64(32, 841.89, true);
  view.setUint32(40, 1, true);
  let at = 44;
  out[at++] = 1; out[at++] = 0;
  for (const v of [0, 0, 0, 1]) { view.setFloat32(at, v, true); at += 4; }
  view.setUint32(at, 4, true); at += 4;
  for (const [op, x, y] of [[0, 0, 0], [1, size, 0], [1, size, -size]]) {
    out[at++] = op; view.setFloat64(at, x, true); view.setFloat64(at + 8, y, true); at += 16;
  }
  out[at++] = 4;
  return out;
}

/** Engine double: `total` pages, `computed` of them laid out so far, `Layout` computes `budget` more. */
function makeEngine({ docId = 1, total = 100, firstComputed = 10 } = {}) {
  const e = { total, computed: firstComputed, rev: 0n, open: false, anchor: 0n, edits: 0,
    atlas: { epoch: 1, sent: 0, fresh: 0 } };
  // Entries made since the last reply: `atlas.fresh` of them, numbered from `atlas.sent`.
  const atlasDelta = () => {
    const a = e.atlas;
    const delta = { epoch: a.epoch, first: a.sent, entries: Array.from({ length: a.fresh }, (_, i) => atlasEntry(a.sent + i + 1)) };
    a.sent += a.fresh; a.fresh = 0;
    return delta;
  };
  const done = () => e.computed >= e.total;
  // While layout is unfinished the engine already announces a few pages that are not computed yet.
  const count = () => (done() ? e.total : Math.min(e.total, e.computed + 4));
  const pageList = (index) => ({ index, display: atdl(Number(e.rev) * 7 + index) });
  e.state = (extra = {}) => ({
    type: "state", revision: e.rev, anchor: e.anchor, focus: e.anchor, focusUpstream: false, docLen: 100000n, modified: e.rev > 0n,
    canUndo: e.rev > 0n, canRedo: false, layoutComplete: done(), pageCount: count(), caret: null, invalidated: [], atlas: atlasDelta(), pages: [], window: null, ...extra,
  });
  e.handle = (req) => {
    const { command: c } = req;
    if (req.docId !== docId && c.type !== "open") return { type: "error", code: "RICH_UNKNOWN_DOCUMENT" };
    switch (c.type) {
      case "open": e.open = true; e.rev = 0n; return e.state({ pages: Array.from({ length: Math.min(16, e.computed) }, (_, i) => pageList(i)) });
      case "close": if (!e.open) return { type: "error", code: "RICH_UNKNOWN_DOCUMENT" }; e.open = false; return { type: "closed" };
      case "layout": e.computed = Math.min(e.total, e.computed + c.budget); return e.state();
      case "pages": {
        if (c.first >= count()) return { type: "error", code: "RICH_PAGE_RANGE" };
        if (c.first + c.count > e.computed) return { type: "error", code: "RICH_NOT_READY" };
        return { type: "pages", revision: e.rev, pageCount: count(), atlas: atlasDelta(), pages: Array.from({ length: c.count }, (_, i) => pageList(c.first + i)) };
      }
      case "insertText": {
        e.rev += 1n; e.edits += 1;
        e.computed = Math.min(e.total, e.computed + 16);
        const at = (e.edits * 37) % Math.max(1, e.computed - 1);
        return e.state({ invalidated: [[at, Math.min(at + 3, count())]], pages: [pageList(at)] });
      }
      case "replaceRange": return { type: "error", code: "RICH_POSITION_OUT_OF_RANGE" };
      case "resync":
        // Like the engine: a new epoch, the atlas of the pages sent again from 0, everything invalidated.
        if (!e.open) return { type: "error", code: "RICH_UNKNOWN_DOCUMENT" };
        e.resyncs = (e.resyncs ?? 0) + 1;
        e.atlas.epoch += 1; e.atlas.sent = 0; e.atlas.fresh = e.resyncAtlas ?? 2;
        return e.state({ invalidated: [[0, count()]] });
      case "shrink": return e.state();
      default: return e.state();
    }
  };
  return e;
}

/** Transport double with a request log, optional reply override and a hold gate. */
function makeTransport(engine, docId = 1) {
  const t = { log: [], override: null, gate: null, calls: 0 };
  t.request = async (frame, { signal } = {}) => {
    t.calls += 1;
    if (signal?.aborted) throw abortError();
    const req = decodeRichRequest(frame);
    t.log.push(req.command.type);
    const response = engine.handle(req);
    let bytes = encodeRichReply({ docId, requestId: req.requestId, response });
    if (t.override) {
      const out = t.override(req, bytes, response);
      if (out instanceof Error) throw out;
      if (out) bytes = out;
    }
    if (t.gate?.when(req.command)) {
      const gate = t.gate;
      gate.hit = true;
      await new Promise((resolve, reject) => {
        gate.release = resolve;
        signal?.addEventListener("abort", () => reject(abortError()), { once: true });
      });
    }
    return bytes;
  };
  return t;
}

function setup(options = {}, engineOptions = {}) {
  const engine = makeEngine(engineOptions);
  const transport = makeTransport(engine);
  const errors = [];
  const client = createRichEditorClient({ transport, docId: 1, yieldTurn: turn, onError: (e) => errors.push(e), ...options });
  return { engine, transport, errors, client };
}

let failed = 0;
async function test(name, fn) {
  try { await fn(); console.log(`ok - ${name}`); } catch (error) { failed += 1; console.error(`not ok - ${name}\n${error.stack}`); }
}
const rejects = (promise, code) => assert.rejects(promise, (error) => error instanceof RichClientError && error.code === code, code);

await test("open starts the pump, which sends Layout until the complete flag, then stops", async () => {
  const { client, transport, engine } = setup();
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await client.idle();
  assert.deepEqual(transport.log, ["open", ...Array(6).fill("layout")], "10 -> 100 pages by steps of 16");
  assert.equal(client.cache.pageCount, 100);
  assert.equal(client.cache.layoutComplete, true);
  assert.equal(engine.computed, 100);
  const calls = transport.calls;
  await turn(); await turn();
  assert.equal(transport.calls, calls, "nothing is sent once layout is complete");
  client.dispose();
});

await test("the pump cedes to user commands between two Layout sends", async () => {
  let yields = 0;
  let pending;
  const holder = {};
  const { client, transport } = setup({
    yieldTurn: async () => { yields += 1; if (yields === 2) pending = holder.client.command({ type: "insertText", text: "a" }); await turn(); },
  });
  holder.client = client;
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await pending;
  await client.idle();
  assert.deepEqual(transport.log.slice(0, 5), ["open", "layout", "layout", "insertText", "layout"]);
  client.dispose();
});

await test("Pages for an unfinished area: RICH_NOT_READY, Layout, then success", async () => {
  const { client, transport } = setup({ layoutBudget: 2 }, { total: 100, firstComputed: 4 });
  await client.open(OPEN);
  client.stopPump();
  await client.idle();
  transport.log.length = 0;
  await client.ensurePages(6, 8);
  assert.equal(transport.log[0], "pages");
  assert.equal(transport.log.at(-1), "pages");
  assert.equal(transport.log.includes("layout"), true, "NOT_READY triggered at least one Layout");
  assert.equal(transport.log.filter((x) => x === "pages").length, transport.log.filter((x) => x === "layout").length + 1, "one retry per Layout");
  assert.equal(client.cache.has(6) && client.cache.has(7), true);
  assert.equal(tagOf(client.cache.page(7)), 7);
  client.dispose();
});

await test("NOT_READY after layout completion is surfaced, not retried forever", async () => {
  const { client, transport, engine } = setup({}, { total: 5, firstComputed: 5 });
  await client.open(OPEN);
  await client.idle();
  client.cache.dropPages();
  engine.handle = ((orig) => (req) => (req.command.type === "pages" ? { type: "error", code: "RICH_NOT_READY" } : orig(req)))(engine.handle);
  await rejects(client.ensurePages(0, 1), "RICH_NOT_READY");
  assert.equal(transport.log.filter((x) => x === "layout").length, 0);
  client.dispose();
});

await test("stopPump aborts the Layout in flight; startPump resynchronises and resumes", async () => {
  const { client, transport, engine } = setup();
  transport.gate = { when: (c) => c.type === "layout" };
  await client.open(OPEN);
  while (!transport.gate.hit) await turn();
  const layoutsBefore = transport.log.length;
  transport.gate = null;
  client.stopPump();
  await client.idle();
  assert.equal(transport.log.length, layoutsBefore + 1, "only the resync Layout follows the stop");
  assert.equal(client.cache.size, 0, "pages dropped because the lost State may have carried invalidations");
  client.startPump();
  await client.whenLayoutComplete();
  await client.idle();
  assert.equal(transport.log[layoutsBefore], "resync", "the resync follows the stop");
  assert.equal(client.cache.pageCount, engine.total);
  client.dispose();
});

await test("aborting the client signal during the pump closes the client", async () => {
  const controller = new AbortController();
  const { client, transport } = setup({ signal: controller.signal });
  transport.gate = { when: (c) => c.type === "layout" };
  await client.open(OPEN);
  while (!transport.gate.hit) await turn();
  const waiting = client.whenLayoutComplete();
  controller.abort();
  await rejects(waiting, "CLIENT_CLOSED");
  await rejects(client.command({ type: "undo" }), "CLIENT_CLOSED");
  const calls = transport.calls;
  await turn(); await turn();
  assert.equal(transport.calls, calls);
  assert.equal(getEventListeners(controller.signal, "abort").length, 0, "dispose detaches its own listener");
});

await test("a caller signal cancels one command and leaves no listener behind", async () => {
  const { client, transport } = setup({}, { total: 10, firstComputed: 10 });
  await client.open(OPEN);
  await client.idle();
  const controller = new AbortController();
  transport.gate = { when: (c) => c.type === "insertText" };
  const sent = client.command({ type: "insertText", text: "x" }, { signal: controller.signal });
  while (!transport.gate.hit) await turn();
  controller.abort();
  await assert.rejects(sent, (error) => error.name === "AbortError");
  transport.gate = null;
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  const next = await client.command({ type: "undo" });
  assert.equal(next.response.type, "state", "the queue keeps working and resynchronises first");
  assert.equal(transport.log.at(-2), "resync");
  const already = new AbortController();
  already.abort();
  await assert.rejects(client.command({ type: "undo" }, { signal: already.signal }), (e) => e.name === "AbortError");
  client.dispose();
});

await test("RICH_* refusals are typed and leave state and queue intact", async () => {
  const { client } = setup({}, { total: 10, firstComputed: 10 });
  await client.open(OPEN);
  await client.idle();
  const before = client.cache.version;
  await rejects(client.command({ type: "replaceRange", start: 0n, end: 0n, text: "q" }), "RICH_POSITION_OUT_OF_RANGE");
  assert.equal(client.cache.version, before);
  const ok = await client.command({ type: "insertText", text: "hi" });
  assert.equal(ok.change.ignored, false);
  await client.command({ type: "setSelection", anchor: 0n, focus: 0n, upstream: false });
  // Protocol-level refusal before sending: typed too, nothing sent.
  const sentBefore = client.cache.version;
  await assert.rejects(client.command({ type: "insertText", text: "￼" }), (e) => e.name === "RichProtocolError" && e.code === "RICH_INVALID_TEXT");
  await assert.rejects(client.command({ type: "nope" }), (e) => e.name === "RichProtocolError");
  assert.equal(client.cache.version, sentBefore);
  await assert.rejects(client.command({ type: "open", ...OPEN }), TypeError);
  client.dispose();
});

await test("engine error replies carry the RICH code name and unknown documents are refused", async () => {
  const engine = makeEngine({ docId: 9 });
  const transport = makeTransport(engine);
  const client = createRichEditorClient({ transport, docId: 1, yieldTurn: turn });
  const error = await client.command({ type: "undo" }).catch((e) => e);
  assert.equal(error.code, "RICH_UNKNOWN_DOCUMENT");
  assert.match(error.message, /RICH_UNKNOWN_DOCUMENT/);
  client.dispose();
});

await test("truncated, corrupted, mismatched and failing replies are rejected without touching the cache", async () => {
  const { client, transport } = setup({}, { total: 10, firstComputed: 10 });
  await client.open(OPEN);
  await client.idle();
  const cases = [
    ["CLIENT_BAD_REPLY", (req, bytes) => bytes.subarray(0, bytes.length - 3)],
    ["CLIENT_BAD_REPLY", (req, bytes) => { const b = Uint8Array.from(bytes); b[0] = 0; return b; }],
    ["CLIENT_BAD_REPLY", (req, bytes) => { const b = Uint8Array.from(bytes); b[20 + 32] = 0xff; return b; }],
    ["CLIENT_BAD_REPLY", () => new Uint8Array(0)],
    ["CLIENT_BAD_REPLY", () => "not bytes"],
    ["CLIENT_MISMATCH", (req) => encodeRichReply({ docId: 1, requestId: req.requestId + 1, response: { type: "closed" } })],
    ["CLIENT_MISMATCH", (req) => encodeRichReply({ docId: 2, requestId: req.requestId, response: { type: "closed" } })],
    ["CLIENT_BAD_REPLY", (req) => encodeRichReply({ docId: 1, requestId: req.requestId, response: { type: "closed" } })],
    ["CLIENT_TRANSPORT", () => new Error("boom")],
  ];
  for (const [code, override] of cases) {
    transport.override = (req, bytes, response) => (req.command.type === "insertText" ? override(req, bytes, response) : undefined);
    await rejects(client.command({ type: "insertText", text: "z" }), code);
    assert.equal(client.cache.size, 0, "the engine may have applied it: pages dropped");
    await client.command({ type: "setSelection", anchor: 0n, focus: 0n, upstream: false }); // resync first
    assert.equal(transport.log.at(-2), "resync");
    await client.ensurePages(0, 10);
    assert.equal(client.cache.size, 10);
  }
  transport.override = null;
  await client.command({ type: "insertText", text: "z" });
  assert.equal(client.cache.size > 0, true);
  client.dispose();
});

await test("a corrupted Layout reply stops the pump, reports it and rejects whenLayoutComplete", async () => {
  const { client, transport, errors } = setup();
  transport.override = (req, bytes) => (req.command.type === "layout" ? bytes.subarray(0, 25) : undefined);
  await client.open(OPEN);
  await rejects(client.whenLayoutComplete(), "CLIENT_BAD_REPLY");
  await client.idle();
  assert.equal(errors.length >= 1, true);
  assert.equal(errors[0].code, "CLIENT_BAD_REPLY");
  const layouts = transport.log.filter((x) => x === "layout").length;
  await turn(); await turn();
  assert.equal(transport.log.filter((x) => x === "layout").length, layouts, "the pump stays stopped");
  client.dispose();
});

await test("a stale Pages reply is ignored and the request is repeated", async () => {
  const { client, transport } = setup({}, { total: 20, firstComputed: 20 });
  await client.open(OPEN);
  await client.idle();
  client.cache.dropPages();
  let first = true;
  transport.override = (req, bytes, response) => {
    if (req.command.type !== "pages" || !first) return undefined;
    first = false;
    return encodeRichReply({ docId: 1, requestId: req.requestId, response: { ...response, revision: response.revision + 5n } });
  };
  await client.ensurePages(3, 5);
  assert.deepEqual(transport.log.slice(-2), ["pages", "pages"]);
  assert.equal(client.cache.has(3) && client.cache.has(4), true);
  client.dispose();
});

await test("visible pages are fetched in the background, again after an invalidation", async () => {
  const { client, transport } = setup({}, { total: 30, firstComputed: 30 });
  await client.open(OPEN); // the Open reply carries pages 0..15
  client.cache.dropPages();
  client.setVisibleRange(0, 3);
  await client.idle();
  assert.deepEqual([0, 1, 2].map((i) => client.cache.has(i)), [true, true, true]);
  assert.equal(transport.log.filter((x) => x === "pages").length, 1, "one Pages request for the three-page run");
  client.cache.apply({ type: "state", revision: 5n, anchor: 0n, focus: 0n, focusUpstream: false, docLen: 5n, modified: true, canUndo: true, canRedo: false, layoutComplete: true, pageCount: 30, caret: null, invalidated: [[1, 2]], atlas: { epoch: 1, first: 0, entries: [] }, pages: [], window: null });
  client.setVisibleRange(0, 3);
  await client.idle();
  assert.equal(client.cache.has(1), false, "stale revision: the client does not fetch for a revision the engine never reached");
  client.dispose();
});

await test("a visible page that is not ready waits for the pump instead of failing", async () => {
  const { client, transport, errors } = setup({}, { total: 40, firstComputed: 4 });
  transport.override = null;
  await client.open(OPEN);
  client.cache.dropPages();
  client.setVisibleRange(30, 34);
  await client.whenLayoutComplete();
  await client.idle();
  assert.deepEqual(errors, []);
  assert.equal([30, 31, 32, 33].every((i) => client.cache.has(i)), true);
  assert.equal(transport.log.includes("layout"), true);
  client.dispose();
});

await test("the atlas epoch and entries are relayed; an index gap is CLIENT_BAD_REPLY and requests the resync", async () => {
  const { client, transport, engine } = setup({}, { total: 30, firstComputed: 30 });
  engine.atlas.fresh = 3;
  await client.open(OPEN);
  assert.equal(client.atlasEpoch, 1);
  assert.equal(client.cache.atlasSize, 3);
  assert.ok(client.glyphResolver(0n, 2));
  engine.atlas.fresh = 2;
  await client.command({ type: "insertText", text: "a" });
  assert.equal(client.cache.atlasSize, 5);
  // A reply to a request that was cancelled after the engine answered: entries 5..6 are lost.
  engine.atlas.sent += 2;
  engine.atlas.fresh = 1;
  transport.log.length = 0;
  await rejects(client.command({ type: "insertText", text: "b" }), "CLIENT_BAD_REPLY");
  assert.equal(client.cache.size, 0, "pages were dropped");
  // The resync runs by itself (idle step): a new epoch, the atlas starts again at 0.
  await client.idle();
  assert.equal(transport.log.includes("resync"), true, "Resync, not Layout");
  assert.equal(engine.resyncs, 1);
  assert.equal(client.atlasEpoch, 2);
  assert.equal(client.cache.atlasSize, 2);
  client.dispose();
});

/** Opens a client on a finished 30-page document with 3 atlas entries, as in the tests above. */
async function opened(options = {}, engineOptions = {}) {
  const ctx = setup(options, { total: 30, firstComputed: 30, ...engineOptions });
  ctx.engine.atlas.fresh = 3;
  await ctx.client.open(OPEN);
  await ctx.client.idle();
  return ctx;
}
const consistent = (ctx) => {
  const { client, engine } = ctx;
  assert.equal(client.atlasEpoch, engine.atlas.epoch, "same epoch");
  assert.equal(client.cache.atlasSize, engine.atlas.sent, "the client holds every entry the engine counted");
};

await test("Resync after an abandoned State reply: the atlas and the pages converge", async () => {
  const ctx = await opened();
  const { client, transport, engine } = ctx;
  engine.atlas.fresh = 2;
  transport.override = (req) => (req.command.type === "insertText" ? new Error("transport down") : null);
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_TRANSPORT");
  transport.override = null;
  await client.idle(); // the idle step sends the resync
  assert.equal(transport.log.filter((t) => t === "resync").length, 1);
  consistent(ctx);
  assert.equal(client.cache.size, 0, "everything was invalidated");
  await client.ensurePages(0, 4);
  consistent(ctx);
  assert.equal(client.cache.missing(0, 4).length, 0);
  assert.equal(ctx.errors.length, 0);
  client.dispose();
});

await test("Resync after an abandoned Pages reply (cancelled request): no CLIENT_BAD_REPLY loop", async () => {
  const ctx = await opened();
  const { client, transport, engine } = ctx;
  engine.atlas.fresh = 2; // entries of the pages that are about to be lost
  const controller = new AbortController();
  transport.gate = { when: (c) => c.type === "pages" };
  const pending = client.ensurePages(20, 23, { signal: controller.signal });
  while (!transport.gate.hit) await turn();
  controller.abort(); // the engine already answered
  await assert.rejects(pending, (e) => e.name === "AbortError");
  transport.gate = null;
  await client.idle();
  assert.equal(transport.log.includes("resync"), true, "the abandoned Pages reply requests a resync");
  await client.ensurePages(20, 23);
  consistent(ctx);
  assert.equal(client.cache.missing(20, 23).length, 0);
  assert.equal(ctx.errors.length, 0);
  // A later reply still follows.
  engine.atlas.fresh = 1;
  await client.command({ type: "insertText", text: "z" });
  consistent(ctx);
  client.dispose();
});

await test("a lost Resync reply is replayed until it succeeds", async () => {
  const ctx = await opened();
  const { client, transport, engine } = ctx;
  engine.atlas.fresh = 1;
  // The reply to the edit and the replies to the Resync are lost (the engine ran them all).
  transport.override = (req) => (["insertText", "resync"].includes(req.command.type) ? new Error("transport down") : null);
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_TRANSPORT");
  await client.idle();
  assert.equal(engine.resyncs, 1, "the idle step tried, and the resync stays requested");
  await rejects(client.command({ type: "undo" }), "CLIENT_TRANSPORT");
  assert.ok(engine.resyncs >= 2, "needsResync stays set while the exchange fails");
  transport.override = null;
  const state = await client.command({ type: "undo" });
  assert.equal(state.response.type, "state");
  assert.ok(engine.resyncs >= 3);
  consistent(ctx);
  await client.ensurePages(0, 3);
  consistent(ctx);
  client.dispose();
});

await test("Resync without a document is refused and the next Open recovers", async () => {
  const ctx = await opened();
  const { client, transport, engine } = ctx;
  engine.atlas.fresh = 1;
  transport.override = (req) => {
    if (req.command.type !== "insertText") return null;
    engine.open = false; // the engine lost the document after answering
    return new Error("transport down");
  };
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_TRANSPORT");
  transport.override = null;
  await client.idle();
  assert.equal(ctx.errors.at(-1)?.code, "RICH_UNKNOWN_DOCUMENT", "the refused Resync is reported");
  await rejects(client.command({ type: "undo" }), "RICH_UNKNOWN_DOCUMENT");
  assert.equal(engine.resyncs ?? 0, 0);
  engine.atlas.epoch = 7; engine.atlas.sent = 0; engine.atlas.fresh = 2;
  await client.open(OPEN);
  consistent(ctx);
  client.dispose();
});

await test("RICH_FRAME_TOO_LARGE is not a refusal: the engine applied the command and lost the reply, so a Resync follows", async () => {
  const ctx = await opened();
  const { client, transport, engine, errors } = ctx;
  engine.atlas.fresh = 2;
  transport.override = (req) => (req.command.type === "insertText"
    ? encodeRichReply({ docId: 1, requestId: req.requestId, response: { type: "error", code: "RICH_FRAME_TOO_LARGE" } }) : null);
  await rejects(client.command({ type: "insertText", text: "a" }), "RICH_FRAME_TOO_LARGE");
  transport.override = null;
  await client.idle();
  assert.equal(engine.resyncs, 1);
  assert.equal(engine.edits, 1, "the engine applied the edit");
  consistent(ctx);
  assert.equal(errors.length, 0);
  client.dispose();
});

await test("Close needs no resync before it, and a Close on a document the engine already closed leaves nothing stuck", async () => {
  const ctx = await opened();
  const { client, transport, engine } = ctx;
  transport.override = (req) => (["insertText", "resync"].includes(req.command.type) ? new Error("transport down") : null);
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_TRANSPORT");
  await client.idle();
  const resyncs = engine.resyncs;
  assert.ok(resyncs >= 1);
  transport.override = null;
  transport.log.length = 0;
  await client.closeDocument();
  assert.deepEqual(transport.log, ["close"], "no Resync before Close");
  // The engine lost the document (closed behind the client's back) while a resync is pending.
  engine.atlas.epoch += 1; engine.atlas.sent = 0; engine.atlas.fresh = 1;
  await client.open(OPEN);
  await client.idle();
  transport.override = (req) => {
    if (req.command.type !== "insertText") return null;
    engine.open = false;
    return new Error("transport down");
  };
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_TRANSPORT");
  await client.idle(); // the Resync is refused: needsResync stays set
  transport.override = null;
  await rejects(client.closeDocument(), "RICH_UNKNOWN_DOCUMENT");
  assert.equal(client.opened, false);
  const before = engine.resyncs ?? 0;
  engine.atlas.epoch += 1; engine.atlas.sent = 0; engine.atlas.fresh = 1;
  await client.open(OPEN);
  await client.command({ type: "undo" });
  await client.idle();
  assert.equal(engine.resyncs ?? 0, before, "no stale resync after the reopen");
  client.dispose();
});

await test("two resyncs in a row caused by the client atlas byte bound stop the pump with a typed fatal error", async () => {
  const engine = makeEngine({ total: 30, firstComputed: 30 });
  const transport = makeTransport(engine);
  const cache = new EditorPageCache();
  const errors = [];
  const client = createRichEditorClient({ transport, docId: 1, cache, yieldTurn: turn, onError: (e) => errors.push(e) });
  await client.open(OPEN);
  await client.idle();
  const real = cache.apply.bind(cache);
  // Every reply that is not the answer to a Resync overflows the byte bound.
  cache.apply = (r) => {
    if (r.type !== "state" || !r.invalidated.some(([a, b]) => a === 0 && b === r.pageCount)) throw new RichClientError("CLIENT_BAD_REPLY", "bytes", { atlasBytes: true });
    return real(r);
  };
  await rejects(client.command({ type: "insertText", text: "a" }), "CLIENT_BAD_REPLY");
  await client.idle();
  assert.equal(engine.resyncs, 1, "the first overflow is recovered by a resync");
  await rejects(client.command({ type: "insertText", text: "b" }), "CLIENT_ATLAS_FULL");
  await client.idle();
  assert.equal(engine.resyncs, 1, "no third resync: the loop is cut");
  const fatal = errors.find((e) => e.code === "CLIENT_ATLAS_FULL");
  assert.ok(fatal instanceof RichClientError);
  const calls = transport.calls;
  await turn(); await turn();
  assert.equal(transport.calls, calls, "the pump does not spin");
  client.dispose();
});

await test("closeDocument sends Close, empties the cache and stops the pump", async () => {
  const { client, transport } = setup();
  await client.open(OPEN);
  const closed = await client.closeDocument();
  assert.equal(closed.response.type, "closed");
  await client.idle();
  assert.equal(client.opened, false);
  assert.equal(client.cache.size, 0);
  assert.equal(client.cache.revision, null);
  const calls = transport.calls;
  await turn(); await turn();
  assert.equal(transport.calls, calls);
  client.dispose();
});

await test("constructor and range arguments are validated", async () => {
  const transport = makeTransport(makeEngine());
  assert.throws(() => createRichEditorClient({ transport, docId: 0 }), TypeError);
  assert.throws(() => createRichEditorClient({ transport: {}, docId: 1 }), TypeError);
  assert.throws(() => createRichEditorClient({ transport, docId: 1, layoutBudget: 0 }), TypeError);
  const client = createRichEditorClient({ transport, docId: 1 });
  assert.throws(() => client.ensurePages(-1, 2), TypeError);
  assert.throws(() => client.setVisibleRange(3, 2), TypeError);
  await assert.rejects(client.command({ type: "undo" }, { signal: {} }), TypeError);
  client.dispose();
  client.dispose();
});

await test("load: 300 pages, 400 edits through the client, no leaked page, listener or request", async () => {
  const { client, transport, engine, errors } = setup({}, { total: 300, firstComputed: 20 });
  await client.open(OPEN);
  await client.whenLayoutComplete();
  assert.equal(client.cache.pageCount, 300);
  client.setVisibleRange(0, 300);
  await client.idle();
  assert.equal(client.cache.size, 300);
  const controller = new AbortController();
  let shrinks = 0;
  for (let n = 0; n < 400; n += 1) {
    if (n % 100 === 50) { engine.total = engine.computed = engine.computed - 25; shrinks += 1; }
    await client.command({ type: "insertText", text: "a" }, { signal: controller.signal });
    assert.equal(client.cache.size <= client.cache.pageCount, true);
    assert.equal([...client.cache.pageMap.keys()].every((i) => i < client.cache.pageCount), true);
  }
  await client.idle();
  assert.equal(shrinks, 4);
  assert.equal(client.cache.pageCount, engine.computed);
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  assert.equal(transport.log.filter((x) => x === "insertText").length, 400);
  assert.deepEqual(errors, []);
  for (const index of client.cache.pageMap.keys()) assert.equal(client.cache.page(index).buffer.byteLength, 44);
  client.dispose();
  assert.equal(client.cache.size, 0);
});

await test("a failing cache.apply on a State reply also drops pages and resynchronises", async () => {
  const engine = makeEngine({ total: 10, firstComputed: 10 });
  const transport = makeTransport(engine);
  const cache = new EditorPageCache();
  const client = createRichEditorClient({ transport, docId: 1, cache, yieldTurn: turn });
  await client.open(OPEN);
  await client.idle();
  const real = cache.apply.bind(cache);
  let once = true;
  cache.apply = (r) => { if (once && r.type === "state") { once = false; throw new RichClientError("CLIENT_BAD_REPLY", "forced"); } return real(r); };
  await rejects(client.command({ type: "insertText", text: "q" }), "CLIENT_BAD_REPLY");
  assert.equal(cache.size, 0);
  await client.command({ type: "undo" });
  assert.equal(transport.log.at(-2), "resync");
  client.dispose();
});

await test("stopPump then setVisibleRange does not spin: the pages are fetched and the loop yields", async () => {
  const { client, transport, errors } = setup({}, { total: 30, firstComputed: 8 });
  await client.open(OPEN);
  client.stopPump();
  await client.idle();
  client.cache.dropPages();
  client.setVisibleRange(0, 5);
  await new Promise((resolve) => setTimeout(resolve, 50)); // a spinning microtask loop would starve this timer
  assert.equal([0, 1, 2, 3, 4].every((i) => client.cache.has(i)), true);
  assert.deepEqual(errors, []);
  const layouts = transport.log.filter((x) => x === "layout").length;
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(transport.log.filter((x) => x === "layout").length, layouts, "the pump stays stopped");
  client.dispose();
});

await test("whenLayoutComplete rejects on Closed and when open fails", async () => {
  const a = setup();
  await a.client.open(OPEN);
  const waiting = a.client.whenLayoutComplete();
  a.client.stopPump();
  await a.client.closeDocument();
  await rejects(waiting, "CLIENT_CLOSED");
  a.client.dispose();
  const b = setup();
  b.transport.override = () => new Error("down");
  const pendingLayout = b.client.whenLayoutComplete();
  await rejects(b.client.open(OPEN), "CLIENT_TRANSPORT");
  await rejects(pendingLayout, "CLIENT_TRANSPORT");
  b.client.dispose();
});

await test("R7a: accessibleWindow, save, makeEditableCopy and insertImage get their own reply types and never reach the cache", async () => {
  const { client, engine, transport } = setup();
  const item = { paraStart: 0n, paraEnd: 2n, role: "paragraph", level: 0, label: "", text: "ab", inlines: [] };
  const base = engine.handle;
  engine.handle = (req) => {
    switch (req.command.type) {
      case "accessibleWindow": return { type: "accessibleWindow", revision: engine.rev, items: [item] };
      case "save": case "makeEditableCopy": return { type: "saveData", revision: engine.rev, bytes: Uint8Array.of(1, 2, 3), notices: [{ code: 0x0300, text: "n" }] };
      case "insertImage": engine.rev += 1n; return engine.state();
      default: return base(req);
    }
  };
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await client.idle();
  const version = client.cache.version;
  const window = await client.accessibleWindow(0n, 2n);
  assert.deepEqual(window, { revision: 0n, items: [item] });
  const saved = await client.command({ type: "save", format: "txt", nowMs: 1n, user: "u", version: "v" });
  assert.equal(saved.response.type, "saveData");
  assert.deepEqual([...saved.response.bytes], [1, 2, 3]);
  assert.equal(saved.change, null, "SaveData is not a cache input");
  const copy = await client.command({ type: "makeEditableCopy", nowMs: 1n, user: "", version: "" });
  assert.equal(copy.response.notices[0].code, 0x0300);
  assert.equal(client.cache.version, version, "neither reply touches the page cache");
  const image = await client.command({ type: "insertImage", alt: "x", bytes: Uint8Array.of(1) });
  assert.equal(image.response.type, "state");
  assert.equal(client.cache.revision, 1n, "InsertImage answers a State the cache applies");
  assert.deepEqual(transport.log.slice(-4), ["accessibleWindow", "save", "makeEditableCopy", "insertImage"]);
  client.dispose();
});

await test("R7a: a refused Open restores the previous document by a Resync instead of leaving an empty client", async () => {
  const { client, engine, transport } = setup();
  const base = engine.handle;
  let refuse = false;
  engine.handle = (req) => (req.command.type === "open" && refuse ? { type: "error", code: "RICH_OPEN_FAILED" } : base(req));
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await client.idle();
  refuse = true;
  await rejects(client.open(OPEN), "RICH_OPEN_FAILED");
  assert.equal(client.opened, true, "the engine kept the previous document, so does the client");
  await client.idle();
  await client.command({ type: "undo" });
  assert.equal(transport.log.includes("resync"), true, "the cache emptied by the refused Open is rebuilt");
  assert.equal(client.cache.pageCount > 0 && client.cache.revision !== null, true);
  refuse = false;
  // A refused first Open leaves nothing to restore.
  const fresh = setup();
  fresh.engine.handle = () => ({ type: "error", code: "RICH_OPEN_FAILED" });
  await rejects(fresh.client.open(OPEN), "RICH_OPEN_FAILED");
  assert.equal(fresh.client.opened, false);
  client.dispose(); fresh.client.dispose();
});

await test("R7a: the page cache keeps the read-only and revision flags of the State", async () => {
  const cache = new EditorPageCache();
  const state = (extra) => ({ type: "state", revision: 0n, anchor: 0n, focus: 0n, focusUpstream: false, docLen: 5n, modified: false, canUndo: false, canRedo: false,
    layoutComplete: true, pageCount: 0, caret: null, invalidated: [], atlas: { epoch: 1, first: 0, entries: [] }, pages: [], window: null, ...extra });
  cache.apply(state({ readOnly: true, hasRevisions: true }));
  assert.equal(cache.readOnly, true);
  assert.equal(cache.hasRevisions, true);
  cache.apply(state({ revision: 1n }));
  assert.equal(cache.readOnly, false, "a later State without the flags clears them");
  cache.apply(state({ readOnly: true, hasRevisions: true, revision: 2n }));
  cache.reset();
  assert.equal(cache.readOnly || cache.hasRevisions, false, "reset clears them");
});

await test("R7b-2: search, statistics, rectangles and properties never reach the cache; replace and apply carry a State that does", async () => {
  const { client, engine, transport } = setup();
  const snapshot = { editable: Object.fromEntries(PROPERTY_FIELDS.map((n) => [n, ""])), limits: Object.fromEntries(PROPERTY_FIELDS.map((n) => [n, 255])),
    created: null, modified: null, lastPrinted: null, lastModifiedBy: null, revision: null, application: null, appVersion: null, template: null,
    totalTime: null, statistics: { pages: null, words: 1, characters: 2, charactersWithSpaces: 2, lines: null, paragraphs: 1 }, noticeCount: 0 };
  const counts = { words: 1, characters: 2, charactersWithSpaces: 2, paragraphs: 1, lines: 1, pages: 1 };
  const base = engine.handle;
  engine.handle = (req) => {
    const c = req.command;
    switch (c.type) {
      case "search": return { type: "searchResult", revision: engine.rev, total: 1, truncated: false, totalCapped: false,
        items: [{ start: 0n, end: 2n, excerpt: { text: "ab", start: 0, end: 2, truncatedBefore: false, truncatedAfter: false } }] };
      case "statistics": return { type: "statistics", revision: engine.rev, anchor: 0n, focus: 0n, layoutComplete: true, document: counts, selection: counts };
      case "searchRects": return { type: "searchRects", revision: engine.rev, rects: [{ item: 0, page: 0, x: 1, y: 2, width: 3, height: 4 }] };
      case "replace": case "replaceAll": engine.rev += 1n; return { type: "replaced", count: 3, state: engine.state() };
      case "propertiesRead": return { type: "properties", revision: engine.rev, snapshot };
      case "propertiesApply": return { type: "propertiesApplied", status: "applied", changed: 1, field: null, max: 0, length: 0, state: engine.state() };
      default: return base(req);
    }
  };
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await client.idle();
  const version = client.cache.version;
  const flags = { matchCase: true };
  const found = await client.command({ type: "search", revision: 0n, flags, query: "ab" });
  assert.equal(found.response.items.length, 1);
  assert.equal(found.change, null);
  assert.equal((await client.command({ type: "statistics" })).response.document.words, 1);
  assert.equal((await client.command({ type: "searchRects", revision: 0n, firstPage: 0, pageCount: 1, ranges: [{ start: 0n, end: 2n }] })).response.rects[0].width, 3);
  assert.equal((await client.command({ type: "propertiesRead" })).response.snapshot.limits.title, 255);
  assert.equal(client.cache.version, version, "none of the four replies touches the page cache");
  const all = await client.command({ type: "replaceAll", revision: 0n, flags: {}, query: "ab", replacement: "x" });
  assert.equal(all.response.count, 3);
  assert.equal(client.cache.revision, 1n, "the State of Replaced is applied");
  assert.equal(all.change.ignored, false);
  const one = await client.command({ type: "replace", revision: 1n, flags: {}, start: 0n, end: 2n, query: "ab", replacement: "x" });
  assert.equal(one.response.type, "replaced");
  assert.equal(client.cache.revision, 2n);
  const applied = await client.command({ type: "propertiesApply", changes: { title: "T" } });
  assert.equal(applied.response.status, "applied");
  assert.equal(applied.change.ignored, false, "the State of PropertiesApplied is applied (the modified flag reaches the editor)");
  assert.equal(transport.log.includes("resync"), false);
  client.dispose();
});

await test("R7b-2: an engine refusal of a query or a replacement is a typed error and forces no resync; a lost Replaced does", async () => {
  const { client, engine, transport } = setup();
  const base = engine.handle;
  let refuse = "RICH_STALE_MATCH";
  engine.handle = (req) => {
    const c = req.command;
    if (c.type === "replace" || c.type === "search") return { type: "error", code: refuse };
    return base(req);
  };
  await client.open(OPEN);
  await client.whenLayoutComplete();
  await client.idle();
  await rejects(client.command({ type: "replace", revision: 0n, flags: {}, start: 0n, end: 1n, query: "a", replacement: "b" }), "RICH_STALE_MATCH");
  refuse = "RICH_STALE_REVISION";
  await rejects(client.command({ type: "search", revision: 0n, flags: {}, query: "a" }), "RICH_STALE_REVISION");
  await client.idle();
  assert.equal(transport.log.includes("resync"), false, "refusals leave the cache as it is");
  // A transport failure after the engine applied a replacement: the cache cannot be trusted, a Resync follows.
  transport.override = (req) => (req.command.type === "replaceAll" ? new Error("lost") : null);
  await rejects(client.command({ type: "replaceAll", revision: 0n, flags: {}, query: "a", replacement: "b" }), "CLIENT_TRANSPORT");
  transport.override = null;
  await client.command({ type: "undo" });
  assert.equal(transport.log.includes("resync"), true, "the lost State forces a resynchronisation");
  client.dispose();
});

/** An engine whose caret only shows once the layout reached `needed` paragraphs, after a far SetSelection. */
function jumpSetup(options = {}, engineOptions = {}, needed = 300) {
  const ctx = setup(options, { total: 1000, firstComputed: 10, ...engineOptions });
  const { engine } = ctx;
  const budgets = [];
  let jumped = false;
  const base = engine.handle;
  engine.handle = (req) => {
    const c = req.command;
    if (c.type === "layout") budgets.push(c.budget);
    if (c.type === "setSelection") { jumped = true; engine.anchor = c.focus; engine.jumps = (engine.jumps ?? 0) + 1; }
    const r = base(req);
    if (r.type === "state" && jumped) return { ...r, focus: engine.anchor, caret: engine.computed >= needed ? { page: 1, x: 10, y: 10, height: 12 } : null };
    return r;
  };
  return { ...ctx, budgets, needed };
}

await test("a selection without caret raises the Layout budget of the pump until the caret arrives", async () => {
  const { client, budgets, engine } = jumpSetup({ layoutBudget: 4 });
  await client.open(OPEN);
  client.stopPump();
  await client.idle();
  budgets.length = 0;
  await client.command({ type: "setSelection", anchor: 99999n, focus: 99999n, upstream: false });
  assert.equal(client.cache.caret, null, "the State of the jump has no caret");
  assert.equal(client.cache.layoutComplete, false);
  client.startPump();
  await client.whenLayoutComplete();
  assert(budgets.length >= 1 && budgets[0] === 64, `budgets ${budgets.slice(0, 3)}`);
  assert(budgets.every((b) => b === 64 || b === 4), "only the two budgets are used");
  assert.notEqual(client.cache.caret, null, "the caret came with a later State");
  assert.equal(engine.computed >= 300, true);
  client.dispose();
});

await test("without a pending selection the pump keeps its normal budget", async () => {
  const { client, budgets } = jumpSetup({ layoutBudget: 4 });
  await client.open(OPEN);
  await client.whenLayoutComplete();
  assert(budgets.length > 0 && budgets.every((b) => b === 4), "normal budget only");
  client.dispose();
});

await test("keystrokes sent while the caret is absent run between two Layout steps and are not lost", async () => {
  const { client, transport, engine } = jumpSetup({ layoutBudget: 4 });
  await client.open(OPEN);
  client.stopPump();
  await client.idle();
  await client.command({ type: "setSelection", anchor: 99999n, focus: 99999n, upstream: false });
  client.startPump();
  const typed = [client.command({ type: "insertText", text: "a" }), client.command({ type: "insertText", text: "b" })];
  await Promise.all(typed);
  assert.equal(engine.edits, 2, "both edits reached the engine");
  const firstEdit = transport.log.indexOf("insertText");
  assert(firstEdit >= 0 && transport.log.slice(firstEdit).includes("layout"), "the pump resumes after the edits");
  assert.equal(client.cache.layoutComplete, false, "the edits did not wait for the whole layout");
  await client.whenLayoutComplete();
  assert.notEqual(client.cache.caret, null);
  client.dispose();
});

await test("a second jump before the pump finishes, a Resync and a closed document keep the client consistent", async () => {
  const { client, transport, engine, errors } = jumpSetup({ layoutBudget: 4 });
  await client.open(OPEN);
  await client.command({ type: "setSelection", anchor: 99999n, focus: 99999n, upstream: false });
  await client.command({ type: "setSelection", anchor: 5n, focus: 5n, upstream: false });
  assert.equal(engine.jumps, 2);
  assert.equal(client.cache.selection.focus, 5n, "the last jump wins");
  client.stopPump();
  await client.idle();
  engine.atlas.fresh = 2;
  await client.command({ type: "resync" });
  client.startPump();
  await client.whenLayoutComplete();
  assert.notEqual(client.cache.caret, null);
  assert.deepEqual(errors, []);
  // Closed document: the pump stops, waiters are released with an error.
  await client.command({ type: "close" });
  await client.idle();
  const calls = transport.calls;
  await turn();
  assert.equal(transport.calls, calls, "no request after Close");
  client.dispose();
});

await test("an Up or End that found no caret is replayed once the caret arrives; a newer command cancels it", async () => {
  const { client, transport, engine } = jumpSetup({ layoutBudget: 4 });
  await client.open(OPEN);
  client.stopPump();
  await client.idle();
  await client.command({ type: "setSelection", anchor: 99999n, focus: 99999n, upstream: false });
  await client.command({ type: "move", direction: "up", extend: false });
  const moves = () => transport.log.filter((x) => x === "move").length;
  assert.equal(moves(), 1);
  client.startPump();
  await client.whenLayoutComplete();
  await client.idle();
  assert.equal(moves(), 2, "the Up was replayed once the caret was there");
  assert.notEqual(client.cache.caret, null);
  // Not replayed after a newer command.
  const { client: second, transport: t2 } = jumpSetup({ layoutBudget: 4 });
  await second.open(OPEN);
  second.stopPump();
  await second.idle();
  await second.command({ type: "setSelection", anchor: 99999n, focus: 99999n, upstream: false });
  await second.command({ type: "move", direction: "pageDown", extend: false });
  await second.command({ type: "insertText", text: "a" });
  second.startPump();
  await second.whenLayoutComplete();
  await second.idle();
  assert.equal(t2.log.filter((x) => x === "move").length, 1, "superseded move is dropped");
  client.dispose(); second.dispose();
});

if (failed) { console.error(`${failed} test(s) failed`); process.exit(1); }
console.log("rich-editor-client: all tests passed");
