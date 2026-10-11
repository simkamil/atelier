// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEditor } from "../web/editor.mjs";
import { createEngineClient } from "../web/engine-client.mjs";
import { encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";

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
  canvas.dispatchEvent(new PointerEvent("pointerdown", { clientX: rect.left + 90, clientY: rect.top + 95, bubbles: true }));
  await waitFor(() => editor.state?.focus >= 0, "Canvas hit-test response");
  assert(document.activeElement === editor.element, "Canvas hit-test restores hidden input focus");

  // Exercise the actual app-level shortcut listener against an inert copy of the product page.
  editor.dispose(); editor = null;
  client.close(); client = null;
  const productResponse = await fetch("/index.html");
  assert(productResponse.ok, "product entry document available to the shortcut harness");
  const productDocument = new DOMParser().parseFromString(await productResponse.text(), "text/html");
  document.body.replaceWith(document.importNode(productDocument.body, true));
  const nativeCalls = [];
  const webviewListeners = new Set();
  let nextAppOpen = { status: "cancelled", payload: "" };
  const fakeWebview = {
    postMessage(message) {
      nativeCalls.push(message);
      const match = /^us120-file\|1\|([1-9][0-9]*)\|(open|save|dirty)\|(.*)$/.exec(message);
      if (!match || match[2] === "dirty") return;
      const [, id, operation, payload] = match;
      const response = operation === "open"
        ? `us120-file|1|${id}|${nextAppOpen.status}|${nextAppOpen.payload}`
        : `us120-file|1|${id}|ok|`;
      queueMicrotask(() => { for (const listener of webviewListeners) listener({ data: response }); });
    },
    addEventListener(type, listener) { if (type === "message") webviewListeners.add(listener); },
    removeEventListener(type, listener) { if (type === "message") webviewListeners.delete(listener); },
  };
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: fakeWebview } });
  const { editorReady } = await import("/app.mjs");
  const app = await editorReady;
  assert(app.status === "ready", "real app entry initializes with fake native WebView");
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
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => app.getState()?.text === "", "Ctrl+N clears the opened document before save shortcut check");
  inputText(app.editor.element, "S");
  await waitFor(() => app.getState()?.text === "S" && app.getState()?.dirty, "app editor becomes dirty before save");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => nativeCalls.some(message => message.includes("|save|")), "Ctrl+S sends a save request to the native host");
  await waitFor(() => !app.getState()?.dirty, "Ctrl+S marks the successfully saved revision clean");
  assert(!app.getState().dirty, "Ctrl+S marks the successfully saved revision clean");
  const saveMessage = nativeCalls.find(message => message.includes("|save|"));
  assert(atob(saveMessage.split("|")[4]) === "S", "Ctrl+S forwards the exact current document bytes");
  app.editor.dispose();

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
