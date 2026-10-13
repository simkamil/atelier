// SPDX-License-Identifier: AGPL-3.0-or-later
import { CanvasRenderer } from "./canvas-renderer.mjs";
import { decodeDisplayList } from "./display-list.mjs";
import { clampZoom, fitZoom as calculateFitZoom, isZoomModalOpen, wheelZoom, zoomScrollPosition } from "./zoom.mjs";
import { visiblePage, pageScrollTop } from "./page-navigation.mjs";

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
  onClose = () => {}, onError = () => {}, onZoom = () => {}, onViewport = () => {} } = {}) {
  if (!canvas?.getContext || !canvas.style || !scrollport?.addEventListener || !scrollport.style
      || !spacer?.style || !textRepresentation || typeof textRepresentation !== "object"
      || typeof textRepresentation.textContent !== "string") {
    throw new TypeError("Canvas, scrollport, spacer and textRepresentation are required.");
  }
  if (![onClose, onError, onZoom, onViewport].every(callback => typeof callback === "function")) {
    throw new TypeError("Preview callbacks must be functions.");
  }

  const renderer = new CanvasRenderer(canvas, { pageBackground: "#ffffff" });
  let document = null;
  let opened = false;
  let disposed = false;
  let returnFocusTo = null;
  let zoom = 1;
  let zoomMode = "custom";

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
    if (canvas.parentElement?.style) {
      canvas.parentElement.style.minWidth = `${pageWidth}px`;
      canvas.parentElement.style.overflowAnchor = "none";
    }
    spacer.style.width = `${Math.max(viewportWidth, pageWidth)}px`;
    spacer.style.overflowAnchor = "none";
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
    try { onViewport(viewportState()); } catch { /* UI callbacks are isolated */ }
  }

  function viewportState() {
    return visiblePage(document?.pages ?? [], { zoom, gap: renderer.pageGap, scrollTop: scrollport.scrollTop, height: scrollport.clientHeight });
  }

  function applyZoom(value, anchor = null, mode = "custom") {
    const next = clampZoom(value);
    if (next === zoom && mode === zoomMode) return;
    const oldZoom = zoom;
    const rect = scrollport.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    const oldClientLeft = scrollport.clientLeft || 0;
    const oldClientTop = scrollport.clientTop || 0;
    const clientX = anchor?.x ?? rect.left + oldClientLeft + scrollport.clientWidth / 2;
    const clientY = anchor?.y ?? rect.top + oldClientTop + scrollport.clientHeight / 2;
    const oldScrollLeft = scrollport.scrollLeft;
    const oldScrollTop = scrollport.scrollTop;
    zoom = next;
    zoomMode = mode;
    updateViewport();
    const newRect = scrollport.getBoundingClientRect?.() ?? rect;
    const newClientLeft = scrollport.clientLeft || 0;
    const newClientTop = scrollport.clientTop || 0;
    const newClientX = anchor?.x ?? newRect.left + newClientLeft + scrollport.clientWidth / 2;
    const newClientY = anchor?.y ?? newRect.top + newClientTop + scrollport.clientHeight / 2;
    const position = zoomScrollPosition({ oldZoom, newZoom: next, scrollLeft: oldScrollLeft,
      scrollTop: oldScrollTop, viewportLeft: rect.left, viewportTop: rect.top, clientX, clientY,
      newClientX, newClientY, oldClientLeft, oldClientTop, newClientLeft, newClientTop,
      pageHeights: document?.pages.map(page => page.page.bounds.height), pageGap: renderer.pageGap });
    scrollport.scrollLeft = position.left;
    scrollport.scrollTop = position.top;
    updateViewport();
    try { onZoom({ zoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
  }

  function refitZoom(mode = zoomMode) {
    if (mode === "custom" || !document) return;
    if (!scrollport.clientWidth || !scrollport.clientHeight) return;
    const next = calculateFitZoom(mode, A4_WIDTH, A4_HEIGHT, scrollport.clientWidth,
      scrollport.clientHeight, renderer.pageGap);
    applyZoom(next, null, mode);
  }

  function handleKeyDown(event) {
    if (event.key === "Escape" && opened) {
      event.preventDefault();
      close();
    }
  }
  function handleResize() { updateViewport(); refitZoom(); }

  function handleWheel(event) {
    wheelZoom(event, { active: opened && !scrollport.hidden && !scrollport.closest?.("[hidden]"),
      modalOpen: isZoomModalOpen(globalThis.document), getZoom: () => zoom,
      setZoom: (value, point) => applyZoom(value, point), scrollport });
  }

  scrollport.addEventListener("scroll", updateViewport, { passive: true });
  scrollport.addEventListener("wheel", handleWheel, { passive: false });
  scrollport.addEventListener("keydown", handleKeyDown);
  globalThis.addEventListener?.("resize", handleResize);
  let resizeFrame = 0;
  const scheduleResize = () => {
    if (resizeFrame || disposed) return;
    resizeFrame = requestAnimationFrame(() => { resizeFrame = 0; handleResize(); });
  };
  const resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleResize);
  resizeObserver?.observe(scrollport);
  if (typeof scrollport.tabIndex === "number" && scrollport.tabIndex < 0) scrollport.tabIndex = 0;

  function setDocument(nextDocument) {
    if (disposed) throw new RichPreviewError("RICH_PREVIEW_CLOSED");
    let prepared;
    try { prepared = prepareDocument(nextDocument); }
    catch (error) { reportError(error); throw error; }

    // All page and metadata checks have passed; publish the new document as one operation.
    document = prepared;
    zoom = 1;
    zoomMode = "custom";
    renderer.setPages(prepared.pages);
    textRepresentation.textContent = prepared.text;
    scrollport.scrollTop = 0;
    scrollport.scrollLeft = 0;
    updateViewport();
    try { onZoom({ zoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
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
    resizeObserver?.disconnect();
    if (resizeFrame) cancelAnimationFrame(resizeFrame);
    scrollport.removeEventListener("wheel", handleWheel);
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
      applyZoom(value);
    },
    fitZoom(mode) {
      if (disposed) throw new RichPreviewError("RICH_PREVIEW_CLOSED");
      if (!document) throw new RichPreviewError("RICH_PREVIEW_NO_DOCUMENT");
      if (!["width", "page", "pages"].includes(mode)) throw new TypeError("invalid zoom fit mode");
      refitZoom(mode);
    },
    getZoom() { return zoom; },
    getZoomMode() { return zoomMode; },
    getViewportState: viewportState,
    goToPage(page) {
      if (disposed || !opened || !document) throw new RichPreviewError("RICH_PREVIEW_NO_DOCUMENT");
      scrollport.scrollTop = pageScrollTop(document.pages, page, zoom, renderer.pageGap);
      updateViewport();
    },
  });
}
