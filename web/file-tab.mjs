// SPDX-License-Identifier: AGPL-3.0-or-later
// Onglet Fichier et accueil (US-320) : nouveau document et modèles embarqués, documents récents du natif
// (épingler, retirer, fichier introuvable signalé) et commandes du document. Le module ne connaît ni le
// moteur ni le DOM de l'application : l'appelant fournit `openDocument(source)` (celui de l'éditeur riche) et
// reçoit les résultats par `onResult`. Aucun chemin ne part vers le natif : toute action désigne un
// récent par son numéro (voir `recentOpen`, `recentPin`, `recentRemove` de native-host.mjs).
import { RICH_LIMITS } from "./rich-editor-protocol.mjs";

/** Modèles fournis (data/templates, ressources de l'exe) : même ordre que dans le ruban. */
export const TEMPLATES = Object.freeze([
  Object.freeze({ id: "letter", path: "/templates/lettre.docx", label: "file-template-letter", keyTip: "L" }),
  Object.freeze({ id: "cv", path: "/templates/cv.docx", label: "file-template-cv", keyTip: "V" }),
  Object.freeze({ id: "report", path: "/templates/rapport.docx", label: "file-template-report", keyTip: "A" }),
  Object.freeze({ id: "minutes", path: "/templates/compte-rendu.docx", label: "file-template-minutes", keyTip: "M" }),
]);

/** Messages du catalogue dont l'onglet a besoin ; l'un d'eux manque, l'application garde l'onglet d'origine. */
export const FILE_TAB_KEYS = Object.freeze([
  "file-group-new", "file-group-document", "file-group-recent",
  ...TEMPLATES.map(template => template.label),
  "file-save", "file-export-pdf", "file-print", "file-options",
  "file-save-unavailable", "file-export-pdf-unavailable", "file-print-unavailable", "file-options-unavailable",
  "file-template-opened", "file-template-error",
  "file-recent-label", "file-recent-empty", "file-recent-loading", "file-recent-error", "file-recent-no-host",
  "file-recent-pin", "file-recent-remove", "file-recent-pin-label", "file-recent-remove-label",
  "file-recent-open-label", "file-recent-open-missing-label", "file-recent-missing",
  "file-recent-missing-status", "file-recent-pinned-status", "file-recent-unpinned-status", "file-recent-removed-status",
  "status-cancelled", "files.error.not_found",
]);

