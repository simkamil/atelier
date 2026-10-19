// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEngineClient } from "/engine-client.mjs";
import { decodeMessage, encodeMessage, MessageType } from "/wasm-protocol.mjs";
import { createRichEditor, EditorError } from "/rich-editor.mjs";
import { createNativeHost } from "/native-host.mjs";
import { createEditorShell } from "/editor-shell.mjs";
import { createZoomControls } from "/zoom-controls.mjs";
import { createNavigationTools } from "/navigation-tools.mjs";
import { createDocumentStatus } from "/document-status.mjs";
import { createSymbolDialog } from "/symbol-dialog.mjs";
import { SYMBOL_CATALOGUE } from "/symbol-catalogue.mjs";
import { createEditorContextMenu } from "/editor-context-menu.mjs";
import { createAboutDialog } from "/about-dialog.mjs";
import { createPropertiesDialog, PROPERTIES_LABEL_KEYS } from "/properties-dialog.mjs";
import { ABOUT_LICENSES } from "/about-licenses.mjs";
import { BUILD_INFO } from "/build-info.mjs";
import { analyzeMessage, createI18n, parseCatalog } from "/i18n.mjs";
import { createImagePicker, imageMessageKey } from "/image-insert.mjs";
import { createFileTab, FILE_TAB_KEYS } from "/file-tab.mjs";
import { createFormatController, createFormatKeyRouter } from "/format-controller.mjs";
import { createCharFormat } from "/format-char.mjs";
import { createParaFormat } from "/format-para.mjs";
import { createStylesPane } from "/styles-pane.mjs";

const READER_MODE_KEY = "atelier.screenReaderMode";
const byId = id => document.getElementById(id);
const nodes = {
  title: byId("status-title"), detail: byId("status-detail"), indicator: byId("status-indicator"), engine: byId("engine-state"),
  editorStatus: byId("editor-status"), documentState: byId("document-state"), canvas: byId("document-canvas"), textRepresentation: byId("document-text"),
  scrollport: byId("document-scroll"), spacer: byId("page-spacer"),
  new: byId("new-document"), open: byId("open-document"), save: byId("save-document"), undo: byId("undo"), redo: byId("redo"),
  frenchSpacing: byId("french-spacing"),
  openDocx: byId("open-docx"),
  readonlyBanner: byId("readonly-banner"), readonlyText: byId("readonly-text"), readonlyCopy: byId("readonly-copy"), options: document.querySelector(".editor-options"),
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
  "action-open-docx": "Ouvrir DOCX…",
  "zoom-group": "Zoom du document", "zoom-range": "Facteur de zoom", "zoom-percent": "Régler le zoom",
  "zoom-title": "Zoom…", "zoom-value": "Pourcentage (10 à 500)", "zoom-custom": "Pourcentage personnalisé",
  "zoom-width": "Largeur de page", "zoom-page": "Page entière", "zoom-pages": "Deux pages",
  "zoom-apply": "Appliquer", "zoom-cancel": "Annuler", "zoom-close": "Fermer",
  "zoom-invalid": "Saisissez un pourcentage entier entre 10 et 500.",
  "input-label": "Saisie du texte du document",
  "mirror.label": "Contenu du document",
  "mirror.heading": "Titre de niveau {level}",
  "mirror.list.item": "Élément de liste, niveau {level}",
  "mirror.cell": "Cellule de tableau",
  "mirror.opaque": "Contenu non textuel",
  "mirror.image.noalt": "Image sans texte alternatif",
  "mirror.empty": "Paragraphe vide",
  "mirror.selection": "Sélection : {text}",
  "mirror.prefixed": "{prefix} : {text}",
  "mirror.selection.added": "Sélectionné : {text}",
  "mirror.selection.removed": "Désélectionné : {text}",
  "mirror.selection.count": "{count, plural, one {# caractère sélectionné} other {# caractères sélectionnés}}",
  "mirror.selection.uncount": "{count, plural, one {# caractère désélectionné} other {# caractères désélectionnés}}",
  "mirror.selection.cleared": "Sélection annulée",
  "mirror.reader.on": "Mode lecteur d’écran activé",
  "mirror.reader.off": "Mode lecteur d’écran désactivé",
  "mirror.reader.truncated": "{selection} Seul le paragraphe courant est dans la zone de saisie.",
  "mirror.reader.toggle": "Mode lecteur d’écran",
  "mirror.reader.toggle.hint": "Active ou désactive le mode lecteur d’écran ({shortcut}).",
  "mirror.reader.shortcut": "Alt+Maj+F1",
  "readonly-banner": "Document en lecture seule — modifications acceptées affichées.",
  "readonly-copy": "Créer une copie modifiable",
  "status-atlas-full": "Ce document contient trop de glyphes différents pour l’affichage ; fermez-le puis rouvrez-le.",
  "status-readonly-refused": "Ce document est en lecture seule : créez une copie modifiable pour le modifier.",
  "status-move-unsupported": "Ce texte contient un lien, un champ, un signet ou un objet : il ne peut pas être déplacé par glisser-déposer.",
  "status-readonly-opened": "Document ouvert en lecture seule : modifications acceptées affichées.",
  "status-copy-created": "Copie modifiable créée : modifications acceptées, commentaires retirés.",
  "status-copy-error": "La copie modifiable n’a pas pu être créée.",
  "status-copy-unsupported": "La copie modifiable ne peut pas être créée : certaines révisions ne s’acceptent pas fidèlement.",
  "status-unsupported-v2": "Cette fonction n’est pas encore disponible avec le nouvel éditeur.",
  "status-clipboard-unavailable": "Le texte sélectionné n’est pas encore prêt pour le presse-papiers. Réessayez dans un instant.",
  "status-selection-too-large": "La sélection est trop longue pour le presse-papiers (256 Kio au maximum).",
  "v2-unavailable-search": "Rechercher, remplacer et les statistiques ne sont pas encore disponibles avec le nouvel éditeur.",
  "v2-unavailable-properties": "Les propriétés du document ne sont pas encore disponibles avec le nouvel éditeur.",
  "dialog-open-title": "Ouvrir un document",
  "dialog-save-title": "Enregistrer le document",
  "dialog-filter-txt": "Texte brut",
  "dialog-filter-docx": "Document Word",
  "dialog-filter-docm": "Document Word avec macros",
  "dialog-filter-dotx": "Modèle Word",
  "dialog-default-name": "Document",
}));
// i18n-ignore-end

