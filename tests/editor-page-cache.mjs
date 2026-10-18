// SPDX-License-Identifier: AGPL-3.0-or-later
// Scenarios of the ATED v2 client page cache (web/editor-page-cache.mjs). Frames are built and
// decoded with the R0 codec, so the cache only ever sees what the product decoder produces.
import assert from "node:assert/strict";
import { RichProtocolError, decodeRichReply, encodeRichReply } from "../web/rich-editor-protocol.mjs";
import { EditorPageCache, MAX_ATLAS_CLIENT_BYTES, RichClientError } from "../web/editor-page-cache.mjs";

// Path2D double: the cache builds each glyph path once.
let paths = 0;
globalThis.Path2D ??= class { constructor() { paths += 1; this.ops = []; } moveTo(x, y) { this.ops.push(["m", x, y]); } lineTo(x, y) { this.ops.push(["l", x, y]); } quadraticCurveTo() {} bezierCurveTo() {} closePath() { this.ops.push(["z"]); } };

const atdl = (tag, size = 44) => { const b = new Uint8Array(size); b.set([0x41, 0x54, 0x44, 0x4c]); b[4] = tag & 0xff; b[5] = tag >> 8; return b; };
const tagOf = (bytes) => bytes[4] | (bytes[5] << 8);
const state = (o = {}) => ({
  type: "state", revision: 0n, anchor: 0n, focus: 0n, focusUpstream: false, docLen: 1000n, modified: false, canUndo: false,
  canRedo: false, layoutComplete: true, pageCount: 0, caret: null, invalidated: [], atlas: { epoch: 1, first: 0, entries: [] }, pages: [], window: null, ...o,
});
const NOATLAS = { epoch: 1, first: 0, entries: [] };
const pagesOf = (...indices) => indices.map((index) => ({ index, display: atdl(index + 1000) }));
const frameOf = (response, requestId = 1) => encodeRichReply({ docId: 1, requestId, response });
const decoded = (response) => decodeRichReply(frameOf(response)).response;
const known = (cache) => [...cache.pageMap.keys()].sort((a, b) => a - b);

let failed = 0;
function test(name, fn) {
  try { fn(); console.log(`ok - ${name}`); } catch (error) { failed += 1; console.error(`not ok - ${name}\n${error.stack}`); }
}

test("state: selection with affinity, flags, caret and text window", () => {
  const cache = new EditorPageCache();
  const change = cache.apply(decoded(state({
    revision: 3n, anchor: 5n, focus: 9n, focusUpstream: true, modified: true, canUndo: true, canRedo: false, layoutComplete: false,
    pageCount: 2, caret: { page: 1, x: 10, y: 20, height: 12 }, pages: pagesOf(0, 1), window: { start: 4n, text: "hello" },
  })));
  assert.deepEqual(change, { ignored: false, updated: [0, 1], dropped: 0, invalidated: 0 });
  assert.equal(cache.revision, 3n);
  assert.deepEqual(cache.selection, { anchor: 5n, focus: 9n, upstream: true });
  assert.equal(cache.modified && cache.canUndo && !cache.canRedo && !cache.layoutComplete, true);
  assert.deepEqual(cache.caret, { page: 1, x: 10, y: 20, height: 12 });
  assert.deepEqual(cache.window, { revision: 3n, start: 4n, text: "hello" });
  // A later revision without window makes the old window unavailable.
  cache.apply(decoded(state({ revision: 4n, pageCount: 2 })));
  assert.equal(cache.window, null);
  assert.equal(cache.caret, null);
  assert.equal(cache.page(1) !== null, true, "unlisted pages stay valid");
});

test("pages are copied: later mutation of the frame does not reach the cache", () => {
  const cache = new EditorPageCache();
  const frame = frameOf(state({ pageCount: 1, pages: pagesOf(0) }));
  const { response } = decodeRichReply(frame);
  assert.equal(response.pages[0].display.buffer, frame.buffer, "decoder returns a view");
  cache.apply(response);
  frame.fill(0xee);
  const kept = cache.page(0);
  assert.equal(tagOf(kept), 1000);
  assert.notEqual(kept.buffer, frame.buffer);
  assert.equal(kept.buffer.byteLength, kept.length, "the cache owns an exact buffer");
});

