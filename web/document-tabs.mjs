// SPDX-License-Identifier: AGPL-3.0-or-later
// Barre d'onglets de documents (US-327). Le module ne connaît ni le moteur ni les éditeurs : il affiche la liste des
// documents, signale ce que l'utilisateur demande (activer, fermer, réordonner) et le reste à `app.mjs`.
//
// Accessibilité : `role="tablist"` ; chaque onglet est un bouton `role="tab"` (`aria-selected`, `aria-controls` vers le
// panneau du document, tabindex itinérant) ; son nom contient le titre du document et « modifié » ; un bouton Fermer
// distinct (hors de l'ordre de tabulation : Ctrl+W et Suppr ferment l'onglet actif au clavier).
// Clavier : flèches gauche/droite, Début, Fin (changent l'onglet, activation automatique) ; Suppr ou Ctrl+W ferment ;
// Alt+Maj+flèche gauche/droite déplace l'onglet (équivalent clavier du glisser) ; Ctrl+Tab / Ctrl+Maj+Tab depuis n'importe
// où, Ctrl+F4 comme Ctrl+W (`handleKeydown`). Glisser : pointeur, seuil de 6 px, l'onglet suit la souris dans la barre ;
// Échap annule ; un déplacement abouti appelle `onReorder(ids)`.
//
// API :
//   const tabs = createDocumentTabs({ document, container, t, onActivate(id), onClose(id), onReorder(ids, movedId) })
//   tabs.add({ id, title, dirty, panelId })   tabs.update(id, { title, dirty, error })   tabs.remove(id)
//   tabs.setActive(id)   tabs.move(id, index)   tabs.order()   tabs.activeId   tabs.size
//   tabs.focusTab(id)    tabs.handleKeydown(event) -> true si le raccourci a été traité
// `t(key, values)` : clés `tabs.list`, `tabs.close`, `tabs.modified`, `tabs.error`, `tabs.untitled` (voir fr-FR.txt, bloc US-327).

/** Clés du catalogue dont la barre a besoin. */
export const DOCUMENT_TAB_KEYS = Object.freeze(["tabs.list", "tabs.close", "tabs.modified", "tabs.error"]);

const DRAG_THRESHOLD = 6;

