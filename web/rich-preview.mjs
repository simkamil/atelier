// SPDX-License-Identifier: AGPL-3.0-or-later
import { CanvasRenderer } from "./canvas-renderer.mjs";
import { decodeDisplayList } from "./display-list.mjs";

const A4_WIDTH = 595.276;
const A4_HEIGHT = 841.89;
const MAX_TEXT_BYTES = 128 * 1024;
const MAX_PAGES = 256;
const MAX_PARAGRAPHS = 1024;
const MAX_GLYPHS = 65_536;
const MAX_DISPLAY_LIST_BYTES = 26 * 1024 * 1024;
const encoder = new TextEncoder();

export class RichPreviewError extends Error {
  constructor(code, message = code) {
    super(message);
    this.name = "RichPreviewError";
    this.code = code;
  }
}

function fail(code, message) { throw new RichPreviewError(code, message); }

function prepareDocument(document) {
  if (!document || typeof document !== "object" || !Array.isArray(document.pages)
      || typeof document.text !== "string") {
    fail("RICH_PREVIEW_INVALID_DOCUMENT", "A rich preview needs text and an array of ATDL pages.");
  }
  const textBytes = encoder.encode(document.text);
  if (textBytes.byteLength > MAX_TEXT_BYTES) fail("RICH_PREVIEW_LIMIT", "Preview text exceeds 128 KiB.");
  const paragraphCount = document.text.length === 0 ? 0 : document.text.split(/\r\n|\r|\n/).length;
  if (paragraphCount > MAX_PARAGRAPHS) fail("RICH_PREVIEW_LIMIT", "Preview contains too many paragraphs.");
  if (document.pages.length === 0 || document.pages.length > MAX_PAGES) {
    fail("RICH_PREVIEW_LIMIT", "Preview page count is outside the supported range.");
  }
  if (!Number.isInteger(document.noticeCount) || document.noticeCount < 0 || document.noticeCount > 0xffffffff) {
    fail("RICH_PREVIEW_INVALID_DOCUMENT", "Preview notice count must be an unsigned 32-bit integer.");
  }

  let totalBytes = 0;
  let totalGlyphs = 0;
  const pages = document.pages.map(page => {
    let decoded;
    if (page instanceof Uint8Array) {
      const bytes = page;
      totalBytes += bytes.byteLength;
      if (totalBytes > MAX_DISPLAY_LIST_BYTES) fail("RICH_PREVIEW_LIMIT", "Preview display lists exceed 26 MiB.");
      try { decoded = decodeDisplayList(bytes); }
      catch (error) { fail("RICH_PREVIEW_INVALID_ATDL", error?.message || "Invalid ATDL page."); }
    } else {
      fail("RICH_PREVIEW_INVALID_ATDL", "Each preview page must be ATDL bytes.");
    }
    const bounds = decoded.page.bounds;
    if (!bounds || !Number.isFinite(bounds.width) || !Number.isFinite(bounds.height)
        || Math.abs(bounds.width - A4_WIDTH) > 0.02 || Math.abs(bounds.height - A4_HEIGHT) > 0.02) {
      fail("RICH_PREVIEW_INVALID_ATDL", "Preview pages must use A4 portrait dimensions.");
    }
    for (const command of decoded.commands) {
      if (command.type === "drawGlyphRun") totalGlyphs += command.glyphs.length;
      if (totalGlyphs > MAX_GLYPHS) fail("RICH_PREVIEW_LIMIT", "Preview contains too many glyphs.");
    }
    return decoded;
  });
  return { pages, text: document.text, noticeCount: document.noticeCount };
}

