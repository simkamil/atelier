// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTextInput } from "../web/text-input.mjs";

const tick = () => new Promise(resolve => setTimeout(resolve, 0));

function check(value, message) {
  if (!value) throw new Error(message);
}

function beforeInput(target, inputType, data = null, { cancelable = true, isComposing = false } = {}) {
  const event = new InputEvent("beforeinput", { inputType, data, bubbles: true, cancelable, isComposing });
  target.dispatchEvent(event);
  return event;
}

function input(target, inputType, data = null, { isComposing = false } = {}) {
  target.dispatchEvent(new InputEvent("input", { inputType, data, bubbles: true, isComposing }));
}

function composition(target, type, data = "") {
  target.dispatchEvent(new CompositionEvent(type, { data, bubbles: true }));
}

function attach({ selection = () => ({ anchor: 2, focus: 2 }), caret = () => ({ x: 15, y: 28, width: 1, height: 18 }), ...callbacks } = {}) {
  return attachTextInput({
    document,
    getSelection: selection,
    getCaret: caret,
    onCommand: callbacks.onCommand ?? (() => {}),
    onComposition: callbacks.onComposition,
    onUnhandled: callbacks.onUnhandled,
    onError: callbacks.onError,
  });
}

/** Run DOM input checks in a real browser document. Importing this module has no side effects. */
export async function runInputChecks() {
  const passed = [];
  const run = async (name, scenario) => {
    await scenario();
    passed.push(name);
  };

  await run("focus, caret placement, invalid rectangles, dialog focus, and lifecycle restoration", async () => {
    const commands = [];
    const errors = [];
    const adapter = attach({ onCommand: command => commands.push(command), onError: error => errors.push(error) });
    const textarea = adapter.element;
    check(textarea.isConnected && textarea.tagName === "TEXTAREA", "adapter creates a real textarea");
    check(!textarea.hasAttribute("aria-hidden") && textarea.getAttribute("aria-label") === "Saisie du texte du document", "created input is exposed to assistive technology with a French label");
    textarea.setAttribute("aria-label", "Saisie personnalisée");
    check(textarea.getAttribute("aria-label") === "Saisie personnalisée", "owner can replace the input label through element");
    check(textarea.style.position === "fixed" && textarea.style.left === "15px" && textarea.style.top === "28px", "textarea follows viewport caret coordinates");
    check(textarea.style.display !== "none" && textarea.style.visibility !== "hidden" && !textarea.isContentEditable, "input remains focusable and is not contenteditable");
    check(adapter.focus() && document.activeElement === textarea, "explicit focus reaches textarea");
    adapter.updateCaret({ left: 31, top: 44, width: 0, height: 16 });
    check(textarea.style.left === "31px" && textarea.style.top === "44px", "updateCaret accepts left/top viewport rectangles");
    let rejected = 0;
    for (const rect of [
      { x: Number.NaN, y: 0, width: 1, height: 1 },
      { x: -1, y: 0, width: 1, height: 1 },
    ]) {
      try { adapter.updateCaret(rect); } catch { rejected += 1; }
    }
    check(rejected === 2 && errors.length === 2, "non-finite and negative caret rectangles are rejected and reported");

    const dialog = document.createElement("dialog");
    dialog.setAttribute("open", "");
    const dialogButton = document.createElement("button");
    dialog.append(dialogButton);
    document.body.append(dialog);
    dialogButton.focus();
    check(!adapter.focus() && document.activeElement === dialogButton, "explicit focus does not steal focus from an open dialog");
    dialog.remove();
    adapter.dispose();
    check(!textarea.isConnected, "dispose removes an owned textarea");

    const host = document.createElement("div");
    const provided = document.createElement("textarea");
    const after = document.createElement("span");
    provided.setAttribute("aria-label", "original");
    provided.style.cssText = "position: absolute; color: red";
    provided.value = "keep";
    host.append(provided, after);
    document.body.append(host);
    const initialAttributes = [...provided.attributes].map(attribute => [attribute.name, attribute.value]);
    const initialStyle = provided.style.cssText;
    const providedAdapter = attach({ element: provided });
    providedAdapter.dispose();
    check(JSON.stringify([...provided.attributes].map(attribute => [attribute.name, attribute.value])) === JSON.stringify(initialAttributes), "provided textarea attributes are restored");
    check(provided.style.cssText === initialStyle && provided.value === "keep", "provided textarea styles and value are restored");
    check(provided.parentNode === host && provided.nextSibling === after, "provided textarea stays in its original position");
    host.remove();
  });

  await run("resolved text, shortcuts, navigation, and AltGraph preservation", async () => {
    const commands = [];
    const adapter = attach({ onCommand: command => commands.push(command) });
    const textarea = adapter.element;
    for (const text of ["é", "€", "@", "#", "{", "[", "à"]) {
      const event = beforeInput(textarea, "insertText", text);
      check(event.defaultPrevented, `resolved ${text} is accepted from beforeinput`);
      input(textarea, "insertText", text);
    }
    check(commands.filter(command => command.type === "insertText").map(command => command.text).join("") === "é€@#{[à", "resolved characters are committed once without key translation");

    const dispatchKey = init => {
      const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
      textarea.dispatchEvent(event);
      return event;
    };
    const before = commands.length;
    check(dispatchKey({ key: "z", ctrlKey: true }).defaultPrevented, "Ctrl+Z is routed");
    check(dispatchKey({ key: "y", ctrlKey: true }).defaultPrevented, "Ctrl+Y is routed");
    check(dispatchKey({ key: "ArrowLeft", shiftKey: true }).defaultPrevented, "selection navigation is routed");
    const altGr = dispatchKey({ key: "@", ctrlKey: true, altKey: true });
    const composing = dispatchKey({ key: "Process", isComposing: true });
    const legacyIme = dispatchKey({ key: "Unidentified", keyCode: 229 });
    check(!altGr.defaultPrevented && !composing.defaultPrevented && !legacyIme.defaultPrevented, "AltGr and composing key events stay with the browser");
    check(commands.slice(before).map(command => command.type).join() === "undo,redo,move", `only recognized shortcut and navigation commands are emitted: ${JSON.stringify(commands.slice(before))}`);
    adapter.dispose();
  });

  await run("non-cloneable owner selection reports through onError", async () => {
    const commands = [];
    const errors = [];
    const adapter = attach({
      selection: () => ({ anchor: 1, callback() {} }),
      onCommand: command => commands.push(command),
      onError: error => errors.push(error),
    });
    const event = new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true, cancelable: true });
    adapter.element.dispatchEvent(event);
    check(event.defaultPrevented && commands.length === 0, "uncloneable selection does not reach command callback");
    check(errors.length === 1 && errors[0].message.includes("structured-cloneable"), "uncloneable selection is reported through onError");
    adapter.dispose();
  });

  await run("editing commands, paste text/plain, and unsupported input delegation", async () => {
    const commands = [];
    const unhandled = [];
    const adapter = attach({ onCommand: command => commands.push(command), onUnhandled: value => unhandled.push(value) });
    const textarea = adapter.element;
    beforeInput(textarea, "insertLineBreak", null);
    beforeInput(textarea, "insertParagraph", null);
    beforeInput(textarea, "deleteContentBackward", null);
    beforeInput(textarea, "deleteContentForward", null);
    beforeInput(textarea, "deleteWordBackward", null, { cancelable: false });
    input(textarea, "deleteWordBackward", null);
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", { value: { types: ["text/plain"], getData: type => type === "text/plain" ? "collé €" : "" } });
    textarea.dispatchEvent(paste);
    beforeInput(textarea, "insertFromPaste", "collé €");
    input(textarea, "insertFromPaste", "collé €");
    const unknownPaste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(unknownPaste, "clipboardData", { value: { types: ["text/html"], getData: () => "<b>rich</b>" } });
    textarea.dispatchEvent(unknownPaste);
    beforeInput(textarea, "formatBold", null);
    input(textarea, "formatBold", null);
    check(commands.filter(command => command.type === "insertText").map(command => command.text).join() === "\n,\n,collé €", "line breaks and plain-text paste commit once");
    check(commands.filter(command => command.type === "delete").map(command => `${command.unit}:${command.direction}`).join() === "content:backward,content:forward,word:backward", "backward and forward deletion commands preserve their exact units");
    check(unhandled.some(value => value.kind === "paste" && value.inputType === "insertFromPaste"), "unsupported paste is explicitly delegated");
    check(unhandled.filter(value => value.inputType === "formatBold").length === 1, "unsupported beforeinput/input pair is delegated once");
    adapter.dispose();

    let delegateCalls = 0;
    let failOnce = true;
    const errors = [];
    const fallback = attach({
      onUnhandled: () => { delegateCalls += 1; if (failOnce) { failOnce = false; throw new Error("delegate failed"); } },
      onError: error => errors.push(error),
    });
    beforeInput(fallback.element, "formatBold", null);
    input(fallback.element, "formatBold", null);
    check(delegateCalls === 2 && errors.length === 1, "failed beforeinput delegation retries through matching input");
    fallback.dispose();

    const nativeFallback = attach();
    nativeFallback.element.value = "native fallback text";
    input(nativeFallback.element, "insertFromRichContent", null);
    check(nativeFallback.element.value === "native fallback text", "unknown input remains in the textarea when no delegate exists");
    nativeFallback.dispose();
  });

  await run("composition preview, stable selection, single commit, and cancellation", async () => {
    let currentSelection = { anchor: 3, focus: 5 };
    const commands = [];
    const previews = [];
    const adapter = attach({
      selection: () => currentSelection,
      onCommand: command => commands.push(command),
      onComposition: state => {
        previews.push(state);
        if (state.phase === "update") {
          state.selection.anchor = 900;
          state.caret.x = 900;
        }
      },
    });
    const textarea = adapter.element;
    composition(textarea, "compositionstart");
    currentSelection = { anchor: 90, focus: 90 };
    composition(textarea, "compositionupdate", "n");
    beforeInput(textarea, "insertCompositionText", "に", { isComposing: true });
    input(textarea, "insertCompositionText", "に", { isComposing: true });
    composition(textarea, "compositionend", "日本");
    beforeInput(textarea, "insertText", "日本");
    input(textarea, "insertText", "日本");
    check(previews.some(state => state.phase === "update" && state.text === "に"), "composition text is an uncommitted preview");
    check(previews.some(state => state.phase === "commit" && state.text === "日本" && state.caret.x === 15), "commit carries preview text and caret geometry");
    check(commands.length === 1 && commands[0].type === "insertText" && commands[0].text === "日本", "Chromium trailing insertText is suppressed after compositionend");
    check(JSON.stringify(commands[0].selection) === JSON.stringify({ anchor: 3, focus: 5 }), "composition commit uses the selection captured at compositionstart");

    composition(textarea, "compositionstart");
    composition(textarea, "compositionupdate", "draft");
    composition(textarea, "compositionend", "");
    check(commands.length === 1 && previews.at(-1).phase === "cancel", "empty compositionend cancels without insertion");

    composition(textarea, "compositionstart");
    composition(textarea, "compositionupdate", "sans input final");
    composition(textarea, "compositionend", "validé");
    check(commands.length === 2 && commands[1].text === "validé", "compositionend commits when no final input event arrives");
    await tick();
    beforeInput(textarea, "insertText", "validé");
    check(commands.length === 3 && commands[2].text === "validé", "a later legitimate insertion identical to the composition is not suppressed");
    adapter.dispose();
  });

  await run("focus changes, callback errors, and disposal during composition", async () => {
    const commands = [];
    const previews = [];
    const errors = [];
    let failPreview = false;
    let failCommand = false;
    const adapter = attach({
      onCommand: command => { if (failCommand) throw new Error("command callback failure"); commands.push(command); },
      onComposition: state => { previews.push(state); if (failPreview) throw new Error("preview callback failure"); },
      onError: error => errors.push(error),
    });
    const textarea = adapter.element;
    const other = document.createElement("input");
    document.body.append(other);
    adapter.focus();
    other.focus();
    await tick();
    check(document.activeElement === other, "focus moving to another field is not stolen");
    composition(textarea, "compositionstart");
    failPreview = true;
    composition(textarea, "compositionupdate", "broken");
    composition(textarea, "compositionend", "broken");
    check(errors.length === 1 && commands.length === 0, "callback failure clears the active composition without committing");
    failPreview = false;
    composition(textarea, "compositionstart");
    composition(textarea, "compositionupdate", "retry");
    failCommand = true;
    composition(textarea, "compositionend", "retry");
    beforeInput(textarea, "insertText", "retry");
    input(textarea, "insertText", "retry");
    failCommand = false;
    check(errors.length === 2 && commands.length === 0, "a failed composition command still suppresses Chromium's trailing insertion");
    composition(textarea, "compositionstart");
    adapter.focus();
    other.focus();
    check(previews.at(-1).phase === "cancel", "leaving the input during composition cancels the transaction without refocusing");
    check(document.activeElement === other, "composition blur does not steal focus back");
    adapter.focus();
    composition(textarea, "compositionstart");
    const countAtDispose = previews.length;
    adapter.dispose();
    check(previews.length === countAtDispose + 1 && previews.at(-1).phase === "cancel", "dispose reports composition cancellation once");
    const commandCount = commands.length;
    beforeInput(textarea, "insertText", "after dispose");
    composition(textarea, "compositionend", "late");
    check(commands.length === commandCount && !textarea.isConnected, "no callbacks occur after disposal and owned node is removed");
    other.remove();
  });

  return passed;
}
