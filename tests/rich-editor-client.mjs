// SPDX-License-Identifier: AGPL-3.0-or-later
// Scenarios of the ATED v2 client (web/rich-editor-client.mjs) against a simulated engine and
// transport. The simulated engine answers with frames built by the R0 codec.
import assert from "node:assert/strict";
import { getEventListeners } from "node:events";
import { decodeRichRequest, encodeRichReply } from "../web/rich-editor-protocol.mjs";
import { RichClientError, createRichEditorClient } from "../web/rich-editor-client.mjs";

const atdl = (tag) => { const b = new Uint8Array(44); b.set([0x41, 0x54, 0x44, 0x4c]); b[4] = tag & 0xff; b[5] = (tag >> 8) & 0xff; return b; };
const tagOf = (bytes) => bytes[4] | (bytes[5] << 8);
const abortError = () => new DOMException("The operation was aborted.", "AbortError");
const turn = () => new Promise((resolve) => setImmediate(resolve));
const OPEN = { kind: "blank", profile: "document", data: new Uint8Array(0) };

/** Engine double: `total` pages, `computed` of them laid out so far, `Layout` computes `budget` more. */
function makeEngine({ docId = 1, total = 100, firstComputed = 10 } = {}) {
  const e = { total, computed: firstComputed, rev: 0n, open: false, anchor: 0n, edits: 0 };
  const done = () => e.computed >= e.total;
  // While layout is unfinished the engine already announces a few pages that are not computed yet.
  const count = () => (done() ? e.total : Math.min(e.total, e.computed + 4));
  const pageList = (index) => ({ index, display: atdl(Number(e.rev) * 7 + index) });
  e.state = (extra = {}) => ({
    type: "state", revision: e.rev, anchor: e.anchor, focus: e.anchor, focusUpstream: false, docLen: 100000n, modified: e.rev > 0n,
    canUndo: e.rev > 0n, canRedo: false, layoutComplete: done(), pageCount: count(), caret: null, invalidated: [], pages: [], window: null, ...extra,
  });
  e.handle = (req) => {
    const { command: c } = req;
    if (req.docId !== docId && c.type !== "open") return { type: "error", code: "RICH_UNKNOWN_DOCUMENT" };
    switch (c.type) {
      case "open": e.open = true; e.rev = 0n; return e.state({ pages: Array.from({ length: Math.min(16, e.computed) }, (_, i) => pageList(i)) });
      case "close": e.open = false; return { type: "closed" };
      case "layout": e.computed = Math.min(e.total, e.computed + c.budget); return e.state();
      case "pages": {
        if (c.first >= count()) return { type: "error", code: "RICH_PAGE_RANGE" };
        if (c.first + c.count > e.computed) return { type: "error", code: "RICH_NOT_READY" };
        return { type: "pages", revision: e.rev, pageCount: count(), pages: Array.from({ length: c.count }, (_, i) => pageList(c.first + i)) };
      }
      case "insertText": {
        e.rev += 1n; e.edits += 1;
        e.computed = Math.min(e.total, e.computed + 16);
        const at = (e.edits * 37) % Math.max(1, e.computed - 1);
        return e.state({ invalidated: [[at, Math.min(at + 3, count())]], pages: [pageList(at)] });
      }
      case "replaceRange": return { type: "error", code: "RICH_POSITION_OUT_OF_RANGE" };
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
  assert.equal(transport.log[layoutsBefore], "layout", "resync Layout of one paragraph");
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
  assert.equal(transport.log.at(-2), "layout");
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
  const snapshot = () => JSON.stringify([client.cache.revision?.toString(), [...client.cache.pageMap.keys()], client.cache.version]);
  const before = snapshot();
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
    transport.override = override;
    await rejects(client.command({ type: "insertText", text: "z" }), code);
    assert.equal(snapshot(), before);
  }
  transport.override = null;
  await client.command({ type: "insertText", text: "z" });
  assert.notEqual(snapshot(), before, "the client recovers on the next valid reply");
  client.dispose();
});

await test("a corrupted Layout reply stops the pump, reports it and rejects whenLayoutComplete", async () => {
  const { client, transport, errors } = setup();
  transport.override = (req, bytes) => (req.command.type === "layout" ? bytes.subarray(0, 25) : undefined);
  await client.open(OPEN);
  await rejects(client.whenLayoutComplete(), "CLIENT_BAD_REPLY");
  await client.idle();
  assert.equal(errors.length, 1);
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
  client.cache.apply({ type: "state", revision: 5n, anchor: 0n, focus: 0n, focusUpstream: false, docLen: 5n, modified: true, canUndo: true, canRedo: false, layoutComplete: true, pageCount: 30, caret: null, invalidated: [[1, 2]], pages: [], window: null });
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

if (failed) { console.error(`${failed} test(s) failed`); process.exit(1); }
console.log("rich-editor-client: all tests passed");
