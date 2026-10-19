// SPDX-License-Identifier: AGPL-3.0-or-later
// Styles (US-250) : la galerie « Styles » de l'onglet Accueil (aperçu en direct au survol et au focus clavier), le volet Styles
// (liste « utilisés / tous », aperçu, style courant) et les raccourcis Titre 1 à 3. Écrit ICI, dans styles-pane.css, dans
// tests/rich-editor-format-styles*.mjs, rich_editor/styles*.rs et le bloc « # US-250 » de web/i18n/fr-FR.txt. Contrat :
// docs/us-240-format-protocol.md, sections 5.4 à 5.6 et 8.3 ; le moteur est rich_editor/styles.rs.
//
// Entrée : createStylesPane({ document, i18n, controller, announce, slot? })
//   document   : le document DOM ; i18n : `i18n.t(key, values)` ; controller : createFormatController (un FormatState par changement,
//                `controller.run(fn)` exécute une commande, relit l'état et rend le focus au document ; `controller.client` pour les
//                commandes qui ne changent rien : liste des styles, aperçu) ; announce(texte, isError) ;
//                slot : l'élément qui accueille le volet (défaut : #styles-slot).
// Sortie : { groups, setBlocked, keyBindings, open, close, toggle, pane, gallery, dispose }
//   groups({ command }) -> le groupe « Styles » : la galerie (info-touche A) et le bouton du volet (info-touche I). V reste libre.
//   keyBindings() -> Ctrl+Alt+1, 2, 3 (Titre 1 à 3 de Word, voir ci-dessous), Ctrl+Maj+1, 2, 3 (les mêmes sur toute disposition) et Ctrl+Maj+N (style Normal).
//
// Aperçu en direct : le survol (ou l'élément actif du clavier) d'une tuile envoie `previewStyle` après 150 ms ; la fin du survol, le
// départ du focus, Échap, la fermeture du volet, le clic, le changement de document ou la perte de fenêtre envoient l'annulation
// (idempotente côté moteur). Le moteur ne touche ni le document, ni l'historique, ni la révision (voir styles.rs) : annuler est donc
// toujours sûr. L'annulation est aussi envoyée après TOUTE erreur d'un aperçu ou d'une application : une réponse d'aperçu perdue laisse
// l'aperçu actif dans le moteur alors que la souris est partie ; elle est reprise deux fois si le transport échoue. Une frappe ou un
// Ctrl+Z pendant l'aperçu l'annule côté moteur (`cancels_preview`) : le panneau n'a rien à faire.
//
// Raccourcis Titre 1 à 3 : Word utilise Ctrl+Alt+1, 2, 3. Sous Windows, Ctrl+Alt est AltGr : sur un clavier AZERTY, AltGr+2 est « ~ »
// et AltGr+3 est « # ». Le routeur ignore donc Ctrl+Alt, et un raccourci pris à l'appui volerait ces caractères. Les trois raccourcis
// sont déclarés `ifNoCharacter` : ils sont reconnus par `code` (Digit1 à Digit3, la position physique, comme Word) mais ne s'exécutent
// qu'au relâchement de la touche, et seulement si l'appui n'a produit AUCUN caractère (ni `beforeinput`, ni `keypress`, ni composition,
// voir format-controller.mjs). Sur AZERTY, Ctrl+Alt+é (« ~ ») et Ctrl+Alt+" (« # ») tapent donc leur caractère ; Ctrl+Alt+& (aucun
// caractère sous AltGr) applique Titre 1 ; sur QWERTY les trois appliquent leur titre. Titre 2 et 3 n'ont donc pas de Ctrl+Alt sur
// AZERTY : Ctrl+Maj+1, 2 et 3 (par `code`, sans Alt, donc sans caractère possible) les donnent sur toutes les dispositions, et
// Ctrl+Maj+N applique le style Normal (Word).

import { canFormat } from "/format-controller.mjs";

