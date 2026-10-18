// SPDX-License-Identifier: AGPL-3.0-or-later
// US-209: pure logic of the document mirror (diff, flat position <-> DOM node mapping, stale revisions) on a minimal fake DOM.
import assert from "node:assert/strict";
import test from "node:test";
import {
  buildSegments, byteToUtf16, createDocumentMirror, DocumentMirrorError, flatToSegmentPoint, planReconcile, segmentPointToFlat,
  sliceSegments, utf16ToByte, validateWindow,
} from "../web/document-mirror.mjs";

class FakeNode {
  constructor(doc, tag, data) { this.ownerDocument = doc; this.tag = tag; this.nodeType = tag ? 1 : 3; this.data = data; this.kids = []; this.parent = null; this.attrs = new Map(); this.dataset = {}; this.id = ""; this.className = ""; }
  get childNodes() { return [...this.kids]; }
  get firstChild() { return this.kids[0] ?? null; }
  get lastChild() { return this.kids.at(-1) ?? null; }
  get nextSibling() { return this.parent ? this.parent.kids[this.parent.kids.indexOf(this) + 1] ?? null : null; }
  append(...nodes) { for (const n of nodes) this.insertBefore(n, null); }
  insertBefore(node, ref) {
    node.remove();
    node.parent = this;
    const at = ref ? this.kids.indexOf(ref) : this.kids.length;
    this.kids.splice(at, 0, node);
  }
  remove() { if (this.parent) { this.parent.kids.splice(this.parent.kids.indexOf(this), 1); this.parent = null; } }
  setAttribute(k, v) { this.attrs.set(k, String(v)); }
  getAttribute(k) { return this.attrs.get(k) ?? null; }
  removeAttribute(k) { this.attrs.delete(k); }
  set textContent(value) { this.kids.forEach(k => { k.parent = null; }); this.kids = []; if (value) this.append(this.ownerDocument.createTextNode(value)); }
  get textContent() { return this.tag ? this.kids.map(k => k.textContent).join("") : this.data; }
}
function fakeDocument() {
  const selections = [];
  const doc = {
    createElement: tag => new FakeNode(doc, tag),
    createTextNode: data => new FakeNode(doc, null, data),
    defaultView: { getSelection: () => ({ setBaseAndExtent: (...args) => selections.push(args) }) },
    selections,
  };
  doc.body = doc.createElement("body");
  return doc;
}
const t = (key, values = {}) => `${key}${Object.keys(values).length ? JSON.stringify(values) : ""}`;
const item = (start, text, extra = {}) => ({
  paraStart: BigInt(start), paraEnd: BigInt(start + Buffer.byteLength(text)), role: 0, level: 0, label: "", text, objects: [], ...extra,
});
const reply = (revision, items) => ({ revision: BigInt(revision), items });
const state = (revision, anchor, focus = anchor, docLen = 10000n) => ({ revision: BigInt(revision), docLen, selection: { anchor: BigInt(anchor), focus: BigInt(focus) } });
const tree = mirror => [...mirror.element.kids[0].kids];

test("byte and UTF-16 offsets agree on multi-byte text", () => {
  const text = "aé€😀b";
  assert.equal(byteToUtf16(text, 0), 0);
  assert.equal(byteToUtf16(text, 3), 2);
  assert.equal(byteToUtf16(text, 4), 2, "inside a character: snapped back");
  assert.equal(byteToUtf16(text, 6), 3);
  assert.equal(byteToUtf16(text, 10), 5);
  assert.equal(utf16ToByte(text, 5), 10);
  assert.equal(utf16ToByte(text, 4), 6, "inside a surrogate pair: snapped back");
});