test("Pages replies out of order fill the cache; a stale revision is ignored", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 7n, pageCount: 12 })));
  assert.deepEqual(cache.missing(0, 12), [{ first: 0, count: 12 }]);
  for (const index of [9, 2, 5]) {
    const change = cache.apply(decoded({ type: "pages", revision: 7n, pageCount: 12, atlas: NOATLAS, pages: pagesOf(index) }));
    assert.equal(change.ignored, false);
  }
  assert.deepEqual(known(cache), [2, 5, 9]);
  assert.deepEqual(cache.missing(0, 12), [{ first: 0, count: 2 }, { first: 3, count: 2 }, { first: 6, count: 3 }, { first: 10, count: 2 }]);
  const before = cache.version;
  const stale = cache.apply(decoded({ type: "pages", revision: 6n, pageCount: 12, atlas: NOATLAS, pages: pagesOf(0, 1) }));
  assert.equal(stale.ignored, true);
  assert.deepEqual(known(cache), [2, 5, 9]);
  assert.equal(cache.version, before, "an ignored reply is not a change");
  assert.equal(cache.apply(decoded({ type: "pages", revision: 8n, pageCount: 12, atlas: NOATLAS, pages: pagesOf(0) })).ignored, true);
});

test("a Pages reply before any State is ignored", () => {
  const cache = new EditorPageCache();
  assert.equal(cache.apply(decoded({ type: "pages", revision: 0n, pageCount: 3, atlas: NOATLAS, pages: pagesOf(0) })).ignored, true);
  assert.equal(cache.size, 0);
});

test("page_count decrease drops pages at or beyond it, increase keeps the others", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 20, pages: pagesOf(...Array.from({ length: 16 }, (_, i) => i)) })));
  cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 20, atlas: NOATLAS, pages: pagesOf(16, 17, 18, 19) }));
  assert.equal(cache.size, 20);
  const change = cache.apply(decoded(state({ revision: 2n, pageCount: 5, invalidated: [[1, 2]], pages: pagesOf(1) })));
  assert.equal(change.dropped, 1 + 15, "one invalidated, fifteen beyond the count (page 1 reloaded after)");
  assert.deepEqual(known(cache), [0, 1, 2, 3, 4]);
  assert.equal(cache.pageCount, 5);
  cache.apply(decoded(state({ revision: 3n, pageCount: 9 })));
  assert.deepEqual(known(cache), [0, 1, 2, 3, 4]);
  assert.deepEqual(cache.missing(0, 100), [{ first: 5, count: 4 }], "missing is clamped to the page count");
  cache.apply(decoded(state({ revision: 4n, pageCount: 0 })));
  assert.equal(cache.size, 0);
});

test("16 pages and several invalidations: invalidate first, then store, then count", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 40, pages: pagesOf(...Array.from({ length: 16 }, (_, i) => i)) })));
  cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 40, atlas: NOATLAS, pages: pagesOf(...Array.from({ length: 16 }, (_, i) => 16 + i)) }));
  cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 40, atlas: NOATLAS, pages: pagesOf(32, 33, 34, 35, 36, 37, 38, 39) }));
  assert.equal(cache.size, 40);
  // Edit: pages 4..9 and 20..23 and 30 changed; only 4 and 20 and 21 arrive with the reply.
  const change = cache.apply(decoded(state({
    revision: 2n, pageCount: 40, invalidated: [[4, 10], [20, 24], [30, 31]], pages: pagesOf(4, 20, 21).map((p) => ({ ...p, display: atdl(p.index + 5000) })),
  })));
  assert.equal(change.invalidated, 3);
  assert.deepEqual(change.updated, [4, 20, 21]);
  assert.equal(cache.size, 40 - 11 + 3);
  assert.equal(tagOf(cache.page(4)), 5004, "the new list replaces the invalidated one");
  assert.equal(cache.page(5), null);
  assert.deepEqual(cache.missing(0, 40), [{ first: 5, count: 5 }, { first: 22, count: 2 }, { first: 30, count: 1 }]);
  // A run of unknown pages never exceeds 16.
  cache.apply(decoded(state({ revision: 3n, pageCount: 40, invalidated: [[0, 40]], pages: [] })));
  assert.deepEqual(cache.missing(0, 40).map((r) => r.count), [16, 16, 8]);
  assert.equal(cache.displayLists("blank").every((p) => p === "blank"), true);
});

