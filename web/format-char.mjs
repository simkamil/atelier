// SPDX-License-Identifier: AGPL-3.0-or-later
// Mise en forme des caractères (US-240) : le groupe « Police » du ruban (police, taille, gras, italique, souligné, barré, indice,
// exposant, couleur de police, surlignage) et ses raccourcis. US-240 écrit ICI (et dans format-char.css,
// tests/rich-editor-format-char.mjs, rich_editor/format_char*.rs) et nulle part ailleurs côté web : ni editor-shell.mjs, ni app.mjs,
// ni rich-editor.mjs, ni index.html ne changent. Le contrat est docs/us-240-format-protocol.md ; le moteur est rich_editor/format_char.rs.
//
// Entrée : createCharFormat({ document, i18n, controller, announce })
//   document   : le document DOM (création des éléments)
//   i18n       : `i18n.t(key, values)` (catalogue) ; les clés de la story vont dans le bloc « # US-240 » de web/i18n/fr-FR.txt
//   controller : createFormatController (web/format-controller.mjs) ; `controller.subscribe(({ state, readOnly,
//                plainText, busy }) => …)` donne l'état du curseur (UN FormatState par changement), `controller.run(client =>
//                client.applyCharFormat({ edits }))` exécute une commande, relit l'état et rend le focus au document ;
//                `canFormat(snapshot)` (même fichier) dit si une commande peut partir
//   announce   : `announce(texte, isError)` écrit dans la région d'état de l'éditeur
// Sortie : { groups, setBlocked, keyBindings, dispose }
//   groups({ command, future }) -> tableau de GROUPES complets du ruban `{ id, label, keyTip?, commands }` : le groupe « Police » de
//     Word. Entrées personnalisées (liste de polices, saisie de taille, sélecteur de couleur) : des éléments créés ici et passés en
//     `element` ; le module nomme ses contrôles (aria-label, info-bulle), gère `disabled` et `aria-disabled` sur le conteneur, et rend
//     le focus au document par `controller.run`.
//   Info-touches (keyTip, UN caractère) de l'onglet Accueil : réservées à US-240 : P F S T K W X Y Z.
//     P police, T taille, F gras, K italique, S souligné, X barré, W indice, Y exposant, Z couleur de police ; le surlignage n'a pas
//     d'info-touche (neuf lettres pour dix commandes).
//   setBlocked(bool) : appelé par app.mjs à chaque changement d'état de l'application.
//   keyBindings() -> [{ key | code, ctrl, shift, alt, run(event) }] pour createFormatKeyRouter : Ctrl+G gras (convention de Word en
//     français), Ctrl+I, Ctrl+U, Ctrl+Maj+> et Ctrl+Maj+< (taille suivante, précédente de la liste de Word), Ctrl+= indice, Ctrl+Maj+=
//     exposant. Sur un clavier AZERTY, « < » est la touche sans Maj et « > » la même avec Maj : le routeur ignore Maj pour ces deux
//     touches et compare la valeur produite.
//   dispose() : retire les écouteurs et le DOM créé.
//
// Polices (T240-F) : le moteur ne dessine qu'avec Carlito (le WASM n'embarque pas les autres). Quand le curseur est sur un texte dont la
// police demandée a une police métrique (Arial -> Liberation Sans, Times New Roman -> Liberation Serif, Courier New -> Liberation Mono,
// Cambria -> Caladea) que le moteur n'utilise pas encore, ou quand l'utilisateur choisit une telle police, le module charge les quatre
// fichiers de la famille depuis les ressources de l'application (`/fonts/<Famille>-<Style>.ttf`) et les envoie par `LoadFonts`. Un
// fichier absent (404) laisse la police de remplacement, indiquée dans l'info-bulle de la boîte de police (critère 4).

import { createColorPicker, createFontCombo, createSplitButton, createTooltip } from "/ui-components.mjs";
import { canFormat } from "/format-controller.mjs";

