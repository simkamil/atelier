// SPDX-License-Identifier: AGPL-3.0-or-later
// R7c: the editor utilities (symbols, NBSP shortcut, context menu, clipboard, About) through the running product with the v2
// editor: the same scenario as editor-utilities-integration-browser.mjs, reading the text through `editor.readText()`.
import { ABOUT_LICENSES } from "../web/about-licenses.mjs";
import { BUILD_INFO } from "../web/build-info.mjs";

const tick = () => new Promise(resolve => setTimeout(resolve, 0));
const stateText = app => app.editor.readText();
const sameSelection = (left, right) => left.anchor === right.anchor && left.focus === right.focus;

/** Verify editor utilities through the running app and its real Worker/WASM. */
export async function checkEditorUtilitiesV2({ app, assert, waitFor, key, inputText, openText }) {
  if (!app?.editor || !app.symbols || !app.about || !app.contextMenu) throw new TypeError("Editor utility app is not ready.");
  const editorInput = app.editor.element;
  const canvas = document.querySelector("#document-canvas");
  const clickCommand = id => {
    const button = document.getElementById(id);
    if (!button) throw new Error(`Missing command button #${id}.`);
    button.click();
    return button;
  };
  const menuItem = action => app.contextMenu.element.querySelectorAll('[role="menuitem"]')[
    ["undo", "redo", "cut", "copy", "paste", "selectAll"].indexOf(action)
  ];
  const selectAll = async () => {
    editorInput.focus(); key(editorInput, "a", { ctrlKey: true });
    await waitFor(async () => app.getState().anchor !== app.getState().focus, "editor selection");
    return app.getState();
  };
  const openContextByMouse = async () => {
    await new Promise(resolve => setTimeout(resolve, 60));
    canvas.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 20 }));
    if (app.contextMenu.element.hidden) throw new Error("Context menu did not open from canvas contextmenu.");
  };
  const activateContextAction = action => {
    const button = menuItem(action);
    if (!button) throw new Error(`Missing context menu action ${action}.`);
    button.click();
  };

  const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  let clipboardText = "";
  let writeFailure = null;
  let holdWrite = null;
  let writeStarted;
  let resolvePendingWrite;
  let staleStatusObserver = null;
  const clipboard = {
    async writeText(text) {
      writeStarted?.();
      if (holdWrite) await holdWrite.promise;
      if (writeFailure) throw writeFailure;
      clipboardText = text;
    },
    async readText() { return clipboardText; },
  };
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard });

  try {
    await openText("AéZ");
    const original = app.getState();
    await selectAll();
    clickCommand("symbols-command");
    const search = app.symbols.element.querySelector("[data-symbol-search]");
    assert(app.symbols.element.open && document.activeElement === search, "Symbols command opens the searchable picker");
    search.value = "U+00A9";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await waitFor(async () => app.symbols.element.querySelector('[role="gridcell"][data-symbol-codepoint="169"]'), "copyright symbol search result");
    app.symbols.element.querySelector('[role="gridcell"][data-symbol-codepoint="169"]').click();
    app.symbols.element.querySelector("[data-symbol-insert]").click();
    await waitFor(async () => (await stateText(app)) === "©", "symbol insertion through Worker/WASM");
    assert((await stateText(app)) === "©" && app.getState().revision > original.revision, "selected multibyte UTF-8 text is replaced by the copyright scalar");
    await app.editor.undo();
    assert((await stateText(app)) === "AéZ", "one undo restores the complete pre-insertion text");
    await app.editor.redo();
    assert((await stateText(app)) === "©", "one redo reapplies the complete symbol insertion");

    editorInput.focus();
    key(editorInput, " ", { ctrlKey: true, shiftKey: true, code: "Space" });
    await waitFor(async () => (await stateText(app)) === "©\u00a0", "Ctrl+Shift+Space inserts NBSP through Worker/WASM");
    assert((await stateText(app)) === "©\u00a0", "Ctrl+Shift+Space inserts a real non-breaking space");

    const beforeComposition = (await stateText(app));
    editorInput.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
    await tick();
    key(editorInput, " ", { ctrlKey: true, shiftKey: true, code: "Space", isComposing: true });
    clickCommand("symbols-command");
    assert((await stateText(app)) === beforeComposition && !app.symbols.element.open, "IME composition suppresses symbol and NBSP editor commands");
    editorInput.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" }));
    await waitFor(async () => app.editor.canEdit, "editor exits composition");

    const selectionForContext = await selectAll();
    await openContextByMouse();
    assert(sameSelection(selectionForContext, app.getState()), "right-click context menu preserves the editor selection");
    const contextCopy = menuItem("copy");
    const contextCut = menuItem("cut");
    assert(contextCopy && !contextCopy.disabled && !contextCopy.hasAttribute("aria-disabled")
      && contextCut && !contextCut.disabled && !contextCut.hasAttribute("aria-disabled"), "context menu enables copy and cut for a nonempty selection with clipboard support");
    app.contextMenu.close();
    key(editorInput, "ContextMenu");
    assert(!app.contextMenu.element.hidden && sameSelection(selectionForContext, app.getState()), "ContextMenu key opens without changing selection anchors");
    app.contextMenu.close();
    key(editorInput, "F10", { shiftKey: true });
    assert(!app.contextMenu.element.hidden && sameSelection(selectionForContext, app.getState()), "Shift+F10 opens without changing selection anchors");
    app.contextMenu.close();

    await selectAll();
    await openContextByMouse();
    activateContextAction("undo");
    await waitFor(async () => (await stateText(app)) === "©", "context Undo action through Worker/WASM");
    assert((await stateText(app)) === "©", "context Undo runs the editor history command");
    await openContextByMouse();
    activateContextAction("redo");
    await waitFor(async () => (await stateText(app)) === "©\u00a0", "context Redo action through Worker/WASM");
    assert((await stateText(app)) === "©\u00a0", "context Redo runs the editor history command");

    writeFailure = new Error("clipboard write rejected");
    await selectAll();
    await openContextByMouse(); activateContextAction("cut");
    await waitFor(async () => document.querySelector("#editor-status")?.classList.contains("error"), "clipboard failure status");
    assert((await stateText(app)) === "©\u00a0", "rejected clipboard write leaves the selected document text unchanged");
    writeFailure = null;

    await openText("copy me");
    await selectAll();
    await openContextByMouse(); activateContextAction("copy");
    await waitFor(async () => clipboardText === "copy me", "context copy writes selected plain text");
    assert(clipboardText === "copy me" && (await stateText(app)) === "copy me", "copy writes exact text without mutating the document");
    clipboardText = "paste ✓";
    await openContextByMouse(); activateContextAction("paste");
    await waitFor(async () => (await stateText(app)) === "paste ✓", "context paste reads text/plain and replaces selection");
    assert((await stateText(app)) === "paste ✓", "paste inserts the clipboard text through the editor transaction");

    await openText("race");
    await selectAll();
    const beforeStaleCut = app.getState();
    const editorStatus = document.querySelector("#editor-status");
    let resolveStaleStatusMutation;
    const staleStatusMutation = new Promise(resolve => { resolveStaleStatusMutation = resolve; });
    staleStatusObserver = new MutationObserver(records => {
      if (records.length && editorStatus.classList.contains("error")) resolveStaleStatusMutation();
    });
    staleStatusObserver.observe(editorStatus, { childList: true, characterData: true, subtree: true });
    holdWrite = { promise: new Promise(resolve => { resolvePendingWrite = resolve; }) };
    let started;
    const began = new Promise(resolve => { started = resolve; });
    writeStarted = started;
    await openContextByMouse(); activateContextAction("cut");
    await began;
    key(editorInput, "ArrowLeft");
    await waitFor(async () => app.getState().anchor === app.getState().focus, "selection changes while clipboard write is pending");
    resolvePendingWrite(); resolvePendingWrite = null; holdWrite = null; writeStarted = null;
    await staleStatusMutation;
    staleStatusObserver.disconnect(); staleStatusObserver = null;
    assert((await stateText(app)) === "race" && app.getState().revision === beforeStaleCut.revision, "async cut refuses its stale snapshot without deleting text");

    await openText("stale");
    await selectAll();
    const staleSnapshot = app.getState();
    key(editorInput, "ArrowLeft");
    await waitFor(async () => app.getState().focus !== staleSnapshot.focus, "caret moves away from editor snapshot");
    let staleEditRejected = false;
    try { await app.editor.editSelection("!", staleSnapshot); }
    catch { staleEditRejected = true; }
    assert(staleEditRejected && (await stateText(app)) === "stale", "editor rejects editSelection after the selection changes from its snapshot");

    // A modal dialog suppresses symbol-picker activation at the application command boundary.
    app.about.open({ returnFocusTo: editorInput });
    assert(app.about.element.open, "About command opens its modal dialog");
    clickCommand("symbols-command");
    assert(!app.symbols.element.open, "the symbol picker stays closed while another modal is open");

    const aboutTabs = [...app.about.element.querySelectorAll('[role="tab"]')];
    const aboutPanels = [...app.about.element.querySelectorAll('[role="tabpanel"]')];
    const codes = [...app.about.element.querySelectorAll(".about-dialog-metadata-value code")].map(node => node.textContent);
    assert(codes.includes(BUILD_INFO.version), "About displays the exact build version");
    const sourceCode = codes.find(value => value === BUILD_INFO.sourceUrl);
    assert(sourceCode && !app.about.element.querySelector("a[href]"), "About displays the exact source URL as selectable text without navigation");
    const displayedLicenses = [...app.about.element.querySelectorAll(".about-dialog-license-text")].map(node => node.textContent);
    const expectedLicenses = [ABOUT_LICENSES.agpl, ...ABOUT_LICENSES.fonts.map(font => font.text), ABOUT_LICENSES.unicode];
    assert(displayedLicenses.length === expectedLicenses.length && expectedLicenses.every((value, index) => displayedLicenses[index] === value), "About displays the complete AGPL, font, and Unicode license texts exactly as bundled");
    assert(aboutPanels.length === aboutTabs.length, "About connects every license tab to a content panel");
    key(app.about.element, "Escape");
    await waitFor(async () => !app.about.element.open, "About dialog closes");
    assert(app.editor.canEdit && document.activeElement === editorInput, "closing About restores the editable input focus");
    const resumed = app.getState();
    const at = Math.min(resumed.anchor, resumed.focus), end = Math.max(resumed.anchor, resumed.focus);
    const before = new TextEncoder().encode(await stateText(app));
    const expected = new TextDecoder().decode(before.subarray(0, at)) + "!" + new TextDecoder().decode(before.subarray(end));
    inputText(editorInput, "!");
    await waitFor(async () => (await stateText(app)) === expected, "typing resumes at the actual caret after About closes");
    assert((await stateText(app)) === expected, "About consultation preserves ordinary text editing");

  } finally {
    app.symbols.close();
    app.about.close();
    app.contextMenu.close({ restoreFocus: false });
    if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
    else delete navigator.clipboard;
    staleStatusObserver?.disconnect();
    resolvePendingWrite?.();
    await openText("");
  }
}