test("displayLists is dense and exposes the cached bytes", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 3, pages: pagesOf(0, 2) })));
  const lists = cache.displayLists();
  assert.equal(lists.length, 3);
  assert.equal(lists[1], null);
  assert.equal(lists[0], cache.page(0));
  assert.equal(lists[2] instanceof Uint8Array, true);
});

test("text window replies follow the revision", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 2n, docLen: 50n })));
  assert.equal(cache.apply(decoded({ type: "textWindow", revision: 1n, start: 0n, docLen: 50n, text: "old" })).ignored, true);
  assert.equal(cache.window, null);
  assert.equal(cache.apply(decoded({ type: "textWindow", revision: 2n, start: 3n, docLen: 50n, text: "abc" })).ignored, false);
  assert.deepEqual(cache.window, { revision: 2n, start: 3n, text: "abc" });
});

test("truncated and corrupted frames are rejected by the codec and never reach the state", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 2, pages: pagesOf(0, 1), anchor: 1n, focus: 1n })));
  const snapshot = JSON.stringify([cache.revision.toString(), known(cache), cache.pageCount]);
  const good = frameOf(state({ revision: 2n, pageCount: 3, invalidated: [[0, 2]], pages: pagesOf(0, 2) }));
  for (let cut = 0; cut < good.length; cut += 1) {
    assert.throws(() => decodeRichReply(good.subarray(0, cut)), RichProtocolError, `prefix ${cut}`);
  }
  const bad = Uint8Array.from(good);
  bad.set([0x58], 0); // magic
  assert.throws(() => decodeRichReply(bad), RichProtocolError);
  const flags = Uint8Array.from(good);
  flags[20 + 32] = 0xff; // reserved flag bits
  assert.throws(() => decodeRichReply(flags), RichProtocolError);
  assert.equal(JSON.stringify([cache.revision.toString(), known(cache), cache.pageCount]), snapshot);
});

test("a malformed reply handed straight to the cache is refused atomically", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 3, pages: pagesOf(0, 1, 2), anchor: 2n, focus: 2n })));
  const snapshot = () => JSON.stringify([cache.revision, known(cache), cache.pageCount, cache.selection, cache.version], (k, v) => (typeof v === "bigint" ? v.toString() : v));
  const before = snapshot();
  const good = decoded(state({ revision: 2n, pageCount: 3, invalidated: [[0, 3]], pages: [] }));
  const cases = [
    { ...good, pages: [{ index: 7, display: atdl(1) }] }, // beyond page count
    { ...good, pages: [{ index: 1, display: atdl(1) }, { index: 1, display: atdl(1) }] }, // not increasing
    { ...good, pages: [{ index: 1, display: new Uint8Array(3) }] }, // too short
    { ...good, invalidated: [[2, 2]] }, // empty range
    { ...good, revision: 2 }, // not a BigInt
    { ...good, pageCount: 1 << 30 }, // over the limit
    { type: "bogus" },
    null,
  ];
  for (const bogus of cases) {
    assert.throws(() => cache.apply(bogus), (error) => error instanceof RichClientError && error.code === "CLIENT_BAD_REPLY");
    assert.equal(snapshot(), before);
  }
});

