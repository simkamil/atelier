// SPDX-License-Identifier: AGPL-3.0-or-later
// Rich editor (engine v2): the editor of the application since R7c; the plain-text editor (v1) was removed in R8.
// Same names, same result shapes and same callbacks (onState, onError, onBusy, onZoom, onViewport, onImages);
// the table of differences is docs/r7-ui-parity.md. It connects the ATED v2 client (rich-editor-client.mjs),
// the page cache with its glyph atlas (editor-page-cache.mjs), the Canvas renderer, the hidden text input,
// the accessible DOM mirror (document-mirror.mjs) and the native file bridge (US-113 tickets).
// No document text is kept here: positions are flat byte offsets (Number, documents are at most 1 MiB), the
// text around the focus comes with each State (`cache.window`) and the rest is asked with `TextWindow`.
import { attachTextInput } from "./text-input.mjs";
import { attachMouseSelection } from "./mouse-selection.mjs";
import { attachTextDragDrop } from "./text-drag-drop.mjs";
import { frenchSpacingEdit } from "./french-spacing.mjs";
import { decodeDisplayList } from "./display-list.mjs";
import { CanvasRenderer } from "./canvas-renderer.mjs";
import { decodeMessage, encodeMessage, MessageType } from "./wasm-protocol.mjs";
import { RICH_LIMITS, RichProtocolError, PROPERTY_FIELDS } from "./rich-editor-protocol.mjs";
import { createRichEditorClient, EditorPageCache, RichClientError } from "./rich-editor-client.mjs";
import { createFormatClient } from "./rich-editor-format.mjs";
import { createRichImageCache } from "./rich-image-cache.mjs";
import { createDocumentMirror } from "./document-mirror.mjs";
import { attachImagePaste, imageAltText, insertImageFiles } from "./image-insert.mjs";
import { clampZoom, fitZoom as calculateFitZoom, isZoomModalOpen, wheelZoom, zoomScrollPosition } from "./zoom.mjs";
import { visiblePage, pageScrollTop } from "./page-navigation.mjs";

export class EditorError extends Error {
  constructor(code, message = code, options = undefined) {
    super(message, options);
    this.name = "EditorError";
    this.code = code;
  }
}

/** What the v2 editor does not offer (docs/r7-ui-parity.md §7); the interface greys the matching commands. */
export const RICH_CAPABILITIES = Object.freeze({
  search: true, replace: true, statistics: true, textDragDrop: true, preview: false, properties: true,
});

const A4 = Object.freeze({ width: 595.276, height: 841.89 });
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const OPEN_FORMATS = ["txt", "docx", "docm", "dotx"];
const SAVE_FORMATS = ["txt", "docx", "docm", "dotx"];
const ZIP_MAGIC = [0x50, 0x4b, 0x03, 0x04];
/** Most bytes of selected text fetched for the clipboard (the engine bound of one TextWindow). */
const MAX_CLIPBOARD_BYTES = RICH_LIMITS.maxWindowBytes;

function nowMs() { return BigInt(Math.max(0, Math.floor(performance.timeOrigin + performance.now()))); }
const byteLength = text => encoder.encode(text).length;

/** Text the engine accepts: line ends become \n, U+FFFC, other controls and lone surrogates are dropped. */
export function sanitizeEditText(text) {
  const normalised = String(text).replace(/\r\n?/g, "\n");
  let out = "";
  for (const scalar of normalised) {
    const code = scalar.codePointAt(0);
    if (code === 0xfffc || (code < 0x20 && code !== 0x0a && code !== 0x09) || code === 0x7f) continue;
    if (code >= 0xd800 && code <= 0xdfff) continue;
    out += scalar;
  }
  return out;
}

/** Alternative text the engine accepts (at most 512 bytes, no control character, no U+FFFE/U+FFFF). */
function sanitizeAlt(text) {
  let out = "";
  let bytes = 0;
  for (const scalar of String(text).replace(/[\u0000-\u001f\u007f￾￿]/g, " ")) {
    const size = byteLength(scalar);
    if (bytes + size > RICH_LIMITS.maxAltBytes) break;
    out += scalar; bytes += size;
  }
  return out.trim();
}

/** PNG, JPEG, GIF or BMP signature: the formats the engine decodes. */
function hasImageSignature(bytes) {
  const at = (...values) => values.every((value, index) => bytes[index] === value);
  return at(0x89, 0x50, 0x4e, 0x47) || at(0xff, 0xd8, 0xff) || at(0x47, 0x49, 0x46, 0x38) || at(0x42, 0x4d);
}

function sniffFormat(bytes) {
  return ZIP_MAGIC.every((b, i) => bytes[i] === b) ? "docx" : "txt";
}

/** Number of `word/media/*` files of a DOCX package, read from the ZIP central directory (0 when unreadable). */
export function countDocxPictures(bytes) {
  try {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let end = -1;
    for (let at = bytes.length - 22; at >= Math.max(0, bytes.length - 22 - 65535); at -= 1) {
      if (view.getUint32(at, true) === 0x06054b50) { end = at; break; }
    }
    if (end < 0) return 0;
    let count = 0;
    let at = view.getUint32(end + 16, true);
    for (let i = 0, n = view.getUint16(end + 10, true); i < n && at + 46 <= bytes.length && view.getUint32(at, true) === 0x02014b50; i += 1) {
      const nameLength = view.getUint16(at + 28, true);
      const name = String.fromCharCode(...bytes.subarray(at + 46, at + 46 + nameLength));
      if (name.startsWith("word/media/")) count += 1;
      at += 46 + nameLength + view.getUint16(at + 30, true) + view.getUint16(at + 32, true);
    }
    return count;
  } catch { return 0; }
}

/** Maps client and codec failures to the `EditorError` codes the interface already knows. */
export function toEditorError(error, context = "") {
  if (error instanceof EditorError) return error;
  if (error?.name === "AbortError") return error;
  const code = error?.code;
  let mapped = null;
  // The queries of R7b-2 keep the codes of the plain editor, whether the codec or the engine refused them.
  if (code === "RICH_EMPTY_QUERY") mapped = "EDITOR_EMPTY_QUERY";
  else if (code === "RICH_QUERY_TOO_LARGE") mapped = "MESSAGE_TOO_LARGE";
  else if (error instanceof RichProtocolError) mapped = "EDITOR_UNSUPPORTED_INPUT";
  else if (code === "RICH_READ_ONLY") mapped = "EDITOR_READ_ONLY";
  else if (code === "RICH_MOVE_UNSUPPORTED") mapped = "EDITOR_MOVE_UNSUPPORTED";
  else if (code === "RICH_STALE_REVISION") mapped = context === "save" ? "EDITOR_STALE_SAVE" : context === "query" ? "EDITOR_STALE_QUERY" : "EDITOR_STALE_EDIT";
  else if (code === "RICH_STALE_MATCH") mapped = "EDITOR_STALE_MATCH";
  else if (code === "RICH_TOO_MANY_MATCHES") mapped = "EDITOR_TOO_MANY_MATCHES";
  else if (code === "RICH_IMAGE_INVALID") mapped = "EDITOR_IMAGE_UNSUPPORTED";
  else if (code === "RICH_LIMIT" && context === "image") mapped = "EDITOR_IMAGE_LIMIT";
  else if (code === "RICH_FRAME_TOO_LARGE" && context === "image") mapped = "EDITOR_IMAGE_TOO_LARGE";
  else if (code === "CLIENT_CLOSED") mapped = "EDITOR_CLOSED";
  return new EditorError(mapped ?? (typeof code === "string" ? code : "EDITOR_FAILED"), String(error?.message ?? code ?? "failed"), { cause: error });
}

/** Transport of `createRichEditorClient` over the engine client: one v2 frame in an `Editor` message. */
function createTransport(client) {
  let outer = 2n;
  return {
    async request(frame, { signal } = {}) {
      const id = outer++;
      const bytes = await client.request(encodeMessage({ type: MessageType.Editor, requestId: id, payload: frame }), { signal });
      const message = decodeMessage(bytes, "event");
      if (message.requestId !== id) throw new EditorError("EDITOR_RESPONSE_MISMATCH");
      if (message.type === MessageType.Failed) throw new EditorError("EDITOR_ENGINE_FAILED", new TextDecoder().decode(message.payload));
      if (message.type !== MessageType.EditorResult) throw new EditorError("EDITOR_RESPONSE_MISMATCH");
      return message.payload;
    },
  };
}

/**
 * Rich clipboard (US-327, docs/us-327-multidoc-protocol.md 5.5): the system clipboard receives the plain text only; the
 * rich fragment of the last copy stays here, in memory, ONE for the application (every editor of the module shares it,
 * a closed document does not take it away: its bytes are a copy). `pending` is the request in flight (a paste that
 * arrives before the answer waits for it); `token` drops the answer of a copy that a newer copy replaced.
 */
const richClipboard = { fragment: null, pending: null, token: 0 };

/** The fragment held for the next paste, `{ text, bytes, lossy }` or null (tests and diagnostics). */
export function richClipboardFragment() { return richClipboard.fragment; }

/** Forgets the held fragment (tests). */
export function forgetRichClipboard() { richClipboard.token += 1; richClipboard.fragment = null; richClipboard.pending = null; }

/** Catalogue keys of the refusals of an Open that the interface tells apart (the others stay `status-open-error`). */
const OPEN_ERROR_KEYS = Object.freeze({ RICH_TOO_MANY_DOCUMENTS: "tabs.error.too_many", RICH_MEMORY: "tabs.error.memory" });

