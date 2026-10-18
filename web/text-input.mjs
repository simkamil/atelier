// SPDX-License-Identifier: AGPL-3.0-or-later

const KEY_COMMANDS = new Map([
  ["ArrowLeft", "left"], ["ArrowRight", "right"], ["ArrowUp", "up"], ["ArrowDown", "down"],
  ["Home", "home"], ["End", "end"], ["PageUp", "pageUp"], ["PageDown", "pageDown"],
]);

function copySelection(value) {
  if (value === undefined || value === null) return value;
  try { return structuredClone(value); }
  catch (error) { throw new TypeError("Selection must be structured-cloneable.", { cause: error }); }
}

function normalizedRect(rect) {
  if (!rect || typeof rect !== "object") throw new TypeError("caret must be a rectangle.");
  const x = rect.x ?? rect.left;
  const y = rect.y ?? rect.top;
  const width = rect.width;
  const height = rect.height;
  if (![x, y, width, height].every(Number.isFinite) || x < 0 || y < 0 || width < 0 || height < 0) {
    throw new TypeError("caret coordinates and dimensions must be finite and non-negative.");
  }
  return { x, y, width, height };
}

function inputSignature(type, data) { return `${type}\u0000${data ?? ""}`; }

function selectedPlainText(snapshot) {
  if (!snapshot || typeof snapshot.text !== "string" || typeof snapshot.revision !== "bigint"
      || !snapshot.selection || !Number.isInteger(snapshot.selection.anchor)
      || !Number.isInteger(snapshot.selection.focus)) return null;
  const start = Math.min(snapshot.selection.anchor, snapshot.selection.focus);
  const end = Math.max(snapshot.selection.anchor, snapshot.selection.focus);
  if (start === end) return null;
  const bytes = new TextEncoder().encode(snapshot.text);
  if (start < 0 || end > bytes.length) return null;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
    return decoder.decode(bytes.subarray(start, end));
  } catch { return null; }
}

function commandForInput(inputType) {
  if (inputType === "insertLineBreak" || inputType === "insertParagraph") return { type: "insertText", text: "\n" };
  if (["insertText", "insertReplacementText", "insertFromPaste", "insertFromDrop", "insertFromYank"].includes(inputType)) {
    return { type: "insertText" };
  }
  const match = /^delete(.+?)(Backward|Forward)$/.exec(inputType);
  if (match) {
    return {
      type: "delete",
      unit: match[1][0].toLowerCase() + match[1].slice(1),
      direction: match[2].toLowerCase(),
      inputType,
    };
  }
  return null;
}