test("hit results, errors and closed replies do not change the cache", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 1, pages: pagesOf(0) })));
  const version = cache.version;
  for (const response of [{ type: "hitResult", revision: 1n, page: 0, position: 0n, upstream: false }, { type: "error", code: "RICH_NOT_READY" }, { type: "closed" }]) {
    assert.equal(cache.apply(response).ignored, true);
  }
  assert.equal(cache.version, version);
});

test("onChange fires for applied replies only, and a throwing listener is isolated", () => {
  const seen = [];
  const cache = new EditorPageCache({ onChange: (change) => { seen.push(change.updated.length); throw new Error("listener"); } });
  cache.apply(decoded(state({ revision: 1n, pageCount: 2, pages: pagesOf(0, 1) })));
  cache.apply(decoded({ type: "pages", revision: 0n, pageCount: 2, atlas: NOATLAS, pages: pagesOf(0) }));
  assert.deepEqual(seen, [2]);
  assert.throws(() => new EditorPageCache({ onChange: 1 }), TypeError);
});

test("load: 300 pages, 400 updates, no leaked page or retained frame", () => {
  const cache = new EditorPageCache({ maxCachedPages: 1000 });
  const total = 300;
  const first = [];
  for (let i = 0; i < total; i += 16) first.push(pagesOf(...Array.from({ length: Math.min(16, total - i) }, (_, k) => i + k)));
  cache.apply(decoded(state({ revision: 1n, pageCount: total, pages: first[0] })));
  for (const batch of first.slice(1)) cache.apply(decoded({ type: "pages", revision: 1n, pageCount: total, atlas: NOATLAS, pages: batch }));
  assert.equal(cache.size, total);
  let pageCount = total;
  const frames = [];
  for (let n = 0; n < 400; n += 1) {
    const at = (n * 37) % pageCount;
    if (n % 50 === 49) pageCount = Math.max(1, pageCount - 20); // shrink now and then
    if (n % 90 === 89) pageCount += 15; // and grow
    const edge = Math.min(at, pageCount - 1);
    const frame = frameOf(state({
      revision: BigInt(n + 2), pageCount, invalidated: [[edge, Math.min(edge + 3, pageCount)]], pages: pagesOf(edge).map((p) => ({ ...p, display: atdl(n) })),
    }));
    frames.push(frame);
    cache.apply(decodeRichReply(frame).response);
    assert.equal(cache.size <= pageCount, true);
    assert.equal([...cache.pageMap.keys()].every((index) => index < pageCount), true);
  }
  // Every cached list owns its bytes: wiping all frames cannot change it.
  const sample = known(cache).map((i) => [i, tagOf(cache.page(i))]);
  for (const frame of frames) frame.fill(0);
  assert.deepEqual(known(cache).map((i) => [i, tagOf(cache.page(i))]), sample);
  for (const index of known(cache)) assert.equal(cache.page(index).buffer.byteLength, cache.page(index).length);
  cache.apply(decoded(state({ revision: 9000n, pageCount: 0 })));
  assert.equal(cache.size, 0);
  cache.reset();
  assert.equal(cache.pageMap.size, 0);
  assert.equal(cache.window, null);
  assert.equal(cache.revision, null);
});

test("eviction keeps the visible window plus margin and drops the farthest pages first", () => {
  const cache = new EditorPageCache({ maxCachedPages: 20, margin: 2 });
  cache.setVisible(10, 14);
  cache.apply(decoded(state({ revision: 1n, pageCount: 100, pages: pagesOf(...Array.from({ length: 16 }, (_, i) => i)) })));
  assert.equal(cache.size, 16);
  const change = cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 100, atlas: NOATLAS, pages: pagesOf(40, 41, 42, 43, 44, 45, 46, 47) }));
  assert.equal(cache.size, 20);
  assert.equal(change.dropped, 4);
  for (let i = 8; i < 16; i += 1) assert.equal(cache.has(i), true, "window " + i);
  assert.equal([44, 45, 46, 47].every((i) => !cache.has(i)), true, "farthest first");
  assert.equal(cache.has(40) && cache.has(43), true);
  assert.equal(cache.missing(10, 14).length, 0);
  assert.equal(cache.missing(44, 45).length, 1, "evicted pages are asked again");
  cache.setVisible(0, 40); // window larger than the bound: it is never evicted
  cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 100, atlas: NOATLAS, pages: pagesOf(60) }));
  assert.equal(cache.has(8), true);
  assert.throws(() => new EditorPageCache({ maxCachedPages: 0 }), TypeError);
});

