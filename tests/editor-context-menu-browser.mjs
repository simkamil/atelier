// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEditorContextMenu } from "../web/editor-context-menu.mjs";

const key = (target, name, extra = {}) => target.dispatchEvent(new KeyboardEvent("keydown", {
  key: name, bubbles: true, cancelable: true, ...extra,
}));
const click = target => target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

export async function run({ assert = (condition, message) => { if (!condition) throw new Error(message); }, fixture } = {}) {
  const root = typeof fixture === "function" ? await fixture() : fixture;
  const host = root instanceof Element ? root : document.createElement("div");
  if (!root) document.body.append(host);
  const target = document.createElement("div");
  target.tabIndex = 0;
  const focusTarget = document.createElement("button");
  focusTarget.type = "button";
  host.append(target, focusTarget);
  let state = { revision: 7, selection: { anchor: 3, focus: 1 }, text: "hello", canUndo: true, canRedo: false, busy: false, enabled: true };
  const commands = [], errors = [];
  const menu = createEditorContextMenu({ target, focusTarget,
    labels: { menu: "Document actions", undo: "Undo", redo: "Redo", cut: "Cut", copy: "Copy safely", paste: "Paste", selectAll: "Select all" },
    getState: () => state,
    clipboardCapabilities: { read: true, write: true },
    onCommand: async (action, snapshot) => { commands.push([action, snapshot]); if (action === "cut") throw new Error("clipboard denied"); },
    onError: error => errors.push(error),
  });
  host.append(menu.element);
  const checks = [];
  const verify = (condition, message) => { assert(condition, message); checks.push(message); };
  try {
    const original = structuredClone(state);
    const contextEvent = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 30, clientY: 40 });
    target.dispatchEvent(contextEvent);
    verify(contextEvent.defaultPrevented && !menu.element.hidden, "right-click opens an accessible menu and prevents the browser menu");
    verify(JSON.stringify(state) === JSON.stringify(original), "opening preserves revision and the active selection");
    verify(menu.element.getAttribute("aria-label") === "Document actions" && menu.element.textContent.includes("Copy safely"), "injected labels are rendered as text");
    const items = [...menu.element.querySelectorAll('[role="menuitem"]')];
    verify(!items[0].hasAttribute("aria-disabled") && items[1].getAttribute("aria-disabled") === "true"
      && !items[2].hasAttribute("aria-disabled") && !items[3].hasAttribute("aria-disabled"), "undo, redo, cut, and copy reflect the opening snapshot and capabilities");

    key(focusTarget, "ContextMenu");
    const keyboardPopup = menu.element;
    const nativeDuplicate = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 35, clientY: 45 });
    target.dispatchEvent(nativeDuplicate);
    verify(nativeDuplicate.defaultPrevented && menu.element === keyboardPopup, "native contextmenu paired with a keyboard open does not create a second popup");
    menu.close();
    const closedKeyboardPopup = menu.element;
    target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 50, clientY: 60 }));
    verify(!menu.element.hidden && menu.element !== closedKeyboardPopup, "right-click immediately after closing a keyboard menu opens a fresh popup");

    state = { ...state, revision: 8, selection: { anchor: 0, focus: 0 }, canUndo: false, canRedo: true };
    key(focusTarget, "F10", { shiftKey: true });
    const refreshed = menu.element;
    verify(!refreshed.hidden && refreshed !== null && refreshed.querySelectorAll('[role="menuitem"]')[0].getAttribute("aria-disabled") === "true"
      && !refreshed.querySelectorAll('[role="menuitem"]')[1].hasAttribute("aria-disabled")
      && refreshed.querySelectorAll('[role="menuitem"]')[2].getAttribute("aria-disabled") === "true", "each opening recalculates action availability from current state");
    const disabledCut = refreshed.querySelectorAll('[role="menuitem"]')[2];
    click(disabledCut);
    verify(commands.length === 0, "an artificially activated disabled action never reaches the command callback");
    click(refreshed.querySelectorAll('[role="menuitem"]')[4]);
    await Promise.resolve();
    verify(commands[0]?.[0] === "paste" && commands[0][1].revision === 8, "keyboard opening sends the captured revision with the action");

    await new Promise(resolve => setTimeout(resolve, 60));
    state = { revision: 9, selection: { anchor: 2, focus: 0 }, text: "hello", canUndo: true, canRedo: true, busy: false, enabled: true };
    target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 20 }));
    click(menu.element.querySelectorAll('[role="menuitem"]')[2]);
    await Promise.resolve(); await Promise.resolve();
    verify(errors.length === 1 && errors[0].message === "clipboard denied", "asynchronous clipboard failure is reported without component mutation");
    verify(commands.at(-1)?.[1].revision === 9, "the command receives the revision captured for its own menu opening");

    state = { ...state, busy: true };
    target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    verify([...menu.element.querySelectorAll('[role="menuitem"]')].every(item => item.getAttribute("aria-disabled") === "true"), "busy state disables all editor actions");
    key(focusTarget, "ContextMenu");
    verify(!menu.element.hidden, "ContextMenu key opens the menu on the focused editor surface");
    menu.close();
    verify(document.activeElement === focusTarget, "closing returns focus to the configured editor input");

    menu.dispose();
    const previous = menu.element;
    key(focusTarget, "F10", { shiftKey: true });
    verify(previous?.parentNode !== host && menu.element === null, "dispose removes the menu and keyboard listener");
    return { checks };
  } finally {
    menu.dispose();
    host.remove();
  }
}