const MIRROR_ROLES = ["paragraph", "heading", "listItem", "tableCell", "opaque"];
const MIRROR_INLINE_KINDS = ["object", "lineBreak", "pageBreak", "symbol"];

/** The codec names roles and inline kinds; the mirror (docs/us-209-document-mirror.md) takes their numbers. */
export function mirrorWindow({ revision, items }) {
  return { revision, items: items.map(item => ({ ...item, role: MIRROR_ROLES.indexOf(item.role),
    inlines: item.inlines.map(inline => ({ ...inline, kind: MIRROR_INLINE_KINDS.indexOf(inline.kind) })) })) };
}

/** A pale 1 x 1 box drawn, stretched, in the place of a picture whose pixels are on their way. */
function placeholderPicture() {
  const box = globalThis.document.createElement("canvas");
  box.width = 1; box.height = 1;
  const context = box.getContext("2d");
  context.fillStyle = "#e6e6e6";
  context.fillRect(0, 0, 1, 1);
  return box;
}

function placeholderPage(width, height) {
  const bounds = { min: { x: 0, y: 0 }, max: { x: width, y: height }, width, height };
  return { page: { bounds, width, height }, commands: [] };
}

// i18n-ignore-start: valeurs par défaut d’un composant autonome ; l’application transmet le texte du catalogue (confirm-discard, dialog-*)
const DEFAULT_CONFIRM_DISCARD = "Le document contient des modifications. Continuer sans les enregistrer ?";
const DEFAULT_DIALOG_LABELS = Object.freeze({ openTitle: "Ouvrir un document", saveTitle: "Enregistrer le document", docx: "Document Word",
  docm: "Document Word avec macros", dotx: "Modèle Word", txt: "Texte brut", defaultName: "Document" });
// i18n-ignore-end

