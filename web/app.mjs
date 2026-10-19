// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEngineClient } from "/engine-client.mjs";
import { createEngineTransport } from "/rich-editor-client.mjs";
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
import { baseName, createFileTab, FILE_TAB_KEYS } from "/file-tab.mjs";
import { createDocumentTabs } from "/document-tabs.mjs";
import { createDialog } from "/ui-components.mjs";
import { createFormatController, createFormatKeyRouter } from "/format-controller.mjs";
import { createCharFormat } from "/format-char.mjs";
import { createParaFormat } from "/format-para.mjs";
import { createStylesPane } from "/styles-pane.mjs";

const READER_MODE_KEY = "atelier.screenReaderMode";
const byId = id => document.getElementById(id);
const nodes = {
  title: byId("status-title"), detail: byId("status-detail"), indicator: byId("status-indicator"), engine: byId("engine-state"),
  editorStatus: byId("editor-status"), documentState: byId("document-state"), canvas: byId("document-canvas"), textRepresentation: byId("document-text"),
  scrollport: byId("document-scroll"), spacer: byId("page-spacer"), tabsSlot: byId("document-tabs-slot"), stylesSlot: byId("styles-slot"),
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
/** Documents open at once: the limit of the engine (`RICH_TOO_MANY_DOCUMENTS`). */
const MAX_DOCUMENTS = 8;

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
                : error?.code === "RICH_TOO_MANY_DOCUMENTS" ? "tabs.error.too_many"
                  : error?.code === "RICH_MEMORY" ? "tabs.error.memory"
                : error?.code === "RICH_REVISIONS_UNSUPPORTED" ? "status-copy-unsupported"
                  : "status-editor-error");
  const text = messages.get(key) ?? FALLBACK.get(key) ?? FALLBACK.get("status-editor-error");
  return key === "tabs.error.too_many" ? text.replace("{max}", String(MAX_DOCUMENTS)) : text;
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
    else showEditorMessage(openFailureMessage(result), true);
  }
  // US-240 / US-243 : contrôleur de mise en forme partagé (un FormatState par changement) et modules du ruban ; l'éditeur
  // de l'onglet affiché est branché sur le contrôleur à chaque bascule (`formatController.attach` / `detach`, voir `switchTo`).
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
  let transport;
  let host;
  let wasmBytes = null;
  let lastReadTicket = null;
  // US-327 : plusieurs documents. Un éditeur par onglet sur UN transport (compteur unique de requêtes externes), un
  // identifiant de document par compteur monotone (jamais réutilisé). `editor` est celui de l'onglet actif : tout le reste
  // de l'application (ruban, zoom, barre d'état, recherche, propriétés) le lit comme avant ; les rappels des autres
  // onglets sont filtrés (docs/us-327-multidoc-protocol.md 7.9).
  let editor = null;
  let active = null;
  const tabs = [];
  let tabsBar = null;
  let docCounter = 0;
  let untitledCounter = 0;
  let opening = false;
  let switching = false;
  let closingTab = false;
  let engineLost = null;
  let windowDirty;
  let formatKeys = null;
  let readerToggle = null;
  let createReaderToggle = null;
  let readerMode = false;
  let applyingReaderMode = false;
  let disposed = false;
  let lostBanner = null;
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
  function showReadOnly(activeBanner) {
    // One status region, always in the DOM: only its text changes (a region inserted with its text is not announced everywhere).
    nodes.readonlyText.textContent = activeBanner ? messages.get("readonly-banner") : "";
    nodes.readonlyBanner.dataset.active = activeBanner ? "true" : "false";
    nodes.readonlyCopy.hidden = !activeBanner;
    if (!activeBanner) nodes.readonlyCopy.disabled = false;
  }
  function dispose() {
    if (disposed) return;
    disposed = true; listeners.abort();
    formatKeys?.dispose(); formatController.dispose(); charFormat.dispose(); paraFormat.detachEditor(); paraFormat.dispose(); stylesPane.dispose(); fileTab?.dispose(); imagePicker?.dispose(); symbols?.dispose(); about?.dispose(); properties?.dispose(); contextMenu?.dispose(); navigation?.dispose(); documentStatus?.dispose(); zoomControls?.dispose();
    for (const tab of tabs) { try { tab.editor.dispose(); } catch { /* already gone */ } }
    tabsBar?.dispose(); lostBanner?.banner.remove(); readerToggle?.dispose();
    host?.close?.(); client?.close(); shell.dispose();
  }
  let initialized = false;
  let busy = false;
  /** The active editor under another name for modules built once (search): they always talk to the document shown. */
  const activeEditor = new Proxy(Object.create(null), {
    get: (_, key) => { const value = editor?.[key]; return typeof value === "function" ? value.bind(editor) : value; },
  });
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
  /** `busy` is what the user sees: the active document is suspended (dialog, open, save) or a document is being opened. */
  function refreshBusy() {
    busy = Boolean(opening || closingTab || editor?.busy || engineLost);
    updateControls(editor?.state);
  }
  const onState = (state, metadata) => {
    nodes.documentState.textContent = messages.get(state.dirty ? "state-dirty" : "state-clean");
    nodes.documentState.classList.toggle("dirty", state.dirty);
    updateControls(state);
    navigation?.update(state,metadata);
  };
  const onError = error => {
    const kind = engineLossKind(error);
    if (kind) { markEngineLost(kind); return; }
    showEditorMessage(messageForError(error, messages), true);
  };

  // ---- onglets : état, titres, hôte agrégé --------------------------------------------------------------------------------
  const tabById = id => tabs.find(tab => tab.id === id) ?? null;
  function tabTitle(tab) {
    const name = tab.editor?.documentName;
    if (name) return name;
    tab.untitled ??= ++untitledCounter;
    return i18n.t("tabs.untitled", { number: tab.untitled });
  }
  function refreshTitle(tab) {
    if (tab.inBar) tabsBar.update(tab.id, { title: tabTitle(tab) });
  }
  /** Window "modified" state: the OR of the tabs (docs 7.9); the native side keeps one boolean, told only when it changes. */
  function aggregateDirty() {
    const any = tabs.some(tab => tab.hostDirty);
    if (any === windowDirty) return Promise.resolve({ kind: "ok" });
    windowDirty = any;
    return host.notifyDirty(any);
  }
  function onTabState(tab, state, metadata) {
    tab.state = state; tab.dirty = state.dirty;
    if (state.dirty) tab.pristine = false;
    if (tab.inBar) tabsBar.update(tab.id, { dirty: state.dirty });
    if (tab === active && !switching) onState(state, metadata);
  }
  function onTabError(tab, error) {
    if (tab.closing || disposed) return;
    if (engineLossKind(error)) { markEngineLost(engineLossKind(error)); return; }
    if (tab === active) { onError(error); return; }
    // A hidden document does not interrupt the one being edited: its error waits on its tab (a mark) until it is shown.
    tab.pendingError = error;
    if (tab.inBar) tabsBar.update(tab.id, { error: true });
  }

  // ---- onglets : DOM et éditeur d'un onglet -----------------------------------------------------------------------------------
  const PANEL_IDS = [["scroll", "document-scroll"], ["canvas", "document-canvas"], ["spacer", "page-spacer"], ["text", "document-text"]];
  /** The first tab keeps the elements of index.html; the others get their own (canvas, scrollport, spacer, text mirror). */
  function buildPanel(first) {
    if (first) return { scroll: nodes.scrollport, canvas: nodes.canvas, spacer: nodes.spacer, text: nodes.textRepresentation };
    const make = (tag, className) => { const node = document.createElement(tag); if (className) node.className = className; return node; };
    const scroll = make("div", "document-scroll");
    scroll.tabIndex = 0; scroll.hidden = true;
    scroll.setAttribute("role", "tabpanel");
    scroll.setAttribute("aria-label", messages.get("pages-label"));
    const stack = make("div", "page-stack");
    const text = make("div", "accessible-document");
    text.setAttribute("role", "region"); text.setAttribute("aria-label", messages.get("document-text-label"));
    const canvas = make("canvas");
    canvas.setAttribute("role", "img"); canvas.setAttribute("aria-label", messages.get("canvas-label"));
    const spacer = make("div");
    spacer.setAttribute("aria-hidden", "true");
    stack.append(text, canvas, spacer);
    scroll.append(stack);
    nodes.stylesSlot.before(scroll);
    return { scroll, canvas, spacer, text };
  }
  /** The shown document carries the ids of index.html (tests and modules look them up); the others have none. */
  function assignPanelIds(tab) {
    for (const other of tabs) for (const [part] of PANEL_IDS) if (other !== tab) other.dom[part].removeAttribute("id");
    for (const [part, id] of PANEL_IDS) tab.dom[part].id = id;
  }
  function createEditorFor(tab) {
    return createRichEditor({
      client, transport, docId: tab.docId, host: tab.host,
      canvas: tab.dom.canvas, scrollport: tab.dom.scroll, spacer: tab.dom.spacer,
      frenchSpacing: nodes.frenchSpacing.checked,
      confirmDiscard: messages.get("confirm-discard"), inputLabel: messages.get("input-label"), textRepresentation: tab.dom.text,
      onState: (state, metadata) => onTabState(tab, state, metadata),
      onError: error => onTabError(tab, error),
      onBusy: () => { if (tab === active && !switching) refreshBusy(); },
      onZoom: () => { if (tab === active && !switching) updateZoom(); },
      onViewport: () => { if (tab === active && !switching) updateStatus(); },
      imageAlt: number => i18n.t("image-alt-default", { number }),
      onImages: ({ inserted, failed }) => { if (tab === active && inserted > 0 && failed === 0) showEditorMessage(i18n.t("image-inserted", { count: inserted })); },
      t: (key, values) => i18n.t(key, values), version: BUILD_INFO.version, screenReaderMode: readerMode,
      onScreenReaderMode: on => {
        readerMode = on;
        try { storage?.setItem(READER_MODE_KEY, String(on)); } catch { /* kept for this session */ }
        // The mode is a preference of the application: every document follows it.
        if (applyingReaderMode) return;
        applyingReaderMode = true;
        try { for (const other of tabs) if (other !== tab) other.editor.setScreenReaderMode(on); } finally { applyingReaderMode = false; }
      },
      onReadOnly: info => { if (tab === active && !switching) showReadOnly(info.readOnly && info.hasRevisions); },
      dialogLabels: { openTitle: messages.get("dialog-open-title"), saveTitle: messages.get("dialog-save-title"), txt: messages.get("dialog-filter-txt"),
        docx: messages.get("dialog-filter-docx"), docm: messages.get("dialog-filter-docm"), dotx: messages.get("dialog-filter-dotx"), defaultName: messages.get("dialog-default-name") },
    });
  }
  function createTab({ first = false } = {}) {
    const docId = ++docCounter;
    const tab = { id: docId, docId, dom: buildPanel(first), editor: null, host: null, state: null, dirty: false, hostDirty: false,
      pristine: true, untitled: null, path: null, lost: false, closing: false, pendingError: null, inBar: false, suspended: false };
    // Each editor gets its own host: `notifyDirty` is aggregated (docs 7.9), the rest is the native host.
    tab.host = Object.freeze({ ...host, notifyDirty: dirty => { tab.hostDirty = Boolean(dirty); return aggregateDirty(); } });
    tab.editor = createEditorFor(tab);
    // A tab that is not shown yet is hidden for its editor too; the first one is shown from the start.
    if (!first) tab.editor.setVisible(false);
    tab.editor.subscribe(({ reason } = {}) => { if (reason === "document" || reason === "document-name") refreshTitle(tab); });
    return tab;
  }
  /** Releases a tab that never joined the bar (refused or cancelled opening) or that was closed. */
  async function discardTab(tab) {
    tab.closing = true;
    try {
      await Promise.race([tab.editor.closeDocument(), new Promise(resolve => setTimeout(resolve, 3000))]);
    } catch { /* the engine is unreachable: dispose() tries once more, bounded */ }
    try { tab.editor.dispose(); } catch { /* already disposed */ }
    tab.hostDirty = false;
    tab.dom.scroll.remove();
  }
  function joinTab(tab) {
    tabs.push(tab);
    tab.inBar = true;
    tabsBar.add({ id: tab.id, title: tabTitle(tab), dirty: tab.dirty, error: false });
  }

  // ---- onglets : bascule (docs 7.5, six étapes) -------------------------------------------------------------------------------
  function rebuildContextMenu() {
    if (!contextAvailable || !active) return;
    contextMenu?.dispose();
    contextMenu = createEditorContextMenu({ target: active.dom.canvas, focusTarget: editor.element,
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
          editor.rememberFragment(snapshot);
          if (action === "cut") await editor.editSelection("", snapshot);
        } else if (action === "paste") {
          const text = await navigator.clipboard.readText();
          await editor.pasteText(text, snapshot);
        } else await editor.selectionAction(action, snapshot);
        editor.focus();
      } });
    document.body.append(contextMenu.element);
  }
  function rebuildReaderToggle() {
    if (!createReaderToggle || !editor) return;
    readerToggle?.dispose();
    readerToggle = createReaderToggle({ container: nodes.options, input: editor.input, t: (key, values) => i18n.t(key, values) });
  }
  /**
   * Shows `next` and hides the active document: (1) the style preview of the document left is cancelled and the engine's
   * answer awaited, (2) the marks overlay is detached, (3) the format controller is detached, (4) the document left is
   * suspended and hidden, (5) the controller and the overlay are attached to the new one, (6) the new one is resumed.
   */
  async function switchTo(next, { focus = true } = {}) {
    if (!next || next === active || switching || disposed) return;
    switching = true;
    const previous = active;
    try {
      symbols?.close(); contextMenu?.close({ restoreFocus: false });
      if (previous) {
        try { await stylesPane.cancelPreview(); } catch { /* the preview ends with the next command at the latest */ }
        paraFormat.detachEditor();
        formatController.detach();
        navigation?.setEnabled(false);
        previous.editor.setSuspended(true); previous.suspended = true;
        previous.editor.setVisible(false);
        previous.dom.scroll.hidden = true;
      }
      active = next; editor = next.editor;
      assignPanelIds(next);
      next.dom.scroll.hidden = false;
      next.editor.setVisible(true);
      void formatController.attach(next.editor);
      paraFormat.attachEditor(next.editor, { canvas: next.dom.canvas, scrollport: next.dom.scroll, spacer: next.dom.spacer });
      if (next.suspended) { next.suspended = false; next.editor.setSuspended(false); }
      tabsBar?.setActive(next.id);
      rebuildContextMenu();
      rebuildReaderToggle();
      const info = next.editor.readOnlyInfo;
      showReadOnly(info.readOnly && info.hasRevisions);
    } finally { switching = false; }
    refreshBusy();
    if (next.editor.state) onState(next.editor.state, { documentReplaced: true });
    updateZoom();
    if (next.pendingError) {
      const error = next.pendingError;
      next.pendingError = null;
      if (next.inBar) tabsBar.update(next.id, { error: next.lost });
      onError(error);
    }
    if (previous) showEditorMessage(i18n.t("tabs.switched", { name: tabTitle(next) }));
    if (focus) next.editor.focus();
  }
  /** User-initiated change of tab (bar, Ctrl+Tab): refused while something modal or in flight holds the document. */
  function requestSwitch(tab) {
    if (!tab || tab === active) return;
    if (!initialized || opening || switching || closingTab || editor?.busy || document.querySelector("dialog:modal")) {
      if (active) tabsBar.setActive(active.id);
      return;
    }
    const keepFocusOnBar = tabsBar.element.contains(document.activeElement);
    void switchTo(tab, { focus: !keepFocusOnBar }).catch(onError);
  }

  // ---- onglets : ouvrir, créer, fermer -----------------------------------------------------------------------------------------
  /** Message of a refused or failed opening: the refusals of the engine and of the bridge have their own text. */
  function openFailureMessage(result) {
    if (result.key === "tabs.error.too_many") return i18n.t(result.key, { max: MAX_DOCUMENTS });
    if (result.key === "tabs.error.memory") return i18n.t(result.key);
    return fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-open-error");
  }
  const documentIsPristine = tab => Boolean(tab) && tab.pristine && !tab.dirty && !tab.lost;
  const canStartOperation = () => initialized && !disposed && !opening && !switching && !closingTab && !engineLost && Boolean(editor) && !editor.busy;
  /** Creates a tab, runs `run(tab)` (which opens the document in it), and shows the tab only if that succeeded. */
  async function openInNewTab(run) {
    const tab = createTab();
    let result;
    try { result = await run(tab); } catch (error) { await discardTab(tab); throw error; }
    if (result.kind !== "ok") { await discardTab(tab); return result; }
    joinTab(tab);
    await switchTo(tab);
    return { ...result, tab };
  }
  /**
   * Opens a document: in a NEW tab, except when the active document is a new one nobody touched (replaced, as in Word).
   * Resolves like `editor.openDocument`: `{ kind: "ok" | "cancelled" | "error", ... }`; refusals carry the engine's code.
   */
  async function openIntoTab(source, { formats, path = null } = {}) {
    if (!canStartOperation()) return { kind: "cancelled" };
    opening = true; refreshBusy();
    const options = formats ? { formats } : undefined;
    try {
      if (documentIsPristine(active)) {
        const target = active;
        const result = await target.editor.openDocument(source, options);
        if (result.kind === "ok") { target.pristine = false; target.path = path; refreshTitle(target); }
        return result;
      }
      if (tabs.length >= MAX_DOCUMENTS) return { kind: "error", key: "tabs.error.too_many", code: "RICH_TOO_MANY_DOCUMENTS" };
      const result = await openInNewTab(tab => tab.editor.openDocument(source, options));
      if (result.kind === "ok") { result.tab.pristine = false; result.tab.path = path; refreshTitle(result.tab); }
      return result;
    } finally { opening = false; refreshBusy(); }
  }
  /** "Nouveau" : un nouvel onglet vierge (rien à faire si le document affiché est déjà un document vierge intact). */
  async function newDocumentTab() {
    if (!canStartOperation()) return { kind: "cancelled" };
    if (documentIsPristine(active)) { editor.focus(); return { kind: "ok", state: editor.state, reused: true }; }
    if (tabs.length >= MAX_DOCUMENTS) return { kind: "error", key: "tabs.error.too_many", code: "RICH_TOO_MANY_DOCUMENTS" };
    opening = true; refreshBusy();
    try { return await openInNewTab(async tab => { await tab.editor.initialize(); return { kind: "ok" }; }); }
    finally { opening = false; refreshBusy(); }
  }
  /** Enregistrer / Ne pas enregistrer / Annuler avant de fermer un document modifié. */
  function askSaveChoice(tab) {
    return new Promise(resolve => {
      const body = document.createElement("div");
      body.className = "document-tabs-confirm";
      const text = document.createElement("p");
      text.textContent = i18n.t("tabs.close.prompt", { name: tabTitle(tab) });
      const row = document.createElement("div");
      row.className = "document-tabs-confirm-actions";
      const choices = [["save", "tabs.close.save", "primary"], ["discard", "tabs.close.discard", ""], ["cancel", "tabs.close.cancel", ""]];
      const buttons = new Map();
      for (const [choice, key, extra] of choices) {
        const button = document.createElement("button");
        button.type = "button"; button.className = `ui-button ${extra}`.trim(); button.textContent = i18n.t(key); button.dataset.choice = choice;
        buttons.set(choice, button); row.append(button);
      }
      body.append(text, row);
      const dialog = createDialog({ label: i18n.t("tabs.close.title"), content: body, closeLabel: i18n.t("tabs.close.cancel"), initialFocus: () => buttons.get("save") });
      let answered = false;
      const finish = choice => {
        if (answered) return;
        answered = true;
        dialog.close(); dialog.dispose();
        resolve(choice);
      };
      for (const [choice, button] of buttons) button.addEventListener("click", () => finish(choice));
      dialog.element.addEventListener("close", () => finish("cancel"));
      document.body.append(dialog.element);
      dialog.open({ returnFocusTo: editor?.element ?? document.activeElement });
    });
  }
  async function saveTab(tab) {
    try {
      const result = await tab.editor.saveAs();
      if (result.kind === "ok") { refreshTitle(tab); void fileTab?.refreshRecents(); return true; }
      if (result.kind === "cancelled") { showEditorMessage(messages.get("status-cancelled")); return false; }
      showEditorMessage(i18n.t("tabs.close.save-failed", { name: tabTitle(tab) }), true);
      return false;
    } catch (error) { onError(error); showEditorMessage(i18n.t("tabs.close.save-failed", { name: tabTitle(tab) }), true); return false; }
  }
  /**
   * Closes a tab: confirmation and save when it is modified, then the view goes to a neighbour (a new blank document when
   * it was the last one), then `Close` in the engine, `dispose()` and the DOM (docs 7.2). Resolves true when it closed.
   */
  async function closeTab(tab, { confirm = true } = {}) {
    if (!tab || !tabs.includes(tab) || closingTab || switching || opening || tab.closing || disposed) return false;
    closingTab = true;
    refreshBusy();
    try {
      if (confirm && tab.dirty && !engineLost) {
        if (tab !== active) await switchTo(tab, { focus: false });
        if (editor.busy) return false;
        const choice = await askSaveChoice(tab);
        if (choice === "cancel") { editor.focus(); return false; }
        if (choice === "save" && !await saveTab(tab)) { editor.focus(); return false; }
      }
      const name = tabTitle(tab);
      tab.closing = true;
      const index = tabs.indexOf(tab);
      if (tab === active) {
        let successor = tabs[index + 1] ?? tabs[index - 1] ?? null;
        if (!successor) {
          // The last document: a new blank one takes its place (the application always has an active document).
          const blank = createTab();
          try { await blank.editor.initialize(); } catch (error) { await discardTab(blank); tab.closing = false; onError(error); return false; }
          joinTab(blank);
          successor = blank;
        }
        await switchTo(successor);
      }
      tabs.splice(tabs.indexOf(tab), 1);
      tabsBar.remove(tab.id); tab.inBar = false;
      tab.hostDirty = false;
      void aggregateDirty();
      try { await tab.editor.closeDocument(); } catch { showEditorMessage(messages.get("status-editor-error"), true); }
      try { tab.editor.dispose(); } catch { /* already disposed */ }
      tab.dom.scroll.remove();
      showEditorMessage(i18n.t("tabs.closed", { name }));
      return true;
    } finally { closingTab = false; refreshBusy(); }
  }
  function reorderTabs(ids, movedId) {
    tabs.sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id));
    const moved = tabById(movedId) ?? active;
    showEditorMessage(i18n.t("tabs.moved", { name: tabTitle(moved), position: tabs.indexOf(moved) + 1, count: tabs.length }));
  }

  // ---- moteur perdu (docs 4) --------------------------------------------------------------------------------------------------
  const WORKER_ERRORS = new Set(["WorkerFatalError", "WorkerClosedError", "WorkerInitError", "WorkerSendError", "WorkerProtocolError"]);
  /** "worker" : le Worker est mort ; "trap" : le moteur a perdu ses sessions (trap WASM, instance recréée) ; null : autre erreur. */
  function engineLossKind(error) {
    for (let current = error, depth = 0; current && depth < 4; current = current.cause, depth += 1) if (WORKER_ERRORS.has(current.name)) return "worker";
    const code = error?.code;
    return code === "RICH_UNKNOWN_DOCUMENT" || code === "EDITOR_ENGINE_FAILED" ? "trap" : null;
  }
  const lostButtons = new Map();
  function buildLostBanner() {
    const banner = document.createElement("div");
    banner.id = "engine-lost-banner"; banner.className = "engine-lost-banner"; banner.hidden = true;
    banner.setAttribute("role", "alert");
    const text = document.createElement("span");
    text.className = "engine-lost-text";
    banner.append(text);
    for (const [name, key] of [["reopen", "tabs.lost.reopen"], ["copy", "tabs.lost.copy"], ["blank", "tabs.lost.blank"], ["restart", "tabs.lost.restart"]]) {
      const button = document.createElement("button");
      button.type = "button"; button.className = "ui-button"; button.textContent = i18n.t(key); button.dataset.action = name;
      lostButtons.set(name, button); banner.append(button);
    }
    nodes.readonlyBanner.after(banner);
    return { banner, text };
  }
  function markEngineLost(kind) {
    if (engineLost || disposed || !lostBanner) return;
    engineLost = { kind };
    for (const tab of tabs) {
      tab.lost = true;
      tab.editor.element.disabled = true;
      if (tab.inBar) tabsBar.update(tab.id, { error: true });
    }
    lostBanner.text.textContent = i18n.t(kind === "worker" ? "tabs.lost.worker" : "tabs.lost.message");
    for (const [name, button] of lostButtons) button.hidden = kind === "worker" ? name !== "restart" : name === "restart";
    lostBanner.banner.hidden = false;
    showEditorMessage(lostBanner.text.textContent, true);
    refreshBusy();
  }
  function tabRecovered(tab) {
    tab.lost = false;
    if (tab.inBar) tabsBar.update(tab.id, { error: false });
    if (tabs.some(other => other.lost)) return;
    engineLost = null;
    lostBanner.banner.hidden = true;
    refreshBusy();
    showEditorMessage(messages.get("tabs.lost.recovered"));
  }
  /** Rouvrir depuis le fichier : le même identifiant est rouvert par un `Open` (le moteur recrée la session). */
  async function recoverByOpening() {
    const tab = active;
    if (!tab || !engineLost || engineLost.kind !== "trap") return;
    try {
      const result = await tab.editor.openDocument(undefined);
      if (result.kind === "ok") { tab.pristine = false; refreshTitle(tab); tabRecovered(tab); void noteOpened(); }
      else if (result.kind === "error") showEditorMessage(openFailureMessage(result), true);
    } catch (error) { showEditorMessage(messageForError(error, messages), true); }
  }
  async function recoverWithBlank() {
    const tab = active;
    if (!tab || !engineLost || engineLost.kind !== "trap") return;
    try {
      const result = await tab.editor.newDocument();
      if (result.kind !== "cancelled") { tab.pristine = true; tab.path = null; tab.untitled = null; refreshTitle(tab); tabRecovered(tab); }
    } catch (error) { showEditorMessage(messageForError(error, messages), true); }
  }
  async function copyDisplayedText() {
    try {
      await navigator.clipboard.writeText(editor.windowText);
      showEditorMessage(messages.get("tabs.lost.copied"));
    } catch { showEditorMessage(messages.get("tabs.lost.copy-failed"), true); }
  }
  /** Worker mort : nouveau Worker, nouveau transport, nouveaux éditeurs ; chaque onglet repart d'un document vierge. */
  async function restartEngine() {
    if (!engineLost || engineLost.kind !== "worker") return;
    try {
      const previous = client;
      const fresh = await connectEngine();
      client = fresh.client; transport = fresh.transport;
      const current = active;
      paraFormat.detachEditor(); formatController.detach(); navigation?.setEnabled(false);
      active = null; editor = null;
      for (const tab of tabs) {
        try { tab.editor.dispose(); } catch { /* the old client is dead */ }
        tab.state = null; tab.dirty = false; tab.hostDirty = false; tab.pristine = true; tab.path = null; tab.untitled = null; tab.lost = false;
        tab.suspended = false; tab.pendingError = null;
        tab.editor = createEditorFor(tab);
        tab.editor.setVisible(false);
        tab.editor.subscribe(({ reason } = {}) => { if (reason === "document" || reason === "document-name") refreshTitle(tab); });
        await tab.editor.initialize();
        if (tab !== current) { tab.editor.setSuspended(true); tab.suspended = true; tab.dom.scroll.hidden = true; }
        if (tab.inBar) tabsBar.update(tab.id, { title: tabTitle(tab), dirty: false, error: false });
      }
      void aggregateDirty();
      try { previous?.close(); } catch { /* already closed */ }
      engineLost = null;
      lostBanner.banner.hidden = true;
      await switchTo(current ?? tabs[0]);
      refreshBusy();
      showEditorMessage(messages.get("tabs.lost.restarted"));
    } catch (error) { showEditorMessage(messageForError(error, messages), true); }
  }
  /** Worker, client du moteur et transport unique (une seule suite d'identifiants de requêtes externes). */
  async function connectEngine() {
    if (!wasmBytes) {
      const wasmResponse = await fetch("/atelier_core_wasm.wasm", { cache: "no-store" });
      if (!wasmResponse.ok) throw new Error("WASM_UNAVAILABLE");
      wasmBytes = await wasmResponse.arrayBuffer();
    }
    const worker = new Worker("/engine-worker.mjs", { type: "module", name: "atelier-engine" });
    const connected = await createEngineClient({ worker, bytes: wasmBytes.slice(0) });
    const ping = encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: new Uint8Array() });
    const reply = decodeMessage(await connected.request(ping), "command");
    if (reply.type !== MessageType.Ping || reply.requestId !== 1n || reply.payload.byteLength !== 0) throw new Error("PING_ECHO_FAILED");
    return { client: connected, transport: createEngineTransport(connected) };
  }

  try {
    const connected = await connectEngine();
    client = connected.client; transport = connected.transport;
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
    try { readerMode = storage?.getItem(READER_MODE_KEY) === "true"; } catch { /* default: off */ }
    tabsBar = createDocumentTabs({ document, container: nodes.tabsSlot, t: (key, values) => i18n.t(key, values),
      onActivate: id => requestSwitch(tabById(id)),
      onClose: id => { void closeTab(tabById(id)).catch(onError); },
      onReorder: (ids, movedId) => reorderTabs(ids, movedId) });
    lostBanner = buildLostBanner();
    lostButtons.get("reopen").addEventListener("click", () => { void recoverByOpening(); }, { signal: listeners.signal });
    lostButtons.get("blank").addEventListener("click", () => { void recoverWithBlank(); }, { signal: listeners.signal });
    lostButtons.get("copy").addEventListener("click", () => { void copyDisplayedText(); }, { signal: listeners.signal });
    lostButtons.get("restart").addEventListener("click", () => { void restartEngine(); }, { signal: listeners.signal });
    const first = createTab({ first: true });
    first.dom.scroll.hidden = false;
    joinTab(first);
    editor = first.editor;
    // Raccourcis des modules de mise en forme : chaque module les déclare dans keyBindings() (voir format-controller.mjs).
    // La cible est l'éditeur de l'onglet affiché.
    formatKeys = createFormatKeyRouter({ document, target: () => editor?.element, modules: [charFormat, paraFormat, stylesPane], isEnabled: () => Boolean(editor?.canEdit) });
    ({ createReaderModeToggle: createReaderToggle } = await import("/document-mirror.mjs"));
    // What the rich editor offers is enabled like in v1; a command it lacks stays greyed with its reason.
    shell.setUnavailable?.(editor.capabilities.search ? null : messages.get("v2-unavailable-search"),
      editor.capabilities.properties ? null : messages.get("v2-unavailable-properties"));
    if (toolsAvailable && editor.capabilities.search) navigation = createNavigationTools({ document, editor: activeEditor, paneSlot: byId("search-slot"), statusSlot: byId("statistics-slot"), i18n,
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
      const result = await openIntoTab(source, { formats: ["docx", "docm", "dotx", "txt"] });
      if (result.kind === "ok") showEditorMessage(messages.get(editor.state?.readOnly ? "status-readonly-opened" : "status-opened"));
      else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
      else showEditorMessage(openFailureMessage(result), true);
      if (result.kind === "ok") void noteOpened();
      return result;
    }
    const openDocx = source => openDocxInEditor(source);
    const state = await editor.initialize();
    initialized = true;
    await switchTo(first, { focus: false });
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
    onState(state);
    showEditorMessage(messages.get("status-ready"));
    nodes.openDocx.addEventListener("click", () => { void openDocx(); }, { signal: listeners.signal });
    if (fileTab) {
      // Les récents se relisent à l'affichage de l'onglet Fichier : un fichier a pu disparaître ou revenir entre-temps.
      const fileTabButton = document.querySelector('[data-ribbon-tab-id="file"]');
      for (const type of ["click", "focus"]) fileTabButton?.addEventListener(type, () => { void fileTab.refreshRecents(); }, { signal: listeners.signal });
      // Récents et modèles s'ouvrent dans un nouvel onglet (ou dans le document vierge intact affiché).
      void fileTab.attach({ host, openDocument: source => openIntoTab(source) });
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
        const result = await newDocumentTab();
        if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else if (result.kind === "error") showEditorMessage(openFailureMessage(result), true);
        else showEditorMessage(messages.get("status-new"));
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.open.addEventListener("click", async () => {
      try {
        const result = await openIntoTab(undefined);
        if (result.kind === "ok") showEditorMessage(messages.get("status-opened"));
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(openFailureMessage(result), true);
        if (result.kind === "ok") void noteOpened();
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.save.addEventListener("click", async () => {
      try {
        const saving = active;
        const result = await editor.saveAs();
        if (result.kind === "ok") { showEditorMessage(messages.get("status-saved")); refreshTitle(saving); void fileTab?.refreshRecents(); }
        else if (result.kind === "cancelled") showEditorMessage(messages.get("status-cancelled"));
        else showEditorMessage(fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get(result.key === "HOST_UNAVAILABLE" ? "status-host-unavailable" : "status-save-error"), true);
      } catch (error) { onError(error); }
    }, { signal: listeners.signal });
    nodes.undo.addEventListener("click", () => { void editor.undo().catch(onError); }, { signal: listeners.signal });
    nodes.redo.addEventListener("click", () => { void editor.redo().catch(onError); }, { signal: listeners.signal });
    nodes.frenchSpacing.addEventListener("change", () => {
      for (const tab of tabs) tab.editor.setFrenchSpacing(nodes.frenchSpacing.checked);
      try { localStorage.setItem("atelier.frenchSpacing", String(nodes.frenchSpacing.checked)); }
      catch { /* Keep the chosen behavior in memory if storage is unavailable. */ }
      editor.focus();
    }, { signal: listeners.signal });
    document.addEventListener("keydown", event => {
      // Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+W, Ctrl+F4 : onglets de documents (document-tabs.mjs), depuis n'importe où dans la fenêtre.
      if (initialized && tabsBar.handleKeydown(event)) return;
      if (document.querySelector("dialog:modal")) return;
      if (symbolsAvailable && event.target === editor.element && event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey
          && (event.code === "Space" || event.key === " ") && !event.isComposing && editor.canEdit) {
        event.preventDefault();
        void editor.editSelection(" ", editor.getState()).catch(onError);
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
    // US-115 : documents demandés par la ligne de commande ou par une seconde instance. Chacun s'ouvre dans son onglet,
    // l'un après l'autre (le moteur est monothread : jamais deux ouvertures en parallèle, la suivante attend la fin de la
    // précédente et la mise en page de son premier écran). Un fichier déjà ouvert par ce moyen est simplement affiché.
    let claiming = false;
    let claimAgain = false;
    const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
    /** Waits until nothing holds the document (dialog, save, other opening); the launched file is not lost meanwhile. */
    async function untilIdle() {
      for (let waited = 0; waited < 120000 && !disposed; waited += 50) {
        if (initialized && !opening && !switching && !closingTab && !editor?.busy && !document.querySelector("dialog:modal")) return true;
        await sleep(50);
      }
      return false;
    }
    async function openLaunched(launched, batch) {
      const name = launched.path ? baseName(launched.path) : null;
      const identity = launched.path ? launched.path.toLowerCase() : null;
      if (!await untilIdle()) { batch.refused.push({ name, reason: messages.get("files.error.queue_full") }); return; }
      const existing = identity ? tabs.find(tab => tab.path === identity) : null;
      if (existing) { await switchTo(existing); batch.already.push(name); return; }
      if (tabs.length >= MAX_DOCUMENTS && !documentIsPristine(active)) { batch.skipped += 1; return; }
      const source = async () => ({ kind: "ok", bytes: launched.bytes, format: launched.type, name: name ?? undefined });
      const result = await openIntoTab(source, { path: identity });
      if (result.kind === "ok") {
        batch.opened += 1;
        await editor.whenFirstScreen();
      } else if (result.code === "RICH_TOO_MANY_DOCUMENTS") batch.skipped += 1;
      else batch.refused.push({ name, reason: result.kind === "cancelled" ? messages.get("status-cancelled") : openFailureMessage(result) });
    }
    function reportLaunch(batch) {
      const parts = [];
      if (batch.opened === 1 && batch.refused.length === 0 && batch.already.length === 0 && batch.skipped === 0) {
        parts.push(messages.get(editor.state?.readOnly ? "status-readonly-opened" : "status-opened"));
      } else if (batch.opened > 0) parts.push(i18n.t("tabs.launch.opened", { count: batch.opened }));
      for (const name of batch.already) parts.push(i18n.t("tabs.launch.already", { name: name ?? "" }));
      for (const refused of batch.refused) parts.push(i18n.t("tabs.launch.refused", { name: refused.name ?? "", reason: refused.reason }));
      if (batch.skipped > 0) parts.push(i18n.t("tabs.launch.skipped", { count: batch.skipped }));
      if (parts.length > 0) showEditorMessage(parts.join(" "), batch.refused.length > 0 || batch.skipped > 0);
    }
    async function claimLaunch() {
      if (disposed) return;
      if (claiming) { claimAgain = true; return; }
      claiming = true;
      try {
        do {
          claimAgain = false;
          const batch = { opened: 0, already: [], refused: [], skipped: 0 };
          for (let attempt = 0; attempt < 40 && !disposed; attempt += 1) {
            const result = await host.takeLaunch();
            if (result.kind === "ok" && !result.document) break;
            if (result.kind === "ok") { await openLaunched(result.document, batch); continue; }
            if (result.kind !== "error" || !result.key.startsWith("files.error.")) break;
            batch.refused.push({ name: result.path ? baseName(result.path) : null, reason: fileErrorMessage(result.key, i18n, messages, fileLimit) ?? messages.get("status-open-error") });
          }
          void fileTab?.refreshRecents();
          reportLaunch(batch);
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
      get editor() { return editor; },
      host,
      get client() { return client; },
      get transport() { return transport; },
      theme: shell.theme, ribbon: shell.ribbon,
      zoomControls,
      fileTab, formatController, stylesPane,
      get navigation() { return navigation; },
      documentStatus,
      symbols, about, properties,
      get contextMenu() { return contextMenu; },
      /** Onglets de documents (US-327) : lecture seule pour l'interface et les tests ; les opérations passent par les commandes. */
      documentTabs: Object.freeze({
        get list() { return tabs.map(tab => ({ id: tab.id, docId: tab.docId, title: tabTitle(tab), dirty: tab.dirty, editor: tab.editor, path: tab.path, lost: tab.lost })); },
        get active() { return active?.id ?? null; },
        get bar() { return tabsBar; },
        get windowDirty() { return windowDirty; },
        get max() { return MAX_DOCUMENTS; },
        get flags() { return { opening, switching, closingTab, engineLost: Boolean(engineLost), initialized, busy }; },
        activate: id => switchTo(tabById(id)),
        close: (id, options) => closeTab(tabById(id), options),
        newDocument: newDocumentTab,
        open: openIntoTab,
        get lost() { return engineLost; },
      }),
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