/** A read-only, independently scrollable ATDL preview. It has no editor/model reference. */
export function createRichPreview({ canvas, scrollport, spacer, textRepresentation,
  onClose = () => {}, onError = () => {} } = {}) {
  if (!canvas?.getContext || !canvas.style || !scrollport?.addEventListener || !scrollport.style
      || !spacer?.style || !textRepresentation || typeof textRepresentation !== "object"
      || typeof textRepresentation.textContent !== "string") {
    throw new TypeError("Canvas, scrollport, spacer and textRepresentation are required.");
  }
  if (typeof onClose !== "function" || typeof onError !== "function") {
    throw new TypeError("Preview callbacks must be functions.");
  }

  const renderer = new CanvasRenderer(canvas, { pageBackground: "#ffffff" });
  let document = null;
  let opened = false;
  let disposed = false;
  let returnFocusTo = null;
  let zoom = 1;

  function reportError(error) {
    try { onError(error); } catch { /* UI callbacks cannot interrupt cleanup. */ }
  }

  function updateViewport() {
    if (disposed) return;
    const viewportWidth = scrollport.clientWidth;
    canvas.style.position = "sticky";
    canvas.style.left = "0px";
    canvas.style.width = `${viewportWidth}px`;
    canvas.style.height = `${Math.max(220, scrollport.clientHeight)}px`;
    const pageWidth = document ? A4_WIDTH * zoom : viewportWidth;
    if (canvas.parentElement?.style) canvas.parentElement.style.minWidth = `${pageWidth}px`;
    spacer.style.width = `${Math.max(viewportWidth, pageWidth)}px`;
    if (document) {
      const totalHeight = document.pages.reduce((height, page) => height + page.page.bounds.height * zoom + renderer.pageGap, 0);
      spacer.style.height = `${Math.max(0, totalHeight - scrollport.clientHeight)}px`;
    }
    renderer.setViewport({
      width: viewportWidth,
      height: scrollport.clientHeight,
      zoom,
      scrollX: scrollport.scrollLeft,
      scrollY: scrollport.scrollTop,
      dpr: globalThis.devicePixelRatio || 1,
    });
  }

  function handleKeyDown(event) {
    if (event.key === "Escape" && opened) {
      event.preventDefault();
      close();
    }
  }
  function handleResize() { updateViewport(); }

  scrollport.addEventListener("scroll", updateViewport, { passive: true });
  scrollport.addEventListener("keydown", handleKeyDown);
  globalThis.addEventListener?.("resize", handleResize);
  if (typeof scrollport.tabIndex === "number" && scrollport.tabIndex < 0) scrollport.tabIndex = 0;

  function setDocument(nextDocument) {
    if (disposed) throw new RichPreviewError("RICH_PREVIEW_CLOSED");
    let prepared;
    try { prepared = prepareDocument(nextDocument); }
    catch (error) { reportError(error); throw error; }

    // All page and metadata checks have passed; publish the new document as one operation.
    document = prepared;
    renderer.setPages(prepared.pages);
    textRepresentation.textContent = prepared.text;
    scrollport.scrollTop = 0;
    scrollport.scrollLeft = 0;
    updateViewport();
    return { pageCount: prepared.pages.length, noticeCount: prepared.noticeCount };
  }

  function open({ returnFocusTo: trigger = globalThis.document?.activeElement ?? null } = {}) {
    if (disposed) throw new RichPreviewError("RICH_PREVIEW_CLOSED");
    if (!document) throw new RichPreviewError("RICH_PREVIEW_NO_DOCUMENT");
    if (opened) return;
    opened = true;
    returnFocusTo = trigger && typeof trigger.focus === "function" ? trigger : null;
    scrollport.hidden = false;
    scrollport.scrollTop = 0;
    updateViewport();
    scrollport.focus?.();
  }

  function close() {
    if (!opened) return;
    opened = false;
    scrollport.hidden = true;
    const target = returnFocusTo;
    returnFocusTo = null;
    target?.focus();
    try { onClose(); } catch { /* The view is already closed and focus restored. */ }
  }

  function dispose() {
    if (disposed) return;
    const wasOpened = opened;
    opened = false;
    disposed = true;
    scrollport.hidden = true;
    scrollport.removeEventListener("scroll", updateViewport);
    scrollport.removeEventListener("keydown", handleKeyDown);
    globalThis.removeEventListener?.("resize", handleResize);
    renderer.destroy();
    const target = returnFocusTo;
    returnFocusTo = null;
    if (wasOpened) {
      target?.focus();
      try { onClose(); } catch { /* Disposal remains complete. */ }
    }
  }

  return Object.freeze({
    setDocument,
    open,
    close,
    dispose,
    setZoom(value) {
      if (disposed) throw new RichPreviewError("RICH_PREVIEW_CLOSED");
      if (!Number.isFinite(value) || value < 0.5 || value > 2) throw new RangeError("zoom is outside the supported range");
      zoom = value;
      updateViewport();
    },
  });
}