const PREVIEW_DELAY = 150;
const PREVIEW_SWITCH_DELAY = 40;
const FILTER_KEY = "atelier.stylesFilter";
const PANE_KEY = "atelier.stylesPane";

/** Noms des styles de Word (`w:name`, en minuscules) -> clé du catalogue : affichés traduits quand le style est intégré. */
const BUILTIN_KEYS = Object.freeze({
  "normal": "styles-builtin-normal",
  "heading 1": "styles-builtin-heading-1", "heading 2": "styles-builtin-heading-2", "heading 3": "styles-builtin-heading-3",
  "heading 4": "styles-builtin-heading-4", "heading 5": "styles-builtin-heading-5", "heading 6": "styles-builtin-heading-6",
  "heading 7": "styles-builtin-heading-7", "heading 8": "styles-builtin-heading-8", "heading 9": "styles-builtin-heading-9",
  "title": "styles-builtin-title", "subtitle": "styles-builtin-subtitle", "quote": "styles-builtin-quote",
  "intense quote": "styles-builtin-intense-quote", "emphasis": "styles-builtin-emphasis", "strong": "styles-builtin-strong",
  "subtle emphasis": "styles-builtin-subtle-emphasis", "intense emphasis": "styles-builtin-intense-emphasis",
  "caption": "styles-builtin-caption", "header": "styles-builtin-header", "footer": "styles-builtin-footer",
  "hyperlink": "styles-builtin-hyperlink", "list paragraph": "styles-builtin-list-paragraph", "no spacing": "styles-builtin-no-spacing",
  "body text": "styles-builtin-body-text",
});

const refused = new Set(["RICH_READ_ONLY", "RICH_FORMAT_PLAIN_TEXT", "RICH_FORMAT_NOT_IMPLEMENTED", "RICH_STALE_REVISION", "RICH_LIMIT", "RICH_UNKNOWN_DOCUMENT", "RICH_UNKNOWN_STYLE"]);

