// SPDX-License-Identifier: AGPL-3.0-or-later
// Mise en forme des paragraphes (US-243), groupe « Paragraphe » de l'onglet Accueil. Écrit ICI, dans format-para.css, dans
// tests/rich-editor-format-para*.mjs, rich_editor/format_para*.rs et le bloc « # US-243 » de web/i18n/fr-FR.txt, et nulle part ailleurs.
//
// Entrée : createParaFormat({ document, i18n, controller, announce })
//   document   : le document DOM ; i18n : `i18n.t(key, values)` ; controller : createFormatController (un FormatState par changement,
//                `controller.run(fn)` exécute une commande, relit l'état et rend le focus au document) ; announce(texte, isError).
// Sortie : { groups, setBlocked, keyBindings, attachEditor, detachEditor, dispose }
//   groups({ command, future })  -> le groupe « Paragraphe » : alignements (boutons à aria-pressed), retraits, menu « Interligne et
//     espacement » (valeurs courantes, Options d'interligne…, espace avant/après), Trame et Bordures (désactivés avec leur raison,
//     jusqu'à US-243-M1), marques de mise en forme (bouton à aria-pressed). Info-touches : G N D J M O L Q ; B est gardée pour
//     les bordures.
//   keyBindings() : Ctrl+L, E, R, J (alignements) ; Ctrl+M et Ctrl+Maj+M (retrait) ; Ctrl+1, 2, 5 (interligne), Ctrl+0 (espace
//     avant) ; Ctrl+Maj+8 (marques). Les chiffres sont reconnus par `code` (Digit1…) : ils fonctionnent aussi en AZERTY.
//   attachEditor(editor, { canvas, scrollport, spacer }?) / detachEditor() : la surcouche des marques de mise en forme se dessine
//     au-dessus du Canvas ; elle lit le zoom (`editor.getZoom()`), le nombre de pages (`editor.state.pageCount`) et, quand l'éditeur
//     l'offre, la géométrie des pages (`editor.getGeometry()` : { pages: [{ minX, minY, width, height }], zoom, pageGap }) ; sans elle,
//     la hauteur de page se déduit de la zone d'espacement du défilement, à défaut une page A4. Défauts du DOM : #document-canvas,
//     #document-scroll, #page-spacer. SANS appel, le bouton existe et mémorise son état, mais rien ne se dessine.
//   Les marques viennent de `FormatMarks` (rich_editor/format_para.rs) : une page à la fois, mises en cache par (génération, révision,
//   nombre de pages, mise en page achevée), jamais dessinées par le moteur : activer l'option n'invalide aucune page.

import { canFormat } from "/format-controller.mjs";
import { createButton, createContextMenu, createDialog } from "/ui-components.mjs";

const SVG = "http://www.w3.org/2000/svg";
const MARKS_KEY = "atelier.formatMarks";
const SPACE_STEP = 240;
const PAGE_GAP = 16;
const A4 = Object.freeze({ width: 595.276, height: 841.89 });
/** Valeurs courantes de l'interligne : 240es de ligne. */
const LINE_VALUES = Object.freeze([240, 276, 360, 480, 600, 720]);
// i18n-ignore: familles de polices CSS, pas du texte d’interface
const markFont = size => `${Math.max(6, size)}px Carlito, "Segoe UI", sans-serif`;
const MARK_COLOR = "rgba(63, 105, 150, 0.85)";

const make = (document, name, className, text) => {
  const node = document.createElement(name);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/** Icône de barres horizontales : `[x, largeur]` par ligne (viewBox 16 × 14). */
function barsIcon(document, bars, arrow = null) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 16 14"); svg.setAttribute("width", "16"); svg.setAttribute("height", "14");
  svg.setAttribute("aria-hidden", "true"); svg.setAttribute("focusable", "false");
  bars.forEach(([x, width], index) => {
    const bar = document.createElementNS(SVG, "rect");
    bar.setAttribute("x", String(x)); bar.setAttribute("y", String(1 + index * 3)); bar.setAttribute("width", String(width)); bar.setAttribute("height", "1.6");
    bar.setAttribute("fill", "currentColor"); svg.append(bar);
  });
  if (arrow) {
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", arrow); path.setAttribute("fill", "none"); path.setAttribute("stroke", "currentColor"); path.setAttribute("stroke-width", "1.4");
    svg.append(path);
  }
  return svg;
}

