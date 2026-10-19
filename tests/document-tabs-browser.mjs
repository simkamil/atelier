// SPDX-License-Identifier: AGPL-3.0-or-later
// US-327 / US-115 (critère 2) : plusieurs documents dans le produit réel (`app.mjs`, éditeur riche v2, vrai Worker et vrai WASM), pont natif simulé.
// Trois onglets, frappe et annulation indépendantes, zoom et défilement par onglet, Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+W, réordonner (glisser et
// clavier), fermeture d'un onglet modifié avec chacun des trois choix, copier-coller entre documents (fragment riche) et texte externe, limite de
// huit documents, perte du moteur et reprise (session perdue, Worker mort), mise en forme sur l'onglet actif après bascule, hôte agrégé, lancement.
const checks = [];
function assert(value, name) { if (!value) throw new Error(name); checks.push(name); }
async function waitFor(predicate, name, timeoutMs = 10000) {
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
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8");
const b64 = bytes => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s); };
const fromB64 = text => Uint8Array.from(atob(text), c => c.charCodeAt(0));
const inputText = (element, text) => element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: text, bubbles: true, cancelable: true }));
const key = (element, name, modifiers = {}) => element.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...modifiers }));
const globalKey = (name, modifiers = {}) => { const event = new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...modifiers }); document.dispatchEvent(event); return event; };