/** Connects the v2 protocol, atlas renderer, hidden native text input, DOM mirror and file host. */
export function createRichEditor({ client, host, canvas, scrollport, spacer, textRepresentation = null, frenchSpacing = true, inputLabel,
  confirmDiscard = DEFAULT_CONFIRM_DISCARD,
  onState = () => {}, onError = () => {}, onBusy = () => {}, onZoom = () => {}, onViewport = () => {}, onImages = () => {}, imageAlt = () => "",
  t = null, user = "", version = "", onReadOnly = () => {}, screenReaderMode = false, onScreenReaderMode = () => {},
  docId = 1, layoutBudget = 16, mirrorContainer = null, dialogLabels = DEFAULT_DIALOG_LABELS, transport = null } = {}) {
  if (!client || typeof client.request !== "function" || !host || typeof host.notifyDirty !== "function"
      || (typeof host.pickOpen !== "function" && (typeof host.open !== "function" || typeof host.save !== "function"))) {
    throw new TypeError("client and native host are required.");
  }
  if (!canvas?.getBoundingClientRect || !scrollport?.addEventListener || !spacer?.style) throw new TypeError("Canvas, scrollport and spacer are required.");
  if (![onState, onError, onBusy, onZoom, onViewport, onImages, onReadOnly, onScreenReaderMode].every(callback => typeof callback === "function")) {
    throw new TypeError("Editor callbacks must be functions.");
  }
  if (t !== null && typeof t !== "function") throw new TypeError("t must be a function.");
  // US-327: with several documents the editors share ONE transport (one outer request counter, see createEngineTransport).
  if (transport !== null && typeof transport?.request !== "function") throw new TypeError("transport must provide request().");
  if (!Number.isInteger(docId) || docId < 1 || docId > 0xffffffff) throw new TypeError("docId must be an integer from 1 to 2^32-1.");

  // The page cache owns the glyph Path2D objects (one per atlas entry): the renderer keeps none of its own, so an
  // atlas epoch change (new document, resync) can never leave an outline of the previous numbering on screen.
  let resolveGlyph = () => null;
  // R7b-1: the bitmaps of the pictures the pages draw (engine `ImageBitmap`). A picture whose bitmap has not
  // arrived yet is a pale box of its size, replaced when it does.
  const images = createRichImageCache();
  let pendingPicture = null;
  const resolvePicture = id => images.get(id) ?? (pendingPicture ??= placeholderPicture());
  const renderer = new CanvasRenderer(canvas, { pageBackground: "#ffffff", maxGlyphCache: 0, resolveGlyph: (font, glyph) => resolveGlyph(font, glyph),
    resolveImage: resolvePicture });
  const compositionPreview = document.createElement("span");
  compositionPreview.className = "composition-preview";
  compositionPreview.setAttribute("aria-hidden", "true");
  compositionPreview.hidden = true;
  document.body.append(compositionPreview);
  if (textRepresentation) { textRepresentation.textContent = ""; textRepresentation.hidden = true; }

  let spacingEnabled = Boolean(frenchSpacing);
  let state;
  let pageLists = [];
  let publishedKey = null;
  let replacedPending = false;
  let queue = Promise.resolve();
  let disposed = false;
  let busy = false;
  let confirming = false;
  let composition = null;
  let compositionStart = null;
  let compositionCommit = null;
  let caretVisible = true;
  let scrollWanted = false;
  let lastHostDirty;
  let pageZoom = 1;
  let zoomMode = "custom";
  let pendingTasks = 0;
  let extensionMode = false;
  let imageCount = 0;
  let readOnlyKey = null;
  let origin = { format: "txt", name: null };
  let mouseSelection;
  let textDragDrop;
  let mirror = null;
  let mirrorTimer = 0;
  let visibleFlat = null;
  let prefetchKey = "";
  let highlight = []; // selection rectangles of the visible pages, points of the page (R7b-1)
  let lastEpoch = null;
  let visible = true; // false while the document is not the one shown (tabs): no blink, no drawing, no layout of the view
  let savedScroll = null;
  let documentClosed = false;
  let highlights = null; // occurrences of the last search to paint (R7b-2)
  const decoded = new WeakMap();
  const abort = new AbortController();

  function reportError(error) { try { onError(error instanceof Error ? error : toEditorError(error)); } catch { /* UI callbacks are isolated */ } }

  const cache = new EditorPageCache({ onChange: change => { try { onCacheChange(change); } catch (error) { reportError(error); } } });
  resolveGlyph = cache.glyphResolver;
  const rich = createRichEditorClient({ transport: transport ?? createTransport(client), docId, cache, layoutBudget, signal: abort.signal,
    onSelectionRects: ({ rects }) => { highlight = rects; repaintOverlay(); },
    hasImage: id => images.has(id),
    onImageBitmap: bitmap => {
      images.add(bitmap.objectId, bitmap).then(held => { if (held && !disposed) renderer.render(); }, error => reportError(toEditorError(error)));
    },
    onError: error => {
      if (disposed) return;
      // The atlas outgrew the client bound right after a resync: the pages cannot be drawn faithfully, the client stopped.
      // Typing is switched off (explicit state) until a document is opened again; the message tells the user to do so.
      if (error?.code === "CLIENT_ATLAS_FULL") input.element.disabled = true;
      reportError(toEditorError(error));
    } });

  const input = attachTextInput({
    container: document.body, label: inputLabel, screenReaderMode,
    // Not while disposing: closing the page turns the mode off in the input, which must not erase the preference.
    onScreenReaderMode: on => { if (disposed) return; try { onScreenReaderMode(on); } catch { /* UI callbacks are isolated */ } },
    getSelection: () => state ? { anchor: state.anchor, focus: state.focus } : { anchor: 0, focus: 0 },
    getCaret: () => caretScreen(),
    getClipboardSnapshot: () => clipboardSnapshot(),
    onCommand: command => { void applyInput(command).catch(error => reportError(toEditorError(error))); },
    onComposition: preview => handleComposition(preview),
    onUnhandled: () => { /* Unsupported browser-native edits stay outside the model. */ },
    onError: reportError,
  });

  // Subscribers of `api.subscribe` (ribbon formatting, shared controller): told of each change that can alter what the
  // formatting commands show or allow. Reasons: "state" (revision, selection, read-only), "document" (opened or reopened),
  // "resync" (new atlas epoch), "busy" (suspended or resumed). Isolated from one another, like the UI callbacks.
  const changeListeners = new Set();
  function notifyChange(reason) {
    for (const listener of [...changeListeners]) { try { listener({ reason }); } catch (error) { reportError(error); } }
  }
  let formatClient = null;

  function setBusy(value) {
    if (busy === value) return;
    busy = value;
    try { onBusy(value); } catch { /* UI callbacks are isolated */ }
    notifyChange("busy");
    input.element.disabled = value;
    if (value) { mouseSelection?.cancel(); textDragDrop?.cancel(); }
  }

  // ---- pages and geometry -------------------------------------------------------------------------------

  function decodedPage(bytes) {
    let page = decoded.get(bytes);
    if (!page) { page = decodeDisplayList(bytes); decoded.set(bytes, page); }
    return page;
  }

  /** Identities of the pictures a decoded page draws (memoised per page). */
  const drawn = new WeakMap();
  function pictureIds(list) {
    let ids = drawn.get(list);
    if (!ids) {
      ids = new Set();
      for (const command of list.commands ?? []) if (command.type === "drawImage") ids.add(command.imageId);
      drawn.set(list, ids);
    }
    return ids;
  }

  function syncPages() {
    const count = cache.pageCount;
    let width = A4.width; let height = A4.height;
    const lists = new Array(count);
    for (let i = 0; i < count; i += 1) {
      const bytes = cache.page(i);
      if (!bytes) continue;
      try { lists[i] = decodedPage(bytes); } catch (error) { reportError(toEditorError(error)); }
    }
    const first = lists.find(Boolean);
    if (first) { width = first.page.bounds.width; height = first.page.bounds.height; }
    const blank = placeholderPage(width, height);
    for (let i = 0; i < count; i += 1) lists[i] ??= blank;
    pageLists = lists;
    const wanted = new Set();
    for (const list of lists) for (const id of pictureIds(list)) wanted.add(id);
    images.retain(wanted);
    try { rich.setWantedImages(wanted); } catch (error) { reportError(toEditorError(error)); }
    renderer.setPages(pageLists);
    updatePageLayout();
  }

  function pageTop(pageIndex) {
    let top = -renderer.scrollY;
    for (let i = 0; i < pageIndex; i += 1) top += pageLists[i].page.bounds.height * renderer.zoom + renderer.pageGap;
    return top;
  }

  function viewportRect(rect, page) {
    const bounds = pageLists[page]?.page.bounds;
    if (!bounds) throw new EditorError("EDITOR_INVALID_STATE");
    return {
      x: -renderer.scrollX + (rect.x - bounds.min.x) * renderer.zoom,
      y: pageTop(page) + (rect.y - bounds.min.y) * renderer.zoom,
      width: rect.width * renderer.zoom,
      height: rect.height * renderer.zoom,
    };
  }

  function caretViewport() {
    if (!state?.caret || !pageLists[state.caret.page]) return null;
    const rect = viewportRect({ x: state.caret.x, y: state.caret.y, width: 1, height: state.caret.height }, state.caret.page);
    return { x: rect.x, y: rect.y, width: 1, height: rect.height };
  }

  function caretScreen() {
    const caret = caretViewport() ?? { x: 0, y: 0, width: 1, height: 14 };
    const bounds = canvas.getBoundingClientRect();
    return {
      ...caret,
      x: Math.max(0, Math.min(Math.max(0, globalThis.innerWidth - 1), bounds.left + caret.x)),
      y: Math.max(0, Math.min(Math.max(0, globalThis.innerHeight - caret.height), bounds.top + caret.y)),
    };
  }

  function repaintOverlay() {
    if (!state || disposed || !visible) return;
    const caret = caretViewport();
    const compositionRects = composition ? [{ ...composition.caret, width: composition.width, height: 1 }] : [];
    const selectionRects = [];
    for (const rect of highlight) if (pageLists[rect.page]) selectionRects.push(viewportRect(rect, rect.page));
    renderer.setEditorOverlay({ selectionRects, caret, compositionRects, caretVisible, ...searchOverlay() });
    input.updateCaret(caretScreen());
  }

  function updatePageLayout() {
    if (disposed || !visible) return;
    const width = pageLists.reduce((max, page) => Math.max(max, page.page.bounds.width * pageZoom), 0);
    const viewportWidth = scrollport.clientWidth;
    const contentWidth = Math.max(width, viewportWidth);
    spacer.style.width = `${contentWidth}px`;
    if (canvas.parentElement?.style) {
      canvas.parentElement.style.minWidth = `${contentWidth}px`;
      canvas.parentElement.style.overflowAnchor = "none";
    }
    spacer.style.overflowAnchor = "none";
    const totalHeight = pageLists.reduce((height, page) => height + page.page.bounds.height * pageZoom + renderer.pageGap, 0);
    spacer.style.height = `${Math.max(0, totalHeight - scrollport.clientHeight)}px`;
  }

  function viewportState() {
    return visiblePage(pageLists, { zoom: pageZoom, gap: renderer.pageGap, scrollTop: scrollport.scrollTop, height: scrollport.clientHeight });
  }

  /** First and end (exclusive) index of the pages that intersect the viewport. */
  function visibleRange() {
    const top = scrollport.scrollTop;
    const bottom = top + scrollport.clientHeight;
    let y = 0;
    let first = -1;
    let end = 0;
    for (let i = 0; i < pageLists.length; i += 1) {
      const height = pageLists[i].page.bounds.height * pageZoom;
      if (y + height >= top && y <= bottom) { if (first < 0) first = i; end = i + 1; }
      y += height + renderer.pageGap;
      if (y > bottom) break;
    }
    return first < 0 ? { first: 0, end: Math.min(1, pageLists.length) } : { first, end };
  }

  function updateViewport() {
    if (disposed || !visible) return;
    const viewportWidth = scrollport.clientWidth;
    canvas.style.position = "sticky";
    canvas.style.left = "0px";
    canvas.style.width = `${viewportWidth}px`;
    canvas.style.height = `${Math.max(220, scrollport.clientHeight)}px`;
    const pageWidth = pageLists.reduce((width, page) => Math.max(width, page.page.bounds.width * pageZoom), 0);
    const contentWidth = Math.max(pageWidth, viewportWidth);
    if (canvas.parentElement?.style) canvas.parentElement.style.minWidth = `${contentWidth}px`;
    spacer.style.width = `${contentWidth}px`;
    renderer.setViewport({
      width: canvas.clientWidth, height: scrollport.clientHeight, zoom: pageZoom,
      scrollX: scrollport.scrollLeft, scrollY: scrollport.scrollTop, dpr: globalThis.devicePixelRatio || 1,
    });
    repaintOverlay();
    if (opened()) {
      const range = visibleRange();
      try { rich.setVisibleRange(range.first, range.end); } catch (error) { reportError(toEditorError(error)); }
      scheduleMirrorVisible();
      scheduleHighlights();
    }
    try { onViewport(viewportState()); } catch { /* UI callbacks are isolated */ }
  }

  let viewportFrame = 0;
  function scheduleViewport() {
    if (viewportFrame || disposed) return;
    viewportFrame = requestAnimationFrame(() => { viewportFrame = 0; updateViewport(); });
  }

  const opened = () => rich.opened;

  // ---- state publication --------------------------------------------------------------------------------

  function buildState() {
    const sel = cache.selection;
    const readOnly = cache.readOnly;
    return {
      kind: "state", revision: cache.revision, anchor: Number(sel.anchor), focus: Number(sel.focus), upstream: sel.upstream,
      docLen: Number(cache.docLen), dirty: cache.modified, canUndo: cache.canUndo && !readOnly, canRedo: cache.canRedo && !readOnly,
      readOnly, hasRevisions: cache.hasRevisions, pageCount: cache.pageCount, layoutComplete: cache.layoutComplete,
      caret: cache.caret ? { ...cache.caret } : null,
    };
  }

  function stateKey(s) {
    const c = s.caret;
    return [s.revision, s.anchor, s.focus, s.upstream, s.docLen, s.dirty, s.canUndo, s.canRedo, s.readOnly, s.hasRevisions,
      c ? `${c.page},${c.x},${c.y},${c.height}` : "-"].join("|");
  }

  function onCacheChange() {
    if (disposed) return;
    if (cache.revision === null) return; // a reset, nothing to show yet
    const resynced = lastEpoch !== null && cache.atlasEpoch !== lastEpoch;
    if (cache.atlasEpoch !== lastEpoch) { lastEpoch = cache.atlasEpoch; images.clear(); }
    refreshHighlightsOnChange();
    const candidate = buildState();
    const previous = state;
    const key = stateKey(candidate);
    const documentReplaced = replacedPending;
    // The state object stays the same while nothing it carries changed (a page fetch, a zoom): consumers compare identity.
    const unchanged = Boolean(previous) && !documentReplaced && key === publishedKey
      && previous.pageCount === candidate.pageCount && previous.layoutComplete === candidate.layoutComplete;
    const next = unchanged ? previous : candidate;
    state = next;
    syncPages();
    const selectionMoved = !previous || previous.revision !== next.revision || previous.anchor !== next.anchor || previous.focus !== next.focus
      || (next.caret !== null && previous.caret === null);
    if (documentReplaced || key !== publishedKey) {
      publishedKey = key;
      replacedPending = false;
      if (selectionMoved && (next.caret || !next.layoutComplete)) scrollWanted = true;
      updateViewport();
      if (documentReplaced || previous?.readOnly !== next.readOnly || previous?.hasRevisions !== next.hasRevisions) announceReadOnly(next);
      try { onState(next, { documentReplaced }); } catch { /* UI callbacks are isolated */ }
      notifyChange(documentReplaced ? "document" : resynced ? "resync" : "state");
      if (lastHostDirty !== next.dirty) {
        lastHostDirty = next.dirty;
        void host.notifyDirty(next.dirty).then(result => {
          if (result?.kind === "error") reportError(new EditorError(result.key));
        }).catch(reportError);
      }
      updateMirror(next);
      prefetchSelection(next);
    } else {
      updateViewport();
      if (resynced) notifyChange("resync");
    }
    // The caret of a far jump arrives after its State and possibly before its page: scrolling waits for both, and the
    // spacer must cover the page before scrollTop is set.
    if (scrollWanted && !next.caret && next.layoutComplete) scrollWanted = false;
    if (visible && scrollWanted && next.caret && caretViewport()) { scrollWanted = false; updateViewport(); scrollCaretIntoView(); }
  }

  function announceReadOnly(next) {
    const info = `${next.readOnly}|${next.hasRevisions}`;
    if (info === readOnlyKey) return;
    readOnlyKey = info;
    try { onReadOnly({ readOnly: next.readOnly, hasRevisions: next.hasRevisions }); } catch { /* UI callbacks are isolated */ }
  }

  function scrollCaretIntoView() {
    const caret = caretViewport();
    if (!caret) return;
    const pad = 24;
    const contentY = caret.y + scrollport.scrollTop;
    if (contentY < scrollport.scrollTop + pad) scrollport.scrollTop = Math.max(0, contentY - pad);
    else if (contentY + caret.height > scrollport.scrollTop + scrollport.clientHeight - pad) {
      scrollport.scrollTop = Math.max(0, contentY + caret.height - scrollport.clientHeight + pad);
    }
  }

  // ---- accessible mirror ----------------------------------------------------------------------------------

  if (t) {
    mirror = createDocumentMirror({
      container: mirrorContainer ?? canvas.parentElement ?? document.body,
      getAccessibleWindow: async (start, end, { signal } = {}) => mirrorWindow(await rich.accessibleWindow(start, end, { signal })),
      t, input, onError: reportError,
    });
    if (input.screenReaderMode()) mirror.setScreenReaderMode(true);
  }

  function updateMirror(next) {
    if (!mirror || next.revision === null) return;
    const update = { revision: next.revision, docLen: BigInt(next.docLen), selection: { anchor: BigInt(next.anchor), focus: BigInt(next.focus) } };
    if (visibleFlat && visibleFlat.revision === next.revision) update.visible = { start: visibleFlat.start, end: visibleFlat.end };
    Promise.resolve(mirror.update(update)).catch(reportError);
  }

  /** Flat positions of the top and bottom of the viewport (two probes), so the mirror follows the scroll. */
  function scheduleMirrorVisible() {
    if (!mirror || mirrorTimer || disposed) return;
    mirrorTimer = setTimeout(() => { mirrorTimer = 0; void refreshVisibleFlat(); }, 150);
  }

  async function refreshVisibleFlat() {
    if (disposed || !state || !opened()) return;
    const range = visibleRange();
    const revision = state.revision;
    const probe = async (page, y) => {
      const bounds = pageLists[page]?.page.bounds;
      if (!bounds || !cache.has(page)) return null;
      try {
        const { response } = await rich.command({ type: "hitTest", page, mode: "probe", x: bounds.min.x + bounds.width / 2, y: bounds.min.y + y });
        return response.position;
      } catch { return null; }
    };
    const lastPage = Math.max(range.first, range.end - 1);
    const a = await probe(range.first, 4);
    const b = await probe(lastPage, pageLists[lastPage]?.page.bounds.height - 4 || 0);
    if (disposed || a === null || b === null || !state || state.revision !== revision) return;
    visibleFlat = { revision, start: a < b ? a : b, end: a < b ? b : a };
    updateMirror(state);
  }

  // ---- clipboard text -------------------------------------------------------------------------------------

  /** Text of [lo, hi) from the window of the current State, or null when it is not held. */
  function textFromWindow(lo, hi) {
    const window = cache.window;
    if (!window) return null;
    const bytes = encoder.encode(window.text);
    const start = Number(window.start);
    if (lo < start || hi > start + bytes.length) return null;
    try { return decoder.decode(bytes.subarray(lo - start, hi - start)); } catch { return null; }
  }

  function selectionBounds(s) { return { lo: Math.min(s.anchor, s.focus), hi: Math.max(s.anchor, s.focus) }; }

  /** Asks the engine for the selected text when the State window does not hold it (copy is synchronous). */
  function prefetchSelection(next) {
    if (next.anchor === next.focus) return;
    const { lo, hi } = selectionBounds(next);
    if (hi - lo > MAX_CLIPBOARD_BYTES || textFromWindow(lo, hi) !== null) return;
    const key = `${next.revision}|${lo}|${hi}`;
    if (key === prefetchKey) return;
    prefetchKey = key;
    void rich.command({ type: "textWindow", start: BigInt(lo), len: hi - lo }).catch(() => { prefetchKey = ""; });
  }

  let lastClipboard = null;
  function clipboardSnapshot() {
    if (!state || disposed || busy || pendingTasks || compositionStart || state.anchor === state.focus) return null;
    const { lo, hi } = selectionBounds(state);
    if (hi - lo > MAX_CLIPBOARD_BYTES) { reportError(new EditorError("EDITOR_SELECTION_TOO_LARGE")); return null; }
    const text = textFromWindow(lo, hi);
    if (text === null) { prefetchKey = ""; prefetchSelection(state); reportError(new EditorError("EDITOR_CLIPBOARD_UNAVAILABLE")); return null; }
    lastClipboard = { revision: state.revision, lo, hi };
    requestFragment(lo, hi, state.revision);
    return { text, selection: { anchor: 0, focus: byteLength(text) }, revision: state.revision };
  }

  /**
   * The copy is synchronous (the text is written at once); the rich fragment of the same range is asked for right after
   * and kept for the next paste. A refusal (too large, empty, stale) leaves text only, and forgets an older fragment.
   */
  function requestFragment(lo, hi, revision) {
    const token = ++richClipboard.token;
    richClipboard.fragment = null;
    let request;
    try { request = rich.copyFragment(BigInt(lo), BigInt(hi), { revision: BigInt(revision) }); } catch { richClipboard.pending = null; return; }
    const mine = request.then(held => {
      if (token === richClipboard.token) richClipboard.fragment = { text: sanitizeEditText(held.text), bytes: held.bytes, lossy: held.lossy };
    }, () => { /* text only */ }).finally(() => { if (richClipboard.pending === mine) richClipboard.pending = null; });
    richClipboard.pending = mine;
  }

  /**
   * Paste rule (docs 5.5): when the text of the clipboard equals the text of the held fragment, the rich fragment is
   * pasted (one undo step); otherwise `plain(text)` inserts the text. A fragment the engine refuses as invalid or too
   * large falls back to the text; any other refusal (read-only, memory) is the user's answer. Runs inside a task.
   */
  async function pasteOrInsert(raw, plain) {
    const wanted = sanitizeEditText(raw);
    if (richClipboard.pending) { try { await richClipboard.pending; } catch { /* text only */ } }
    const held = richClipboard.fragment;
    if (held && wanted.length > 0 && held.text === wanted && state && !state.readOnly) {
      extensionMode = false;
      try { return await rich.pasteFragment(held.bytes); } catch (error) {
        const mapped = toEditorError(error);
        if (mapped.code !== "RICH_FRAGMENT_INVALID" && mapped.code !== "RICH_LIMIT") throw mapped;
      }
    }
    return wanted.length === 0 ? state : plain(wanted);
  }

  /** Text of the selection of `snapshot` (an `editor.getState()` taken earlier): window first, else `TextWindow`. */
  async function selectedText(snapshot) {
    checkActionSnapshot(snapshot);
    const { lo, hi } = selectionBounds(snapshot);
    if (hi === lo) return "";
    if (hi - lo > MAX_CLIPBOARD_BYTES) throw new EditorError("EDITOR_SELECTION_TOO_LARGE");
    const held = textFromWindow(lo, hi);
    if (held !== null) return held;
    return readText(lo, hi);
  }

  async function readText(start = 0, end = state?.docLen ?? 0) {
    if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start) throw new TypeError("Invalid text range.");
    let out = "";
    let at = start;
    while (at < end) {
      const { response } = await rich.command({ type: "textWindow", start: BigInt(at), len: Math.min(RICH_LIMITS.maxWindowBytes, end - at) });
      if (response.text.length === 0) break;
      out += response.text;
      at += byteLength(response.text);
    }
    return out;
  }

  // ---- zoom ---------------------------------------------------------------------------------------------

  function applyZoom(value, anchor = null, mode = "custom") {
    const next = clampZoom(value);
    if (next === pageZoom && mode === zoomMode) return;
    const oldZoom = pageZoom;
    const rect = scrollport.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    const oldClientLeft = scrollport.clientLeft || 0;
    const oldClientTop = scrollport.clientTop || 0;
    const clientX = anchor?.x ?? rect.left + oldClientLeft + scrollport.clientWidth / 2;
    const clientY = anchor?.y ?? rect.top + oldClientTop + scrollport.clientHeight / 2;
    const oldScrollLeft = scrollport.scrollLeft;
    const oldScrollTop = scrollport.scrollTop;
    pageZoom = next;
    zoomMode = mode;
    updatePageLayout();
    updateViewport();
    const newRect = scrollport.getBoundingClientRect?.() ?? rect;
    const newClientLeft = scrollport.clientLeft || 0;
    const newClientTop = scrollport.clientTop || 0;
    const newClientX = anchor?.x ?? newRect.left + newClientLeft + scrollport.clientWidth / 2;
    const newClientY = anchor?.y ?? newRect.top + newClientTop + scrollport.clientHeight / 2;
    const position = zoomScrollPosition({ oldZoom, newZoom: next,
      scrollLeft: oldScrollLeft, scrollTop: oldScrollTop, viewportLeft: rect.left, viewportTop: rect.top,
      clientX, clientY, newClientX, newClientY, oldClientLeft, oldClientTop, newClientLeft, newClientTop,
      pageHeights: pageLists.map(page => page.page.bounds.height), pageGap: renderer.pageGap });
    scrollport.scrollLeft = position.left;
    scrollport.scrollTop = position.top;
    updateViewport();
    repaintOverlay();
    try { onZoom({ zoom: pageZoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
  }

  function refitZoom(mode = zoomMode) {
    if (mode === "custom" || !pageLists.length) return;
    if (!scrollport.clientWidth || !scrollport.clientHeight) return;
    const bounds = pageLists[0].page.bounds;
    applyZoom(calculateFitZoom(mode, bounds.width, bounds.height, scrollport.clientWidth, scrollport.clientHeight, renderer.pageGap), null, mode);
  }

  function resetZoom() {
    pageZoom = 1;
    zoomMode = "custom";
    try { onZoom({ zoom: pageZoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
  }

  // ---- commands -----------------------------------------------------------------------------------------

  function enqueueTask(run) {
    pendingTasks += 1;
    const result = queue.then(run, run).finally(() => { pendingTasks -= 1; });
    queue = result.catch(() => {});
    return result;
  }

  async function send(command, context = "") {
    if (disposed) throw new EditorError("EDITOR_CLOSED");
    try { return await rich.command(command); } catch (error) { throw toEditorError(error, context); }
  }

  function currentState() {
    if (!state) throw new EditorError("EDITOR_INVALID_STATE");
    return state;
  }

  function checkActionSnapshot(snapshot) {
    if (disposed || busy || !state || compositionStart) throw new EditorError(disposed ? "EDITOR_CLOSED" : "EDITOR_BUSY");
    if (!snapshot || snapshot.revision !== state.revision || snapshot.anchor !== state.anchor || snapshot.focus !== state.focus) {
      throw new EditorError("EDITOR_STALE_SELECTION");
    }
  }

  const setSelection = (anchor, focus, upstream = false) => send({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream });

  async function insertWithSpacing(text, inputType) {
    const collapsed = state.anchor === state.focus;
    const raw = await send({ type: "insertText", text });
    if (!collapsed || !spacingEnabled) return raw;
    const window = cache.window;
    if (!window || window.revision !== cache.revision) return raw;
    const startBytes = Number(window.start);
    const focus = state.focus - startBytes;
    if (focus < 0 || focus > byteLength(window.text)) return raw;
    const edit = frenchSpacingEdit({ text: window.text, anchor: focus, focus }, text, inputType, spacingEnabled);
    if (!edit) return raw;
    return send({ type: "replaceRange", start: BigInt(edit.start + startBytes), end: BigInt(edit.end + startBytes), text: sanitizeEditText(edit.text) });
  }

  async function applyInput(command) {
    if (!state || busy) return undefined;
    if (command.type === "extensionMode") { extensionMode = command.enabled; return undefined; }
    if (command.type === "selectAll") {
      extensionMode = false;
      return enqueueTask(() => setSelection(0, currentState().docLen));
    }
    if (command.type === "deleteSelection") {
      return enqueueTask(async () => {
        const { lo, hi } = selectionBounds(currentState());
        if (command.revision !== state.revision || !lastClipboard || lastClipboard.revision !== state.revision
            || lastClipboard.lo !== lo || lastClipboard.hi !== hi || lo === hi) throw new EditorError("EDITOR_STALE_SELECTION");
        return send({ type: "delete", forward: false, unit: "grapheme" });
      });
    }
    if (command.type === "insertText") {
      const committed = command.inputType === "insertCompositionText" ? compositionCommit : null;
      if (command.inputType === "insertCompositionText") compositionCommit = null;
      const text = sanitizeEditText(command.text ?? "");
      if (command.inputType === "insertFromPaste") return enqueueTask(() => pasteOrInsert(command.text ?? "", wanted => insertWithSpacing(wanted, command.inputType)));
      return enqueueTask(async () => {
        if (committed && state?.revision === committed.revision
            && committed.selection && Number.isInteger(committed.selection.anchor) && Number.isInteger(committed.selection.focus)) {
          await setSelection(committed.selection.anchor, committed.selection.focus);
        }
        if (text.length === 0) return state;
        return insertWithSpacing(text, command.inputType);
      });
    }
    if (command.type === "delete") {
      if (!["content", "word"].includes(command.unit)) throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
      return enqueueTask(() => send({ type: "delete", forward: command.direction !== "backward", unit: command.unit === "word" ? "word" : "grapheme" }));
    }
    if (command.type === "undo") return enqueueTask(() => send({ type: "undo" }));
    if (command.type === "redo") return enqueueTask(() => send({ type: "redo" }));
    if (command.type === "move") {
      let direction = command.direction;
      if (command.toDocumentEdge) direction = direction === "home" ? "documentStart" : "documentEnd";
      if (command.byWord && !command.toDocumentEdge) {
        direction = { left: "wordLeft", right: "wordRight", up: "paragraphUp", down: "paragraphDown" }[direction];
        if (!direction) throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
      }
      return enqueueTask(() => send({ type: "move", direction, extend: Boolean(command.extend || extensionMode) }));
    }
    throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
  }

  function handleComposition(preview) {
    if (preview.phase === "cancel" || preview.phase === "commit") {
      if (preview.phase === "commit") compositionCommit = compositionStart;
      composition = null;
      compositionStart = null;
      compositionPreview.hidden = true;
      compositionPreview.textContent = "";
      repaintOverlay();
      return;
    }
    if (preview.phase === "start") {
      compositionStart = { selection: preview.selection, revision: state?.revision };
      mouseSelection?.cancel();
      textDragDrop?.cancel();
    }
    const caret = preview.caret;
    const bounds = canvas.getBoundingClientRect();
    composition = { caret: { x: caret.x - bounds.left, y: caret.y - bounds.top, height: caret.height }, width: [...preview.text].length * 7 };
    compositionPreview.textContent = preview.text;
    const screenCaret = caretScreen();
    compositionPreview.style.left = `${screenCaret.x}px`;
    compositionPreview.style.top = `${screenCaret.y}px`;
    compositionPreview.hidden = preview.text.length === 0;
    repaintOverlay();
  }

  // ---- mouse, drop, images --------------------------------------------------------------------------------

  const getGeometry = () => ({
    pages: pageLists.map(({ page }) => ({ minX: page.bounds.min.x, minY: page.bounds.min.y, width: page.bounds.width, height: page.bounds.height })),
    zoom: pageZoom, pageGap: renderer.pageGap,
  });

  mouseSelection = attachMouseSelection({
    canvas, scrollport, getGeometry,
    getSelection: () => ({ anchor: state?.anchor ?? 0, focus: state?.focus ?? 0 }),
    isEnabled: () => Boolean(state && !busy && !compositionStart && !disposed),
    selectPoint: values => enqueueTask(async () => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      extensionMode = false;
      try {
        if (values.unit === "word" || values.unit === "paragraph") {
          // Double and triple click: the engine knows the words and paragraphs; the position comes from a probe.
          const { response } = await rich.command({ type: "hitTest", page: values.page, mode: "probe", x: values.x, y: values.y });
          await rich.command({ type: "selectUnit", position: response.position, unit: values.unit, extend: values.extend });
        } else {
          await rich.command({ type: "hitTest", page: values.page, mode: values.extend ? "extend" : "setCaret", x: values.x, y: values.y });
        }
      } catch (error) {
        // A page that is not laid out yet cannot be hit: keep the selection, never an error for a pointer move.
        if (error?.code !== "RICH_NOT_READY" && error?.code !== "RICH_PAGE_RANGE") throw toEditorError(error);
      }
      return { anchor: state.anchor, focus: state.focus };
    }),
    focus: () => input.focus(),
    onError: error => reportError(toEditorError(error)),
  });

  async function insertImages(files, { revision, target } = {}) {
    const result = await insertImageFiles(files, {
      insertBytes: (bytes, file) => enqueueTask(async () => {
        if (busy || compositionStart || disposed) throw new EditorError(disposed ? "EDITOR_CLOSED" : "EDITOR_BUSY");
        if (target !== undefined) {
          if (revision !== state.revision) throw new EditorError("EDITOR_STALE_EDIT");
          await setSelection(target, target);
        }
        const alt = sanitizeAlt(imageAltText(file, imageAlt(imageCount + 1)));
        let reply;
        try { reply = await send({ type: "insertImage", alt, bytes }, "image"); } catch (error) {
          // The engine answers one code for every undecodable image: a known signature means a damaged file, not another format.
          if (error?.code === "EDITOR_IMAGE_UNSUPPORTED" && hasImageSignature(bytes)) throw new EditorError("EDITOR_IMAGE_CORRUPT", error.message, { cause: error });
          throw error;
        }
        imageCount += 1;
        return reply;
      }),
      onError: error => reportError(toEditorError(error, "image")),
    });
    try { onImages(result); } catch { /* UI callbacks are isolated */ }
    return result;
  }

  const imagePaste = attachImagePaste({ target: input.element, onFiles: files => { void insertImages(files); },
    isEnabled: () => Boolean(state && !busy && !compositionStart && !disposed && !state.readOnly) });

  // Text drag and drop (R7c): the module of v1 over the v2 commands. The snapshot carries no text (`text: null`: the engine checks
  // the boundaries, `RICH_POSITION_NOT_BOUNDARY`); a probe gives a position, no caret (no drop marker). Dropped pictures go
  // through `insertImages` as before; text dropped from outside is inserted at the position; an internal drag is `MoveText`.
  const dragReady = () => Boolean(state && !busy && !compositionStart && !disposed && !state.readOnly);
  textDragDrop = attachTextDragDrop({
    allowNoCaret: true,
    canvas, scrollport, getGeometry,
    getSelectionSnapshot: () => state ? { text: null, anchor: state.anchor, focus: state.focus, revision: state.revision } : null,
    isPointInSelection: (x, y) => {
      if (!state) return false;
      const bounds = canvas.getBoundingClientRect();
      return highlight.some(rect => {
        if (!pageLists[rect.page]) return false;
        const r = viewportRect(rect, rect.page);
        return x >= bounds.left + r.x && x <= bounds.left + r.x + r.width && y >= bounds.top + r.y && y <= bounds.top + r.y + r.height;
      });
    },
    isEnabled: dragReady,
    probePoint: point => enqueueTask(async () => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      const { response } = await rich.command({ type: "hitTest", page: point.page, mode: "probe", x: point.x, y: point.y });
      return { offset: Number(response.position), revision: response.revision, caret: null };
    }),
    onDrop: drop => enqueueTask(async () => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      if (state.readOnly) throw new EditorError("EDITOR_READ_ONLY");
      if (drop.revision !== state.revision) throw new EditorError("EDITOR_STALE_EDIT");
      if (drop.kind === "external") {
        return send({ type: "replaceRange", start: BigInt(drop.target), end: BigInt(drop.target), text: sanitizeEditText(drop.text) });
      }
      const start = Math.min(drop.sourceAnchor, drop.sourceFocus);
      const end = Math.max(drop.sourceAnchor, drop.sourceFocus);
      return send({ type: "moveText", start: BigInt(start), end: BigInt(end), target: BigInt(drop.target), copy: drop.kind === "copy" });
    }),
    onFilesDrop: drop => insertImages(drop.files.filter(file => typeof file.type === "string" && file.type.startsWith("image/")), { revision: drop.revision, target: drop.target }),
    onClick: point => enqueueTask(async () => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      if (point.revision !== state.revision) throw new EditorError("EDITOR_STALE_EDIT");
      return setSelection(point.offset, point.offset);
    }),
    onDragState: () => {},
    focus: () => input.focus(),
    onError: error => reportError(toEditorError(error)),
  });

  scrollport.addEventListener("scroll", updateViewport, { passive: true });
  const onWheel = event => wheelZoom(event, { active: Boolean(state && !busy && !disposed && !compositionStart
      && !scrollport.hidden && !scrollport.closest?.("[hidden]")),
    modalOpen: isZoomModalOpen(globalThis.document), getZoom: () => pageZoom,
    setZoom: (value, point) => applyZoom(value, point), scrollport });
  scrollport.addEventListener("wheel", onWheel, { passive: false });
  // The user takes the scroll over (wheel, touch, scrollbar or scrolling keys): a caret still to come no longer scrolls the view.
  const onUserScroll = () => { scrollWanted = false; };
  const USER_SCROLL = ["wheel", "touchmove", "pointerdown", "keydown"];
  for (const type of USER_SCROLL) scrollport.addEventListener(type, onUserScroll, { passive: true });
  globalThis.addEventListener?.("resize", scheduleViewport);
  let fitResizeFrame = 0;
  const scheduleRefit = () => {
    if (fitResizeFrame || disposed || !visible) return;
    fitResizeFrame = requestAnimationFrame(() => { fitResizeFrame = 0; refitZoom(); });
  };
  const fitResizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleRefit);
  fitResizeObserver?.observe(scrollport);
  globalThis.addEventListener?.("resize", scheduleRefit);
  const blink = setInterval(() => { if (!visible) return; caretVisible = !caretVisible; repaintOverlay(); }, 530);

  // ---- documents ------------------------------------------------------------------------------------------

  /**
   * Opens `data` as `format` ("blank" | "txt" | "docx"): the previous document survives a refused Open. A blank document is
   * a rich document with the default profile of US-202 (A4, Carlito 11 pt, default styles; `FormatState.plain` is false) and
   * is saved as DOCX first; only a TXT file opened as such stays plain text.
   */
  async function loadDocument(format, data = new Uint8Array(), name = null) {
    const spec = format === "blank" ? { kind: "blank", profile: "document", data: new Uint8Array() }
      : format === "txt" ? { kind: "txt", profile: "txt", data } : { kind: "docx", profile: "document", data };
    replacedPending = true;
    scrollWanted = false;
    publishedKey = null;
    images.clear();
    highlight = [];
    try {
      await rich.open(spec);
    } catch (error) {
      replacedPending = false;
      throw toEditorError(error);
    }
    imageCount = 0;
    input.element.disabled = busy;
    visibleFlat = null;
    prefetchKey = "";
    lastClipboard = null;
    origin = { format: format === "blank" ? "docx" : format, name };
    resetZoom();
    if (!state) throw new EditorError("EDITOR_INVALID_STATE");
    return state;
  }

  // ---- search, replace, statistics, properties (R7b-2) ------------------------------------------------------

  // Highlights of the occurrences of the last search: the rectangles of the visible pages come from the engine
  // (`searchRects`, page coordinates), as they are needed. { result, index, revision, ranges, pages: Map<page, rects[]>,
  // fetching, layoutDone }. Declared with the other state at the top of the closure.

  function searchOverlay() {
    if (!highlights || !state || highlights.revision !== state.revision) return {};
    const searchRects = []; const activeSearchRects = [];
    const range = visibleRange();
    for (let page = range.first; page < range.end; page += 1) {
      for (const { item, rect } of highlights.pages.get(page) ?? []) {
        const mapped = viewportRect(rect, page);
        if (mapped.y + mapped.height >= 0 && mapped.y <= scrollport.clientHeight && mapped.x + mapped.width >= 0 && mapped.x <= scrollport.clientWidth) {
          (item === highlights.index ? activeSearchRects : searchRects).push(mapped);
        }
      }
    }
    return { searchRects, activeSearchRects };
  }

  function scheduleHighlights() {
    const current = highlights;
    if (!current || current.fetching || disposed || !state || current.revision !== state.revision || !pageLists.length) return;
    const range = visibleRange();
    let first = -1; let last = -1;
    for (let page = range.first; page < range.end; page += 1) {
      if (current.pages.has(page)) continue;
      if (first < 0) first = page;
      last = page;
    }
    if (first < 0) return;
    const pageCount = Math.min(RICH_LIMITS.maxPagesPerResponse, last - first + 1);
    current.fetching = true;
    rich.command({ type: "searchRects", revision: BigInt(current.revision), firstPage: first, pageCount, ranges: current.ranges })
      .then(({ response }) => {
        if (highlights !== current || disposed) return;
        for (let page = first; page < first + pageCount; page += 1) current.pages.set(page, []);
        for (const rect of response.rects) {
          current.pages.get(rect.page)?.push({ item: rect.item, rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height } });
        }
        current.fetching = false;
        repaintOverlay();
        scheduleHighlights();
      })
      .catch(error => {
        if (highlights !== current || disposed) return;
        current.fetching = false;
        // A stale revision is followed by a State that clears the highlights; any other refusal gives up on these pages.
        if (error?.code !== "RICH_STALE_REVISION" && error?.name !== "AbortError") {
          for (let page = first; page < first + pageCount; page += 1) current.pages.set(page, []);
        }
      });
  }

  /** The layout finished since the rectangles were asked for: pages that were not reached then are asked for again. */
  function refreshHighlightsOnChange() {
    if (!highlights) return;
    const done = cache.layoutComplete === true;
    if (done !== highlights.layoutDone) {
      highlights.layoutDone = done;
      if (done) highlights.pages.clear();
    }
    if (cache.revision !== null && highlights.revision !== cache.revision) highlights = null;
  }

  function checkQueryState() {
    if (disposed) throw new EditorError("EDITOR_CLOSED");
    if (!state || busy) throw new EditorError("EDITOR_BUSY");
  }

  // The queries (search, statistics, properties) change nothing: they go straight to the client, whose queue keeps their order
  // with the edits, and they do not count as pending editor tasks (the synchronous clipboard waits for those).
  async function search(queryText, options = {}) {
    if (typeof queryText !== "string") throw new TypeError("Search query must be a string.");
    checkQueryState();
    const { response } = await send({ type: "search", revision: BigInt(state.revision), flags: options ?? {}, query: queryText }, "query");
    return {
      kind: "search", revision: response.revision, total: response.total, totalCapped: response.totalCapped, truncated: response.truncated,
      items: response.items.map(item => ({ start: Number(item.start), end: Number(item.end), excerpt: item.excerpt })),
    };
  }

  async function statistics() {
    checkQueryState();
    const { response } = await send({ type: "statistics" }, "query");
    return {
      kind: "statistics", revision: response.revision, anchor: Number(response.anchor), focus: Number(response.focus),
      layoutComplete: response.layoutComplete, document: response.document, selection: response.selection,
    };
  }

  function replace(all, values, signal) {
    return enqueueTask(async () => {
      if (signal?.aborted || !state || values?.revision !== state.revision) throw new EditorError("EDITOR_STALE_QUERY");
      if (busy || disposed || compositionStart) throw new EditorError(disposed ? "EDITOR_CLOSED" : "EDITOR_BUSY");
      extensionMode = false;
      const common = { revision: state.revision, flags: values.options ?? {}, query: values.query, replacement: sanitizeEditText(values.replacement) };
      const command = all ? { type: "replaceAll", ...common } : { type: "replace", ...common, start: BigInt(values.start), end: BigInt(values.end) };
      const { response } = await send(command, "query");
      return { kind: "replacement", count: response.count, state };
    });
  }

  function setHighlights(result, index = -1) {
    if (disposed) return false;
    if (result === null) {
      highlights = null;
      repaintOverlay();
      return true;
    }
    if (!state || result?.kind !== "search" || result.revision !== state.revision) return false;
    if (!Array.isArray(result.items) || result.items.length > RICH_LIMITS.maxSearchItems || !Number.isInteger(index) || index < -1 || index >= result.items.length) {
      throw new EditorError("EDITOR_INVALID_RESPONSE");
    }
    for (const item of result.items) {
      if (!Number.isInteger(item.start) || !Number.isInteger(item.end) || item.start < 0 || item.end <= item.start || item.end > state.docLen) {
        throw new EditorError("EDITOR_INVALID_RESPONSE");
      }
    }
    if (highlights?.result === result) {
      // Same occurrences, another current one: nothing to ask the engine again.
      highlights.index = index;
    } else {
      highlights = { result, index, revision: state.revision, pages: new Map(), fetching: false, layoutDone: cache.layoutComplete === true,
        ranges: result.items.map(item => ({ start: BigInt(item.start), end: BigInt(item.end) })) };
    }
    repaintOverlay();
    scheduleHighlights();
    return true;
  }

  function selectSearchResult(result, index, { signal } = {}) {
    return enqueueTask(async () => {
      if (signal?.aborted || disposed || busy || !state || compositionStart || result?.revision !== state.revision) throw new EditorError("EDITOR_STALE_QUERY");
      const item = result.items?.[index];
      if (!item) throw new EditorError("EDITOR_INVALID_RESPONSE");
      await setSelection(item.start, item.end);
      if (result.revision === state.revision) setHighlights(result, index);
      return state;
    });
  }

  async function readProperties() {
    checkQueryState();
    const { response } = await send({ type: "propertiesRead" }, "properties");
    // A document that is not a DOCX keeps its properties only in a DOCX file: the dialog says so.
    return { kind: "snapshot", ...response.snapshot, docxOnly: !["docx", "docm", "dotx"].includes(origin.format) };
  }

  /** `changes` maps property names (PROPERTY_FIELDS) to their new text; an empty text removes the property. */
  function applyProperties(changes) {
    if (changes === null || typeof changes !== "object") return Promise.reject(new TypeError("changes must be an object."));
    for (const name of Object.keys(changes)) if (!PROPERTY_FIELDS.includes(name)) return Promise.reject(new TypeError(`unknown property ${name}.`));
    return enqueueTask(async () => {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      if (!state || busy || compositionStart) throw new EditorError("EDITOR_BUSY");
      const { response } = await send({ type: "propertiesApply", changes }, "properties");
      if (response.status === "tooLong") return { kind: "tooLong", field: response.field, max: response.max, length: response.length };
      return { kind: "applied", changed: response.changed, state };
    });
  }

  const dialogFormats = (formats, labels) => formats.map(format => ({ format, label: labels[format] }));

  /** Native open dialog and ticket read (US-113), or the bytes-only `host.open()` of simple hosts. */
  async function pickAndRead(formats = OPEN_FORMATS) {
    if (typeof host.pickOpen !== "function") return host.open();
    const picked = await host.pickOpen({ title: dialogLabels.openTitle, filters: dialogFormats(formats, dialogLabels) });
    if (picked.kind !== "ok") return picked;
    if (picked.size > RICH_LIMITS.maxOpenBytes) { void host.releaseTicket?.(picked.ticket); return { kind: "error", key: "files.error.too_large" }; }
    const read = await host.readTicket(picked.ticket, { copy: true });
    if (read.kind !== "ok") return read;
    return { kind: "ok", bytes: read.bytes, format: picked.format, name: picked.name, identity: picked.identity ?? null };
  }

  function baseName(name) {
    const stem = typeof name === "string" ? name.replace(/\.[^./\\]*$/, "") : "";
    return stem && !/[\\/:*?"<>|]/.test(stem) && stem.length <= 100 && !/[\u0000-\u001f]/.test(stem) && !stem.endsWith(".") ? stem : dialogLabels.defaultName;
  }

  async function saveToBytes(format) {
    const stamp = { nowMs: nowMs(), user, version };
    const { response } = await send({ type: "save", format, ...stamp }, "save");
    if (response.type !== "saveData") throw new EditorError("EDITOR_INVALID_EXPORT");
    return { revision: response.revision, bytes: response.bytes.slice(), notices: response.notices };
  }

  function defaultSaveFormat() { return SAVE_FORMATS.includes(origin.format) ? origin.format : "txt"; }

  const api = {
    get state() { return state; },
    get busy() { return busy; },
    get element() { return input.element; },
    get engine() { return "v2"; },
    /** Géométrie des pages en points et zoom courant (surcouches du ruban, US-243). */
    getGeometry,
    capabilities: RICH_CAPABILITIES,
    get input() { return input; },
    get mirror() { return mirror; },
    get readOnlyInfo() { return { readOnly: state?.readOnly ?? false, hasRevisions: state?.hasRevisions ?? false }; },
    /** Pictures the cached pages draw: `{page, id (BigInt), bounds}` in points of the page (R7b-1; for the interface tests). */
    pictureBoxes() {
      const out = [];
      pageLists.forEach((list, page) => { for (const command of list.commands ?? []) if (command.type === "drawImage") out.push({ page, id: command.imageId, bounds: command.bounds }); });
      return out;
    },
    /** Selection rectangles of the visible pages, points of the page (R7b-1). */
    get selectionRects() { return highlight.map(rect => ({ ...rect })); },
    /** Pictures whose bitmap is held (R7b-1). */
    get pictureBitmaps() { return images.size; },
    focus() { return input.focus(); },
    /** Typed formatting client (`createFormatClient`, rich-editor-format.mjs) on the editor's own queue: one per editor. */
    get format() { return formatClient ??= createFormatClient(rich); },
    /** `listener({ reason })` after each change that can alter formatting state or availability; returns the unsubscribe. */
    subscribe(listener) {
      if (typeof listener !== "function") throw new TypeError("A listener function is required.");
      changeListeners.add(listener);
      return () => { changeListeners.delete(listener); };
    },
    setFrenchSpacing(enabled) { spacingEnabled = Boolean(enabled); },
    /** Name of the file the document came from (or was saved as), null for a new document (tab title). */
    get documentName() { return origin.name; },
    get documentFormat() { return origin.format; },
    /** Native dialog and ticket read without opening anything: `{ kind: "ok", bytes, format, name, identity }` (the application decides where the file opens). */
    readFromDialog(formats = OPEN_FORMATS) { return pickAndRead(formats); },
    /**
     * The whole text of the document as the engine last showed it, or null when this is not certain (a window of
     * the text only, an object or a symbol in it, or a window older than the last state): the application can then
     * rebuild the document from it after the engine was lost; null means the tab must be shown as lost instead.
     */
    completeText() {
      if (state && state.docLen === 0) return "";
      const held = cache.window;
      if (!state || !held || Number(held.start) !== 0 || held.text.includes("\ufffc") || byteLength(held.text) !== state.docLen) return null;
      return held.text;
    },
    /** The identifier of the document in the engine. */
    get docId() { return docId; },
    get visible() { return visible; },
    /**
     * Whether this document is the one shown (tabs, US-327). Hidden: the caret does not blink, nothing is drawn, the
     * resize and scroll of the view are ignored, the highlights are cleared and the focus is released; the state, the
     * layout pump and the accessible mirror keep working (a hidden document is still laid out, fairly, docs 3). Call
     * `setVisible(false)` BEFORE hiding the DOM (the scroll position is saved) and `setVisible(true)` AFTER showing it
     * (the view is measured again, the scroll position and the zoom come back).
     */
    setVisible(value) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      const next = Boolean(value);
      if (next === visible) return;
      if (!next) {
        savedScroll = { left: scrollport.scrollLeft, top: scrollport.scrollTop };
        visible = false;
        mouseSelection?.cancel(); textDragDrop?.cancel();
        clearTimeout(mirrorTimer); mirrorTimer = 0;
        if (viewportFrame) { cancelAnimationFrame(viewportFrame); viewportFrame = 0; }
        if (fitResizeFrame) { cancelAnimationFrame(fitResizeFrame); fitResizeFrame = 0; }
        renderer.setEditorOverlay({ selectionRects: [], caret: null, compositionRects: [], caretVisible: false });
        if (globalThis.document?.activeElement === input.element) input.element.blur();
        return;
      }
      visible = true;
      caretVisible = true;
      updatePageLayout();
      if (savedScroll) { scrollport.scrollLeft = savedScroll.left; scrollport.scrollTop = savedScroll.top; savedScroll = null; }
      updateViewport();
      refitZoom();
      renderer.render();
      if (scrollWanted && state?.caret && caretViewport()) { scrollWanted = false; scrollCaretIntoView(); }
    },
    /** Copies the rich fragment of the selection of `snapshot` after the text went to the clipboard (context menu). */
    rememberFragment(snapshot) {
      checkActionSnapshot(snapshot);
      const { lo, hi } = selectionBounds(snapshot);
      if (hi > lo && hi - lo <= MAX_CLIPBOARD_BYTES) requestFragment(lo, hi, snapshot.revision);
    },
    /** Paste of the context menu: the rich fragment when `text` is its text (docs 5.5), else the text in the selection. */
    pasteText(text, snapshot) {
      if (typeof text !== "string") return Promise.reject(new TypeError("Pasted text must be a string."));
      return enqueueTask(() => {
        checkActionSnapshot(snapshot);
        extensionMode = false;
        return pasteOrInsert(text, wanted => {
          const { lo, hi } = selectionBounds(state);
          return send({ type: "replaceRange", start: BigInt(lo), end: BigInt(hi), text: wanted });
        });
      });
    },
    /**
     * Closes the document in the engine (`Close`: model, history, pages and atlas are freed and the weight goes back to
     * the budget) and drops what the editor held for it. It also resolves when the engine no longer knows the document
     * (`RICH_UNKNOWN_DOCUMENT`). Closing a tab: confirmation and save first, then `closeDocument()`, then `dispose()`.
     */
    async closeDocument({ signal } = {}) {
      if (disposed || documentClosed) return;
      documentClosed = true;
      try { await rich.closeDocument({ signal }); } catch (error) {
        if (error?.code !== "RICH_UNKNOWN_DOCUMENT" && error?.code !== "CLIENT_CLOSED") { documentClosed = false; throw toEditorError(error); }
      }
      images.clear(); highlight = []; highlights = null; visibleFlat = null; lastClipboard = null; prefetchKey = "";
      if (!disposed) repaintOverlay();
    },
    get documentClosed() { return documentClosed; },
    /** The last text window the engine gave (at most 256 KiB around the focus): what stays readable when the engine is lost. */
    get windowText() { return cache.textWindow?.text ?? ""; },
    /** Resolves true when the first page is drawn-ready (or the layout is complete), false after `timeoutMs`: launch of several files opens them one after the other. */
    async whenFirstScreen(timeoutMs = 2000) {
      const end = performance.now() + timeoutMs;
      while (!disposed && performance.now() < end) {
        if (state && (cache.layoutComplete || cache.has(0))) return true;
        await new Promise(resolve => setTimeout(resolve, 16));
      }
      return false;
    },
    setSuspended(value) { if (disposed) throw new EditorError("EDITOR_CLOSED"); setBusy(Boolean(value)); },
    setScreenReaderMode(enabled) { input.setScreenReaderMode(Boolean(enabled)); },
    setZoom(zoom) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      applyZoom(zoom);
    },
    fitZoom(mode) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      if (!pageLists.length) throw new EditorError("EDITOR_INVALID_STATE");
      if (!["width", "page", "pages"].includes(mode)) throw new TypeError("invalid zoom fit mode");
      refitZoom(mode);
    },
    getZoom() { return pageZoom; },
    getZoomMode() { return zoomMode; },
    getViewportState: viewportState,
    goToPage(page) {
      if (disposed || busy || !state || compositionStart) throw new EditorError("EDITOR_BUSY");
      scrollport.scrollTop = pageScrollTop(pageLists, page, pageZoom, renderer.pageGap);
      updateViewport();
    },
    async initialize() { return enqueueTask(() => loadDocument("blank")); },
    getState() { return state; },
    get canEdit() { return Boolean(state && !busy && !disposed && !compositionStart && !state.readOnly); },
    editSelection(text, snapshot) {
      if (typeof text !== "string") return Promise.reject(new TypeError("Inserted text must be a string."));
      return enqueueTask(() => {
        checkActionSnapshot(snapshot);
        extensionMode = false;
        const { lo, hi } = selectionBounds(state);
        return send({ type: "replaceRange", start: BigInt(lo), end: BigInt(hi), text: sanitizeEditText(text) });
      });
    },
    selectionAction(action, snapshot) {
      return enqueueTask(() => {
        checkActionSnapshot(snapshot);
        extensionMode = false;
        if (action === "selectAll") return setSelection(0, state.docLen);
        if (action === "undo") return send({ type: "undo" });
        if (action === "redo") return send({ type: "redo" });
        throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
      });
    },
    selectedText,
    readText,
    search,
    statistics,
    replaceCurrent(values, { signal } = {}) { return replace(false, values, signal); },
    replaceAll(values, { signal } = {}) { return replace(true, values, signal); },
    setSearchHighlights: setHighlights,
    selectSearchResult,
    readProperties,
    applyProperties,
    insertImages,
    async exportDocx() {
      const saved = await enqueueTask(() => saveToBytes("docx"));
      return { bytes: saved.bytes, noticeCount: saved.notices.length, pictureCount: countDocxPictures(saved.bytes) };
    },
    async exportDocument() {
      const saved = await enqueueTask(() => saveToBytes("txt"));
      return { revision: saved.revision, bytes: saved.bytes, notices: saved.notices };
    },
    async newDocument() {
      if (busy || !await this.confirmReplace()) return { kind: "cancelled" };
      setBusy(true);
      try { return await enqueueTask(() => loadDocument("blank")); }
      finally { setBusy(false); input.focus(); }
    },
    /** `source` returns the bytes (`{kind:"ok", bytes, format?, name?}`); the native dialog by default. */
    async openDocument(source = undefined, { formats = OPEN_FORMATS } = {}) {
      if (busy || !await this.confirmReplace()) return { kind: "cancelled" };
      setBusy(true);
      try {
        const result = await (source ?? (() => pickAndRead(formats)))();
        if (result.kind !== "ok") return result;
        const format = OPEN_FORMATS.includes(result.format) ? result.format : result.format === undefined ? sniffFormat(result.bytes) : null;
        if (format === null) return { kind: "error", key: "files.error.unsupported_format" };
        if (result.bytes.length > RICH_LIMITS.maxOpenBytes) return { kind: "error", key: "files.error.too_large" };
        try {
          const opened = await enqueueTask(() => loadDocument(format === "txt" ? "txt" : "docx", result.bytes, result.name ?? null));
          return { kind: "ok", state: opened, identity: result.identity ?? null };
        } catch (error) {
          if (error?.code === "EDITOR_CLOSED") throw error;
          return { kind: "error", key: OPEN_ERROR_KEYS[error?.code] ?? "status-open-error", code: error?.code };
        }
      } finally { setBusy(false); input.focus(); }
    },
    async saveAs() {
      if (busy) return { kind: "error", key: "files.error.queue_full" };
      setBusy(true);
      let ticket = null;
      try {
        let format = defaultSaveFormat();
        let savedName = null;
        if (typeof host.pickSave === "function") {
          const picked = await host.pickSave({ title: dialogLabels.saveTitle, filters: dialogFormats([format, ...SAVE_FORMATS.filter(f => f !== format)], dialogLabels),
            suggestedName: `${baseName(origin.name)}.${format}` });
          if (picked.kind !== "ok") return picked;
          ticket = picked.ticket;
          if (!SAVE_FORMATS.includes(picked.format)) return { kind: "error", key: "files.error.unsupported_format" };
          format = picked.format;
          savedName = typeof picked.name === "string" && picked.name !== "" ? picked.name : null;
        }
        const saved = await enqueueTask(() => saveToBytes(format));
        const written = ticket === null ? await host.save(saved.bytes) : await host.writeTicket(ticket, saved.bytes);
        ticket = null; // consumed by the host, whatever the result
        if (written.kind !== "ok") return written;
        const marked = await enqueueTask(() => send({ type: "markSaved", revision: saved.revision }, "save"));
        if (marked.change?.ignored) throw new EditorError("EDITOR_STALE_SAVE");
        origin = { format, name: savedName ?? origin.name };
        notifyChange("document-name");
        return { kind: "ok", state, notices: saved.notices };
      } catch (error) {
        throw toEditorError(error, "save");
      } finally {
        if (ticket !== null) void Promise.resolve(host.releaseTicket?.(ticket)).catch(() => {});
        setBusy(false);
        input.focus();
      }
    },
    /** Read-only DOCX (revisions, comments): a copy with every change accepted and the comments removed, opened as editable. */
    async makeEditableCopy() {
      if (busy) return { kind: "error", key: "files.error.queue_full" };
      if (!state?.readOnly) return { kind: "error", key: "status-editor-error" };
      setBusy(true);
      try {
        const copy = await enqueueTask(async () => {
          const { response } = await send({ type: "makeEditableCopy", nowMs: nowMs(), user, version });
          return { bytes: response.bytes.slice(), notices: response.notices };
        });
        const opened = await enqueueTask(() => loadDocument("docx", copy.bytes, origin.name));
        return { kind: "ok", state: opened, notices: copy.notices };
      } catch (error) {
        throw toEditorError(error);
      } finally { setBusy(false); input.focus(); }
    },
    undo() { return busy ? Promise.reject(new EditorError("EDITOR_BUSY")) : enqueueTask(() => send({ type: "undo" })); },
    redo() { return busy ? Promise.reject(new EditorError("EDITOR_BUSY")) : enqueueTask(() => send({ type: "redo" })); },
    async confirmReplace() {
      if (!state?.dirty) return true;
      if (busy || confirming) return false;
      confirming = true;
      input.focus();
      setBusy(true);
      try { return globalThis.confirm(confirmDiscard); }
      finally { confirming = false; setBusy(false); input.focus(); }
    },
    /** Resolves when the layout of the whole document is done (tests, statistics). */
    whenLayoutComplete() { return rich.whenLayoutComplete(); },
    dispose() {
      if (disposed) return;
      disposed = true;
      highlights = null;
      clearInterval(blink);
      clearTimeout(mirrorTimer);
      if (viewportFrame) cancelAnimationFrame(viewportFrame);
      scrollport.removeEventListener("scroll", updateViewport);
      scrollport.removeEventListener("wheel", onWheel);
      for (const type of USER_SCROLL) scrollport.removeEventListener(type, onUserScroll);
      scrollWanted = false;
      textDragDrop.dispose();
      mouseSelection.dispose();
      imagePaste.dispose();
      changeListeners.clear();
      // The session is closed in the engine unless it already is (best effort, bounded): a tab that goes away must give
      // its weight back to the budget. The client goes after the Close (or two seconds later).
      const finish = () => { abort.abort(); rich.dispose(); };
      if (rich.opened && !documentClosed) {
        documentClosed = true;
        rich.stopPump();
        let timer = 0;
        const bound = new Promise(resolve => { timer = setTimeout(resolve, 2000); });
        Promise.race([rich.closeDocument().catch(() => {}), bound]).then(() => { clearTimeout(timer); finish(); });
      } else finish();
      mirror?.dispose();
      globalThis.removeEventListener?.("resize", scheduleViewport);
      fitResizeObserver?.disconnect();
      globalThis.removeEventListener?.("resize", scheduleRefit);
      if (fitResizeFrame) cancelAnimationFrame(fitResizeFrame);
      input.dispose(); renderer.destroy();
      compositionPreview.remove();
    },
  };
  return Object.freeze(api);
}

export { RichClientError };