const ALIGN_ICONS = Object.freeze({
  left: [[0, 16], [0, 10], [0, 16], [0, 8]], center: [[0, 16], [3, 10], [0, 16], [4, 8]],
  right: [[0, 16], [6, 10], [0, 16], [8, 8]], justify: [[0, 16], [0, 16], [0, 16], [0, 16]],
});

/**
 * Surcouche des marques de mise en forme (T243-C) : un Canvas transparent posé sur celui du document, qui dessine ¶, points des
 * espaces, flèches des tabulations, cercles des espaces insécables et marques de saut d'après `FormatMarks`.
 */
function createMarksOverlay({ document, i18n, controller }) {
  let editor = null; let canvas = null; let scrollport = null; let spacer = null; let overlay = null;
  let on = false; let disposed = false; let frame = 0; let generation = 0; let sawState = false;
  let cacheKey = ""; let drawKey = ""; let drawn = 0;
  const stops = []; // listeners and observers to release on detach
  // page -> { key, marks }: an entry of an older key keeps being drawn until the answer for the new key replaces it (no flicker)
  const marks = new Map(); const inflight = new Map(); const retries = new Map();

  const keyOf = () => {
    const state = controller.snapshot().state;
    return `${generation}|${state ? state.revision : "-"}|${editor?.state?.pageCount ?? "-"}|${editor?.state?.layoutComplete ?? "-"}`;
  };

  function geometry() {
    const given = editor.getGeometry?.();
    if (given?.pages?.length) return { zoom: given.zoom, gap: given.pageGap ?? PAGE_GAP, pages: given.pages };
    const zoom = editor.getZoom();
    const count = Math.max(1, editor.state?.pageCount ?? 1);
    let height = A4.height;
    const spacerHeight = parseFloat(spacer?.style.height ?? "0");
    if (spacerHeight > 0) {
      const derived = (spacerHeight + scrollport.clientHeight - count * PAGE_GAP) / (count * zoom);
      if (Number.isFinite(derived) && derived > 50) height = derived;
    }
    return { zoom, gap: PAGE_GAP, pages: Array.from({ length: count }, () => ({ minX: 0, minY: 0, width: A4.width, height })) };
  }

  function visiblePages(geo) {
    const out = []; let top = -scrollport.scrollTop;
    const limit = canvas.clientHeight;
    for (let page = 0; page < geo.pages.length; page += 1) {
      const height = geo.pages[page].height * geo.zoom;
      if (top + height >= 0 && top <= limit) out.push({ page, top });
      top += height + geo.gap;
      if (top > limit) break;
    }
    return out;
  }

  function fetchPage(page) {
    if (inflight.get(page) === cacheKey || marks.get(page)?.key === cacheKey) return;
    const client = controller.client;
    if (!client) return;
    const mine = cacheKey;
    inflight.set(page, mine);
    client.formatMarks({ first: page, count: 1 }).then(reply => {
      inflight.delete(page);
      if (disposed || mine !== cacheKey) return;
      marks.set(page, { key: mine, marks: reply.marks }); retries.delete(page); schedule();
    }, error => {
      inflight.delete(page);
      if (disposed || mine !== cacheKey) return;
      if (error?.code === "RICH_NOT_READY" && (retries.get(page) ?? 0) < 25) {
        retries.set(page, (retries.get(page) ?? 0) + 1);
        setTimeout(() => { if (!disposed && mine === cacheKey) fetchPage(page); }, 120);
      } else { marks.set(page, { key: mine, marks: [] }); schedule(); } // a page that vanished, or too many marks: nothing is drawn for it
    });
  }

  function paint(geo, pages) {
    const dpr = globalThis.devicePixelRatio || 1;
    const width = canvas.clientWidth; const height = canvas.clientHeight;
    overlay.style.width = `${width}px`; overlay.style.height = `${height}px`; overlay.style.marginTop = `${-height}px`;
    const backing = [Math.round(width * dpr), Math.round(height * dpr)];
    if (overlay.width !== backing[0]) overlay.width = backing[0];
    if (overlay.height !== backing[1]) overlay.height = backing[1];
    const ctx = overlay.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = MARK_COLOR; ctx.strokeStyle = MARK_COLOR;
    ctx.textBaseline = "alphabetic";
    let count = 0;
    for (const { page, top } of pages) {
      const origin = geo.pages[page];
      for (const mark of marks.get(page)?.marks ?? []) {
        const x = -scrollport.scrollLeft + (mark.x - origin.minX) * geo.zoom;
        const y = top + (mark.y - origin.minY) * geo.zoom;
        const w = mark.width * geo.zoom;
        count += 1;
        switch (mark.kind) {
          case "space": ctx.beginPath(); ctx.arc(x + w / 2, y - 0.3 * 11 * geo.zoom, Math.max(0.6, 0.9 * geo.zoom), 0, 2 * Math.PI); ctx.fill(); break;
          case "noBreakSpace": ctx.beginPath(); ctx.arc(x + w / 2, y - 0.3 * 11 * geo.zoom, Math.max(1.2, 2.2 * geo.zoom), 0, 2 * Math.PI); ctx.lineWidth = Math.max(0.6, geo.zoom * 0.8); ctx.stroke(); break;
          case "tab": {
            const mid = y - 0.28 * 11 * geo.zoom; const end = x + Math.max(w, 3 * geo.zoom) - geo.zoom;
            ctx.beginPath(); ctx.lineWidth = Math.max(0.6, geo.zoom * 0.8);
            ctx.moveTo(x + geo.zoom, mid); ctx.lineTo(end, mid);
            ctx.moveTo(end - 2.5 * geo.zoom, mid - 2 * geo.zoom); ctx.lineTo(end, mid); ctx.lineTo(end - 2.5 * geo.zoom, mid + 2 * geo.zoom);
            ctx.stroke(); break;
          }
          case "paragraphEnd": ctx.font = markFont(w * 2 * geo.zoom); ctx.fillText("¶", x + geo.zoom, y); break;
          case "lineBreak": ctx.font = markFont(w * 2 * geo.zoom); ctx.fillText("↵", x + geo.zoom, y); break;
          default: { // pageBreak, columnBreak: dotted rule with its label
            const label = i18n.t("para-mark-page-break");
            ctx.font = markFont(w * 2 * geo.zoom);
            const rule = Math.max(20 * geo.zoom, 40);
            ctx.save(); ctx.setLineDash([2, 2]); ctx.beginPath(); ctx.moveTo(x + 3 * geo.zoom, y - 3 * geo.zoom); ctx.lineTo(x + 3 * geo.zoom + rule, y - 3 * geo.zoom); ctx.stroke(); ctx.restore();
            ctx.fillText(label, x + 6 * geo.zoom + rule, y);
          }
        }
      }
    }
    drawn = count;
  }

  function update() {
    frame = 0;
    if (!on || disposed || !editor || !overlay) return;
    const state = controller.snapshot().state;
    if (state && !sawState) sawState = true;
    if (!state && sawState) { sawState = false; generation += 1; } // a document is being opened: the revisions start again
    const key = keyOf();
    if (key !== cacheKey) { cacheKey = key; retries.clear(); } // the old marks stay drawn until the new answers arrive
    if (!state || !editor.state) return;
    const geo = geometry(); const pages = visiblePages(geo);
    for (const [page] of marks) if (!pages.some(p => p.page === page) && marks.get(page).key !== cacheKey) marks.delete(page);
    for (const { page } of pages) fetchPage(page);
    const signature = `${cacheKey}|${geo.zoom}|${scrollport.scrollLeft}|${scrollport.scrollTop}|${canvas.clientWidth}x${canvas.clientHeight}|${pages.map(p => `${p.page}:${marks.get(p.page)?.key}:${marks.get(p.page)?.marks.length ?? "-"}`).join(",")}|${devicePixelRatio}`;
    if (signature !== drawKey) { drawKey = signature; paint(geo, pages); }
  }
  /** One redraw per frame, asked for by an event (scroll, resize, zoom, state, answer); no permanent loop. */
  function schedule() { if (!frame && on && editor && overlay && !disposed) frame = requestAnimationFrame(update); }
  function watch() {
    const add = (target, type) => { target.addEventListener(type, schedule, { passive: true }); stops.push(() => target.removeEventListener(type, schedule)); };
    add(scrollport, "scroll"); add(globalThis, "resize");
    const observe = (Observer, target, options) => { if (!target || typeof Observer !== "function") return; const o = new Observer(schedule); o.observe(target, options); stops.push(() => o.disconnect()); };
    observe(globalThis.ResizeObserver, canvas);
    // A zoom or a new page layout rewrites the style of the spacer and of the canvas.
    observe(globalThis.MutationObserver, spacer, { attributes: true, attributeFilter: ["style"] });
    observe(globalThis.MutationObserver, canvas, { attributes: true, attributeFilter: ["style"] });
    stops.push(controller.subscribe(schedule));
    if (typeof editor.subscribe === "function") stops.push(editor.subscribe(schedule));
  }
  function unwatch() { while (stops.length) stops.pop()(); }
  const start = () => { drawKey = ""; schedule(); };

  function ensureOverlay() {
    if (overlay || !canvas) return;
    overlay = make(document, "canvas", "para-marks-overlay");
    overlay.setAttribute("aria-hidden", "true");
    canvas.after(overlay);
  }
  function removeOverlay() { overlay?.remove(); overlay = null; drawKey = ""; marks.clear(); }

  return {
    attach(target, options = {}) {
      editor = target;
      canvas = options.canvas ?? document.getElementById("document-canvas");
      scrollport = options.scrollport ?? document.getElementById("document-scroll");
      spacer = options.spacer ?? document.getElementById("page-spacer");
      unwatch(); watch();
      if (on) { ensureOverlay(); start(); }
    },
    detach() { unwatch(); editor = null; if (frame) cancelAnimationFrame(frame); frame = 0; removeOverlay(); },
    set(value) {
      on = Boolean(value);
      if (on && editor && canvas) { ensureOverlay(); drawKey = ""; start(); }
      if (!on) { if (frame) cancelAnimationFrame(frame); frame = 0; removeOverlay(); }
    },
    get active() { return on; },
    /** Nombre de marques dessinées au dernier passage (pour les tests). */
    get drawn() { return drawn; },
    get element() { return overlay; },
    dispose() { disposed = true; unwatch(); if (frame) cancelAnimationFrame(frame); removeOverlay(); },
  };
}

