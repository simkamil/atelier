// SPDX-License-Identifier: AGPL-3.0-or-later
// Client state of one ATED v2 rich-editor document (US-203, R4). Applies the
// replies decoded by web/rich-editor-protocol.mjs following the client rules of
// docs/us-203-rich-editor-protocol.md: invalidations first, then pages, then the
// page count, selection with affinity, flags and text window. Display lists
// arrive as views into the received frame and are copied here, so the cache
// never retains a reply frame. Application is atomic: a reply that fails the
// checks leaves the state untouched.
//
// Glyph atlas (R2b): pages draw glyphs with `drawGlyphRun` of font 0 whose glyph
// id is an atlas index. State and Pages replies carry the entries that are new
// since the previous reply (`atlas.first` = the number the client already
// knows, `atlas.epoch` changes when the engine numbers from 0 again). The atlas
// is kept between pages, bounded by RICH_LIMITS.maxAtlasEntries, emptied only by
// an epoch change or reset(); dropPages() keeps it. `glyphResolver` is the
// `resolveGlyph` of CanvasRenderer; each Path2D is built once.
import { RICH_LIMITS } from "./rich-editor-protocol.mjs";
import { decodeDisplayList } from "./display-list.mjs";
import { makePath } from "./canvas-renderer.mjs";

/** Client bound on the bytes of the ATDL entries of one atlas (the engine bound is 4096 entries of at most 64 KiB). */
export const MAX_ATLAS_CLIENT_BYTES = 16 * 1024 * 1024;

export class RichClientError extends Error {
  /** `code` is a RICH_* engine error name or a CLIENT_* name; `detail` carries the optional cause. */
  constructor(code, message = code, detail = undefined) {
    super(message);
    this.name = "RichClientError";
    this.code = code;
    if (detail !== undefined) this.cause = detail;
  }
}

function invalid(message) { throw new RichClientError("CLIENT_BAD_REPLY", message); }

export class EditorPageCache {
  /** @param {{onChange?: (change: object) => void, maxCachedPages?: number, margin?: number}} [options] */
  constructor({ onChange, maxCachedPages = 256, margin = 8 } = {}) {
    if (!Number.isInteger(maxCachedPages) || maxCachedPages < 1 || !Number.isInteger(margin) || margin < 0) throw new TypeError("Invalid cache bounds.");
    this.maxCachedPages = maxCachedPages;
    this.margin = margin;
    this.visible = null;
    if (onChange !== undefined && typeof onChange !== "function") throw new TypeError("onChange must be a function.");
    this.onChange = onChange;
    this.pageMap = new Map();
    this.atlas = [];
    this.atlasBytes = 0;
    this.atlasEpoch = null;
    this.version = 0;
    this.glyphResolver = (fontId, glyphId) => this.#glyph(fontId, glyphId);
    this.reset();
  }

  /** Forget everything (document opened, closed or replaced). */
  reset() {
    this.pageMap.clear();
    this.atlas = [];
    this.atlasBytes = 0;
    this.atlasEpoch = null;
    this.revision = null;
    this.pageCount = 0;
    this.docLen = 0n;
    this.selection = { anchor: 0n, focus: 0n, upstream: false };
    this.caret = null;
    this.modified = false;
    this.readOnly = false; // US-218: DOCX with revisions or comments (edits refused by the engine)
    this.hasRevisions = false;
    this.canUndo = false;
    this.canRedo = false;
    this.layoutComplete = false;
    this.textWindow = null;
    this.acceptAnyRevision = true;
    this.version += 1;
  }

  /** Pages of [first, end) plus the margin are never evicted. */
  setVisible(first, end) { this.visible = { first, end }; }

  /** Drop every cached page but keep the rest (used when invalidations may have been lost). */
  dropPages() {
    this.pageMap.clear();
    this.acceptAnyRevision = true; // resync: the next State may carry any revision
    this.version += 1;
  }

  get size() { return this.pageMap.size; }

  /** Number of glyph atlas entries known. */
  get atlasSize() { return this.atlas.length; }