test("segments and flat position mapping round-trip", () => {
  const text = "ab￼cé￼d";
  const segs = buildSegments(item(0, text, { objects: [{ offset: 2n, alt: "chat" }] }));
  assert.deepEqual(segs.map(s => [s.kind, s.byteStart, s.byteLen]), [["text", 0, 2], ["object", 2, 3], ["text", 5, 3], ["gap", 8, 3], ["text", 11, 1]]);
  for (const rel of [0, 1, 2, 5, 6, 8, 11, 12]) {
    const point = flatToSegmentPoint(segs, rel);
    assert.equal(segmentPointToFlat(segs, point), rel, `rel ${rel}`);
  }
  assert.deepEqual(flatToSegmentPoint(segs, 7), { seg: 2, utf16: 1, side: "in" }, "after é");
  assert.deepEqual(flatToSegmentPoint(segs, 12), { seg: 4, utf16: 1, side: "in" });
  assert.deepEqual(flatToSegmentPoint([], 0), { seg: -1, utf16: 0, side: "start" });
  assert.equal(sliceSegments(segs, 1, 6, a => `[${a}]`), "b[chat]c");
  assert.equal(sliceSegments(segs, 3, 4, a => a), "", "inside an object: nothing");
});

test("planReconcile reuses unchanged paragraphs, even after a shift", () => {
  assert.deepEqual(planReconcile(["a", "b", "c"], ["a", "b", "c"]), [0, 1, 2]);
  assert.deepEqual(planReconcile(["a", "b", "c"], ["a", "x", "c"]), [0, -1, 2]);
  assert.deepEqual(planReconcile(["a", "b", "c"], ["a", "n", "b", "c"]), [0, -1, 1, 2]);
  assert.deepEqual(planReconcile(["a", "b", "c", "d"], ["a", "d", "c", "b"]), [0, 3, 2, 1]);
  assert.deepEqual(planReconcile(["a", "a"], ["a", "a", "a"]), [0, 1, -1]);
  assert.deepEqual(planReconcile([], ["a"]), [-1]);
  assert.deepEqual(planReconcile(["a"], []), []);
});

test("validateWindow refuses malformed replies with a typed error", () => {
  const good = reply(1, [item(0, "x")]);
  assert.equal(validateWindow(good).items.length, 1);
  const refused = [null, {}, { revision: 1, items: [] }, { revision: 1n, items: "no" }, reply(1, [{ ...item(0, "x"), role: 9 }]),
    reply(1, [{ ...item(0, "x"), text: 5 }]), reply(1, [item(5, "abc"), item(6, "d")]), reply(1, [{ ...item(0, "x"), objects: [{ offset: 1, alt: "a" }] }]),
    reply(1, [{ ...item(0, "x"), paraEnd: -1n }]), reply(1, Array.from({ length: 257 }, (_, i) => item(i * 4, "x"))),
    reply(1, [item(0, "x".repeat(262145))])];
  for (const value of refused) assert.throws(() => validateWindow(value), error => error instanceof DocumentMirrorError && error.code === "MIRROR_BAD_WINDOW");
});

test("structure: headings, lists, images, cells, opaque blocks, empty paragraphs", async () => {
  const doc = fakeDocument();
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: async () => reply(1, [
    item(0, "Titre", { role: 1, level: 2 }), item(6, "Neuf", { role: 1, level: 8 }),
    item(11, "un", { role: 2, level: 0, label: "1." }), item(14, "deux", { role: 2, level: 1, label: "•" }),
    item(19, "ab￼cd￼", { objects: [{ offset: 2n, alt: "Un chat" }, { offset: 7n, alt: " " }] }),
    item(35, "cellule", { role: 3 }), item(43, "￼", { role: 4 }), item(47, ""),
  ]) });
  assert.equal((await mirror.update(state(1, 0))).status, "applied");
  const top = tree(mirror);
  assert.deepEqual(top.map(n => n.tag), ["h2", "div", "div", "p", "p", "p", "p"]);
  assert.equal(top[1].getAttribute("role"), "heading");
  assert.equal(top[1].getAttribute("aria-level"), "8");
  assert.equal(top[2].getAttribute("role"), "list");
  assert.deepEqual(top[2].kids.map(n => [n.getAttribute("role"), n.getAttribute("aria-level"), n.textContent]), [["listitem", "1", "1. un"], ["listitem", "2", "• deux"]]);
  const para = top[3];
  assert.equal(para.kids[1].getAttribute("role"), "img");
  assert.equal(para.kids[1].getAttribute("aria-label"), "Un chat");
  assert.equal(para.kids[3].getAttribute("aria-label"), "mirror.image.noalt", "blank alt falls back to the i18n key");
  assert.equal(top[4].dataset.mirrorRole, "cell");
  assert.equal(top[5].textContent, "mirror.opaque");
  assert.equal(top[6].kids[0].tag, "br", "empty paragraph keeps a line break so it is announced");
  assert.equal(mirror.element.getAttribute("role"), "document");
  assert.equal(mirror.element.getAttribute("aria-label"), "mirror.label");
});