document.addEventListener("securitypolicyviolation", event => {
  const blocked = event.blockedURI || "";
  const directive = event.effectiveDirective || "";
  const message = `us112-csp|${directive}|${blocked}`;
  if (message.length <= 768) globalThis.chrome?.webview?.postMessage(message);
});

const FILE_ERROR_KEYS = ["files.error.invalid_text", "files.error.too_large", "files.error.queue_full", "files.error.unsupported_type", "files.error.timeout"];
const MAX_RICH_FILE_BYTES = 32 * 1024 * 1024;

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
function fileErrorMessage(key, i18n, messages, limit = MAX_RICH_FILE_BYTES) {
  if (!FILE_ERROR_KEYS.includes(key)) return messages.get(key);
  return i18n.t(key, key === "files.error.too_large" ? { limit: i18n.formatFileSize(limit, { binary: true }) } : {});
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
  const key = imageMessageKey(error?.code) ?? (error?.code === "EDITOR_STALE_SAVE" ? "status-stale-save"
    : error?.code === "EDITOR_UNSUPPORTED_INPUT" ? "status-unsupported-input"
      : ["EDITOR_INVALID_STATE", "EDITOR_INVALID_EXPORT", "INVALID_RESPONSE"].includes(error?.code) ? "status-invalid-document"
        : error?.code === "EDITOR_READ_ONLY" ? "status-readonly-refused"
        : error?.code === "EDITOR_MOVE_UNSUPPORTED" ? "status-move-unsupported"
          : error?.code === "EDITOR_UNSUPPORTED_V2" ? "status-unsupported-v2"
            : ["EDITOR_CLIPBOARD_UNAVAILABLE"].includes(error?.code) ? "status-clipboard-unavailable"
              : error?.code === "EDITOR_SELECTION_TOO_LARGE" ? "status-selection-too-large"
                : error?.code === "CLIENT_ATLAS_FULL" ? "status-atlas-full"
                : error?.code === "RICH_REVISIONS_UNSUPPORTED" ? "status-copy-unsupported"
                  : "status-editor-error");
  return messages.get(key) ?? FALLBACK.get(key) ?? FALLBACK.get("status-editor-error");
}

