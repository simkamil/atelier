// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEngineClient } from "/engine-client.mjs";
import { decodeMessage, encodeMessage, MessageType } from "/wasm-protocol.mjs";
import { createEditor, EditorError } from "/editor.mjs";
import { createNativeHost } from "/native-host.mjs";
import { createRichPreview } from "/rich-preview.mjs";
import { createEditorShell } from "/editor-shell.mjs";
import { createZoomControls } from "/zoom-controls.mjs";
import { createNavigationTools } from "/navigation-tools.mjs";
import { createDocumentStatus } from "/document-status.mjs";
import { createSymbolDialog } from "/symbol-dialog.mjs";
import { SYMBOL_CATALOGUE } from "/symbol-catalogue.mjs";
import { createEditorContextMenu } from "/editor-context-menu.mjs";
import { createAboutDialog } from "/about-dialog.mjs";
import { ABOUT_LICENSES } from "/about-licenses.mjs";
import { BUILD_INFO } from "/build-info.mjs";
import { analyzeMessage, createI18n, parseCatalog } from "/i18n.mjs";

const byId = id => document.getElementById(id);
const nodes = {
  title: byId("status-title"), detail: byId("status-detail"), indicator: byId("status-indicator"), engine: byId("engine-state"),
  editorStatus: byId("editor-status"), documentState: byId("document-state"), canvas: byId("document-canvas"), textRepresentation: byId("document-text"),
  scrollport: byId("document-scroll"), spacer: byId("page-spacer"),
  new: byId("new-document"), open: byId("open-document"), save: byId("save-document"), undo: byId("undo"), redo: byId("redo"),
  frenchSpacing: byId("french-spacing"),
  openDocx: byId("open-docx"), closePreview: byId("close-preview"),
  textWorkspace: byId("text-workspace"), richWorkspace: byId("rich-workspace"),
};
// i18n-ignore-start: dernier recours quand web/i18n/fr-FR.txt est indisponible ; texte identique au catalogue (contrôlé par tests/i18n.mjs)
const FALLBACK = new Map(Object.entries({
  "status-a11y-ok": "Écho confirmé", "status-a11y-failed": "Échec", "status-ok": "Le moteur a renvoyé l’écho Ping.",
  "status-failed": "Le contrôle du moteur a échoué.", "status-success-detail": "Worker WebAssembly : transport vérifié, écho Ping reçu.",
  "status-failure-detail": "Le contrôle local n’a pas abouti.", "engine-ready": "Transport vérifié", "engine-error": "Indisponible",
  title: "Document texte",
  "status-loading": "Initialisation du document…", "status-ready": "Document prêt.", "status-saved": "Document enregistré.",
  "status-opened": "Document ouvert.", "status-new": "Document vierge créé.", "status-cancelled": "Opération annulée.",
  "status-host-unavailable": "Le dialogue de fichiers est indisponible.", "status-open-error": "Le document n’a pas pu être ouvert.",
  "status-save-error": "Le document n’a pas pu être enregistré.", "status-editor-error": "L’opération n’a pas pu être effectuée.",
  "files.error.invalid_text": "Le fichier ne contient pas de texte UTF-8 valide.",
  "files.error.too_large": "Le fichier dépasse la taille maximale de {limit}.",
  "files.error.queue_full": "Une autre opération sur les fichiers est en cours.",
  "status-stale-save": "Le document a changé pendant l’enregistrement. Enregistrez-le à nouveau.",
  "status-unsupported-input": "Cette commande de saisie n’est pas disponible.", "status-invalid-document": "Le document ou sa mise en page est invalide.",
  "confirm-discard": "Le document contient des modifications. Continuer sans les enregistrer ?",
  "toolbar-label": "Actions du document", "document-text-label": "Texte du document",
  "workspace-label": "Éditeur de texte", "pages-label": "Pages du document", "canvas-label": "Pages du document texte brut",
  "state-clean": "Enregistré", "state-dirty": "Modifications non enregistrées",
  "options-label": "Options", "option-french-spacing": "Espaces françaises automatiques",
  "action-open-docx": "Ouvrir DOCX…", "action-close-preview": "Retour au texte",
  "status-preview-loading": "Ouverture de l’aperçu DOCX…",
  "status-preview-ready": "Aperçu DOCX en lecture seule. Mise en page simplifiée.",
  "status-preview-notices": "Aperçu DOCX en lecture seule. Certains éléments ne sont pas affichés.",
  "status-preview-error": "L’aperçu DOCX n’a pas pu être ouvert. Document invalide, non pris en charge ou trop volumineux (8 Mio maximum).",
  "preview-title": "Aperçu DOCX", "preview-eyebrow": "DOCUMENT WORD · LECTURE SEULE", "preview-state": "Lecture seule", "preview-footer": "Atelier · aperçu DOCX",
  "zoom-group": "Zoom du document", "zoom-range": "Facteur de zoom", "zoom-percent": "Régler le zoom",
  "zoom-title": "Zoom…", "zoom-value": "Pourcentage (10 à 500)", "zoom-custom": "Pourcentage personnalisé",
  "zoom-width": "Largeur de page", "zoom-page": "Page entière", "zoom-pages": "Deux pages",
  "zoom-apply": "Appliquer", "zoom-cancel": "Annuler", "zoom-close": "Fermer",
  "zoom-invalid": "Saisissez un pourcentage entier entre 10 et 500.",
  "input-label": "Saisie du texte du document",
}));
// i18n-ignore-end