test("a State with a lower revision is ignored except after reset or dropPages; selection must lie in the document", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 5n, pageCount: 1 })));
  assert.equal(cache.apply(decoded(state({ revision: 4n, pageCount: 3 }))).ignored, true);
  assert.equal(cache.pageCount, 1);
  assert.equal(cache.apply(decoded(state({ revision: 5n, pageCount: 2 }))).ignored, false);
  cache.dropPages();
  assert.equal(cache.apply(decoded(state({ revision: 2n, pageCount: 3 }))).ignored, false);
  assert.equal(cache.revision, 2n);
  assert.equal(cache.apply(decoded(state({ revision: 1n }))).ignored, true, "back to strict after a State");
  cache.reset();
  assert.equal(cache.apply(decoded(state({ revision: 0n }))).ignored, false);
  const bad = { ...decoded(state({ revision: 3n })), focus: 2000n };
  assert.throws(() => cache.apply(bad), (e) => e.code === "CLIENT_BAD_REPLY");
});

// One glyph atlas entry: ATDL, a black nonzero FillPath of a triangle of size `size`.
function entry(size) {
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
const withAtlas = (epoch, first, sizes) => ({ epoch, first, entries: sizes.map(entry) });

test("atlas: entries stored in order, kept across pages and dropPages, one Path2D per glyph", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 2, atlas: withAtlas(5, 0, [1, 2, 3]), pages: pagesOf(0) })));
  assert.equal(cache.atlasEpoch, 5);
  assert.equal(cache.atlasSize, 3);
  cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 2, atlas: withAtlas(5, 3, [4]), pages: pagesOf(1) }));
  assert.equal(cache.atlasSize, 4);
  cache.dropPages();
  assert.equal(cache.atlasSize, 4, "dropPages keeps the atlas");
  assert.equal(cache.size, 0);
  const before = paths;
  const a = cache.glyphResolver(0n, 2);
  assert.ok(a.path instanceof Path2D);
  assert.equal(cache.glyphResolver(0n, 2), a, "the same entry again");
  assert.equal(cache.glyphResolver(0, 2), a, "fontId 0 as a number too");
  assert.equal(paths, before + 1, "Path2D built once");
  assert.equal(cache.glyphResolver(1n, 0), null, "only font 0 is the atlas");
  assert.equal(cache.glyphResolver(0n, 4), null, "unknown index");
  assert.equal(cache.glyphResolver(0n, -1), null);
  cache.apply(decoded(state({ revision: 2n, pageCount: 2, atlas: withAtlas(5, 4, []) })));
  assert.equal(cache.atlasSize, 4, "a reply with nothing new keeps everything");
});