export function createDocumentTabs({ document: doc = globalThis.document, container, t, onActivate = () => {}, onClose = () => {}, onReorder = () => {} } = {}) {
  if (!container?.append || typeof t !== "function") throw new TypeError("container and t are required.");
  const list = doc.createElement("div");
  list.className = "document-tabs";
  list.setAttribute("role", "tablist");
  list.setAttribute("aria-label", t("tabs.list"));
  list.setAttribute("aria-orientation", "horizontal");
  container.append(list);

  /** id -> { id, item, tab, title, close, titleNode, dirtyNode, model } */
  const entries = new Map();
  let activeId = null;
  let disposed = false;
  let drag = null;
  let suppressClick = false;

  const items = () => [...list.querySelectorAll(":scope > .document-tab")];
  const order = () => items().map(node => Number(node.dataset.tabId));
  const entryOf = id => entries.get(id);

  function label(entry) {
    const { title, dirty, error } = entry.model;
    return [title, dirty ? t("tabs.modified") : "", error ? t("tabs.error") : ""].filter(Boolean).join(", ");
  }

  function paint(entry) {
    entry.titleNode.textContent = entry.model.title;
    entry.dirtyNode.hidden = !entry.model.dirty;
    entry.errorNode.hidden = !entry.model.error;
    entry.tab.setAttribute("aria-label", label(entry));
    entry.tab.title = entry.model.title;
    entry.close.setAttribute("aria-label", t("tabs.close", { name: entry.model.title }));
    entry.item.classList.toggle("dirty", Boolean(entry.model.dirty));
  }

  function refreshRoving() {
    for (const entry of entries.values()) {
      const selected = entry.id === activeId;
      entry.tab.setAttribute("aria-selected", String(selected));
      entry.tab.tabIndex = selected ? 0 : -1;
      entry.item.classList.toggle("active", selected);
    }
  }

  function activate(id, { focus = false } = {}) {
    if (!entries.has(id)) return;
    if (id !== activeId) { activeId = id; refreshRoving(); onActivate(id); }
    if (focus) entries.get(id)?.tab.focus();
  }

  function step(delta) {
    const ids = order();
    if (ids.length === 0) return null;
    const at = Math.max(0, ids.indexOf(activeId));
    return ids[(at + delta + ids.length) % ids.length];
  }

  function move(id, index) {
    const entry = entryOf(id);
    if (!entry) return false;
    const nodes = items().filter(node => node !== entry.item);
    const target = Math.max(0, Math.min(nodes.length, index));
    if (items().indexOf(entry.item) === target) return false;
    if (target >= nodes.length) list.append(entry.item); else nodes[target].before(entry.item);
    return true;
  }

  function keyOnTab(event, entry) {
    if (event.ctrlKey || event.metaKey) return;
    const ids = order();
    const at = ids.indexOf(entry.id);
    if (event.altKey && event.shiftKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      if (move(entry.id, at + (event.key === "ArrowLeft" ? -1 : 1))) { entry.tab.focus(); onReorder(order(), entry.id); }
      return;
    }
    if (event.altKey || event.shiftKey) return;
    let target = null;
    if (event.key === "ArrowRight") target = ids[(at + 1) % ids.length];
    else if (event.key === "ArrowLeft") target = ids[(at - 1 + ids.length) % ids.length];
    else if (event.key === "Home") target = ids[0];
    else if (event.key === "End") target = ids[ids.length - 1];
    else if (event.key === "Delete") { event.preventDefault(); onClose(entry.id); return; }
    if (target === null) return;
    event.preventDefault();
    activate(target, { focus: true });
  }

  // ---- drag to reorder ------------------------------------------------------------------------------------------------
  function dropIndex(x, dragged) {
    const others = items().filter(node => node !== dragged);
    let index = 0;
    for (const node of others) {
      const rect = node.getBoundingClientRect();
      if (x > rect.left + rect.width / 2) index += 1;
    }
    return index;
  }

  function onPointerMove(event) {
    if (!drag || event.pointerId !== drag.pointerId) return;
    if (!drag.moving) {
      if (Math.abs(event.clientX - drag.startX) < DRAG_THRESHOLD) return;
      drag.moving = true;
      drag.entry.item.classList.add("dragging");
    }
    move(drag.entry.id, dropIndex(event.clientX, drag.entry.item));
  }

  function endDrag(event, cancelled) {
    if (!drag || (event && event.pointerId !== drag.pointerId)) return;
    const { entry, moving, before } = drag;
    doc.removeEventListener("pointermove", onPointerMove, true);
    doc.removeEventListener("pointerup", onPointerUp, true);
    doc.removeEventListener("pointercancel", onPointerCancel, true);
    doc.removeEventListener("keydown", onDragKey, true);
    try { entry.tab.releasePointerCapture?.(drag.pointerId); } catch { /* synthetic pointers have no capture */ }
    drag = null;
    entry.item.classList.remove("dragging");
    if (!moving) return;
    suppressClick = true;
    setTimeout(() => { suppressClick = false; }, 0);
    if (cancelled) {
      list.append(...before.map(id => entryOf(id)?.item).filter(Boolean));
      return;
    }
    const after = order();
    if (after.join() !== before.join()) onReorder(after, entry.id);
  }
  const onPointerUp = event => endDrag(event, false);
  const onPointerCancel = event => endDrag(event, true);
  const onDragKey = event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); endDrag(null, true); } };

  function onPointerDown(event, entry) {
    if (event.button !== 0 || drag || items().length < 2) return;
    drag = { entry, pointerId: event.pointerId, startX: event.clientX, moving: false, before: order() };
    try { entry.tab.setPointerCapture?.(event.pointerId); } catch { /* synthetic pointers have no capture */ }
    doc.addEventListener("pointermove", onPointerMove, true);
    doc.addEventListener("pointerup", onPointerUp, true);
    doc.addEventListener("pointercancel", onPointerCancel, true);
    doc.addEventListener("keydown", onDragKey, true);
  }

  const api = {
    element: list,
    add({ id, title, dirty = false, panelId = null, error = false, index = null }) {
      if (disposed) throw new TypeError("The tab bar is disposed.");
      if (!Number.isInteger(id) || entries.has(id)) throw new TypeError("Tab id must be a new integer.");
      const item = doc.createElement("div");
      item.className = "document-tab";
      item.setAttribute("role", "presentation");
      item.dataset.tabId = String(id);
      const tab = doc.createElement("button");
      tab.type = "button";
      tab.className = "document-tab-label";
      tab.id = `document-tab-${id}`;
      tab.setAttribute("role", "tab");
      tab.setAttribute("aria-selected", "false");
      tab.tabIndex = -1;
      if (panelId) tab.setAttribute("aria-controls", panelId);
      const titleNode = doc.createElement("span");
      titleNode.className = "document-tab-title";
      const dirtyNode = doc.createElement("span");
      dirtyNode.className = "document-tab-dirty";
      dirtyNode.setAttribute("aria-hidden", "true");
      dirtyNode.textContent = "●";
      const errorNode = doc.createElement("span");
      errorNode.className = "document-tab-error";
      errorNode.setAttribute("aria-hidden", "true");
      errorNode.textContent = "!";
      tab.append(titleNode, dirtyNode, errorNode);
      const close = doc.createElement("button");
      close.type = "button";
      close.className = "document-tab-close";
      close.tabIndex = -1;
      close.textContent = "×";
      item.append(tab, close);
      const entry = { id, item, tab, close, titleNode, dirtyNode, errorNode, model: { title, dirty: Boolean(dirty), error: Boolean(error) } };
      entries.set(id, entry);
      if (index === null || index >= items().length) list.append(item); else items()[Math.max(0, index)].before(item);
      paint(entry);
      tab.addEventListener("click", () => { if (suppressClick) return; activate(id); });
      tab.addEventListener("keydown", event => keyOnTab(event, entry));
      tab.addEventListener("pointerdown", event => onPointerDown(event, entry));
      tab.addEventListener("auxclick", event => { if (event.button === 1) { event.preventDefault(); onClose(id); } });
      close.addEventListener("click", () => { if (!suppressClick) onClose(id); });
      refreshRoving();
      return tab;
    },
    update(id, changes) {
      const entry = entryOf(id);
      if (!entry) return;
      Object.assign(entry.model, Object.fromEntries(Object.entries(changes).filter(([key]) => key in entry.model)));
      paint(entry);
    },
    remove(id) {
      const entry = entryOf(id);
      if (!entry) return;
      entries.delete(id);
      entry.item.remove();
      if (activeId === id) activeId = null;
      refreshRoving();
    },
    /** Marque l'onglet actif sans appeler `onActivate` (l'application a déjà basculé). */
    setActive(id) {
      if (!entries.has(id)) return;
      activeId = id;
      refreshRoving();
    },
    move(id, index) { return move(id, index); },
    order,
    focusTab(id) { entryOf(id)?.tab.focus(); },
    get activeId() { return activeId; },
    get size() { return entries.size; },
    /** Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+W, Ctrl+F4 : vrai si l'événement a été traité (preventDefault déjà fait). */
    handleKeydown(event) {
      if (disposed || event.isComposing || event.altKey || event.metaKey || !event.ctrlKey) return false;
      if (doc.querySelector("dialog:modal")) return false;
      if (event.key === "Tab") {
        event.preventDefault();
        const target = step(event.shiftKey ? -1 : 1);
        if (target !== null && target !== activeId) activate(target);
        return true;
      }
      if (!event.shiftKey && (event.key.toLowerCase() === "w" || event.key === "F4")) {
        event.preventDefault();
        if (activeId !== null) onClose(activeId);
        return true;
      }
      return false;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      endDrag(null, true);
      entries.clear();
      list.remove();
    },
  };
  return Object.freeze(api);
}
