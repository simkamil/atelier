// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTextInput } from "./text-input.mjs";
import { attachMouseSelection } from "./mouse-selection.mjs";
import { attachTextDragDrop } from "./text-drag-drop.mjs";
import { frenchSpacingEdit } from "./french-spacing.mjs";
import { decodeDisplayList } from "./display-list.mjs";
import { CanvasRenderer } from "./canvas-renderer.mjs";
import { decodeMessage, encodeMessage, MessageType } from "./wasm-protocol.mjs";
import { decodeEditorResponse, EditorOpcode, encodeEditorCommand } from "./editor-protocol.mjs";

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
export function createEditor({ client, host, canvas, scrollport, spacer, textRepresentation = null, frenchSpacing = true, confirmDiscard = "Le document contient des modifications. Continuer sans les enregistrer ?", onState = () => {}, onError = () => {}, onBusy = () => {} } = {}) {
  if (!client || typeof client.request !== "function" || !host || typeof host.open !== "function"
      || typeof host.save !== "function" || typeof host.notifyDirty !== "function") throw new TypeError("client and native host are required.");
  if (!canvas?.getBoundingClientRect || !scrollport?.addEventListener || !spacer?.style) throw new TypeError("Canvas, scrollport and spacer are required.");
  if (textRepresentation !== null && (typeof textRepresentation !== "object" || typeof textRepresentation.textContent !== "string")) {
    throw new TypeError("textRepresentation must be a text-capable DOM element.");
  }
  if (![onState, onError, onBusy].every(callback => typeof callback === "function")) throw new TypeError("Editor callbacks must be functions.");

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
    renderer.setEditorOverlay({ selectionRects, caret, compositionRects, caretVisible });
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
    canvas.style.height = `${Math.max(220, scrollport.clientHeight)}px`;
    renderer.setViewport({
      width: canvas.clientWidth,
      height: scrollport.clientHeight,
      zoom: pageZoom,
      scrollX: scrollport.scrollLeft,
      scrollY: scrollport.scrollTop,
      dpr: globalThis.devicePixelRatio || 1,
    });
    repaintOverlay();
  }

  let viewportFrame = 0;
  function scheduleViewport() {
    if (viewportFrame || disposed) return;
    viewportFrame = requestAnimationFrame(() => {
      viewportFrame = 0;
      updateViewport();
    });
  }

  function publish(nextState) {
    if (nextState.kind !== "state") throw new EditorError("EDITOR_INVALID_STATE");
    // Decode every page before publishing state; a malformed page must never blank the prior UI.
    const pages = nextState.pages.map(bytes => decodeDisplayList(bytes));
    if (pages.length === 0 || pages.length > 1024 || pages.some(page =>
      Math.abs(page.page.bounds.width - 595.276) > 0.02 || Math.abs(page.page.bounds.height - 841.89) > 0.02)) {
      throw new EditorError("EDITOR_INVALID_STATE");
    }
    state = nextState;
    decodedPages = pages;
    if (textRepresentation) textRepresentation.textContent = nextState.text;
    renderer.setPages(pages);
    const totalHeight = pages.reduce((height, page) => height + page.page.bounds.height * pageZoom + renderer.pageGap, 0);
    spacer.style.height = `${Math.max(0, totalHeight - scrollport.clientHeight)}px`;
    const caret = viewportRect({ x: nextState.caret.x, y: nextState.caret.y, width: 1, height: nextState.caret.height }, nextState.caret.page);
    const pad = 24;
    const caretContentY = caret.y + scrollport.scrollTop;
    if (caretContentY < scrollport.scrollTop + pad) scrollport.scrollTop = Math.max(0, caretContentY - pad);
    else if (caretContentY + caret.height > scrollport.scrollTop + scrollport.clientHeight - pad) {
      scrollport.scrollTop = Math.max(0, caretContentY + caret.height - scrollport.clientHeight + pad);
    }
    updateViewport();
    try { onState(nextState); } catch { /* UI callbacks are isolated */ }
    if (lastHostDirty !== nextState.dirty) {
      lastHostDirty = nextState.dirty;
      void host.notifyDirty(nextState.dirty).then(result => {
        if (result?.kind === "error") reportError(new EditorError(result.key));
      }).catch(reportError);
    }
    return nextState;
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
        EditorOpcode.DeleteWordBackward, EditorOpcode.DeleteWordForward, EditorOpcode.Undo, EditorOpcode.Redo].includes(opcode)) extensionMode = false;
      if (opcode === EditorOpcode.ProbePoint && response.kind !== "point") throw new EditorError("EDITOR_INVALID_RESPONSE");
       if (opcode === EditorOpcode.PreviewDocx && response.kind !== "preview") throw new EditorError("EDITOR_INVALID_RESPONSE");
       if (![EditorOpcode.Export, EditorOpcode.ProbePoint, EditorOpcode.PreviewDocx].includes(opcode)) publish(response);
      return response;
  }

  function enqueueTask(run) {
    pendingTasks += 1;
    const result = queue.then(run, run).finally(() => { pendingTasks -= 1; });
    queue = result.catch(() => {});
    return result;
  }

  function enqueue(opcode, values = {}) { return enqueueTask(() => dispatch(opcode, values)); }

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
      renderer.setEditorOverlay({ selectionRects: state?.selectionRects.map(rect => viewportRect(rect, rect.page)) ?? [], caret: state ? caretViewport() : null, caretVisible });
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
  globalThis.addEventListener?.("resize", scheduleViewport);
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
    setZoom(zoom) { if (!Number.isFinite(zoom) || zoom < 0.5 || zoom > 2) throw new RangeError("zoom is outside the supported range"); pageZoom = zoom; if (state) publish(state); },
    async initialize() { return enqueue(EditorOpcode.State); },
    getState() { return state; },
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
      mouseSelection.dispose();
      textDragDrop.dispose();
      globalThis.removeEventListener?.("resize", scheduleViewport);
      input.dispose(); renderer.destroy();
      compositionPreview.remove();
      dropMarker.remove();
    },
  });
}