test("atlas: a new epoch empties the atlas and the pages before storing the reply", () => {
  const changes = [];
  const cache = new EditorPageCache({ onChange: (c) => changes.push(c) });
  cache.apply(decoded(state({ revision: 1n, pageCount: 3, atlas: withAtlas(1, 0, [1, 2]), pages: pagesOf(0, 1, 2) })));
  const old = cache.glyphResolver(0n, 1);
  const change = cache.apply(decoded(state({ revision: 2n, pageCount: 3, atlas: withAtlas(2, 0, [7]), pages: pagesOf(1) })));
  assert.equal(change.atlasReset, true);
  assert.equal(change.dropped, 3);
  assert.deepEqual(known(cache), [1], "only the page of the reply survives");
  assert.equal(cache.atlasSize, 1);
  assert.equal(cache.atlasEpoch, 2);
  assert.notEqual(cache.glyphResolver(0n, 0), old);
  assert.equal(cache.glyphResolver(0n, 1), null);
  // Pages replies change the epoch too.
  const pageChange = cache.apply(decoded({ type: "pages", revision: 2n, pageCount: 3, atlas: withAtlas(3, 0, [1]), pages: pagesOf(2) }));
  assert.equal(pageChange.atlasReset, true);
  assert.deepEqual(known(cache), [2]);
  assert.equal(changes.length, 3);
  // The epoch of a reply that is otherwise ignored (stale revision) still applies.
  const stale = cache.apply(decoded({ type: "pages", revision: 1n, pageCount: 3, atlas: withAtlas(4, 0, [9, 9]), pages: pagesOf(0) }));
  assert.equal(stale.ignored, true);
  assert.equal(stale.atlasReset, true);
  assert.equal(cache.atlasSize, 2);
  assert.equal(changes.length, 4, "listeners hear of the reset");
  cache.reset();
  assert.equal(cache.atlasSize, 0);
  assert.equal(cache.atlasEpoch, null);
  assert.equal(cache.glyphResolver(0n, 0), null);
});

test("atlas: a first index that does not follow is CLIENT_BAD_REPLY and changes nothing", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 1, atlas: withAtlas(1, 0, [1, 2]), pages: pagesOf(0) })));
  for (const first of [0, 1, 3, 4000]) {
    assert.throws(
      () => cache.apply(decoded(state({ revision: 2n, pageCount: 1, atlas: withAtlas(1, first, [3]), pages: pagesOf(0) }))),
      (e) => e instanceof RichClientError && e.code === "CLIENT_BAD_REPLY" && e.cause?.atlas === true,
    );
    assert.equal(cache.atlasSize, 2);
    assert.equal(cache.revision, 1n);
  }
  // A new epoch must start at 0.
  assert.throws(() => cache.apply(decoded(state({ revision: 2n, pageCount: 1, atlas: withAtlas(2, 2, [3]) }))), (e) => e.code === "CLIENT_BAD_REPLY");
  assert.equal(cache.atlasEpoch, 1);
  assert.equal(cache.atlasSize, 2);
  assert.equal(cache.size, 1);
});

test("atlas: bounded, and entries must be one black nonzero contour", () => {
  const cache = new EditorPageCache();
  const one = entry(1);
  const big = { epoch: 1, first: 0, entries: Array.from({ length: 4096 }, () => one) };
  cache.apply(decoded(state({ revision: 1n, atlas: big })));
  assert.equal(cache.atlasSize, 4096);
  assert.throws(() => cache.apply({ ...decoded(state({ revision: 2n })), atlas: { epoch: 1, first: 4096, entries: [one] } }), (e) => e.code === "CLIENT_BAD_REPLY");
  const fresh = new EditorPageCache();
  const white = entry(1); new DataView(white.buffer).setFloat32(46, 1, true);
  const evenOdd = entry(1); evenOdd[45] = 1;
  const rect = entry(1); rect[44] = 5; // not a fillPath
  for (const bad of [white, evenOdd, rect, atdl(1), entry(1).subarray(0, 60)]) {
    assert.throws(() => fresh.apply({ ...decoded(state({ revision: 1n })), atlas: { epoch: 1, first: 0, entries: [bad] } }), (e) => e.code === "CLIENT_BAD_REPLY");
    assert.equal(fresh.atlasSize, 0);
  }
  assert.throws(() => fresh.apply({ ...decoded(state({ revision: 1n })), atlas: undefined }), (e) => e.code === "CLIENT_BAD_REPLY");
});