test("removed U+FFFC (breaks) do not glue words together", async () => {
  const doc = fakeDocument();
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: async () => reply(1, [item(0, "un￼deux"), item(10, "a ￼b")]) });
  await mirror.update(state(1, 0));
  assert.deepEqual(tree(mirror).map(n => n.textContent), ["un deux", "a b"]);
});

test("revision update rebuilds only the changed paragraphs and keeps node identity", async () => {
  const doc = fakeDocument();
  let revision = 1, items = [item(0, "alpha"), item(6, "beta"), item(11, "gamma")];
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: async () => reply(revision, items) });
  await mirror.update(state(1, 0));
  const before = tree(mirror);
  assert.deepEqual([mirror.stats.created, mirror.stats.reused], [3, 0]);
  revision = 2; items = [item(0, "alpha"), item(6, "beta!"), item(12, "gamma")];
  await mirror.update(state(2, 0));
  const after = tree(mirror);
  assert.equal(after[0], before[0]);
  assert.notEqual(after[1], before[1]);
  assert.equal(after[2], before[2], "a shifted but identical paragraph is reused");
  assert.deepEqual([mirror.stats.created, mirror.stats.reused, mirror.stats.removed], [4, 2, 1]);
  revision = 3; items = [item(0, "alpha"), item(6, "nouveau"), item(14, "beta!"), item(20, "gamma")];
  await mirror.update(state(3, 0));
  assert.equal(tree(mirror)[2], after[1]);
  assert.deepEqual(tree(mirror).map(n => n.textContent), ["alpha", "nouveau", "beta!", "gamma"]);
});

test("selection moves with the model, with direction, and clamps outside the window", async () => {
  const doc = fakeDocument();
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: async () => reply(1, [item(10, "café"), item(16, "ab￼cd", { objects: [{ offset: 2n, alt: "x" }] })]) });
  await mirror.update(state(1, 12, 14));
  const last = () => doc.selections.at(-1);
  const [first, second] = tree(mirror);
  assert.deepEqual([last()[0], last()[1], last()[2], last()[3]], [first.kids[0], 2, first.kids[0], 3]);
  await mirror.update(state(1, 14, 12));
  assert.deepEqual([last()[1], last()[3]], [3, 2], "anchor after focus keeps its direction");
  await mirror.update(state(1, 17));
  assert.deepEqual([last()[0], last()[1]], [second.kids[0], 1]);
  await mirror.update(state(1, 18));
  assert.deepEqual([last()[0], last()[1]], [second, 1], "right before the object");
  await mirror.update(state(1, 21));
  assert.deepEqual([last()[0], last()[1]], [second.kids[2], 0], "right after the object (start of the next text)");
  await mirror.update(state(1, 0, 500));
  assert.deepEqual([last()[0], last()[1], last()[2], last()[3]], [first.kids[0], 0, second.kids[2], 2], "clamped to the mirrored window");
});

test("stale revisions and superseded requests never reach the DOM", async () => {
  const doc = fakeDocument();
  const waiting = [];
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: (start, end, { signal }) => new Promise((resolve, reject) => {
    signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
    waiting.push({ resolve, signal, start, end });
  }) });
  const first = mirror.update(state(1, 0));
  const second = mirror.update(state(2, 0));
  assert.equal(waiting[0].signal.aborted, true, "the older request is cancelled");
  assert.equal(mirror.stats.aborted, 1);
  waiting[0].resolve(reply(1, [item(0, "ancien")]));
  waiting[1].resolve(reply(2, [item(0, "récent")]));
  assert.equal((await first).status, "stale");
  assert.equal((await second).status, "applied");
  assert.deepEqual(tree(mirror).map(n => n.textContent), ["récent"]);
  // Reply carrying another revision than requested is dropped.
  const third = mirror.update(state(3, 0));
  waiting[2].resolve(reply(4, [item(0, "trop tard")]));
  assert.equal((await third).status, "stale");
  assert.equal(tree(mirror)[0].textContent, "récent");
});

