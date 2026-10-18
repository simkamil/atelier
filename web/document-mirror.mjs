// SPDX-License-Identifier: AGPL-3.0-or-later
// US-209 (R5): accessible DOM mirror of the visible part of the Canvas document (SPEC 6.14, D13).
// The mirror is built with createElement/textContent only. The data comes from an injected
// `getAccessibleWindow(start, end, { signal })` (flat positions, see docs/us-209-document-mirror.md).

const FFFC = "￼";
export const MAX_ITEMS = 256;
export const MAX_TEXT_UNITS = 256 * 1024;
const MAX_SELECTION_ANNOUNCE = 1000;
const MAX_PARAGRAPH_ANNOUNCE = 400;
const ROLE_HEADING = 1, ROLE_LIST_ITEM = 2, ROLE_CELL = 3, ROLE_OPAQUE = 4;
const ROLE_NAMES = ["paragraph", "heading", "list-item", "cell", "opaque"];

export class DocumentMirrorError extends Error {
  constructor(code, message, options) { super(message, options); this.name = "DocumentMirrorError"; this.code = code; }
}
const invalidInput = (code, message) => new DocumentMirrorError(code, message);
const bytesOf = cp => cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;

/** UTF-16 offset in `text` of a UTF-8 byte offset (snapped back to a scalar boundary). */
export function byteToUtf16(text, bytes) {
  let b = 0, u = 0;
  for (const ch of text) {
    const n = bytesOf(ch.codePointAt(0));
    if (b + n > bytes) break;
    b += n; u += ch.length;
  }
  return u;
}
/** UTF-8 byte offset of a UTF-16 offset in `text`. */
export function utf16ToByte(text, units) {
  let b = 0, u = 0;
  for (const ch of text) {
    if (u + ch.length > units) break;
    b += bytesOf(ch.codePointAt(0)); u += ch.length;
  }
  return b;
}

/**
 * Splits a paragraph into segments with byte ranges relative to paraStart:
 * text runs, objects (U+FFFC listed in `objects`, with alt) and gaps (other U+FFFC: breaks, symbols).
 */
export function buildSegments(item) {
  const alts = new Map();
  for (const object of item.objects) alts.set(Number(object.offset), object.alt);
  const out = [];
  let byte = 0, buffer = "", start = 0;
  const flush = () => { if (buffer) { out.push({ kind: "text", byteStart: start, byteLen: byte - start, text: buffer }); buffer = ""; } };
  for (const ch of item.text) {
    if (ch === FFFC) {
      flush();
      out.push(alts.has(byte) ? { kind: "object", byteStart: byte, byteLen: 3, alt: alts.get(byte) } : { kind: "gap", byteStart: byte, byteLen: 3 });
      byte += 3;
    } else {
      if (!buffer) start = byte;
      buffer += ch; byte += bytesOf(ch.codePointAt(0));
    }
  }
  flush();
  return out;
}

/** Flat offset (relative to the paragraph) to a segment point; seg -1 for an empty paragraph. */
export function flatToSegmentPoint(segments, rel) {
  if (!segments.length) return { seg: -1, utf16: 0, side: "start" };
  const last = segments.length - 1;
  for (let i = 0; i < segments.length; i += 1) {
    const s = segments[i];
    const end = s.byteStart + s.byteLen;
    if (rel < end || (i === last && rel <= end)) {
      if (rel <= s.byteStart) return { seg: i, utf16: 0, side: "start" };
      if (s.kind === "text") return { seg: i, utf16: byteToUtf16(s.text, rel - s.byteStart), side: "in" };
      return { seg: i, utf16: 0, side: "start" };
    }
  }
  const s = segments[last];
  return { seg: last, utf16: s.kind === "text" ? s.text.length : 0, side: "end" };
}
/** Inverse of flatToSegmentPoint. */
export function segmentPointToFlat(segments, point) {
  if (point.seg < 0 || !segments.length) return 0;
  const s = segments[point.seg];
  if (s.kind === "text") return s.byteStart + utf16ToByte(s.text, point.utf16);
  return s.byteStart + (point.side === "end" ? s.byteLen : 0);
}