  #glyph(fontId, glyphId) {
    if ((fontId !== 0n && fontId !== 0) || !Number.isInteger(glyphId)) return null;
    const entry = this.atlas[glyphId];
    if (!entry) return null;
    entry.path ??= makePath(entry.elements);
    return entry;
  }

  /**
   * Check the atlas section of a reply without touching the state. An epoch
   * that differs from the known one means the atlas and the pages are emptied
   * (their glyph indices belong to the old numbering) before the entries are
   * stored; `first` must then be 0, otherwise the number of entries already known.
   * @returns {{fresh: boolean, epoch: number, decoded: object[], bytes: number}}
   */
  #checkAtlas(atlas) {
    if (!atlas || !Number.isInteger(atlas.epoch) || !Number.isInteger(atlas.first) || !Array.isArray(atlas.entries)) invalid("Atlas section is missing.");
    const fresh = atlas.epoch !== this.atlasEpoch;
    const known = fresh ? 0 : this.atlas.length;
    if (atlas.first !== known) {
      throw new RichClientError("CLIENT_BAD_REPLY", "Atlas entries do not follow the known ones.", { atlas: true, first: atlas.first, known });
    }
    if (known + atlas.entries.length > RICH_LIMITS.maxAtlasEntries) invalid("Atlas is over its bound.");
    let bytes = fresh ? 0 : this.atlasBytes;
    const decoded = atlas.entries.map((entry) => {
      let list;
      try { list = decodeDisplayList(entry); } catch { return invalid("Atlas entry is not a display list."); }
      const [command] = list.commands;
      if (list.commands.length !== 1 || command.type !== "fillPath" || command.rule !== "nonZero"
          || command.color.r !== 0 || command.color.g !== 0 || command.color.b !== 0 || command.color.a !== 1) invalid("Atlas entry must be one black nonzero contour.");
      bytes += entry.length;
      return { elements: command.path, path: null };
    });
    if (bytes > MAX_ATLAS_CLIENT_BYTES) throw new RichClientError("CLIENT_BAD_REPLY", "Atlas is over the client byte bound.", { atlasBytes: true });
    return { fresh, epoch: atlas.epoch, decoded, bytes };
  }

  /** Store a checked atlas section (cannot fail). @returns {boolean} true when a known epoch was replaced (pages were dropped) */
  #commitAtlas({ fresh, epoch, decoded, bytes }) {
    const replaced = fresh && this.atlasEpoch !== null;
    if (fresh) {
      this.atlas = [];
      this.atlasEpoch = epoch;
      this.pageMap.clear();
    }
    this.atlas.push(...decoded);
    this.atlasBytes = bytes;
    return replaced;
  }

  /** Text window sent with the current revision, or null when it is outdated. */
  get window() {
    return this.textWindow !== null && this.textWindow.revision === this.revision ? this.textWindow : null;
  }

  has(index) { return this.pageMap.has(index); }

  /** Display list (ATDL bytes, owned by the cache) of a page, or null when unknown. */
  page(index) { return this.pageMap.get(index) ?? null; }

  /** Dense array for `CanvasRenderer.setPages`: unknown pages are replaced by `placeholder`. */
  displayLists(placeholder = null) {
    const out = new Array(this.pageCount);
    for (let i = 0; i < out.length; i += 1) out[i] = this.pageMap.get(i) ?? placeholder;
    return out;
  }

  /** Runs of at most 16 unknown pages inside [first, end), clamped to the page count. */
  missing(first, end) {
    const stop = Math.min(end, this.pageCount);
    const runs = [];
    for (let i = Math.max(first, 0); i < stop; i += 1) {
      if (this.pageMap.has(i)) continue;
      const last = runs[runs.length - 1];
      if (last && last.first + last.count === i && last.count < RICH_LIMITS.maxPagesPerResponse) last.count += 1;
      else runs.push({ first: i, count: 1 });
    }
    return runs;
  }

  /**
   * Apply a decoded reply (`decodeRichReply(...).response`).
   * @returns {{ignored: boolean, updated: number[], dropped: number, invalidated: number}}
   */
  apply(response) {
    let change;
    let atlasReset = false;
    const held = this.pageMap.size;
    // Validate the whole reply first, mutate afterwards: a reply that fails leaves the state untouched.
    const withAtlas = response?.type === "state" || response?.type === "pages";
    const atlas = withAtlas ? this.#checkAtlas(response.atlas) : null;
    switch (response?.type) {
      case "state": case "pages": case "textWindow": case "hitResult": case "closed": case "error": break;
      default: return invalid("Reply type is not a cache input.");
    }
    const ignored = response.type === "state" ? this.#checkState(response)
      : response.type === "pages" ? this.#checkPagesReply(response)
        : response.type === "textWindow" ? this.#checkWindow(response) : true;
    if (withAtlas) atlasReset = this.#commitAtlas(atlas);
    if (ignored) change = { ignored: true, updated: [], dropped: 0, invalidated: 0 };
    else if (response.type === "state") change = this.#applyState(response);
    else if (response.type === "pages") change = this.#applyPages(response);
    else change = this.#applyWindow(response);
    if (atlasReset) { change.atlasReset = true; change.dropped += held; }
    if (!change.ignored || atlasReset) {
      change.dropped += this.#evict();
      this.version += 1;
      if (this.onChange) {
        try { this.onChange(change); } catch { /* isolate user callbacks */ }
      }
    }
    return change;
  }

  /** Keep at most maxCachedPages pages: the ones farthest from the visible window (or the caret page) go first. */
  #evict() {
    if (this.pageMap.size <= this.maxCachedPages) return 0;
    const centre = this.visible ?? { first: this.caret?.page ?? 0, end: (this.caret?.page ?? 0) + 1 };
    const lo = centre.first - this.margin;
    const hi = centre.end + this.margin;
    const distance = (i) => (i < lo ? lo - i : i >= hi ? i - hi + 1 : 0);
    const candidates = [...this.pageMap.keys()].filter((i) => distance(i) > 0).sort((a, b) => distance(b) - distance(a));
    let dropped = 0;
    for (const index of candidates) {
      if (this.pageMap.size <= this.maxCachedPages) break;
      this.pageMap.delete(index);
      dropped += 1;
    }
    return dropped;
  }

  #checkPages(pages, pageCount) {
    let previous = -1;
    for (const page of pages) {
      if (!Number.isInteger(page?.index) || page.index <= previous || page.index >= pageCount) invalid("Page index is outside the page count.");
      if (!(page.display instanceof Uint8Array) || page.display.length < RICH_LIMITS.minPageDisplayBytes) invalid("Page display list is malformed.");
      previous = page.index;
    }
  }

  #store(pages, pageCount, ranges) {
    let dropped = 0;
    for (const [first, end] of ranges) {
      for (let i = first; i < Math.min(end, this.pageCount); i += 1) if (this.pageMap.delete(i)) dropped += 1;
    }
    if (pageCount < this.pageCount) {
      for (const index of [...this.pageMap.keys()]) if (index >= pageCount) { this.pageMap.delete(index); dropped += 1; }
    }
    this.pageCount = pageCount;
    const updated = [];
    for (const { index, display } of pages) {
      this.pageMap.set(index, display.slice());
      updated.push(index);
    }
    return { updated, dropped };
  }

  /** @returns {boolean} true when the reply is outdated and ignored; throws when it is malformed */
  #checkState(s) {
    if (!this.acceptAnyRevision && this.revision !== null && typeof s.revision === "bigint" && s.revision < this.revision) return true;
    if (typeof s.revision !== "bigint" || typeof s.anchor !== "bigint" || typeof s.focus !== "bigint" || typeof s.docLen !== "bigint") invalid("State positions must be BigInt.");
    if (!Number.isInteger(s.pageCount) || s.pageCount < 0 || s.pageCount > RICH_LIMITS.maxPages) invalid("State page count is out of range.");
    if (!Array.isArray(s.invalidated) || !Array.isArray(s.pages)) invalid("State lists are missing.");
    for (const range of s.invalidated) {
      if (!Array.isArray(range) || !Number.isInteger(range[0]) || !Number.isInteger(range[1]) || range[0] < 0 || range[0] >= range[1]) invalid("Invalidation range is malformed.");
    }
    if (s.anchor > s.docLen || s.focus > s.docLen) invalid("Selection lies outside the document.");
    const c = s.caret;
    if (c && (!Number.isInteger(c.page) || c.page < 0 || c.page >= s.pageCount || ![c.x, c.y, c.height].every(Number.isFinite) || c.height < 0)) invalid("Caret is malformed.");
    if (s.window && (typeof s.window.start !== "bigint" || typeof s.window.text !== "string")) invalid("State text window is malformed.");
    this.#checkPages(s.pages, s.pageCount);
    return false;
  }

  #applyState(s) {
    const { updated, dropped } = this.#store(s.pages, s.pageCount, s.invalidated);
    this.acceptAnyRevision = false;
    this.revision = s.revision;
    this.docLen = s.docLen;
    this.selection = { anchor: s.anchor, focus: s.focus, upstream: s.focusUpstream === true };
    this.caret = s.caret ? { ...s.caret } : null;
    this.modified = s.modified === true;
    this.readOnly = s.readOnly === true;
    this.hasRevisions = s.hasRevisions === true;
    this.canUndo = s.canUndo === true;
    this.canRedo = s.canRedo === true;
    this.layoutComplete = s.layoutComplete === true;
    if (s.window) this.textWindow = { revision: s.revision, start: s.window.start, text: s.window.text };
    return { ignored: false, updated, dropped, invalidated: s.invalidated.length };
  }

  #checkPagesReply(r) {
    if (this.revision === null || r.revision !== this.revision) return true;
    if (!Number.isInteger(r.pageCount) || r.pageCount < 0 || r.pageCount > RICH_LIMITS.maxPages || !Array.isArray(r.pages)) invalid("Pages reply is malformed.");
    this.#checkPages(r.pages, r.pageCount);
    return false;
  }

  #applyPages(r) {
    const { updated, dropped } = this.#store(r.pages, r.pageCount, []);
    return { ignored: false, updated, dropped, invalidated: 0 };
  }

  #checkWindow(r) {
    if (this.revision === null || r.revision !== this.revision) return true;
    if (typeof r.start !== "bigint" || typeof r.text !== "string") invalid("Text window reply is malformed.");
    return false;
  }

  #applyWindow(r) {
    this.textWindow = { revision: r.revision, start: r.start, text: r.text };
    return { ignored: false, updated: [], dropped: 0, invalidated: 0 };
  }
}
