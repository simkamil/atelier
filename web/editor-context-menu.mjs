// SPDX-License-Identifier: AGPL-3.0-or-later
import { createContextMenu } from "./ui-components.mjs";

const ACTIONS = ["undo", "redo", "cut", "copy", "paste", "selectAll"];
const REQUIRED_LABELS = ["menu", "undo", "redo", "cut", "copy", "paste", "selectAll"];

function cloneSnapshot(state) {
  if (state === null || typeof state !== "object") throw new TypeError("getState must return an editor snapshot.");
  try { return structuredClone(state); }
  catch (error) { throw new TypeError("Editor snapshot must be structured-cloneable.", { cause: error }); }
}

function isSelected(snapshot) {
  const anchor = snapshot.selection?.anchor ?? snapshot.anchor;
  const focus = snapshot.selection?.focus ?? snapshot.focus;
  return Number.isFinite(anchor) && Number.isFinite(focus) && anchor !== focus;
}

function capability(value) { return typeof value === "function" ? Boolean(value()) : Boolean(value); }

/** Attach a snapshot-based TXT context menu to an editor input surface. */
export function createEditorContextMenu({
  target, focusTarget = target, labels = {}, getState, onCommand, onError = () => {}, clipboardCapabilities = {},
} = {}) {
  if (!target?.addEventListener || !focusTarget?.addEventListener) throw new TypeError("target and focusTarget are required DOM elements.");
  if (typeof getState !== "function" || typeof onCommand !== "function" || typeof onError !== "function") {
    throw new TypeError("getState, onCommand, and onError must be functions.");
  }
  const menuLabels = labels;
  for (const name of REQUIRED_LABELS) {
    if (typeof menuLabels[name] !== "string" || !menuLabels[name].trim()) throw new TypeError(`Missing editor context menu label: ${name}.`);
  }

  let context = null;
  let disposed = false;
  let suppressKeyboardContext = false;

  const reportError = error => { try { onError(error); } catch { /* keep menu usable */ } };
  const readSnapshot = () => cloneSnapshot(getState());
  const close = options => {
    suppressKeyboardContext = false;
    context?.menu.close(options);
  };

  function createFor(snapshot) {
    const blocked = snapshot.enabled === false || snapshot.busy === true || snapshot.preview === true;
    const selected = isSelected(snapshot);
    const canWrite = !blocked && capability(clipboardCapabilities.write);
    const canRead = !blocked && capability(clipboardCapabilities.read);
    const enabled = {
      undo: !blocked && Boolean(snapshot.canUndo),
      redo: !blocked && Boolean(snapshot.canRedo),
      cut: canWrite && selected,
      copy: canWrite && selected,
      paste: canRead,
      selectAll: !blocked && (snapshot.hasText === true || (typeof snapshot.text === "string" && snapshot.text.length > 0)),
    };
    const items = ACTIONS.map(action => ({ label: menuLabels[action], value: action, disabled: !enabled[action] }));
    const onSelect = action => {
      if (!enabled[action]) return;
      try {
        const result = onCommand(action, snapshot);
        Promise.resolve(result).catch(reportError);
      } catch (error) { reportError(error); }
    };

    const menu = createContextMenu({ label: menuLabels.menu, items, onSelect });
    menu.popup.dataset.editorContextMenu = "";
    return menu;
  }

  function open({ x, y } = {}) {
    if (disposed) return false;
    let snapshot;
    try { snapshot = readSnapshot(); }
    catch (error) { reportError(error); return false; }
    if (snapshot.composing || snapshot.composition) return false;

    const previous = context?.menu;
    const wasConnected = Boolean(previous?.popup.isConnected);
    let menu;
    try { menu = createFor(snapshot); }
    catch (error) { reportError(error); return false; }
    if (wasConnected) previous.popup.replaceWith(menu.popup);
    previous?.dispose();
    context = { menu, snapshot };
    const rect = focusTarget.getBoundingClientRect();
    menu.open({ x: x ?? rect.left, y: y ?? rect.bottom, returnFocusTo: focusTarget });
    return true;
  }

  const onContext = event => {
    if (event.target !== target && !target.contains?.(event.target)) return;
    if (event.isComposing) return;
    event.preventDefault();
    if (suppressKeyboardContext && context?.menu.popup.hidden === false) {
      suppressKeyboardContext = false;
      return;
    }
    suppressKeyboardContext = false;
    open({ x: event.clientX, y: event.clientY });
  };
  const onKey = event => {
    if (event.target !== focusTarget && !focusTarget.contains?.(event.target)) return;
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key !== "ContextMenu" && event.key !== "Menu" && !(event.key === "F10" && event.shiftKey)) return;
    event.preventDefault();
    suppressKeyboardContext = open();
  };

  target.addEventListener("contextmenu", onContext);
  focusTarget.addEventListener("keydown", onKey);
  // Provide a mountable hidden element without changing the editor's focus.
  try { const snapshot = readSnapshot(); context = { menu: createFor(snapshot), snapshot }; }
  catch (error) { reportError(error); }

  const api = {
    get element() { return context?.menu.popup ?? null; },
    open,
    close,
    dispose() {
      if (disposed) return;
      disposed = true;
      suppressKeyboardContext = false;
      target.removeEventListener("contextmenu", onContext);
      focusTarget.removeEventListener("keydown", onKey);
      context?.menu.dispose();
      context = null;
    },
  };
  return Object.freeze(api);
}
