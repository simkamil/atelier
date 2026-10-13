// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTextInput } from "./text-input.mjs";
import { attachMouseSelection } from "./mouse-selection.mjs";
import { attachTextDragDrop } from "./text-drag-drop.mjs";
import { frenchSpacingEdit } from "./french-spacing.mjs";
import { decodeDisplayList } from "./display-list.mjs";
import { CanvasRenderer } from "./canvas-renderer.mjs";
import { decodeMessage, encodeMessage, MessageType } from "./wasm-protocol.mjs";
import { decodeEditorResponse, EditorOpcode, encodeEditorCommand } from "./editor-protocol.mjs";
import { clampZoom, fitZoom as calculateFitZoom, isZoomModalOpen, wheelZoom, zoomScrollPosition } from "./zoom.mjs";
import { visiblePage, pageScrollTop } from "./page-navigation.mjs";

export class EditorError extends Error {
  constructor(code, message = code) { super(message); this.name = "EditorError"; this.code = code; }
}

function nowMs() { return BigInt(Math.max(0, Math.floor(performance.timeOrigin + performance.now()))); }

function pageTop(renderer, pageIndex) {
  let top = -renderer.scrollY;
  for (let i = 0; i < pageIndex; i += 1) top += renderer.pages[i].page.bounds.height * renderer.zoom + renderer.pageGap;
  return top;
}