test("atlas: a reply invalid after a valid atlas section changes nothing (atomic application)", () => {
  const cache = new EditorPageCache();
  cache.apply(decoded(state({ revision: 1n, pageCount: 2, atlas: withAtlas(1, 0, [1, 2]), pages: pagesOf(0, 1) })));
  const snapshot = () => JSON.stringify([cache.atlasEpoch, cache.atlasSize, cache.atlasBytes, known(cache), String(cache.revision), cache.pageCount]);
  const before = snapshot();
  const good = decoded(state({ revision: 2n, pageCount: 2, atlas: withAtlas(2, 0, [5]), pages: pagesOf(0) }));
  // Same epoch change (which empties atlas and pages) followed by an invalid State, then an invalid Pages reply.
  const badState = [
    { ...good, anchor: 5000n },
    { ...good, pageCount: 1, pages: pagesOf(0, 1) },
    { ...good, invalidated: [[3, 3]] },
    { ...good, revision: 2 },
  ];
  for (const bad of badState) {
    assert.throws(() => cache.apply(bad), (e) => e.code === "CLIENT_BAD_REPLY");
    assert.equal(snapshot(), before);
  }
  const goodPages = decoded({ type: "pages", revision: 1n, pageCount: 2, atlas: withAtlas(3, 0, [5, 6]), pages: pagesOf(1) });
  for (const bad of [{ ...goodPages, pageCount: 1 }, { ...goodPages, pages: [{ index: 1, display: new Uint8Array(3) }] }, { ...goodPages, pageCount: -1 }]) {
    assert.throws(() => cache.apply(bad), (e) => e.code === "CLIENT_BAD_REPLY");
    assert.equal(snapshot(), before);
  }
  assert.equal(cache.glyphResolver(0n, 1) !== null, true, "the old atlas still resolves");
  // The valid reply then applies as a whole.
  const change = cache.apply(good);
  assert.equal(change.atlasReset, true);
  assert.equal(cache.atlasEpoch, 2);
});

test("atlas: the bytes of the client atlas are bounded (MAX_ATLAS_CLIENT_BYTES)", () => {
  const cache = new EditorPageCache();
  const entryOf = (points) => {
    const out = new Uint8Array(44 + 22 + (points + 1) * 17 + 1);
    const view = new DataView(out.buffer);
    out.set([0x41, 0x54, 0x44, 0x4c, 1, 0, 0, 0]);
    view.setFloat64(24, 595.276, true); view.setFloat64(32, 841.89, true);
    view.setUint32(40, 1, true);
    let at = 44;
    out[at++] = 1; out[at++] = 0;
    for (const v of [0, 0, 0, 1]) { view.setFloat32(at, v, true); at += 4; }
    view.setUint32(at, points + 2, true); at += 4;
    out[at] = 0; at += 17;
    for (let i = 0; i < points; i += 1) { out[at] = 1; view.setFloat64(at + 1, i, true); at += 17; }
    out[at] = 4;
    return out;
  };
  const big = entryOf(3800);
  assert.equal(big.length < 64 * 1024, true);
  const fit = Math.floor(MAX_ATLAS_CLIENT_BYTES / big.length);
  const reply = (entries, first, rev) => ({ ...decoded(state({ revision: rev })), atlas: { epoch: 1, first, entries } });
  cache.apply(reply(Array.from({ length: fit }, () => big), 0, 1n));
  assert.equal(cache.atlasSize, fit);
  assert.equal(cache.atlasBytes, fit * big.length);
  assert.throws(() => cache.apply(reply(Array.from({ length: 2 }, () => big), fit, 2n)), (e) => e.code === "CLIENT_BAD_REPLY" && e.cause?.atlas !== true);
  assert.equal(cache.atlasSize, fit);
  assert.equal(cache.revision, 1n);
  // A new epoch counts from zero again.
  cache.apply({ ...decoded(state({ revision: 3n })), atlas: { epoch: 2, first: 0, entries: [big] } });
  assert.equal(cache.atlasBytes, big.length);
});

if (failed) { console.error(`${failed} test(s) failed`); process.exit(1); }
console.log("editor-page-cache: all tests passed");