/** Pont natif simulé : dialogues, tickets, lancement, et les messages « dirty » du natif. */
function createFakeBridge() {
  const listeners = new Set();
  const bridge = {
    launchReplies: [], notify(data) { for (const listener of listeners) listener({ data }); },
    calls: [], files: [], nextOpen: { cancelled: true }, nextSave: {}, reads: new Map(), writes: new Map(), serial: 1,
    dirtyCalls: () => bridge.calls.filter(call => /\|dirty\|/.test(call)),
    webview: {
      postMessage(message) {
        bridge.calls.push(message);
        const match = /^us120-file\|1\|([1-9][0-9]*)\|(\w+)\|(.*)$/.exec(message);
        if (!match || match[2] === "dirty") return;
        const [, id, operation, payload] = match;
        const reply = (status, body = "") => queueMicrotask(() => { for (const listener of listeners) listener({ data: `us120-file|1|${id}|${status}|${body}` }); });
        if (operation === "pickOpen") {
          const next = bridge.nextOpen;
          if (next.cancelled) return reply("cancelled");
          const ticket = `r${bridge.serial++}`;
          bridge.reads.set(ticket, next.bytes);
          return reply("ok", `${ticket};${next.format};${next.bytes.length};${b64(encoder.encode(next.name))}`);
        }
        if (operation === "readTicket") {
          const bytes = bridge.reads.get(payload); bridge.reads.delete(payload);
          return bytes ? reply("ok", `b64;${b64(bytes)}`) : reply("error", "files.error.unknown_ticket");
        }
        if (operation === "pickSave") {
          const spec = decoder.decode(fromB64(payload));
          const next = bridge.nextSave;
          if (next.cancelled) return reply("cancelled");
          const name = next.name ?? /^name:(.*)$/m.exec(spec)?.[1] ?? "Document.docx";
          const ticket = `w${bridge.serial++}`;
          bridge.writes.set(ticket, []);
          return reply("ok", `${ticket};${next.format ?? name.split(".").pop()};${b64(encoder.encode(name))}`);
        }
        if (operation === "writeTicket") {
          const [ticket, , last, data] = payload.split(";");
          const chunks = bridge.writes.get(ticket);
          if (!chunks) return reply("error", "files.error.unknown_ticket");
          chunks.push(fromB64(data));
          if (last === "1") {
            const bytes = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0)); let at = 0;
            for (const c of chunks) { bytes.set(c, at); at += c.length; }
            bridge.writes.delete(ticket); bridge.files.push({ ticket, bytes });
          }
          return reply("ok");
        }
        if (operation === "launch") { const next = bridge.launchReplies.shift() ?? { status: "ok", payload: "" }; return reply(next.status, next.payload); }
        if (operation === "recentList") return reply("ok", `b64;${b64(new Uint8Array())}`);
        return reply("ok");
      },
      addEventListener(type, listener) { if (type === "message") listeners.add(listener); },
      removeEventListener(type, listener) { if (type === "message") listeners.delete(listener); },
    },
  };
  return bridge;
}

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
  localStorage.setItem("atelier.frenchSpacing", "false");
  localStorage.removeItem("atelier.screenReaderMode");
  window.confirm = () => true;
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.documentTabs, "the product starts with the document tabs");
  const { richClipboardFragment, forgetRichClipboard } = await import("/rich-editor.mjs");
  const { encodeRichRequest } = await import("/rich-editor-protocol.mjs");
  const tabsApi = app.documentTabs;
  const status = document.querySelector("#editor-status");
  const list = () => tabsApi.list;
  const tabOf = index => list()[index];
  const active = () => list().find(tab => tab.id === tabsApi.active);
  const textOf = tab => tab.editor.readText();
  const type = text => inputText(app.editor.element, text);
  const typeAndWait = async text => { const before = app.editor.state.docLen; type(text); await waitFor(() => app.editor.state.docLen === before + new TextEncoder().encode(text).length, `typed ${text}`); };
  const ctrlZ = () => key(app.editor.element, "z", { ctrlKey: true });
  const newTab = async () => { const before = list().length; await waitFor(() => !document.querySelector("#new-document").disabled, "New is available"); document.querySelector("#new-document").click(); await waitFor(() => list().length === before + 1 && !app.editor.busy, "a new tab opens"); };
  const visiblePanels = () => [...document.querySelectorAll(".document-scroll")].filter(node => !node.hidden);
  const tabButtons = () => [...document.querySelectorAll(".document-tab-label")];
  const settleLayout = async () => { await app.editor.whenLayoutComplete(); await sleep(80); };

  // --- barre d'onglets : sémantique ---------------------------------------------------------------------------------------
  const bar = document.querySelector(".document-tabs");
  assert(bar && bar.getAttribute("aria-label") === "Documents ouverts" && tabButtons().length === 1, "the tab bar is a tablist with one tab");
  assert(tabButtons()[0].getAttribute("aria-selected") === "true" && tabButtons()[0].textContent.includes("Document 1"), "the first document is the selected tab, named Document 1");
  assert(visiblePanels().length === 1 && visiblePanels()[0].getAttribute("role") === "tabpanel" && visiblePanels()[0].id === "document-scroll", "one tab panel is shown and carries the ids of the page");

  // --- trois onglets, frappe et annulation indépendantes -------------------------------------------------------------------
  await typeAndWait("un");
  app.editor.setZoom(1.5);
  await sleep(150);
  const firstScroll = document.getElementById("document-scroll").scrollTop;
  const first = active();
  await newTab();
  await typeAndWait("deux");
  const second = active();
  await newTab();
  await typeAndWait("trois");
  const third = active();
  assert(list().length === 3 && tabButtons().length === 3 && new Set([first.docId, second.docId, third.docId]).size === 3, "three tabs, three distinct documents");
  assert(visiblePanels().length === 1 && document.querySelectorAll(".document-scroll").length === 3 && document.querySelectorAll("#document-canvas").length === 1, "three DOMs, one shown, the page ids belong to the shown one");
  assert(tabButtons().map(button => button.getAttribute("aria-selected")).join() === "false,false,true", "the new tab is selected");
  assert(tabButtons().every(button => /modifié/.test(button.getAttribute("aria-label"))), "every modified tab says so in its name");
  assert(await textOf(first) === "un" && await textOf(second) === "deux" && await textOf(third) === "trois", "each tab holds its own text");
  ctrlZ();
  await waitFor(async () => await textOf(third) === "", "Ctrl+Z undoes the typing of the shown tab");
  assert(await textOf(first) === "un" && await textOf(second) === "deux" && first.editor.state.dirty && second.editor.state.dirty, "the other tabs keep their text and their history");
  key(app.editor.element, "y", { ctrlKey: true });
  await waitFor(async () => await textOf(third) === "trois", "Ctrl+Y redoes it");
  assert(first.editor.state.canUndo && second.editor.state.canUndo, "the history of the other tabs is untouched");
  assert(bridge.dirtyCalls().at(-1).endsWith("|dirty|1"), "the window is modified when a tab is");

  // --- Ctrl+Tab, Ctrl+Maj+Tab ---------------------------------------------------------------------------------------------
  let event = globalKey("Tab", { ctrlKey: true });
  assert(event.defaultPrevented, "Ctrl+Tab is ours");
  await waitFor(() => tabsApi.active === first.id, "Ctrl+Tab goes round to the first tab");
  assert(document.activeElement === app.editor.element && app.editor.getZoom() === 1.5, "the focus is in the shown document and its zoom is kept");
  globalKey("Tab", { ctrlKey: true, shiftKey: true });
  await waitFor(() => tabsApi.active === third.id, "Ctrl+Maj+Tab goes back to the last tab");
  globalKey("Tab", { ctrlKey: true });
  await waitFor(() => tabsApi.active === first.id, "Ctrl+Tab again");
  globalKey("Tab", { ctrlKey: true });
  await waitFor(() => tabsApi.active === second.id, "Ctrl+Tab to the second tab");
  assert(tabButtons()[1].getAttribute("aria-selected") === "true" && visiblePanels().length === 1, "the selected tab and the shown panel follow");

  // --- zoom et défilement conservés par onglet -----------------------------------------------------------------------------
  const largeText = Array.from({ length: 300 }, (_, index) => `ligne ${index}`).join("\n");
  bridge.nextOpen = { name: "grand.txt", format: "txt", bytes: encoder.encode(largeText) };
  document.querySelector("#open-document").click();
  await waitFor(() => list().length === 4 && app.editor.state?.docLen > 1000, "a long text opens in a fourth tab");
  const fourth = active();
  await settleLayout();
  app.editor.setZoom(0.75);
  const scroller = () => document.getElementById("document-scroll");
  await waitFor(() => scroller().scrollHeight > scroller().clientHeight + 600, "the long document can scroll");
  scroller().scrollTop = 700;
  await waitFor(() => scroller().scrollTop > 600, "scrolled");
  const scrolled = scroller().scrollTop;
  globalKey("Tab", { ctrlKey: true });
  await waitFor(() => tabsApi.active === first.id, "to the first tab");
  assert(app.editor.getZoom() === 1.5 && Math.abs(scroller().scrollTop - firstScroll) < 2, `the first tab keeps its own zoom and scroll position (${app.editor.getZoom()}, ${scroller().scrollTop})`);
  assert(document.querySelector("#document-zoom input, #document-zoom select, #document-zoom button") !== null, "the zoom control is shown");
  globalKey("Tab", { ctrlKey: true, shiftKey: true });
  await waitFor(() => tabsApi.active === fourth.id, "back to the long document");
  assert(Math.abs(app.editor.getZoom() - 0.75) < 0.001 && Math.abs(scroller().scrollTop - scrolled) < 2, "the long document comes back at its zoom and scroll position");
  assert(app.getState().pageCount > 3, "its page count is the one of its own document");

  // --- fermer : Ctrl+W, onglet propre sans question ---------------------------------------------------------------------------
  const asked = [];
  window.confirm = text => { asked.push(text); return true; };
  await newTab();
  const blank = active();
  const countBefore = list().length;
  globalKey("w", { ctrlKey: true });
  await waitFor(() => list().length === countBefore - 1, "Ctrl+W closes a clean tab");
  assert(!document.querySelector("dialog[open]") && tabsApi.active === fourth.id, "no question for a clean tab, the neighbour is shown");
  assert(blank.editor.documentClosed === true, "closing a tab closes its document in the engine");

  // --- fermer un onglet modifié : Annuler, Ne pas enregistrer, Enregistrer ------------------------------------------------------
  await tabsApi.activate(third.id);
  const filesBefore = bridge.files.length;
  const choose = choice => { const dialog = document.querySelector("dialog[open]"); dialog.querySelector(`[data-choice="${choice}"]`).click(); };
  globalKey("w", { ctrlKey: true });
  await waitFor(() => document.querySelector("dialog[open]"), "closing a modified tab asks what to do");
  assert(/Enregistrer les modifications de Document 3/.test(document.querySelector("dialog[open]").textContent), "the question names the document");
  assert(["save", "discard", "cancel"].every(name => document.querySelector(`dialog[open] [data-choice="${name}"]`)), "Enregistrer, Ne pas enregistrer, Annuler");
  choose("cancel");
  await waitFor(() => !document.querySelector("dialog[open]"), "Annuler closes the question");
  assert(list().length === 4 && tabsApi.active === third.id && await textOf(third) === "trois", "Annuler keeps the tab and its text");
  globalKey("w", { ctrlKey: true });
  await waitFor(() => document.querySelector("dialog[open]"), "asked again");
  choose("discard");
  await waitFor(() => list().length === 3, "Ne pas enregistrer closes the tab");
  assert(bridge.files.length === filesBefore && !list().some(tab => tab.docId === third.docId), "nothing was written and the document is gone");
  await tabsApi.activate(second.id);
  bridge.nextSave = { format: "docx", name: "deux.docx" };
  globalKey("w", { ctrlKey: true });
  await waitFor(() => document.querySelector("dialog[open]"), "asked for the second tab");
  choose("save");
  await waitFor(() => list().length === 2, "Enregistrer writes then closes");
  assert(bridge.files.length === filesBefore + 1 && bridge.files.at(-1).bytes[0] === 0x50, "the document was written as a DOCX before closing");
  assert(tabsApi.active === first.id || tabsApi.active === fourth.id, "a neighbour is shown after the close");

  // --- réordonner : glisser et clavier ---------------------------------------------------------------------------------------
  await newTab(); await typeAndWait("cinq");
  const fifth = active();
  const order = () => tabsApi.bar.order().join();
  const initialOrder = [first.id, fourth.id, fifth.id].join();
  assert(order() === initialOrder && list().map(tab => tab.id).join() === initialOrder, "the tabs are in opening order");
  const rect = id => document.getElementById(`document-tab-${id}`).getBoundingClientRect();
  const mouse = (type, x, id = 1) => document.dispatchEvent(new PointerEvent(type, { pointerId: id, clientX: x, clientY: 10, button: 0, bubbles: true }));
  const grab = document.getElementById(`document-tab-${first.id}`);
  const center = id => rect(id).left + rect(id).width / 2;
  grab.dispatchEvent(new PointerEvent("pointerdown", { pointerId: 1, clientX: center(first.id), clientY: 10, button: 0, bubbles: true }));
  mouse("pointermove", center(first.id) + 3);
  assert(order() === initialOrder, "a movement under the threshold does not drag");
  mouse("pointermove", rect(fifth.id).right - 4);
  mouse("pointerup", rect(fifth.id).right - 4);
  await waitFor(() => order() === [fourth.id, fifth.id, first.id].join(), "dragging the first tab to the end reorders the bar");
  assert(list().map(tab => tab.id).join() === order(), "the application follows the bar");
  assert(/déplacé en position 3 sur 3/.test(status.textContent), "the move is announced");
  document.getElementById(`document-tab-${first.id}`).focus();
  document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, shiftKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => order() === [fourth.id, first.id, fifth.id].join(), "Alt+Maj+Gauche moves the focused tab one place to the left");
  assert(document.activeElement === document.getElementById(`document-tab-${first.id}`), "the focus stays on the moved tab");
  document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  await waitFor(() => tabsApi.active === fifth.id, "the arrow keys change tab");
  assert(await textOf(fourth) === largeText && await textOf(first) === "un", "reordering did not touch the documents");

  // --- copier-coller entre documents ------------------------------------------------------------------------------------------
  await newTab();
  const source = active();
  await typeAndWait("Paragraphe riche");
  key(app.editor.element, "a", { ctrlKey: true });
  await waitFor(() => app.editor.state.anchor === 0 && app.editor.state.focus === app.editor.state.docLen, "selected");
  const black = { style: "single", eighths: 24, space: 2, color: null };
  await app.formatController.run(client => client.applyCharFormat({ scope: "wordOrTyping", edits: [{ attr: "bold", op: "set", value: true }] }));
  await app.formatController.run(client => client.applyParaFormat({ edits: [{ attr: "borders", op: "set", value: { top: black, left: black, bottom: black, right: black, between: "none", bar: "none" } }] }));
  const describe = async tab => {
    const formatted = await tab.editor.format.formatState();
    return JSON.stringify({ chars: formatted.chars, paragraph: formatted.paragraph }, (_, value) => typeof value === "bigint" ? Number(value) : value);
  };
  await waitFor(async () => (await source.editor.format.formatState()).paragraph.borders.left.state === "value", "bold, bordered paragraph in the source");
  forgetRichClipboard();
  const copied = new DataTransfer();
  app.editor.element.dispatchEvent(new ClipboardEvent("copy", { clipboardData: copied, bubbles: true, cancelable: true }));
  assert(copied.getData("text/plain") === "Paragraphe riche", "the system clipboard receives the plain text");
  await waitFor(() => richClipboardFragment() !== null, "the rich fragment is kept in memory after the copy");
  await newTab();
  const target = active();
  const pasted = new DataTransfer(); pasted.setData("text/plain", copied.getData("text/plain"));
  app.editor.element.dispatchEvent(new ClipboardEvent("paste", { clipboardData: pasted, bubbles: true, cancelable: true }));
  await waitFor(async () => await textOf(target) === "Paragraphe riche", "the paragraph is pasted in the other tab");
  key(app.editor.element, "a", { ctrlKey: true });
  await waitFor(() => app.editor.state.anchor === 0 && app.editor.state.focus === app.editor.state.docLen, "selected in the target");
  await app.formatController.refresh();
  await waitFor(async () => (await target.editor.format.formatState()).paragraph.borders.left.state === "value", "the border came with the paragraph");
  assert(await describe(source) === await describe(target), "bold and the border are identical in the other document (nothing lost)");
  assert(await textOf(source) === "Paragraphe riche", "the source is untouched");
  ctrlZ();
  await waitFor(async () => await textOf(target) === "", "one Ctrl+Z undoes the whole paste in the target");
  assert(await textOf(source) === "Paragraphe riche", "the history of the source is separate");
  // Texte venu d'ailleurs : texte seul.
  await newTab(); await typeAndWait("x");
  key(app.editor.element, "a", { ctrlKey: true });
  const external = new DataTransfer(); external.setData("text/plain", "texte externe");
  app.editor.element.dispatchEvent(new ClipboardEvent("paste", { clipboardData: external, bubbles: true, cancelable: true }));
  await waitFor(async () => await textOf(active()) === "texte externe", "external text is pasted as text");
  key(app.editor.element, "a", { ctrlKey: true });
  await waitFor(() => app.editor.state.focus === app.editor.state.docLen && app.editor.state.anchor === 0, "selected");
  const externalState = await app.editor.format.formatState();
  assert(externalState.chars.bold.state !== "value" || externalState.chars.bold.value !== true, "external text does not take the bold of the earlier copy");
  assert(externalState.paragraph.borders.left.state !== "value", "external text brings no border");
  assert(richClipboardFragment() !== null, "the held fragment is not consumed by a text that does not match it");

  // --- mise en forme sur l'onglet actif après bascule (aperçu de style annulé) ---------------------------------------------------
  await tabsApi.activate(source.id);
  await waitFor(() => document.querySelector('[data-style-id="atelier.title"]'), "the style gallery is loaded for the shown tab");
  const ink = () => {
    const canvasNode = document.getElementById("document-canvas");
    const view = document.createElement("canvas"); view.width = canvasNode.width; view.height = canvasNode.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvasNode, 0, 0);
    const data = ctx.getImageData(0, 0, view.width, Math.min(view.height, 400)).data;
    let total = 0; for (let i = 0; i < data.length; i += 4) total += 1 - (data[i] + data[i + 1] + data[i + 2]) / 765;
    return total;
  };
  const settledInk = async () => { let last = ink(); for (let i = 0; i < 40; i += 1) { await sleep(60); const next = ink(); if (Math.abs(next - last) < 0.5) return next; last = next; } return last; };
  const plainInk = await settledInk();
  document.querySelector('[data-style-id="atelier.title"]').dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  await waitFor(() => app.stylesPane.state.preview.active, "a style preview is shown in the first document");
  await waitFor(async () => Math.abs(await settledInk() - plainInk) > plainInk * 0.04, "the preview changes what the page shows");
  await tabsApi.activate(target.id);
  assert(!app.stylesPane.state.preview.active, "switching tab ends the style preview");
  await tabsApi.activate(source.id);
  assert(Math.abs(await settledInk() - plainInk) < plainInk * 0.05, "back in the first document the preview is gone: its page is the plain one");
  await tabsApi.activate(target.id);
  assert(app.formatController.client === app.editor.format, "the format controller serves the shown document");
  type("z"); await waitFor(async () => (await textOf(target)) === "z", "typing goes to the shown tab only");
  key(app.editor.element, "a", { ctrlKey: true });
  await waitFor(() => app.editor.state.anchor === 0 && app.editor.state.focus === 1, "selected");
  await app.formatController.run(client => client.applyCharFormat({ scope: "wordOrTyping", edits: [{ attr: "italic", op: "set", value: true }] }));
  await waitFor(async () => (await target.editor.format.formatState()).chars.italic.value === true, "italic applied in the shown tab");
  assert((await source.editor.format.formatState()).chars.italic.value !== true, "the other tab did not take the formatting");
  assert(document.querySelectorAll(".para-marks-overlay").length <= 1, "at most one marks overlay exists, on the shown tab");
  document.querySelector("#para-marks, [data-ribbon-command-id=\"para-marks\"]")?.click?.();

  // --- limite de huit documents -----------------------------------------------------------------------------------------------
  while (list().length < 8) { await newTab(); await typeAndWait("n"); }
  assert(list().length === 8, "eight documents are open");
  const texts = await Promise.all(list().map(tab => textOf(tab)));
  document.querySelector("#new-document").click();
  await waitFor(() => /maximal de documents ouverts \(8\)/.test(status.textContent), "the ninth document is refused with a message");
  assert(status.classList.contains("error") && list().length === 8, "refused, still eight tabs");
  bridge.nextOpen = { name: "neuf.txt", format: "txt", bytes: encoder.encode("neuf") };
  document.querySelector("#open-document").click();
  await waitFor(() => /maximal de documents ouverts/.test(status.textContent) && status.classList.contains("error"), "opening a file is refused as well");
  assert(list().length === 8 && (await Promise.all(list().map(tab => textOf(tab)))).join("|") === texts.join("|"), "the eight other documents are intact");
  type("!"); await waitFor(async () => (await textOf(active())).endsWith("!"), "the shown document still works");
  const victim = list().find(tab => tab.id !== tabsApi.active && !tab.dirty) ?? list().find(tab => tab.id !== tabsApi.active);
  await tabsApi.close(victim.id, { confirm: false });
  assert(list().length === 7, "a tab is closed without a question when told so");
  bridge.nextOpen = { name: "neuf.txt", format: "txt", bytes: encoder.encode("neuf") };
  document.querySelector("#open-document").click();
  await waitFor(() => list().length === 8 && app.editor.state.docLen === 4, "a freed place (and its engine session) makes the opening possible again");

  // --- l'hôte agrégé : modifié tant qu'un onglet l'est ---------------------------------------------------------------------------
  assert(tabsApi.windowDirty === true && bridge.dirtyCalls().at(-1).endsWith("|dirty|1"), "the window is modified while a tab is");
  await waitFor(() => !tabsApi.flags.opening && !app.editor.busy && app.editor.canEdit, "the opening is over");
  const dirtyBefore = bridge.dirtyCalls().length;
  type("?"); await waitFor(async () => (await textOf(active())).includes("?"), "typed");
  assert(bridge.dirtyCalls().length === dirtyBefore, "the native side is told only when the aggregate changes");
  while (list().length > 1) { const other = list().find(tab => tab.id !== tabsApi.active); await tabsApi.close(other.id, { confirm: false }); }
  assert(list().length === 1 && tabButtons().length === 1, "all other tabs closed");
  await tabsApi.close(tabsApi.active, { confirm: false });
  assert(list().length === 1 && app.editor.state.docLen === 0 && !app.editor.state.dirty && visiblePanels().length === 1, "closing the last tab leaves a new blank document");
  await waitFor(() => bridge.dirtyCalls().at(-1).endsWith("|dirty|0"), "the window is clean again");
  assert(tabsApi.windowDirty === false, "the aggregate is clean");

  // --- US-115 : plusieurs fichiers lancés, un onglet chacun, l'un après l'autre ------------------------------------------------------
  const launchPayload = (path, text, remaining) => `txt;${remaining};${b64(encoder.encode(path))};${b64(encoder.encode(text))}`;
  bridge.launchReplies.push(
    { status: "ok", payload: launchPayload("C:/docs/a.txt", "alpha", 2) },
    { status: "ok", payload: launchPayload("C:/docs/b.txt", "beta", 1) },
    { status: "ok", payload: launchPayload("C:/docs/c.txt", "gamma", 0) });
  bridge.notify("us115-launch|1");
  await waitFor(() => list().length === 3 && !tabsApi.flags.opening && /ouverts/.test(status.textContent), "three launched files open in three tabs");
  assert(list().map(tab => tab.title).join("|") === "a.txt|b.txt|c.txt", "tabs are named after the files, in the order of the launch");
  assert((await Promise.all(list().map(tab => textOf(tab)))).join("|") === "alpha|beta|gamma", "each file is in its own tab");
  assert(/3 documents ouverts/.test(status.textContent) && tabsApi.active === list()[2].id, "the opening is announced and the last file is shown");
  bridge.launchReplies.push({ status: "ok", payload: launchPayload("C:/DOCS/B.txt", "autre", 0) });
  bridge.notify("us115-launch|1");
  await waitFor(() => tabsApi.active === list()[1].id, "a file already open is switched to");
  assert(list().length === 3 && /déjà ouvert/.test(status.textContent) && await textOf(list()[1]) === "beta", "no second tab for the same file");
  bridge.launchReplies.push({ status: "ok", payload: launchPayload("C:/docs/d.txt", "delta", 0) });
  bridge.notify("us115-launch|1");
  await waitFor(() => list().length === 4, "a file launched while the application is open gets a new tab");
  assert(await textOf(list()[3]) === "delta" && list()[3].title === "d.txt", "it holds the file");

  // --- le moteur perd ses sessions (trap) : tous les onglets passent à « perdu », reprise par onglet --------------------------------------
  await tabsApi.activate(list()[0].id);
  const lostTabs = list().map(tab => tab);
  for (const tab of lostTabs) await app.transport.request(encodeRichRequest({ docId: tab.docId, requestId: 900000 + tab.docId, command: { type: "close" } }));
  inputText(app.editor.element, "!");
  await waitFor(() => !document.querySelector("#engine-lost-banner").hidden, "the lost engine is announced");
  assert(document.querySelector("#engine-lost-banner").getAttribute("role") === "alert" && tabsApi.lost?.kind === "trap", "the banner is an alert and the loss is a lost session");
  assert(list().every(tab => tab.lost) && tabButtons().every(button => /erreur/.test(button.getAttribute("aria-label"))), "every tab is marked lost");
  assert(app.editor.element.disabled === true, "typing is cut");
  assert(app.editor.windowText.includes("alpha"), "the last text shown stays readable");
  assert(document.querySelector('#engine-lost-banner [data-action="restart"]').hidden && !document.querySelector('#engine-lost-banner [data-action="blank"]').hidden, "the banner offers to reopen, copy or replace by a blank document");
  for (let index = 0; index < lostTabs.length; index += 1) {
    await tabsApi.activate(list()[index].id);
    document.querySelector('#engine-lost-banner [data-action="blank"]').click();
    await waitFor(() => !list()[index].lost, `tab ${index + 1} is rebuilt by a new Open of its identifier`);
  }
  await waitFor(() => document.querySelector("#engine-lost-banner").hidden && tabsApi.lost === null, "the banner goes away when every tab is recovered");
  type("ok"); await waitFor(async () => await textOf(active()) === "ok", "the recovered document works");
  assert(lostTabs.every(tab => list().some(other => other.docId === tab.docId)), "the identifiers are the same");

  // --- Worker mort : nouveau Worker, nouveaux éditeurs -------------------------------------------------------------------------------------
  app.client.close();
  inputText(app.editor.element, "!");
  await waitFor(() => !document.querySelector("#engine-lost-banner").hidden && tabsApi.lost?.kind === "worker", "a dead Worker is announced");
  assert(!document.querySelector('#engine-lost-banner [data-action="restart"]').hidden, "the banner offers to restart the engine");
  document.querySelector('#engine-lost-banner [data-action="restart"]').click();
  await waitFor(() => document.querySelector("#engine-lost-banner").hidden && tabsApi.lost === null && !app.editor.busy, "the engine restarts");
  assert(list().length === 4 && list().every(tab => !tab.lost), "the tabs are kept");
  type("ça marche"); await waitFor(async () => await textOf(active()) === "ça marche", "the new engine works");

  assert(pageErrors.length === 0, `no page error: ${pageErrors.join(" | ").slice(0, 400)}`);
  report = { status: "passed", checks, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, error: String(error?.stack ?? error), pageErrors, status_text: document.querySelector("#editor-status")?.textContent, debug: (() => { try { return JSON.stringify({ busy: app.editor.busy, active: app.documentTabs.active, tabs: app.documentTabs.list.map(t => [t.id, t.title, t.dirty, t.editor.busy]), flags: app.documentTabs.flags, newDisabled: document.querySelector("#new-document").disabled }); } catch (e) { return String(e); } })(), userAgent: navigator.userAgent };
} finally {
  try { app?.dispose?.(); } catch { /* arrêt du produit */ }
}
resultNode.textContent = JSON.stringify(report, null, 2);
await fetch("/__tabs_result", { method: "POST", body: JSON.stringify(report) });