test("same revision, same range: one request; selection-only updates reuse the window", async () => {
  const doc = fakeDocument();
  let calls = 0;
  const mirror = createDocumentMirror({ container: doc.body, t, getAccessibleWindow: async (a, b) => { calls += 1; return reply(1, [item(0, "alpha")]); } });
  const [a, b] = [mirror.update(state(1, 0, 0, 100n)), mirror.update(state(1, 3, 3, 100n))];
  assert.equal(a, b);
  await a;
  assert.equal(calls, 1);
  assert.equal((await mirror.update(state(1, 5, 5, 100n))).status, "selection");
  assert.equal(calls, 1);
  const last = doc.selections.at(-1);
  assert.equal(last[1], 5, "the latest selection wins");
});

test("window range: visible range plus margin, clamped to the document", async () => {
  const doc = fakeDocument();
  const ranges = [];
  const mirror = createDocumentMirror({ container: doc.body, t, margin: 100n, getAccessibleWindow: async (a, b) => { ranges.push([a, b]); return reply(1, []); } });
  await mirror.update({ ...state(1, 0, 0, 1000n), visible: { start: 50n, end: 300n } });
  await mirror.update({ ...state(1, 0, 0, 1000n), visible: { start: 900n, end: 990n } });
  assert.deepEqual(ranges, [[0n, 400n], [800n, 1000n]]);
});

test("provider failures and malformed data go to onError; the previous mirror stays", async () => {
  const doc = fakeDocument();
  const errors = [];
  let mode = "ok";
  const mirror = createDocumentMirror({ container: doc.body, t, onError: e => errors.push(e), getAccessibleWindow: async () => {
    if (mode === "throw") throw new Error("boom");
    return mode === "bad" ? { revision: 2n, items: [{}] } : reply(mode === "ok" ? 1 : 3, [item(0, "ok")]);
  } });
  await mirror.update(state(1, 0));
  mode = "throw"; assert.equal((await mirror.update(state(2, 0))).status, "error");
  mode = "bad"; assert.equal((await mirror.update(state(3, 0))).status, "error");
  assert.deepEqual(errors.map(e => e.code), ["MIRROR_WINDOW_FAILED", "MIRROR_BAD_WINDOW"]);
  assert.equal(tree(mirror)[0].textContent, "ok");
  assert.equal((await mirror.update({ revision: 1, docLen: 1n, selection: {} })).status, "error");
  assert.equal(errors.at(-1).code, "MIRROR_BAD_STATE");
  assert.throws(() => createDocumentMirror({}), error => error.code === "MIRROR_BAD_OPTIONS");
});

test("announcements: new paragraph, selection, character moves; aria-activedescendant", async () => {
  const doc = fakeDocument();
  const input = doc.createElement("textarea");
  const mirror = createDocumentMirror({ container: doc.body, t, activeDescendantTarget: input, getAccessibleWindow: async () => reply(1, [
    item(0, "Titre", { role: 1, level: 1 }), item(6, "abc def"),
  ]) });
  const live = mirror.element.kids[1];
  await mirror.update(state(1, 0));
  assert.equal(live.textContent, 'mirror.heading{"level":1} : Titre');
  assert.equal(input.getAttribute("aria-activedescendant"), tree(mirror)[0].id);
  await mirror.update(state(1, 7));
  assert.equal(live.textContent, "abc def");
  assert.equal(input.getAttribute("aria-activedescendant"), tree(mirror)[1].id);
  await mirror.update(state(1, 8));
  assert.equal(live.textContent, "c", "caret moved inside the paragraph: the next character is announced");
  await mirror.update(state(1, 6, 13));
  assert.equal(live.textContent, 'mirror.selection{"text":"abc def"}');
  mirror.dispose();
  assert.equal(input.getAttribute("aria-activedescendant"), null);
  assert.equal(mirror.element.parent, null);
  assert.equal((await mirror.update(state(1, 0))).status, "disposed");
});