/** Attach a browser-owned text input surface to an editor's selection and caret. */
export function attachTextInput({
  document: ownerDocument = globalThis.document,
  container,
  element,
  getSelection,
  getCaret,
  getClipboardSnapshot,
  onCommand,
  onComposition = () => {},
  onUnhandled,
  onError = () => {},
  onScreenReaderMode,
  screenReaderMode = false,
  label,
} = {}) {
  if (!ownerDocument?.createElement) throw new TypeError("A DOM document is required.");
  if (typeof getSelection !== "function" || typeof getCaret !== "function") {
    throw new TypeError("getSelection and getCaret callbacks are required.");
  }
  if (typeof onCommand !== "function" || typeof onComposition !== "function"
      || (getClipboardSnapshot !== undefined && typeof getClipboardSnapshot !== "function")
      || (onUnhandled !== undefined && typeof onUnhandled !== "function") || typeof onError !== "function"
      || (onScreenReaderMode !== undefined && typeof onScreenReaderMode !== "function")) {
    throw new TypeError("Input callbacks must be functions.");
  }
  const initialCaret = normalizedRect(getCaret());
  if (element !== undefined && (element?.tagName !== "TEXTAREA" || element.ownerDocument !== ownerDocument)) {
    throw new TypeError("element must be a textarea in the supplied document.");
  }

  const textarea = element ?? ownerDocument.createElement("textarea");
  const created = element === undefined;
  const originalAttributes = created ? null : [...textarea.attributes].map(attribute => [attribute.name, attribute.value]);
  const originalValue = created ? "" : textarea.value;
  const originalSelection = created ? null : [textarea.selectionStart, textarea.selectionEnd, textarea.selectionDirection];
  const originalParent = created ? null : textarea.parentNode;
  const originalNextSibling = created ? null : textarea.nextSibling;
  const target = container ?? ownerDocument.body;
  if (created) {
    if (!target?.append) throw new TypeError("container must accept DOM children.");
    // i18n-ignore: libellé par défaut d’un composant autonome ; l’éditeur transmet le texte du catalogue (input-label)
    textarea.setAttribute("aria-label", label || "Saisie du texte du document");
    textarea.setAttribute("autocomplete", "off");
    textarea.setAttribute("autocapitalize", "off");
    textarea.setAttribute("spellcheck", "false");
    textarea.tabIndex = 0;
    target.append(textarea);
  }
  const style = textarea.style;
  const originalStyle = created ? null : style.cssText;
  Object.assign(style, {
    position: "fixed", left: "0px", top: "0px", width: "1px", height: "1px",
    padding: "0", border: "0", margin: "0", opacity: "0", overflow: "hidden",
    resize: "none", pointerEvents: "none", zIndex: "-1",
  });
  style.left = `${initialCaret.x}px`;
  style.top = `${initialCaret.y}px`;

  let disposed = false;
  let composition = null;
  let nativeComposing = false;
  let suppressCompositionInput = null;
  let compositionSuppressTimer;
  let suppressPaste = null;
  let pasteSuppressTimer;
  let handledInput = null;
  let selectionErrorInput = null;
  let clearTimer;
  // Screen reader mode (US-209): the textarea holds the current paragraph and its selection instead of staying empty.
  let readerMode = false;
  let readerContent = null;

  function reportError(error) {
    try { onError(error); } catch { /* keep the input surface usable */ }
  }

  function clearValue() {
    textarea.value = "";
    try { textarea.setSelectionRange(0, 0); } catch { /* detached or unsupported */ }
  }

  /** Back to the resting content: empty, or the paragraph given to screen reader mode. */
  function resetValue() {
    if (!readerMode || !readerContent) { clearValue(); return; }
    const { text, start, end, backward } = readerContent;
    if (textarea.value !== text) textarea.value = text;
    try { textarea.setSelectionRange(start, end, start === end ? "none" : backward ? "backward" : "forward"); } catch { /* detached or unsupported */ }
  }

  /** `{ text, start, end, backward }`: start and end are UTF-16 offsets in `text`. Ignored content is kept for later. */
  function setScreenReaderContent(content) {
    if (disposed || !readerMode) return false;
    if (!content || typeof content.text !== "string" || !Number.isInteger(content.start) || !Number.isInteger(content.end)
        || content.start < 0 || content.end < content.start || content.end > content.text.length) {
      readerContent = null;
      if (!nativeComposing) clearValue();
      return false;
    }
    readerContent = { text: content.text, start: content.start, end: content.end, backward: content.backward === true };
    if (!nativeComposing) resetValue(); // a composition in progress keeps its text; the end of it resets
    return true;
  }

  function setScreenReaderMode(enabled) {
    const on = Boolean(enabled);
    if (disposed || on === readerMode) return;
    readerMode = on;
    readerContent = null;
    if (on) textarea.setAttribute("aria-multiline", "true");
    else textarea.removeAttribute("aria-multiline");
    if (!nativeComposing) clearValue();
    try { onScreenReaderMode?.(on); } catch (error) { reportError(error); }
  }

  function readSelection() { return copySelection(getSelection()); }

  function readCaret() { return normalizedRect(getCaret()); }

  function markHandledInput(signature) {
    handledInput = signature;
    setTimeout(() => { if (handledInput === signature) handledInput = null; }, 0);
  }

  function scheduleClear() {
    clearTimeout(clearTimer);
    clearTimer = setTimeout(() => { if (!nativeComposing && !disposed) resetValue(); }, 0);
  }

  function emitCommand(command, selection = undefined) {
    if (disposed) return false;
    try {
      const currentSelection = copySelection(selection === undefined ? readSelection() : selection);
      onCommand(Object.freeze({ ...command, selection: currentSelection }));
      return true;
    } catch (error) {
      composition = null;
      scheduleClear();
      reportError(error);
      return false;
    }
  }

  function emitComposition(phase, state = composition) {
    if (!state || disposed) return true;
    try {
      onComposition(Object.freeze({
        phase, text: state.text, selection: copySelection(state.selection), caret: { ...state.caret },
      }));
      return true;
    } catch (error) {
      composition = null;
      scheduleClear();
      reportError(error);
      return false;
    }
  }

  function emitUnhandled(event, kind = "beforeinput") {
    if (disposed || !onUnhandled) return false;
    try {
      let selection;
      try { selection = readSelection(); } catch (error) { reportError(error); }
      onUnhandled(Object.freeze({
        kind,
        inputType: event.inputType ?? "",
        data: typeof event.data === "string" ? event.data : null,
        selection,
      }));
      return true;
    } catch (error) { reportError(error); return false; }
  }

  function updateComposition(text) {
    if (!composition || disposed || typeof text !== "string") return;
    composition.text = text;
    emitComposition("update");
  }

  function onKeyDown(event) {
    if (disposed || textarea.disabled || event.isComposing || event.keyCode === 229 || nativeComposing) return;
    // Ctrl+Alt+Z toggles screen reader mode. A real Z key reports "z"; AltGr+Z on a layout that maps it reports its character.
    if (event.ctrlKey && event.altKey && !event.shiftKey && !event.metaKey && (event.key === "z" || event.key === "Z")) {
      event.preventDefault();
      setScreenReaderMode(!readerMode);
      return;
    }
    if (event.key === "AltGraph" || event.getModifierState?.("AltGraph") || (event.ctrlKey && event.altKey)) return;
    const key = event.key;
    let command;
    if (event.ctrlKey && !event.altKey && !event.metaKey) {
      const normalizedKey = typeof key === "string" ? key.toLowerCase() : "";
      if (normalizedKey === "a" && !event.shiftKey) command = { type: "selectAll" };
      else if (normalizedKey === "z" && !event.shiftKey) command = { type: "undo" };
      else if (normalizedKey === "y" && !event.shiftKey || normalizedKey === "z" && event.shiftKey) command = { type: "redo" };
    }
    if (!command && !event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && key === "F8") {
      command = { type: "extensionMode", enabled: true };
    }
    if (!command && !event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && key === "Escape") {
      command = { type: "extensionMode", enabled: false };
    }
    if (!command && !event.altKey && !event.metaKey && KEY_COMMANDS.has(key)) {
      command = {
        type: "move", direction: KEY_COMMANDS.get(key),
        extend: Boolean(event.shiftKey), byWord: Boolean(event.ctrlKey),
        toDocumentEdge: Boolean(event.ctrlKey && (key === "Home" || key === "End")),
      };
    }
    if (command) {
      event.preventDefault();
      emitCommand(command);
    }
  }

  function onBeforeInput(event) {
    if (disposed) return;
    const inputType = event.inputType ?? "";
    const data = typeof event.data === "string" ? event.data : null;

    if (nativeComposing && (inputType === "insertCompositionText" || event.isComposing)) {
      if (data !== null) updateComposition(data);
      return;
    }
    if (suppressCompositionInput && inputType === "insertText" && data === suppressCompositionInput) {
      suppressCompositionInput = null;
      clearTimeout(compositionSuppressTimer);
      if (event.cancelable) event.preventDefault();
      markHandledInput(inputSignature(inputType, data));
      scheduleClear();
      return;
    }
    if (suppressPaste !== null && inputType === "insertFromPaste" && data === suppressPaste) {
      suppressPaste = null;
      clearTimeout(pasteSuppressTimer);
      if (event.cancelable) event.preventDefault();
      markHandledInput(inputSignature(inputType, data));
      scheduleClear();
      return;
    }

    const mapped = commandForInput(inputType);
    if (!mapped) {
      if (emitUnhandled(event)) markHandledInput(inputSignature(inputType, data));
      return;
    }
    let command = mapped;
    if (mapped.type === "insertText") {
      const text = mapped.text ?? data;
      if (text === null) {
        if (emitUnhandled(event)) markHandledInput(inputSignature(inputType, data));
        return;
      }
      command = { type: "insertText", text, inputType };
    }
    let selection;
    try { selection = readSelection(); }
    catch (error) {
      reportError(error);
      selectionErrorInput = inputSignature(inputType, data);
      return;
    }
    if (event.cancelable) event.preventDefault();
    emitCommand(command, selection);
    markHandledInput(inputSignature(inputType, data));
    if (!nativeComposing) scheduleClear();
  }

  function onInput(event) {
    if (disposed || nativeComposing || event.isComposing) return;
    const inputType = event.inputType ?? "";
    const data = typeof event.data === "string" ? event.data : textarea.value;
    const signature = inputSignature(inputType, data);
    if (selectionErrorInput === signature) {
      selectionErrorInput = null;
      if (emitUnhandled(event, "input")) scheduleClear();
      return;
    }
    if (handledInput === signature || handledInput === inputSignature(inputType, null)) {
      handledInput = null;
      scheduleClear();
      return;
    }
    if (suppressCompositionInput && inputType === "insertText" && data === suppressCompositionInput) {
      suppressCompositionInput = null;
      clearTimeout(compositionSuppressTimer);
      scheduleClear();
      return;
    }
    if (suppressPaste !== null && inputType === "insertFromPaste" && data === suppressPaste) {
      suppressPaste = null;
      clearTimeout(pasteSuppressTimer);
      scheduleClear();
      return;
    }
    const mapped = commandForInput(inputType);
    if (mapped) {
      const command = mapped.type === "insertText"
        ? { type: "insertText", text: mapped.text ?? data, inputType }
        : mapped;
      if (command.type !== "insertText" || typeof command.text === "string") emitCommand(command);
    } else if (emitUnhandled(event, "input")) {
      scheduleClear();
    } else {
      return; // Leave browser-managed text in the textarea when no delegate is installed.
    }
    if (mapped) scheduleClear();
  }

  function onCompositionStart() {
    if (disposed || nativeComposing) return;
    nativeComposing = true;
    try {
      composition = { text: "", selection: readSelection(), caret: readCaret() };
      emitComposition("start");
    } catch (error) {
      composition = null;
      scheduleClear();
      reportError(error);
    }
  }

  function onCompositionUpdate(event) {
    if (!composition || disposed) return;
    if (typeof event.data === "string") updateComposition(event.data);
  }

  function onCompositionEnd(event) {
    if (!nativeComposing || disposed) return;
    nativeComposing = false;
    if (!composition) { scheduleClear(); return; }
    const state = composition;
    const committedText = typeof event.data === "string" ? event.data : state.text;
    composition = null;
    if (committedText.length === 0) {
      emitComposition("cancel", state);
      scheduleClear();
      return;
    }
    state.text = committedText;
    suppressCompositionInput = committedText;
    clearTimeout(compositionSuppressTimer);
    compositionSuppressTimer = setTimeout(() => { suppressCompositionInput = null; }, 0);
    if (!emitComposition("commit", state)) return;
    if (!emitCommand({ type: "insertText", text: committedText, inputType: "insertCompositionText" }, state.selection)) return;
    scheduleClear();
  }

  function onBlur() {
    if (disposed || (!nativeComposing && !composition)) return;
    nativeComposing = false;
    const state = composition;
    composition = null;
    suppressCompositionInput = null;
    emitComposition("cancel", state);
    scheduleClear();
  }

  function onPaste(event) {
    if (disposed || textarea.disabled || nativeComposing) return;
    let text;
    try {
      if (event.clipboardData?.types && [...event.clipboardData.types].includes("text/plain")) {
        text = event.clipboardData.getData("text/plain");
      }
    } catch { /* delegate when clipboard access is restricted */ }
    if (typeof text !== "string") {
      emitUnhandled({ inputType: "insertFromPaste", data: null }, "paste");
      return;
    }
    event.preventDefault();
    suppressPaste = text;
    clearTimeout(pasteSuppressTimer);
    pasteSuppressTimer = setTimeout(() => { suppressPaste = null; }, 0);
    emitCommand({ type: "insertText", text, inputType: "insertFromPaste" });
    scheduleClear();
  }

  function onClipboard(event, cut = false) {
    if (disposed || textarea.disabled || !getClipboardSnapshot) return;
    // Once the editor owns clipboard snapshots, never let the browser copy transient textarea content.
    event.preventDefault();
    if (nativeComposing || composition || !event.clipboardData) return;
    let snapshot;
    try { snapshot = getClipboardSnapshot(); }
    catch (error) { reportError(error); return; }
    const text = selectedPlainText(snapshot);
    if (text === null) return;
    try { event.clipboardData.setData("text/plain", text); }
    catch { return; }
    if (cut) emitCommand({ type: "deleteSelection", selection: snapshot.selection, revision: snapshot.revision });
  }

  function onCopy(event) { onClipboard(event, false); }
  function onCut(event) { onClipboard(event, true); }

  for (const [type, listener] of [
    ["keydown", onKeyDown], ["beforeinput", onBeforeInput], ["input", onInput],
    ["compositionstart", onCompositionStart], ["compositionupdate", onCompositionUpdate],
    ["compositionend", onCompositionEnd], ["paste", onPaste], ["blur", onBlur],
    ["copy", onCopy], ["cut", onCut],
  ]) textarea.addEventListener(type, listener);

  function updateCaret(rect = undefined) {
    if (disposed) return false;
    try {
      const caret = rect === undefined ? readCaret() : normalizedRect(rect);
      style.left = `${caret.x}px`;
      style.top = `${caret.y}px`;
      return true;
    } catch (error) {
      reportError(error);
      throw error;
    }
  }

  function focus() {
    if (disposed) return false;
    if (ownerDocument.querySelector?.("dialog[open]")) return false;
    try {
      updateCaret();
      textarea.focus({ preventScroll: true });
      return ownerDocument.activeElement === textarea;
    } catch (error) {
      reportError(error);
      return false;
    }
  }

  function dispose() {
    if (disposed) return;
    if (composition) {
      const state = composition;
      composition = null;
      emitComposition("cancel", state);
    }
    nativeComposing = false;
    disposed = true;
    clearTimeout(clearTimer);
    clearTimeout(compositionSuppressTimer);
    clearTimeout(pasteSuppressTimer);
    for (const [type, listener] of [
      ["keydown", onKeyDown], ["beforeinput", onBeforeInput], ["input", onInput],
      ["compositionstart", onCompositionStart], ["compositionupdate", onCompositionUpdate],
      ["compositionend", onCompositionEnd], ["paste", onPaste], ["blur", onBlur],
      ["copy", onCopy], ["cut", onCut],
    ]) textarea.removeEventListener(type, listener);
    clearValue();
    if (created) textarea.remove();
    else {
      for (const attribute of [...textarea.attributes]) textarea.removeAttribute(attribute.name);
      for (const [name, value] of originalAttributes) textarea.setAttribute(name, value);
      if (originalStyle !== null) textarea.style.cssText = originalStyle;
      textarea.value = originalValue;
      if (originalSelection) {
        try { textarea.setSelectionRange(...originalSelection); } catch { /* original type no longer supports selection */ }
      }
      if (originalParent && textarea.parentNode !== originalParent) {
        originalParent.insertBefore(textarea, originalNextSibling?.parentNode === originalParent ? originalNextSibling : null);
      }
    }
  }

  if (screenReaderMode) setScreenReaderMode(true);

  return Object.freeze({ element: textarea, focus, updateCaret, setScreenReaderMode, setScreenReaderContent, screenReaderMode: () => readerMode, dispose });
}