/** For each new signature, the index of the reusable old entry or -1. Prefix, suffix, then multiset. */
export function planReconcile(oldSigs, newSigs) {
  const plan = new Array(newSigs.length).fill(-1);
  let lo = 0;
  while (lo < oldSigs.length && lo < newSigs.length && oldSigs[lo] === newSigs[lo]) { plan[lo] = lo; lo += 1; }
  let oh = oldSigs.length, nh = newSigs.length;
  while (oh > lo && nh > lo && oldSigs[oh - 1] === newSigs[nh - 1]) { plan[nh - 1] = oh - 1; oh -= 1; nh -= 1; }
  const buckets = new Map();
  for (let i = lo; i < oh; i += 1) { const q = buckets.get(oldSigs[i]); if (q) q.push(i); else buckets.set(oldSigs[i], [i]); }
  for (let i = lo; i < nh; i += 1) { const q = buckets.get(newSigs[i]); if (q?.length) plan[i] = q.shift(); }
  return plan;
}

const isBig = v => typeof v === "bigint" && v >= 0n;
/** Checks a decoded AccessibleWindow reply; throws DocumentMirrorError(MIRROR_BAD_WINDOW). */
export function validateWindow(result) {
  if (!result || typeof result !== "object" || !isBig(result.revision) || !Array.isArray(result.items)) throw invalidInput("MIRROR_BAD_WINDOW", "Malformed window");
  if (result.items.length > MAX_ITEMS) throw invalidInput("MIRROR_BAD_WINDOW", "Too many items");
  let units = 0, previousEnd = -1n;
  const items = result.items.map(raw => {
    if (!raw || typeof raw !== "object" || !isBig(raw.paraStart) || !isBig(raw.paraEnd) || raw.paraEnd < raw.paraStart
      || !Number.isInteger(raw.role) || raw.role < 0 || raw.role > 4 || !Number.isInteger(raw.level) || raw.level < 0 || raw.level > 9
      || typeof raw.label !== "string" || typeof raw.text !== "string" || !Array.isArray(raw.objects)) throw invalidInput("MIRROR_BAD_WINDOW", "Malformed item");
    if (raw.paraStart <= previousEnd) throw invalidInput("MIRROR_BAD_WINDOW", "Items out of order");
    previousEnd = raw.paraEnd;
    units += raw.text.length + raw.label.length;
    if (units > MAX_TEXT_UNITS) throw invalidInput("MIRROR_BAD_WINDOW", "Text too large");
    const objects = raw.objects.map(o => {
      if (!o || !isBig(o.offset) || typeof o.alt !== "string") throw invalidInput("MIRROR_BAD_WINDOW", "Malformed object");
      return { offset: o.offset, alt: o.alt };
    });
    return { paraStart: raw.paraStart, paraEnd: raw.paraEnd, role: raw.role, level: raw.level, label: raw.label, text: raw.text, objects };
  });
  return { revision: result.revision, items };
}

export const signatureOf = item => `${item.role}|${item.level}|${item.label}|${item.text}|${item.objects.map(o => `${o.offset}:${o.alt}`).join(",")}`;

/** Text of segments between two relative byte offsets, objects given by their alt text. */
export function sliceSegments(segments, from, to, objectText) {
  let out = "";
  for (const s of segments) {
    const a = Math.max(from, s.byteStart), b = Math.min(to, s.byteStart + s.byteLen);
    if (b < a || (b === a && s.kind !== "text")) continue;
    if (s.kind === "text") out += s.text.slice(byteToUtf16(s.text, a - s.byteStart), byteToUtf16(s.text, b - s.byteStart));
    else if (s.kind === "object" && a === s.byteStart && b === s.byteStart + s.byteLen) out += objectText(s.alt);
  }
  return out;
}

function syncChildren(parent, wanted) {
  let cursor = parent.firstChild;
  for (const node of wanted) {
    if (node === cursor) cursor = cursor.nextSibling;
    else parent.insertBefore(node, cursor);
  }
  while (cursor) { const next = cursor.nextSibling; cursor.remove(); cursor = next; }
}

/**
 * @param {object} options
 * @param {Element} options.container parent of the mirror root
 * @param {(start: bigint, end: bigint, o: {signal: AbortSignal}) => Promise<object>} options.getAccessibleWindow
 * @param {(key: string, values?: object) => string} options.t translator (i18n.t)
 * @param {Element} [options.activeDescendantTarget] input that gets aria-activedescendant (off by default)
 * @param {boolean} [options.announce=true] live region for the current paragraph and selection
 * @param {boolean} [options.domSelection=true] mirror the model selection in the DOM selection
 * @param {bigint} [options.margin=4096n] flat bytes requested around the visible range
 * @param {string} [options.lang] language of the mirror
 * @param {(error: Error) => void} [options.onError]
 */