const MIN_HALF_POINTS = 2;
const MAX_HALF_POINTS = 3276;
const MAX_NAME_BYTES = 255;
const RECENT_KEY = "atelier.recentFonts";
const RECENT_MAX = 5;
const RECENT_PREFERENCE_LENGTH = 64;
/** Sizes of Word's list, in points. */
const SIZE_LIST = [8, 9, 10, 10.5, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];
const STYLE_FILES = ["Regular", "Bold", "Italic", "BoldItalic"];
/** Word name -> family of the catalogue and the base of its file names (the metric substitutes the application serves). */
const FONT_FAMILIES = new Map([
  // i18n-ignore: noms de familles de polices, pas du texte d'interface
  ["arial", ["Liberation Sans", "LiberationSans"]], ["liberation sans", ["Liberation Sans", "LiberationSans"]],
  // i18n-ignore: noms de familles de polices
  ["times new roman", ["Liberation Serif", "LiberationSerif"]], ["liberation serif", ["Liberation Serif", "LiberationSerif"]],
  // i18n-ignore: noms de familles de polices
  ["courier new", ["Liberation Mono", "LiberationMono"]], ["liberation mono", ["Liberation Mono", "LiberationMono"]],
  ["cambria", ["Caladea", "Caladea"]], ["caladea", ["Caladea", "Caladea"]],
]);
const UNDERLINES = [["single", "char-underline-single"], ["double", "char-underline-double"], ["thick", "char-underline-thick"],
  ["dotted", "char-underline-dotted"], ["dashed", "char-underline-dashed"], ["wave", "char-underline-wave"]];
/** [catalogue key, hex]: the key of the label is in the catalogue block of US-240. */
const THEME_COLORS = [["char-color-black", "#000000"], ["char-color-white", "#ffffff"], ["char-color-slate", "#44546a"], ["char-color-silver", "#e7e6e6"],
  ["char-color-azure", "#4472c4"], ["char-color-orange", "#ed7d31"], ["char-color-gray", "#a5a5a5"], ["char-color-gold", "#ffc000"],
  ["char-color-sky", "#5b9bd5"], ["char-color-leaf", "#70ad47"]];
const STANDARD_COLORS = [["char-color-darkred", "#c00000"], ["char-color-red", "#ff0000"], ["char-color-amber", "#ffc000"], ["char-color-yellow", "#ffff00"],
  ["char-color-lightgreen", "#92d050"], ["char-color-green", "#00b050"], ["char-color-cyan", "#00b0f0"], ["char-color-blue", "#0070c0"],
  ["char-color-navy", "#002060"], ["char-color-purple", "#7030a0"]];
/** The sixteen highlight colours of WordprocessingML: the only ones a DOCX keeps exactly. */
const HIGHLIGHT_COLORS = [["char-hl-yellow", "#ffff00"], ["char-hl-green", "#00ff00"], ["char-hl-cyan", "#00ffff"], ["char-hl-magenta", "#ff00ff"],
  ["char-hl-blue", "#0000ff"], ["char-hl-red", "#ff0000"], ["char-hl-darkblue", "#000080"], ["char-hl-darkcyan", "#008080"],
  ["char-hl-darkgreen", "#008000"], ["char-hl-darkmagenta", "#800080"], ["char-hl-darkred", "#800000"], ["char-hl-darkyellow", "#808000"],
  ["char-hl-darkgray", "#808080"], ["char-hl-lightgray", "#c0c0c0"], ["char-hl-black", "#000000"], ["char-hl-white", "#ffffff"]];

const toHex = color => `#${[color.r, color.g, color.b].map(v => v.toString(16).padStart(2, "0")).join("")}`;
const fromHex = hex => ({ r: parseInt(hex.slice(1, 3), 16), g: parseInt(hex.slice(3, 5), 16), b: parseInt(hex.slice(5, 7), 16) });
const familyKey = name => String(name).trim().replace(/\s+/g, " ").toLowerCase();
const byteLength = value => new TextEncoder().encode(value).length;
// eslint-disable-next-line no-control-regex
const hasControl = value => /[\u0000-\u001f\u007f-\u009f]/u.test(value);

