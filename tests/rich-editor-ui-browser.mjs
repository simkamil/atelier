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
    launchReplies: [], notify(data) { for (const listener of listeners) listener({ data }); },
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
        if (operation === "launch") { const next = bridge.launchReplies.shift() ?? { status: "ok", payload: "" }; return reply(next.status, next.payload); }
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
  // US-327 : un document s'ouvre dans un nouvel onglet quand le document affiché est modifié ; `editor` désigne toujours celui de l'onglet actif.
  const editor = new Proxy({}, { get: (_, key) => { const target = app.editor; const value = target[key]; return typeof value === "function" ? value.bind(target) : value; } });
  const status = document.querySelector("#editor-status");
  // US-327 : le canvas et le défilement sont ceux du document affiché (celui qui porte les identifiants de index.html).
  const liveNode = id => new Proxy({}, {
    get: (_, key) => { const node = document.getElementById(id); const value = node[key]; return typeof value === "function" ? value.bind(node) : value; },
    set: (_, key, value) => { document.getElementById(id)[key] = value; return true; },
  });
  const canvasNode = () => document.getElementById("document-canvas");
  const canvas = liveNode("document-canvas");
  const scrollport = liveNode("document-scroll");
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
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvasNode(), 0, 0);
    const data = ctx.getImageData(Math.round(x0 * ratio), Math.round(y0 * ratio), Math.max(1, Math.round((x1 - x0) * ratio)), Math.max(1, Math.round((y1 - y0) * ratio))).data;
    let dark = 0;
    for (let i = 0; i < data.length; i += 4) if (data[i] < 100 && data[i + 1] < 100 && data[i + 2] < 100) dark += 1;
    return dark;
  };
  const pageRect = () => ({ left: 0, top: 0 });
  // R7b-1: colour of one Canvas pixel (page points at zoom 1, scrolled to the top).
  const pixelAt = (x, y) => {
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvasNode(), 0, 0);
    return [...ctx.getImageData(Math.round(x * ratio), Math.round(y * ratio), 1, 1).data];
  };
  const near = (rgb, expected, tolerance = 40) => expected.every((v, i) => Math.abs(rgb[i] - v) <= tolerance);
  assert(darkPixels(90, 20, 300, 140) === 0, "blank page area (beside the caret) is white");
  // US-202: a new document is the rich default profile, A4 with 25 mm margins (71 pt) used by the layout.
  assert(Math.abs(state().caret.x - 70.9) < 1 && Math.abs(state().caret.y - 70.9) < 1, "the blank document uses the A4 profile margins (25 mm)");

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

  // --- formatting controller (preparation of US-240 / US-243): one FormatState per change, UI unchanged ------------------------------------
  {
    const controller = app.formatController;
    const groupNamed = name => [...document.querySelectorAll("[data-ribbon-group-id]")].find(group => group.querySelector(".ribbon-group-label")?.textContent === name);
    const fontGroup = groupNamed("Police");
    const paragraphGroup = groupNamed("Paragraphe");
    assert(fontGroup && paragraphGroup && fontGroup.nextElementSibling === paragraphGroup
      && fontGroup.querySelector("input[role=combobox]")?.getAttribute("aria-label") === "Police",
      "the Home tab shows the groups Police then Paragraphe (filled by format-char.mjs and format-para.mjs)");
    let requests = 0;
    const original = editor.format.formatState;
    editor.format.formatState = (...args) => { requests += 1; return original.apply(editor.format, args); };
    const seen = [];
    const unsubscribe = controller.subscribe(snapshot => seen.push(snapshot));
    await sleep(150);
    const settled = () => requests;
    let snapshot = controller.snapshot();
    assert(snapshot.state?.type === "formatState" && snapshot.state.revision === state().revision && snapshot.readOnly === false
      && snapshot.plainText === false && snapshot.state.plain === false && snapshot.busy === false, `the controller holds the FormatState of the current revision (a new document is rich, not plain text) ${JSON.stringify({ t: snapshot.state?.type, r: String(snapshot.state?.revision), s: String(state().revision), ro: snapshot.readOnly, p: snapshot.plainText, b: snapshot.busy, n: requests })}`);
    assert(seen.length >= 1 && seen[0] === controller.snapshot(), "a subscriber receives the current snapshot at once and then the changes");
    inputText(editor.element, "Q");
    await waitFor(() => state().docLen === 2 && controller.snapshot().state?.revision === state().revision, "FormatState after typing");
    const afterCaret = settled();
    key(editor.element, "ArrowLeft");
    await waitFor(() => state().focus === 1 && controller.snapshot().state !== snapshot.state, "FormatState after a caret move");
    await sleep(150);
    assert(settled() === afterCaret + 1, `one FormatState request for one caret move (${settled() - afterCaret})`);
    const beforeTyping = settled();
    inputText(editor.element, "R");
    await waitFor(() => state().docLen === 3 && controller.snapshot().state?.revision === state().revision, "FormatState after a keystroke");
    await sleep(150);
    assert(settled() === beforeTyping + 1, `one FormatState request for one keystroke (${settled() - beforeTyping})`);
    // A command read back: state read once more and the focus returns to the document.
    snapshot = controller.snapshot();
    const beforeRun = settled();
    editor.setSuspended(true); await sleep(50);
    assert(controller.snapshot().busy === true && settled() === beforeRun, "a suspended editor is published as busy without a request");
    editor.setSuspended(false);
    await waitFor(() => controller.snapshot().busy === false, "busy cleared");
    document.querySelector("#undo").focus();
    await controller.run(client => client.formatState());
    assert(document.activeElement === editor.element, "the focus returns to the document after a formatting command");
    unsubscribe();
    editor.format.formatState = original;
    await controller.refresh();
    await text();
  }

  // --- mirror and reader mode ----------------------------------------------------------------------------------
  // US-327 : chaque onglet a son miroir ; `mirror` désigne celui de l'onglet affiché (celui qui porte l'identifiant #document-scroll).
  const mirror = new Proxy({}, { get: (_, key) => { const node = document.querySelector("#document-scroll .document-mirror"); const value = node?.[key]; return typeof value === "function" ? value.bind(node) : value; } });
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
  // US-327 : chaque ouverture crée un onglet (8 au plus) ; ce scénario ne s'intéresse qu'au document affiché : les autres onglets sont fermés avant chaque ouverture.
  const trimTabs = async () => { for (const tab of app.documentTabs.list) if (tab.id !== app.documentTabs.active) await app.documentTabs.close(tab.id, { confirm: false }); };
  const clickAndWait = async (button, scenario, ready) => { await trimTabs(); scenario(); button.click(); await waitFor(ready, `${button.id} flow`); };
  const openButton = document.querySelector("#open-document");
  const openDocxButton = document.querySelector("#open-docx");
  const saveButton = document.querySelector("#save-document");
  const message = () => status.textContent;
  await waitFor(() => !openButton.disabled, "open button enabled");
  // US-327 : ouvrir un fichier ne remplace plus un document modifié : il s'ouvre dans un nouvel onglet, sans rien demander ni perdre.
  let discardAsked = 0;
  window.confirm = () => { discardAsked += 1; return false; };
  assert(state().dirty, "the document is dirty before opening a file");
  const modifiedDocument = app.editor;
  const modifiedText = await modifiedDocument.readText();
  const tabsBeforeOpen = app.documentTabs.list.length;
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("un café trois\r\nseconde ligne") };
  await clickAndWait(openButton, () => {}, () => message() === "Document ouvert.");
  assert(discardAsked === 0 && app.documentTabs.list.length === tabsBeforeOpen + 1 && app.editor !== modifiedDocument, "a modified document is not replaced: the file opens in a new tab and nothing is asked");
  assert(await modifiedDocument.readText() === modifiedText && modifiedDocument.state.dirty, "the modified document keeps its text and its modifications in its own tab");
  window.confirm = () => true;
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
  const bannerActive = () => banner.dataset.active === "true";
  const bannerStatus = document.querySelector("#readonly-text");
  assert(!bannerActive() && bannerStatus.getAttribute("role") === "status" && bannerStatus.textContent === "" && document.querySelector("#readonly-copy").hidden, "read-only banner is one status region always in the DOM, empty when not read-only");
  bridge.nextOpen = { name: "basic.docx", format: "docx", bytes: await fixture("basic.docx") };
  await clickAndWait(openDocxButton, () => {}, () => message() === "Document ouvert.");
  assert(state().docLen > 0 && !state().readOnly && !bannerActive() && !state().dirty, "plain DOCX opens editable, without the banner");
  assert(/^Ouvrir un document\nfilter:docx=/.test(bridge.lastOpenSpec), "Open DOCX lists DOCX formats first");
  inputText(editor.element, "Z");
  await waitFor(() => state().dirty, "plain DOCX accepts typing");
  document.querySelector("#undo").click();
  await waitFor(() => !state().canUndo, "undo in DOCX");

  bridge.nextOpen = { name: "revisions.docx", format: "docx", bytes: await fixture("revisions.docx") };
  window.confirm = () => true;
  await clickAndWait(openDocxButton, () => {}, () => message().includes("lecture seule"));
  assert(state().readOnly && state().hasRevisions, "DOCX with revisions opens read-only");
  assert(bannerActive() && bannerStatus.textContent === "Document en lecture seule — modifications acceptées affichées." && banner.textContent.includes("Document en lecture seule — modifications acceptées affichées."), "banner announces read-only with accepted changes shown");
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
  await clickAndWait(copyButton, () => {}, () => !bannerActive());
  assert(!state().readOnly && !state().dirty && message().startsWith("Copie modifiable créée"), "editable copy replaces the read-only document and hides the banner");
  assert(await text() === revisionsText, "the copy shows the accepted text");
  inputText(editor.element, "Z");
  await waitFor(() => state().dirty, "the copy accepts typing");
  document.querySelector("#undo").click();
  await waitFor(() => !state().canUndo, "copy undo");

  bridge.nextOpen = { name: "comments.docx", format: "docx", bytes: await fixture("comments.docx") };
  await clickAndWait(openDocxButton, () => {}, () => state().readOnly);
  assert(bannerActive() && state().hasRevisions, "DOCX with comments opens read-only with the banner");
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("retour") };
  await clickAndWait(openButton, () => {}, () => !state().readOnly && !bannerActive());
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

  // --- search, replace, statistics, properties (R7b-2) -----------------------------------------------------------------
  app.ribbon.selectTab("home");
  for (const id of ["search-command", "replace-command", "statistics-command", "properties-command"]) {
    const button = document.querySelector(`#${id}`);
    for (const tab of ["home", "view", "file"]) { if (button.offsetParent !== null) break; app.ribbon.selectTab(tab); }
    assert(button && !button.disabled && !button.hasAttribute("aria-disabled") && !/nouvel éditeur/.test(button.title), `${id} is enabled in v2`);
  }
  assert(editor.capabilities.search && editor.capabilities.replace && editor.capabilities.statistics && editor.capabilities.properties
    && editor.capabilities.textDragDrop && !editor.capabilities.preview, "capabilities list what v2 offers now");
  const tintOf = () => {
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvasNode(), 0, 0);
    const { data } = ctx.getImageData(0, 0, view.width, view.height);
    let yellow = 0; let orange = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i] >= 250 && data[i + 1] >= 225 && data[i + 1] <= 245 && data[i + 2] >= 165 && data[i + 2] <= 195) yellow += 1;
      else if (data[i] >= 240 && data[i + 1] >= 195 && data[i + 1] <= 212 && data[i + 2] >= 150 && data[i + 2] <= 175) orange += 1;
      // The current occurrence is also the selection (R7b-1 draws its green highlight over the orange one).
      else if (data[i] >= 188 && data[i] <= 208 && data[i + 1] >= 171 && data[i + 1] <= 191 && data[i + 2] >= 133 && data[i + 2] <= 153) orange += 1;
    }
    return { yellow, orange };
  };
  const searchSlot = document.querySelector("#search-slot");
  const queryNode = () => document.querySelector("[data-search-query]");
  const replaceShown = () => document.querySelector("[data-replace-text]").offsetParent !== null;
  const setQuery = value => { queryNode().value = value; queryNode().dispatchEvent(new Event("input", { bubbles: true })); };
  const found = (count, name) => waitFor(() => app.navigation.searchResult?.revision === state().revision && app.navigation.searchResult.items.length === count, name);
  const findText = "pomme poire pomme\nPOMME fin";
  bridge.nextOpen = { name: "find.txt", format: "txt", bytes: encoder.encode(findText) };
  await clickAndWait(openButton, () => {}, () => message() === "Document ouvert." && state().docLen === byteLengthOf(findText));

  key(editor.element, "f", { ctrlKey: true });
  await waitFor(() => !searchSlot.hidden && document.activeElement === queryNode(), "Ctrl+F opens and focuses the search pane");
  assert(!replaceShown(), "Ctrl+F shows no replacement controls");
  setQuery("pomme");
  await found(3, "three occurrences of pomme");
  assert(document.querySelector("[data-search-count]").textContent.includes("3"), "the pane shows the number of occurrences");
  await waitFor(() => tintOf().yellow > 0, "every occurrence of the pane is highlighted on the Canvas");
  document.querySelector("[data-search-next]").click();
  await waitFor(() => state().anchor === 0 && state().focus === 5, "Next selects the first occurrence from the caret");
  await waitFor(() => tintOf().orange > 0, "the current occurrence is highlighted in another colour");
  document.querySelector("[data-search-next]").click();
  await waitFor(() => state().anchor === 12 && state().focus === 17, "Next selects the second occurrence");
  document.querySelector("[data-search-previous]").click();
  await waitFor(() => state().anchor === 0 && state().focus === 5, "Previous goes back");
  const matchCase = document.querySelector('[data-search-option="matchCase"]');
  matchCase.checked = true; matchCase.dispatchEvent(new Event("change", { bubbles: true }));
  await found(2, "match case leaves two occurrences");
  matchCase.checked = false; matchCase.dispatchEvent(new Event("change", { bubbles: true }));
  await found(3, "three occurrences again");
  key(queryNode(), "Escape");
  await waitFor(() => searchSlot.hidden, "Escape closes the pane");
  await waitFor(() => { const t = tintOf(); return t.yellow === 0 && t.orange === 0; }, "closing the pane clears the highlights");

  // Replace one, then all, from Ctrl+H; one undo takes the whole replacement back.
  key(editor.element, "h", { ctrlKey: true });
  await waitFor(() => !searchSlot.hidden && replaceShown(), "Ctrl+H opens the replacement controls");
  setQuery("pomme");
  await found(3, "occurrences for the replacement");
  document.querySelector("[data-search-result=\"0\"]").click();
  await waitFor(() => state().anchor === 0 && state().focus === 5, "choosing a result selects it");
  document.querySelector("[data-replace-text]").value = "kiwi";
  document.querySelector("[data-replace-one]").click();
  await waitFor(async () => await text() === "kiwi poire pomme\nPOMME fin", "the selected occurrence is replaced");
  await waitFor(() => document.querySelector("[data-replace-status]").textContent.replace(/\D/g, "") === "1", "the replacement count is announced");
  assert(state().dirty && state().canUndo, "replacing one occurrence is a dirty, undoable edit");
  await found(2, "the search follows the new revision");
  document.querySelector("[data-replace-all]").click();
  await waitFor(async () => await text() === "kiwi poire kiwi\nkiwi fin", "Replace all replaces every occurrence, case-insensitively");
  await waitFor(() => document.querySelector("[data-replace-status]").textContent.replace(/\D/g, "") === "2", "Replace all announces its count");
  document.querySelector("#undo").click();
  await waitFor(async () => await text() === "kiwi poire pomme\nPOMME fin", "one Undo takes Replace all back");
  setQuery("introuvable");
  await found(0, "no occurrence");
  const beforeNone = state().revision;
  document.querySelector("[data-replace-all]").click();
  await waitFor(() => /Aucune/i.test(document.querySelector("[data-replace-status]").textContent), "zero matches report none");
  assert(state().revision === beforeNone, "Replace all without occurrence changes nothing");
  key(queryNode(), "Escape");
  await waitFor(() => searchSlot.hidden, "pane closed");

  // Statistics: footer status and dialog.
  await waitFor(() => /mots?/.test(document.querySelector("[data-statistics-status]").textContent), "the footer shows the word count");
  const wordsStatus = document.querySelector("[data-statistics-status]").textContent;
  assert(/5 mots?/.test(wordsStatus.replace(/\s/g, " ")), `the footer counts the words of the document (${wordsStatus})`);
  const statisticsCommand = document.querySelector("#statistics-command");
  key(editor.element, "ArrowRight");
  await waitFor(() => state().anchor === state().focus, "a caret, no selection");
  statisticsCommand.click();
  await waitFor(() => document.querySelector("[data-statistics-dialog]")?.open, "the statistics dialog opens");
  const stat = name => document.querySelector(`[data-statistics-count="${name}"]`).textContent.replace(/\s/g, " ").trim();
  await waitFor(() => stat("document:words") === "5", "document words");
  assert(stat("document:paragraphs") === "2" && stat("document:lines") === "2" && stat("document:pages") === "1"
      && stat("document:characters") === "22" && stat("document:charactersWithSpaces") === "25", "document counts: paragraphs, lines, pages, characters, with spaces");
  assert(stat("selection:words") === "—" || stat("selection:words") === "0", "no selection, no selection count");
  document.querySelector("[data-statistics-dialog] .ui-dialog-close").click();
  await waitFor(() => !document.querySelector("[data-statistics-dialog]").open, "the statistics dialog closes");
  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === state().docLen, "select all for the selection counts");
  await waitFor(() => /sélection|\//i.test(document.querySelector("[data-statistics-status]").textContent), "the footer shows the selection");
  key(editor.element, "ArrowLeft");
  await waitFor(() => state().anchor === state().focus, "selection collapsed");

  // Properties: dialog on the open document, apply, modified, saved with the file.
  bridge.nextOpen = { name: "proprietes.txt", format: "txt", bytes: encoder.encode("un deux trois quatre cinq") };
  await clickAndWait(openButton, () => {}, () => message() === "Document ouvert." && state().docLen === 25 && !state().dirty);
  const propertyDialog = app.properties?.element;
  assert(propertyDialog instanceof HTMLDialogElement, "the properties dialog is part of the app");
  const propertiesCommand = document.querySelector("#properties-command");
  const propertyField = name => document.querySelector(`#${propertyDialog.querySelector(`label[for$="-${name}"]`).htmlFor}`);
  const typeProperty = (name, value) => { const input = propertyField(name); input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); };
  const openProperties = async () => { propertiesCommand.focus(); propertiesCommand.click(); await waitFor(() => propertyDialog.open, "properties dialog opens"); };
  const closedProperties = () => waitFor(() => !propertyDialog.open, "properties dialog closes");
  await openProperties();
  assert(document.activeElement === propertyField("title") && propertyField("title").value === "", "focus lands on the Title field, empty for a document without properties");
  assert(propertyDialog.querySelector(".properties-readonly").textContent.includes("5"), "the read-only counts follow the live document");
  typeProperty("title", "é".repeat(256));
  assert(propertyField("title").getAttribute("aria-invalid") === "true" && propertyDialog.querySelector(".properties-ok").disabled, "a title over the limit is refused, OK disabled");
  typeProperty("title", "Rapport « Œuvre »");
  typeProperty("author", "Élodie");
  propertyDialog.querySelector(".properties-cancel").click();
  await closedProperties();
  assert(document.activeElement === propertiesCommand && !state().dirty, "Cancel keeps the document clean and restores the focus");
  await openProperties();
  assert(propertyField("title").value === "" && propertyField("author").value === "", "Cancel applied nothing");
  typeProperty("title", "Rapport « Œuvre »");
  typeProperty("keywords", "un; deux");
  propertyDialog.querySelector(".properties-ok").click();
  await closedProperties();
  assert(state().dirty && state().revision === 0n && document.querySelector("#document-state").textContent === "Modifications non enregistrées",
    "applying properties marks the document modified without a text edit");
  await openProperties();
  assert(propertyField("title").value === "Rapport « Œuvre »" && propertyField("keywords").value === "un; deux", "reopening reads the values back");
  propertyDialog.querySelector(".properties-cancel").click();
  await closedProperties();
  bridge.nextSave = { format: "docx", name: "find.docx" };
  const savedBefore = bridge.files.length;
  await clickAndWait(saveButton, () => {}, () => bridge.files.length === savedBefore + 1 && message() === "Document enregistré.");
  assert(!state().dirty, "saving clears the modified state raised by the properties");
  bridge.nextOpen = { name: "find.docx", format: "docx", bytes: bridge.files.at(-1).bytes };
  await clickAndWait(openDocxButton, () => {}, () => message() === "Document ouvert.");
  await openProperties();
  assert(propertyField("title").value === "Rapport « Œuvre »" && propertyField("keywords").value === "un; deux", "the properties were written in the saved DOCX");
  propertyDialog.querySelector(".properties-cancel").click();
  await closedProperties();

  // A read-only document is searched, never replaced.
  bridge.nextOpen = { name: "revisions.docx", format: "docx", bytes: await fixture("revisions.docx") };
  await clickAndWait(openDocxButton, () => {}, () => state().readOnly);
  key(editor.element, "h", { ctrlKey: true });
  await waitFor(() => !searchSlot.hidden, "Ctrl+H opens the pane in a read-only document");
  assert(!replaceShown(), "no replacement controls are offered in a read-only document");
  setQuery("fin");
  await waitFor(() => app.navigation.searchResult?.revision === state().revision && app.navigation.searchResult.items.length >= 1, "a read-only document is searched");
  const readOnlyHit = app.navigation.searchResult.items[0];
  await editor.selectSearchResult(app.navigation.searchResult, 0);
  assert(state().anchor === readOnlyHit.start && state().focus === readOnlyHit.end, "a result of a read-only document can be selected");
  const failure = await editor.replaceAll({ revision: state().revision, query: "fin", replacement: "x", options: {} }).catch(error => error.code);
  assert(failure === "EDITOR_READ_ONLY" && !state().dirty, "the engine refuses the replacement of a read-only document");
  key(queryNode(), "Escape");
  await waitFor(() => searchSlot.hidden, "pane closed in read-only");
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("retour") };
  await clickAndWait(openButton, () => {}, () => !state().readOnly && !bannerActive());

  key(editor.element, "a", { ctrlKey: true });
  await waitFor(() => state().anchor === 0 && state().focus === 6, "select all of the text");
  key(editor.element, "f", { ctrlKey: true });
  await waitFor(() => !searchSlot.hidden && queryNode().value === "retour", "Ctrl+F seeds the query with the selected text");
  key(queryNode(), "Escape");
  await waitFor(() => searchSlot.hidden, "pane closed after the seed check");
  const dropLength = state().docLen;
  const textDrop = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: (() => { const t = new DataTransfer(); t.setData("text/plain", "déposé"); return t; })(), clientX: 100, clientY: 100 });
  canvas.dispatchEvent(textDrop);
  await waitFor(() => state().docLen === dropLength + 8, "dropped text is inserted at the drop point (R7c)");
  assert(textDrop.defaultPrevented && editor.capabilities.textDragDrop, "text drag and drop is offered in v2: the drop is handled, not refused");
  await editor.undo();
  await waitFor(() => state().docLen === dropLength, "one undo removes the dropped text");
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
  // R7b-1: the page draws the picture (its bitmap comes from the engine) at its size, on the line of the text.
  scrollport.scrollTop = 0;
  await waitFor(() => editor.pictureBoxes().length === 1, "the page draws the inserted picture");
  {
    const box = editor.pictureBoxes()[0].bounds;
    assert(Math.abs(box.width - 30) < 0.05 && Math.abs(box.height - 15) < 0.05, "a 40 x 20 px picture is drawn 30 x 15 pt (96 dpi)");
    await waitFor(() => near(pixelAt(box.min.x + box.width / 2, box.min.y + box.height / 2), [255, 0, 0]), "the picture pixels are painted on the Canvas");
    assert(editor.pictureBitmaps >= 1, "the bitmap of the picture is held by the client");
    assert(near(pixelAt(box.min.x + box.width + 40, box.min.y + 2), [255, 255, 255], 8), "the page beside the picture stays white");
  }
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
  // R7b-1: the selection is highlighted (rectangles from the engine, drawn by the overlay).
  await waitFor(() => editor.selectionRects.length >= 1, "the engine sends the rectangles of the selection");
  {
    const [rect] = editor.selectionRects;
    assert(rect.page === 0 && rect.width > 100 && rect.height > 5, "the highlight covers the selected line");
    await waitFor(() => near(pixelAt(rect.x + 2, rect.y + rect.height - 1), [202, 220, 215], 14), "the highlight is visible at the pixel (selection colour over white)");
    assert(near(pixelAt(rect.x + rect.width + 30, rect.y + rect.height - 1), [255, 255, 255], 6), "the page beyond the selection stays white");
  }
  assert(await editor.selectedText(state()) === (await text()).slice(0, state().focus), "the dragged selection reads back through the window");
  // R7b-1: double click selects the word, triple click the paragraph, dragging after a double click grows by words.
  pointer("pointerdown", start.x + 20, start.y, 1);
  pointer("pointerup", start.x + 20, start.y, 1);
  await waitFor(() => state().anchor === state().focus && state().focus > 0, "a click inside the first word");
  pointer("pointerdown", start.x + 20, start.y, 2);
  pointer("pointerup", start.x + 20, start.y, 2);
  await waitFor(() => state().anchor === 0 && state().focus === 7, "double click selects the word under the pointer");
  assert(await editor.selectedText(state()) === "premier", "the selected word reads back");
  await waitFor(() => editor.selectionRects.length === 1, "the word is highlighted");
  pointer("pointerdown", start.x + 20, start.y, 3);
  pointer("pointerup", start.x + 20, start.y, 3);
  await waitFor(() => state().anchor === 0 && state().focus === 25, "triple click selects the paragraph with its break");
  assert(await editor.selectedText(state()) === "premier mot deuxieme mot\n", "the selected paragraph reads back");
  pointer("pointerdown", start.x + 20, start.y, 2);
  pointer("pointermove", end.x - 12, end.y, 2);
  pointer("pointerup", end.x - 12, end.y, 2);
  await waitFor(() => state().anchor === 0 && state().focus === 24, "dragging after a double click grows the selection by words");
  pointer("pointerdown", start.x + 20, start.y, 1);
  pointer("pointerup", start.x + 20, start.y, 1);
  await waitFor(() => state().anchor === state().focus, "a single click collapses the selection again");
  await waitFor(() => editor.selectionRects.length === 0, "the highlight goes with the selection");

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

  // Ctrl+End on a document whose tail is not laid out yet: the engine answers within its budget with the selection but
  // no caret, the Layout pump goes on in the background, the caret shows and the view scrolls to it; the UI never freezes.
  const jumpLines = Array.from({ length: 8000 }, (_, i) => `saut ${i} avec du texte a lire`);
  const jumpLen = jumpLines.join("\n").length;
  bridge.nextOpen = { name: "saut.txt", format: "txt", bytes: encoder.encode(jumpLines.join("\n")) };
  await clickAndWait(openButton, () => {}, () => state().docLen === jumpLen);
  const wasComplete = state().layoutComplete;
  let worstGap = 0;
  let lastFrame = performance.now();
  let probing = true;
  const probe = () => { const now = performance.now(); worstGap = Math.max(worstGap, now - lastFrame); lastFrame = now; if (probing) requestAnimationFrame(probe); };
  requestAnimationFrame(probe);
  const jumpStart = performance.now();
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => state().focus === state().docLen, "Ctrl+End moves the selection at once on a long document being laid out", 3000);
  const sawNoCaret = state().caret === null && !state().layoutComplete;
  await waitFor(() => state().caret !== null, "the caret shows once the layout reaches the end of the document", 30000);
  const caretPageTop = () => (state().caret.page * (841.89 + 16) + state().caret.y) * editor.getZoom();
  await waitFor(() => scrollport.scrollTop + scrollport.clientHeight >= caretPageTop() && scrollport.scrollTop <= caretPageTop(), `the view follows the caret (scrollTop ${scrollport.scrollTop}, caret at ${caretPageTop()}, complete ${state().layoutComplete})`, 15000);
  probing = false;
  const jumpTime = performance.now() - jumpStart;
  assert(scrollport.scrollTop > 0 && darkPixels(0, 0, 400, 400) >= 0, "the view scrolled to the caret");
  assert(worstGap < 400, `the page stayed responsive while the caret was reached (worst frame gap ${Math.round(worstGap)} ms)`);
  console.log(JSON.stringify({ jumpToCaretMs: Math.round(jumpTime), worstFrameGapMs: Math.round(worstGap), openedIncomplete: !wasComplete, selectionWithoutCaretSeen: sawNoCaret }));
  // A keystroke typed after the jump lands at the end of the document.
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => state().focus === 0, "back to the start after the jump");
  await waitFor(() => state().layoutComplete, "the layout finishes", 60000);
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

  // --- R7c: the utilities (symbols, NBSP, context menu, clipboard, About) through the product with the v2 editor ------------------------
  {
    const { checkEditorUtilitiesV2 } = await import("/tests/editor-utilities-integration-v2.mjs");
    const openAppText = async value => { window.confirm = () => true; await editor.openDocument(async () => ({ kind: "ok", bytes: encoder.encode(value), format: "txt" })); };
    await checkEditorUtilitiesV2({ app, assert, waitFor, key, inputText, openText: openAppText });
    const { checkDocumentStatusV2 } = await import("/tests/document-status-integration-v2.mjs");
    await checkDocumentStatusV2({ app, assert, waitFor, key, inputText, openText: openAppText });
  }

  // --- R7c: application shortcuts and documents launched by the shell (US-115) with the v2 editor -------------------------------------
  {
    const statusText = () => document.querySelector("#editor-status").textContent;
    const callsBefore = bridge.calls.length;
    window.confirm = () => true;
    await editor.openDocument(async () => ({ kind: "ok", bytes: encoder.encode(""), format: "txt" }));
    inputText(editor.element, "N");
    await waitFor(async () => (await text()) === "N" && state().dirty, "app editor becomes dirty");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true, cancelable: true }));
    await waitFor(async () => state().docLen === 0 && !state().dirty, "Ctrl+N invokes the app New action");
    assert(bridge.calls.slice(callsBefore).every(message => !/\|(pickOpen|pickSave)\|/.test(message)), "Ctrl+N does not request file access");
    // Decision US-202: New is a rich document (default profile), saved as DOCX first under Document.docx; a TXT stays plain text.
    await app.formatController.refresh();
    assert(app.formatController.snapshot().state.plain === false && app.formatController.snapshot().plainText === false, "Ctrl+N creates a rich document (FormatState.plain is false)");
    const filesNew = bridge.files.length;
    bridge.nextSave = {};
    document.querySelector("#save-document").click();
    await waitFor(() => bridge.files.length === filesNew + 1, "the new document is saved");
    assert(/^Enregistrer le document\nfilter:docx=/m.test(bridge.lastSaveSpec) && /name:Document\.docx$/m.test(bridge.lastSaveSpec), "Save as proposes DOCX first and the name Document.docx for a new document");
    assert(decoder.decode(bridge.files.at(-1).bytes).includes("word/document.xml"), "the new document is written as a DOCX package");
    await editor.openDocument(async () => ({ kind: "ok", bytes: encoder.encode("brut"), format: "txt" }));
    await app.formatController.refresh();
    assert(app.formatController.snapshot().state.plain === true && app.formatController.snapshot().plainText === true, "a TXT document stays plain text");
    await editor.newDocument();
    await app.formatController.refresh();
    assert(app.formatController.snapshot().state.plain === false, "New after a TXT gives a rich document again");
    const before = bridge.calls.length;
    bridge.nextOpen = { cancelled: true };
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true, cancelable: true }));
    await waitFor(() => bridge.calls.slice(before).some(message => message.includes("|pickOpen|")), "Ctrl+O sends an open request to the native host");
    await waitFor(() => statusText().includes("annul"), "Ctrl+O cancellation is surfaced");
    const largeText = Array.from({ length: 300 }, () => "app page").join("\n");
    bridge.nextOpen = { name: "grand.txt", format: "txt", bytes: encoder.encode(largeText) };
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true, cancelable: true }));
    await waitFor(async () => (await text()) === largeText && !state().dirty, "Ctrl+O loads the multi-page fixture through the ticket dialogs");
    assert(document.documentElement.scrollHeight <= innerHeight && document.body.scrollHeight <= innerHeight,
      "a long v2 document does not grow the product body or move the toolbar and footer outside the viewport");
    const launchBase64 = value => b64(encoder.encode(value));
    const notifyLaunch = () => bridge.notify("us115-launch|1");
    const trimTabs = async () => { for (const tab of app.documentTabs.list) if (tab.id !== app.documentTabs.active) await app.documentTabs.close(tab.id, { confirm: false }); };
    const tabCount = () => app.documentTabs.list.length;
    await trimTabs();
    // US-115 / US-327 : un fichier lancé s'ouvre dans un nouvel onglet ; son identité (chemin) et ses octets arrivent ensemble.
    let tabsBefore = tabCount();
    bridge.launchReplies.push({ status: "ok", payload: `txt;0;${launchBase64("C:/dossier/argument.txt")};${launchBase64("lancé par argument")}` });
    notifyLaunch();
    await waitFor(async () => (await text()) === "lancé par argument", "a launched .txt opens in the v2 editor");
    assert(statusText() === "Document ouvert." && tabCount() === tabsBefore + 1, "a launched file opens in a new tab and the opening is announced");
    assert(app.documentTabs.list.find(tab => tab.id === app.documentTabs.active).title === "argument.txt", "the tab is named after the launched file");
    // Plusieurs fichiers : chacun dans son onglet, l'un après l'autre ; un fichier refusé ne cache pas le suivant.
    tabsBefore = tabCount();
    bridge.launchReplies.push(
      { status: "ok", payload: `txt;2;${launchBase64("C:/d/un.txt")};${launchBase64("premier")}` },
      { status: "error", payload: `files.error.unsupported_type;${launchBase64("C:/d/image.png")}` },
      { status: "ok", payload: `txt;0;${launchBase64("C:/d/trois.txt")};${launchBase64("troisième")}` });
    notifyLaunch();
    await waitFor(async () => (await text()) === "troisième" && tabCount() === tabsBefore + 2, "several launched files open one tab each");
    assert(app.documentTabs.list.slice(-2).map(tab => tab.title).join("|") === "un.txt|trois.txt", "each launched file has its own tab, in launch order");
    assert(statusText().includes("2 documents ouverts") && statusText().includes("image.png") && statusText().includes(".txt ou .docx"), "the refused file is named and explained in the same message");
    // Le même fichier relancé : son onglet est affiché, rien n'est rouvert.
    tabsBefore = tabCount();
    bridge.launchReplies.push({ status: "ok", payload: `txt;0;${launchBase64("C:/D/UN.txt")};${launchBase64("autre contenu")}` });
    notifyLaunch();
    await waitFor(async () => (await text()) === "premier", "a file already open is switched to, not reopened");
    assert(tabCount() === tabsBefore && statusText().includes("déjà ouvert"), "no duplicate tab and the user is told");
    bridge.launchReplies.push({ status: "error", payload: "files.error.unsupported_type" });
    notifyLaunch();
    await waitFor(() => statusText().includes(".txt ou .docx"), "an unsupported launched file is explained");
    assert((await text()) === "premier", "a refused launched file leaves the document unchanged");
    bridge.launchReplies.push({ status: "ok", payload: "txt;0;;***" });
    notifyLaunch();
    await sleep(50);
    assert((await text()) === "premier", "a malformed launch reply is ignored");
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
