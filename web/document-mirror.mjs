// SPDX-License-Identifier: AGPL-3.0-or-later
// US-209 (R5): accessible DOM mirror of the visible part of the Canvas document (SPEC 6.14, D13).
// The mirror is built with createElement/textContent only. The data comes from an injected
// `getAccessibleWindow(start, end, { signal })` (flat positions, see docs/us-209-document-mirror.md).

const FFFC = "￼";
export const MAX_ITEMS = 256;
export const MAX_TEXT_UNITS = 256 * 1024;
const SELECTION_TEXT_LIMIT = 100;
const MAX_PARAGRAPH_ANNOUNCE = 400;
/** UTF-16 units of the paragraph given to the input in screen reader mode, around the caret. */
export const READER_WINDOW = 2000;
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
 * Splits a paragraph into segments with byte ranges relative to paraStart: text runs and one segment per
 * U+FFFC: object (inline kind 0, alt text), break (kinds 1 and 2) or symbol (kind 3, displayable value).
 * Every non-text segment is 3 flat bytes; a symbol is `value.length` UTF-16 units in the DOM.
 */
export function buildSegments(item) {
  const inlines = new Map();
  for (const inline of item.inlines) inlines.set(Number(inline.offset), inline);
  const out = [];
  let byte = 0, buffer = "", start = 0;
  const flush = () => { if (buffer) { out.push({ kind: "text", byteStart: start, byteLen: byte - start, text: buffer }); buffer = ""; } };
  for (const ch of item.text) {
    if (ch === FFFC) {
      flush();
      const inline = inlines.get(byte);
      out.push(inline?.kind === 0 ? { kind: "object", byteStart: byte, byteLen: 3, alt: inline.value }
        : inline?.kind === 3 ? { kind: "symbol", byteStart: byte, byteLen: 3, value: inline.value }
        : { kind: "break", byteStart: byte, byteLen: 3, page: inline?.kind === 2 });
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
      return { seg: i, utf16: 0, side: rel >= end ? "end" : "start" };
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
const inlineKey = i => `${i.kind}:${i.value}`;
export const signatureOf = item => `${item.role}|${item.level}|${item.label}|${item.text}|${item.inlines.map(i => `${i.offset}:${inlineKey(i)}`).join(",")}`;
/** Same structure (role, level, label, inline kinds and values) whatever the text: the node can be updated in place. */
export const structureOf = item => `${item.role}|${item.level}|${item.label}|${item.inlines.map(inlineKey).join(",")}`;

/** Checks a decoded AccessibleWindow reply; throws DocumentMirrorError(MIRROR_BAD_WINDOW). */
export function validateWindow(result) {
  if (!result || typeof result !== "object" || !isBig(result.revision) || !Array.isArray(result.items)) throw invalidInput("MIRROR_BAD_WINDOW", "Malformed window");
  if (result.items.length > MAX_ITEMS) throw invalidInput("MIRROR_BAD_WINDOW", "Too many items");
  let units = 0, previousEnd = -1n;
  const items = result.items.map(raw => {
    if (!raw || typeof raw !== "object" || !isBig(raw.paraStart) || !isBig(raw.paraEnd) || raw.paraEnd < raw.paraStart
      || !Number.isInteger(raw.role) || raw.role < 0 || raw.role > 4 || !Number.isInteger(raw.level) || raw.level < 0 || raw.level > 9
      || typeof raw.label !== "string" || typeof raw.text !== "string" || !Array.isArray(raw.inlines)) throw invalidInput("MIRROR_BAD_WINDOW", "Malformed item");
    if (raw.paraStart <= previousEnd) throw invalidInput("MIRROR_BAD_WINDOW", "Items out of order");
    previousEnd = raw.paraEnd;
    const offsets = [];
    let bytes = 0;
    for (const ch of raw.text) { if (ch === FFFC) offsets.push(BigInt(bytes)); bytes += bytesOf(ch.codePointAt(0)); }
    if (raw.paraEnd - raw.paraStart !== BigInt(bytes)) throw invalidInput("MIRROR_BAD_WINDOW", "Paragraph range does not match its text");
    units += bytes + raw.label.length;
    const inlines = raw.inlines.map(o => {
      if (!o || typeof o !== "object" || !isBig(o.offset) || !Number.isInteger(o.kind) || o.kind < 0 || o.kind > 3 || typeof o.value !== "string") throw invalidInput("MIRROR_BAD_WINDOW", "Malformed inline");
      units += o.value.length;
      return { offset: o.offset, kind: o.kind, value: o.value };
    }).sort((x, y) => x.offset < y.offset ? -1 : x.offset > y.offset ? 1 : 0);
    if (units > MAX_TEXT_UNITS) throw invalidInput("MIRROR_BAD_WINDOW", "Text too large");
    if (inlines.length !== offsets.length || inlines.some((o, i) => o.offset !== offsets[i])) throw invalidInput("MIRROR_BAD_WINDOW", "Each U+FFFC needs exactly one inline");
    return { paraStart: raw.paraStart, paraEnd: raw.paraEnd, role: raw.role, level: raw.level, label: raw.label, text: raw.text, inlines };
  });
  return { revision: result.revision, items };
}

/** Text of segments between two relative byte offsets: alt text for objects, value for symbols, a space for breaks. */
export function sliceSegments(segments, from, to, objectText) {
  let out = "";
  for (const s of segments) {
    const a = Math.max(from, s.byteStart), b = Math.min(to, s.byteStart + s.byteLen);
    if (b < a || (b === a && s.kind !== "text")) continue;
    if (s.kind === "text") out += s.text.slice(byteToUtf16(s.text, a - s.byteStart), byteToUtf16(s.text, b - s.byteStart));
    else if (a === s.byteStart && b === s.byteStart + s.byteLen) out += s.kind === "object" ? objectText(s.alt) : s.kind === "symbol" ? s.value : " ";
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
 * @param {object} [options.input] text input handle (web/text-input.mjs): receives screen reader mode and content
 * @param {Element} [options.activeDescendantTarget] input that gets aria-activedescendant (off by default)
 * @param {boolean} [options.announce=true] live region for the current paragraph and selection
 * @param {boolean} [options.domSelection=true] mirror the model selection in the DOM selection
 * @param {number} [options.announceDelay=150] ms over which announcements are coalesced (0: immediate)
 * @param {(fn: () => void, ms: number) => unknown} [options.schedule] timer, replaceable in tests
 * @param {bigint} [options.margin=4096n] flat bytes requested around the visible range
 * @param {string} [options.lang] language of the mirror (default: the page language)
 * @param {(error: Error) => void} [options.onError]
 */
export function createDocumentMirror({ container, getAccessibleWindow, t, input = null, activeDescendantTarget = null, announce = true, domSelection = true,
  announceDelay = 150, schedule = (fn, ms) => setTimeout(fn, ms), margin = 4096n, lang, onError = () => {} } = {}) {
  if (!container?.ownerDocument || typeof getAccessibleWindow !== "function" || typeof t !== "function") throw invalidInput("MIRROR_BAD_OPTIONS", "container, getAccessibleWindow and t are required");
  const doc = container.ownerDocument;
  const idPrefix = `document-mirror-${Math.random().toString(36).slice(2, 8)}`;
  const root = doc.createElement("div");
  root.className = "document-mirror";
  root.setAttribute("role", "document");
  root.setAttribute("aria-label", t("mirror.label"));
  const language = lang ?? doc.documentElement?.lang;
  if (language) root.setAttribute("lang", language);
  const content = doc.createElement("div");
  const live = doc.createElement("div");
  live.setAttribute("role", "status");
  live.setAttribute("aria-live", "polite");
  live.setAttribute("aria-atomic", "true");
  root.append(content, live);
  container.append(root);

  let entries = [];   // { sig, key, item, segs, node, nodes, id }
  let coverage = null; // { revision, start, end }: the range asked for, even if the reply was truncated
  let pending = null;  // { controller, revision, start, end, promise }
  let latest = null;
  let lastAnnounce = { key: -1n, revision: -1n, anchor: -1n, focus: -1n };
  let announceTimer = null;
  let readerMode = input?.screenReaderMode?.() === true;
  let readerCross = false;
  let unsubscribe = null;
  let wrappers = [];
  let counter = 0;
  let disposed = false;
  const stats = { fetches: 0, aborted: 0, stale: 0, created: 0, reused: 0, updated: 0, removed: 0 };

  const altText = alt => alt.trim() ? alt : t("mirror.image.noalt");
  const headingLevel = item => Math.min(Math.max(item.level, 1), 9);

  function makeElement(item) {
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
    return el;
  }

  /** Fills `el` with the label and one DOM node per segment; returns the node of each segment (or null). */
  function fill(el, item, segs) {
    let hasContent = false;
    if (item.role === ROLE_LIST_ITEM && item.label) {
      const label = doc.createElement("span");
      label.dataset.mirrorLabel = "";
      label.textContent = `${item.label} `;
      el.append(label);
    }
    const nodes = segs.map(s => {
      let node = null;
      if (s.kind === "text") node = doc.createTextNode(s.text);
      else if (item.role === ROLE_OPAQUE) node = doc.createTextNode(t("mirror.opaque"));
      else if (s.kind === "object") {
        node = doc.createElement("span");
        node.setAttribute("role", "img");
        node.setAttribute("aria-label", altText(s.alt));
        node.dataset.mirrorObject = "";
      } else if (s.kind === "symbol") { if (s.value) node = doc.createTextNode(s.value); }
      else node = doc.createElement("br");
      if (node) { el.append(node); hasContent = true; }
      return node;
    });
    if (!hasContent) el.append(doc.createElement("br"));
    return nodes;
  }

  /** Same structure, other text: change the text nodes in place and keep the element (and its id). */
  function updateInPlace(entry, item) {
    const segs = buildSegments(item);
    const sameShape = segs.length === entry.segs.length && segs.every((s, i) => s.kind === entry.segs[i].kind);
    if (sameShape) segs.forEach((s, i) => { if (s.kind === "text" && entry.nodes[i].data !== s.text) entry.nodes[i].data = s.text; });
    else {
      while (entry.node.firstChild) entry.node.firstChild.remove();
      entry.nodes = fill(entry.node, item, segs);
    }
    entry.segs = segs;
    entry.item = item;
    entry.sig = signatureOf(item);
    stats.updated += 1;
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
    const old = entries;
    const plan = planReconcile(old.map(e => e.sig), sigs);
    const used = new Set(plan.filter(i => i >= 0));
    const exact = plan.map(from => from >= 0);
    // Paragraphs whose text alone changed (typing): pair them, in order, with an unused old entry of the same structure.
    let floor = 0;
    plan.forEach((from, i) => {
      if (from >= 0) { floor = from + 1; return; }
      for (let j = floor; j < old.length; j += 1) {
        if (!used.has(j) && old[j].key === structureOf(window.items[i])) { plan[i] = j; used.add(j); floor = j + 1; updateInPlace(old[j], window.items[i]); return; }
      }
    });
    old.forEach((entry, i) => { if (!used.has(i)) { entry.node.remove(); stats.removed += 1; } });
    entries = window.items.map((item, i) => {
      if (plan[i] >= 0) { const e = old[plan[i]]; if (exact[i]) stats.reused += 1; e.item = item; return e; }
      stats.created += 1;
      const segs = buildSegments(item);
      const el = makeElement(item);
      return { sig: sigs[i], key: structureOf(item), item, segs, node: el, nodes: fill(el, item, segs), id: el.id };
    });
    structure();
  }

  /**
   * Paragraph holding a flat position and the offset in it. Outside the mirrored window: null, or with `clamp`
   * the nearest edge.
   */
  function entryAt(position, clamp = false) {
    if (!entries.length) return null;
    const first = entries[0].item, last = entries.at(-1).item;
    if (position > last.paraEnd) return clamp ? { index: entries.length - 1, rel: Number(last.paraEnd - last.paraStart) } : null;
    if (position < first.paraStart) return clamp ? { index: 0, rel: 0 } : null;
    for (let i = 0; i < entries.length; i += 1) {
      const { paraStart, paraEnd } = entries[i].item;
      if (position <= paraEnd) return { index: i, rel: position < paraStart ? 0 : Number(position - paraStart) };
    }
    return null;
  }

  function domPoint(position, clamp) {
    const at = entryAt(position, clamp);
    if (!at) return null;
    const { node, segs, nodes } = entries[at.index];
    const point = flatToSegmentPoint(segs, at.rel);
    if (point.seg < 0) return { node, offset: 0 };
    const target = nodes[point.seg];
    const kind = segs[point.seg].kind;
    const after = point.side === "end";
    if (target && (kind === "text" || kind === "symbol")) return { node: target, offset: kind === "text" ? point.utf16 : after ? target.data.length : 0 };
    const children = [...node.childNodes];
    if (target) return { node, offset: children.indexOf(target) + (after ? 1 : 0) };
    for (let j = point.seg + (after ? 1 : 0); j < nodes.length; j += 1) if (nodes[j]) return { node, offset: children.indexOf(nodes[j]) };
    return { node, offset: children.length };
  }

  function say(message, always = false) {
    if (!announce && !always) return;
    live.textContent = message === live.textContent ? `${message} ` : message;
  }
  const plain = entry => {
    let out = "";
    for (const n of entry.node.childNodes) out += n.nodeType === 3 ? n.data : n.tagName === "BR" ? " " : n.getAttribute?.("aria-label") ?? n.textContent;
    return out.trim();
  };
  const clip = (text, max) => text.length > max ? `${text.slice(0, max)}…` : text;
  const firstGrapheme = text => typeof Intl.Segmenter === "function" ? [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text)][0]?.segment : Array.from(text)[0];

  /** What follows the caret at `rel`, found through the segments (no text search): a character, alt text, a symbol. */
  function charAfter(entry, rel) {
    const point = flatToSegmentPoint(entry.segs, rel);
    if (point.seg < 0) return null;
    for (let i = point.seg; i < entry.segs.length; i += 1) {
      const s = entry.segs[i];
      if (s.kind === "text") {
        const rest = s.text.slice(i === point.seg ? point.utf16 : 0);
        if (rest) return firstGrapheme(rest);
      } else if (i === point.seg && point.side === "end") return null;
      else if (s.kind === "object") return altText(s.alt);
      else if (s.kind === "symbol") { if (s.value) return s.value; } else return null;
    }
    return null;
  }

  function describe(entry) {
    const text = clip(plain(entry) || t("mirror.empty"), MAX_PARAGRAPH_ANNOUNCE);
    const item = entry.item;
    const prefix = item.role === ROLE_HEADING ? t("mirror.heading", { level: headingLevel(item) })
      : item.role === ROLE_LIST_ITEM ? t("mirror.list.item", { level: item.level + 1 })
      : item.role === ROLE_CELL ? t("mirror.cell") : "";
    return prefix ? t("mirror.prefixed", { prefix, text }) : text;
  }

  /** Text between two flat positions inside the mirrored window. */
  function rangeText(lo, hi) {
    let text = "";
    for (const e of entries) {
      if (e.item.paraEnd < lo || e.item.paraStart > hi) continue;
      const from = lo > e.item.paraStart ? Number(lo - e.item.paraStart) : 0;
      const to = hi < e.item.paraEnd ? Number(hi - e.item.paraStart) : Number(e.item.paraEnd - e.item.paraStart);
      text += `${text ? " " : ""}${sliceSegments(e.segs, from, to, altText)}`;
    }
    return text;
  }
  const selectionMessage = (text, textKey, countKey) => {
    const count = Array.from(text).length;
    return count > SELECTION_TEXT_LIMIT ? t(countKey, { count }) : t(textKey, { text });
  };

  function flushAnnounce() {
    announceTimer = null;
    if (!announce || disposed || !latest) return;
    const { revision, selection } = latest;
    const { anchor, focus } = selection;
    const at = entryAt(focus);
    if (!at) return; // focus outside the mirrored window: nothing is said
    const current = entries[at.index];
    const last = lastAnnounce;
    lastAnnounce = { key: current.item.paraStart, revision, anchor, focus };
    if (anchor === last.anchor && focus === last.focus) return;
    const anchorAt = entryAt(anchor);
    // In screen reader mode the input reads the caret and the selection itself, except across paragraphs.
    if (readerMode && anchorAt?.index === at.index) return;
    const wasRange = last.anchor !== last.focus;
    // Reader mode, selection across paragraphs: the input only holds the focus paragraph. Say so once.
    const cross = readerMode && anchor !== focus;
    const wasCross = readerCross;
    readerCross = cross;
    const sayRange = message => say(cross && !wasCross ? t("mirror.reader.truncated", { selection: message }) : message);
    if (anchor !== focus) {
      const lo = anchor < focus ? anchor : focus, hi = anchor < focus ? focus : anchor;
      if (wasRange && last.anchor === anchor && last.revision === revision) {
        const dlo = last.focus < focus ? last.focus : focus, dhi = last.focus < focus ? focus : last.focus;
        const grew = (focus > anchor ? focus - anchor : anchor - focus) > (last.focus > anchor ? last.focus - anchor : anchor - last.focus);
        sayRange(grew ? selectionMessage(rangeText(dlo, dhi), "mirror.selection.added", "mirror.selection.count")
          : selectionMessage(rangeText(dlo, dhi), "mirror.selection.removed", "mirror.selection.uncount"));
      } else sayRange(selectionMessage(rangeText(lo, hi), "mirror.selection", "mirror.selection.count"));
    } else if (last.key !== current.item.paraStart) say(describe(current));
    else if (wasRange) say(t("mirror.selection.cleared"));
    else if (revision === last.revision) {
      // Same paragraph, caret moved without an edit: say what now follows the caret.
      const next = charAfter(current, at.rel);
      if (next) say(next);
    }
  }

  function requestAnnounce() {
    if (!announce) return;
    if (announceDelay <= 0) { flushAnnounce(); return; }
    if (announceTimer === null) announceTimer = schedule(flushAnnounce, announceDelay);
  }

  /** Text of the current paragraph for the focused input, with the selection clamped to it (screen reader mode). */
  function readerContent(state) {
    const { anchor, focus } = state.selection;
    const at = entryAt(focus);
    if (!at) return null;
    const entry = entries[at.index];
    const prefix = entry.item.role === ROLE_LIST_ITEM && entry.item.label ? `${entry.item.label} ` : "";
    const pieces = entry.segs.map(s => entry.item.role === ROLE_OPAQUE && s.kind !== "text" ? t("mirror.opaque")
      : s.kind === "text" ? s.text : s.kind === "object" ? altText(s.alt) : s.kind === "symbol" ? s.value : "\n");
    const text = prefix + pieces.join("");
    const offsetOf = position => {
      if (position < entry.item.paraStart) return 0;
      if (position > entry.item.paraEnd) return text.length;
      const point = flatToSegmentPoint(entry.segs, Number(position - entry.item.paraStart));
      let offset = prefix.length;
      if (point.seg < 0) return offset;
      for (let i = 0; i < point.seg; i += 1) offset += pieces[i].length;
      if (entry.segs[point.seg].kind === "text") offset += point.utf16;
      else if (point.side === "end") offset += pieces[point.seg].length;
      return offset;
    };
    const a = offsetOf(anchor), f = offsetOf(focus);
    // Long paragraph: only a window of READER_WINDOW units around the focus goes to the input.
    let lo = 0, hi = text.length;
    if (text.length > READER_WINDOW) {
      lo = Math.max(0, Math.min(f - READER_WINDOW / 2, text.length - READER_WINDOW));
      hi = lo + READER_WINDOW;
      if (/[\udc00-\udfff]/.test(text[lo])) lo += 1;
      if (/[\ud800-\udbff]/.test(text[hi - 1])) hi -= 1;
    }
    const inside = x => Math.min(Math.max(x, lo), hi) - lo;
    return { text: text.slice(lo, hi), start: Math.min(inside(a), inside(f)), end: Math.max(inside(a), inside(f)), backward: f < a };
  }

  function syncSelection(state) {
    const { anchor, focus } = state.selection;
    const f = domPoint(focus, false);
    if (!f) return; // focus outside the mirrored window: leave the DOM selection and the input alone
    const a = domPoint(anchor, true);
    const at = entryAt(focus);
    const current = entries[at.index];
    if (activeDescendantTarget) activeDescendantTarget.setAttribute("aria-activedescendant", current.id);
    if (readerMode) {
      try { input?.setScreenReaderContent?.(readerContent(state)); } catch (error) { onError(error); }
    } else if (domSelection) {
      try { doc.defaultView.getSelection().setBaseAndExtent(a.node, a.offset, f.node, f.offset); } catch (error) { onError(error); }
    }
    requestAnnounce();
  }

  function wanted(state) {
    const clamp = v => v < 0n ? 0n : v > state.docLen ? state.docLen : v;
    const { focus } = state.selection;
    let from = state.visible?.start ?? focus, to = state.visible?.end ?? focus;
    if (focus < from - margin || focus > to + margin) { from = focus; to = focus; } // the focus is what is read
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
        coverage = { revision: window.revision, start: want.start, end: want.end };
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

  /**
   * Screen reader mode (off by default): the focused input holds the current paragraph, with the selection, so that
   * the reader reads caret and selection natively; the mirror stays available in browse mode.
   */
  function setScreenReaderMode(enabled) {
    const on = Boolean(enabled);
    if (disposed || on === readerMode) return;
    readerMode = on;
    readerCross = false;
    input?.setScreenReaderMode?.(on);
    say(t(on ? "mirror.reader.on" : "mirror.reader.off"), true);
    if (latest) syncSelection(latest);
  }
  // The input is the other half of the mode: any change made there (shortcut, button) reaches the mirror, never out of sync.
  unsubscribe = input?.onScreenReaderModeChange?.(on => setScreenReaderMode(on)) ?? null;
  if (readerMode) input?.setScreenReaderMode?.(true);

  function dispose() {
    if (disposed) return;
    disposed = true;
    pending?.controller.abort();
    unsubscribe?.();
    if (readerMode) input?.setScreenReaderMode?.(false);
    activeDescendantTarget?.removeAttribute("aria-activedescendant");
    root.remove();
    entries = []; wrappers = [];
  }

  return Object.freeze({ element: root, update, dispose, setScreenReaderMode, screenReaderMode: () => readerMode, stats, entryCount: () => entries.length });
}

/**
 * Visible toggle for screen reader mode, for users who cannot use Alt+Shift+F1. It follows the input whatever changes the
 * mode (shortcut, API, this button). Place `element` in the options or the ribbon.
 * @param {{container: Element, input: object, t: (key: string, values?: object) => string}} options
 */
export function createReaderModeToggle({ container, input, t } = {}) {
  if (!container?.ownerDocument || typeof input?.setScreenReaderMode !== "function" || typeof input?.onScreenReaderModeChange !== "function" || typeof t !== "function") {
    throw invalidInput("MIRROR_BAD_OPTIONS", "container, input and t are required");
  }
  const button = container.ownerDocument.createElement("button");
  button.type = "button";
  button.className = "document-mirror-reader-toggle";
  button.setAttribute("aria-keyshortcuts", "Alt+Shift+F1");
  button.textContent = t("mirror.reader.toggle");
  button.title = t("mirror.reader.toggle.hint", { shortcut: t("mirror.reader.shortcut") });
  const show = on => button.setAttribute("aria-pressed", String(on));
  show(input.screenReaderMode() === true);
  const unsubscribe = input.onScreenReaderModeChange(show);
  const click = () => { input.setScreenReaderMode(!input.screenReaderMode()); input.focus?.(); };
  button.addEventListener("click", click);
  container.append(button);
  return Object.freeze({ element: button, dispose() { unsubscribe(); button.removeEventListener("click", click); button.remove(); } });
}
