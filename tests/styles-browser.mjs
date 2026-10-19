// SPDX-License-Identifier: AGPL-3.0-or-later
// US-250 : les styles (galerie, aperçu au survol, volet, raccourcis Titre 1 à 3) dans le produit réel (`app.mjs`), pont natif simulé (US-113 : tickets), vrai Worker et vrai WASM.
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
/** The text area of the first page, enlarged, as a PNG data URL: what the failure looked like. */
function cropPng() {
  try {
    const source = document.querySelector("#document-canvas");
    const out = document.createElement("canvas");
    out.width = 480; out.height = 240;
    const ctx = out.getContext("2d");
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, out.width, out.height);
    ctx.drawImage(source, 60, 60, 160, 80, 0, 0, 480, 240);
    return out.toDataURL("image/png");
  } catch { return null; }
}
try {
  const bridge = createFakeBridge();
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: bridge.webview } });
  localStorage.removeItem("atelier.screenReaderMode");
  localStorage.removeItem("atelier.recentFonts");
  localStorage.removeItem("atelier.stylesPane");
  localStorage.removeItem("atelier.stylesFilter");
  localStorage.setItem("atelier.frenchSpacing", "false");
  window.confirm = () => true;
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.engine === "v2", "product initializes with the v2 editor");
  const editor = app.editor;
  const controller = app.formatController;
  const styles = app.stylesPane;
  const canvas = document.querySelector("#document-canvas");
  const state = () => app.getState();
  const text = () => editor.readText();
  const snap = () => controller.snapshot().state;
  const cmd = id => document.querySelector(`[data-ribbon-command-id="${id}"]`);
  const ratio = devicePixelRatio || 1;
  app.ribbon.selectTab("home");

  // The canvas as the user sees it, on white.
  const image = () => {
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvas, 0, 0);
    return ctx.getImageData(0, 0, view.width, view.height);
  };
  const ink = (x0, y0, x1, y1, picture = image()) => {
    let total = 0;
    for (let y = Math.round(y0 * ratio); y < Math.round(y1 * ratio); y += 1) for (let x = Math.round(x0 * ratio); x < Math.round(x1 * ratio); x += 1) {
      const at = (y * picture.width + x) * 4;
      total += 1 - (picture.data[at] + picture.data[at + 1] + picture.data[at + 2]) / 765;
    }
    return total;
  };
  const AREA = [60, 50, 420, 160]; // the first lines of the first page, at 100 %
  const inkNow = () => ink(...AREA);
  /** The ink of the page once two readings agree (the page is repainted asynchronously). */
  const settled = async () => {
    let last = inkNow();
    for (let i = 0; i < 40; i += 1) { await sleep(60); const next = inkNow(); if (Math.abs(next - last) < 0.05) return next; last = next; }
    return last;
  };
  const waitState = (predicate, name) => waitFor(() => { const s = snap(); return s && s.revision === state().revision && predicate(s); }, name);
  const selectAll = async () => { key(editor.element, "a", { ctrlKey: true }); await waitFor(() => state().anchor === 0 && state().focus === state().docLen, "select all"); await waitState(s => !s.collapsed || state().docLen === 0, "state of the selection"); };
  const caretHome = async () => { key(editor.element, "Home", { ctrlKey: true }); await waitFor(() => state().focus === 0 && state().anchor === 0, "caret at the start"); await waitState(s => s.collapsed, "state of the caret"); };
  const tile = (id, root = cmd("styles-gallery")) => root.querySelector(`[data-style-id="${id}"]`);
  const list = () => cmd("styles-gallery").querySelector(".styles-gallery-list");
  const names = () => [...list().querySelectorAll(".styles-tile")].map(node => node.querySelector(".styles-tile-name").textContent);
  const paragraphStyle = () => snap().paragraph.style;
  const hover = node => node.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  const leave = root => root.dispatchEvent(new PointerEvent("pointerleave", { bubbles: false }));
  const previewing = () => styles.state.preview.active;
  const click = node => node.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  const keyup = (name, init = {}) => editor.element.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, cancelable: true, ...init, key: name }));
  const keydownInit = (init) => editor.element.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  // A keystroke with Ctrl+Alt (what Windows sends for AltGr); `typed` is the text the keystroke produces, if any.
  const chord = async (code, keyName, typed = null, extra = {}) => {
    keydownInit({ ctrlKey: true, altKey: true, code, key: keyName, ...extra });
    if (typed !== null) { inputText(editor.element, typed); }
    keyup(keyName, { code, ...extra });
    await sleep(60);
  };

  // --- the gallery in the ribbon ---------------------------------------------------------------------------------------------------------
  const group = cmd("styles-gallery").closest(".ribbon-group");
  const homeGroups = [...cmd("styles-gallery").closest(".ribbon-panel").querySelectorAll(".ribbon-group")].map(node => node.dataset.ribbonGroupId);
  assert(group.dataset.ribbonGroupId === "styles" && group.querySelector(".ribbon-group-label").textContent === "Styles", "the Home tab shows the group Styles");
  assert(homeGroups.indexOf("styles") === homeGroups.indexOf("para-paragraph") + 1 && homeGroups.indexOf("styles") > 0, `the group Styles comes after Paragraphe (${homeGroups})`);
  assert(JSON.stringify([...group.querySelectorAll("[data-ribbon-command-id]")].map(node => node.dataset.ribbonCommandId)) === JSON.stringify(["styles-gallery", "styles-pane"]), "the group holds the gallery and the pane button");
  await waitFor(() => !cmd("styles-gallery").hasAttribute("aria-disabled") || cmd("styles-gallery").getAttribute("aria-disabled") === "false", "gallery enabled");
  await waitFor(() => list().querySelectorAll(".styles-tile").length > 0, "the gallery holds styles");
  assert(list().getAttribute("role") === "listbox" && list().getAttribute("aria-label") === "Galerie de styles" && list().tabIndex === 0, "the gallery is a labelled listbox with one tab stop");
  const expected = ["Normal", "Sans interligne", "Titre 1", "Titre 2", "Titre 3", "Titre", "Sous-titre", "Citation", "Citation intense", "Accentuation", "Élevé"];
  assert(JSON.stringify(names()) === JSON.stringify(expected), `the gallery lists the quick styles in Word's order, in French (${names()})`);
  assert(tile("atelier.heading.1").querySelector(".styles-tile-name").style.fontWeight === "700" && tile("atelier.normal").querySelector(".styles-tile-name").style.fontWeight === "400"
    && parseFloat(tile("atelier.heading.1").querySelector(".styles-tile-name").style.fontSize) > parseFloat(tile("atelier.normal").querySelector(".styles-tile-name").style.fontSize), "each tile is drawn in its own style");
  assert(cmd("styles-pane").textContent === "Volet Styles" && cmd("styles-pane").getAttribute("aria-expanded") === "false" && cmd("styles-pane").getAttribute("aria-controls") === "styles-slot" && !cmd("styles-pane").hasAttribute("aria-haspopup"), "the pane button is named, controls the pane and is collapsed");

  inputText(editor.element, "Hello world");
  await waitFor(() => state().docLen === 11, "typed text");
  await waitFor(() => inkNow() > 20, "the text is painted");
  await waitState(() => true, "state of the typed text");
  const plainInk = await settled();
  const aria = id => tile(id).getAttribute("aria-selected");
  assert(aria("atelier.normal") === "true" && aria("atelier.heading.1") === "false" && snap().paragraph.style.value === "atelier.normal", "the current style (Normal) is marked in the gallery");

  // --- live preview on hover ----------------------------------------------------------------------------------------------------------------
  const revision = state().revision;
  hover(tile("atelier.title"));
  await waitFor(() => previewing(), "the preview is sent after the delay");
  await waitFor(async () => (await settled()) > plainInk * 1.4, "the page shows the title style while hovering");
  assert(state().revision === revision && snap().paragraph.style.value === "atelier.normal", "a preview changes neither the revision nor the model");
  assert(aria("atelier.normal") === "true", "the current style stays the real one while previewing");
  leave(list());
  await waitFor(() => !previewing(), "leaving cancels the preview");
  await waitFor(async () => Math.abs((await settled()) - plainInk) < plainInk * 0.03, "the page is back to its plain drawing");
  assert(state().revision === revision, "the revision is still the same after the cancel");
  // Sliding over several tiles: the last one wins, and leaving restores.
  hover(tile("atelier.heading.1")); hover(tile("atelier.heading.2")); hover(tile("atelier.title"));
  await waitFor(() => previewing() && styles.state.preview.shown.id === "atelier.title", "the last tile hovered is the one previewed");
  leave(list());
  await waitFor(async () => !previewing() && Math.abs((await settled()) - plainInk) < plainInk * 0.03, "the page is restored after a slide");
  // A hover shorter than the delay sends nothing.
  hover(tile("atelier.title")); leave(list());
  await sleep(300);
  assert(!previewing() && Math.abs((await settled()) - plainInk) < plainInk * 0.03, "a hover shorter than the delay never reaches the engine");
  // A keystroke ends the preview in the engine: the typed text is drawn without it.
  hover(tile("atelier.title"));
  await waitFor(async () => previewing() && (await settled()) > plainInk * 1.4, "previewing again");
  key(editor.element, "End", { ctrlKey: true });
  inputText(editor.element, "!");
  await waitFor(async () => await text() === "Hello world!", "the keystroke is typed during the preview");
  await waitFor(async () => (await settled()) < plainInk * 1.3, "the typed page no longer shows the preview");
  leave(list());
  await waitFor(() => !previewing(), "the cancel after the keystroke is harmless");
  assert(snap().paragraph.style.value === "atelier.normal", "the style is still Normal after a typed character ended the preview");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(async () => await text() === "Hello world", "Ctrl+Z after a preview undoes the keystroke, not a ghost step");
  await waitState(() => true, "state after undo");
  const inkBefore = await settled();

  // --- applying: one step, direct formatting kept -----------------------------------------------------------------------------------------
  await selectAll();
  key(editor.element, "i", { ctrlKey: true });
  await waitState(s => s.chars.italic.state === "value" && s.chars.italic.value === true, "italic on the whole text");
  const beforeApply = state().revision;
  hover(tile("atelier.quote"));
  await waitFor(() => previewing(), "preview of Citation");
  click(tile("atelier.heading.2"));
  await waitFor(() => snap() && snap().paragraph.style.state === "value" && snap().paragraph.style.value === "atelier.heading.2", "a click applies the style");
  await waitState(() => true, "state after the click");
  assert(state().revision === beforeApply + 1n && !previewing(), "one revision, no preview left");
  assert(snap().chars.italic.state === "value" && snap().chars.italic.value === true, "the direct italic is kept by the paragraph style");
  assert(snap().chars.bold.value === true, "and the style's own bold applies");
  assert(aria("atelier.heading.2") === "true" && aria("atelier.normal") === "false", "the gallery marks the new current style");
  assert(document.activeElement === editor.element, "the document gets the focus back");
  await waitFor(async () => (await settled()) > inkBefore * 1.15, "the page is drawn in the new style");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "one Ctrl+Z restores the style");
  await waitState(() => true, "state after the undo");
  assert(snap().chars.italic.value === true, "the direct italic survives the undo of the style");
  key(editor.element, "y", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.2", "Ctrl+Y applies it again");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "back to Normal");
  await waitState(() => true, "state back to Normal");

  // --- keyboard in the gallery: arrows preview, Enter applies, Escape cancels ---------------------------------------------------------------------
  const nowInk = await settled();
  list().focus();
  await waitFor(() => document.activeElement === list(), "the gallery takes the focus");
  assert(list().getAttribute("aria-activedescendant") === tile("atelier.normal").id, "the active tile is the current style");
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  assert(list().getAttribute("aria-activedescendant") === tile("atelier.heading.1").id, "the arrows move the active tile");
  await waitFor(() => previewing() && styles.state.preview.shown.id === "atelier.heading.1", "the active tile is previewed");
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  await waitFor(() => !previewing() && document.activeElement === editor.element, "Escape cancels the preview and gives the focus back");
  await waitFor(async () => Math.abs((await settled()) - nowInk) < nowInk * 0.03, "the page is restored after Escape");
  list().focus();
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.1", "Enter applies the active style");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "undone");
  await waitState(() => true, "state after undo");
  // Blur ends a preview.
  list().focus();
  list().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  await waitFor(() => previewing(), "preview by keyboard");
  editor.element.focus();
  await waitFor(() => !previewing(), "leaving the gallery cancels the preview");

  // --- the shortcuts Titre 1 to 3: QWERTY ---------------------------------------------------------------------------------------------------------
  await chord("Digit2", "2");
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.2", "Ctrl+Alt+2 applies Titre 2 (QWERTY: the key types nothing)");
  assert(await text() === "Hello world", "no character was typed");
  await chord("Digit1", "1");
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.1", "Ctrl+Alt+1 applies Titre 1");
  await chord("Digit3", "3");
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.3", "Ctrl+Alt+3 applies Titre 3");
  key(editor.element, "n", { ctrlKey: true, shiftKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "Ctrl+Maj+N applies Normal");
  assert(state().docLen === 11 && !document.querySelector("dialog:modal"), "Ctrl+Maj+N did not open a new document");
  // Ctrl+Maj+1 to 3: the layout-proof alternative (AZERTY has no Ctrl+Alt+2 or 3 that types nothing).
  for (const [code, level] of [["Digit3", 3], ["Digit2", 2], ["Digit1", 1]]) {
    editor.element.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ctrlKey: true, shiftKey: true, code, key: String(level) }));
    await waitFor(() => snap() && snap().paragraph.style.value === `atelier.heading.${level}`, `Ctrl+Maj+${level} applies Titre ${level}`);
  }
  key(editor.element, "n", { ctrlKey: true, shiftKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "Ctrl+Maj+N applies Normal again");
  // --- AZERTY: AltGr+é is « ~ », AltGr+" is « # »: the characters are typed, no heading is applied -----------------------------------------------
  key(editor.element, "End", { ctrlKey: true });
  await waitFor(() => state().focus === state().docLen && state().anchor === state().docLen, "caret at the end (typing must not replace a selection)");
  await waitState(s => s.collapsed, "state of the caret");
  const rev = state().revision;
  await chord("Digit2", "~", "~", { modifierAltGraph: true });
  await waitFor(async () => (await text()).endsWith("~") || (await text()).includes("~"), "AZERTY AltGr+é types « ~ »");
  await chord("Digit3", "#", "#", { modifierAltGraph: true });
  await waitFor(async () => (await text()).includes("#"), "AZERTY AltGr+\" types « # »");
  await waitState(() => true, "state after the characters");
  assert(snap().paragraph.style.value === "atelier.normal", "no heading was applied by the AltGr characters");
  assert(state().revision === rev + 2n, `exactly the two typed characters changed the document (${state().revision - rev})`);
  // AltGr+& types nothing on AZERTY: the heading applies, as Ctrl+Alt+1 does in Word.
  await chord("Digit1", "&", null, { modifierAltGraph: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.1", "AZERTY Ctrl+Alt+& (no character) applies Titre 1");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "one Ctrl+Z undoes the heading");
  const trail = [await text()];
  for (let i = 0; i < 3 && (await text()) !== "Hello world"; i += 1) { trail.push("z"); key(editor.element, "z", { ctrlKey: true }); await sleep(200); trail.push(await text()); } // each typed character is a step or a run
  assert((await text()) === "Hello world", `the typed characters are undone (${JSON.stringify(trail)})`);
  await waitState(() => true, "state after the undos");

  // --- the pane: list used / all, preview, current ---------------------------------------------------------------------------------------------------
  cmd("styles-pane").click();
  await waitFor(() => styles.pane && !document.querySelector("#styles-slot").hidden, "the pane opens in its slot");
  assert(cmd("styles-pane").getAttribute("aria-expanded") === "true", "the pane button is expanded");
  const paneList = () => styles.pane.querySelector(".styles-pane-list");
  const rows = () => [...paneList().querySelectorAll(".styles-row")].map(node => node.querySelector(".styles-tile-name").textContent);
  await waitFor(() => rows().length > 0, "the pane lists styles");
  assert(JSON.stringify(rows()) === JSON.stringify(["Normal"]), `the used styles of a new document: Normal only (${rows()})`);
  assert(styles.pane.querySelector(".styles-pane-current").textContent === "Style courant : Normal.", `the pane shows the current style (${styles.pane.querySelector(".styles-pane-current").textContent})`);
  const filter = styles.pane.querySelector(".styles-pane-filter-select");
  filter.value = "all"; filter.dispatchEvent(new Event("change", { bubbles: true }));
  await waitFor(() => rows().length > 12, "all the styles are listed");
  assert(rows().includes("Titre 4") && rows().includes("Lien hypertexte") && rows().includes("Légende") && rows()[0] === "Normal", `the full list holds the styles that are not in the gallery (${rows()})`);
  assert(paneList().querySelector('[data-style-id="atelier.normal"]').getAttribute("aria-selected") === "true", "the pane marks the current style");
  assert(paneList().getAttribute("role") === "listbox" && styles.pane.getAttribute("aria-label") === "Styles", "the pane list is a listbox in a labelled region");
  // Hover in the pane: its own sample and the preview in the document.
  const row = id => paneList().querySelector(`[data-style-id="${id}"]`);
  hover(row("atelier.heading.4"));
  await waitFor(() => styles.pane.querySelector(".styles-pane-sample-name").textContent.startsWith("Titre 4"), "the pane preview names the style under the pointer");
  await waitFor(() => previewing() && styles.state.preview.shown.id === "atelier.heading.4", "the pane hover previews in the document too");
  leave(paneList());
  await waitFor(() => !previewing(), "leaving the pane list cancels the preview");
  click(row("atelier.heading.4"));
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.4", "a click in the pane applies the style (also one that the gallery lacks)");
  await waitFor(() => styles.pane.querySelector(".styles-pane-current").textContent === "Style courant : Titre 4.", "the pane shows Titre 4 as current");
  await waitFor(() => row("atelier.heading.4")?.getAttribute("aria-selected") === "true", "the pane marks Titre 4");
  filter.value = "used"; filter.dispatchEvent(new Event("change", { bubbles: true }));
  await waitFor(() => JSON.stringify(rows()) === JSON.stringify(["Normal", "Titre 4"]), `the used styles now are Normal and Titre 4 (${rows()})`);
  assert(tile("atelier.heading.4") === null && names().every(name => name !== "Titre 4"), "the gallery keeps only the quick styles");
  // A character style from the pane, and its removal.
  filter.value = "all"; filter.dispatchEvent(new Event("change", { bubbles: true }));
  await waitFor(() => rows().length > 12, "all styles again");
  await selectAll();
  click(row("atelier.emphasis"));
  await waitFor(() => snap() && snap().chars.style.state === "value" && snap().chars.style.value === "atelier.emphasis", "a character style is applied to the selection");
  await waitFor(() => styles.pane.querySelector(".styles-pane-current").textContent.includes("Style de caractère : Accentuation."), "the pane shows the character style as current");
  const clear = styles.pane.querySelector(".styles-pane-clear");
  await waitFor(() => !clear.disabled, "the removal of the character style is offered");
  clear.click();
  await waitFor(() => snap() && snap().chars.style.state === "unset", "the character style is removed");
  key(editor.element, "z", { ctrlKey: true }); key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.4", "undo of both");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.normal", "undo of Titre 4");
  await waitState(() => true, "state after the undos");
  // Escape in the pane closes it and ends any preview.
  styles.pane.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  await waitFor(() => document.querySelector("#styles-slot").hidden && !previewing(), "Escape in the pane closes it");
  assert(cmd("styles-pane").getAttribute("aria-expanded") === "false", "the pane button is collapsed again");
  cmd("styles-pane").click();
  await waitFor(() => !document.querySelector("#styles-slot").hidden, "the pane opens again");
  document.querySelector(".styles-pane-close").click();
  await waitFor(() => document.querySelector("#styles-slot").hidden, "the close button closes the pane");

  // --- DOCX round trip: the styles, their names and the application ------------------------------------------------------------------------------
  await selectAll();
  click(tile("atelier.heading.1"));
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.1", "Titre 1 applied before saving");
  const files = bridge.files.length;
  bridge.nextSave = { format: "docx", name: "styles.docx" };
  document.querySelector("#save-document").click();
  await waitFor(() => bridge.files.length === files + 1, "the DOCX is written");
  const docx = bridge.files.at(-1).bytes;
  bridge.nextOpen = { name: "styles.docx", format: "docx", bytes: docx };
  document.querySelector("#open-docx").click();
  await waitFor(async () => await text() === "Hello world" && !state().dirty, "the DOCX is reopened");
  await waitFor(() => list().querySelectorAll(".styles-tile").length === expected.length, "the gallery is reloaded for the reopened document");
  assert(JSON.stringify(names()) === JSON.stringify(expected), `the names and the order of the styles survive the DOCX (${names()})`);
  await caretHome();
  await waitState(s => s.paragraph.style.state === "value", "state of the reopened document");
  assert(snap().paragraph.style.value === "atelier.heading.1" && aria("atelier.heading.1") === "true", "the applied style survives the DOCX and is current");
  assert(tile("atelier.title") && tile("atelier.normal"), "the tiles of the reopened document exist");
  // The preview of a reopened document: a fresh session.
  hover(tile("atelier.title"));
  await waitFor(() => previewing(), "preview in the reopened document");
  leave(list());
  await waitFor(() => !previewing(), "and its cancel");
  await chord("Digit3", "3");
  await waitFor(() => snap() && snap().paragraph.style.value === "atelier.heading.3", "Ctrl+Alt+3 works in the reopened document");

  // --- a plain-text document takes no style ----------------------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "brut.txt", format: "txt", bytes: encoder.encode("texte brut") };
  document.querySelector("#open-document").click();
  await waitFor(async () => await text() === "texte brut", "a TXT document is opened");
  await waitFor(() => controller.snapshot().plainText === true && cmd("styles-gallery").getAttribute("aria-disabled") === "true", "the gallery is disabled for a plain-text document");
  assert(list().getAttribute("aria-disabled") === "true" && list().tabIndex === -1, "its listbox is not a tab stop any more");
  cmd("styles-pane").click();
  await waitFor(() => !document.querySelector("#styles-slot").hidden, "the pane opens on a plain-text document");
  assert(styles.pane.querySelector(".styles-pane-list").getAttribute("aria-disabled") === "true", "and its list is disabled");
  document.querySelector(".styles-pane-close").click();
  const plainRevision = state().revision;
  await chord("Digit1", "1");
  await sleep(150);
  assert(!previewing() && state().revision === plainRevision && (await text()) === "texte brut", "the shortcuts change nothing in a plain-text document");

  assert(pageErrors.length === 0, `no uncaught error or rejection during the whole run: ${pageErrors.join(" | ").slice(0, 400)}`);
  report = { status: "passed", checks, skipped, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, skipped, error: error.stack ?? String(error), userAgent: navigator.userAgent,
    context: { status: document.querySelector("#editor-status")?.textContent, errors: pageErrors, png: cropPng() } };
}
resultNode.textContent = JSON.stringify(report, null, 2);
if (!/[?&]keep/.test(location.search)) { try { app?.dispose?.(); } catch { /* the page is going away */ } }
await fetch("/__format_result", { method: "POST", body: JSON.stringify(report) });
