// SPDX-License-Identifier: AGPL-3.0-or-later
// Scénario v2 de tests/run-editor-browser.mjs (R7a) : l'éditeur riche derrière l'API de createEditor, Worker et WASM réels.
// Reprend les scénarios de editor-browser.mjs que v2 sait faire (saisie, UTF-8, presse-papiers, mouvements, suppression par mot,
// espaces françaises, ouverture, enregistrement, course à l'enregistrement, composition, zoom). Les sections propres à v1 sont
// listées dans `skipped` avec la raison. Les attentes changent là où le texte plat de v2 diffère de celui de v1 : les fins de
// ligne sont \n (v1 gardait \r\n dans son texte), les positions comptent un octet par \n.
import { createRichEditor } from "../web/rich-editor.mjs";
import { createEngineClient } from "../web/engine-client.mjs";
import { createI18n, parseCatalog } from "../web/i18n.mjs";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { checkEditorZoom } from "./zoom-browser.mjs";
import { documentText, insertTextCommand } from "./editor-test-adapter.mjs";
import { run as checkFind } from "./editor-find-browser.mjs";

const checks = [];
const skipped = [];
function assert(value, name) {
  if (!value) throw new Error(name);
  checks.push(name);
}
const skip = (name, reason) => skipped.push(`${name} (${reason})`);
async function waitFor(predicate, name, timeoutMs = 8000) {
  const end = performance.now() + timeoutMs;
  while (performance.now() < end) {
    if (await predicate()) return;
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
const encoder = new TextEncoder();

const resultNode = document.querySelector("#result");
const savedFiles = [];
const dirtyNotifications = [];
let nextOpen = { kind: "cancelled" };
let nextSave = { kind: "ok" };
let holdSave;
let client;
let editor;
let report;
try {
  const wasmResponse = await fetch("/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm");
  assert(wasmResponse.ok, "fresh optimized WASM available");
  client = await createEngineClient({ worker: new Worker("/web/engine-worker.mjs", { type: "module" }), bytes: await wasmResponse.arrayBuffer() });
  const ping = encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: new Uint8Array() });
  assert(decodeMessage(await client.request(ping), "command").type === MessageType.Ping, "legacy Ping echo remains exact");
  const catalog = new Map([...parseCatalog(await (await fetch("/i18n/fr-FR.txt")).text()).entries].map(([k, entry]) => [k, entry.value]));
  const i18n = createI18n({ locale: "fr-FR", catalog });
  // Simple host (open/save by bytes, as the v1 suite): the v2 editor falls back to it when the host has no ticket dialogs.
  const host = {
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
  const errors = [];
  editor = createRichEditor({ client, host, canvas, scrollport, spacer, textRepresentation: document.querySelector("#document-text"),
    confirmDiscard: "confirm", t: (k, v) => i18n.t(k, v), onError: error => errors.push(error.code) });
  const initial = await editor.initialize();
  assert(initial.kind === "state" && initial.docLen === 0 && !initial.dirty && initial.pageCount >= 1, "initial empty v2 state with one A4 page");
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

  const text = () => documentText(editor);
  inputText(editor.element, "A"); inputText(editor.element, "é"); inputText(editor.element, "B");
  await waitFor(async () => await text() === "AéB", "ordered rapid input");
  const mirror = document.querySelector(".document-mirror");
  await waitFor(() => mirror.textContent.includes("AéB"), "mirror shows accented text");
  assert(mirror.textContent.includes("AéB") && document.querySelector("#document-text").hidden, "the DOM mirror replaces the plain text representation");
  assert(editor.state.dirty && editor.state.focus === 4, "UTF-8 selection follows rapid multibyte typing");
  key(editor.element, "ArrowLeft");
  await waitFor(() => editor.state.focus === 3, "caret movement");
  key(editor.element, "ArrowLeft", { shiftKey: true });
  await waitFor(() => editor.state.anchor === 3 && editor.state.focus === 1, "directed selection");
  inputText(editor.element, "Z");
  await waitFor(async () => await text() === "AZB", "selection replacement");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(async () => await text() === "AéB", "undo through hidden input");
  key(editor.element, "y", { ctrlKey: true });
  await waitFor(async () => await text() === "AZB", "redo through hidden input");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteContentBackward", bubbles: true, cancelable: true }));
  await waitFor(async () => await text() === "AB", "grapheme backspace through hidden input");

  // ---- clipboard (synthetic ClipboardEvents; DOM, Worker and WASM are real) ----------------------------------------------------
  const openText = async value => {
    window.confirm = () => true;
    nextOpen = { kind: "ok", bytes: encoder.encode(value) };
    await editor.openDocument();
  };
  const clipboard = (type, { text: data = "", fail = false } = {}) => {
    const written = [];
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: {
      types: ["text/plain", "text/html"],
      getData: format => format === "text/plain" ? data : "<script>bad()</script>",
      setData(format, value) { if (fail) throw new Error("denied"); written.push([format, value]); },
    } });
    editor.element.dispatchEvent(event);
    return { event, written };
  };
  const selectedText = async () => editor.selectedText(editor.state);
  await openText("un café trois\r\nligne suivante");
  assert(await text() === "un café trois\nligne suivante", "an opened TXT is normalised to \\n paragraph separators in the flat text");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "Ctrl+Right next word");
  key(editor.element, "ArrowRight", { ctrlKey: true, shiftKey: true });
  await waitFor(() => editor.state.anchor === 3 && editor.state.focus === 9, "Ctrl+Shift+Right word selection");
  assert(await selectedText() === "café ", "word selection uses UTF-8 boundaries and includes the separating space");
  const beforeCopy = editor.state;
  const copied = clipboard("copy");
  assert(copied.event.defaultPrevented && copied.written.length === 1 && copied.written[0][0] === "text/plain"
    && copied.written[0][1] === "café " && editor.state.revision === beforeCopy.revision && !editor.state.dirty,
    "real model selection copies exact text synchronously without mutation");
  const deniedCut = clipboard("cut", { fail: true });
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(deniedCut.event.defaultPrevented && await text() === "un café trois\nligne suivante" && editor.state.revision === beforeCopy.revision,
    "failed clipboard write preserves the real model");
  const cut = clipboard("cut");
  assert(cut.written[0][1] === "café ", "cut writes before returning to the browser");
  await waitFor(async () => await text() === "un trois\nligne suivante", "cut range mutation");
  await editor.undo();
  assert(await text() === "un café trois\nligne suivante" && editor.state.anchor === 3 && editor.state.focus === 9,
    "one undo restores the cut text and its selection");
  key(editor.element, "v", { ctrlKey: true, shiftKey: true });
  clipboard("paste", { text: "é🙂" });
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertFromPaste", data: "é🙂", bubbles: true, cancelable: true }));
  await waitFor(async () => await text() === "un é🙂trois\nligne suivante", "plain paste replacement");
  assert(!(await text()).includes("<script>"), "plain paste replaces the selection once and ignores HTML");
  clipboard("paste", { text: "a\r\nb" });
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertFromPaste", data: "a\r\nb", bubbles: true, cancelable: true }));
  await waitFor(async () => (await text()).includes("a\nb"), "pasted CRLF becomes a paragraph break");
  await editor.undo();
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(async () => editor.state.anchor === 0 && editor.state.focus === editor.state.docLen, "Ctrl+A whole document");
  assert(await selectedText() === await text(), "Ctrl+A selects the real document rather than the hidden textarea");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 0, "document start before F8");
  key(editor.element, "F8");
  key(editor.element, "ArrowRight");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 2, "F8 extends successive movements");
  key(editor.element, "Escape");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.anchor === 2 && editor.state.focus === 2, "Escape ends extension mode");
  key(editor.element, "F8");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === editor.state.docLen, "Ctrl+A selects all while extension mode is active");
  key(editor.element, "ArrowLeft");
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 0, "Ctrl+A exits extension mode before the next caret movement");

  await openText("é abc\r\nxy🙂z");
  key(editor.element, "ArrowDown", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 7, "Ctrl+Down moves by paragraph using UTF-8 offsets");
  key(editor.element, "ArrowUp", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0, "Ctrl+Up moves by paragraph using UTF-8 offsets");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 3, "Ctrl+Right moves to the first word boundary in UTF-8 bytes");
  key(editor.element, "ArrowLeft", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0, "Ctrl+Left moves by word using grapheme-safe UTF-8 offsets");

  await openText("un café trois");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "word start before deletion");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteWordForward", bubbles: true, cancelable: true }));
  await waitFor(async () => await text() === "un trois", "Ctrl+Delete semantic input");
  await editor.undo();
  assert(await text() === "un café trois", "word deletion is undone as a single edit in real WASM");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.anchor === editor.state.focus && editor.state.focus === 0, "word deletion-backward starts at document beginning");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 3, "word deletion-backward first word boundary");
  key(editor.element, "ArrowRight", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 9, "caret after accented word uses UTF-8 bytes");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteWordBackward", bubbles: true, cancelable: true }));
  await waitFor(async () => await text() === "un trois" && editor.state.focus === 3, "Ctrl+Backspace deletes the preceding word through semantic beforeinput");
  await editor.undo();
  assert(await text() === "un café trois" && editor.state.focus === 9, "one undo restores the deleted word");

  // ---- mouse ------------------------------------------------------------------------------------------------------------------------
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
  await waitFor(async () => await selectedText() === "café" && capturedPointer === null, "double-click selection");
  assert(await selectedText() === "café", "double-click selects a whole accented word through engine unit selection");
  pointer("pointerdown", wordPoint.x + 3, wordPoint.y, 3);
  pointer("pointerup", wordPoint.x + 3, wordPoint.y, 3);
  await waitFor(async () => await selectedText() === "un café trois\n" && capturedPointer === null, "triple-click paragraph selection");
  assert(await selectedText() === "un café trois\n", "triple-click selects the logical paragraph including its break");
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
  assert(await selectedText() === "un café trois", "rapid down/move/up preserves the final drag range while WASM is asynchronous");
  assert(document.activeElement === editor.element, "completed mouse drag restores hidden input focus");
  // Text drag and drop (R7c): MoveText, one undo step, Ctrl copy, Escape, external text.
  await openText("AéBxyz");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 7, "drop target at line end");
  const dropEnd = caretPoint();
  key(editor.element, "Home");
  key(editor.element, "ArrowRight", { shiftKey: true });
  key(editor.element, "ArrowRight", { shiftKey: true });
  await waitFor(() => editor.state.anchor === 0 && editor.state.focus === 3 && editor.selectionRects.length > 0, "UTF-8 source selection for move");
  const selectedRect = editor.selectionRects[0];
  const canvasBounds = canvas.getBoundingClientRect();
  const sourcePoint = { x: canvasBounds.left + selectedRect.x + 2, y: canvasBounds.top + selectedRect.y + 7 - scrollport.scrollTop };
  const dragRevision = editor.state.revision;
  assert(editor.capabilities.textDragDrop === true, "the v2 editor declares text drag and drop");
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  await new Promise(resolve => setTimeout(resolve, 60));
  assert(editor.state.anchor === 0 && editor.state.focus === 3 && editor.state.revision === dragRevision && await text() === "AéBxyz",
    "drag position probe preserves the selection and the revision");
  pointer("pointerup", dropEnd.x, dropEnd.y);
  await waitFor(async () => await text() === "BxyzAé" && capturedPointer === null, "atomic selected text move");
  assert(editor.state.anchor === 4 && editor.state.focus === 7, "the moved text is selected after the drop");
  await editor.undo();
  assert(await text() === "AéBxyz" && editor.state.anchor === 0 && editor.state.focus === 3, "one undo restores the moved UTF-8 text and the source selection");
  await editor.redo();
  assert(await text() === "BxyzAé", "one redo reapplies the complete move");
  await editor.undo();
  const highlighted = () => editor.selectionRects.length > 0 && Math.abs(editor.selectionRects[0].x - selectedRect.x) < 0.5;
  await waitFor(highlighted, "the source highlight is back after undo");
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  pointer("pointerup", dropEnd.x, dropEnd.y, 1, { ctrlKey: true });
  await waitFor(async () => await text() === "AéBxyzAé", "Ctrl selected text copy");
  await editor.undo();
  assert(await text() === "AéBxyz", "Ctrl copy is a single undoable edit");
  await waitFor(highlighted, "the source highlight is back after the copy undo");
  pointer("pointerdown", sourcePoint.x, sourcePoint.y);
  pointer("pointermove", dropEnd.x, dropEnd.y);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  pointer("pointerup", dropEnd.x, dropEnd.y);
  await new Promise(resolve => setTimeout(resolve, 60));
  assert(await text() === "AéBxyz" && capturedPointer === null, "Escape cancels the complete drag without mutation");
  const externalText = new DataTransfer();
  externalText.setData("text/plain", "🙂\r\n: <b>texte</b>");
  const over = new DragEvent("dragover", { clientX: dropEnd.x, clientY: dropEnd.y, dataTransfer: externalText, bubbles: true, cancelable: true });
  canvas.dispatchEvent(over);
  const drop = new DragEvent("drop", { clientX: dropEnd.x, clientY: dropEnd.y, dataTransfer: externalText, bubbles: true, cancelable: true });
  canvas.dispatchEvent(drop);
  await waitFor(async () => (await text()).startsWith("AéBxyz🙂") && (await text()).includes("<b>texte</b>"), "external plain text drop");
  assert(over.defaultPrevented && drop.defaultPrevented, "text drop prevents browser default handling");
  await editor.undo();
  assert(await text() === "AéBxyz", "one undo restores the original document after external insertion");

  // ---- French spacing, with the whole text read back from the window of the State --------------------------------------------------------------------
  await openText("café");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 5, "French punctuation insertion point");
  inputText(editor.element, ":");
  await waitFor(async () => await text() === "café :", "automatic NBSP before colon");
  assert(editor.state.focus === 8, "French correction keeps a UTF-8 caret after punctuation");
  await editor.undo();
  assert(await text() === "café:", "immediate undo removes only automatic spacing and keeps raw punctuation");
  await editor.redo();
  assert(await text() === "café :", "redo restores automatic spacing");
  inputText(editor.element, "X");
  await waitFor(async () => await text() === "café :X", "typing after French correction");
  await editor.undo();
  assert(await text() === "café :", "subsequent typing stays outside substitution undo group");
  editor.setFrenchSpacing(false);
  await openText("mot");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 3, "disabled spacing insertion point");
  inputText(editor.element, "?");
  await waitFor(async () => await text() === "mot?", "disabled spacing leaves raw text");
  editor.setFrenchSpacing(true);
  await openText("");
  inputText(editor.element, "«");
  await waitFor(async () => await text() === "« ", "opening guillemet auto space");
  inputText(editor.element, "é");
  inputText(editor.element, "»");
  await waitFor(async () => await text() === "« é »", "closing guillemet auto space");
  await openText("");
  clipboard("paste", { text: "mot: «texte»!?" });
  await waitFor(async () => await text() === "mot: «texte»!?", "paste bypasses French spacing");
  await openText("mot");
  key(editor.element, "End");
  await waitFor(() => editor.state.focus === 3, "IME punctuation insertion point");
  editor.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  editor.element.dispatchEvent(new CompositionEvent("compositionend", { data: ":", bubbles: true }));
  await waitFor(async () => await text() === "mot:", "IME punctuation bypasses French spacing");
  await openText("motX");
  key(editor.element, "End");
  key(editor.element, "ArrowLeft", { shiftKey: true });
  await waitFor(() => editor.state.anchor === 4 && editor.state.focus === 3, "selection before punctuation replacement");
  inputText(editor.element, ":");
  await waitFor(async () => await text() === "mot:", "selection replacement bypasses French spacing");

  // ---- files: open, save, race ---------------------------------------------------------------------------------------------------------------------
  await openText("AB");
  inputText(editor.element, "!");
  await waitFor(async () => editor.state.dirty && await text() === "!AB", "dirty fixture before open cancellation");
  const previous = editor.state;
  nextOpen = { kind: "cancelled" };
  window.confirm = () => true;
  assert((await editor.openDocument()).kind === "cancelled", "native open cancellation returned to controller");
  assert(await text() === "!AB" && editor.state.dirty && editor.state === previous, "open cancellation preserves document and dirty state");
  nextOpen = { kind: "ok", bytes: encoder.encode("pas un docx"), format: "docx" };
  const refused = await editor.openDocument(async () => nextOpen);
  assert(refused.kind === "error" && refused.key === "status-open-error" && await text() === "!AB" && editor.state.dirty,
    "a refused open reports an error and leaves the previous document intact");
  await waitFor(() => editor.state.pageCount >= 1 && editor.state.caret !== null, "the cache is rebuilt after a refused open");
  skip("an invalid UTF-8 file raises EDITOR_INVALID_UTF8", "v2 detects the encoding (Windows-1252 and others) instead of refusing the file");

  const sourceFile = new Uint8Array([0xef, 0xbb, 0xbf, 0x41, 0xc3, 0xa9, 0x0d, 0x0a, 0x42]);
  nextOpen = { kind: "ok", bytes: sourceFile };
  const opened = await editor.openDocument();
  assert(opened.kind === "ok" && await text() === "Aé\nB" && !opened.state.dirty && opened.state.revision === 0n, "load keeps accents and splits paragraphs at the CRLF");
  await waitFor(() => document.querySelector(".document-mirror").textContent.includes("Aé"), "mirror exposes accents and paragraphs after load");
  assert((await editor.exportDocument()).bytes.every((byte, i) => byte === sourceFile[i]), "Save exports the exact loaded file bytes (BOM and CRLF kept)");

  const largeText = Array.from({ length: 300 }, () => "ligne").join("\r\n");
  nextOpen = { kind: "ok", bytes: encoder.encode(largeText) };
  await editor.openDocument();
  await waitFor(() => editor.state.pageCount >= 2, `document paginates across A4 pages`);
  assert(scrollport.scrollHeight > scrollport.clientHeight, "page stack creates a real scroll range");
  scrollport.scrollTop = 260;
  scrollport.dispatchEvent(new Event("scroll"));
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(Number.parseFloat(editor.element.style.top) >= 0 && Number.parseFloat(editor.element.style.top) <= innerHeight,
    "hidden input caret stays bounded on screen after scroll");
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => editor.state.focus === encoder.encode(largeText.replaceAll("\r\n", "\n")).length, "move caret to document end");
  await waitFor(() => scrollport.scrollTop > 260 && Number.parseFloat(editor.element.style.top) <= innerHeight - 12, "editing navigation scrolls the caret into the visible Canvas viewport");
  assert(scrollport.scrollTop > 260, "Ctrl+End scrolled down to the caret page");

  nextOpen = { kind: "ok", bytes: sourceFile };
  await editor.openDocument();
  editor.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  editor.element.dispatchEvent(new CompositionEvent("compositionupdate", { data: "界", bubbles: true }));
  const preview = document.querySelector(".composition-preview");
  assert(!preview.hidden && preview.textContent === "界" && getComputedStyle(preview).textDecorationLine.includes("underline"),
    "IME preview text is temporarily underlined at the caret");
  editor.element.dispatchEvent(new CompositionEvent("compositionend", { data: "界", bubbles: true }));
  await waitFor(async () => await text() === "界Aé\nB", "composition commit");
  assert(preview.hidden && editor.state.dirty, "composition mutates model only at commit");

  nextSave = { kind: "cancelled" };
  assert((await editor.saveAs()).kind === "cancelled" && editor.state.dirty, "save cancellation retains dirty state");
  nextSave = { kind: "ok" };
  const saved = await editor.saveAs();
  const expectedSaved = new Uint8Array([0xef, 0xbb, 0xbf, ...encoder.encode("界Aé\r\nB")]);
  assert(saved.kind === "ok" && !editor.state.dirty, "successful save marks exactly the saved revision clean");
  assert(savedFiles.at(-1).length === expectedSaved.length && savedFiles.at(-1).every((byte, index) => byte === expectedSaved[index]), "host receives exact UTF-8/BOM/CRLF bytes");

  inputText(editor.element, "!");
  await waitFor(() => editor.state.dirty, "dirty before racing save");
  let releaseSave;
  const saveGate = new Promise(resolve => { releaseSave = resolve; });
  holdSave = { promise: saveGate };
  const racingSave = editor.saveAs();
  await waitFor(() => editor.busy, "native save dialog pending");
  let busyUndo = false;
  try { await editor.undo(); } catch (error) { busyUndo = error.code === "EDITOR_BUSY"; }
  assert(busyUndo, "public undo is guarded while a native file operation is active");
  const externalEdit = encodeMessage({ type: MessageType.Editor, requestId: 900n, payload: insertTextCommand("Q") });
  await client.request(externalEdit);
  releaseSave();
  let stale = false;
  try { await racingSave; } catch (error) { stale = error.code === "EDITOR_STALE_SAVE"; }
  assert(stale, "out-of-band revision change rejects stale save confirmation");
  assert((await text()).includes("Q") && editor.state.dirty, "stale save leaves the live document dirty and readable");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.revision > 0n, "next State brings the client back to the engine revision");
  nextSave = { kind: "ok" };
  await editor.saveAs();
  assert(!editor.state.dirty, "retry saves the current revision");
  assert(dirtyNotifications.includes(false) && dirtyNotifications.includes(true), "dirty state reaches the host channel");

  const rect = canvas.getBoundingClientRect();
  pointer("pointerdown", rect.left + 90, rect.top + 95);
  pointer("pointerup", rect.left + 90, rect.top + 95);
  await waitFor(() => capturedPointer === null, "Canvas pointer release");
  assert(document.activeElement === editor.element, "Canvas hit-test restores hidden input focus");

  await checkEditorZoom({ editor, scrollport, canvas, openText, assert, waitFor, key, inputText, pointer, textOf: text });

  // R7b-2: search, replacement, statistics and properties run here in v2 (they were skipped in R7a).
  await checkFind({ assert, waitFor, editor, openText, text, canvas });

  for (const [name, reason] of [
    ["the properties dialog on the DOCX preview (properties-integration)", "v2 has no preview: the dialog reads the open document; its scenario (six fields, limits, cancel, OK, reread from the DOCX) is run by run-rich-editor-ui-browser"],
    ["DOCX preview mode (openDocx, closePreview)", "replaced by opening the DOCX in the editor"],
  ]) skip(name, reason);
  assert(errors.every(code => typeof code === "string"), "every reported error carries a typed code");
  report = { status: "passed", engine: "v2", checks, skipped, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", engine: "v2", message: String(error?.stack ?? error), checks, skipped, userAgent: navigator.userAgent };
}
resultNode.textContent = JSON.stringify(report, null, 2);
try { editor?.dispose?.(); client?.close?.(); } catch { /* the page is going away */ }
await fetch("/__editor_result", { method: "POST", body: JSON.stringify(report) });