function showEditorMessage(text, isError = false) {
  nodes.editorStatus.textContent = text;
  nodes.editorStatus.classList.toggle("error", isError);
}

async function start() {
  const { i18n, messages } = await loadCatalog();
  let storage = null;
  try { storage = globalThis.localStorage; } catch { /* preference storage can be blocked */ }
  const engine = "v2";
  const fileLimit = MAX_RICH_FILE_BYTES;
  document.documentElement.dataset.engine = engine;
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
  let properties;
  let contextMenu;
  let symbolSnapshot;
  const symbolKeys = ["title", "close", "symbols", "specialCharacters", "search", "block", "allBlocks", "font", "recent", "insert", "noResults", "insertionFailed", "previous", "next", "page"];
  const specialKeys = ["emDash", "enDash", "noBreakSpace", "narrowNoBreakSpace", "nonBreakingHyphen", "copyright", "registered", "trademark", "section", "paragraph", "ellipsis"];
  const aboutKeys = ["dialogTitle", "close", "tabList", "overviewTab", "agplTab", "fontLicensePrefix", "unicodeTab", "version", "buildDate", "sourceUrl", "agplHeading", "unicodeHeading"];
  const contextKeys = ["menu", "undo", "redo", "cut", "copy", "paste", "selectAll"];
  const symbolsAvailable = [...symbolKeys.map(key => `symbols-${key}`), ...specialKeys.map(key => `symbols-${key}`), "symbolsTitle"].every(key => messages.has(key));
  const propertiesAvailable = [...PROPERTIES_LABEL_KEYS.map(key => `properties-${key}`), "propertiesTitle", "status-properties-loadError"].every(key => messages.has(key));
  const imageKeys = ["image-error-unsupported", "image-error-corrupt", "image-error-too-large", "image-error-limit", "image-error-read", "image-inserted", "ribbon-image"];
  const imageAvailable = imageKeys.every(key => messages.has(key));
  const aboutAvailable = [...aboutKeys.map(key => `about-${key}`), "aboutTitle", "about-buildUnavailable"].every(key => messages.has(key));
  const contextAvailable = contextKeys.every(key => messages.has(`context-${key}`));
  // US-320 : l'onglet Fichier (modèles, récents, commandes) ; sans ses messages, l'onglet d'origine reste.
  const fileTab = FILE_TAB_KEYS.every(key => messages.has(key))
    ? createFileTab({ document, i18n, onStatus: (text, isError) => showEditorMessage(text, isError), onResult: (result, success) => reportOpened(result, success) })
    : null;
  /** Message d'une ouverture lancée depuis l'onglet Fichier (modèle ou récent). */
  function reportOpened(result, success = messages.get("status-opened")) {
    if (result.kind === "ok") showEditorMessage(editor?.state?.readOnly ? messages.get("status-readonly-opened") : success);
    else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
    else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-open-error"), true);
  }
  // US-240 / US-243 : contrôleur de mise en forme partagé (un FormatState par changement) et modules du ruban ; l'éditeur
  // est branché sur le contrôleur à son ouverture (`formatController.attach`).
  const formatController = createFormatController({ onError: error => onError(error) });
  const announceFormat = (text, isError = false) => showEditorMessage(text, isError);
  const charFormat = createCharFormat({ document, i18n, controller: formatController, announce: announceFormat });
  const paraFormat = createParaFormat({ document, i18n, controller: formatController, announce: announceFormat });
  // US-250 : galerie de styles, volet Styles et raccourcis Titre 1 à 3 (styles-pane.mjs).
  const stylesPane = createStylesPane({ document, i18n, controller: formatController, announce: announceFormat, slot: byId("styles-slot") });
  const shell = createEditorShell({ toolbar: document.querySelector(".toolbar"), messages, navigationAvailable: toolsAvailable,
    symbolsAvailable, imageAvailable, aboutAvailable, propertiesAvailable, fileTab, charFormat, paraFormat, stylesPane, onImage: () => { void insertPickedImages(); }, onProperties: () => {
      if (!properties || document.querySelector("dialog:modal")) return;
      if (!initialized || busy || !editor?.capabilities?.properties) return;
      properties.open({ returnFocusTo: document.activeElement }).catch(() => showEditorMessage(messages.get("status-properties-loadError"), true));
    }, onSymbols: () => {
      if (!symbols || !editor?.canEdit || document.querySelector("dialog:modal")) return;
      symbolSnapshot = editor.getState(); symbols.open({ returnFocusTo: editor.element });
    },
    onAbout: () => about?.open({ returnFocusTo: document.activeElement }),
    onZoom: () => zoomControls?.open({ returnFocusTo: document.activeElement }),
    onSearch: () => navigation?.openSearch(), onReplace: () => navigation?.openReplace(), onStatistics: () => navigation?.openStatistics() });
  let client;
  let host;
  let lastReadTicket = null;
  let editor;
  let formatKeys = null;
  let disposed = false;
  const imagePicker = imageAvailable ? createImagePicker() : null;
  async function insertPickedImages() {
    if (!imagePicker || !editor?.canEdit || document.querySelector("dialog:modal")) return;
    try {
      const files = await imagePicker.open();
      if (files.length > 0) await editor.insertImages(files);
    } catch (error) { onError(error); }
    editor.focus();
  }
  const listeners = new AbortController();
  /** Banner of a read-only DOCX (revisions or comments, v2): accepted changes are shown, a copy can be made editable. */
  function showReadOnly(active) {
    // One status region, always in the DOM: only its text changes (a region inserted with its text is not announced everywhere).
    nodes.readonlyText.textContent = active ? messages.get("readonly-banner") : "";
    nodes.readonlyBanner.dataset.active = active ? "true" : "false";
    nodes.readonlyCopy.hidden = !active;
    if (!active) nodes.readonlyCopy.disabled = false;
  }
  function dispose() {
    if (disposed) return;
    disposed = true; listeners.abort();
    formatKeys?.dispose(); formatController.dispose(); charFormat.dispose(); paraFormat.detachEditor(); paraFormat.dispose(); stylesPane.dispose(); fileTab?.dispose(); imagePicker?.dispose(); symbols?.dispose(); about?.dispose(); properties?.dispose(); contextMenu?.dispose(); navigation?.dispose(); documentStatus?.dispose(); zoomControls?.dispose(); editor?.dispose(); host?.close?.(); client?.close(); shell.dispose();
  }
  let initialized = false;
  let busy = false;
  function updateStatus() {
    documentStatus?.setState({ ...editor?.getViewportState(), language: null, mode: "text",
      dirty: editor?.state?.dirty ?? null, busy: !initialized || busy });
  }
  if (statusKeys.every(key => messages.has(key))) {
    const footer = document.querySelector("footer");
    footer.tabIndex = 0;
    footer.setAttribute("aria-label", messages.get("statusConfigure"));
    documentStatus = createDocumentStatus({ document, i18n, labels: Object.fromEntries(statusKeys.map(key => [key, messages.get(key)])), menuTarget: footer,
      onGoToPage: page => {
        if (disposed || !initialized || busy) return;
        try { editor.goToPage(page); } catch (error) { onError(error); }
      },
      onVisibilityChange: ({ field, visible }) => {
        if (field === "words") byId("statistics-slot").hidden = !visible;
        if (field === "zoom") byId("document-zoom").hidden = !visible;
      } });
    byId("document-status-slot").append(documentStatus.element);
  }
  const zoomLabels = Object.fromEntries(["group", "range", "percent", "title", "value", "custom", "width", "page", "pages", "apply", "cancel", "close", "invalid"]
    .map(key => [key, messages.get(`zoom-${key}`) ?? key]));
  zoomControls = createZoomControls({ document, labels: zoomLabels, i18n, onChange: ({ zoom, mode }) => {
    if (disposed || !initialized || busy) return;
    if (mode === "custom") editor.setZoom(zoom);
    else editor.fitZoom(mode);
  } });
  byId("document-zoom").append(zoomControls.element);
  const updateZoom = () => {
    const disabled = !initialized || busy;
    zoomControls.setState({ zoom: editor?.getZoom() ?? 1, mode: editor?.getZoomMode() ?? "custom", disabled });
    shell.setZoomEnabled(!disabled);
    shell.setPropertiesEnabled(propertiesAvailable && !disabled && Boolean(editor?.capabilities?.properties));
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
    fileTab?.setBlocked(blocked);
    charFormat.setBlocked(blocked); paraFormat.setBlocked(blocked); stylesPane.setBlocked(blocked);
    updateZoom();
    const toolsEnabled = !blocked;
    const writable = !state?.readOnly;
    navigation?.setEnabled(toolsEnabled);
    shell.setNavigationEnabled(toolsEnabled && toolsAvailable && Boolean(editor?.capabilities?.search));
    shell.setSymbolsEnabled(toolsEnabled && symbolsAvailable && writable);
    shell.setImageEnabled(toolsEnabled && imageAvailable && writable);
    if (!toolsEnabled) { symbols?.close(); contextMenu?.close({ restoreFocus: false }); }
    shell.refresh();
  };
  const onBusy = value => { busy = value; updateControls(editor?.state); };
  const onState = (state, metadata) => {
    nodes.documentState.textContent = messages.get(state.dirty ? "state-dirty" : "state-clean");
    nodes.documentState.classList.toggle("dirty", state.dirty);
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

    try {
      const native = createNativeHost();
      // US-320 : le dernier ticket de lecture désigne le fichier à noter dans les récents une fois le document accepté.
      host = Object.freeze({ ...native, readTicket: (ticket, options) => { lastReadTicket = ticket; return native.readTicket(ticket, options); } });
    } catch {
      host = {
        open: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        openDocx: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        takeLaunch: async () => ({ kind: "ok" }),
        onLaunch: () => () => {},
        save: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }),
        notifyDirty: async () => ({ kind: "ok" }),
        close() {},
      };
    }
    try { nodes.frenchSpacing.checked = localStorage.getItem("atelier.frenchSpacing") !== "false"; }
    catch { /* The option works for this session even without preference storage. */ }
    const editorOptions = {
      client, host, canvas: nodes.canvas, scrollport: nodes.scrollport, spacer: nodes.spacer,
      frenchSpacing: nodes.frenchSpacing.checked,
      confirmDiscard: messages.get("confirm-discard"), inputLabel: messages.get("input-label"), textRepresentation: nodes.textRepresentation, onState, onError, onBusy,
      onZoom: updateZoom,
      onViewport: updateStatus,
      imageAlt: number => i18n.t("image-alt-default", { number }),
      onImages: ({ inserted, failed }) => { if (inserted > 0 && failed === 0) showEditorMessage(i18n.t("image-inserted", { count: inserted })); },
    };
    {
      let readerMode = false;
      try { readerMode = storage?.getItem(READER_MODE_KEY) === "true"; } catch { /* default: off */ }
      editor = createRichEditor({ ...editorOptions, t: (key, values) => i18n.t(key, values), version: BUILD_INFO.version, screenReaderMode: readerMode,
        onScreenReaderMode: on => { try { storage?.setItem(READER_MODE_KEY, String(on)); } catch { /* kept for this session */ } },
        onReadOnly: info => showReadOnly(info.readOnly && info.hasRevisions),
        dialogLabels: { openTitle: messages.get("dialog-open-title"), saveTitle: messages.get("dialog-save-title"), txt: messages.get("dialog-filter-txt"),
          docx: messages.get("dialog-filter-docx"), docm: messages.get("dialog-filter-docm"), dotx: messages.get("dialog-filter-dotx"), defaultName: messages.get("dialog-default-name") } });
      void formatController.attach(editor);
      // US-243 : surcouche des marques de mise en forme, dessinée au-dessus des pages de l'éditeur.
      paraFormat.attachEditor(editor, { canvas: editorOptions.canvas, scrollport: editorOptions.scrollport, spacer: editorOptions.spacer });
      // Raccourcis des modules de mise en forme : chaque module les déclare dans keyBindings() (voir format-controller.mjs).
      formatKeys = createFormatKeyRouter({ document, target: () => editor?.element, modules: [charFormat, paraFormat, stylesPane], isEnabled: () => Boolean(editor?.canEdit) });
      const { createReaderModeToggle } = await import("/document-mirror.mjs");
      const toggle = createReaderModeToggle({ container: nodes.options, input: editor.input, t: (key, values) => i18n.t(key, values) });
      listeners.signal.addEventListener("abort", () => toggle.dispose(), { once: true });
      // What the rich editor offers is enabled like in v1; a command it lacks stays greyed with its reason.
      shell.setUnavailable?.(editor.capabilities.search ? null : messages.get("v2-unavailable-search"),
        editor.capabilities.properties ? null : messages.get("v2-unavailable-properties"));
    }
    if (toolsAvailable && editor.capabilities.search) navigation = createNavigationTools({ document, editor, paneSlot: byId("search-slot"), statusSlot: byId("statistics-slot"), i18n,
      labels: Object.fromEntries(toolKeys.map(key => [key, messages.get(key)])) });
    /** Le document vient d'être accepté par l'éditeur : le natif note le fichier lu par le dernier ticket. */
    async function noteOpened() {
      const ticket = lastReadTicket;
      lastReadTicket = null;
      if (ticket) { try { await host.recentNote?.(ticket); } catch { /* confort : les récents ne gênent pas l'ouverture */ } }
      await fileTab?.refreshRecents();
    }
    async function openDocxInEditor(source) {
      if (!initialized || busy || disposed) return { kind: "cancelled" };
      const result = await editor.openDocument(source, { formats: ["docx", "docm", "dotx", "txt"] });
      if (result.kind === "ok") showEditorMessage(messages.get(editor.state?.readOnly ? "status-readonly-opened" : "status-opened"));
      else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
      else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-open-error"), true);
      if (result.kind === "ok") void noteOpened();
      return result;
    }
    const openDocx = source => openDocxInEditor(source);
    const state = await editor.initialize();
    initialized = true;
    if (symbolsAvailable) {
      symbols = createSymbolDialog({ catalogue: SYMBOL_CATALOGUE, i18n,
        labels: { ...Object.fromEntries(symbolKeys.map(key => [key, messages.get(`symbols-${key}`)])),
          specialNames: Object.fromEntries(specialKeys.map(key => [key, messages.get(`symbols-${key}`)])) },
        onInsert: async text => {
          if (!editor.canEdit) return false;
          try { await editor.editSelection(text, symbolSnapshot); editor.focus(); return true; }
          catch (error) { onError(error); return false; }
        } });
      document.body.append(symbols.element);
    }
    if (aboutAvailable) {
      about = createAboutDialog({ labels: Object.fromEntries(aboutKeys.map(key => [key, messages.get(`about-${key}`)])),
        metadata: { ...BUILD_INFO, buildDate: buildDateText(BUILD_INFO.buildDate, i18n, messages), buildDateIso: BUILD_INFO.buildDate }, licenseTexts: ABOUT_LICENSES });
      document.body.append(about.element);
    }
    if (propertiesAvailable) {
      properties = createPropertiesDialog({ i18n,
        load: () => editor.readProperties(),
        apply: async changes => {
          const applied = await editor.applyProperties(changes);
          return applied.kind === "tooLong" ? applied : { kind: "ok" };
        } });
      document.body.append(properties.element);
    }
    if (contextAvailable) {
      contextMenu = createEditorContextMenu({ target: nodes.canvas, focusTarget: editor.element,
        labels: Object.fromEntries(contextKeys.map(key => [key, messages.get(`context-${key}`)])),
        getState: () => { const current = editor.getState(); return { revision: current.revision, anchor: current.anchor, focus: current.focus,
          hasText: current.docLen > 0,
          canUndo: current.canUndo, canRedo: current.canRedo, enabled: editor.canEdit, busy }; },
        clipboardCapabilities: { read: () => typeof navigator.clipboard?.readText === "function", write: () => typeof navigator.clipboard?.writeText === "function" },
        onError: () => showEditorMessage(messages.get("utilities-clipboardError"), true),
        onCommand: async (action, snapshot) => {
          if (!editor.canEdit) throw new EditorError("EDITOR_BUSY");
          if (["copy", "cut"].includes(action)) {
            const selected = await editor.selectedText(snapshot);
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
    if (fileTab) {
      // Les récents se relisent à l'affichage de l'onglet Fichier : un fichier a pu disparaître ou revenir entre-temps.
      const fileTabButton = document.querySelector('[data-ribbon-tab-id="file"]');
      for (const type of ["click", "focus"]) fileTabButton?.addEventListener(type, () => { void fileTab.refreshRecents(); }, { signal: listeners.signal });
      void fileTab.attach({ host, openDocument: source => editor.openDocument(source) });
    }
    nodes.readonlyCopy.addEventListener("click", async () => {
      if (nodes.readonlyCopy.disabled) return;
      nodes.readonlyCopy.disabled = true;
      try {
        const result = await editor.makeEditableCopy();
        if (result.kind === "ok") showEditorMessage(messages.get("status-copy-created"));
        else showEditorMessage(messages.get("status-copy-error"), true);
      } catch (error) { onError(error); if (error?.code === "RICH_REVISIONS_UNSUPPORTED") showEditorMessage(messages.get("status-copy-unsupported"), true); }
      finally { nodes.readonlyCopy.disabled = false; }
    }, { signal: listeners.signal });

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
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-open-error"), true);
        if (result.kind === "ok") void noteOpened();
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.save.addEventListener("click", async () => {
      try {
        const result = await editor.saveAs();
        if (result.kind === "ok") { showEditorMessage(messages.get("status-saved")); void fileTab?.refreshRecents(); }
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-save-error"), true);
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
          && (event.code === "Space" || event.key === " ") && !event.isComposing && editor.canEdit) {
        event.preventDefault();
        void editor.editSelection("\u00a0", editor.getState()).catch(onError);
        return;
      }
      // Ctrl+N, O, S only: Ctrl+Maj+N is the style Normal (US-250, Word's), not « Nouveau ».
      if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.isComposing) return;
      const action = event.key.toLowerCase();
      const button = action === "n" ? nodes.new : action === "o" ? nodes.open : action === "s" ? nodes.save : null;
      if (!button) return;
      event.preventDefault();
      if (!button.disabled) button.click();
    }, { signal: listeners.signal });
    updateControls(editor.state);
    // US-115 : documents demandés par la ligne de commande ou par une seconde instance. Le premier
    // s'ouvre ; les suivants sont signalés (un seul document à la fois, pas encore d'onglets).
    let claiming = false;
    let claimAgain = false;
    async function openLaunched(launched) {
      const source = async () => ({ kind: "ok", bytes: launched.bytes, format: launched.type });
      if (launched.type === "docx") await openDocx(source);
      else {
        const result = await editor.openDocument(source);
        if (result.kind === "ok") showEditorMessage(messages.get("status-opened"));
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get("status-open-error"), true);
      }
      // Un seul message : le résultat de l'ouverture suivi du nombre de fichiers ignorés.
      void fileTab?.refreshRecents();
      if (launched.skipped > 0) {
        const failed = nodes.editorStatus.classList.contains("error");
        showEditorMessage(`${nodes.editorStatus.textContent} ${i18n.t("status-launch-skipped", { count: launched.skipped })}`, failed);
      }
    }
    async function claimLaunch() {
      if (disposed) return;
      if (claiming) { claimAgain = true; return; }
      claiming = true;
      try {
        do {
          claimAgain = false;
          for (let attempt = 0; attempt < 16 && !disposed; attempt += 1) {
            const result = await host.takeLaunch();
            if (result.kind === "ok" && !result.document) break;
            if (result.kind === "ok") { await openLaunched(result.document); break; }
            if (result.kind !== "error" || !result.key.startsWith("files.error.")) break;
            showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get("status-open-error"), true);
          }
        } while (claimAgain && !disposed);
      } catch (error) { if (!disposed) onError(error); }
      finally { claiming = false; }
    }
    const stopLaunch = host.onLaunch(() => { void claimLaunch(); });
    listeners.signal.addEventListener("abort", stopLaunch, { once: true });
    void claimLaunch();
    editor.focus();
    return Object.freeze({
      status: "ready", engine,
      getState: () => editor.getState(),
      exportDocument: () => editor.exportDocument(),
      editor, host,
      theme: shell.theme, ribbon: shell.ribbon,
      zoomControls,
      fileTab, formatController, stylesPane,
      navigation,
      documentStatus,
      symbols, about, properties, contextMenu,
      openDocx, dispose, claimLaunch,
    });
  } catch (error) {
    showPing(false, messages);
    showEditorMessage(messages.get("status-editor-error"), true);
    dispose();
    return Object.freeze({ status: "failed", code: error?.code ?? "EDITOR_INITIALIZATION_FAILED" });
  }

}

export const editorReady = start();