/** Connects the editor protocol, ATDL renderer, hidden native text input and file host. */
export function createEditor({ client, host, canvas, scrollport, spacer, textRepresentation = null, frenchSpacing = true, confirmDiscard = "Le document contient des modifications. Continuer sans les enregistrer ?", onState = () => {}, onError = () => {}, onBusy = () => {}, onZoom = () => {}, onViewport = () => {} } = {}) {
  if (!client || typeof client.request !== "function" || !host || typeof host.open !== "function"
      || typeof host.save !== "function" || typeof host.notifyDirty !== "function") throw new TypeError("client and native host are required.");
  if (!canvas?.getBoundingClientRect || !scrollport?.addEventListener || !spacer?.style) throw new TypeError("Canvas, scrollport and spacer are required.");
  if (textRepresentation !== null && (typeof textRepresentation !== "object" || typeof textRepresentation.textContent !== "string")) {
    throw new TypeError("textRepresentation must be a text-capable DOM element.");
  }
  if (![onState, onError, onBusy, onZoom, onViewport].every(callback => typeof callback === "function")) throw new TypeError("Editor callbacks must be functions.");

  const renderer = new CanvasRenderer(canvas, { pageBackground: "#ffffff" });
  const compositionPreview = document.createElement("span");
  compositionPreview.className = "composition-preview";
  compositionPreview.setAttribute("aria-hidden", "true");
  compositionPreview.hidden = true;
  document.body.append(compositionPreview);
  const dropMarker = document.createElement("span");
  dropMarker.className = "text-drop-marker";
  dropMarker.setAttribute("aria-hidden", "true");
  dropMarker.hidden = true;
  document.body.append(dropMarker);
  let dropCaret = null;
  let spacingEnabled = Boolean(frenchSpacing);
  let state;
  let decodedPages = [];
  let searchHighlights = null;
  let searchIndex = -1;
  let queue = Promise.resolve();
  let requestId = 2n;
  let disposed = false;
  let busy = false;
  let confirming = false;
  let composition = null;
  let compositionStart = null;
  let compositionCommit = null;
  let caretVisible = true;
  let lastHostDirty;
  let pageZoom = 1;
  let zoomMode = "custom";
  let pendingTasks = 0;
  let extensionMode = false;
  let mouseSelection;
  let textDragDrop;
  const input = attachTextInput({
    container: document.body,
    getSelection: () => state ? { anchor: state.anchor, focus: state.focus } : { anchor: 0, focus: 0 },
    getCaret: () => caretScreen(),
    getClipboardSnapshot: () => {
      if (!state || disposed || busy || pendingTasks || compositionStart || state.anchor === state.focus) return null;
      return { text: state.text,
        selection: { anchor: state.anchor, focus: state.focus }, revision: state.revision };
    },
    onCommand: command => { void applyInput(command).catch(reportError); },
    onComposition: preview => handleComposition(preview),
    onUnhandled: () => { /* Unsupported browser-native edits stay outside the model. */ },
    onError: reportError,
  });

  function reportError(error) { try { onError(error); } catch { /* UI callbacks are isolated */ } }

  function setBusy(value) {
    if (busy === value) return;
    busy = value;
    try { onBusy(value); } catch { /* UI callbacks are isolated */ }
    input.element.disabled = value;
    if (value) { mouseSelection?.cancel(); textDragDrop?.cancel(); }
  }

  function viewportRect(rect, page) {
    const bounds = decodedPages[page]?.page.bounds;
    if (!bounds) throw new EditorError("EDITOR_INVALID_STATE");
    const top = pageTop(renderer, page);
    return {
      x: -renderer.scrollX + (rect.x - bounds.min.x) * renderer.zoom,
      y: top + (rect.y - bounds.min.y) * renderer.zoom,
      width: rect.width * renderer.zoom,
      height: rect.height * renderer.zoom,
    };
  }

  function caretViewport() {
    if (!state) return { x: 0, y: 0, width: 1, height: 14 };
    const rect = viewportRect({ x: state.caret.x, y: state.caret.y, width: 1, height: state.caret.height }, state.caret.page);
    return { x: rect.x, y: rect.y, width: 1, height: rect.height };
  }

  function caretScreen() {
    const caret = caretViewport();
    const bounds = canvas.getBoundingClientRect();
    return {
      ...caret,
      x: Math.max(0, Math.min(Math.max(0, globalThis.innerWidth - 1), bounds.left + caret.x)),
      y: Math.max(0, Math.min(Math.max(0, globalThis.innerHeight - caret.height), bounds.top + caret.y)),
    };
  }

  function repaintOverlay() {
    if (!state || disposed) return;
    const selectionRects = [];
    for (const rect of state.selectionRects) selectionRects.push(viewportRect(rect, rect.page));
    const caret = caretViewport();
    const compositionRects = composition ? [{ ...composition.caret, width: composition.width, height: 1 }] : [];
    const searchRects = [], activeSearchRects = [];
    if (searchHighlights?.revision === state.revision) {
      searchHighlights.items.forEach((item, index) => {
        for (const rect of item.rects) {
          const mapped = viewportRect(rect, rect.page);
          if (mapped.y + mapped.height >= 0 && mapped.y <= scrollport.clientHeight
              && mapped.x + mapped.width >= 0 && mapped.x <= scrollport.clientWidth) {
            (index === searchIndex ? activeSearchRects : searchRects).push(mapped);
          }
        }
      });
    }
    renderer.setEditorOverlay({ selectionRects, searchRects, activeSearchRects, caret, compositionRects, caretVisible });
    input.updateCaret(caretScreen());
    dropMarker.hidden = !dropCaret;
    if (dropCaret) {
      const rect = viewportRect({ ...dropCaret, width: 2 }, dropCaret.page);
      const bounds = canvas.getBoundingClientRect();
      dropMarker.style.left = `${bounds.left + rect.x}px`;
      dropMarker.style.top = `${bounds.top + rect.y}px`;
      dropMarker.style.height = `${rect.height}px`;
    }
  }

  function updateViewport() {
    if (disposed) return;
    const viewportWidth = scrollport.clientWidth;
    canvas.style.position = "sticky";
    canvas.style.left = "0px";
    canvas.style.width = `${viewportWidth}px`;
    canvas.style.height = `${Math.max(220, scrollport.clientHeight)}px`;
    const pageWidth = decodedPages.reduce((width, page) => Math.max(width, page.page.bounds.width * pageZoom), 0);
    const contentWidth = Math.max(pageWidth, viewportWidth);
    if (canvas.parentElement?.style) canvas.parentElement.style.minWidth = `${contentWidth}px`;
    spacer.style.width = `${contentWidth}px`;
    renderer.setViewport({
      width: canvas.clientWidth,
      height: scrollport.clientHeight,
      zoom: pageZoom,
      scrollX: scrollport.scrollLeft,
      scrollY: scrollport.scrollTop,
      dpr: globalThis.devicePixelRatio || 1,
    });
    repaintOverlay();
    try { onViewport(viewportState()); } catch { /* UI callbacks are isolated */ }
  }

  function viewportState() {
    return visiblePage(decodedPages, { zoom: pageZoom, gap: renderer.pageGap, scrollTop: scrollport.scrollTop, height: scrollport.clientHeight });
  }

  let viewportFrame = 0;
  function scheduleViewport() {
    if (viewportFrame || disposed) return;
    viewportFrame = requestAnimationFrame(() => {
      viewportFrame = 0;
      updateViewport();
    });
  }

  function publish(nextState, { documentReplaced = false } = {}) {
    if (nextState.kind !== "state") throw new EditorError("EDITOR_INVALID_STATE");
    // Decode every page before publishing state; a malformed page must never blank the prior UI.
    const pages = nextState.pages.map(bytes => decodeDisplayList(bytes));
    if (pages.length === 0 || pages.length > 1024 || pages.some(page =>
      Math.abs(page.page.bounds.width - 595.276) > 0.02 || Math.abs(page.page.bounds.height - 841.89) > 0.02)) {
      throw new EditorError("EDITOR_INVALID_STATE");
    }
    if (documentReplaced || state?.revision !== nextState.revision) { searchHighlights = null; searchIndex = -1; }
    state = nextState;
    decodedPages = pages;
    if (textRepresentation) textRepresentation.textContent = nextState.text;
    renderer.setPages(pages);
    updatePageLayout();
    const caret = viewportRect({ x: nextState.caret.x, y: nextState.caret.y, width: 1, height: nextState.caret.height }, nextState.caret.page);
    const pad = 24;
    const caretContentY = caret.y + scrollport.scrollTop;
    if (caretContentY < scrollport.scrollTop + pad) scrollport.scrollTop = Math.max(0, caretContentY - pad);
    else if (caretContentY + caret.height > scrollport.scrollTop + scrollport.clientHeight - pad) {
      scrollport.scrollTop = Math.max(0, caretContentY + caret.height - scrollport.clientHeight + pad);
    }
    updateViewport();
    try { onState(nextState, { documentReplaced }); } catch { /* UI callbacks are isolated */ }
    if (lastHostDirty !== nextState.dirty) {
      lastHostDirty = nextState.dirty;
      void host.notifyDirty(nextState.dirty).then(result => {
        if (result?.kind === "error") reportError(new EditorError(result.key));
      }).catch(reportError);
    }
    return nextState;
  }

  function updatePageLayout() {
    if (disposed) return;
    const width = decodedPages.reduce((max, page) => Math.max(max, page.page.bounds.width * pageZoom), 0);
    const viewportWidth = scrollport.clientWidth;
    const contentWidth = Math.max(width, viewportWidth);
    spacer.style.width = `${contentWidth}px`;
    if (canvas.parentElement?.style) {
      canvas.parentElement.style.minWidth = `${contentWidth}px`;
      canvas.parentElement.style.overflowAnchor = "none";
    }
    spacer.style.overflowAnchor = "none";
    const viewportHeight = scrollport.clientHeight;
    const totalHeight = decodedPages.reduce((height, page) => height + page.page.bounds.height * pageZoom + renderer.pageGap, 0);
    spacer.style.height = `${Math.max(0, totalHeight - viewportHeight)}px`;
  }

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
    // Read client dimensions after layout: a zoom can add or remove a scrollbar.
    const newRect = scrollport.getBoundingClientRect?.() ?? rect;
    const newClientLeft = scrollport.clientLeft || 0;
    const newClientTop = scrollport.clientTop || 0;
    const newClientX = anchor?.x ?? newRect.left + newClientLeft + scrollport.clientWidth / 2;
    const newClientY = anchor?.y ?? newRect.top + newClientTop + scrollport.clientHeight / 2;
    const position = zoomScrollPosition({ oldZoom, newZoom: next,
      scrollLeft: oldScrollLeft, scrollTop: oldScrollTop, viewportLeft: rect.left, viewportTop: rect.top,
      clientX, clientY, newClientX, newClientY, oldClientLeft, oldClientTop, newClientLeft, newClientTop,
      pageHeights: decodedPages.map(page => page.page.bounds.height), pageGap: renderer.pageGap });
    scrollport.scrollLeft = position.left;
    scrollport.scrollTop = position.top;
    updateViewport();
    repaintOverlay();
    try { onZoom({ zoom: pageZoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
  }

  function refitZoom(mode = zoomMode) {
    if (mode === "custom" || !decodedPages.length) return;
    if (!scrollport.clientWidth || !scrollport.clientHeight) return;
    const bounds = decodedPages[0].page.bounds;
    const next = calculateFitZoom(mode, bounds.width, bounds.height, scrollport.clientWidth,
      scrollport.clientHeight, renderer.pageGap);
    applyZoom(next, null, mode);
  }

  async function dispatch(opcode, values = {}, signal) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      if (requestId > (1n << 64n) - 1n) throw new EditorError("EDITOR_REQUEST_IDS_EXHAUSTED");
      const id = requestId++;
      const payload = encodeEditorCommand(opcode, values);
      const frame = encodeMessage({ type: MessageType.Editor, requestId: id, payload });
       const bytes = await client.request(frame, { signal });
       if (disposed) throw new EditorError("EDITOR_CLOSED");
      const message = decodeMessage(bytes, "event");
      if (message.requestId !== id || (message.type !== MessageType.EditorResult && message.type !== MessageType.Failed)) {
        throw new EditorError("EDITOR_RESPONSE_MISMATCH");
      }
      const response = decodeEditorResponse(message.type, message.payload);
      if (response.kind === "failed") throw new EditorError(response.code);
    if ([EditorOpcode.Load, EditorOpcode.InsertText, EditorOpcode.DropText, EditorOpcode.ReplaceRange, EditorOpcode.DeleteBackward, EditorOpcode.DeleteForward,
         EditorOpcode.DeleteWordBackward, EditorOpcode.DeleteWordForward, EditorOpcode.Undo, EditorOpcode.Redo, EditorOpcode.Replace, EditorOpcode.ReplaceAll].includes(opcode)) extensionMode = false;
      if (opcode === EditorOpcode.ProbePoint && response.kind !== "point") throw new EditorError("EDITOR_INVALID_RESPONSE");
       if (opcode === EditorOpcode.PreviewDocx && response.kind !== "preview") throw new EditorError("EDITOR_INVALID_RESPONSE");
       if (opcode === EditorOpcode.Search && response.kind !== "search") throw new EditorError("EDITOR_INVALID_RESPONSE");
        if (opcode === EditorOpcode.Statistics && response.kind !== "statistics") throw new EditorError("EDITOR_INVALID_RESPONSE");
        if ([EditorOpcode.Replace, EditorOpcode.ReplaceAll].includes(opcode) && response.kind !== "replacement") throw new EditorError("EDITOR_INVALID_RESPONSE");
       if (![EditorOpcode.Export, EditorOpcode.ProbePoint, EditorOpcode.PreviewDocx, EditorOpcode.Search, EditorOpcode.Statistics].includes(opcode)) {
        if (opcode === EditorOpcode.Load || opcode === EditorOpcode.State && !state) {
          pageZoom = 1;
          zoomMode = "custom";
          try { onZoom({ zoom: pageZoom, mode: zoomMode }); } catch { /* UI callbacks are isolated */ }
        }
         publish(response.kind === "replacement" ? response.state : response, { documentReplaced: opcode === EditorOpcode.Load });
      }
      return response;
  }

  function enqueueTask(run) {
    pendingTasks += 1;
    const result = queue.then(run, run).finally(() => { pendingTasks -= 1; });
    queue = result.catch(() => {});
    return result;
  }

  function enqueue(opcode, values = {}) { return enqueueTask(() => dispatch(opcode, values)); }
  function query(opcode, values = {}) {
    return enqueueTask(() => {
      if (!state || busy || disposed) throw new EditorError(disposed ? "EDITOR_CLOSED" : "EDITOR_BUSY");
      return dispatch(opcode, { ...values, revision: state.revision });
    });
  }

  function replace(opcode, values, signal) {
    return enqueueTask(() => {
      if (signal?.aborted || !state || values.revision !== state.revision) throw new EditorError("EDITOR_STALE_QUERY");
      if (busy || disposed || compositionStart) throw new EditorError(disposed ? "EDITOR_CLOSED" : "EDITOR_BUSY");
      // Mutations are only cancelled before dispatch; cancelling an in-flight
      // Worker edit could leave its model ahead of the last published UI state.
      return dispatch(opcode, { ...values, nowMs: nowMs() });
    });
  }

  async function applyInput(command) {
    if (!state || busy) return;
    textDragDrop?.cancel();
    if (command.type === "extensionMode") { extensionMode = command.enabled; return; }
    if (command.type === "selectAll") {
      extensionMode = false;
      return enqueueTask(() => dispatch(EditorOpcode.SetSelection, { anchor: 0, focus: new TextEncoder().encode(state.text).length }));
    }
    if (command.type === "deleteSelection") {
      return enqueueTask(async () => {
        if (command.revision !== state.revision || command.selection?.anchor !== state.anchor
            || command.selection?.focus !== state.focus || state.anchor === state.focus) {
          throw new EditorError("EDITOR_STALE_SELECTION");
        }
        return dispatch(EditorOpcode.DeleteBackward, { nowMs: nowMs() });
      });
    }
    if (command.type === "insertText") {
      const committed = command.inputType === "insertCompositionText" ? compositionCommit : null;
      if (command.inputType === "insertCompositionText") compositionCommit = null;
      return enqueueTask(async () => {
        if (committed && state?.revision === committed.revision
            && committed.selection && Number.isInteger(committed.selection.anchor) && Number.isInteger(committed.selection.focus)) {
          await dispatch(EditorOpcode.SetSelection, committed.selection);
        }
        const collapsed = state.anchor === state.focus;
        const raw = await dispatch(EditorOpcode.InsertText, { text: command.text, nowMs: nowMs() });
        const edit = collapsed ? frenchSpacingEdit(raw, command.text, command.inputType, spacingEnabled) : null;
        if (!edit) return raw;
        return dispatch(EditorOpcode.ReplaceRange, { ...edit, revision: raw.revision, nowMs: nowMs() });
      });
    }
    if (command.type === "delete") {
      if (!["content", "word"].includes(command.unit)) throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
      const opcode = command.unit === "word"
        ? (command.direction === "backward" ? EditorOpcode.DeleteWordBackward : EditorOpcode.DeleteWordForward)
        : (command.direction === "backward" ? EditorOpcode.DeleteBackward : EditorOpcode.DeleteForward);
      return enqueue(opcode, { nowMs: nowMs() });
    }
    if (command.type === "undo") return enqueue(EditorOpcode.Undo);
    if (command.type === "redo") return enqueue(EditorOpcode.Redo);
    if (command.type === "move") {
      let direction = command.direction;
      if (command.toDocumentEdge) direction = direction === "home" ? "documentStart" : "documentEnd";
      if (command.byWord && !command.toDocumentEdge) {
        direction = { left: "wordLeft", right: "wordRight", up: "paragraphUp", down: "paragraphDown" }[direction];
        if (!direction) throw new EditorError("EDITOR_UNSUPPORTED_INPUT");
      }
      return enqueue(EditorOpcode.Move, { direction, extend: Boolean(command.extend || extensionMode) });
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

  const getGeometry = () => ({
    pages: decodedPages.map(({ page }) => ({ minX: page.bounds.min.x, minY: page.bounds.min.y,
      width: page.bounds.width, height: page.bounds.height })),
    zoom: pageZoom, pageGap: renderer.pageGap,
  });
  textDragDrop = attachTextDragDrop({
    canvas, scrollport, getGeometry,
    getSelectionSnapshot: () => state ? { text: state.text, anchor: state.anchor,
      focus: state.focus, revision: state.revision } : null,
    isPointInSelection: (x, y) => {
      if (!state) return false;
      const bounds = canvas.getBoundingClientRect();
      return state.selectionRects.some(rect => {
        const r = viewportRect(rect, rect.page);
        return x >= bounds.left + r.x && x <= bounds.left + r.x + r.width
          && y >= bounds.top + r.y && y <= bounds.top + r.y + r.height;
      });
    },
    isEnabled: () => Boolean(state && !busy && !compositionStart && !disposed),
    probePoint: point => enqueueTask(() => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      return dispatch(EditorOpcode.ProbePoint, point);
    }),
    onDrop: drop => enqueueTask(() => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      return dispatch(EditorOpcode.DropText, { ...drop, text: drop.kind === "external" ? drop.text : "",
        sourceAnchor: drop.kind === "external" ? 0 : drop.sourceAnchor,
        sourceFocus: drop.kind === "external" ? 0 : drop.sourceFocus, nowMs: nowMs() });
    }),
    onClick: point => enqueueTask(() => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      if (point.revision !== state.revision) throw new EditorError("EDITOR_STALE_EDIT");
      return dispatch(EditorOpcode.SetSelection, { anchor: point.offset, focus: point.offset });
    }),
    onDragState: caret => { dropCaret = caret; repaintOverlay(); },
    focus: () => input.focus(), onError: reportError,
  });
  mouseSelection = attachMouseSelection({
    canvas, scrollport,
    getGeometry,
    getSelection: () => ({ anchor: state?.anchor ?? 0, focus: state?.focus ?? 0 }),
    isEnabled: () => Boolean(state && !busy && !compositionStart && !disposed),
    selectPoint: values => enqueueTask(async () => {
      if (busy || compositionStart || disposed) throw new EditorError("EDITOR_BUSY");
      extensionMode = false;
      return dispatch(EditorOpcode.SelectPoint, values);
    }),
    focus: () => input.focus(),
    onError: reportError,
  });
  scrollport.addEventListener("scroll", updateViewport, { passive: true });
  const onWheel = event => wheelZoom(event, { active: Boolean(state && !busy && !disposed && !compositionStart
      && !scrollport.hidden && !scrollport.closest?.("[hidden]")),
    modalOpen: isZoomModalOpen(globalThis.document), getZoom: () => pageZoom,
    setZoom: (value, point) => applyZoom(value, point), scrollport });
  scrollport.addEventListener("wheel", onWheel, { passive: false });
  globalThis.addEventListener?.("resize", scheduleViewport);
  let fitResizeFrame = 0;
  const scheduleRefit = () => {
    if (fitResizeFrame || disposed) return;
    fitResizeFrame = requestAnimationFrame(() => { fitResizeFrame = 0; refitZoom(); });
  };
  const fitResizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleRefit);
  fitResizeObserver?.observe(scrollport);
  globalThis.addEventListener?.("resize", scheduleRefit);
  const blink = setInterval(() => { caretVisible = !caretVisible; repaintOverlay(); }, 530);

  return Object.freeze({
    get state() { return state; },
    get busy() { return busy; },
    get element() { return input.element; },
    focus() { return input.focus(); },
    setFrenchSpacing(enabled) { spacingEnabled = Boolean(enabled); },
    setSuspended(value) { if (disposed) throw new EditorError("EDITOR_CLOSED"); setBusy(Boolean(value)); },
    previewDocx(bytes, { signal } = {}) {
      return enqueueTask(() => dispatch(EditorOpcode.PreviewDocx, { bytes }, signal));
    },
    setZoom(zoom) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      applyZoom(zoom);
    },
    fitZoom(mode) {
      if (disposed) throw new EditorError("EDITOR_CLOSED");
      if (!decodedPages.length) throw new EditorError("EDITOR_INVALID_STATE");
      if (!["width", "page", "pages"].includes(mode)) throw new TypeError("invalid zoom fit mode");
      refitZoom(mode);
    },
    getZoom() { return pageZoom; },
    getZoomMode() { return zoomMode; },
    getViewportState: viewportState,
    goToPage(page) {
      if (disposed || busy || !state || compositionStart) throw new EditorError("EDITOR_BUSY");
      scrollport.scrollTop = pageScrollTop(decodedPages, page, pageZoom, renderer.pageGap);
      updateViewport();
    },
    async initialize() { return enqueue(EditorOpcode.State); },
    getState() { return state; },
    search(queryText, options = {}) { return query(EditorOpcode.Search, { query: queryText, options }); },
    statistics() { return query(EditorOpcode.Statistics); },
    replaceCurrent(values, { signal } = {}) { return replace(EditorOpcode.Replace, values, signal); },
    replaceAll(values, { signal } = {}) { return replace(EditorOpcode.ReplaceAll, values, signal); },
    setSearchHighlights(result, index = -1) {
      if (disposed) return false;
      if (result === null) { searchHighlights = null; searchIndex = -1; repaintOverlay(); return true; }
      if (!state || result?.kind !== "search" || result.revision !== state.revision) return false;
      if (!Array.isArray(result.items) || result.items.length > 1000 || !Number.isInteger(index) || index < -1 || index >= result.items.length) {
        throw new EditorError("EDITOR_INVALID_RESPONSE");
      }
      let rectCount = 0;
      for (const item of result.items) {
        if (!Number.isInteger(item.start) || !Number.isInteger(item.end) || item.start < 0 || item.end <= item.start || item.end > state.textBytes.length
            || !Array.isArray(item.rects)) throw new EditorError("EDITOR_INVALID_RESPONSE");
        rectCount += item.rects.length;
        if (rectCount > 8192 || item.rects.some(rect => !decodedPages[rect.page])) throw new EditorError("EDITOR_INVALID_RESPONSE");
      }
      searchHighlights = result; searchIndex = index; repaintOverlay(); return true;
    },
    selectSearchResult(result, index, { signal } = {}) {
      return enqueueTask(async () => {
        if (signal?.aborted || disposed || busy || !state || result?.revision !== state.revision) throw new EditorError("EDITOR_STALE_QUERY");
        const item = result.items?.[index];
        if (!item) throw new EditorError("EDITOR_INVALID_RESPONSE");
        const selected = await dispatch(EditorOpcode.SetSelection, { anchor: item.start, focus: item.end });
        if (result.revision === selected.revision) { searchHighlights = result; searchIndex = index; repaintOverlay(); }
        return selected;
      });
    },
    async exportDocument() {
      const result = await enqueue(EditorOpcode.Export);
      if (result.kind !== "export") throw new EditorError("EDITOR_INVALID_EXPORT");
      return { revision: result.revision, bytes: result.bytes.slice() };
    },
    async newDocument() {
      if (busy || !await this.confirmReplace()) return { kind: "cancelled" };
      setBusy(true);
      try { return await enqueue(EditorOpcode.Load, { bytes: new Uint8Array() }); }
      finally { setBusy(false); input.focus(); }
    },
    async openDocument() {
      if (busy || !await this.confirmReplace()) return { kind: "cancelled" };
      setBusy(true);
      try {
        const result = await host.open();
        if (result.kind !== "ok") return result;
        return { kind: "ok", state: await enqueue(EditorOpcode.Load, { bytes: result.bytes }) };
      } finally { setBusy(false); input.focus(); }
    },
    async saveAs() {
      if (busy) return { kind: "error", key: "files.error.queue_full" };
      setBusy(true);
      try {
        const exported = await enqueue(EditorOpcode.Export);
        if (exported.kind !== "export") throw new EditorError("EDITOR_INVALID_EXPORT");
        const result = await host.save(exported.bytes);
        if (result.kind !== "ok") return result;
        const saved = await enqueue(EditorOpcode.MarkSaved, { revision: exported.revision });
        return { kind: "ok", state: saved };
      } finally { setBusy(false); input.focus(); }
    },
    undo() { return busy ? Promise.reject(new EditorError("EDITOR_BUSY")) : enqueue(EditorOpcode.Undo); },
    redo() { return busy ? Promise.reject(new EditorError("EDITOR_BUSY")) : enqueue(EditorOpcode.Redo); },
    async confirmReplace() {
      if (!state?.dirty) return true;
      if (busy || confirming) return false;
      confirming = true;
      input.focus();
      setBusy(true);
      try { return globalThis.confirm(confirmDiscard); }
      finally { confirming = false; setBusy(false); input.focus(); }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      clearInterval(blink);
      if (viewportFrame) cancelAnimationFrame(viewportFrame);
      scrollport.removeEventListener("scroll", updateViewport);
      scrollport.removeEventListener("wheel", onWheel);
      mouseSelection.dispose();
      textDragDrop.dispose();
      globalThis.removeEventListener?.("resize", scheduleViewport);
      fitResizeObserver?.disconnect();
      globalThis.removeEventListener?.("resize", scheduleRefit);
      if (fitResizeFrame) cancelAnimationFrame(fitResizeFrame);
      input.dispose(); renderer.destroy();
      compositionPreview.remove();
      dropMarker.remove();
    },
  });
}
