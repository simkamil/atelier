// SPDX-License-Identifier: AGPL-3.0-or-later
// R7a : le produit réel (`app.mjs`) avec l'éditeur v2 (`?engine=v2`), pont natif simulé (US-113 : tickets), vrai Worker et vrai WASM.
const checks = [];
const skipped = [];
function assert(value, name) { if (!value) throw new Error(name); checks.push(name); }
async function waitFor(predicate, name, timeoutMs = 8000) {
  const end = performance.now() + timeoutMs;
  while (performance.now() < end) {
    let ok = false;
    try { ok = await predicate(); } catch { ok = false; }
    if (ok) return;
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  throw new Error(`Timed out waiting for ${name}`);
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const inputText = (element, text) => element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: text, bubbles: true, cancelable: true }));
const key = (element, name, modifiers = {}) => element.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...modifiers }));
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8");
const b64 = bytes => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s); };
const byteLengthOf = value => encoder.encode(value).length;
const fromB64 = text => Uint8Array.from(atob(text), c => c.charCodeAt(0));

/** Pont natif simulé : dialogues, tickets de lecture et d'écriture, avec scénarios programmables. */
function createFakeBridge() {
  const listeners = new Set();
  const bridge = {
    calls: [], files: [], released: [], nextOpen: { cancelled: true }, nextSave: {}, failWrite: false, holdSave: null,
    lastSaveSpec: null, lastOpenSpec: null, reads: new Map(), writes: new Map(), serial: 1,
    webview: {
      postMessage(message) {
        bridge.calls.push(message);
        const match = /^us120-file\|1\|([1-9][0-9]*)\|(\w+)\|(.*)$/.exec(message);
        if (!match || match[2] === "dirty") return;
        const [, id, operation, payload] = match;
        const reply = (status, body = "") => queueMicrotask(() => { for (const listener of listeners) listener({ data: `us120-file|1|${id}|${status}|${body}` }); });
        if (operation === "pickOpen") {
          bridge.lastOpenSpec = decoder.decode(fromB64(payload));
          const next = bridge.nextOpen;
          if (next.cancelled) return reply("cancelled");
          if (next.error) return reply("error", next.error);
          const ticket = `r${bridge.serial++}`;
          bridge.reads.set(ticket, next.bytes);
          return reply("ok", `${ticket};${next.format};${next.bytes.length};${b64(encoder.encode(next.name))}`);
        }
        if (operation === "readTicket") {
          const bytes = bridge.reads.get(payload);
          bridge.reads.delete(payload);
          return bytes ? reply("ok", `b64;${b64(bytes)}`) : reply("error", "files.error.unknown_ticket");
        }
        if (operation === "pickSave") {
          bridge.lastSaveSpec = decoder.decode(fromB64(payload));
          const send = () => {
            const next = bridge.nextSave;
            if (next.cancelled) return reply("cancelled");
            const name = /^name:(.*)$/m.exec(bridge.lastSaveSpec)?.[1] ?? "Document.txt";
            const format = next.format ?? name.split(".").pop();
            const ticket = `w${bridge.serial++}`;
            bridge.writes.set(ticket, []);
            return reply("ok", `${ticket};${format};${b64(encoder.encode(next.name ?? name))}`);
          };
          if (bridge.holdSave) { const hold = bridge.holdSave; bridge.holdSave = null; hold.then(send); return; }
          return send();
        }
        if (operation === "writeTicket") {
          const [ticket, , last, data] = payload.split(";");
          if (bridge.failWrite) { bridge.writes.delete(ticket); return reply("error", "files.error.save_failed"); }
          const chunks = bridge.writes.get(ticket);
          if (!chunks) return reply("error", "files.error.unknown_ticket");
          chunks.push(fromB64(data));
          if (last === "1") {
            const total = chunks.reduce((n, c) => n + c.length, 0);
            const bytes = new Uint8Array(total); let at = 0;
            for (const c of chunks) { bytes.set(c, at); at += c.length; }
            bridge.writes.delete(ticket);
            bridge.files.push({ ticket, bytes });
          }
          return reply("ok");
        }
        if (operation === "releaseTicket") { bridge.released.push(payload); bridge.writes.delete(payload); bridge.reads.delete(payload); return reply("ok"); }
        return reply("ok");
      },
      addEventListener(type, listener) { if (type === "message") listeners.add(listener); },
      removeEventListener(type, listener) { if (type === "message") listeners.delete(listener); },
    },
  };
  return bridge;
}

async function fixture(name) {
  const response = await fetch(`/fixtures/${name}`);
  if (!response.ok) throw new Error(`fixture ${name} unavailable`);
  return new Uint8Array(await response.arrayBuffer());
}
async function pngBytes(color) {
  const canvas = new OffscreenCanvas(40, 20);
  const ctx = canvas.getContext("2d"); ctx.fillStyle = color; ctx.fillRect(0, 0, 40, 20);
  return new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer());
}

