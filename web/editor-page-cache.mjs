// SPDX-License-Identifier: AGPL-3.0-or-later
// Client state of one ATED v2 rich-editor document (US-203, R4). Applies the
// replies decoded by web/rich-editor-protocol.mjs following the client rules of
// docs/us-203-rich-editor-protocol.md: invalidations first, then pages, then the
// page count, selection with affinity, flags and text window. Display lists
// arrive as views into the received frame and are copied here, so the cache
// never retains a reply frame. Application is atomic: a reply that fails the
// checks leaves the state untouched.
import { RICH_LIMITS } from "./rich-editor-protocol.mjs";

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
  /** @param {{onChange?: (change: object) => void}} [options] */
  constructor({ onChange } = {}) {
    if (onChange !== undefined && typeof onChange !== "function") throw new TypeError("onChange must be a function.");
    this.onChange = onChange;
    this.pageMap = new Map();
    this.version = 0;
    this.reset();
  }

  /** Forget everything (document opened, closed or replaced). */
  reset() {
    this.pageMap.clear();
    this.revision = null;
    this.pageCount = 0;
    this.docLen = 0n;
    this.selection = { anchor: 0n, focus: 0n, upstream: false };
    this.caret = null;
    this.modified = false;
    this.canUndo = false;
    this.canRedo = false;
    this.layoutComplete = false;
    this.textWindow = null;
    this.version += 1;
  }

  /** Drop every cached page but keep the rest (used when invalidations may have been lost). */
  dropPages() {
    this.pageMap.clear();
    this.version += 1;
  }

  get size() { return this.pageMap.size; }

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
    switch (response?.type) {
      case "state": change = this.#applyState(response); break;
      case "pages": change = this.#applyPages(response); break;
      case "textWindow": change = this.#applyWindow(response); break;
      case "hitResult": case "closed": case "error": change = { ignored: true, updated: [], dropped: 0, invalidated: 0 }; break;
      default: return invalid("Reply type is not a cache input.");
    }
    if (!change.ignored) {
      this.version += 1;
      if (this.onChange) {
        try { this.onChange(change); } catch { /* isolate user callbacks */ }
      }
    }
    return change;
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

  #applyState(s) {
    if (typeof s.revision !== "bigint" || typeof s.anchor !== "bigint" || typeof s.focus !== "bigint" || typeof s.docLen !== "bigint") invalid("State positions must be BigInt.");
    if (!Number.isInteger(s.pageCount) || s.pageCount < 0 || s.pageCount > RICH_LIMITS.maxPages) invalid("State page count is out of range.");
    if (!Array.isArray(s.invalidated) || !Array.isArray(s.pages)) invalid("State lists are missing.");
    for (const range of s.invalidated) {
      if (!Array.isArray(range) || !Number.isInteger(range[0]) || !Number.isInteger(range[1]) || range[0] < 0 || range[0] >= range[1]) invalid("Invalidation range is malformed.");
    }
    this.#checkPages(s.pages, s.pageCount);
    const { updated, dropped } = this.#store(s.pages, s.pageCount, s.invalidated);
    this.revision = s.revision;
    this.docLen = s.docLen;
    this.selection = { anchor: s.anchor, focus: s.focus, upstream: s.focusUpstream === true };
    this.caret = s.caret ? { ...s.caret } : null;
    this.modified = s.modified === true;
    this.canUndo = s.canUndo === true;
    this.canRedo = s.canRedo === true;
    this.layoutComplete = s.layoutComplete === true;
    if (s.window) this.textWindow = { revision: s.revision, start: s.window.start, text: s.window.text };
    return { ignored: false, updated, dropped, invalidated: s.invalidated.length };
  }

  #applyPages(r) {
    if (this.revision === null || r.revision !== this.revision) return { ignored: true, updated: [], dropped: 0, invalidated: 0 };
    if (!Number.isInteger(r.pageCount) || r.pageCount < 0 || r.pageCount > RICH_LIMITS.maxPages || !Array.isArray(r.pages)) invalid("Pages reply is malformed.");
    this.#checkPages(r.pages, r.pageCount);
    const { updated, dropped } = this.#store(r.pages, r.pageCount, []);
    return { ignored: false, updated, dropped, invalidated: 0 };
  }

  #applyWindow(r) {
    if (this.revision === null || r.revision !== this.revision) return { ignored: true, updated: [], dropped: 0, invalidated: 0 };
    if (typeof r.start !== "bigint" || typeof r.text !== "string") invalid("Text window reply is malformed.");
    this.textWindow = { revision: r.revision, start: r.start, text: r.text };
    return { ignored: false, updated: [], dropped: 0, invalidated: 0 };
  }
}