const make = (document, tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

/** Nom du fichier : dernier élément d'un chemin Windows (ou POSIX). */
export function baseName(path) {
  const parts = String(path).split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : String(path);
}

/**
 * @param {{ document: Document, i18n: { t: Function, has: Function, formatDateTime: Function },
 *   onStatus?: (text: string, isError?: boolean) => void,
 *   onResult?: (result: object, success?: string) => void,
 *   fetchTemplate?: (path: string) => Promise<Response> }} options
 */
export function createFileTab({ document, i18n, onStatus = () => {}, onResult = () => {}, fetchTemplate = path => fetch(path, { cache: "no-store" }) } = {}) {
  if (!document || !i18n) throw new TypeError("document and i18n are required.");
  for (const key of FILE_TAB_KEYS) if (!i18n.has(key)) throw new TypeError(`Missing file tab message: ${key}.`);
  const t = (key, params) => i18n.t(key, params);

  let host = null;
  let openDocument = null;
  let blocked = true;
  let disposed = false;
  let entries = [];
  let refreshToken = 0;
  let lastTicket = null;

  const templateButtons = new Map();
  for (const template of TEMPLATES) {
    const button = make(document, "button", "", t(template.label));
    button.type = "button";
    button.id = `file-template-${template.id}`;
    button.disabled = true;
    button.dataset.template = template.id;
    templateButtons.set(template.id, button);
  }

  /** Commande visible mais sans moteur : focalisable, annoncée avec sa raison, qui dit pourquoi au clic. */
  const unavailable = (id, labelKey, reasonKey) => {
    const button = make(document, "button", "", t(labelKey));
    button.type = "button";
    button.id = id;
    button.dataset.unavailable = "true";
    button.setAttribute("aria-disabled", "true");
    button.title = t(reasonKey);
    const reason = make(document, "span", "", t(reasonKey));
    reason.id = `${id}-reason`;
    reason.hidden = true;
    button.append(reason);
    button.setAttribute("aria-describedby", reason.id);
    button.addEventListener("click", event => { event.preventDefault(); onStatus(t(reasonKey), false); });
    return button;
  };
  const saveButton = unavailable("file-save", "file-save", "file-save-unavailable");
  const pdfButton = unavailable("file-export-pdf", "file-export-pdf", "file-export-pdf-unavailable");
  const printButton = unavailable("file-print", "file-print", "file-print-unavailable");
  const optionsButton = unavailable("file-options", "file-options", "file-options-unavailable");

  const recentRoot = make(document, "div", "file-recent");
  recentRoot.id = "file-recent";
  recentRoot.setAttribute("role", "group");
  recentRoot.setAttribute("aria-label", t("file-recent-label"));
  const note = make(document, "p", "file-recent-note", t("file-recent-loading"));
  note.setAttribute("role", "status");
  note.tabIndex = -1;
  const list = make(document, "ul", "file-recent-list");
  list.hidden = true;
  recentRoot.append(note, list);

  function renderNote(text, isError = false) {
    entries = [];
    list.replaceChildren();
    list.hidden = true;
    note.hidden = false;
    note.textContent = text;
    note.classList.toggle("error", isError);
  }

  function describe(entry) {
    const name = baseName(entry.path);
    const date = i18n.formatDateTime(new Date(entry.used * 1000));
    return { name, date };
  }

  /** Redessine la liste ; `focus` redonne le focus au contrôle `{ id, part }` (ou à son voisin si l'entrée a disparu). */
  function render(focus = null, previousOrder = []) {
    list.replaceChildren();
    if (entries.length === 0) { renderNote(t("file-recent-empty")); if (focus) note.focus(); return; }
    note.hidden = true;
    list.hidden = false;
    for (const entry of entries) {
      const { name, date } = describe(entry);
      const item = make(document, "li", "file-recent-item");
      item.dataset.recentId = String(entry.id);
      item.dataset.pinned = String(entry.pinned);
      item.dataset.missing = String(!entry.present);
      const open = make(document, "button", "file-recent-open");
      open.type = "button";
      open.disabled = blocked;
      open.setAttribute("aria-label", entry.present
        ? t("file-recent-open-label", { name, path: entry.path, date })
        : t("file-recent-open-missing-label", { name, path: entry.path, date }));
      open.title = entry.path;
      open.append(make(document, "span", "file-recent-name", name), make(document, "span", "file-recent-path", entry.path),
        make(document, "span", "file-recent-date", date));
      if (!entry.present) open.append(make(document, "span", "file-recent-missing", t("file-recent-missing")));
      open.addEventListener("click", () => { void openRecent(entry); });
      const pin = make(document, "button", "file-recent-pin", t("file-recent-pin"));
      pin.type = "button";
      pin.disabled = blocked;
      pin.setAttribute("aria-pressed", String(entry.pinned));
      pin.setAttribute("aria-label", t("file-recent-pin-label", { name }));
      pin.addEventListener("click", () => { void togglePin(entry); });
      const remove = make(document, "button", "file-recent-remove", t("file-recent-remove"));
      remove.type = "button";
      remove.disabled = blocked;
      remove.setAttribute("aria-label", t("file-recent-remove-label", { name }));
      remove.addEventListener("click", () => { void removeRecent(entry); });
      item.append(open, pin, remove);
      list.append(item);
    }
    if (!focus) return;
    const ids = entries.map(entry => entry.id);
    let target = ids.includes(focus.id) ? focus.id : null;
    let part = focus.part;
    if (target === null) {
      // L'entrée a disparu : le focus passe à l'entrée qui a pris sa place, sinon à la précédente.
      const index = previousOrder.indexOf(focus.id);
      target = ids[Math.min(index < 0 ? 0 : index, ids.length - 1)];
      part = "open";
    }
    list.querySelector(`[data-recent-id="${target}"] .file-recent-${part}`)?.focus();
  }

  async function refreshRecents(focus = null, previousOrder = []) {
    const token = ++refreshToken;
    if (!host || typeof host.recentList !== "function") { renderNote(t("file-recent-no-host")); return; }
    let result;
    try { result = await host.recentList(); } catch { result = { kind: "error" }; }
    if (disposed || token !== refreshToken) return;
    if (result.kind !== "ok") { renderNote(t("file-recent-error"), true); return; }
    entries = result.entries;
    render(focus, previousOrder);
  }

  const failure = key => (typeof key === "string" && i18n.has(key) ? t(key) : t("file-recent-error"));

  async function togglePin(entry) {
    if (blocked || !host) return;
    const { name } = describe(entry);
    const order = entries.map(item => item.id);
    let result;
    try { result = await host.recentPin(entry.id, !entry.pinned); } catch { result = { kind: "error" }; }
    if (result.kind === "ok") onStatus(t(entry.pinned ? "file-recent-unpinned-status" : "file-recent-pinned-status", { name }), false);
    else onStatus(failure(result.key), true);
    await refreshRecents({ id: entry.id, part: "pin" }, order);
  }

  async function removeRecent(entry) {
    if (blocked || !host) return;
    const { name } = describe(entry);
    const order = entries.map(item => item.id);
    let result;
    try { result = await host.recentRemove(entry.id); } catch { result = { kind: "error" }; }
    if (result.kind === "ok") onStatus(t("file-recent-removed-status", { name }), false);
    else onStatus(failure(result.key), true);
    await refreshRecents({ id: entry.id, part: "open" }, order);
  }

  /** Lecture d'un récent comme le dialogue d'ouverture : ticket de lecture du natif, jamais un chemin. */
  async function readRecent(entry) {
    const picked = await host.recentOpen(entry.id);
    if (picked.kind !== "ok") return picked;
    lastTicket = picked.ticket;
    if (picked.size > RICH_LIMITS.maxOpenBytes) { void host.releaseTicket?.(picked.ticket); return { kind: "error", key: "files.error.too_large" }; }
    let identity = null;
    try { const known = await host.fileIdentity?.(picked.ticket); if (known?.kind === "ok") identity = known.identity; } catch { /* the file opens anyway */ }
    const read = await host.readTicket(picked.ticket, { copy: true });
    if (read.kind !== "ok") return read;
    return { kind: "ok", bytes: read.bytes, format: picked.format, name: picked.name, identity };
  }

  async function openRecent(entry) {
    if (blocked || !host || !openDocument) return { kind: "cancelled" };
    const { name } = describe(entry);
    const notFound = { kind: "error", key: "files.error.not_found" };
    if (!entry.present) {
      // Signalé introuvable : on relit d'abord la liste (le fichier a pu revenir), sans demander de confirmer l'abandon du document.
      await refreshRecents({ id: entry.id, part: "open" }, entries.map(item => item.id));
      if (!entries.find(item => item.id === entry.id)?.present) { onStatus(t("file-recent-missing-status", { name }), true); return notFound; }
    }
    let result;
    lastTicket = null;
    try { result = await openDocument(() => readRecent(entry)); }
    catch { result = { kind: "error", key: "status-open-error" }; }
    // Le document est accepté : seulement maintenant le natif note le fichier (remonte en tête de la liste).
    if (result.kind === "ok" && lastTicket) { try { await host.recentNote?.(lastTicket); } catch { /* le confort des récents ne gêne pas l'ouverture */ } }
    if (result.kind === "error" && result.key === "files.error.not_found") {
      onStatus(t("file-recent-missing-status", { name }), true);
      await refreshRecents({ id: entry.id, part: "open" }, entries.map(item => item.id));
      return result;
    }
    onResult(result);
    await refreshRecents();
    return result;
  }

  async function openTemplate(template) {
    if (blocked || !openDocument) return { kind: "cancelled" };
    const source = async () => {
      let response;
      try { response = await fetchTemplate(template.path); } catch { return { kind: "error", key: "file-template-error" }; }
      if (!response?.ok) return { kind: "error", key: "file-template-error" };
      // Un modèle s'ouvre comme un document sans nom : aucun nom, donc « Enregistrer sous » demande un emplacement.
      return { kind: "ok", bytes: new Uint8Array(await response.arrayBuffer()), format: "docx" };
    };
    let result;
    try { result = await openDocument(source); }
    catch { result = { kind: "error", key: "file-template-error" }; }
    onResult(result, t("file-template-opened", { name: t(template.label) }));
    return result;
  }

  for (const template of TEMPLATES) templateButtons.get(template.id).addEventListener("click", () => { void openTemplate(template); });

  const onWindowFocus = () => { if (host && !blocked) void refreshRecents(); };
  document.defaultView?.addEventListener("focus", onWindowFocus);

  return Object.freeze({
    element: recentRoot,
    /** Groupes du ruban pour l'onglet Fichier ; `existing` : commandes déjà portées par la page. */
    groups({ command, existing }) {
      const optional = (id, label, keyTip, element) => (element ? [command(id, label, keyTip, element)] : []);
      return [
        { id: "new", label: t("file-group-new"), commands: [
          ...optional("new", "action-new", "N", existing.new),
          ...TEMPLATES.map(template => command(`template-${template.id}`, template.label, template.keyTip, templateButtons.get(template.id))),
        ] },
        { id: "files", label: i18n.has("ribbon-files-group") ? t("ribbon-files-group") : t("file-group-document"), commands: [
          ...optional("open", "action-open", "O", existing.open),
          ...optional("open-docx", "action-open-docx", "D", existing.openDocx),
          command("save", "file-save", "E", saveButton),
          ...optional("save-as", "action-save", "S", existing.save),
          command("export-pdf", "file-export-pdf", "X", pdfButton),
          command("print", "file-print", "I", printButton),
        ] },
        { id: "document", label: t("file-group-document"), commands: [
          ...optional("properties", "propertiesTitle", "R", existing.properties),
          command("options", "file-options", "T", optionsButton),
          ...optional("about", "aboutTitle", "P", existing.about),
        ] },
        { id: "recent", label: t("file-group-recent"), commands: [{ id: "recent", label: t("file-recent-label"), element: recentRoot }] },
      ];
    },
    /** Branche l'hôte natif et l'éditeur ; liste les récents aussitôt. */
    attach({ host: nextHost, openDocument: nextOpen }) {
      host = nextHost ?? null;
      openDocument = nextOpen ?? null;
      return refreshRecents();
    },
    refreshRecents: () => refreshRecents(),
    /** Pendant une opération du document, les ouvertures sont refusées. */
    setBlocked(value) {
      blocked = Boolean(value);
      for (const button of templateButtons.values()) button.disabled = blocked;
      for (const button of list.querySelectorAll("button")) button.disabled = blocked;
    },
    openTemplate: id => { const template = TEMPLATES.find(item => item.id === id); return template ? openTemplate(template) : Promise.resolve({ kind: "error", key: "file-template-error" }); },
    get entries() { return entries.map(entry => ({ ...entry })); },
    dispose() {
      if (disposed) return;
      disposed = true;
      refreshToken += 1;
      document.defaultView?.removeEventListener("focus", onWindowFocus);
    },
  });
}