/** Replaces the page body by the product body (as the other product suites do) and returns the cleanup-free app. */
async function productPage() {
  const product = await fetch("/index.html");
  const doc = new DOMParser().parseFromString(await product.text(), "text/html");
  for (const source of doc.querySelectorAll('link[rel="stylesheet"]')) {
    if (document.head.querySelector(`link[href="${source.getAttribute("href")}"]`)) continue;
    const sheet = document.createElement("link");
    sheet.rel = "stylesheet"; sheet.href = source.getAttribute("href");
    const loaded = new Promise((yes, no) => { sheet.onload = yes; sheet.onerror = no; });
    document.head.append(sheet); await loaded;
  }
  document.body.replaceWith(document.importNode(doc.body, true));
}

const resultNode = document.querySelector("#result");
const pageErrors = [];
addEventListener("error", event => pageErrors.push(String(event.error?.stack ?? event.message)));
addEventListener("unhandledrejection", event => pageErrors.push(String(event.reason?.stack ?? event.reason)));
let report;
let app;
try {
  const bridge = createFakeBridge();
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: bridge.webview } });
  localStorage.removeItem("atelier.screenReaderMode");
  localStorage.setItem("atelier.frenchSpacing", "false");
  window.confirm = () => true;
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.engine === "v2", "product initializes with the v2 editor from ?engine=v2");
  const editor = app.editor;
  const status = document.querySelector("#editor-status");
  const canvas = document.querySelector("#document-canvas");
  const scrollport = document.querySelector("#document-scroll");
  const state = () => app.getState();
  const text = () => editor.readText();
  assert(document.documentElement.dataset.engine === "v2" && editor.engine === "v2", "page and editor report the v2 engine");
  assert(state().kind === "state" && state().docLen === 0 && !state().dirty && state().revision === 0n && state().pageCount >= 1, "initial blank v2 state");
  assert(!("text" in state()) && !("pages" in state()), "v2 state carries no document text and no page bytes");

  // Canvas: white A4 page, text appears with the atlas glyphs.
  const ratio = devicePixelRatio || 1;
  const darkPixels = (x0, y0, x1, y1) => {
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvas, 0, 0);
    const data = ctx.getImageData(Math.round(x0 * ratio), Math.round(y0 * ratio), Math.max(1, Math.round((x1 - x0) * ratio)), Math.max(1, Math.round((y1 - y0) * ratio))).data;
    let dark = 0;
    for (let i = 0; i < data.length; i += 4) if (data[i] < 100 && data[i + 1] < 100 && data[i + 2] < 100) dark += 1;
    return dark;
  };
  const pageRect = () => ({ left: 0, top: 0 });
  assert(darkPixels(60, 20, 300, 80) === 0, "blank page area (beside the caret) is white");

  // --- typing, selection, undo/redo through the hidden input -------------------------------------------------
  inputText(editor.element, "A"); inputText(editor.element, "é"); inputText(editor.element, "B");
  await waitFor(() => state().docLen === 4, "ordered rapid input");
  assert(await text() === "AéB" && state().dirty && state().focus === 4 && state().anchor === 4, "UTF-8 flat positions follow multibyte typing");
  await waitFor(() => darkPixels(0, 0, 400, 140) > 20, "typed glyphs painted from the atlas");
  checks.push("typed glyphs are painted on the Canvas (atlas glyph runs, no page bytes in the state)");
  assert(document.querySelector("#document-state").textContent === "Modifications non enregistrées", "dirty indicator shows unsaved changes");
  key(editor.element, "ArrowLeft");
  await waitFor(() => state().focus === 3, "caret movement");
  key(editor.element, "ArrowLeft", { shiftKey: true });
  await waitFor(() => state().anchor === 3 && state().focus === 1, "directed selection");
  inputText(editor.element, "Z");
  await waitFor(async () => await text() === "AZB", "selection replacement");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(async () => await text() === "AéB", "undo through hidden input");
  key(editor.element, "y", { ctrlKey: true });
  await waitFor(async () => await text() === "AZB", "redo through hidden input");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteContentBackward", bubbles: true, cancelable: true }));
  await waitFor(async () => await text() === "AB", "grapheme backspace");
  assert(document.querySelector("#undo").disabled === false && document.querySelector("#redo").disabled === true, "undo enabled, redo disabled after edits");
  document.querySelector("#undo").click();
  await waitFor(async () => await text() === "AZB", "undo button");
  document.querySelector("#redo").click();
  await waitFor(async () => await text() === "AB", "redo button");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => state().focus === 0, "Ctrl+Home");
  key(editor.element, "End");
  await waitFor(() => state().focus === 2, "End");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === 2, "select all");
  inputText(editor.element, "\r\n");
  checks.push("a pasted CRLF is normalised to one paragraph break");
  await waitFor(async () => (await text()) === "\n", "line break replaces the selection");

  // --- mirror and reader mode ----------------------------------------------------------------------------------
  const mirror = document.querySelector(".document-mirror");
  assert(mirror && mirror.getAttribute("role") === "document", "accessible DOM mirror exists in the page stack");
  inputText(editor.element, "Titre");
  await waitFor(() => mirror.textContent.includes("Titre"), "mirror shows the typed paragraph");
  const toggle = document.querySelector(".document-mirror-reader-toggle");
  assert(toggle && toggle.getAttribute("aria-pressed") === "false" && toggle.textContent === "Mode lecteur d’écran", "reader mode toggle is in the options, off by default");
  document.querySelector(".editor-options").open = true;
  toggle.click();
  assert(toggle.getAttribute("aria-pressed") === "true" && localStorage.getItem("atelier.screenReaderMode") === "true", "toggle turns reader mode on and persists the preference");
  editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "F1", altKey: true, shiftKey: true, bubbles: true, cancelable: true }));
  assert(toggle.getAttribute("aria-pressed") === "false" && localStorage.getItem("atelier.screenReaderMode") === "false", "Alt+Shift+F1 turns it off and persists");
  toggle.click();
  await waitFor(() => mirror.querySelector("[role=status]")?.textContent.includes("lecteur"), "reader mode announced");


  // --- open a text file through the native dialog and tickets ---------------------------------------------
  const clickAndWait = async (button, scenario, ready) => { scenario(); button.click(); await waitFor(ready, `${button.id} flow`); };
  const openButton = document.querySelector("#open-document");
  const openDocxButton = document.querySelector("#open-docx");
  const saveButton = document.querySelector("#save-document");
  const message = () => status.textContent;
  await waitFor(() => !openButton.disabled, "open button enabled");
  window.confirm = () => false;
  assert(state().dirty, "the document is dirty before the confirmation checks");
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("un café trois\r\nseconde ligne") };
  const callsBefore = bridge.calls.length;
  openButton.click();
  await waitFor(() => message() === "Opération annulée.", "declined discard confirmation cancels the open");
  assert(!bridge.calls.slice(callsBefore).some(call => call.includes("|pickOpen|")), "no file dialog is opened when the discard is declined");
  window.confirm = () => true;
  await clickAndWait(openButton, () => {}, () => message() === "Document ouvert.");
  await waitFor(async () => await text() === "un café trois\nseconde ligne", "TXT content opened");
  assert(!state().dirty && state().revision === 0n && state().docLen === byteLengthOf("un café trois\nseconde ligne"), "opened TXT is clean at revision 0");
  assert(/filter:txt=Texte brut/.test(bridge.lastOpenSpec) && /filter:docx=Document Word/.test(bridge.lastOpenSpec), "open dialog lists the localised formats (txt, docx)");
  await waitFor(() => mirror.textContent.includes("seconde ligne"), "mirror follows the opened document");
  await waitFor(() => darkPixels(60, 60, 400, 140) > 20, "opened text painted");
  bridge.nextOpen = { error: "files.error.not_found" };
  openButton.click();
  await waitFor(() => message() === "Le fichier est introuvable.", "dialog error keeps its translated message");
  assert(await text() === "un café trois\nseconde ligne" && !state().dirty, "a refused file leaves the document untouched");
  bridge.nextOpen = { name: "invalide.docx", format: "docx", bytes: encoder.encode("pas un docx") };
  openButton.click();
  await waitFor(() => message() === "Le document n’a pas pu être ouvert.", "invalid DOCX is reported");
  await waitFor(async () => await text() === "un café trois\nseconde ligne", "the engine keeps the previous document after a refused Open");
  await waitFor(() => darkPixels(60, 60, 400, 140) > 20, "pages are rebuilt by a transparent resync after a refused Open");
  inputText(editor.element, "!");
  await waitFor(async () => (await text()).includes("!"), "the document stays editable after a refused Open");
  document.querySelector("#undo").click();
  await waitFor(async () => await text() === "un café trois\nseconde ligne", "undo after the refused Open");

  // --- DOCX ------------------------------------------------------------------------------------------------
  const banner = document.querySelector("#readonly-banner");
  assert(banner.hidden && banner.getAttribute("role") === "status", "read-only banner is a hidden status region");
  bridge.nextOpen = { name: "basic.docx", format: "docx", bytes: await fixture("basic.docx") };
  await clickAndWait(openDocxButton, () => {}, () => message() === "Document ouvert.");
  assert(state().docLen > 0 && !state().readOnly && banner.hidden && !state().dirty, "plain DOCX opens editable, without the banner");
  assert(/^Ouvrir un document\nfilter:docx=/.test(bridge.lastOpenSpec), "Open DOCX lists DOCX formats first");
  inputText(editor.element, "Z");
  await waitFor(() => state().dirty, "plain DOCX accepts typing");
  document.querySelector("#undo").click();
  await waitFor(() => !state().canUndo, "undo in DOCX");

  bridge.nextOpen = { name: "revisions.docx", format: "docx", bytes: await fixture("revisions.docx") };
  window.confirm = () => true;
  await clickAndWait(openDocxButton, () => {}, () => message().includes("lecture seule"));
  assert(state().readOnly && state().hasRevisions, "DOCX with revisions opens read-only");
  assert(!banner.hidden && banner.textContent.includes("Document en lecture seule — modifications acceptées affichées."), "banner announces read-only with accepted changes shown");
  const copyButton = document.querySelector("#readonly-copy");
  assert(copyButton.tagName === "BUTTON" && copyButton.textContent === "Créer une copie modifiable" && !copyButton.disabled, "banner offers a reachable button to make an editable copy");
  const revisionsLength = state().docLen;
  const revisionsText = await text();
  inputText(editor.element, "Z");
  await waitFor(() => message() === "Ce document est en lecture seule : créez une copie modifiable pour le modifier.", "typing is refused with an explanation");
  assert(state().docLen === revisionsLength && !state().dirty && !state().canUndo, "refused typing changes nothing");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "deleteContentBackward", bubbles: true, cancelable: true }));
  key(editor.element, "z", { ctrlKey: true });
  await sleep(60);
  assert(state().docLen === revisionsLength && !state().dirty, "delete and undo are refused too");
  assert(document.querySelector("#undo").disabled && document.querySelector("#redo").disabled, "undo and redo are disabled in read-only");
  assert(document.querySelector("#image-command").disabled && document.querySelector("#symbols-command").disabled, "image and symbol commands are disabled in read-only");
  const readOnlyImage = await editor.insertImages([new File([await pngBytes("#f00")], "rouge.png", { type: "image/png" })]);
  assert(readOnlyImage.failed === 1 && readOnlyImage.inserted === 0 && state().docLen === revisionsLength, "image insertion is refused in read-only");
  key(editor.element, "ArrowRight");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === revisionsLength, "selection and moves stay allowed in read-only");
  assert(!document.querySelector("#save-document").disabled, "saving stays allowed in read-only");
  await clickAndWait(copyButton, () => {}, () => banner.hidden);
  assert(!state().readOnly && !state().dirty && message().startsWith("Copie modifiable créée"), "editable copy replaces the read-only document and hides the banner");
  assert(await text() === revisionsText, "the copy shows the accepted text");
  inputText(editor.element, "Z");
  await waitFor(() => state().dirty, "the copy accepts typing");
  document.querySelector("#undo").click();
  await waitFor(() => !state().canUndo, "copy undo");

  bridge.nextOpen = { name: "comments.docx", format: "docx", bytes: await fixture("comments.docx") };
  await clickAndWait(openDocxButton, () => {}, () => state().readOnly);
  assert(!banner.hidden && state().hasRevisions, "DOCX with comments opens read-only with the banner");
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("retour") };
  await clickAndWait(openButton, () => {}, () => !state().readOnly && banner.hidden);
  assert(await text() === "retour", "opening a TXT after a read-only DOCX hides the banner");

  // --- save ----------------------------------------------------------------------------------------------------
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => state().focus === state().docLen, "caret at the end of the opened text");
  inputText(editor.element, " é");
  await waitFor(async () => await text() === "retour é", "text to save");
  bridge.nextSave = {};
  const files = bridge.files.length;
  await clickAndWait(saveButton, () => {}, () => bridge.files.length === files + 1 && message() === "Document enregistré.");
  assert(/^Enregistrer le document\nfilter:txt=Texte brut/m.test(bridge.lastSaveSpec) && /name:essai\.txt$/m.test(bridge.lastSaveSpec), "save dialog suggests the opened name and TXT first");
  assert(decoder.decode(bridge.files.at(-1).bytes) === "retour é" && /^w\d+$/.test(bridge.files.at(-1).ticket), "TXT bytes are written to the write ticket");
  assert(!state().dirty && document.querySelector("#document-state").textContent === "Enregistré", "MarkSaved clears the dirty state");
  inputText(editor.element, "x");
  await waitFor(() => state().dirty, "dirty again");
  bridge.nextSave = { format: "docx", name: "essai.docx" };
  await clickAndWait(saveButton, () => {}, () => bridge.files.length === files + 2 && message() === "Document enregistré.");
  const docx = bridge.files.at(-1).bytes;
  assert(docx[0] === 0x50 && docx[1] === 0x4b && decoder.decode(docx).includes("word/document.xml"), "DOCX chosen in the dialog is written as a package");
  assert(!state().dirty, "DOCX save marks the document saved");
  inputText(editor.element, "y");
  await waitFor(() => state().dirty, "dirty before failing save");
  bridge.nextSave = {}; bridge.failWrite = true;
  await clickAndWait(saveButton, () => {}, () => message() === "Le fichier n’a pas pu être enregistré. L’original est conservé.");
  bridge.failWrite = false;
  assert(state().dirty && bridge.files.length === files + 2, "a failed write leaves the document modified and writes nothing");
  bridge.nextSave = { cancelled: true };
  await clickAndWait(saveButton, () => {}, () => message() === "Opération annulée.");
  assert(state().dirty, "a cancelled save dialog changes nothing");
  bridge.holdSave = new Promise(resolve => setTimeout(resolve, 150));
  bridge.nextSave = {};
  saveButton.click();
  await waitFor(() => editor.busy, "busy during the save dialog");
  const lengthDuringSave = state().docLen;
  inputText(editor.element, "q");
  key(editor.element, "ArrowLeft");
  await sleep(60);
  assert(state().docLen === lengthDuringSave && editor.element.disabled, "input is ignored while the save dialog is open");
  await waitFor(() => !editor.busy && bridge.files.length === files + 3, "save finishes after the held dialog");
  assert(!state().dirty, "the held save marked the document saved");
  assert(await editor.exportDocument().then(r => r.bytes.length > 0 && typeof r.revision === "bigint"), "exportDocument returns the TXT bytes and the revision");

  // --- disabled functions ------------------------------------------------------------------------------------------
  app.ribbon.selectTab("home");
  for (const id of ["search-command", "replace-command", "statistics-command", "properties-command"]) {
    const button = document.querySelector(`#${id}`);
    assert(button && button.disabled && /nouvel éditeur/.test(button.title) && button.getAttribute("aria-description") === button.title, `${id} is greyed with an explanatory tooltip in v2`);
  }
  assert(await editor.search("a").catch(error => error.code) === "EDITOR_UNSUPPORTED_V2" && await editor.statistics().catch(error => error.code) === "EDITOR_UNSUPPORTED_V2"
    && await editor.replaceAll({}).catch(error => error.code) === "EDITOR_UNSUPPORTED_V2" && editor.setSearchHighlights(null) === true && editor.setSearchHighlights({}) === false,
    "search, statistics and replace reject with a typed error, never silently");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true, cancelable: true }));
  assert(document.querySelector("#search-slot").hidden && !document.querySelector(".search-pane"), "Ctrl+F opens no search pane in v2");
  assert(JSON.stringify(editor.capabilities) === JSON.stringify({ search: false, replace: false, statistics: false, textDragDrop: false, preview: false, properties: false, wordSelect: false }), "capabilities list what v2 does not offer");
  const dropLength = state().docLen;
  const textDrop = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: (() => { const t = new DataTransfer(); t.setData("text/plain", "déposé"); return t; })(), clientX: 100, clientY: 100 });
  canvas.dispatchEvent(textDrop);
  await waitFor(() => message() === "Cette fonction n’est pas encore disponible avec le nouvel éditeur.", "dropped text is refused with a message");
  assert(textDrop.defaultPrevented && state().docLen === dropLength, "text drag and drop is refused, the document is unchanged");
  app.ribbon.selectTab("file");


  // --- images ------------------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "image.txt", format: "txt", bytes: encoder.encode("AB") };
  await clickAndWait(openButton, () => {}, () => message() === "Document ouvert." && state().docLen === 2);
  key(editor.element, "ArrowRight");
  await waitFor(() => state().focus === 1, "caret between A and B");
  app.ribbon.selectTab("insert");
  const imageCommand = document.querySelector("#image-command");
  assert(!imageCommand.disabled && imageCommand.textContent === "Image", "Image command is enabled in an editable v2 document");
  const fileInput = document.querySelector("#image-file-input");
  fileInput.click = () => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([redPng], "rouge.png", { type: "image/png" }));
    fileInput.files = transfer.files;
    fileInput.dispatchEvent(new Event("change"));
  };
  const redPng = await pngBytes("rgb(255,0,0)");
  const before = state().docLen;
  imageCommand.click();
  await waitFor(() => state().docLen === before + 3, "InsertImage adds one three-byte object at the caret");
  assert(message().includes("image insérée") && !status.classList.contains("error"), "success is announced");
  assert(state().dirty && state().canUndo && state().focus === 4, "image insertion is undoable and moves the caret after the object");
  assert(document.activeElement === editor.element, "focus returns to the document after the picker");
  await waitFor(() => mirror.querySelector("[role=img]"), "mirror exposes the picture");
  assert(mirror.querySelector("[role=img]").getAttribute("aria-label") !== "", "mirror image has an accessible label (alt text, or the no-alt message)");
  const exported = await editor.exportDocx();
  assert(exported.pictureCount === 1 && exported.bytes[0] === 0x50, "DOCX export carries the inserted picture");
  document.querySelector("#undo").click();
  await waitFor(() => state().docLen === before, "undo removes the picture");
  const pasteEvent = (types, files) => {
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { types, files, getData: () => "", setData() {} } });
    editor.element.dispatchEvent(event);
    return event;
  };
  const pasteEventResult = pasteEvent(["Files"], [new File([redPng], "colle.png", { type: "image/png" })]);
  await waitFor(() => state().docLen === before + 3, "pasted image is inserted");
  assert(pasteEventResult.defaultPrevented, "image paste is intercepted");
  const corrupt = await editor.insertImages([new File([encoder.encode("not an image")], "faux.png", { type: "image/png" })]);
  assert(corrupt.failed === 1 && corrupt.inserted === 0 && message().startsWith("Format d’image non pris en charge") && state().docLen === before + 3,
    "an invalid image is refused by the engine with the image message");
  const dropFile = new DragEvent("drop", { bubbles: true, cancelable: true, clientX: 90, clientY: 100,
    dataTransfer: (() => { const t = new DataTransfer(); t.items.add(new File([redPng], "depose.png", { type: "image/png" })); return t; })() });
  canvas.dispatchEvent(dropFile);
  await waitFor(() => state().docLen === before + 6, "a dropped image file is inserted at the drop point");
  assert(dropFile.defaultPrevented, "image drop is handled");
  app.ribbon.selectTab("file");

  // --- French spacing --------------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "vide.txt", format: "txt", bytes: new Uint8Array() };
  await clickAndWait(openButton, () => {}, () => state().docLen === 0 && message() === "Document ouvert.");
  const spacing = document.querySelector("#french-spacing");
  spacing.checked = true; spacing.dispatchEvent(new Event("change"));
  for (const c of "Bonjour") inputText(editor.element, c);
  inputText(editor.element, ":");
  await waitFor(async () => await text() === "Bonjour :", "French spacing inserts a no-break space before a colon");
  inputText(editor.element, "!");
  for (const c of " Ok") inputText(editor.element, c);
  inputText(editor.element, "?");
  await waitFor(async () => (await text()).endsWith("Ok ?"), "French spacing inserts a narrow no-break space before a question mark");
  spacing.checked = false; spacing.dispatchEvent(new Event("change"));
  inputText(editor.element, ";");
  await waitFor(async () => (await text()).endsWith("?;"), "French spacing off leaves punctuation alone");

  // --- mouse -----------------------------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "souris.txt", format: "txt", bytes: encoder.encode("premier mot deuxieme mot\nautre ligne") };
  await clickAndWait(openButton, () => {}, () => state().docLen === 36 && message() === "Document ouvert.");
  await waitFor(() => darkPixels(60, 60, 400, 140) > 20, "text painted before the mouse tests");
  // Pointer capture needs a trusted hardware event: only capture is shimmed, as in the v1 editor suite.
  let capturedPointer = null;
  canvas.setPointerCapture = id => { capturedPointer = id; };
  canvas.hasPointerCapture = id => capturedPointer === id;
  canvas.releasePointerCapture = id => { if (capturedPointer === id) capturedPointer = null; };
  const pointer = (type, x, y, detail = 1, modifiers = {}) => canvas.dispatchEvent(new PointerEvent(type,
    { pointerId: 9, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : 1, clientX: x, clientY: y, detail, bubbles: true, cancelable: true, ...modifiers }));
  const canvasBox = () => canvas.getBoundingClientRect();
  const lineOne = () => ({ x: canvasBox().left + state().caret.x, y: canvasBox().top + state().caret.y + 7 - scrollport.scrollTop });
  const start = lineOne();
  key(editor.element, "End");
  await waitFor(() => state().focus === 24, "End goes to the end of the first line");
  const end = lineOne();
  pointer("pointerdown", start.x + 1, start.y, 1);
  pointer("pointerup", start.x + 1, start.y, 1);
  await waitFor(() => state().anchor === 0 && state().focus === 0, "click puts the caret at the start of the line (HitTest setCaret)");
  pointer("pointerdown", start.x + 1, start.y, 1);
  pointer("pointermove", end.x - 1, end.y, 1);
  pointer("pointerup", end.x - 1, end.y, 1);
  await waitFor(() => state().anchor === 0 && state().focus >= 22, "drag selects the line (HitTest extend)");
  assert(await editor.selectedText(state()) === (await text()).slice(0, state().focus), "the dragged selection reads back through the window");
  pointer("pointerdown", start.x + 20, start.y, 2);
  pointer("pointerup", start.x + 20, start.y, 2);
  await sleep(100);
  assert(state().anchor === state().focus, "double-click is a plain click in v2 (no word selection: documented limit)");
  skipped.push("double-click word and triple-click paragraph selection: absent in v2 (docs/r7-ui-parity.md §7)");

  // --- clipboard ----------------------------------------------------------------------------------------------------------------
  const clipboard = (type, { fail = false } = {}) => {
    const written = [];
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { types: ["text/plain"], getData: () => "", setData(format, value) { if (fail) throw new Error("denied"); written.push([format, value]); } } });
    editor.element.dispatchEvent(event);
    return { event, written };
  };
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => state().focus === 0, "start of the document");
  for (let i = 0; i < 3; i += 1) key(editor.element, "ArrowRight", { shiftKey: true });
  await waitFor(() => state().focus === 3, "three characters selected");
  const copied = clipboard("copy");
  assert(copied.event.defaultPrevented && copied.written.length === 1 && copied.written[0][1] === "pre", "copy writes the selected text synchronously from the State window");
  const revisionBeforeCut = state().revision;
  const cut = clipboard("cut");
  await waitFor(async () => (await text()).startsWith("mier mot"), "cut removes the selection");
  assert(cut.written[0][1] === "pre" && state().revision > revisionBeforeCut, "cut writes the text then deletes the selection");

  // --- zoom and scrolling over a long document ----------------------------------------------------------------------------------------------
  const longLines = Array.from({ length: 3000 }, (_, i) => `ligne ${i} avec du texte à lire`);
  bridge.nextOpen = { name: "long.txt", format: "txt", bytes: encoder.encode(longLines.join("\r\n")) };
  await clickAndWait(openButton, () => {}, () => state().docLen > 50000 && message() === "Document ouvert.");
  await waitFor(() => state().layoutComplete && state().pageCount > 30, "background layout reaches the end of a 3000-line document", 30000);
  assert(editor.getViewportState().totalPages === state().pageCount && state().pageCount > 30, "page count follows the engine layout");
  const pageStatus = document.querySelector(".document-status-page");
  await waitFor(() => pageStatus.textContent === `Page 1 sur ${state().pageCount}`, "status bar shows the page count of the engine layout");
  editor.goToPage(12);
  await waitFor(() => editor.getViewportState().page === 12 && pageStatus.textContent === `Page 12 sur ${state().pageCount}`, "go-to-page scrolls and the status bar follows");
  await waitFor(() => darkPixels(60, 20, 400, 300) > 50, "the page reached by go-to-page is fetched and painted", 15000);
  editor.goToPage(1);
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === state().docLen, "select all on a long document");
  const bigCopy = await (async () => {
    for (let attempt = 0; attempt < 100; attempt += 1) {
      const result = clipboard("copy");
      if (result.written.length) return result;
      await sleep(20);
    }
    return clipboard("copy");
  })();
  assert(bigCopy.written.length === 1 && bigCopy.written[0][1] === longLines.join("\n"), "copy of a selection larger than the State window fetches the whole text");
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => state().focus === 0, "back to the start");
  scrollport.scrollTop = 0;
  const pageHeight = () => (841.89 + 16) * editor.getZoom();
  scrollport.scrollTop = pageHeight() * 20;
  await waitFor(() => darkPixels(60, 20, 400, 300) > 50, "pages far from the caret are fetched when scrolled into view", 15000);
  assert(editor.getViewportState().page >= 20, "viewport state follows the scroll position");
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => state().focus === state().docLen && editor.getViewportState().page === state().pageCount, "Ctrl+End scrolls the caret page into view", 15000);
  await waitFor(() => darkPixels(60, 20, 400, 300) > 50, "the last page is painted");
  editor.setZoom(0.5);
  await waitFor(() => editor.getZoom() === 0.5 && darkPixels(0, 0, 400, 400) > 20, "zoom 50 % repaints");
  editor.fitZoom("width");
  assert(editor.getZoomMode() === "width" && editor.getZoom() > 0.1, "fit width works on v2 pages");
  editor.setZoom(1);
  scrollport.scrollTop = 0;

  // --- selection too large for the clipboard -----------------------------------------------------------------------------------------
  const hugeLine = "x".repeat(99);
  bridge.nextOpen = { name: "enorme.txt", format: "txt", bytes: encoder.encode(Array.from({ length: 3200 }, () => hugeLine).join("\n")) };
  await clickAndWait(openButton, () => {}, () => state().docLen > 300000 && message() === "Document ouvert.", 20000);
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === state().docLen, "select all on a 320 KB document");
  const refused = clipboard("copy");
  await waitFor(() => message() === "La sélection est trop longue pour le presse-papiers (256 Kio au maximum).", "oversized copy is reported");
  assert(refused.written.length === 0 && refused.event.defaultPrevented, "oversized copy writes nothing and says why");
  const typed = state().docLen;
  inputText(editor.element, "W");
  await waitFor(() => state().docLen === 1, "typing replaces the huge selection");
  document.querySelector("#undo").click();
  await waitFor(() => state().docLen === typed, "undo restores the huge document");

  // --- forced resynchronisation: lost replies (a second editor on its own Worker, the reply is dropped after the engine ran) -------------------
  {
    const { createEngineClient } = await import("/engine-client.mjs");
    const { createRichEditor } = await import("/rich-editor.mjs");
    const wasm = await (await fetch("/atelier_core_wasm.wasm")).arrayBuffer();
    const real = await createEngineClient({ worker: new Worker("/engine-worker.mjs", { type: "module" }), bytes: wasm });
    const lose = { next: 0 };
    const lossy = { request: async (frame, options) => { const reply = await real.request(frame, options); if (lose.next > 0) { lose.next -= 1; throw new Error("reply lost"); } return reply; }, close: () => real.close() };
    const stage = document.createElement("div");
    stage.innerHTML = '<div class="document-scroll" style="height:420px;width:700px;overflow:auto"><div class="page-stack" style="position:relative"><canvas></canvas><div></div></div></div>';
    document.body.append(stage);
    const scroller = stage.querySelector(".document-scroll");
    const canvas2 = stage.querySelector("canvas");
    const errors2 = [];
    const second = createRichEditor({ client: lossy, host: { open: async () => ({ kind: "cancelled" }), save: async () => ({ kind: "cancelled" }), notifyDirty: async () => ({ kind: "ok" }) },
      canvas: canvas2, scrollport: scroller, spacer: stage.querySelector(".page-stack > div"), onError: error => errors2.push(error.code), t: (k) => k });
    await second.initialize();
    const dark2 = () => {
      const view = document.createElement("canvas"); view.width = canvas2.width; view.height = canvas2.height;
      const ctx = view.getContext("2d"); ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvas2, 0, 0);
      const data = ctx.getImageData(Math.round(60 * ratio), Math.round(60 * ratio), Math.round(300 * ratio), Math.round(60 * ratio)).data;
      let n = 0; for (let i = 0; i < data.length; i += 4) if (data[i] < 100) n += 1; return n;
    };
    for (const c of "ab") inputText(second.element, c);
    await waitFor(() => second.state.docLen === 2, "second editor typed");
    window.__dbg = () => ({ w: canvas2.width, h: canvas2.height, cw: canvas2.clientWidth, ch: canvas2.clientHeight, dark: dark2(), scroll: scroller.clientHeight, state: String(second.state?.pageCount) });
    // The blinking caret adds dark pixels: the minimum over one blink period leaves only the glyphs.
    const minDark = async () => { let low = Infinity; for (let i = 0; i < 9; i += 1) { low = Math.min(low, dark2()); await sleep(150); } return low; };
    await waitFor(async () => await minDark() >= 3, "second editor paints through the atlas");
    const darkBefore = await minDark();
    await second.whenLayoutComplete();
    lose.next = 1;
    inputText(second.element, "w"); // a glyph the atlas has never held: its entry is lost with the reply
    await waitFor(() => errors2.length === 1, "the lost reply is reported once");
    await waitFor(async () => await second.readText() === "abw" && second.state.docLen === 3, "the engine applied the edit and the client resynchronised");
    await waitFor(async () => await minDark() >= darkBefore + 3, "pages and atlas are rebuilt after the resync (the new glyph is drawn)", 15000);
    inputText(second.element, "z");
    await waitFor(async () => await second.readText() === "abwz", "the editor keeps working after the resync");
    lose.next = 2;
    inputText(second.element, "y");
    await waitFor(() => errors2.length >= 2, "a second lost reply is reported");
    await waitFor(async () => (await second.readText()).startsWith("abwzy") && !second.busy, "state converges after a lost State and a lost Resync");
    await waitFor(async () => (await second.readText()) === "abwzy" && await minDark() >= darkBefore + 6, "resync retried until the pages are back", 15000);
    assert(errors2.every(code => typeof code === "string") && second.state.dirty, "errors are typed and the document stays dirty");
    second.dispose();
    stage.remove();
    real.close();
  }

  // --- a second product instance: persisted reader mode, close during a save ---------------------------------------------------------------------
  app.dispose();
  await productPage();
  const { editorReady: secondReady } = await import("/app.mjs?second");
  const app2 = await secondReady;
  assert(app2.status === "ready" && app2.engine === "v2", "a fresh instance starts in v2 again");
  const toggle2 = document.querySelector(".document-mirror-reader-toggle");
  assert(localStorage.getItem("atelier.screenReaderMode") === "true" && toggle2.getAttribute("aria-pressed") === "true" && app2.editor.input.screenReaderMode(), "reader mode preference is restored");
  const writesBefore = bridge.files.length;
  bridge.holdSave = new Promise(resolve => setTimeout(resolve, 200));
  bridge.nextSave = {};
  document.querySelector("#save-document").click();
  await waitFor(() => app2.editor.busy, "second instance is saving");
  app2.dispose();
  await sleep(500);
  assert(bridge.files.length === writesBefore, "closing during the save dialog writes nothing");
  assert(pageErrors.length === 0, `no uncaught error or rejection during the whole run: ${pageErrors.join(" | ").slice(0, 300)}`);
  report = { status: "passed", checks, skipped, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, skipped, error: error.stack ?? String(error), userAgent: navigator.userAgent,
    context: { status: document.querySelector("#editor-status")?.textContent, mirror: document.querySelector(".document-mirror")?.outerHTML.slice(0, 1500), errors: pageErrors, dbg: window.__dbg?.() } };
}
resultNode.textContent = JSON.stringify(report, null, 2);
try { app?.dispose?.(); } catch { /* the page is going away */ }
await fetch("/__rich_result", { method: "POST", body: JSON.stringify(report) });
