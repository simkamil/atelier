// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEditor } from "../web/editor.mjs";
import { createEngineClient } from "../web/engine-client.mjs";
import { encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { checkEditorZoom, checkAppZoom } from "./zoom-browser.mjs";
import { checkNavigation } from "./navigation-browser.mjs";
import { run as checkReplacement } from "./replacement-browser.mjs";
import { checkDocumentStatus } from "./document-status-integration-browser.mjs";

const checks = [];
function assert(value, name) {
  if (!value) throw new Error(name);
  checks.push(name);
}
async function waitFor(predicate, name, timeoutMs = 5000) {
  const end = performance.now() + timeoutMs;
  while (performance.now() < end) {
    if (predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  throw new Error(`Timed out waiting for ${name}`);
}
function inputText(element, text) {
  element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: text, bubbles: true, cancelable: true }));
}
function key(element, name, modifiers = {}) {
  element.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...modifiers }));
}

const resultNode = document.querySelector("#result");
let client;
let editor;
let host;
let report;
const savedFiles = [];
const dirtyNotifications = [];
let nextOpen = { kind: "cancelled" };
let nextSave = { kind: "ok" };
let holdSave;
try {
  const wasmResponse = await fetch("/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm");
  assert(wasmResponse.ok, "fresh optimized WASM available");
  const worker = new Worker("/web/engine-worker.mjs", { type: "module" });
  client = await createEngineClient({ worker, bytes: await wasmResponse.arrayBuffer() });
  const ping = encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: new Uint8Array() });
  assert(decodeMessage(await client.request(ping), "command").type === MessageType.Ping, "legacy Ping echo remains exact");
  host = {
    async open() { return nextOpen; },
    async save(bytes) {
      savedFiles.push(bytes.slice());
      if (holdSave) { const pending = holdSave; holdSave = null; await pending.promise; }
      return nextSave;
    },
    async notifyDirty(dirty) { dirtyNotifications.push(dirty); return { kind: "ok" }; },
    close() {},
  };
  const canvas = document.querySelector("#document-canvas");
  const scrollport = document.querySelector("#document-scroll");
  const spacer = document.querySelector("#page-spacer");
  scrollport.style.height = "520px";
  canvas.style.width = "100%";
  editor = createEditor({ client, host, canvas, scrollport, spacer,
    textRepresentation: document.querySelector("#document-text"), confirmDiscard: "confirm" });
  const textRepresentation = document.querySelector("#document-text");
  const initial = await editor.initialize();
  assert(initial.kind === "state" && initial.text === "" && !initial.dirty && initial.pages.length === 1, "initial empty A4 state");
  assert(getComputedStyle(editor.element).opacity === "0", "input surface is hidden while Canvas remains the document view");
  const ratio = devicePixelRatio || 1;
  const pageX = Math.round(20 * ratio), sampleY = Math.round(20 * ratio);
  const outsideX = Math.round((595.276 + 20) * ratio);
  assert(canvas.width > outsideX, "editor Canvas exposes a grey area beside the A4 page");
  const composite = document.createElement("canvas");
  composite.width = canvas.width; composite.height = canvas.height;
  const compositeContext = composite.getContext("2d");
  compositeContext.fillStyle = getComputedStyle(scrollport).backgroundColor;
  compositeContext.fillRect(0, 0, composite.width, composite.height);
  compositeContext.drawImage(canvas, 0, 0);
  const pagePixel = compositeContext.getImageData(pageX, sampleY, 1, 1).data;
  const outsidePixel = compositeContext.getImageData(outsideX, sampleY, 1, 1).data;
  assert([...pagePixel].join(",") === "255,255,255,255", "A4 page background is opaque white in the real Canvas pixels");
  const expectedGrey = getComputedStyle(scrollport).backgroundColor.match(/\d+/g).slice(0, 3).join(",");
  assert([...outsidePixel].slice(0, 3).join(",") === expectedGrey && outsidePixel[3] === 255,
    "area outside the A4 page composites to the grey document-scroll background");

  inputText(editor.element, "A");
  inputText(editor.element, "é");
  inputText(editor.element, "B");
  await waitFor(() => editor.state?.text === "AéB", "ordered rapid input");
  assert(textRepresentation.textContent === "AéB", "accessible text representation updates with accented document text");
  assert(editor.state.dirty && editor.state.focus === 4, "UTF-8 selection follows rapid multibyte typing");
  key(editor.element, "ArrowLeft");
  await waitFor(() => editor.state?.focus === 3, "caret movement");
  key(editor.element, "ArrowLeft", { shiftKey: true });
  await waitFor(() => editor.state?.anchor === 3 && editor.state?.focus === 1, "directed selection");
  inputText(editor.element, "Z");
  await waitFor(() => editor.state?.text === "AZB", "selection replacement");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => editor.state?.text === "AéB", "undo through hidden input");
  key(editor.element, "y", { ctrlKey: true });
  await waitFor(() => editor.state?.text === "AZB", "redo through hidden input");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteContentBackward", bubbles: true, cancelable: true }));
  await waitFor(() => editor.state?.text === "AB", "grapheme backspace through hidden input");
  assert(editor.state.pages.every(page => page instanceof Uint8Array), "editor state contains ATDL pages");

  // ClipboardEvents are synthetic; the browser DOM and Worker/WASM stay real.
  async function openText(text) {
    window.confirm = () => true;
    nextOpen = { kind: "ok", bytes: new TextEncoder().encode(text) };
    await editor.openDocument();
  }
  const clipboard = (type, { text = "", fail = false } = {}) => {
    const written = [];
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: {
      types: ["text/plain", "text/html"],
      getData: format => format === "text/plain" ? text : "<script>bad()</script>",
      setData(format, value) { if (fail) throw new Error("denied"); written.push([format, value]); },
    } });
    editor.element.dispatchEvent(event);
    return { event, written };
  };
  const selectedText = () => new TextDecoder("utf-8", { ignoreBOM: true }).decode(
    new TextEncoder().encode(editor.state.text).subarray(Math.min(editor.state.anchor, editor.state.focus), Math.max(editor.state.anchor, editor.state.focus)));
  await openText("un café trois\r\nligne suivante");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "Ctrl+Right next word");
  key(editor.element, "ArrowRight", { ctrlKey: true, shiftKey: true });
  await waitFor(() => editor.state.anchor === 3 && editor.state.focus === 9, "Ctrl+Shift+Right word selection");
  assert(selectedText() === "café ", "word selection uses UTF-8 boundaries and includes the separating space");
  const beforeCopy = editor.state;
  const copied = clipboard("copy");
  assert(copied.event.defaultPrevented && copied.written.length === 1 && copied.written[0][0] === "text/plain"
    && copied.written[0][1] === "café " && editor.state.revision === beforeCopy.revision && !editor.state.dirty,
    "real model selection copies exact text synchronously without mutation");
  const deniedCut = clipboard("cut", { fail: true });
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(deniedCut.event.defaultPrevented && editor.state.text === beforeCopy.text && editor.state.revision === beforeCopy.revision,
    "failed clipboard write preserves the real model");
  const cut = clipboard("cut");
  assert(cut.written[0][1] === "café ", "cut writes before returning to the browser");
  await waitFor(() => editor.state.text === "un trois\r\nligne suivante", "cut range mutation");
  await editor.undo();
  assert(editor.state.text === beforeCopy.text && editor.state.anchor === 3 && editor.state.focus === 9,
    "one undo restores the cut text and its directed selection");
  key(editor.element, "v", { ctrlKey: true, shiftKey: true });
  clipboard("paste", { text: "é🙂" });
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertFromPaste", data: "é🙂", bubbles: true, cancelable: true }));
  await waitFor(() => editor.state.text === "un é🙂trois\r\nligne suivante", "plain paste replacement");
  assert(!editor.state.text.includes("<script>") && editor.state.text.split("é🙂").length === 2,
    "plain paste replaces selection once and ignores HTML");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === new TextEncoder().encode(editor.state.text).length, "Ctrl+A whole document");
  assert(selectedText() === editor.state.text, "Ctrl+A selects the real document rather than the hidden textarea");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 0, "document start before F8");
  key(editor.element, "F8");
  key(editor.element, "ArrowRight");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 2, "F8 extends successive movements");
  key(editor.element, "Escape");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.anchor === 2 && editor.state.focus === 2, "Escape ends extension mode");
  assert(editor.state.anchor === editor.state.focus, "Escape returns to ordinary caret navigation");

  key(editor.element, "F8");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === new TextEncoder().encode(editor.state.text).length,
    "Ctrl+A selects all while extension mode is active");
  key(editor.element, "ArrowLeft");
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 0,
    "Ctrl+A exits extension mode before the next caret movement");

  await openText("é abc\r\nxy🙂z");
  key(editor.element, "ArrowDown", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 8,
    "Ctrl+Down moves by paragraph using UTF-8 offsets");
  key(editor.element, "ArrowUp", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0,
    "Ctrl+Up moves by paragraph using UTF-8 offsets");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 3,
    "Ctrl+Right moves to the first word boundary in UTF-8 bytes");
  key(editor.element, "ArrowLeft", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0,
    "Ctrl+Left moves by word using grapheme-safe UTF-8 offsets");

  await openText("un café trois");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "word start before deletion");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteWordForward", bubbles: true, cancelable: true }));
  await waitFor(() => editor.state.text === "un trois", "Ctrl+Delete semantic input");
  await editor.undo();
  assert(editor.state.text === "un café trois", "word deletion is undone as a single edit in real WASM");

  await openText("un café trois");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0,
    "word deletion-backward starts at document beginning");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "word deletion-backward first word boundary");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 9, "caret after accented word uses UTF-8 bytes");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteWordBackward", bubbles: true, cancelable: true }));
  await waitFor(() => editor.state.text === "un trois" && editor.state.focus === 3,
    "Ctrl+Backspace deletes the preceding word through semantic beforeinput");
  await editor.undo();
  assert(editor.state.text === "un café trois" && editor.state.anchor === 3 && editor.state.focus === 9
    && selectedText() === "café ", "one undo restores the deleted word and original selection");

  // Pointer capture requires a trusted hardware event. Shim only capture in this synthetic harness.
  let capturedPointer = null;
  canvas.setPointerCapture = id => { capturedPointer = id; };
  canvas.hasPointerCapture = id => capturedPointer === id;
  canvas.releasePointerCapture = id => { if (capturedPointer === id) capturedPointer = null; };
  const pointer = (type, x, y, detail = 1, modifiers = {}) => canvas.dispatchEvent(new PointerEvent(type,
    { pointerId: 9, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : 1,
      clientX: x, clientY: y, detail, bubbles: true, cancelable: true, ...modifiers }));
  const caretPoint = () => { const r = canvas.getBoundingClientRect(); return { x: r.left + editor.state.caret.x + 0.2,
    y: r.top + editor.state.caret.y + 7 - scrollport.scrollTop }; };
  await openText("un café trois\r\nseconde ligne");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "word target for mouse");
  const wordPoint = caretPoint();
  pointer("pointerdown", wordPoint.x + 3, wordPoint.y, 2);
  pointer("pointerup", wordPoint.x + 3, wordPoint.y, 2);
  await waitFor(() => selectedText() === "café" && capturedPointer === null, "double-click selection");
  assert(selectedText() === "café", "double-click selects a whole accented word through engine unit selection");
  pointer("pointerdown", wordPoint.x + 3, wordPoint.y, 3);
  pointer("pointerup", wordPoint.x + 3, wordPoint.y, 3);
  await waitFor(() => selectedText() === "un café trois\r\n" && capturedPointer === null, "triple-click paragraph selection");
  assert(selectedText() === "un café trois\r\n", "triple-click selects the logical paragraph including CRLF");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 0, "document start before drag end lookup");
  key(editor.element, "End");
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 14, "line end before drag");
  const dragEnd = caretPoint();
  key(editor.element, "Home");
  await waitFor(() => editor.state.focus === 0, "line start before drag");
  const dragStart = caretPoint();
  pointer("pointerdown", dragStart.x, dragStart.y);
  pointer("pointermove", dragEnd.x, dragEnd.y);
  pointer("pointerup", dragEnd.x, dragEnd.y);
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 14 && capturedPointer === null, "drag final point");
  assert(selectedText() === "un café trois", "rapid down/move/up preserves the final drag range while WASM is asynchronous");
  assert(document.activeElement === editor.element, "completed mouse drag restores hidden input focus");
  await openText("AéBxyz");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 7, "drop target at line end");
  const dropEnd = caretPoint();
  key(editor.element, "Home");
  key(editor.element, "ArrowRight", { shiftKey: true });
  key(editor.element, "ArrowRight", { shiftKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 3, "UTF-8 source selection for move");
  const selectedRect = editor.state.selectionRects[0];
  const canvasBounds = canvas.getBoundingClientRect();
  const sourcePoint = { x: canvasBounds.left + selectedRect.x + 2,
    y: canvasBounds.top + selectedRect.y + 7 - scrollport.scrollTop };
  const dragRevision = editor.state.revision;
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  await waitFor(() => !document.querySelector(".text-drop-marker").hidden, "visible drag insertion marker");
  assert(editor.state.anchor === 0 && editor.state.focus === 3 && editor.state.revision === dragRevision,
    "drag position probe preserves the selection revision and text");
  assert(getComputedStyle(document.querySelector(".text-drop-marker")).width === "2px", "drop indicator is a visible two-pixel insertion line");
  pointer("pointerup", dropEnd.x, dropEnd.y);
  await waitFor(() => editor.state.text === "BxyzAé" && capturedPointer === null, "atomic selected text move");
  assert(document.querySelector(".text-drop-marker").hidden, "drop clears insertion marker");
  await editor.undo();
  assert(editor.state.text === "AéBxyz" && editor.state.anchor === 0 && editor.state.focus === 3,
    "one undo restores moved UTF-8 text and original directed selection");
  await editor.redo();
  assert(editor.state.text === "BxyzAé", "one redo reapplies the complete move");
  await editor.undo();
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  pointer("pointerup", dropEnd.x, dropEnd.y, 1, { ctrlKey: true });
  await waitFor(() => editor.state.text === "AéBxyzAé", "Ctrl selected text copy");
  await editor.undo();
  assert(editor.state.text === "AéBxyz", "Ctrl copy is a single undoable edit");
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  pointer("pointerup", dropEnd.x, dropEnd.y);
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(editor.state.text === "AéBxyz" && capturedPointer === null && document.querySelector(".text-drop-marker").hidden,
    "Escape cancels the complete drag without mutation or residual marker");
  const externalText = new DataTransfer();
  externalText.setData("text/plain", "🙂\r\n: <b>texte</b>");
  const over = new DragEvent("dragover", { clientX: dropEnd.x, clientY: dropEnd.y, dataTransfer: externalText, bubbles: true, cancelable: true });
  canvas.dispatchEvent(over);
  await waitFor(() => !document.querySelector(".text-drop-marker").hidden, "external text drop marker");
  const drop = new DragEvent("drop", { clientX: dropEnd.x, clientY: dropEnd.y, dataTransfer: externalText, bubbles: true, cancelable: true });
  canvas.dispatchEvent(drop);
  await waitFor(() => editor.state.text === "AéBxyz🙂\r\n: <b>texte</b>", "external plain text drop");
  assert(over.defaultPrevented && drop.defaultPrevented, "text drop prevents browser default handling");
  assert(new TextDecoder().decode((await editor.exportDocument()).bytes) === editor.state.text,
    "external drop exports exact Unicode CRLF and literal markup without typography correction");
  await editor.undo();
  assert(editor.state.text === "AéBxyz" && editor.state.anchor === 0 && editor.state.focus === 3,
    "one undo restores the original document and selection after external insertion");

  await openText("café");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 5, "French punctuation insertion point");
  inputText(editor.element, ":");
  await waitFor(() => editor.state.text === "café\u00a0:", "automatic NBSP before colon");
  assert(editor.state.focus === 8, "French correction keeps a UTF-8 caret after punctuation");
  await editor.undo();
  assert(editor.state.text === "café:", "immediate undo removes only automatic spacing and keeps raw punctuation");
  await editor.redo();
  assert(editor.state.text === "café\u00a0:", "redo restores automatic spacing");
  inputText(editor.element, "X");
  await waitFor(() => editor.state.text === "café\u00a0:X", "typing after French correction");
  await editor.undo();
  assert(editor.state.text === "café\u00a0:", "subsequent typing stays outside substitution undo group");
  editor.setFrenchSpacing(false);
  await openText("mot");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 3, "disabled spacing insertion point");
  inputText(editor.element, "?");
  await waitFor(() => editor.state.text === "mot?", "disabled spacing leaves raw text");
  assert(editor.state.text === "mot?", "French spacing can be disabled for the current editor");
  editor.setFrenchSpacing(true);
  await openText("");
  inputText(editor.element, "«");
  await waitFor(() => editor.state.text === "«\u00a0", "opening guillemet auto space");
  inputText(editor.element, "é");
  inputText(editor.element, "»");
  await waitFor(() => editor.state.text === "«\u00a0é\u00a0»", "closing guillemet auto space");
  assert(editor.state.text === "«\u00a0é\u00a0»", "French quote interiors use nonbreaking spaces");
  await openText("");
  clipboard("paste", { text: "mot: «texte»!?" });
  await waitFor(() => editor.state.text === "mot: «texte»!?", "paste bypasses French spacing");
  assert(editor.state.text === "mot: «texte»!?", "pasted punctuation and guillemets remain exact");
  await openText("mot");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 3, "IME punctuation insertion point");
  editor.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  editor.element.dispatchEvent(new CompositionEvent("compositionend", { data: ":", bubbles: true }));
  await waitFor(() => editor.state.text === "mot:", "IME punctuation bypasses French spacing");
  assert(editor.state.text === "mot:", "composition punctuation stays exact with automatic spacing enabled");
  await openText("motX");
  key(editor.element, "End");
  key(editor.element, "ArrowLeft", { shiftKey: true });
  await waitFor(() => editor.state.anchor === 4 && editor.state.focus === 3, "selection before punctuation replacement");
  inputText(editor.element, ":");
  await waitFor(() => editor.state.text === "mot:", "selection replacement bypasses French spacing");
  assert(editor.state.text === "mot:", "replacing a selection does not add an automatic transaction");
  await openText("AB");
  inputText(editor.element, "!");
  await waitFor(() => editor.state.dirty && editor.state.text === "!AB", "dirty fixture before open cancellation");

  const previous = editor.state;
  nextOpen = { kind: "cancelled" };
  window.confirm = () => true;
  assert((await editor.openDocument()).kind === "cancelled", "native open cancellation returned to controller");
  assert(editor.state.text === previous.text && editor.state.dirty, "open cancellation preserves document and dirty state");
  nextOpen = { kind: "ok", bytes: Uint8Array.of(0xff) };
  let invalidOpen = false;
  try { await editor.openDocument(); } catch (error) { invalidOpen = error.code === "EDITOR_INVALID_UTF8"; }
  assert(invalidOpen && editor.state.text === previous.text && editor.state.dirty, "invalid open leaves the previous document intact");

  const sourceFile = new Uint8Array([0xef, 0xbb, 0xbf, 0x41, 0xc3, 0xa9, 0x0d, 0x0a, 0x42]);
  nextOpen = { kind: "ok", bytes: sourceFile };
  const opened = await editor.openDocument();
  assert(opened.kind === "ok" && opened.state.text === "Aé\r\nB" && opened.state.bom && !opened.state.dirty, "load preserves BOM metadata and CRLF content");
  assert(textRepresentation.textContent === "Aé\r\nB", "accessible text representation exposes accents and line breaks after load");
  assert((await editor.exportDocument()).bytes.every((byte, i) => byte === sourceFile[i]), "ATEX exports the exact loaded file bytes");

  const largeText = Array.from({ length: 300 }, () => "ligne").join("\r\n");
  nextOpen = { kind: "ok", bytes: new TextEncoder().encode(largeText) };
  await editor.openDocument();
  assert(editor.state.pages.length >= 2, `document paginates across A4 pages (${editor.state.pages.length} page(s), ${editor.state.text.length} chars)`);
  assert(scrollport.scrollHeight > scrollport.clientHeight, "page stack creates a real scroll range");
  scrollport.scrollTop = 260;
  scrollport.dispatchEvent(new Event("scroll"));
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(Number.parseFloat(editor.element.style.top) >= 0 && Number.parseFloat(editor.element.style.top) <= innerHeight,
    "hidden input caret stays bounded on screen after scroll");
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => editor.state?.focus === new TextEncoder().encode(largeText).length, "move caret to document end");
  assert(scrollport.scrollTop > 260 && Number.parseFloat(editor.element.style.top) <= innerHeight - 12,
    "editing navigation scrolls the caret into the visible Canvas viewport");

  const smallFile = sourceFile;
  nextOpen = { kind: "ok", bytes: smallFile };
  await editor.openDocument();
  editor.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  editor.element.dispatchEvent(new CompositionEvent("compositionupdate", { data: "界", bubbles: true }));
  const preview = document.querySelector(".composition-preview");
  assert(!preview.hidden && preview.textContent === "界" && getComputedStyle(preview).textDecorationLine.includes("underline"),
    "IME preview text is temporarily underlined at the caret");
  editor.element.dispatchEvent(new CompositionEvent("compositionend", { data: "界", bubbles: true }));
  await waitFor(() => editor.state?.text === "界Aé\r\nB", "composition commit");
  assert(preview.hidden && editor.state.dirty, "composition mutates model only at commit");

  nextSave = { kind: "cancelled" };
  assert((await editor.saveAs()).kind === "cancelled" && editor.state.dirty, "save cancellation retains dirty state");
  nextSave = { kind: "ok" };
  const saved = await editor.saveAs();
  const expectedSaved = new Uint8Array([0xef, 0xbb, 0xbf, ...new TextEncoder().encode("界Aé\r\nB")]);
  assert(saved.kind === "ok" && !editor.state.dirty, "successful save marks exactly the exported revision clean");
  assert(savedFiles.at(-1).every((byte, index) => byte === expectedSaved[index]), "native host receives exact UTF-8/BOM/CRLF bytes");

  inputText(editor.element, "!");
  await waitFor(() => editor.state?.dirty, "dirty before racing save");
  let releaseSave;
  const saveGate = new Promise(resolve => { releaseSave = resolve; });
  holdSave = { promise: saveGate };
  const racingSave = editor.saveAs();
  await waitFor(() => editor.busy, "native save dialog pending");
  let busyUndo = false;
  try { await editor.undo(); } catch (error) { busyUndo = error.code === "EDITOR_BUSY"; }
  assert(busyUndo, "public undo is guarded while a native file operation is active");
  const externalEdit = encodeMessage({ type: MessageType.Editor, requestId: 900n,
    payload: encodeEditorCommand(EditorOpcode.InsertText, { nowMs: 999n, text: "Q" }) });
  await client.request(externalEdit);
  releaseSave();
  let stale = false;
  try { await racingSave; } catch (error) { stale = error.code === "EDITOR_STALE_SAVE"; }
  assert(stale, "out-of-band revision change rejects stale save confirmation");
  await editor.initialize();
  assert(editor.state.dirty && editor.state.text.includes("Q"), "stale save leaves the live document dirty and accessible");
  nextSave = { kind: "ok" };
  await editor.saveAs();
  assert(!editor.state.dirty, "retry saves the current revision");
  assert(dirtyNotifications.includes(false) && dirtyNotifications.includes(true), "dirty state reaches the host channel");

  const rect = canvas.getBoundingClientRect();
  pointer("pointerdown", rect.left + 90, rect.top + 95);
  pointer("pointerup", rect.left + 90, rect.top + 95);
  await waitFor(() => capturedPointer === null, "Canvas pointer release");
  await waitFor(() => editor.state?.focus >= 0, "Canvas hit-test response");
  assert(document.activeElement === editor.element, "Canvas hit-test restores hidden input focus");

  // Exercise the actual app-level shortcut listener against an inert copy of the product page.
  await checkEditorZoom({ editor, scrollport, canvas, openText, assert, waitFor, key, inputText, pointer });
  editor.dispose(); editor = null;
  client.close(); client = null;
  const productResponse = await fetch("/index.html");
  assert(productResponse.ok, "product entry document available to the shortcut harness");
  const productDocument = new DOMParser().parseFromString(await productResponse.text(), "text/html");
  for (const source of productDocument.querySelectorAll('link[rel="stylesheet"]')) {
    const sheet = document.createElement("link");
    sheet.rel = "stylesheet"; sheet.href = source.getAttribute("href");
    const loaded = new Promise((yes, no) => { sheet.onload = yes; sheet.onerror = no; });
    document.head.append(sheet); await loaded;
  }
  document.body.replaceWith(document.importNode(productDocument.body, true));
  const nativeCalls = [];
  const webviewListeners = new Set();
  let nextAppOpen = { status: "cancelled", payload: "" };
  let nextDocxOpen = { status: "cancelled", payload: "" };
  let holdDocxReply;
  const fakeWebview = {
    postMessage(message) {
      nativeCalls.push(message);
      const match = /^us120-file\|1\|([1-9][0-9]*)\|(open|openDocx|save|dirty)\|(.*)$/.exec(message);
      if (!match || match[2] === "dirty") return;
      const [, id, operation, payload] = match;
      const response = operation === "openDocx" ? `us120-file|1|${id}|${nextDocxOpen.status}|${nextDocxOpen.payload}` : operation === "open"
        ? `us120-file|1|${id}|${nextAppOpen.status}|${nextAppOpen.payload}`
        : `us120-file|1|${id}|ok|`;
      const reply = () => { for (const listener of webviewListeners) listener({ data: response }); };
      if (operation === "openDocx" && holdDocxReply) { holdDocxReply.resolve = reply; return; }
      queueMicrotask(reply);
    },
    addEventListener(type, listener) { if (type === "message") webviewListeners.add(listener); },
    removeEventListener(type, listener) { if (type === "message") webviewListeners.delete(listener); },
  };
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: fakeWebview } });
  localStorage.setItem("atelier.frenchSpacing", "false");
  const { editorReady } = await import("/app.mjs");
  const app = await editorReady;
  assert(app.status === "ready", "real app entry initializes with fake native WebView");
  assert(document.querySelectorAll('#ribbon [role="tab"]').length === 6, "real app exposes all six ribbon tabs");
  assert(document.querySelector('#ribbon [data-ribbon-command-id="font"]').disabled,
    "real app marks unavailable formatting commands disabled");
  const themeState = app.getState();
  app.theme.setTheme("light");
  const lightChrome = getComputedStyle(document.querySelector(".masthead")).backgroundColor;
  const documentContext = document.querySelector("#document-canvas").getContext("2d");
  const paperBeforeTheme = [...documentContext.getImageData(30, 30, 1, 1).data].join(",");
  app.theme.setTheme("dark");
  assert(getComputedStyle(document.querySelector(".masthead")).backgroundColor !== lightChrome,
    "real app theme changes the visible shell palette without reloading");
  assert(app.getState() === themeState && [...documentContext.getImageData(30, 30, 1, 1).data].join(",") === paperBeforeTheme,
    "real app theme preserves exact TXT state and white document pixels");
  app.theme.setTheme("light");
  app.ribbon.selectTab("view");
  document.querySelector('[data-ribbon-command-id="appearance"]').click();
  const appearanceDialog = document.querySelector(".shell-preferences").closest("dialog");
  assert(appearanceDialog.open && appearanceDialog.getAttribute("aria-labelledby"), "real app appearance command opens a named modal");
  const modalNativeCalls = nativeCalls.length;
  for (const key of ["n", "o", "s"]) appearanceDialog.querySelector("button").dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey: true, bubbles: true, cancelable: true }));
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(nativeCalls.length === modalNativeCalls && app.getState() === themeState,
    "appearance modal blocks editor new/open/save shortcuts behind the dialog");
  appearanceDialog.querySelector(".ui-dialog-close").click();
  assert(!appearanceDialog.open, "real app appearance modal can close and return to the ribbon");
  app.ribbon.selectTab("file");
  assert(!document.querySelector("#french-spacing").checked, "app initialization restores a saved disabled French-spacing preference");
  assert(innerWidth >= 1200 && innerHeight >= 800, "product viewport is at least 1280×800 for layout verification");
  assert(document.documentElement.scrollHeight <= innerHeight && document.body.scrollHeight <= innerHeight,
    "product shell keeps the document scrollbar inside the normal viewport");
  window.confirm = () => true;
  inputText(app.editor.element, "N");
  await waitFor(() => app.getState()?.text === "N" && app.getState()?.dirty, "app editor becomes dirty");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => app.getState()?.text === "" && !app.getState()?.dirty, "Ctrl+N invokes the app New action");
  assert(app.getState().text === "" && !app.getState().dirty, "Ctrl+N discards only after the app confirmation and creates a clean document");
  assert(nativeCalls.every(message => !message.includes("|open|") && !message.includes("|save|")),
    "Ctrl+N does not request file access");
  const appLargeText = Array.from({ length: 300 }, () => "app page").join("\r\n");
  nextAppOpen = { status: "ok", payload: btoa(appLargeText) };
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => app.getState()?.text === appLargeText, "Ctrl+O loads the multi-page app fixture");
  const productScrollport = document.querySelector("#document-scroll");
  const openAppText = async text => {
    const bytes = new TextEncoder().encode(text);
    let binary = "";
    for (let i=0;i<bytes.length;i+=0x8000) binary+=String.fromCharCode(...bytes.subarray(i,i+0x8000));
    nextAppOpen = {status:"ok",payload:btoa(binary)};
    await app.editor.openDocument();
  };
  await checkNavigation({ app, assert, waitFor, key, inputText, openText: openAppText });
  await checkReplacement({ app, assert, openText: openAppText });
  await checkDocumentStatus({ app, assert, waitFor, key, inputText, openText: openAppText });
  nextAppOpen = {status:"ok",payload:btoa(appLargeText)};
  await app.editor.openDocument();
  assert(productScrollport.scrollHeight > productScrollport.clientHeight, "multi-page document scrolls inside document-scroll");
  assert(document.documentElement.scrollHeight <= innerHeight && document.body.scrollHeight <= innerHeight,
    "long document does not grow the product body or move toolbar/footer outside the viewport");
  nextAppOpen = { status: "cancelled", payload: "" };
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => nativeCalls.some(message => message.includes("|open|")), "Ctrl+O sends an open request to the native host");
  await waitFor(() => document.querySelector("#editor-status")?.textContent.includes("annul"), "Ctrl+O cancellation is surfaced");
  assert(nativeCalls.some(message => message.includes("|open|"))
    && document.querySelector("#editor-status").textContent.includes("annul"), "Ctrl+O uses the native open path and surfaces cancellation");
  assert(app.getState().text === appLargeText && !app.getState().dirty, "Ctrl+O cancellation preserves the current clean document");
  await checkAppZoom({ app, scrollport: productScrollport, assert, waitFor });
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => app.getState()?.text === "", "Ctrl+N clears the opened document before save shortcut check");
  inputText(app.editor.element, "S");
  await waitFor(() => app.getState()?.text === "S" && app.getState()?.dirty, "app editor becomes dirty before save");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => nativeCalls.some(message => message.includes("|save|")), "Ctrl+S sends a save request to the native host");
  await waitFor(() => !app.getState()?.dirty, "Ctrl+S marks the successfully saved revision clean");
  assert(!app.getState().dirty, "Ctrl+S marks the successfully saved revision clean");
  assert(document.querySelector('[data-status-field="saved"]').textContent.includes("Enregistré"), "status bar reflects the native save confirmation");
  const saveMessage = nativeCalls.find(message => message.includes("|save|"));
  assert(atob(saveMessage.split("|")[4]) === "S", "Ctrl+S forwards the exact current document bytes");
  const option = document.querySelector("#french-spacing");
  app.ribbon.selectTab("home");
  assert(option && !option.disabled && option.labels.length === 1 && option.labels[0].textContent.trim().length > 0,
    "French spacing option is an enabled native checkbox with visible label");
  document.querySelector(".editor-options").open = true;
  option.focus();
  assert(document.activeElement === option, "French spacing option is keyboard focusable");
  option.checked = false;
  option.dispatchEvent(new Event("change", { bubbles: true }));
  inputText(app.editor.element, ":");
  await waitFor(() => app.getState().text === "S:", "real app option disables substitution");
  assert(localStorage.getItem("atelier.frenchSpacing") === "false", "French spacing preference is saved locally");
  option.checked = true;
  option.dispatchEvent(new Event("change", { bubbles: true }));
  inputText(app.editor.element, "X");
  inputText(app.editor.element, "?");
  await waitFor(() => app.getState().text === "S:X\u202f?", "real app option re-enables narrow NBSP");
  assert(app.getState().text.endsWith("X\u202f?"), "enabled app option applies French spacing through real Worker WASM");
  const docxResponse = await fetch("/tests/docx-reference/basic.docx");
  const docxBytes = new Uint8Array(await docxResponse.arrayBuffer());
  let docxBinary = "";
  for (let offset = 0; offset < docxBytes.length; offset += 0x8000) docxBinary += String.fromCharCode(...docxBytes.subarray(offset,offset+0x8000));
  const docxBase64 = btoa(docxBinary);
  assert(docxBytes.length > 131075 && docxBytes.length < 8*1024*1024, "independent DOCX fixture exercises a native response larger than the TXT limit");
  const beforePreview = app.getState();
  app.editor.setZoom(1.5);
  app.ribbon.selectTab("file");
  nextDocxOpen = {status:"ok",payload:docxBase64};
  assert((await app.openDocx()).kind === "ok", "real app imports DOCX through native host and Worker/WASM");
  assert(app.previewActive && document.querySelector("#text-workspace").hidden && !document.querySelector("#rich-workspace").hidden,
    "read-only DOCX workspace replaces only the visible TXT workspace");
  assert(document.querySelector("#search-command").disabled && document.querySelector("#statistics-command").disabled
      && document.querySelector("[data-statistics-status]").disabled,
    "search and statistics tools are explicitly unavailable in the simplified DOCX preview");
  assert(document.querySelector("#rich-text").textContent.includes("Référence de titre") && document.querySelector("#rich-text").textContent.length > 100,
    "accessible preview mirror contains actual imported text");
  assert(document.querySelector("#rich-canvas").width > 0 && document.querySelector("#rich-canvas").height > 0,
    "rich preview Canvas has a real visible viewport");
  const pixels = document.querySelector("#rich-canvas").getContext("2d").getImageData(0,0,document.querySelector("#rich-canvas").width,document.querySelector("#rich-canvas").height).data;
  let bluePixels = 0;
  for (let i=0; i<pixels.length; i+=4) if (pixels[i+3] > 0 && pixels[i+2] > pixels[i] + 30 && pixels[i+2] > pixels[i+1]) bluePixels += 1;
  assert(bluePixels > 20, "real DOCX preview paints the blue styled heading as visible Canvas pixels");
  const richScroll = document.querySelector("#rich-scroll");
  const previewZoomRange = document.querySelector("[data-zoom-range]");
  assert(!previewZoomRange.disabled && document.querySelector("[data-zoom-percent]").textContent === "100%",
    "DOCX preview has its own enabled zoom controls starting at 100 percent");
  previewZoomRange.value = "200"; previewZoomRange.dispatchEvent(new Event("input", { bubbles: true }));
  assert(document.querySelector("[data-zoom-percent]").textContent === "200%" && app.editor.getZoom() === 1.5,
    "preview zoom changes independently of the suspended TXT view");
  previewZoomRange.value = "100"; previewZoomRange.dispatchEvent(new Event("input", { bubbles: true }));
  richScroll.style.width = "360px"; window.dispatchEvent(new Event("resize"));
  await new Promise(resolve => requestAnimationFrame(resolve));
  assert(richScroll.scrollWidth > richScroll.clientWidth, "narrow DOCX preview exposes horizontal scrolling for the complete A4 page");
  richScroll.scrollLeft = 120; richScroll.dispatchEvent(new Event("scroll"));
  assert(richScroll.scrollLeft === 120 && Math.abs(document.querySelector("#rich-canvas").getBoundingClientRect().left - richScroll.getBoundingClientRect().left) < 3,
    "preview Canvas remains pinned to viewport during horizontal page scrolling");
  richScroll.style.width = ""; richScroll.scrollLeft = 0; window.dispatchEvent(new Event("resize"));
  assert(app.getState() === beforePreview && app.editor.busy && app.editor.element.disabled,
    "preview leaves exact TXT state object untouched and freezes its input");
  assert(document.querySelector('[data-status-field="view"]').textContent.includes("DOCX")
      && document.querySelector('[data-status-field="saved"]').textContent.includes("Lecture seule")
      && document.querySelector("#replace-command").disabled && !app.navigation.openReplace(),
    "status describes DOCX read-only view and replacement stays unavailable");
  document.querySelector(".document-status-page").click();
  const previewPageInput = document.querySelector("[data-goto-page]");
  previewPageInput.value = "1";
  document.querySelector("[data-goto-apply]").click();
  assert(app.getState() === beforePreview && richScroll.scrollTop === 0
      && !document.querySelector("[data-status-goto-dialog]").open,
    "preview page navigation uses its own viewport without touching TXT state");
  for (const id of ["new-document","open-document","open-docx","save-document","undo","redo","french-spacing"]) assert(document.getElementById(id).disabled, `preview disables ${id}`);
  assert(document.querySelector("#editor-status").textContent.includes("lecture seule"), "preview announces read-only mode");
  assert(document.querySelector("#page-title").textContent.includes("DOCX") && document.title.includes("DOCX") && document.querySelector("#document-state").textContent === "Lecture seule",
    "preview primary identity and state badge describe DOCX rather than TXT save state");
  assert(document.querySelector(".preview-caption").textContent.includes("sauts de page") && document.querySelector(".preview-caption").textContent.includes("pieds de page"),
    "preview explicitly explains omitted content and Word page breaks");
  const callsBeforeShortcut = nativeCalls.length;
  const saveShortcut = new KeyboardEvent("keydown",{key:"s",ctrlKey:true,bubbles:true,cancelable:true});
  richScroll.dispatchEvent(saveShortcut);
  assert(nativeCalls.length === callsBeforeShortcut && saveShortcut.defaultPrevented, "Ctrl+S in preview suppresses native/browser saving");
  key(document.querySelector("#rich-scroll"),"Escape");
  assert(!app.previewActive && !app.editor.busy && app.getState() === beforePreview && document.activeElement === document.querySelector("#open-docx"),
    "Escape returns to exact TXT state and restores trigger focus");
  assert(document.querySelector("#page-title").textContent === "Document texte" && document.querySelector("#document-state").textContent.includes("non enregistrées"),
    "closing preview restores TXT identity and dirty badge");
  assert(app.editor.getZoom() === 1.5 && document.querySelector("[data-zoom-percent]").textContent === "150%",
    "returning from DOCX restores the TXT session zoom in the status controls");
  await app.editor.undo(); assert(app.getState().text !== beforePreview.text, "TXT undo history remains available after preview");
  await app.editor.redo(); assert(app.getState().text === beforePreview.text && app.getState().dirty === beforePreview.dirty, "TXT redo restores exact content and dirty after preview");
  app.editor.fitZoom("width");
  const fittedTextZoom = app.editor.getZoom();
  assert((await app.openDocx()).kind === "ok", "reopening preview for fitted TXT view");
  await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  assert(app.editor.getZoom() === fittedTextZoom && app.editor.getZoomMode() === "width",
    "hiding a fitted TXT viewport does not refit it against zero dimensions");
  app.closePreview();
  await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  assert(app.editor.getZoomMode() === "width" && Math.abs(app.editor.getZoom() * 595.276 - productScrollport.clientWidth) < 2,
    "returning to a fitted TXT view preserves its mode and recomputes visible dimensions");
  app.editor.setZoom(1.5);
  const beforeFailure=app.getState();
  nextDocxOpen={status:"cancelled",payload:""}; await app.openDocx();
  assert(app.getState()===beforeFailure && !app.previewActive && !app.editor.busy, "cancelled native DOCX dialogue preserves TXT and releases controls");
  nextDocxOpen={status:"error",payload:"files.error.io"}; await app.openDocx();
  assert(app.getState()===beforeFailure && !app.previewActive, "native DOCX read failure preserves TXT");
  nextDocxOpen={status:"ok",payload:btoa("invalid DOCX")}; await app.openDocx();
  assert(app.getState()===beforeFailure && !app.previewActive && document.querySelector("#editor-status").classList.contains("error"), "invalid DOCX reports an error without partial preview or TXT mutation");
  nextDocxOpen={status:"ok",payload:docxBase64}; holdDocxReply={};
  const lateOpen=app.openDocx(); app.closePreview(); holdDocxReply.resolve(); holdDocxReply=null; await lateOpen;
  assert(!app.previewActive && app.getState()===beforeFailure && !app.editor.busy, "late native response after close cannot reopen preview or mutate TXT");
  holdDocxReply={}; const disposedOpen=app.openDocx(); app.dispose(); holdDocxReply.resolve(); holdDocxReply=null; await disposedOpen;
  assert(webviewListeners.size === 0, "disposing app during DOCX dialogue removes native listener and ignores late response");
  localStorage.removeItem("atelier.frenchSpacing");
  const { editorReady: defaultReady } = await import("/app.mjs?preference-default");
  const defaultApp = await defaultReady;
  assert(defaultApp.status === "ready" && document.querySelector("#french-spacing").checked,
    "fresh app initialization enables French spacing when no preference exists");
  defaultApp.dispose();

  report = { status: "passed", userAgent: navigator.userAgent, checks };
} catch (error) {
  report = { status: "failed", message: error?.stack ?? String(error), checks };
} finally {
  editor?.dispose();
  client?.close();
  resultNode.textContent = JSON.stringify(report, null, 2);
  if (new URL(location.href).searchParams.get("automated") === "1") {
    await fetch("/__editor_result", { method: "POST", body: JSON.stringify(report) });
  }
}
