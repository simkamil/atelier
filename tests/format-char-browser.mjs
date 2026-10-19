// SPDX-License-Identifier: AGPL-3.0-or-later
// US-240 : la mise en forme des caractères dans le produit réel (`app.mjs`), pont natif simulé (US-113 : tickets), vrai Worker et vrai WASM.
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
  localStorage.setItem("atelier.frenchSpacing", "false");
  window.confirm = () => true;
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.engine === "v2", "product initializes with the v2 editor");
  // US-327 : le document affiché change quand un document s'ouvre dans un nouvel onglet ; ces références désignent toujours celui de l'onglet actif.
  const liveNode = id => new Proxy({}, {
    get: (_, key) => { const node = document.getElementById(id); const value = node[key]; return typeof value === "function" ? value.bind(node) : value; },
    set: (_, key, value) => { document.getElementById(id)[key] = value; return true; },
  });
  const canvasNode = () => document.getElementById("document-canvas");
  const editor = new Proxy({}, { get: (_, key) => { const target = app.editor; const value = target[key]; return typeof value === "function" ? value.bind(target) : value; } });
  const controller = app.formatController;
  const canvas = liveNode("document-canvas");
  const state = () => app.getState();
  const text = () => editor.readText();
  const snap = () => controller.snapshot().state;
  const chars = () => snap().chars;
  const cmd = id => document.querySelector(`[data-ribbon-command-id="${id}"]`);
  const pressed = id => cmd(id).getAttribute("aria-pressed");
  const ratio = devicePixelRatio || 1;
  app.ribbon.selectTab("home");

  // The canvas as the user sees it, on white.
  const image = () => {
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvasNode(), 0, 0);
    return ctx.getImageData(0, 0, view.width, view.height);
  };
  /** Ink (0..1 per pixel) of a rectangle in page points. */
  const ink = (x0, y0, x1, y1, picture = image()) => {
    let total = 0;
    for (let y = Math.round(y0 * ratio); y < Math.round(y1 * ratio); y += 1) for (let x = Math.round(x0 * ratio); x < Math.round(x1 * ratio); x += 1) {
      const at = (y * picture.width + x) * 4;
      total += 1 - (picture.data[at] + picture.data[at + 1] + picture.data[at + 2]) / 765;
    }
    return total;
  };
  const count = (predicate, x0, y0, x1, y1, picture = image()) => {
    let found = 0;
    for (let y = Math.round(y0 * ratio); y < Math.round(y1 * ratio); y += 1) for (let x = Math.round(x0 * ratio); x < Math.round(x1 * ratio); x += 1) {
      const at = (y * picture.width + x) * 4;
      if (predicate(picture.data[at], picture.data[at + 1], picture.data[at + 2])) found += 1;
    }
    return found;
  };
  /** Lowest dark row of the text of the first line (no descender in "Hello world": the baseline). */
  const baseline = (picture = image()) => {
    for (let y = 130 * ratio; y > 60 * ratio; y -= 1) {
      for (let x = 76 * ratio; x < 200 * ratio; x += 1) if (picture.data[(y * picture.width + x) * 4] < 110) return y / ratio;
    }
    return NaN;
  };
  const TEXT = [76, 60, 150, 100]; // x0, y0, x1, y1 around "Hello world" at 11 pt
  /** The page with the caret at the start of the document (the selection highlight would tint the ink). */
  const view = async () => {
    key(editor.element, "Home", { ctrlKey: true });
    await waitFor(() => state().focus === 0 && state().anchor === 0, "caret at the start");
    // The page is repainted asynchronously: wait until two views agree.
    let picture = image();
    for (let i = 0; i < 30; i += 1) {
      await sleep(80);
      const next = image();
      const same = Math.abs(ink(76, 50, 260, 130, next) - ink(76, 50, 260, 130, picture)) < 0.05;
      picture = next;
      if (same) break;
    }
    return picture;
  };
  const waitState = (predicate, name) => waitFor(() => { const s = snap(); return s && s.revision === state().revision && predicate(s); }, name);
  const selectAll = async () => { key(editor.element, "a", { ctrlKey: true }); await waitFor(() => state().anchor === 0 && state().focus === state().docLen, "select all"); };
  const selectRange = async (from, to) => {
    key(editor.element, "Home", { ctrlKey: true });
    await waitFor(() => state().focus === 0, "home");
    for (let i = 0; i < from; i += 1) key(editor.element, "ArrowRight");
    for (let i = from; i < to; i += 1) key(editor.element, "ArrowRight", { shiftKey: true });
    await waitFor(() => state().anchor === from && state().focus === to, `selection ${from}..${to}`);
    await waitState(s => BigInt(s.anchor) === BigInt(from) && BigInt(s.focus) === BigInt(to), "format state of the selection");
  };
  const caretAt = async position => {
    key(editor.element, "Home", { ctrlKey: true });
    await waitFor(() => state().focus === 0, "home");
    for (let i = 0; i < position; i += 1) key(editor.element, "ArrowRight");
    await waitFor(() => state().focus === position && state().anchor === position, `caret ${position}`);
    await waitState(s => s.collapsed && BigInt(s.focus) === BigInt(position), "format state of the caret");
  };
  const press = async (id, until, name) => { cmd(id).click(); await waitFor(until, name); await waitState(() => true, `${name}: state read`); };

  // --- the group and its controls ------------------------------------------------------------------------------------------------------
  const group = cmd("bold").closest(".ribbon-group");
  assert(group?.dataset.ribbonGroupId === "font" && group.querySelector(".ribbon-group-label")?.textContent === "Police", "the Home tab shows the group Police");
  const order = [...group.querySelectorAll("[data-ribbon-command-id]")].map(node => node.dataset.ribbonCommandId);
  assert(JSON.stringify(order) === JSON.stringify(["font", "size", "bold", "italic", "underline", "strike", "subscript", "superscript", "color", "highlight"]), `the group holds the ten controls of US-240 in Word's order (${order})`);
  assert(cmd("bold").getAttribute("aria-label") === "Gras" && cmd("italic").getAttribute("aria-label") === "Italique"
    && cmd("underline").querySelector(".ui-split-action").getAttribute("aria-label") === "Souligné" && cmd("strike").getAttribute("aria-label") === "Barré"
    && cmd("subscript").getAttribute("aria-label") === "Indice" && cmd("superscript").getAttribute("aria-label") === "Exposant"
    && cmd("font").querySelector("input").getAttribute("aria-label") === "Police" && cmd("size").querySelector("input").getAttribute("aria-label") === "Taille de police"
    && cmd("color").querySelector(".ui-color-trigger").getAttribute("aria-label") === "Couleur de police"
    && cmd("highlight").querySelector(".ui-color-trigger").getAttribute("aria-label") === "Surlignage", "every control has its accessible name");
  await waitFor(() => snap() && !cmd("bold").disabled, "controls enabled for a rich document");
  assert(cmd("font").getAttribute("aria-disabled") === "false" && cmd("bold").getAttribute("aria-pressed") === "false", "controls are enabled and unpressed on a blank document");
  assert(cmd("font").querySelector("input").value === "Carlito" && cmd("size").querySelector("input").value === "11", `the boxes show the font and the size of the caret (${cmd("font").querySelector("input").value} / ${cmd("size").querySelector("input").value} / ${JSON.stringify(snap()?.chars?.font)})`);

  inputText(editor.element, "Hello world");
  await waitFor(() => state().docLen === 11, "typed text");
  await waitFor(() => ink(...TEXT) > 20, "the text is painted");
  const plainPicture = await view();
  const plainInk = ink(...TEXT, plainPicture);
  const base = baseline(plainPicture);
  assert(base > 70 && base < 100, `the baseline of the first line is found (${base})`);
  const plainBelow = ink(TEXT[0], base + 1, TEXT[2], base + 5, plainPicture);

  // --- bold, italic, and the mixed state ---------------------------------------------------------------------------------------------------
  await selectRange(0, 5);
  await press("bold", () => chars().bold.state === "value" && chars().bold.value === true, "bold on the selection");
  assert(pressed("bold") === "true", "the bold button is pressed when the selection is bold");
  await selectAll();
  await waitState(s => s.chars.bold.state === "mixed", "mixed state");
  assert(pressed("bold") === "mixed", "the bold button shows the mixed state (aria-pressed=mixed)");
  await press("bold", () => chars().bold.state === "value" && chars().bold.value === true, "a mixed selection becomes bold");
  assert(pressed("bold") === "true", "bold is pressed for the whole selection");
  const boldInk = ink(...TEXT, await view());
  assert(boldInk > plainInk * 1.05, `bold text is drawn heavier (${boldInk.toFixed(1)} against ${plainInk.toFixed(1)})`);
  await selectAll();
  await press("bold", () => chars().bold.state === "value" && chars().bold.value === false, "bold off");
  assert(pressed("bold") === "false" && Math.abs(ink(...TEXT, await view()) - plainInk) < plainInk * 0.02, "removing bold restores the plain drawing");
  await selectAll();
  key(editor.element, "g", { ctrlKey: true });
  await waitFor(() => chars().bold.state === "value" && chars().bold.value === true, "Ctrl+G toggles bold");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => chars().bold.state === "value" && chars().bold.value === false, "one Ctrl+Z undoes one formatting");
  assert(!state().dirty || state().canUndo !== undefined, "undo restores the state in one step");
  key(editor.element, "i", { ctrlKey: true });
  await waitFor(() => chars().italic.state === "value" && chars().italic.value === true, "Ctrl+I toggles italic");
  assert(pressed("italic") === "true", "the italic button is pressed");
  key(editor.element, "i", { ctrlKey: true });
  await waitFor(() => chars().italic.value === false, "italic off");

  // --- strike-through ----------------------------------------------------------------------------------------------------------------------
  await press("strike", () => chars().strike.state === "value" && chars().strike.value === true, "strike on");
  assert(pressed("strike") === "true", "the strike button is pressed");
  const struck = await view();
  const through = ink(TEXT[0], base - 9, TEXT[2], base - 1, struck) - ink(TEXT[0], base - 9, TEXT[2], base - 1, plainPicture);
  assert(through > 8, `a line is drawn through the text (${through.toFixed(1)} more ink)`);
  await selectAll();
  await press("strike", () => chars().strike.value === false, "strike off");

  // --- underlines: the six styles are drawn differently ----------------------------------------------------------------------------------
  const underlineButton = cmd("underline").querySelector(".ui-split-action");
  const underlineMenu = cmd("underline").querySelector(".ui-split-disclosure");
  const inkBelow = picture => ink(TEXT[0], base + 1, TEXT[2], base + 5, picture);
  const rowsBelow = picture => {
    let rows = 0;
    for (let y = Math.round((base + 1) * ratio); y < Math.round((base + 6) * ratio); y += 1) {
      if (ink(TEXT[0], y / ratio, TEXT[2], (y + 1) / ratio, picture) > 4) rows += 1;
    }
    return rows;
  };
  underlineButton.click();
  await waitFor(() => chars().underline.state === "value" && chars().underline.value === "single", "the underline button toggles the simple underline");
  await waitState(() => true, "state after underline");
  assert(underlineButton.getAttribute("aria-pressed") === "true", "the underline button is pressed");
  let shown;
  await waitFor(async () => { shown = await view(); return inkBelow(shown) > plainBelow + 20; }, "the simple underline is painted");
  const single = inkBelow(shown);
  await selectAll();
  assert(single > plainBelow + 20, `a simple underline is drawn under the text (${single.toFixed(1)} against ${plainBelow.toFixed(1)})`);
  const measured = { single };
  measured.singleRows = rowsBelow(shown);
  for (const [style, label] of [["double", "Double"], ["thick", "Épais"], ["dotted", "Pointillé"], ["dashed", "Tirets"], ["wave", "Ondulé"]]) {
    underlineMenu.click();
    const item = [...cmd("underline").querySelectorAll(".ui-menu-item")].find(node => node.textContent === label);
    assert(item, `the underline menu offers ${label}`);
    item.click();
    await waitFor(() => chars().underline.state === "value" && chars().underline.value === style, `underline ${style}`);
    await waitState(() => true, `state after ${style}`);
    const previous = Math.round(inkBelow(shown));
    await waitFor(async () => { shown = await view(); return Math.round(inkBelow(shown)) !== previous; }, `the ${style} underline is painted`);
    measured[style] = inkBelow(shown);
    measured[`${style}Rows`] = rowsBelow(shown);
    await selectAll();
  }
  assert(measured.double > measured.single * 1.4, `the double underline carries more ink than the simple one (${measured.double.toFixed(1)} / ${measured.single.toFixed(1)})`);
  assert(measured.thick > measured.single * 1.4, `the thick underline is thicker (${measured.thick.toFixed(1)} / ${measured.single.toFixed(1)})`);
  assert(measured.dotted < measured.single * 0.8 && measured.dotted > measured.single * 0.15, `the dotted underline is broken (${measured.dotted.toFixed(1)} / ${measured.single.toFixed(1)})`);
  assert(measured.dashed < measured.single * 0.95 && measured.dashed > measured.dotted * 0.9, `the dashed underline has longer dashes (${measured.dashed.toFixed(1)})`);
  assert(measured.wave > 0 && measured.waveRows >= measured.singleRows, `the wavy underline is drawn (${measured.wave.toFixed(1)}, ${measured.waveRows} rows)`);
  assert(new Set([measured.single, measured.double, measured.thick, measured.dotted, measured.dashed, measured.wave].map(v => Math.round(v))).size === 6, "the six underline styles produce six different drawings");
  underlineMenu.click();
  [...cmd("underline").querySelectorAll(".ui-menu-item")].find(node => node.textContent === "Aucun soulignement").click();
  await waitFor(() => chars().underline.state === "value" && chars().underline.value === "none", "no underline");
  await waitState(() => true, "state after none");
  assert(underlineButton.getAttribute("aria-pressed") === "false", "the underline button is released");
  key(editor.element, "u", { ctrlKey: true });
  await waitFor(() => chars().underline.value === "single", "Ctrl+U toggles the simple underline");
  key(editor.element, "u", { ctrlKey: true });
  await waitFor(() => chars().underline.value === "none", "Ctrl+U again removes it");

  // --- superscript and subscript: the word "world" against "Hello" on the same line ------------------------------------------------------------
  const bottomOf = (picture, x0, x1) => {
    for (let y = 130 * ratio; y > 60 * ratio; y -= 1) {
      for (let x = x0 * ratio; x < x1 * ratio; x += 1) if (picture.data[(y * picture.width + x) * 4] < 110) return y / ratio;
    }
    return NaN;
  };
  const LEFT = [76, 95]; const RIGHT = [104, 135];
  const widthOf = picture => {
    let last = 0;
    for (let x = 76 * ratio; x < 200 * ratio; x += 1) {
      for (let y = 60 * ratio; y < 100 * ratio; y += 1) if (picture.data[(y * picture.width + x) * 4] < 110) { last = x / ratio; break; }
    }
    return last;
  };
  const plainWidth = widthOf(plainPicture);
  await selectRange(6, 11);
  await press("superscript", () => chars().verticalAlign.state === "value" && chars().verticalAlign.value === "superscript", "superscript on");
  assert(pressed("superscript") === "true" && pressed("subscript") === "false", "the superscript button is pressed, not the subscript one");
  let raised;
  await waitFor(async () => { raised = await view(); return widthOf(raised) < plainWidth - 4; }, "the superscript word is set smaller");
  const raisedGap = bottomOf(raised, ...LEFT) - bottomOf(raised, ...RIGHT);
  assert(raisedGap >= 2, `the superscript word sits above the line (${raisedGap}px higher)`);
  await selectRange(6, 11);
  await press("subscript", () => chars().verticalAlign.value === "subscript", "subscript on");
  assert(pressed("subscript") === "true" && pressed("superscript") === "false", "the subscript button replaces the superscript one");
  let lowered;
  await waitFor(async () => { lowered = await view(); return bottomOf(lowered, ...LEFT) - bottomOf(lowered, ...RIGHT) < raisedGap - 2; }, "the subscript word moves down");
  const loweredGap = bottomOf(lowered, ...LEFT) - bottomOf(lowered, ...RIGHT);
  assert(loweredGap <= -1 && widthOf(lowered) < plainWidth - 4, `the subscript word sits below the line, smaller (${loweredGap}px)`);
  await selectRange(6, 11);
  key(editor.element, "=", { ctrlKey: true });
  await waitFor(() => chars().verticalAlign.value === "baseline", "Ctrl+= toggles the subscript off");
  key(editor.element, "+", { ctrlKey: true });
  await waitFor(() => chars().verticalAlign.value === "superscript", "Ctrl++ sets the superscript");
  key(editor.element, "+", { ctrlKey: true });
  await waitFor(() => chars().verticalAlign.value === "baseline", "Ctrl++ again removes it");
  await waitFor(async () => Math.abs(widthOf(await view()) - plainWidth) < 1.5, "the text is back to its plain width");
  await selectAll();

  // --- size: list, free entry, Ctrl+Maj+> and < ---------------------------------------------------------------------------------------------
  const sizeInput = cmd("size").querySelector("input");
  const setSize = async value => {
    sizeInput.focus(); sizeInput.value = value;
    sizeInput.dispatchEvent(new InputEvent("input", { bubbles: true }));
    sizeInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  };
  const inkBefore = ink(...TEXT, await view());
  await selectAll();
  await setSize("14");
  await waitFor(() => chars().size.state === "value" && chars().size.value === 28, "a typed size is applied (14 pt)");
  await waitState(() => true, "state after the size");
  assert(sizeInput.value === "14" && document.activeElement === editor.element, "the size box shows the size and the focus returns to the document");
  assert(ink(TEXT[0], 60, 200, 110, await view()) > inkBefore * 1.2, "a larger size is drawn larger");
  await selectAll();
  await setSize("10,5");
  await waitFor(() => chars().size.value === 21, "a size with a decimal comma (10,5 pt = 21 half-points)");
  await waitState(() => true, "state after 10,5");
  assert(sizeInput.value === "10,5", "the box shows 10,5 with the decimal comma");
  const sizeBefore = chars().size.value;
  await setSize("0");
  await waitFor(() => document.querySelector("#editor-status").textContent.includes("Taille invalide"), "an invalid size is explained");
  assert(chars().size.value === sizeBefore, "an invalid size changes nothing");
  key(editor.element, ">", { ctrlKey: true, shiftKey: true });
  await waitFor(() => chars().size.value === 22, "Ctrl+Maj+> goes to the next size of the list");
  key(editor.element, "<", { ctrlKey: true, shiftKey: true });
  await waitFor(() => chars().size.value === 21, "Ctrl+Maj+< goes back");
  await setSize("11");
  await waitFor(() => chars().size.value === 22, "size back to 11 pt");
  // The list: Word's sizes, chosen with the keyboard.
  sizeInput.focus(); sizeInput.value = "";
  sizeInput.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  const options = [...cmd("size").querySelectorAll(".ui-listbox-option")].map(node => node.textContent);
  assert(options.includes("10,5") && options.includes("72") && options.includes("8"), `the size list offers Word's sizes (${options.join(" ")})`);
  [...cmd("size").querySelectorAll(".ui-listbox-option")].find(node => node.textContent === "18").click();
  await waitFor(() => chars().size.value === 36, "a size chosen in the list");
  await setSize("11");
  await waitFor(() => chars().size.value === 22, "size reset");

  // --- font: list, typed name, fonts of the theme, replacement indicated ------------------------------------------------------------------
  const fontInput = cmd("font").querySelector("input");
  const tooltipOf = input => document.getElementById((input.getAttribute("aria-describedby") ?? "").split(" ").pop())?.textContent ?? "";
  const widthCarlito = state().caret.x;
  fontInput.focus();
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  await waitFor(() => cmd("font").querySelectorAll(".ui-listbox-option").length > 3, "the font list opens with preview entries");
  const fonts = [...cmd("font").querySelectorAll(".ui-listbox-option")].map(node => ({ label: node.textContent, family: node.style.fontFamily }));
  assert(fonts.some(f => f.label.startsWith("Carlito")) && fonts.some(f => f.label.includes("Arial")) && fonts.some(f => f.label.includes("Times New Roman")), "the font list names the Word fonts");
  assert(fonts.every(f => f.family !== ""), "each entry of the list is shown in its own font (preview)");
  assert(fonts.some(f => f.label.includes("remplacée par")), "a font the engine replaces is marked in the list");
  fontInput.value = "Arial";
  fontInput.dispatchEvent(new InputEvent("input", { bubbles: true }));
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  fontInput.value = "Arial";
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await waitFor(() => chars().font.state === "value" && chars().font.value.requested === "Arial", "Arial is applied");
  await waitFor(() => chars().font.value.used === "Liberation Sans", "the family of Arial is fetched from the resources and the engine draws with it");
  await waitState(() => true, "state after the font");
  assert(chars().font.value.substituted === true && fontInput.value === "Arial", "the box keeps the requested name");
  assert(tooltipOf(fontInput) === "La police Arial n’est pas disponible : Liberation Sans la remplace." && cmd("font").dataset.substituted === "true", `the replacement is indicated in the tooltip (${tooltipOf(fontInput)})`);
  assert(localStorage.getItem("atelier.recentFonts") === JSON.stringify(["Arial"]), "the font is remembered as a recent font");
  assert(Math.abs(state().caret.x - widthCarlito) > 2, "the text is laid out with the metrics of Liberation Sans, not Carlito");
  await selectAll();
  fontInput.value = "Zork Display";
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await waitFor(() => chars().font.value.requested === "Zork Display", "an unknown font name is accepted");
  await waitState(() => true, "state after the unknown font");
  assert(chars().font.value.used === "Carlito" && tooltipOf(fontInput).includes("Carlito la remplace"), "the replacement of an unknown font is indicated");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => chars().font.value.requested === "Arial", "undo restores the previous font in one step");
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(() => chars().font.value.requested === "Carlito", "undo restores the original font");
  await waitState(() => true, "state after undo");
  assert(chars().font.value.substituted === false && chars().font.value.used === "Carlito" && cmd("font").dataset.substituted === "false", "the document font is drawn as itself and says nothing");
  fontInput.focus();
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  await waitFor(() => [...cmd("font").querySelectorAll(".ui-listbox-option")].some(node => node.textContent.includes("(récente")), "recent fonts head the list");
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  editor.focus();

  // --- colour and highlight -----------------------------------------------------------------------------------------------------------------
  await selectAll();
  const redPixels = async () => count((r, g, b) => r > 200 && g < 60 && b < 60, TEXT[0], TEXT[1], TEXT[2], TEXT[3], await view());
  const yellowPixels = async () => count((r, g, b) => r > 235 && g > 235 && b < 80, TEXT[0], TEXT[1], TEXT[2], TEXT[3], await view());
  assert(await redPixels() === 0 && await yellowPixels() === 0, "no colour on the page before the colour commands");
  await selectAll();
  cmd("color").querySelector(".ui-color-trigger").click();
  cmd("color").querySelector('.ui-color-swatch[data-value="#ff0000"]').click();
  await waitFor(() => chars().color.state === "value" && chars().color.value.r === 255 && chars().color.value.g === 0, "the font colour is applied");
  await waitFor(async () => await redPixels() > 20, "the text is painted in red");
  await selectAll();
  await waitState(() => true, "state after the colour");
  assert(cmd("color").style.getPropertyValue("--char-swatch") === "#ff0000", "the colour button shows the colour of the selection");
  cmd("highlight").querySelector(".ui-color-trigger").click();
  cmd("highlight").querySelector('.ui-color-swatch[data-value="#ffff00"]').click();
  await waitFor(() => chars().highlight.state === "value" && chars().highlight.value.r === 255 && chars().highlight.value.b === 0, "the highlight is applied");
  await waitFor(async () => await yellowPixels() > 200, "a yellow rectangle is drawn under the text");
  await waitFor(async () => await redPixels() > 20, "the red text stays readable above the highlight");
  await selectAll();
  await waitState(() => true, "state after the highlight");
  assert(cmd("highlight").querySelector(".ui-color-custom").hidden, "no free highlight colour (a DOCX keeps the sixteen of Word)");
  cmd("highlight").querySelector(".ui-color-trigger").click();
  cmd("highlight").querySelector(".char-color-none").click();
  await waitFor(() => chars().highlight.state === "unset", "No colour removes the highlight");
  await waitFor(async () => await yellowPixels() === 0, "the highlight is gone from the page");
  await selectAll();
  cmd("color").querySelector(".ui-color-trigger").click();
  cmd("color").querySelector(".char-color-none").click();
  await waitFor(() => chars().color.state === "unset", "Automatic removes the font colour");
  await waitFor(async () => await redPixels() === 0, "the text is black again");

  // --- no selection: the word under the caret, then the typing attributes ------------------------------------------------------------------------
  await caretAt(8); // inside "world"
  key(editor.element, "i", { ctrlKey: true });
  await waitFor(() => chars().italic.state === "value" && chars().italic.value === true && snap().collapsed, "a caret in a word formats the word");
  await selectRange(6, 11);
  assert(chars().italic.value === true, "the word under the caret is italic");
  await selectRange(0, 5);
  assert(chars().italic.value === false, "the other words are not");
  key(editor.element, "End");
  await waitFor(() => state().focus === 11, "end of the text");
  inputText(editor.element, " ");
  await waitFor(() => state().docLen === 12, "a space");
  key(editor.element, "g", { ctrlKey: true });
  await waitFor(() => snap().typing === true && chars().bold.value === true, "outside a word the format becomes a typing attribute");
  assert(pressed("bold") === "true", "the bold button shows the pending attribute");
  inputText(editor.element, "abc");
  await waitFor(() => state().docLen === 15, "typed text");
  await selectRange(12, 15);
  assert(chars().bold.state === "value" && chars().bold.value === true, "the typed text is bold");
  await selectRange(0, 12);
  assert(chars().bold.value === false, "the text before is not");
  await caretAt(15);
  key(editor.element, "z", { ctrlKey: true });
  await waitFor(async () => !(await text()).includes("abc"), "one Ctrl+Z removes the text and its formatting together");

  // --- save as DOCX and reopen -----------------------------------------------------------------------------------------------------------------
  await selectAll();
  inputText(editor.element, "Save me");
  await waitFor(async () => await text() === "Save me", "text to save");
  await selectAll();
  await waitState(s => s.chars.bold.state === "value", "state of the new text");
  fontInput.value = "Times New Roman";
  fontInput.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  await waitFor(() => chars().font.value.used === "Liberation Serif", "Times New Roman is drawn with Liberation Serif");
  await setSize("15");
  await waitFor(() => chars().size.value === 30, "size 15");
  await press("bold", () => chars().bold.value === true, "bold");
  await press("italic", () => chars().italic.value === true, "italic");
  await press("strike", () => chars().strike.value === true, "strike");
  underlineMenu.click();
  [...cmd("underline").querySelectorAll(".ui-menu-item")].find(node => node.textContent === "Ondulé").click();
  await waitFor(() => chars().underline.value === "wave", "wavy underline");
  cmd("color").querySelector(".ui-color-trigger").click();
  cmd("color").querySelector('.ui-color-swatch[data-value="#0070c0"]').click();
  await waitFor(() => chars().color.state === "value" && chars().color.value.b === 192, "blue");
  cmd("highlight").querySelector(".ui-color-trigger").click();
  cmd("highlight").querySelector('.ui-color-swatch[data-value="#00ff00"]').click();
  await waitFor(() => chars().highlight.state === "value" && chars().highlight.value.g === 255, "green highlight");
  await press("superscript", () => chars().verticalAlign.value === "superscript", "superscript");
  await waitState(() => true, "state before saving");
  // The family the engine draws with is not a property of the document: it is loaded again for the new document.
  const stable = value => JSON.stringify({ ...value, font: { ...value.font, value: { requested: value.font.value.requested } } });
  const before = stable(chars());
  const files = bridge.files.length;
  bridge.nextSave = { format: "docx", name: "mise-en-forme.docx" };
  document.querySelector("#save-document").click();
  await waitFor(() => bridge.files.length === files + 1, "the DOCX is written");
  const docx = bridge.files.at(-1).bytes;
  assert(docx[0] === 0x50 && docx[1] === 0x4b, "the saved file is a DOCX package");
  bridge.nextOpen = { name: "mise-en-forme.docx", format: "docx", bytes: docx };
  document.querySelector("#open-docx").click();
  await waitFor(async () => await text() === "Save me" && !state().dirty, "the DOCX is reopened");
  await selectAll();
  await waitState(s => s.chars.bold.state === "value", "state after reopening");
  assert(stable(chars()) === before, `every attribute survives the save and the reopening ${JSON.stringify(chars())}`);
  assert(pressed("bold") === "true" && pressed("italic") === "true" && pressed("strike") === "true" && pressed("superscript") === "true"
    && cmd("underline").querySelector(".ui-split-action").getAttribute("aria-pressed") === "true", "the buttons reflect the reopened formatting");
  await waitFor(() => chars().font.value.used === "Liberation Serif", "the family of the reopened text is loaded again");
  await waitFor(async () => count((r, g, b) => g > 235 && r < 80 && b < 80, 60, 50, 200, 110, await view()) > 30, "the green highlight is drawn after reopening");

  // --- a plain-text document takes no formatting -------------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "brut.txt", format: "txt", bytes: encoder.encode("texte brut") };
  document.querySelector("#open-document").click();
  await waitFor(async () => await text() === "texte brut", "a TXT document is opened");
  await waitFor(() => controller.snapshot().plainText === true && cmd("bold").disabled, "the controls are disabled for a plain-text document");
  assert(cmd("font").getAttribute("aria-disabled") === "true" && cmd("font").querySelector("input").disabled && cmd("size").getAttribute("aria-disabled") === "true"
    && cmd("underline").getAttribute("aria-disabled") === "true" && cmd("color").getAttribute("aria-disabled") === "true" && cmd("highlight").getAttribute("aria-disabled") === "true",
    "every container is aria-disabled and its real controls are disabled");
  key(editor.element, "g", { ctrlKey: true });
  await new Promise(resolve => setTimeout(resolve, 80));
  assert((await text()) === "texte brut" && !state().dirty, "a shortcut changes nothing in a plain-text document");

  assert(pageErrors.length === 0, `no uncaught error or rejection during the whole run: ${pageErrors.join(" | ").slice(0, 400)}`);
  report = { status: "passed", checks, skipped, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, skipped, error: error.stack ?? String(error), userAgent: navigator.userAgent,
    context: { status: document.querySelector("#editor-status")?.textContent, errors: pageErrors, png: cropPng() } };
}
resultNode.textContent = JSON.stringify(report, null, 2);
if (!/[?&]keep/.test(location.search)) { try { app?.dispose?.(); } catch { /* the page is going away */ } }
await fetch("/__format_result", { method: "POST", body: JSON.stringify(report) });