export function createCharFormat({ document, i18n, controller, announce } = {}) {
  const t = (key, values) => i18n.t(key, values);
  const tip = (labelKey, shortcutKey) => (shortcutKey ? t("char-tip-shortcut", { label: t(labelKey), shortcut: t(shortcutKey) }) : t(labelKey));
  const disposers = [];
  let blocked = false;
  let disposed = false;
  let snapshot = controller.snapshot();
  let hadState = false;
  const pending = new Set();       // catalogue families being fetched
  const failed = new Set();        // catalogue families whose files the application does not serve
  let optionsRevision = null;      // revision the font list was read at
  let optionsLoad = null;

  const el = (name, className, text) => {
    const node = document.createElement(name);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const withTooltip = (trigger, text) => { const handle = createTooltip({ trigger, text }); disposers.push(() => handle.dispose()); return handle; };
  const usable = () => canFormat(snapshot) && !blocked;

  // ---- commands --------------------------------------------------------------------------------------------------------------------

  async function run(edits) {
    if (!usable()) return;
    try {
      await controller.run(client => client.applyCharFormat({ scope: "wordOrTyping", edits }));
    } catch (error) {
      if (error?.name === "AbortError" || error?.code === "CLIENT_CLOSED") return;
      announce?.(t("char-error"), true);
    }
  }
  const toggle = attr => run([{ attr, op: "toggle" }]);
  const toggleChoice = (attr, value) => run([{ attr, op: "toggle", value }]);
  const setValue = (attr, value) => run([{ attr, op: "set", value }]);
  const clear = attr => run([{ attr, op: "clear" }]);
  const stepSize = up => run([{ attr: "size", op: up ? "stepUp" : "stepDown" }]);

  // ---- fonts ---------------------------------------------------------------------------------------------------------------------

  function recentFonts() {
    try {
      const stored = JSON.parse(globalThis.localStorage?.getItem(RECENT_KEY) ?? "[]");
      return Array.isArray(stored) ? stored.filter(name => typeof name === "string" && name.length <= RECENT_PREFERENCE_LENGTH).slice(0, RECENT_MAX) : [];
    } catch { return []; }
  }
  function rememberFont(name) {
    try {
      const next = [name, ...recentFonts().filter(other => other !== name)].slice(0, RECENT_MAX);
      globalThis.localStorage?.setItem(RECENT_KEY, JSON.stringify(next));
    } catch { /* storage unavailable: the recent list is a convenience */ }
  }

  /** Fetches the four files of a catalogue family and gives them to the engine; the state is read again afterwards. */
  async function ensureFamily(family, base) {
    if (disposed || pending.has(family) || failed.has(family) || !controller.client) return;
    pending.add(family);
    try {
      const files = await Promise.all(STYLE_FILES.map(async style => {
        try {
          const response = await fetch(`/fonts/${base}-${style}.ttf`);
          return response.ok ? new Uint8Array(await response.arrayBuffer()) : null;
        } catch { return null; }
      }));
      const fonts = files.filter(file => file && file.length > 0 && file.length <= 1024 * 1024);
      if (fonts.length === 0) { failed.add(family); return; }
      try { await controller.client.loadFonts({ fonts }); } catch { failed.add(family); return; }
      optionsRevision = null;
      await controller.refresh();
    } finally { pending.delete(family); }
  }
  const catalogueOf = name => FONT_FAMILIES.get(familyKey(name)) ?? null;
  function ensureFor(name, used) {
    const entry = catalogueOf(name);
    if (entry && used !== entry[0]) void ensureFamily(entry[0], entry[1]);
  }

  // ---- controls --------------------------------------------------------------------------------------------------------------------

  const fontOptions = [];
  const fontCombo = createFontCombo({ label: t("char-font"), options: fontOptions, value: "", onChange: value => { void applyFont(value); } });
  const fontInput = fontCombo.input;
  fontCombo.element.classList.add("char-control", "char-font");
  const sizeOptions = SIZE_LIST.map(points => ({ label: i18n.formatNumber(points), value: String(points) }));
  const sizeCombo = createFontCombo({ label: t("char-size"), options: sizeOptions, value: "", onChange: value => { void applySize(value); } });
  const sizeInput = sizeCombo.input;
  sizeCombo.element.classList.add("char-control", "char-size");
  sizeInput.inputMode = "decimal";
  sizeInput.setAttribute("aria-label", t("char-size"));
  const fontTip = withTooltip(fontInput, t("char-font"));
  withTooltip(sizeInput, tip("char-size", "char-shortcut-size"));

  // Mouse access to the list (keyboard: ArrowDown).
  function arrow(combo, input, labelKey) {
    const button = el("button", "ui-button char-combo-arrow", "▾");
    button.type = "button"; button.tabIndex = -1; button.setAttribute("aria-hidden", "true");
    const open = () => { input.focus(); input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })); };
    button.addEventListener("click", open);
    button.title = t(labelKey);
    combo.element.append(button);
    disposers.push(() => button.removeEventListener("click", open));
    return button;
  }
  const fontArrow = arrow(fontCombo, fontInput, "char-font-list");
  const sizeArrow = arrow(sizeCombo, sizeInput, "char-size-list");

  /** Free entry: Enter (or leaving the box) commits what was typed when no list entry is being chosen. */
  function commitOnEnter(input, commit) {
    const onKey = event => {
      if (event.key !== "Enter" || event.isComposing || input.getAttribute("aria-activedescendant")) return;
      event.preventDefault();
      commit(input.value);
    };
    input.addEventListener("keydown", onKey);
    disposers.push(() => input.removeEventListener("keydown", onKey));
  }
  commitOnEnter(fontInput, name => { void applyFont(name.trim()); });
  commitOnEnter(sizeInput, text => { void applySize(text); });
  // Escape in a box that has no list open gives the typed text up and the focus back to the document.
  for (const input of [fontInput, sizeInput]) {
    const onEscape = event => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      event.preventDefault();
      renderBoxes();
      void controller.run(async () => {});
    };
    input.addEventListener("keydown", onEscape);
    disposers.push(() => input.removeEventListener("keydown", onEscape));
  }
  const onSizeChange = () => { /* a value typed then Tab: committed like Enter */ if (document.activeElement !== sizeInput) void applySize(sizeInput.value); };
  sizeInput.addEventListener("change", onSizeChange);
  disposers.push(() => sizeInput.removeEventListener("change", onSizeChange));

  async function applyFont(name) {
    if (!name || byteLength(name) > MAX_NAME_BYTES || hasControl(name)) { announce?.(t("char-font-invalid"), true); renderBoxes(); return; }
    rememberFont(name);
    optionsRevision = null;
    const entry = catalogueOf(name);
    if (entry) await ensureFamily(entry[0], entry[1]);
    await setValue("font", name);
  }
  async function applySize(text) {
    const points = Number(String(text).trim().replace(",", "."));
    const half = Math.round(points * 2);
    if (!Number.isFinite(points) || half < MIN_HALF_POINTS || half > MAX_HALF_POINTS || points <= 0) {
      announce?.(t("char-size-invalid"), true); renderBoxes(); return;
    }
    await setValue("size", half);
  }

  /** The font list: recent fonts, fonts of the theme, then every font the engine knows or the document names. */
  function loadOptions() {
    const revision = snapshot.state?.revision ?? null;
    if (!controller.client || revision === null || optionsRevision === revision || optionsLoad) return optionsLoad;
    optionsLoad = (async () => {
      try {
        const list = await controller.client.listFonts();
        const entries = new Map(list.families.map(family => [family.name, family]));
        const names = [];
        const seen = new Set();
        const labelOf = name => {
          const entry = entries.get(name);
          if (name === list.themeMajor && name === list.themeMinor) return t("char-font-theme", { name });
          if (name === list.themeMajor) return t("char-font-theme-heading", { name });
          if (name === list.themeMinor) return t("char-font-theme-body", { name });
          if (recentFonts().includes(name)) return entry?.substituted ? t("char-font-recent-replaced", { name, used: entry.substitute }) : t("char-font-recent", { name });
          return entry?.substituted ? t("char-font-replaced", { name, used: entry.substitute }) : name;
        };
        for (const name of [...recentFonts(), list.themeMajor, list.themeMinor, ...[...entries.keys()].sort((a, b) => a.localeCompare(b))]) {
          if (!name || seen.has(name)) continue;
          seen.add(name); names.push(name);
        }
        fontOptions.splice(0, fontOptions.length, ...names.map(name => ({ label: labelOf(name), value: name })));
        optionsRevision = revision;
        for (const family of list.families) if (family.used) ensureFor(family.name, family.substitute);
      } catch { /* the list is a convenience: the box still takes a typed name */ } finally { optionsLoad = null; }
    })();
    return optionsLoad;
  }
  const onFontFocus = () => { void loadOptions(); };
  fontInput.addEventListener("focus", onFontFocus);
  disposers.push(() => fontInput.removeEventListener("focus", onFontFocus));

  // Buttons whose state follows the selection (aria-pressed: true, false, or mixed).
  const pressables = [];
  function pressable({ id, labelKey, glyphKey, shortcutKey, onActivate, className }) {
    const button = el("button", `ui-button char-control char-button ${className ?? ""}`.trim(), t(glyphKey));
    button.type = "button";
    button.setAttribute("aria-label", t(labelKey));
    button.setAttribute("aria-pressed", "false");
    const click = () => { if (!button.disabled) onActivate(); };
    button.addEventListener("click", click);
    disposers.push(() => button.removeEventListener("click", click));
    withTooltip(button, tip(labelKey, shortcutKey));
    const entry = { id, button, shortcutKey };
    pressables.push(entry);
    return button;
  }
  const bold = pressable({ id: "bold", labelKey: "char-bold", glyphKey: "char-bold-glyph", shortcutKey: "char-shortcut-bold", className: "char-bold", onActivate: () => toggle("bold") });
  const italic = pressable({ id: "italic", labelKey: "char-italic", glyphKey: "char-italic-glyph", shortcutKey: "char-shortcut-italic", className: "char-italic", onActivate: () => toggle("italic") });
  const strike = pressable({ id: "strike", labelKey: "char-strike", glyphKey: "char-strike-glyph", className: "char-strike", onActivate: () => toggle("strike") });
  const subscript = pressable({ id: "subscript", labelKey: "char-subscript", glyphKey: "char-subscript-glyph", shortcutKey: "char-shortcut-subscript", onActivate: () => toggleChoice("verticalAlign", "subscript") });
  const superscript = pressable({ id: "superscript", labelKey: "char-superscript", glyphKey: "char-superscript-glyph", shortcutKey: "char-shortcut-superscript", onActivate: () => toggleChoice("verticalAlign", "superscript") });

  // Underline: the button toggles the simple style, the menu picks one of the six or none.
  const underlineItems = [...UNDERLINES.map(([name, key]) => ({ label: t(key), value: name })), { label: t("char-underline-none"), value: "none" }];
  const underline = createSplitButton({
    label: t("char-underline"), menuLabel: t("char-underline-menu"), items: underlineItems,
    onActivate: () => { void toggleChoice("underline", "single"); },
    onSelect: value => { void setValue("underline", value); },
  });
  underline.element.classList.add("char-control", "char-underline");
  underline.action.textContent = t("char-underline-glyph");
  underline.action.setAttribute("aria-label", t("char-underline"));
  underline.action.setAttribute("aria-pressed", "false");
  withTooltip(underline.action, tip("char-underline", "char-shortcut-underline"));

  // Colours: a palette, plus "Automatic" (font) or "No colour" (highlight) which clear the attribute.
  let silent = false;   // a value set from the state is not a command
  function colorControl({ id, labelKey, glyphKey, themed, colors, noneKey, attr }) {
    const make = list => list.map(([key, hex]) => ({ label: t(key), value: hex }));
    const picker = createColorPicker({
      label: t(labelKey), customLabel: t("char-color-custom"), value: "#000000",
      themeColors: themed ? { label: t("char-colors-theme"), colors: make(themed) } : { label: "", colors: [] },
      standardColors: { label: t(themed ? "char-colors-standard" : "char-colors-highlight"), colors: make(colors) },
      onChange: hex => { if (!silent) debounced(attr, hex); },
    });
    picker.element.classList.add("char-control", `char-${id}`);
    picker.trigger.textContent = t(glyphKey);
    picker.trigger.setAttribute("aria-label", t(labelKey));
    withTooltip(picker.trigger, tip(labelKey));
    const none = el("button", "ui-button char-color-none", t(noneKey));
    none.type = "button";
    const onNone = () => { picker.close({ restoreFocus: false }); void clear(attr); };
    none.addEventListener("click", onNone);
    picker.popup.prepend(none);
    disposers.push(() => none.removeEventListener("click", onNone));
    if (!themed) {
      // A DOCX keeps only the sixteen highlight colours exactly: no free colour here.
      const custom = picker.popup.querySelector(".ui-color-custom");
      if (custom) custom.hidden = true;
    }
    return picker;
  }
  // The custom colour input fires continuously: only the last colour of a burst is applied (one undo step).
  const timers = new Map();
  function debounced(attr, hex) {
    clearTimeout(timers.get(attr));
    timers.set(attr, setTimeout(() => { timers.delete(attr); void setValue(attr, fromHex(hex)); }, 120));
  }
  disposers.push(() => { for (const timer of timers.values()) clearTimeout(timer); timers.clear(); });
  const fontColor = colorControl({ id: "color", labelKey: "char-color", glyphKey: "char-color-glyph", themed: THEME_COLORS, colors: STANDARD_COLORS, noneKey: "char-color-automatic", attr: "color" });
  const highlight = colorControl({ id: "highlight", labelKey: "char-highlight", glyphKey: "char-highlight-glyph", themed: null, colors: HIGHLIGHT_COLORS, noneKey: "char-highlight-none", attr: "highlight" });

  // ---- reflecting the selection ----------------------------------------------------------------------------------------------------

  const setPressed = (button, field) => {
    button.setAttribute("aria-pressed", field?.state === "mixed" ? "mixed" : String(field?.state === "value" && field.value === true));
  };
  const showText = (input, text) => { if (document.activeElement !== input && input.value !== text) input.value = text; };

  function renderBoxes() {
    const chars = snapshot.state?.chars;
    const font = chars?.font;
    const size = chars?.size;
    let fontText = "";
    let tooltip = t("char-font");
    if (font?.state === "value") {
      fontText = font.value.requested;
      if (font.value.substituted) tooltip = t("char-font-substituted", { requested: font.value.requested, used: font.value.used });
    }
    showText(fontInput, fontText);
    fontInput.removeAttribute("aria-description");
    if (font?.state === "value" && font.value.substituted) fontInput.setAttribute("aria-description", tooltip);
    fontInput.title = "";
    fontTip.tooltip.textContent = tooltip;
    fontCombo.element.dataset.substituted = String(font?.state === "value" && font.value.substituted === true);
    showText(sizeInput, size?.state === "value" ? i18n.formatNumber(size.value / 2) : "");
  }

  function render() {
    const state = snapshot.state;
    if (!state) optionsRevision = null; // another document: the font list is read again
    const enabled = usable();
    for (const { button } of pressables) { button.disabled = !enabled; button.setAttribute("aria-disabled", String(!enabled)); }
    for (const combo of [fontCombo, sizeCombo]) { combo.input.disabled = !enabled; combo.element.setAttribute("aria-disabled", String(!enabled)); }
    for (const arrowButton of [fontArrow, sizeArrow]) arrowButton.disabled = !enabled;
    for (const button of [underline.action, underline.disclosure]) { button.disabled = !enabled; button.setAttribute("aria-disabled", String(!enabled)); }
    underline.element.setAttribute("aria-disabled", String(!enabled));
    for (const picker of [fontColor, highlight]) {
      picker.trigger.disabled = !enabled; picker.trigger.setAttribute("aria-disabled", String(!enabled));
      picker.element.setAttribute("aria-disabled", String(!enabled));
    }
    const chars = state?.chars;
    setPressed(bold, chars?.bold); setPressed(italic, chars?.italic); setPressed(strike, chars?.strike);
    const align = chars?.verticalAlign;
    const alignPressed = value => align?.state === "mixed" ? "mixed" : String(align?.state === "value" && align.value === value);
    subscript.setAttribute("aria-pressed", alignPressed("subscript")); superscript.setAttribute("aria-pressed", alignPressed("superscript"));
    const line = chars?.underline;
    underline.action.setAttribute("aria-pressed", line?.state === "mixed" ? "mixed" : String(line?.state === "value" && line.value !== "none"));
    underline.element.dataset.underline = line?.state === "value" ? line.value : line?.state ?? "";
    for (const [picker, field, hexes] of [[fontColor, chars?.color, null], [highlight, chars?.highlight, HIGHLIGHT_COLORS.map(([, hex]) => hex)]]) {
      const shown = field?.state === "value" ? toHex(field.value) : null;
      picker.element.style.setProperty("--char-swatch", shown ?? "transparent");
      picker.element.dataset.colorState = field?.state ?? "";
      if (shown && (!hexes || hexes.includes(shown))) { silent = true; try { picker.setValue(shown); } finally { silent = false; } }
    }
    renderBoxes();
    // First use of a family: the file is fetched when the caret reaches text in a font the engine only substitutes.
    const font = chars?.font;
    if (font?.state === "value") ensureFor(font.value.requested, font.value.used);
    if (state && !hadState) void loadOptions();
    hadState = Boolean(state);
  }

  const unsubscribe = controller.subscribe(next => { snapshot = next; if (!disposed) render(); });
  disposers.push(unsubscribe);

  // ---- ribbon and shortcuts --------------------------------------------------------------------------------------------------------

  const shortcut = (spec, action) => ({ ...spec, run: () => { if (usable()) action(); } });
  return Object.freeze({
    groups({ command }) {
      return [{ id: "font", label: t("ribbon-font"), commands: [
        command("font", "char-font", "P", fontCombo.element),
        command("size", "char-size", "T", sizeCombo.element),
        command("bold", "char-bold", "F", bold),
        command("italic", "char-italic", "K", italic),
        command("underline", "char-underline", "S", underline.element),
        command("strike", "char-strike", "X", strike),
        command("subscript", "char-subscript", "W", subscript),
        command("superscript", "char-superscript", "Y", superscript),
        command("color", "char-color", "Z", fontColor.element),
        command("highlight", "char-highlight", undefined, highlight.element),
      ] }];
    },
    setBlocked(value) { blocked = Boolean(value); render(); },
    keyBindings() {
      return [
        shortcut({ key: "g", ctrl: true }, () => toggle("bold")),
        shortcut({ key: "i", ctrl: true }, () => toggle("italic")),
        shortcut({ key: "u", ctrl: true }, () => toggleChoice("underline", "single")),
        shortcut({ key: ">", ctrl: true, shift: undefined }, () => stepSize(true)),
        shortcut({ key: "<", ctrl: true, shift: undefined }, () => stepSize(false)),
        shortcut({ key: "=", ctrl: true, shift: false }, () => toggleChoice("verticalAlign", "subscript")),
        shortcut({ key: "+", ctrl: true, shift: undefined }, () => toggleChoice("verticalAlign", "superscript")),
      ];
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      for (const dispose of disposers.splice(0)) { try { dispose(); } catch { /* already gone */ } }
      fontCombo.dispose(); sizeCombo.dispose(); underline.dispose(); fontColor.dispose(); highlight.dispose();
      for (const { button } of pressables) button.remove();
    },
  });
}