const make = (document, name, className, text) => {
  const node = document.createElement(name);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let sequence = 0;

export function createStylesPane({ document, i18n, controller, announce = () => {}, slot = null } = {}) {
  let blocked = false; let disposed = false;
  let generation = 0; let sawState = false;
  let list = null;            // { generation, styles, failed }
  let listSeq = 0; let loadingList = false;
  let current = { paragraph: null, character: null };
  let paneOpen = false; let paneBuilt = null;
  const stops = [];

  // ---- names and tiles -------------------------------------------------------------------------------------------------------
  /** Name to show: translated for a built-in style, else the name of the document, else the identifier. */
  function nameOf(style) {
    const raw = style.name || style.id;
    const key = style.builtIn ? BUILTIN_KEYS[raw.toLowerCase()] : undefined;
    return key ? i18n.t(key) : raw;
  }
  function paintSample(node, style) {
    const sample = style.sample;
    // i18n-ignore: famille de polices CSS, pas du texte d'interface
    node.style.fontFamily = sample.font ? `${JSON.stringify(sample.font)}, Carlito, "Segoe UI", sans-serif` : "";
    node.style.fontSize = `${Math.min(Math.max((sample.halfPoints || 22) / 2, 11), 20)}px`;
    node.style.fontWeight = sample.bold ? "700" : "400";
    node.style.fontStyle = sample.italic ? "italic" : "normal";
    node.style.textDecoration = [sample.underline !== "none" ? "underline" : "", sample.strike ? "line-through" : ""].filter(Boolean).join(" ") || "none";
    node.style.color = sample.color ? `rgb(${sample.color.r}, ${sample.color.g}, ${sample.color.b})` : "";
  }

  // ---- state -> what is current ----------------------------------------------------------------------------------------------
  function currentOf(snapshot) {
    const state = snapshot.state;
    if (!state) return { paragraph: null, character: null };
    const p = state.paragraph.style;
    const paragraph = p.state === "value" ? p.value : p.state === "unset" ? (list?.styles.find(s => s.target === "paragraph" && s.isDefault)?.id ?? null) : null;
    const c = state.chars.style;
    return { paragraph, character: c.state === "value" ? c.value : null };
  }
  const isCurrent = style => (style.target === "character" ? current.character : current.paragraph) === style.id;

  // ---- live preview ------------------------------------------------------------------------------------------------------------
  const preview = { timer: 0, epoch: 0, active: false, shown: null };
  const canAct = () => !disposed && !blocked && canFormat(controller.snapshot());
  function request(style) {
    if (!style) { endPreview(); return; }
    if (!canAct()) return;
    // The style the selection already has looks like itself: nothing to show, and a preview of another one ends.
    if (isCurrent(style)) { endPreview(); return; }
    if (preview.active && preview.shown?.id === style.id && preview.shown?.target === style.target) return;
    clearTimeout(preview.timer);
    const mine = ++preview.epoch;
    preview.timer = setTimeout(() => { void sendPreview(mine, style); }, preview.active ? PREVIEW_SWITCH_DELAY : PREVIEW_DELAY);
  }
  async function sendPreview(mine, style) {
    preview.timer = 0;
    const client = controller.client;
    if (mine !== preview.epoch || !client || !canAct()) return;
    // Optimistic: whatever the answer, the engine may hold a preview from now on.
    preview.active = true; preview.shown = { id: style.id, target: style.target };
    try {
      await client.previewStyle({ target: style.target, id: style.id });
    } catch (error) {
      if (error?.code === "CLIENT_CLOSED") { preview.active = false; preview.shown = null; return; }
      if (error?.code === "RICH_UNKNOWN_STYLE") list = null;
      if (mine === preview.epoch) preview.epoch += 1;
      await cancelNow();
      return;
    }
    if (mine !== preview.epoch) await cancelNow(); // the hover ended while the command was in flight
  }
  /** Ends the preview: cancels at once the one shown and the one that is about to be. */
  function endPreview() {
    clearTimeout(preview.timer); preview.timer = 0; preview.epoch += 1;
    if (preview.active) void cancelNow();
  }
  async function cancelNow(attempt = 0) {
    preview.active = false; preview.shown = null;
    const client = controller.client;
    if (!client) return;
    try {
      await client.cancelPreview({ target: "paragraph" });
    } catch (error) {
      if (error?.code === "CLIENT_CLOSED" || disposed) return;
      if (typeof error?.code === "string" && error.code.startsWith("RICH_")) return; // a refusal: nothing to cancel
      if (attempt < 2) { await sleep(150); if (!preview.active && !disposed) await cancelNow(attempt + 1); }
    }
  }
  /** US-327 : annule l'aperçu et ATTEND la réponse du moteur (changement d'onglet : l'aperçu ne doit pas rester affiché dans le document quitté). */
  async function cancelPreviewNow() {
    clearTimeout(preview.timer); preview.timer = 0; preview.epoch += 1;
    if (preview.active) await cancelNow();
  }
  /** Before an application: the engine ends the preview itself (`cancels_preview`), nothing to send. */
  function forgetPreview() { clearTimeout(preview.timer); preview.timer = 0; preview.epoch += 1; preview.active = false; preview.shown = null; }

  // ---- listbox (shared by the gallery and the pane) ----------------------------------------------------------------------------
  function createListbox({ className, orientation, label, tileClass, onApply, onActive }) {
    const root = make(document, "div", className);
    root.setAttribute("role", "listbox"); root.setAttribute("aria-orientation", orientation); root.setAttribute("aria-label", label);
    root.tabIndex = 0;
    const id = `styles-listbox-${++sequence}`;
    let items = []; let active = -1;
    const rebuild = styles => {
      root.replaceChildren();
      items = styles.map((style, index) => {
        const node = make(document, "div", tileClass);
        node.id = `${id}-${index}`; node.setAttribute("role", "option");
        node.dataset.styleId = style.id; node.dataset.target = style.target; node.dataset.index = String(index);
        const text = make(document, "span", "styles-tile-name", nameOf(style));
        paintSample(text, style);
        node.append(text);
        node.title = `${nameOf(style)} (${i18n.t(style.target === "character" ? "styles-kind-character" : "styles-kind-paragraph")})`;
        root.append(node);
        return { style, node };
      });
      active = Math.min(active, items.length - 1);
      refreshMarks();
    };
    const refreshMarks = () => {
      items.forEach(({ style, node }, index) => {
        const here = isCurrent(style);
        node.setAttribute("aria-selected", String(here));
        node.classList.toggle("is-current", here);
        node.classList.toggle("is-active", index === active && root.contains(document.activeElement));
      });
      if (active >= 0 && items[active]) root.setAttribute("aria-activedescendant", items[active].node.id); else root.removeAttribute("aria-activedescendant");
    };
    function setActive(index, { notify = true } = {}) {
      if (!items.length) { active = -1; refreshMarks(); return; }
      active = Math.max(0, Math.min(items.length - 1, index));
      refreshMarks();
      items[active].node.scrollIntoView?.({ block: "nearest", inline: "nearest" });
      if (notify) onActive?.(items[active].style);
    }
    const optionOf = event => {
      const node = event.target instanceof Element ? event.target.closest('[role="option"]') : null;
      return node && root.contains(node) ? items[Number(node.dataset.index)] : null;
    };
    root.addEventListener("click", event => {
      const item = optionOf(event);
      if (!item || root.getAttribute("aria-disabled") === "true") return; // a click on the list itself (key tip): nothing
      active = items.indexOf(item); refreshMarks();
      onApply(item.style);
    });
    root.addEventListener("pointerover", event => {
      const item = optionOf(event);
      if (item && root.getAttribute("aria-disabled") !== "true") { onActive?.(item.style); request(item.style); }
    });
    root.addEventListener("pointerleave", () => {
      // The pointer is gone: back to what the keyboard has on its active tile, if the list has the focus.
      if (root.contains(document.activeElement) && items[active]) request(items[active].style); else endPreview();
      onActive?.(items[active]?.style ?? null);
    });
    root.addEventListener("focus", () => {
      if (root.getAttribute("aria-disabled") === "true") return;
      // The keyboard starts from the current style (the first tile when it is not in the list).
      const here = items.findIndex(({ style }) => isCurrent(style) && style.target === "paragraph");
      active = here >= 0 ? here : 0;
      refreshMarks();
      if (items[active]) { onActive?.(items[active].style); request(items[active].style); }
    });
    root.addEventListener("blur", () => { refreshMarks(); endPreview(); });
    root.addEventListener("keydown", event => {
      if (root.getAttribute("aria-disabled") === "true" || event.altKey || event.ctrlKey || event.metaKey) return;
      const forward = orientation === "horizontal" ? "ArrowRight" : "ArrowDown";
      const backward = orientation === "horizontal" ? "ArrowLeft" : "ArrowUp";
      let handled = true;
      if (event.key === forward) setActive(active + 1);
      else if (event.key === backward) setActive(active - 1);
      else if (event.key === "Home") setActive(0);
      else if (event.key === "End") setActive(items.length - 1);
      else if ((event.key === "Enter" || event.key === " ") && items[active]) onApply(items[active].style);
      else if (event.key === "Escape") { endPreview(); void controller.run(async () => {}); }
      else handled = false;
      if (handled) { event.preventDefault(); event.stopPropagation(); if (event.key !== "Escape" && items[active]) request(items[active].style); }
    });
    return {
      root, rebuild, refreshMarks, setActive,
      get items() { return items; },
      set disabled(value) { root.setAttribute("aria-disabled", String(Boolean(value))); root.tabIndex = value ? -1 : 0; },
    };
  }

  // ---- applying ---------------------------------------------------------------------------------------------------------------
  async function perform(style) {
    if (!canAct()) return;
    forgetPreview();
    try {
      await controller.run(client => client.applyStyle({ target: style.target, id: style.id }));
      announce(i18n.t("styles-status-applied", { name: nameOf(style) }), false);
    } catch (error) {
      if (error?.code === "CLIENT_CLOSED") return;
      void cancelNow(); // after any error, whatever the engine holds
      if (error?.code === "RICH_UNKNOWN_STYLE") { list = null; void ensureList(); }
      if (error?.outcome === "applied") { announce(i18n.t("styles-status-applied", { name: nameOf(style) }), false); return; }
      announce(i18n.t(refused.has(error?.code) || error?.outcome === "not-applied" ? "styles-error-refused" : "styles-error-unknown"), true);
    }
  }
  async function clearCharacterStyle() {
    if (!canAct()) return;
    forgetPreview();
    try {
      await controller.run(client => client.applyStyle({ target: "character", id: "" }));
      announce(i18n.t("styles-status-cleared"), false);
    } catch (error) {
      if (error?.code === "CLIENT_CLOSED") return;
      void cancelNow();
      announce(i18n.t(refused.has(error?.code) || error?.outcome === "not-applied" ? "styles-error-refused" : "styles-error-unknown"), true);
    }
  }

  // ---- the list ---------------------------------------------------------------------------------------------------------------
  let listPromise = null;
  /** The list of every style, read once per document (the catalog does not change in this story); a failure is kept until `list` is dropped. */
  function ensureList() {
    if (list && list.generation === generation && !list.failed) return Promise.resolve(list);
    if (listPromise) return listPromise;
    listPromise = loadList().finally(() => { listPromise = null; });
    return listPromise;
  }
  async function loadList() {
    const client = controller.client;
    if (!client || !controller.snapshot().state) return null;
    const mine = generation; const seq = ++listSeq; loadingList = true;
    try {
      const { styles } = await client.listStyles({ filter: "all", usedOnly: false });
      if (disposed || mine !== generation || seq !== listSeq) return null;
      list = { generation: mine, styles, failed: false };
    } catch (error) {
      if (disposed || mine !== generation || error?.code === "CLIENT_CLOSED") return null;
      list = { generation: mine, styles: [], failed: true };
      announce(i18n.t("styles-error-list"), true);
    } finally { loadingList = false; }
    renderGallery(); updateCurrent();
    return list;
  }

  // ---- gallery ----------------------------------------------------------------------------------------------------------------
  const gallery = make(document, "div", "styles-gallery");
  const galleryBox = createListbox({
    className: "styles-gallery-list", orientation: "horizontal", label: i18n.t("styles-gallery"), tileClass: "styles-tile",
    onApply: style => { void perform(style); }, onActive: () => {},
  });
  const scrollButtons = make(document, "div", "styles-gallery-scroll");
  const scrollButton = (label, glyph, direction) => {
    const button = make(document, "button", "styles-scroll-button", glyph);
    button.type = "button"; button.tabIndex = -1; button.setAttribute("aria-label", label); button.title = label;
    button.addEventListener("click", () => { galleryBox.root.scrollBy?.({ left: direction * Math.max(120, galleryBox.root.clientWidth * 0.8), behavior: "smooth" }); });
    return button;
  };
  const previousButton = scrollButton(i18n.t("styles-gallery-previous"), "‹", -1);
  const nextButton = scrollButton(i18n.t("styles-gallery-next"), "›", 1);
  scrollButtons.append(previousButton, nextButton);
  // The list comes first: the key tip of the gallery focuses the first focusable element of its command.
  gallery.append(galleryBox.root, scrollButtons);
  const emptyNote = make(document, "span", "styles-gallery-empty", i18n.t("styles-unavailable")); emptyNote.hidden = true;
  gallery.append(emptyNote);
  gallery.setAttribute("role", "group"); gallery.setAttribute("aria-label", i18n.t("styles-gallery"));

  function renderGallery() {
    const quick = (list?.styles ?? []).filter(style => style.quick);
    galleryBox.rebuild(quick);
    emptyNote.hidden = quick.length > 0 || !list;
  }
  function updateCurrent() {
    galleryBox.refreshMarks();
    paneBuilt?.refreshMarks();
    const here = galleryBox.items.find(({ style }) => style.target === "paragraph" && isCurrent(style));
    here?.node.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    paneBuilt?.showCurrent();
  }

  // ---- the pane -----------------------------------------------------------------------------------------------------------------
  let paneTimer = 0; let paneSeq = 0; let paneKey = ""; let paneFilter = "used";
  try { paneFilter = globalThis.localStorage?.getItem(FILTER_KEY) === "all" ? "all" : "used"; } catch { /* the preference store can be blocked */ }
  function buildPane() {
    const host = slot ?? document.getElementById("styles-slot");
    if (!host) return null;
    const root = make(document, "aside", "styles-pane");
    root.setAttribute("aria-label", i18n.t("styles-pane-title"));
    const header = make(document, "div", "styles-pane-header");
    const title = make(document, "h2", "styles-pane-title", i18n.t("styles-pane-title"));
    const close = make(document, "button", "styles-pane-close", "✕");
    close.type = "button"; close.setAttribute("aria-label", i18n.t("styles-pane-close")); close.title = i18n.t("styles-pane-close");
    close.addEventListener("click", () => closePane());
    header.append(title, close);
    const filterLabel = make(document, "label", "styles-pane-filter", i18n.t("styles-filter-label"));
    const filter = make(document, "select", "styles-pane-filter-select");
    for (const [value, label] of [["used", i18n.t("styles-filter-used")], ["all", i18n.t("styles-filter-all")]]) { const option = make(document, "option", "", label); option.value = value; filter.append(option); }
    filter.value = paneFilter;
    filterLabel.append(filter);
    const previewBox = make(document, "section", "styles-pane-preview");
    previewBox.setAttribute("aria-label", i18n.t("styles-preview-title"));
    const sample = make(document, "p", "styles-pane-sample", i18n.t("styles-preview-sample"));
    const sampleName = make(document, "p", "styles-pane-sample-name");
    previewBox.append(sample, sampleName);
    const currentLine = make(document, "p", "styles-pane-current");
    const empty = make(document, "p", "styles-pane-empty", i18n.t("styles-empty")); empty.hidden = true;
    const clear = make(document, "button", "styles-pane-clear", i18n.t("styles-clear-char"));
    clear.type = "button";
    clear.addEventListener("click", () => { void clearCharacterStyle(); });
    let shown = [];
    let hovered = null;
    const box = createListbox({
      className: "styles-pane-list", orientation: "vertical", label: i18n.t("styles-list-label"), tileClass: "styles-row",
      onApply: style => { void perform(style); },
      onActive: style => { hovered = style; showPreview(); },
    });
    function showPreview() {
      const style = hovered ?? shown.find(isCurrent) ?? null;
      if (!style) { sample.style.cssText = ""; sampleName.textContent = ""; return; }
      paintSample(sample, style);
      sampleName.textContent = `${nameOf(style)} · ${i18n.t(style.target === "character" ? "styles-kind-character" : "styles-kind-paragraph")}`;
    }
    function showCurrent() {
      const byId = (id, target) => (list?.styles ?? []).find(s => s.id === id && s.target === target);
      const paragraph = byId(current.paragraph, "paragraph");
      const character = byId(current.character, "character");
      const parts = [paragraph ? i18n.t("styles-current", { name: nameOf(paragraph) }) : i18n.t("styles-current-none")];
      if (character) parts.push(i18n.t("styles-current-char", { name: nameOf(character) }));
      currentLine.textContent = parts.join(" ");
      clear.disabled = !character || !canAct();
      showPreview();
    }
    function refreshMarks() { box.refreshMarks(); }
    filter.addEventListener("change", () => {
      paneFilter = filter.value === "all" ? "all" : "used";
      try { globalThis.localStorage?.setItem(FILTER_KEY, paneFilter); } catch { /* ignored */ }
      paneKey = ""; void loadPane();
    });
    root.addEventListener("keydown", event => { if (event.key === "Escape" && event.target !== box.root) { closePane(); event.preventDefault(); } });
    root.append(header, filterLabel, box.root, empty, previewBox, currentLine, clear);
    host.append(root);
    return {
      root, box, filter, showCurrent, refreshMarks,
      setStyles(styles) { shown = styles; box.rebuild(styles); empty.hidden = styles.length > 0; showCurrent(); },
      setDisabled(value) { box.disabled = value; filter.disabled = false; clear.disabled = value || !current.character; },
      dispose() { root.remove(); },
    };
  }
  async function loadPane() {
    if (!paneOpen || disposed) return;
    const client = controller.client;
    if (!client || !controller.snapshot().state) return;
    const mine = generation; const seq = ++paneSeq;
    try {
      const { styles } = await client.listStyles({ filter: "all", usedOnly: paneFilter === "used" });
      if (disposed || !paneOpen || mine !== generation || seq !== paneSeq) return;
      paneBuilt?.setStyles(styles);
    } catch (error) {
      if (disposed || error?.code === "CLIENT_CLOSED" || mine !== generation) return;
      paneBuilt?.setStyles([]);
      announce(i18n.t("styles-error-list"), true);
    }
  }
  function schedulePane() {
    if (!paneOpen || disposed) return;
    const snapshot = controller.snapshot();
    const key = `${generation}|${snapshot.state?.revision ?? "-"}`;
    if (key === paneKey) return;
    paneKey = key;
    clearTimeout(paneTimer);
    paneTimer = setTimeout(() => { void loadPane(); }, 120);
  }
  function openPane() {
    if (disposed) return false;
    paneBuilt ??= buildPane();
    if (!paneBuilt) return false;
    const host = paneBuilt.root.parentElement;
    paneOpen = true; host.hidden = false; host.dataset.open = "true";
    try { globalThis.localStorage?.setItem(PANE_KEY, "true"); } catch { /* ignored */ }
    paneButton.setAttribute("aria-expanded", "true");
    paneBuilt.setDisabled(!canAct());
    paneKey = ""; schedulePane(); void ensureList();
    paneBuilt.box.root.focus?.();
    return true;
  }
  function closePane({ focusDocument = true } = {}) {
    if (!paneOpen) return;
    paneOpen = false; endPreview();
    const host = paneBuilt?.root.parentElement;
    if (host) { host.hidden = true; delete host.dataset.open; }
    try { globalThis.localStorage?.setItem(PANE_KEY, "false"); } catch { /* ignored */ }
    paneButton.setAttribute("aria-expanded", "false");
    if (focusDocument) void controller.run(async () => {});
  }

  // ---- ribbon controls -----------------------------------------------------------------------------------------------------------
  const paneButton = make(document, "button", "styles-pane-button", i18n.t("styles-pane-open"));
  paneButton.type = "button"; paneButton.setAttribute("aria-expanded", "false"); paneButton.setAttribute("aria-controls", "styles-slot"); // a non-modal pane: expanded, not a popup
  paneButton.addEventListener("click", () => { if (paneButton.getAttribute("aria-disabled") !== "true") { if (paneOpen) closePane(); else openPane(); } });

  // ---- state -> controls ------------------------------------------------------------------------------------------------------
  const setDisabled = (node, disabled) => { node.disabled = disabled; node.setAttribute("aria-disabled", String(disabled)); };
  function render(snapshot) {
    if (disposed) return;
    const can = canFormat(snapshot) && !blocked;
    if (snapshot.state) sawState = true;
    else if (sawState) {
      // A document is being opened: the revisions start again, the session and its preview are gone.
      sawState = false; generation += 1; list = null; forgetPreview();
      renderGallery();
    }
    current = currentOf(snapshot);
    galleryBox.disabled = !can; gallery.setAttribute("aria-disabled", String(!can));
    setDisabled(previousButton, !can); setDisabled(nextButton, !can);
    gallery.dataset.plain = String(Boolean(snapshot.plainText));
    setDisabled(paneButton, !snapshot.state || blocked);
    if (!can) endPreview();
    paneBuilt?.setDisabled(!can);
    if (snapshot.state && (!list || list.generation !== generation) && !loadingList) void ensureList();
    updateCurrent();
    schedulePane();
  }
  const unsubscribe = controller.subscribe(render);

  // ---- safety nets: a preview never outlives the hover ------------------------------------------------------------------------------
  const onHide = () => endPreview();
  const onVisibility = () => { if (document.hidden) endPreview(); };
  globalThis.addEventListener?.("blur", onHide); stops.push(() => globalThis.removeEventListener?.("blur", onHide));
  document.addEventListener("visibilitychange", onVisibility); stops.push(() => document.removeEventListener("visibilitychange", onVisibility));

  // ---- shortcuts ---------------------------------------------------------------------------------------------------------------------
  async function heading(level) {
    if (!canAct()) return;
    const known = await ensureList() ?? list;
    const style = known?.styles.find(s => s.target === "paragraph" && s.headingLevel === level);
    if (!style) { announce(i18n.t("styles-status-no-heading", { level }), true); return; }
    await perform(style);
  }
  async function normal() {
    if (!canAct()) return;
    const known = await ensureList() ?? list;
    const style = known?.styles.find(s => s.target === "paragraph" && s.isDefault);
    if (style) await perform(style);
  }
  const KEY_BINDINGS = [
    // By `code`, deferred to the release of the key and only when the keystroke typed nothing (see the header: AZERTY).
    { code: "Digit1", ctrl: true, alt: true, ifNoCharacter: true, run: () => { void heading(1); } },
    { code: "Digit2", ctrl: true, alt: true, ifNoCharacter: true, run: () => { void heading(2); } },
    { code: "Digit3", ctrl: true, alt: true, ifNoCharacter: true, run: () => { void heading(3); } },
    // Whatever the layout: Ctrl+Maj+1 to 3 never type anything (no Alt), by `code` too. The only way to Titre 2 and 3 on an AZERTY keyboard.
    { code: "Digit1", ctrl: true, shift: true, run: () => { void heading(1); } },
    { code: "Digit2", ctrl: true, shift: true, run: () => { void heading(2); } },
    { code: "Digit3", ctrl: true, shift: true, run: () => { void heading(3); } },
    { key: "n", ctrl: true, shift: true, run: () => { void normal(); } },
  ];

  try { if (globalThis.localStorage?.getItem(PANE_KEY) === "true") queueMicrotask(() => { if (!disposed) openPane(); }); } catch { /* ignored */ }

  render(controller.snapshot());
  return Object.freeze({
    groups({ command }) {
      return [{ id: "styles", label: i18n.t("styles-group"), commands: [
        command("styles-gallery", "styles-gallery", "A", gallery),
        command("styles-pane", "styles-pane-open", "I", paneButton),
      ] }];
    },
    setBlocked(value) { blocked = Boolean(value); render(controller.snapshot()); },
    keyBindings() { return KEY_BINDINGS; },
    open: openPane,
    close: closePane,
    cancelPreview: cancelPreviewNow,
    toggle() { if (paneOpen) closePane(); else openPane(); },
    /** Pour les tests : la liste lue, le style courant et l'état de l'aperçu. */
    get state() { return { list: list?.styles ?? null, current, preview: { active: preview.active, shown: preview.shown }, paneOpen }; },
    get gallery() { return gallery; },
    get pane() { return paneBuilt?.root ?? null; },
    dispose() {
      if (disposed) return;
      endPreview(); disposed = true; unsubscribe(); clearTimeout(paneTimer);
      while (stops.length) stops.pop()();
      paneBuilt?.dispose();
      for (const node of [gallery, paneButton]) node.remove();
    },
  });
}