export function createParaFormat({ document, i18n, controller, announce = () => {} } = {}) {
  let blocked = false; let disposed = false;
  let marksOn = false;
  try { marksOn = globalThis.localStorage?.getItem(MARKS_KEY) === "true"; } catch { /* the preference store can be blocked */ }
  const overlay = createMarksOverlay({ document, i18n, controller });
  overlay.set(marksOn);

  // ---- running a command -----------------------------------------------------------------------------------------------------
  const refused = new Set(["RICH_READ_ONLY", "RICH_FORMAT_PLAIN_TEXT", "RICH_FORMAT_NOT_IMPLEMENTED", "RICH_STALE_REVISION", "RICH_LIMIT", "RICH_UNKNOWN_DOCUMENT"]);
  async function perform(fn, status) {
    if (disposed || blocked || !canFormat(controller.snapshot())) return;
    try {
      await controller.run(fn);
      if (status) announce(status, false);
    } catch (error) {
      if (error?.code === "CLIENT_CLOSED") return;
      if (error?.outcome === "applied") { if (status) announce(status, false); return; }
      announce(i18n.t(refused.has(error?.code) || error?.outcome === "not-applied" ? "para-error-refused" : "para-error-unknown"), true);
    }
  }
  const apply = (edits, status) => perform(client => client.applyParaFormat({ edits }), status);

  // Alignment: a second click on the active one goes back to left (Word), decided on a FormatState of the current selection.
  const align = (name, status) => perform(async client => {
    const current = (await client.formatState()).paragraph.alignment;
    const target = current.state === "value" && current.value === name && name !== "left" ? "left" : name;
    return client.applyParaFormat({ edits: [{ attr: "alignment", op: "set", value: target }] });
  }, status);
  const indent = (up) => apply([{ attr: "indentLeft", op: up ? "stepUp" : "stepDown" }], i18n.t(up ? "para-status-indent-up" : "para-status-indent-down"));
  const lineSpacing = (rule, value, status) => apply([{ attr: "lineSpacing", op: "set", value: { rule, value } }], status);
  // Space before/after: the engine toggles when every paragraph agrees; a mixed selection gets the 12 pt (Word's "Add").
  const space = (attr, status) => perform(async client => {
    const current = (await client.formatState()).paragraph[attr];
    const edit = current.state === "mixed" ? { attr, op: "set", value: SPACE_STEP } : { attr, op: "toggle" };
    return client.applyParaFormat({ edits: [edit] });
  }, status);
  const spaceBefore = () => space("spaceBefore", i18n.t("para-status-space-before"));
  const spaceAfter = () => space("spaceAfter", i18n.t("para-status-space-after"));

  function toggleMarks() {
    marksOn = !marksOn;
    overlay.set(marksOn);
    try { globalThis.localStorage?.setItem(MARKS_KEY, String(marksOn)); } catch { /* ignored */ }
    marksButton.setAttribute("aria-pressed", String(marksOn));
    announce(i18n.t(marksOn ? "para-status-marks-on" : "para-status-marks-off"), false);
  }

  // ---- controls ---------------------------------------------------------------------------------------------------------------
  const controls = [];
  const named = (button, label) => { button.setAttribute("aria-label", label); button.title = label; return button; };
  function iconButton(className, label, icon, onActivate) {
    const button = make(document, "button", `para-button ${className}`);
    button.type = "button"; button.append(icon); named(button, label);
    button.addEventListener("click", () => { if (button.getAttribute("aria-disabled") !== "true") onActivate(); });
    controls.push(button);
    return button;
  }
  const alignNames = ["left", "center", "right", "justify"];
  const alignLabels = { left: i18n.t("para-align-left"), center: i18n.t("para-align-center"), right: i18n.t("para-align-right"), justify: i18n.t("para-align-justify") };
  const alignStatus = { left: i18n.t("para-status-align-left"), center: i18n.t("para-status-align-center"), right: i18n.t("para-status-align-right"), justify: i18n.t("para-status-align-justify") };
  const alignButtons = Object.fromEntries(alignNames.map(name => {
    const button = iconButton("para-align", alignLabels[name], barsIcon(document, ALIGN_ICONS[name]), () => align(name, alignStatus[name]));
    button.dataset.align = name; button.setAttribute("aria-pressed", "false");
    return [name, button];
  }));
  const indentDown = iconButton("para-indent", i18n.t("para-indent-decrease"), barsIcon(document, [[0, 16], [6, 10], [6, 10], [0, 16]], "M4 4.5 L1.5 7 L4 9.5"), () => indent(false));
  const indentUp = iconButton("para-indent", i18n.t("para-indent-increase"), barsIcon(document, [[0, 16], [6, 10], [6, 10], [0, 16]], "M1.5 4.5 L4 7 L1.5 9.5"), () => indent(true));

  // Line spacing and spacing menu.
  const spacingButton = make(document, "button", "para-button para-spacing");
  spacingButton.type = "button"; spacingButton.setAttribute("aria-haspopup", "menu"); spacingButton.setAttribute("aria-expanded", "false");
  spacingButton.append(barsIcon(document, [[5, 11], [5, 11], [5, 11]], "M2 3 L2 11 M0.5 4.5 L2 3 L3.5 4.5 M0.5 9.5 L2 11 L3.5 9.5"));
  named(spacingButton, i18n.t("para-spacing"));
  controls.push(spacingButton);
  const lineLabels = [i18n.t("para-line-1"), i18n.t("para-line-115"), i18n.t("para-line-15"), i18n.t("para-line-2"), i18n.t("para-line-25"), i18n.t("para-line-3")];
  const menuItems = [
    ...LINE_VALUES.map((value, index) => ({ value: `line:${value}`, label: lineLabels[index] })),
    { value: "options", label: i18n.t("para-line-options") },
    { value: "space-before", label: i18n.t("para-space-before-add") },
    { value: "space-after", label: i18n.t("para-space-after-add") },
  ];
  const menu = createContextMenu({ label: i18n.t("para-spacing-menu"), items: menuItems, onSelect: choice => {
    if (choice.startsWith("line:")) { const value = Number(choice.slice(5)); lineSpacing("auto", value, i18n.t("para-status-line", { value: lineLabels[LINE_VALUES.indexOf(value)] })); }
    else if (choice === "options") openLineDialog();
    else if (choice === "space-before") spaceBefore();
    else if (choice === "space-after") spaceAfter();
  } });
  menu.popup.classList.add("para-spacing-menu");
  document.body.append(menu.popup);
  const menuButtons = [...menu.popup.querySelectorAll(".ui-menu-item")];
  const shortcuts = { 0: "Control+1", 2: "Control+5", 3: "Control+2", [LINE_VALUES.length + 1]: "Control+0" };
  for (const [index, keys] of Object.entries(shortcuts)) menuButtons[index].setAttribute("aria-keyshortcuts", keys);
  LINE_VALUES.forEach((_, index) => { menuButtons[index].setAttribute("role", "menuitemradio"); menuButtons[index].setAttribute("aria-checked", "false"); });
  spacingButton.addEventListener("click", () => {
    if (spacingButton.getAttribute("aria-disabled") === "true") return;
    const rect = spacingButton.getBoundingClientRect();
    menu.open({ x: rect.left, y: rect.bottom + 2, returnFocusTo: spacingButton });
    spacingButton.setAttribute("aria-expanded", "true");
  });
  const observer = new MutationObserver(() => spacingButton.setAttribute("aria-expanded", String(!menu.popup.hidden)));
  observer.observe(menu.popup, { attributes: true, attributeFilter: ["hidden"] });

  // Shading and borders: no model field yet (US-243-M1): disabled, with the reason.
  function unavailable(id, label, reason) {
    const button = make(document, "button", "para-button para-unavailable", label);
    button.type = "button"; button.id = `para-${id}`;
    button.setAttribute("aria-disabled", "true"); button.title = reason;
    const note = make(document, "span", "", reason); note.id = `para-${id}-reason`; note.hidden = true;
    button.append(note); button.setAttribute("aria-describedby", note.id);
    button.addEventListener("click", event => { event.preventDefault(); announce(reason, false); });
    return button;
  }
  const shadingButton = unavailable("shading", i18n.t("para-shading"), i18n.t("para-shading-reason"));
  const bordersButton = unavailable("borders", i18n.t("para-borders"), i18n.t("para-borders-reason"));

  const marksButton = make(document, "button", "para-button para-marks", "¶");
  marksButton.type = "button"; named(marksButton, i18n.t("para-marks"));
  marksButton.setAttribute("aria-pressed", String(marksOn));
  marksButton.addEventListener("click", () => { if (marksButton.getAttribute("aria-disabled") !== "true") toggleMarks(); });

  // ---- "Line spacing options" dialog -----------------------------------------------------------------------------------------
  const kinds = [
    { id: "single", label: i18n.t("para-rule-single"), rule: "auto", fixed: 240 }, { id: "one-half", label: i18n.t("para-rule-15"), rule: "auto", fixed: 360 },
    { id: "double", label: i18n.t("para-rule-double"), rule: "auto", fixed: 480 },
    { id: "at-least", label: i18n.t("para-rule-atleast"), rule: "atLeast", unit: "pt", min: 0.05, max: 1584 },
    { id: "exact", label: i18n.t("para-rule-exact"), rule: "exact", unit: "pt", min: 0.05, max: 1584 },
    { id: "multiple", label: i18n.t("para-rule-multiple"), rule: "auto", unit: "lines", min: 0.0625, max: 10 },
  ];
  const dialogBody = make(document, "div", "para-line-dialog");
  const typeLabel = make(document, "label", "para-line-field", i18n.t("para-line-dialog-type"));
  const typeSelect = make(document, "select", "para-line-type");
  for (const kind of kinds) { const option = make(document, "option", "", kind.label); option.value = kind.id; typeSelect.append(option); }
  typeLabel.append(typeSelect);
  const valueLabel = make(document, "label", "para-line-field", i18n.t("para-line-dialog-value"));
  const valueInput = make(document, "input", "para-line-value"); valueInput.type = "number"; valueInput.step = "any";
  const unitText = make(document, "span", "para-line-unit");
  valueLabel.append(valueInput, unitText);
  const invalid = make(document, "p", "para-line-invalid", i18n.t("para-dialog-invalid")); invalid.hidden = true; invalid.setAttribute("role", "alert");
  const applyButton = createButton({ label: i18n.t("para-dialog-apply"), className: "para-line-apply", onActivate: () => applyDialog() });
  const cancelButton = createButton({ label: i18n.t("para-dialog-cancel"), onActivate: () => dialog.close() });
  const actions = make(document, "div", "para-line-actions"); actions.append(applyButton.element, cancelButton.element);
  dialogBody.append(typeLabel, valueLabel, invalid, actions);
  const dialog = createDialog({ label: i18n.t("para-line-dialog-title"), content: dialogBody, closeLabel: i18n.t("action-close"), initialFocus: () => typeSelect });
  dialog.element.classList.add("para-line-dialog-root"); document.body.append(dialog.element);
  const currentKind = () => kinds.find(kind => kind.id === typeSelect.value);
  function syncDialogFields() {
    const kind = currentKind();
    const fixed = kind.fixed !== undefined;
    valueLabel.hidden = fixed; invalid.hidden = true;
    if (!fixed) {
      unitText.textContent = i18n.t(kind.unit === "pt" ? "para-unit-pt" : "para-unit-lines");
      valueInput.min = String(kind.min); valueInput.max = String(kind.max);
    }
  }
  typeSelect.addEventListener("change", () => {
    const kind = currentKind();
    if (kind.fixed === undefined && !Number.isFinite(valueInput.valueAsNumber)) valueInput.value = kind.unit === "pt" ? "12" : "1";
    syncDialogFields();
  });
  function openLineDialog() {
    const line = controller.snapshot().state?.paragraph.lineSpacing;
    let kind = kinds[0]; let value = "";
    if (line?.state === "value") {
      const { rule, value: raw } = line.value;
      if (rule === "auto") kind = kinds.find(k => k.fixed === raw) ?? { ...kinds[5] };
      else kind = kinds.find(k => k.rule === rule);
      if (kind.fixed === undefined) value = String(kind.unit === "lines" ? raw / 240 : raw / 20);
    }
    typeSelect.value = kind.id; valueInput.value = value || (kind.unit === "pt" ? "12" : "1");
    syncDialogFields();
    dialog.open({ returnFocusTo: spacingButton });
  }
  function applyDialog() {
    const kind = currentKind();
    let value = kind.fixed;
    if (value === undefined) {
      const number = valueInput.valueAsNumber;
      if (!Number.isFinite(number) || number < kind.min || number > kind.max) { invalid.hidden = false; valueInput.focus(); return; }
      value = Math.round(number * (kind.unit === "pt" ? 20 : 240));
      if (kind.rule === "auto" && (value < 15 || value > 2400)) { invalid.hidden = false; valueInput.focus(); return; }
    }
    const shown = kind.fixed === undefined ? `${kind.label} ${valueInput.value} ${unitText.textContent}` : kind.label;
    dialog.close(); // restores the focus to the opener; the command then gives it to the document
    lineSpacing(kind.rule, value, i18n.t("para-status-line", { value: shown }));
  }

  // ---- state -> controls --------------------------------------------------------------------------------------------------------
  const setDisabled = (button, disabled) => { button.disabled = disabled; button.setAttribute("aria-disabled", String(disabled)); };
  function render(snapshot) {
    if (disposed) return;
    const can = canFormat(snapshot) && !blocked;
    const paragraph = snapshot.state?.paragraph;
    for (const name of alignNames) {
      const alignment = paragraph?.alignment;
      alignButtons[name].setAttribute("aria-pressed", String(Boolean(can && alignment?.state === "value" && alignment.value === name)));
    }
    for (const button of controls) setDisabled(button, !can);
    const usable = Boolean(snapshot.state) && !snapshot.busy && !blocked;
    setDisabled(marksButton, !usable);
    if (!can) menu.close({ restoreFocus: false });
    const line = paragraph?.lineSpacing;
    LINE_VALUES.forEach((value, index) => {
      const checked = line?.state === "value" && line.value.rule === "auto" && line.value.value === value;
      menuButtons[index].setAttribute("aria-checked", String(checked));
      menuButtons[index].textContent = `${checked ? "✓ " : ""}${lineLabels[index]}`;
    });
    const label = (attr, add, remove) => (paragraph?.[attr]?.state === "value" && paragraph[attr].value > 0 ? remove : add);
    menuButtons[LINE_VALUES.length + 1].textContent = label("spaceBefore", i18n.t("para-space-before-add"), i18n.t("para-space-before-remove"));
    menuButtons[LINE_VALUES.length + 2].textContent = label("spaceAfter", i18n.t("para-space-after-add"), i18n.t("para-space-after-remove"));
  }
  const unsubscribe = controller.subscribe(render);

  // ---- public ---------------------------------------------------------------------------------------------------------------------
  const KEY_BINDINGS = [
    { key: "l", ctrl: true, run: () => align("left", alignStatus.left) }, { key: "e", ctrl: true, run: () => align("center", alignStatus.center) },
    { key: "r", ctrl: true, run: () => align("right", alignStatus.right) }, { key: "j", ctrl: true, run: () => align("justify", alignStatus.justify) },
    { key: "m", ctrl: true, run: () => indent(true) }, { key: "m", ctrl: true, shift: true, run: () => indent(false) },
    { code: "Digit1", ctrl: true, run: () => lineSpacing("auto", 240, i18n.t("para-status-line", { value: lineLabels[0] })) },
    { code: "Digit2", ctrl: true, run: () => lineSpacing("auto", 480, i18n.t("para-status-line", { value: lineLabels[3] })) },
    { code: "Digit5", ctrl: true, run: () => lineSpacing("auto", 360, i18n.t("para-status-line", { value: lineLabels[2] })) },
    { code: "Digit0", ctrl: true, run: () => spaceBefore() },
    { code: "Digit8", ctrl: true, shift: true, run: () => { if (marksButton.getAttribute("aria-disabled") !== "true") toggleMarks(); } },
  ];

  render(controller.snapshot());
  return Object.freeze({
    groups({ command }) {
      return [{ id: "para-paragraph", label: i18n.t("ribbon-paragraph"), commands: [
        command("para-align-left", "para-align-left", "G", alignButtons.left), command("para-align-center", "para-align-center", "N", alignButtons.center),
        command("para-align-right", "para-align-right", "D", alignButtons.right), command("para-align-justify", "para-align-justify", "J", alignButtons.justify),
        command("para-indent-decrease", "para-indent-decrease", "M", indentDown), command("para-indent-increase", "para-indent-increase", "O", indentUp),
        command("para-spacing", "para-spacing", "L", spacingButton),
        command("para-shading", "para-shading", undefined, shadingButton), command("para-borders", "para-borders", undefined, bordersButton),
        command("para-marks", "para-marks", "Q", marksButton),
      ] }];
    },
    setBlocked(value) { blocked = Boolean(value); render(controller.snapshot()); },
    keyBindings() { return KEY_BINDINGS; },
    attachEditor(editor, options) { overlay.attach(editor, options); },
    detachEditor() { overlay.detach(); },
    /** Surcouche (tests) : `{ active, drawn, element }`. */
    get marks() { return { active: overlay.active, drawn: overlay.drawn, element: overlay.element }; },
    dispose() {
      if (disposed) return;
      disposed = true; unsubscribe(); observer.disconnect(); overlay.dispose(); menu.dispose(); dialog.dispose();
      applyButton.dispose(); cancelButton.dispose();
      for (const node of [...controls, shadingButton, bordersButton, marksButton, menu.popup, dialog.element]) node.remove();
    },
  });
}