document.addEventListener("securitypolicyviolation", event => {
  const blocked = event.blockedURI || "";
  const directive = event.effectiveDirective || "";
  const message = `us112-csp|${directive}|${blocked}`;
  if (message.length <= 768) globalThis.chrome?.webview?.postMessage(message);
});

const FILE_ERROR_KEYS = ["files.error.invalid_text", "files.error.too_large", "files.error.queue_full"];
const MAX_TEXT_FILE_BYTES = 128 * 1024;

/** Loads the catalogue and returns the translator plus `messages`, the reader used to build module labels. */
async function loadCatalog() {
  const entries = new Map(FALLBACK);
  try {
    const response = await fetch("/i18n/fr-FR.txt", { cache: "no-store" });
    if (response.ok) for (const [key, entry] of parseCatalog(await response.text()).entries) entries.set(key, entry.value);
  } catch { /* French fallback keeps the shell understandable without the catalog. */ }
  const i18n = createI18n({ locale: "fr-FR", catalog: entries });
  const hasParams = new Map();
  /** Plain messages are translated; messages with parameters stay templates for modules that cannot receive `i18n`. */
  const messages = Object.freeze({
    has: key => i18n.has(key),
    get(key) {
      if (!i18n.has(key)) return undefined;
      if (!hasParams.has(key)) hasParams.set(key, analyzeMessage(entries.get(key)).params.size > 0);
      return hasParams.get(key) ? entries.get(key) : i18n.t(key);
    },
  });
  for (const element of document.querySelectorAll("[data-i18n]")) {
    if (i18n.has(element.dataset.i18n)) element.textContent = messages.get(element.dataset.i18n);
  }
  for (const element of document.querySelectorAll("[data-i18n-aria-label]")) {
    if (i18n.has(element.dataset.i18nAriaLabel)) element.setAttribute("aria-label", messages.get(element.dataset.i18nAriaLabel));
  }
  return { i18n, messages };
}