export function createDocumentMirror({ container, getAccessibleWindow, t, activeDescendantTarget = null, announce = true, domSelection = true,
  margin = 4096n, lang, onError = () => {} } = {}) {
  if (!container?.ownerDocument || typeof getAccessibleWindow !== "function" || typeof t !== "function") throw invalidInput("MIRROR_BAD_OPTIONS", "container, getAccessibleWindow and t are required");
  const doc = container.ownerDocument;
  const idPrefix = `document-mirror-${Math.random().toString(36).slice(2, 8)}`;
  const root = doc.createElement("div");
  root.className = "document-mirror";
  root.setAttribute("role", "document");
  root.setAttribute("aria-label", t("mirror.label"));
  if (lang) root.setAttribute("lang", lang);
  const content = doc.createElement("div");
  const live = doc.createElement("div");
  live.setAttribute("role", "status");
  live.setAttribute("aria-live", "polite");
  live.setAttribute("aria-atomic", "true");
  root.append(content, live);
  container.append(root);

  let entries = [];   // { sig, item, segs, node, nodes, id }
  let coverage = null; // { revision, start, end }
  let pending = null;  // { controller, revision, start, end, promise }
  let latest = null;
  let lastAnnounce = { key: "", revision: -1n, anchor: -1n, focus: -1n };
  let wrappers = [];
  let counter = 0;
  let disposed = false;
  const stats = { fetches: 0, aborted: 0, stale: 0, created: 0, reused: 0, removed: 0 };

  const altText = alt => alt.trim() ? alt : t("mirror.image.noalt");
  const headingLevel = item => Math.min(Math.max(item.level, 1), 9);

  function render(item, segs) {
    let el;
    if (item.role === ROLE_HEADING) {
      const level = headingLevel(item);
      if (level <= 6) el = doc.createElement(`h${level}`);
      else { el = doc.createElement("div"); el.setAttribute("role", "heading"); el.setAttribute("aria-level", String(level)); }
    } else if (item.role === ROLE_LIST_ITEM) {
      el = doc.createElement("div"); el.setAttribute("role", "listitem"); el.setAttribute("aria-level", String(item.level + 1));
    } else el = doc.createElement("p");
    el.dataset.mirrorRole = ROLE_NAMES[item.role];
    counter += 1;
    el.id = `${idPrefix}-${counter}`;
    let hasContent = false;
    if (item.role === ROLE_LIST_ITEM && item.label) {
      const label = doc.createElement("span");
      label.dataset.mirrorLabel = "";
      label.textContent = `${item.label} `;
      el.append(label);
    }
    const nodes = segs.map((s, i) => {
      let node = null;
      if (s.kind === "text") node = doc.createTextNode(s.text);
      else if (s.kind === "object") {
        node = doc.createElement("span");
        node.setAttribute("role", "img");
        node.setAttribute("aria-label", altText(s.alt));
        node.dataset.mirrorObject = "";
      } else if (item.role === ROLE_OPAQUE) node = doc.createTextNode(t("mirror.opaque"));
      else {
        const before = segs[i - 1], after = segs[i + 1];
        if (before?.kind === "text" && after?.kind === "text" && !/\s$/u.test(before.text) && !/^\s/u.test(after.text)) node = doc.createTextNode(" ");
      }
      if (node) { el.append(node); hasContent = true; }
      return node;
    });
    if (!hasContent) el.append(doc.createElement("br"));
    return { el, nodes };
  }

  function structure() {
    const top = [], runs = [];
    for (const entry of entries) {
      if (entry.item.role === ROLE_LIST_ITEM) {
        const last = top.at(-1);
        if (last?.run) last.run.push(entry.node);
        else { const run = [entry.node]; runs.push(run); top.push({ run }); }
      } else top.push({ node: entry.node });
    }
    while (wrappers.length < runs.length) {
      const wrapper = doc.createElement("div"); wrapper.setAttribute("role", "list"); wrappers.push(wrapper);
    }
    for (const wrapper of wrappers.slice(runs.length)) wrapper.remove();
    wrappers = wrappers.slice(0, runs.length);
    let k = 0;
    syncChildren(content, top.map(part => {
      if (!part.run) return part.node;
      const wrapper = wrappers[k]; syncChildren(wrapper, runs[k]); k += 1; return wrapper;
    }));
  }

  function applyWindow(window) {
    const sigs = window.items.map(signatureOf);
    const plan = planReconcile(entries.map(e => e.sig), sigs);
    const used = new Set(plan.filter(i => i >= 0));
    entries.forEach((entry, i) => { if (!used.has(i)) { entry.node.remove(); stats.removed += 1; } });
    const old = entries;
    entries = window.items.map((item, i) => {
      if (plan[i] >= 0) { stats.reused += 1; const e = old[plan[i]]; e.item = item; return e; }
      stats.created += 1;
      const segs = buildSegments(item);
      const { el, nodes } = render(item, segs);
      return { sig: sigs[i], item, segs, node: el, nodes, id: el.id };
    });
    structure();
  }

  /** Paragraph holding a flat position (clamped to the mirrored window) and the offset in it. */
  function entryAt(position) {
    if (!entries.length) return null;
    for (let i = 0; i < entries.length; i += 1) {
      const { paraStart, paraEnd } = entries[i].item;
      if (position <= paraEnd) return { index: i, rel: position < paraStart ? 0 : Number(position - paraStart) };
    }
    const last = entries.length - 1;
    return { index: last, rel: Number(entries[last].item.paraEnd - entries[last].item.paraStart) };
  }

  function domPoint(position) {
    const at = entryAt(position);
    if (!at) return null;
    const { node, segs, nodes } = entries[at.index];
    const point = flatToSegmentPoint(segs, at.rel);
    if (point.seg < 0) return { node, offset: 0 };
    const target = nodes[point.seg];
    if (segs[point.seg].kind === "text") return { node: target, offset: point.utf16 };
    const after = point.side === "end";
    const children = [...node.childNodes];
    if (target) return { node, offset: children.indexOf(target) + (after ? 1 : 0) };
    for (let j = point.seg + (after ? 1 : 0); j < nodes.length; j += 1) if (nodes[j]) return { node, offset: children.indexOf(nodes[j]) };
    return { node, offset: children.length };
  }

  function say(message) {
    if (!announce) return;
    live.textContent = message === live.textContent ? `${message} ` : message;
  }
  const plain = entry => {
    let out = "";
    for (const n of entry.node.childNodes) out += n.nodeType === 3 ? n.data : n.getAttribute?.("aria-label") ?? n.textContent;
    return out.trim();
  };
  const clip = (text, max) => text.length > max ? `${text.slice(0, max)}…` : text;

  function describe(entry) {
    const text = clip(plain(entry) || t("mirror.empty"), MAX_PARAGRAPH_ANNOUNCE);
    const item = entry.item;
    const prefix = item.role === ROLE_HEADING ? t("mirror.heading", { level: headingLevel(item) })
      : item.role === ROLE_LIST_ITEM ? t("mirror.list.item", { level: item.level + 1 })
      : item.role === ROLE_CELL ? t("mirror.cell") : "";
    return prefix ? `${prefix} : ${text}` : text;
  }

  function announceSelection(state, at, current) {
    const { anchor, focus } = state.selection;
    const last = lastAnnounce;
    lastAnnounce = { key: current.id, revision: state.revision, anchor, focus };
    if (anchor === last.anchor && focus === last.focus) return;
    if (anchor !== focus) {
      const lo = anchor < focus ? anchor : focus, hi = anchor < focus ? focus : anchor;
      let text = "";
      for (const e of entries) {
        if (e.item.paraEnd < lo || e.item.paraStart > hi) continue;
        const from = lo > e.item.paraStart ? Number(lo - e.item.paraStart) : 0;
        const to = hi < e.item.paraEnd ? Number(hi - e.item.paraStart) : Number(e.item.paraEnd - e.item.paraStart);
        text += `${text ? " " : ""}${sliceSegments(e.segs, from, to, altText)}`;
      }
      say(t("mirror.selection", { text: clip(text, MAX_SELECTION_ANNOUNCE) }));
    } else if (last.key !== current.id) say(describe(current));
    else if (state.revision === last.revision) {
      // Same paragraph, caret moved without an edit: announce the character after the caret.
      const text = plain(current);
      const rest = text.slice(byteToUtf16(text, at.rel));
      const next = typeof Intl.Segmenter === "function" ? [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(rest)][0]?.segment : Array.from(rest)[0];
      if (next) say(next);
    }
  }

  function syncSelection(state) {
    const { anchor, focus } = state.selection;
    const a = domPoint(anchor), f = domPoint(focus);
    const at = entryAt(focus);
    const current = at ? entries[at.index] : null;
    if (activeDescendantTarget) {
      if (current) activeDescendantTarget.setAttribute("aria-activedescendant", current.id);
      else activeDescendantTarget.removeAttribute("aria-activedescendant");
    }
    if (domSelection && a && f) {
      try { doc.defaultView.getSelection().setBaseAndExtent(a.node, a.offset, f.node, f.offset); } catch (error) { onError(error); }
    }
    if (announce && current) announceSelection(state, at, current);
  }

  function wanted(state) {
    const clamp = v => v < 0n ? 0n : v > state.docLen ? state.docLen : v;
    const from = state.visible?.start ?? state.selection.focus, to = state.visible?.end ?? state.selection.focus;
    return { start: clamp(from - margin), end: clamp(to + margin) };
  }

  function checkState(state) {
    const s = state?.selection;
    if (!state || !isBig(state.revision) || !isBig(state.docLen) || !s || !isBig(s.anchor) || !isBig(s.focus) || s.anchor > state.docLen || s.focus > state.docLen
      || (state.visible && (!isBig(state.visible.start) || !isBig(state.visible.end) || state.visible.end < state.visible.start))) throw invalidInput("MIRROR_BAD_STATE", "Malformed state");
  }

  /**
   * Call with each State and whenever the visible range changes:
   * `{ revision, docLen, selection: { anchor, focus }, visible?: { start, end } }` (BigInt flat positions).
   * Resolves `{ status }`: "applied", "selection" (window already current), "stale", "error" or "disposed".
   */
  function update(state) {
    if (disposed) return Promise.resolve({ status: "disposed" });
    try { checkState(state); } catch (error) { onError(error); return Promise.resolve({ status: "error" }); }
    latest = state;
    const want = wanted(state);
    if (coverage && coverage.revision === state.revision && coverage.start <= want.start && coverage.end >= want.end) {
      syncSelection(state);
      return Promise.resolve({ status: "selection" });
    }
    if (pending && pending.revision === state.revision && pending.start <= want.start && pending.end >= want.end) return pending.promise;
    if (pending) { pending.controller.abort(); stats.aborted += 1; }
    const controller = new AbortController();
    const request = { controller, revision: state.revision, start: want.start, end: want.end, promise: null };
    pending = request;
    stats.fetches += 1;
    request.promise = (async () => {
      try {
        const raw = await getAccessibleWindow(want.start, want.end, { signal: controller.signal });
        if (disposed || controller.signal.aborted || pending !== request) { stats.stale += 1; return { status: "stale" }; }
        const window = validateWindow(raw);
        if (window.revision !== request.revision || latest.revision !== request.revision) { stats.stale += 1; return { status: "stale" }; }
        applyWindow(window);
        coverage = window.items.length >= MAX_ITEMS
          ? { revision: window.revision, start: window.items[0].paraStart, end: window.items.at(-1).paraEnd }
          : { revision: window.revision, start: want.start, end: want.end };
        syncSelection(latest);
        return { status: "applied" };
      } catch (error) {
        if (controller.signal.aborted || error?.name === "AbortError") { stats.stale += 1; return { status: "stale" }; }
        onError(error instanceof DocumentMirrorError ? error : new DocumentMirrorError("MIRROR_WINDOW_FAILED", "Window request failed", { cause: error }));
        return { status: "error" };
      } finally { if (pending === request) pending = null; }
    })();
    return request.promise;
  }

  function dispose() {
    if (disposed) return;
    disposed = true;
    pending?.controller.abort();
    activeDescendantTarget?.removeAttribute("aria-activedescendant");
    root.remove();
    entries = []; wrappers = [];
  }

  return Object.freeze({ element: root, update, dispose, stats, entryCount: () => entries.length });
}