/** Build date (UTC day, YYYY-MM-DD) written the French way; any other value is shown unchanged. */
function buildDateText(value, i18n, messages) {
  if (value === null || value === undefined) return messages.get("about-buildUnavailable");
  const day = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00Z`) : null;
  return day && !Number.isNaN(day.getTime()) ? i18n.formatDate(day, { timeZone: "UTC" }) : value;
}

/** Message of a rejected file operation: catalogue key from the host, with its named parameters. */
function fileErrorMessage(key, i18n, messages) {
  if (!FILE_ERROR_KEYS.includes(key)) return messages.get(key);
  return i18n.t(key, key === "files.error.too_large" ? { limit: i18n.formatFileSize(MAX_TEXT_FILE_BYTES, { binary: true }) } : {});
}

function showPing(ok, messages) {
  document.documentElement.dataset.engineEcho = ok ? "ok" : "failed";
  nodes.title.textContent = messages.get(ok ? "status-ok" : "status-failed");
  nodes.detail.textContent = messages.get(ok ? "status-success-detail" : "status-failure-detail");
  nodes.indicator.className = `status-indicator ${ok ? "success" : "error"}`;
  nodes.indicator.setAttribute("aria-label", messages.get(ok ? "status-a11y-ok" : "status-a11y-failed"));
  nodes.engine.textContent = messages.get(ok ? "engine-ready" : "engine-error");
  nodes.engine.className = `engine-state ${ok ? "success" : "error"}`;
}

function messageForError(error, messages) {
  const key = error?.code === "EDITOR_STALE_SAVE" ? "status-stale-save"
    : error?.code === "EDITOR_UNSUPPORTED_INPUT" ? "status-unsupported-input"
      : ["EDITOR_INVALID_STATE", "EDITOR_INVALID_EXPORT", "INVALID_RESPONSE"].includes(error?.code) ? "status-invalid-document"
        : "status-editor-error";
  return messages.get(key) ?? FALLBACK.get(key);
}

function showEditorMessage(text, isError = false) {
  nodes.editorStatus.textContent = text;
  nodes.editorStatus.classList.toggle("error", isError);
}

async function start() {
  const { i18n, messages } = await loadCatalog();
  const toolKeys = ["searchTitle", "searchQuery", "searchMatchCase", "searchWholeWord", "searchIgnoreAccents", "searchMatchNbsp",
    "searchPrevious", "searchNext", "searchClose", "searchCount", "searchEmptyQuery", "searchNoResults", "searchTruncated",
    "searchBusy", "searchError", "searchDisabled", "searchWrapped", "searchResult", "searchOptions", "searchResults",
    "statusDocument", "statusSelection", "statisticsMetric", "statisticsDialogTitle", "statisticsClose", "statisticsScopeDocument",
    "statisticsScopeSelection", "statisticsFieldPages", "statisticsFieldWords", "statisticsFieldCharacters",
    "statisticsFieldCharactersWithSpaces", "statisticsFieldParagraphs", "statisticsFieldLines", "statisticsLoading",
    "statisticsError", "statisticsUnavailableData", "statisticsNotesTextBoxes", "statisticsNotesUnavailable",
    "replaceTitle", "replaceText", "replaceOne", "replaceAll", "replaceBusy", "replaceError", "replaceNone", "replaceCount"];
  const toolsAvailable = toolKeys.every(key => messages.has(key));
  const statusKeys = ["statusPageCount", "statusPageUnavailable", "statusLanguage", "statusLanguageUnknown", "statusViewText", "statusViewPreview",
    "statusSaved", "statusUnsaved", "statusReadOnly", "statusConfigure", "statusShowPage", "statusShowWords", "statusShowLanguage", "statusShowView",
    "statusShowZoom", "statusShowSaved", "statusGotoTitle", "statusGotoPage", "statusGotoApply", "statusGotoCancel", "statusGotoInvalid", "statusGotoRange", "statusGotoClose"];
  let zoomControls;
  let navigation;
  let documentStatus;
  let symbols;
  let about;
  let contextMenu;
  let symbolSnapshot;
  const symbolKeys = ["title", "close", "symbols", "specialCharacters", "search", "block", "allBlocks", "font", "recent", "insert", "noResults", "insertionFailed", "previous", "next", "page"];
  const specialKeys = ["emDash", "enDash", "noBreakSpace", "narrowNoBreakSpace", "nonBreakingHyphen", "copyright", "registered", "trademark", "section", "paragraph", "ellipsis"];
  const aboutKeys = ["dialogTitle", "close", "tabList", "overviewTab", "agplTab", "fontLicensePrefix", "unicodeTab", "version", "buildDate", "sourceUrl", "agplHeading", "unicodeHeading"];
  const contextKeys = ["menu", "undo", "redo", "cut", "copy", "paste", "selectAll"];
  const symbolsAvailable = [...symbolKeys.map(key => `symbols-${key}`), ...specialKeys.map(key => `symbols-${key}`), "symbolsTitle"].every(key => messages.has(key));
  const aboutAvailable = [...aboutKeys.map(key => `about-${key}`), "aboutTitle", "about-buildUnavailable"].every(key => messages.has(key));
  const contextAvailable = contextKeys.every(key => messages.has(`context-${key}`));
  const shell = createEditorShell({ toolbar: document.querySelector(".toolbar"), messages, navigationAvailable: toolsAvailable,
    symbolsAvailable, aboutAvailable, onSymbols: () => {
      if (!symbols || !editor?.canEdit || previewActive || document.querySelector("dialog:modal")) return;
      symbolSnapshot = editor.getState(); symbols.open({ returnFocusTo: editor.element });
    },
    onAbout: () => about?.open({ returnFocusTo: document.activeElement }),
    onZoom: () => zoomControls?.open({ returnFocusTo: document.activeElement }),
    onSearch: () => navigation?.openSearch(), onReplace: () => navigation?.openReplace(), onStatistics: () => navigation?.openStatistics() });
  let client;
  let host;
  let editor;
  let preview;
  let previewActive = false;
  let previewReady = false;
  let previewGeneration = 0;
  let previewAbort;
  let disposed = false;
  const listeners = new AbortController();
  const identityNodes = [byId("page-title"), document.querySelector('[data-i18n="eyebrow"]'), document.querySelector('[data-i18n="footer"]')];
  const textIdentity = identityNodes.map(node => node.textContent);
  const textWindowTitle = document.title;
  function setPreviewIdentity(active) {
    ["preview-title", "preview-eyebrow", "preview-footer"].forEach((key,index) => { identityNodes[index].textContent = active ? messages.get(key) : textIdentity[index]; });
    document.title = active ? `${messages.get("preview-title")} — Atelier` : textWindowTitle;
    nodes.documentState.textContent = active ? messages.get("preview-state") : messages.get(editor?.state?.dirty ? "state-dirty" : "state-clean");
    nodes.documentState.classList.toggle("dirty", !active && Boolean(editor?.state?.dirty));
  }
  function dispose() {
    if (disposed) return;
    disposed = true; previewGeneration += 1; previewAbort?.abort(); listeners.abort();
    symbols?.dispose(); about?.dispose(); contextMenu?.dispose(); navigation?.dispose(); documentStatus?.dispose(); zoomControls?.dispose(); preview?.dispose(); editor?.dispose(); host?.close?.(); client?.close(); shell.dispose();
  }
  let initialized = false;
  let busy = false;
  function updateStatus() {
    const view = previewActive ? preview : editor;
    documentStatus?.setState({ ...view?.getViewportState(), language: null, mode: previewActive ? "preview" : "text",
      dirty: previewActive ? null : editor?.state?.dirty ?? null, busy: !initialized || (previewActive ? !previewReady : busy) });
  }
  if (statusKeys.every(key => messages.has(key))) {
    const footer = document.querySelector("footer");
    footer.tabIndex = 0;
    footer.setAttribute("aria-label", messages.get("statusConfigure"));
    documentStatus = createDocumentStatus({ document, i18n, labels: Object.fromEntries(statusKeys.map(key => [key, messages.get(key)])), menuTarget: footer,
      onGoToPage: page => {
        if (disposed || !initialized || (previewActive ? !previewReady : busy)) return;
        try { (previewActive ? preview : editor).goToPage(page); } catch (error) { onError(error); }
      },
      onVisibilityChange: ({ field, visible }) => {
        if (field === "words") byId("statistics-slot").hidden = !visible;
        if (field === "zoom") byId("document-zoom").hidden = !visible;
      } });
    byId("document-status-slot").append(documentStatus.element);
  }
  const zoomLabels = Object.fromEntries(["group", "range", "percent", "title", "value", "custom", "width", "page", "pages", "apply", "cancel", "close", "invalid"]
    .map(key => [key, messages.get(`zoom-${key}`) ?? key]));
  zoomControls = createZoomControls({ document, labels: zoomLabels, onChange: ({ zoom, mode }) => {
    if (disposed || !initialized || (previewActive ? !previewReady : busy)) return;
    const view = previewActive ? preview : editor;
    if (mode === "custom") view.setZoom(zoom);
    else view.fitZoom(mode);
  } });
  byId("document-zoom").append(zoomControls.element);
  const updateZoom = () => {
    const view = previewActive ? preview : editor;
    const disabled = !initialized || (previewActive ? !previewReady : busy);
    zoomControls.setState({ zoom: view?.getZoom() ?? 1, mode: view?.getZoomMode() ?? "custom", disabled });
    shell.setZoomEnabled(!disabled);
    updateStatus();
  };
  updateZoom();
  window.addEventListener("pagehide", dispose, { once: true, signal: listeners.signal });
  const updateControls = state => {
    const blocked = !initialized || busy;
    nodes.new.disabled = blocked; nodes.open.disabled = blocked; nodes.save.disabled = blocked;
    nodes.undo.disabled = blocked || !state?.canUndo; nodes.redo.disabled = blocked || !state?.canRedo;
    nodes.frenchSpacing.disabled = blocked;
    nodes.openDocx.disabled = blocked;
    updateZoom();
    const toolsEnabled = !blocked && !previewActive;
    navigation?.setEnabled(toolsEnabled, { preview: previewActive });
    shell.setNavigationEnabled(toolsEnabled && toolsAvailable);
    shell.setSymbolsEnabled(toolsEnabled && symbolsAvailable);
    if (!toolsEnabled) { symbols?.close(); contextMenu?.close({ restoreFocus: false }); }
    shell.refresh();
  };
  const onBusy = value => { busy = value; updateControls(editor?.state); };
  const onState = (state, metadata) => {
    if (!previewActive) {
      nodes.documentState.textContent = messages.get(state.dirty ? "state-dirty" : "state-clean");
      nodes.documentState.classList.toggle("dirty", state.dirty);
    }
    updateControls(state);
    navigation?.update(state,metadata);
  };
  const onError = error => {
    showEditorMessage(messageForError(error, messages), true);
  };
  try {
    const wasmResponse = await fetch("/atelier_core_wasm.wasm", { cache: "no-store" });
    if (!wasmResponse.ok) throw new Error("WASM_UNAVAILABLE");
    const bytes = await wasmResponse.arrayBuffer();
    const worker = new Worker("/engine-worker.mjs", { type: "module", name: "atelier-engine" });
    client = await createEngineClient({ worker, bytes });
    const ping = encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: new Uint8Array() });
    const reply = decodeMessage(await client.request(ping), "command");
    if (reply.type !== MessageType.Ping || reply.requestId !== 1n || reply.payload.byteLength !== 0) throw new Error("PING_ECHO_FAILED");
    showPing(true, messages);

    try { host = createNativeHost(); }
    catch {
      host = {
        open: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        openDocx: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        save: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        notifyDirty: async () => ({ kind: "ok" }),
        close() {},
      };
    }
    try { nodes.frenchSpacing.checked = localStorage.getItem("atelier.frenchSpacing") !== "false"; }
    catch { /* The option works for this session even without preference storage. */ }
    editor = createEditor({
      client, host, canvas: nodes.canvas, scrollport: nodes.scrollport, spacer: nodes.spacer,
      frenchSpacing: nodes.frenchSpacing.checked,
      confirmDiscard: messages.get("confirm-discard"), inputLabel: messages.get("input-label"), textRepresentation: nodes.textRepresentation, onState, onError, onBusy,
      onZoom: () => { if (!previewActive) updateZoom(); },
      onViewport: () => { if (!previewActive) updateStatus(); },
    });
    function closePreview() {
      if (!previewActive || disposed) return;
      previewGeneration += 1; previewAbort?.abort(); previewAbort = null;
      previewActive = false;
      previewReady = false;
      nodes.richWorkspace.hidden = true; nodes.textWorkspace.hidden = false; nodes.closePreview.hidden = true;
      editor.setSuspended(false); preview.close(); shell.ribbon.selectTab("file"); shell.ribbon.setCollapsed(false); nodes.openDocx.focus();
      setPreviewIdentity(false);
      showEditorMessage(messages.get("status-ready"));
    }
    preview = createRichPreview({ canvas: byId("rich-canvas"), scrollport: byId("rich-scroll"),
      spacer: byId("rich-spacer"), textRepresentation: byId("rich-text"), onClose: closePreview, onError,
      onZoom: () => { if (previewActive) updateZoom(); }, onViewport: () => { if (previewActive) updateStatus(); } });
    if (toolsAvailable) navigation = createNavigationTools({ document, editor, paneSlot: byId("search-slot"), statusSlot: byId("statistics-slot"), i18n,
      labels: Object.fromEntries(toolKeys.map(key => [key, messages.get(key)])) });
    async function openDocx() {
      if (!initialized || busy || disposed) return { kind: "cancelled" };
      const generation = ++previewGeneration;
      previewAbort = new AbortController();
      const signal = previewAbort.signal;
      previewActive = true; previewReady = false; editor.setSuspended(true); nodes.closePreview.hidden = false;
      setPreviewIdentity(true);
      showEditorMessage(messages.get("status-preview-loading"));
      const current = () => !disposed && generation === previewGeneration;
      try {
        const result = await host.openDocx();
        if (!current()) return { kind: "cancelled" };
        if (result.kind !== "ok") {
          closePreview();
          showEditorMessage(messages.get(result.kind === "cancelled" ? "status-cancelled" : result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-preview-error"), result.kind !== "cancelled");
          return result;
        }
        const document = await editor.previewDocx(result.bytes, { signal });
        if (!current()) return { kind: "cancelled" };
        preview.setDocument(document);
        nodes.textWorkspace.hidden = true; nodes.richWorkspace.hidden = false;
        preview.open({ returnFocusTo: nodes.openDocx });
        previewReady = true; updateZoom();
        showEditorMessage(messages.get(document.noticeCount ? "status-preview-notices" : "status-preview-ready"));
        return { kind: "ok" };
      } catch (error) {
        if (!current()) return { kind: "cancelled" };
        closePreview(); showEditorMessage(messages.get("status-preview-error"), true);
        return { kind: "error", key: error.code ?? "EDITOR_PREVIEW_FAILED" };
      }
    }
    const state = await editor.initialize();
    initialized = true;
    if (symbolsAvailable) {
      symbols = createSymbolDialog({ catalogue: SYMBOL_CATALOGUE, i18n,
        labels: { ...Object.fromEntries(symbolKeys.map(key => [key, messages.get(`symbols-${key}`)])),
          specialNames: Object.fromEntries(specialKeys.map(key => [key, messages.get(`symbols-${key}`)])) },
        onInsert: async text => {
          if (previewActive || !editor.canEdit) return false;
          try { await editor.editSelection(text, symbolSnapshot); editor.focus(); return true; }
          catch (error) { onError(error); return false; }
        } });
      document.body.append(symbols.element);
    }
    if (aboutAvailable) {
      about = createAboutDialog({ labels: Object.fromEntries(aboutKeys.map(key => [key, messages.get(`about-${key}`)])),
        metadata: { ...BUILD_INFO, buildDate: buildDateText(BUILD_INFO.buildDate, i18n, messages) }, licenseTexts: ABOUT_LICENSES });
      document.body.append(about.element);
    }
    if (contextAvailable) {
      contextMenu = createEditorContextMenu({ target: nodes.canvas, focusTarget: editor.element,
        labels: Object.fromEntries(contextKeys.map(key => [key, messages.get(`context-${key}`)])),
        getState: () => { const current = editor.getState(); return { revision: current.revision, anchor: current.anchor, focus: current.focus,
          text: current.text, canUndo: current.canUndo, canRedo: current.canRedo, enabled: editor.canEdit && !previewActive, busy, preview: previewActive }; },
        clipboardCapabilities: { read: () => typeof navigator.clipboard?.readText === "function", write: () => typeof navigator.clipboard?.writeText === "function" },
        onError: () => showEditorMessage(messages.get("utilities-clipboardError"), true),
        onCommand: async (action, snapshot) => {
          if (previewActive || !editor.canEdit) throw new EditorError("EDITOR_BUSY");
          if (["copy", "cut"].includes(action)) {
            const bytes = new TextEncoder().encode(snapshot.text);
            const selected = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(Math.min(snapshot.anchor, snapshot.focus), Math.max(snapshot.anchor, snapshot.focus)));
            await navigator.clipboard.writeText(selected);
            if (action === "cut") await editor.editSelection("", snapshot);
          } else if (action === "paste") {
            const text = await navigator.clipboard.readText();
            await editor.editSelection(text, snapshot);
          } else await editor.selectionAction(action, snapshot);
          editor.focus();
        } });
      document.body.append(contextMenu.element);
    }
    onState(state);
    showEditorMessage(messages.get("status-ready"));
    nodes.openDocx.addEventListener("click", () => { void openDocx(); }, { signal: listeners.signal });
    nodes.closePreview.addEventListener("click", closePreview, { signal: listeners.signal });

    nodes.new.addEventListener("click", async () => {
      try {
        const result = await editor.newDocument();
        if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(messages.get("status-new"));
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.open.addEventListener("click", async () => {
      try {
        const result = await editor.openDocument();
        if (result.kind === "ok") showEditorMessage(messages.get("status-opened"));
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-open-error"), true);
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.save.addEventListener("click", async () => {
      try {
        const result = await editor.saveAs();
        if (result.kind === "ok") showEditorMessage(messages.get("status-saved"));
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-save-error"), true);
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.undo.addEventListener("click", () => { void editor.undo().catch(onError); }, { signal: listeners.signal });
    nodes.redo.addEventListener("click", () => { void editor.redo().catch(onError); }, { signal: listeners.signal });
    nodes.frenchSpacing.addEventListener("change", () => {
      editor.setFrenchSpacing(nodes.frenchSpacing.checked);
      try { localStorage.setItem("atelier.frenchSpacing", String(nodes.frenchSpacing.checked)); }
      catch { /* Keep the chosen behavior in memory if storage is unavailable. */ }
      editor.focus();
    }, { signal: listeners.signal });
    document.addEventListener("keydown", event => {
      if (document.querySelector("dialog:modal")) return;
      if (symbolsAvailable && event.target === editor.element && event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey
          && (event.code === "Space" || event.key === " ") && !event.isComposing && editor.canEdit && !previewActive) {
        event.preventDefault();
        void editor.editSelection("\u00a0", editor.getState()).catch(onError);
        return;
      }
      if (previewActive && event.key === "Escape" && !event.isComposing) { event.preventDefault(); closePreview(); return; }
      if (!(event.ctrlKey || event.metaKey) || event.altKey || event.isComposing) return;
      const action = event.key.toLowerCase();
      const button = action === "n" ? nodes.new : action === "o" ? nodes.open : action === "s" ? nodes.save : null;
      if (!button) return;
      event.preventDefault();
      if (!button.disabled) button.click();
    }, { signal: listeners.signal });
    updateControls(editor.state);
    editor.focus();
    return Object.freeze({
      status: "ready",
      getState: () => editor.getState(),
      exportDocument: () => editor.exportDocument(),
      editor,
      theme: shell.theme, ribbon: shell.ribbon,
      zoomControls,
      navigation,
      documentStatus,
      symbols, about, contextMenu,
      openDocx, closePreview, dispose,
      get previewActive() { return previewActive; },
    });
  } catch (error) {
    showPing(false, messages);
    showEditorMessage(messages.get("status-editor-error"), true);
    dispose();
    return Object.freeze({ status: "failed", code: error?.code ?? "EDITOR_INITIALIZATION_FAILED" });
  }

}

export const editorReady = start();
