// SPDX-License-Identifier: AGPL-3.0-or-later
// US-243 : le ruban de mise en forme des paragraphes dans le produit réel (`app.mjs`) avec l'éditeur v2 (`?engine=v2`), pont natif simulé (US-113 : tickets), vrai Worker et vrai WASM.
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
  localStorage.removeItem("atelier.formatMarks");
  localStorage.setItem("atelier.frenchSpacing", "false");
  window.confirm = () => true;
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.engine === "v2", "product initializes with the v2 editor");
  const editor = app.editor; const controller = app.formatController;
  const canvas = document.querySelector("#document-canvas");
  const scrollport = document.querySelector("#document-scroll");
  const status = document.querySelector("#editor-status");
  const ratio = devicePixelRatio || 1;
  const state = () => editor.getState();
  const paragraph = () => controller.snapshot().state?.paragraph;
  const quiet = () => sleep(120);
  const ribbonButton = id => document.querySelector(`[data-ribbon-command-id="${id}"]`);
  app.ribbon.selectTab("home");

  // ---- the Home tab carries the Paragraph group --------------------------------------------------------------------------------
  const group = ribbonButton("para-align-left")?.closest(".ribbon-group");
  assert(group?.dataset.ribbonGroupId === "para-paragraph", "the Home tab holds the group Paragraphe");
  const ids = ["para-align-left", "para-align-center", "para-align-right", "para-align-justify", "para-indent-decrease", "para-indent-increase", "para-spacing", "para-shading", "para-borders", "para-marks"];
  assert(ids.every(id => group.contains(ribbonButton(id))), "the group holds alignments, indents, spacing, shading, borders and marks");
  assert(ids.every(id => ribbonButton(id).getAttribute("aria-label") || ribbonButton(id).textContent.trim()), "every control has an accessible name");
  const shading = ribbonButton("para-shading"); const borders = ribbonButton("para-borders");
  assert(shading.getAttribute("aria-disabled") === "true" && borders.getAttribute("aria-disabled") === "true" && shading.title.includes("pas encore") && borders.title.includes("pas encore"), "shading and borders are disabled and say why");
  shading.click();
  assert(status.textContent === shading.title, "a click on a disabled entry announces its reason");

  // ---- Canvas measurements -------------------------------------------------------------------------------------------------------
  const zoom = () => editor.getZoom();
  const inkLines = () => {
    const view = document.createElement("canvas"); view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.fillStyle = "#ffffff"; ctx.fillRect(0, 0, view.width, view.height); ctx.drawImage(canvas, 0, 0);
    const { data, width, height } = ctx.getImageData(0, 0, view.width, view.height);
    const pageWidth = Math.round(595.276 * zoom() * ratio); const lines = []; let open = null; let blank = 0;
    const close = () => { lines.push({ top: open.top / ratio, bottom: open.bottom / ratio, left: open.left / ratio, right: open.right / ratio }); open = null; blank = 0; };
    for (let y = 0; y < height; y += 1) {
      let left = -1; let right = -1;
      for (let x = 0; x < Math.min(width, pageWidth); x += 1) {
        const i = (y * width + x) * 4;
        if (data[i] < 110 && data[i + 1] < 110 && data[i + 2] < 110) { if (left < 0) left = x; right = x; }
      }
      if (left >= 0) { blank = 0; open ??= { top: y, bottom: y, left, right }; open.bottom = y; open.left = Math.min(open.left, left); open.right = Math.max(open.right, right); }
      else if (open && ++blank >= 3) close();
    }
    if (open) close();
    return lines;
  };
  const MARGIN = 70.866; const PAGE = 595.276;

  // ---- three paragraphs ----------------------------------------------------------------------------------------------------------
  const typeLine = text => inputText(editor.element, text);
  const enter = () => editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertParagraph", bubbles: true, cancelable: true }));
  typeLine("Premier"); enter(); typeLine("Deuxième paragraphe"); enter(); typeLine("Troisième");
  await waitFor(() => state().docLen === 39, "three paragraphs typed");
  const toTop = async () => { key(editor.element, "Home", { ctrlKey: true }); await waitFor(() => state().focus === 0, "caret at the start"); await quiet(); };
  const selectAll = async () => { key(editor.element, "a", { ctrlKey: true }); await waitFor(() => state().anchor !== state().focus, "everything selected"); await quiet(); };
  await selectAll();
  assert(paragraph()?.alignment.state === "value" && paragraph().alignment.value === "left", "the three paragraphs start aligned left");
  assert(ribbonButton("para-align-left").getAttribute("aria-pressed") === "true" && ribbonButton("para-align-center").getAttribute("aria-pressed") === "false", "the button of the current alignment is pressed");
  await toTop();
  let lines = inkLines();
  assert(lines.length === 3 && lines.every(line => Math.abs(line.left - MARGIN * zoom()) < 3), `three text lines start at the left margin ${JSON.stringify(lines)}`);
  const pitch = () => { const l = inkLines(); return [l[1].top - l[0].top, l[2].top - l[1].top]; };
  const pitch0 = pitch();

  // ---- alignments: every touched paragraph, visible on the Canvas, one undo -------------------------------------------------------
  const revision0 = state().revision;
  await selectAll();
  ribbonButton("para-align-center").click();
  await waitFor(() => paragraph()?.alignment.value === "center" && state().revision === revision0 + 1n, "center applied to the selection");
  assert(document.activeElement === editor.element, "the focus returns to the document after a ribbon command");
  assert(ribbonButton("para-align-center").getAttribute("aria-pressed") === "true" && ribbonButton("para-align-left").getAttribute("aria-pressed") === "false", "the pressed button follows the selection");
  await toTop(); await quiet();
  lines = inkLines();
  assert(lines.length === 3 && lines.every(line => Math.abs((line.left + line.right) / 2 - PAGE / 2 * zoom()) < 4), `all three paragraphs are drawn centered ${JSON.stringify(lines)}`);
  document.querySelector("#undo").click();
  await waitFor(() => paragraph()?.alignment.value === "left" && state().anchor !== state().focus, "one undo step restores the three paragraphs and the selection");
  await quiet(); lines = inkLines();
  assert(lines.every(line => Math.abs(line.left - MARGIN * zoom()) < 3), "the Canvas shows the paragraphs left again");
  // Keyboard: Ctrl+R on the first paragraph only, then a mixed selection.
  await toTop();
  key(editor.element, "r", { ctrlKey: true });
  await waitFor(() => paragraph()?.alignment.value === "right", "Ctrl+R aligns the paragraph of the caret");
  await quiet(); lines = inkLines();
  assert(Math.abs(lines[0].right - (PAGE - MARGIN) * zoom()) < 4 && Math.abs(lines[1].left - MARGIN * zoom()) < 3, "only the first paragraph moved to the right margin");
  await selectAll();
  assert(paragraph().alignment.state === "mixed" && ["left", "center", "right", "justify"].every(name => ribbonButton(`para-align-${name}`).getAttribute("aria-pressed") === "false"), "a mixed selection shows no pressed alignment");
  ribbonButton("para-align-justify").click();
  await waitFor(() => paragraph()?.alignment.value === "justify", "justify unifies a mixed selection");
  ribbonButton("para-align-justify").click();
  await waitFor(() => paragraph()?.alignment.value === "left", "a second click on the active alignment goes back to left");
  key(editor.element, "e", { ctrlKey: true });
  await waitFor(() => paragraph()?.alignment.value === "center", "Ctrl+E centers");
  key(editor.element, "l", { ctrlKey: true });
  await waitFor(() => paragraph()?.alignment.value === "left", "Ctrl+L aligns left");

  // ---- indents ---------------------------------------------------------------------------------------------------------------------
  key(editor.element, "m", { ctrlKey: true });
  await waitFor(() => paragraph()?.indentLeft.value === 720, "Ctrl+M increases the indent by 720 twips");
  ribbonButton("para-indent-increase").click();
  await waitFor(() => paragraph()?.indentLeft.value === 1440, "the Increase indent button steps again");
  await toTop(); await quiet(); lines = inkLines();
  assert(lines.every(line => Math.abs(line.left - (MARGIN + 72) * zoom()) < 3), `the three paragraphs are indented by 72 pt on the Canvas ${JSON.stringify(lines)}`);
  await selectAll();
  key(editor.element, "M", { ctrlKey: true, shiftKey: true });
  await waitFor(() => paragraph()?.indentLeft.value === 720, "Ctrl+Shift+M decreases the indent");
  ribbonButton("para-indent-decrease").click();
  await waitFor(() => paragraph()?.indentLeft.value === 0, "the Decrease indent button reaches zero");

  // ---- line spacing and spacing ------------------------------------------------------------------------------------------------------
  const lineKey = (code, modifiers = {}) => editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "x", code, bubbles: true, cancelable: true, ctrlKey: true, ...modifiers }));
  lineKey("Digit2");
  await waitFor(() => paragraph()?.lineSpacing.value?.value === 480, "Ctrl+2 sets the double line spacing");
  await toTop(); await quiet();
  const doubled = pitch();
  assert(doubled[0] > pitch0[0] + 7 && doubled[1] > pitch0[1] + 7, `the lines are farther apart on the Canvas (${pitch0} -> ${doubled})`);
  await selectAll();
  lineKey("Digit1");
  await waitFor(() => paragraph()?.lineSpacing.value?.value === 240, "Ctrl+1 sets the single line spacing");
  await toTop(); await quiet();
  const single = pitch();
  assert(Math.abs(single[0] - pitch0[0]) < 1.5 && Math.abs(single[1] - pitch0[1]) < 1.5, "single spacing draws the original pitch again");
  await selectAll();
  lineKey("Digit5");
  await waitFor(() => paragraph()?.lineSpacing.value?.value === 360, "Ctrl+5 sets 1.5 lines");
  // Menu: the checked value, a value, the space entries.
  const spacingButton = ribbonButton("para-spacing");
  spacingButton.click();
  const menu = document.querySelector(".para-spacing-menu");
  await waitFor(() => !menu.hidden && menu.contains(document.activeElement), "the spacing menu opens with the focus inside");
  const items = [...menu.querySelectorAll(".ui-menu-item")];
  assert(items.length === 9 && items[2].getAttribute("aria-checked") === "true" && items[2].textContent.startsWith("✓"), "the menu checks the current value (1,5)");
  assert(/Supprimer l’espace après/.test(items[8].textContent) && /Ajouter un espace avant/.test(items[7].textContent), "the space entries say add or remove from the state");
  items[4].click();
  await waitFor(() => paragraph()?.lineSpacing.value?.value === 600 && menu.hidden, "the menu sets 2,5 lines and closes");
  await waitFor(() => document.activeElement === editor.element, "the focus goes back to the document after the menu");
  spacingButton.click(); await waitFor(() => !menu.hidden, "menu reopened");
  [...menu.querySelectorAll(".ui-menu-item")][8].click();
  await waitFor(() => paragraph()?.spaceAfter.value === 0, "Remove space after clears it");
  spacingButton.click(); await waitFor(() => !menu.hidden, "menu reopened again");
  assert(/Ajouter un espace après/.test([...menu.querySelectorAll(".ui-menu-item")][8].textContent), "the entry now offers to add");
  key(menu, "Escape"); await waitFor(() => menu.hidden, "Escape closes the menu");
  await waitFor(() => document.activeElement === spacingButton, "Escape restores the opener");
  editor.focus();
  await toTop(); await quiet();
  const beforeSpace = inkLines().map(line => line.top);
  await selectAll();
  lineKey("Digit0");
  await waitFor(() => paragraph()?.spaceBefore.value === 240, "Ctrl+0 adds 12 pt before");
  await toTop(); await quiet();
  const withSpace = inkLines().map(line => line.top);
  assert(withSpace[2] - withSpace[1] > beforeSpace[2] - beforeSpace[1] + 1, "adding space before widens the gap on the Canvas");
  await selectAll();
  lineKey("Digit0");
  await waitFor(() => paragraph()?.spaceBefore.value === 0, "Ctrl+0 again removes it");

  // ---- the "Line spacing options" dialog -----------------------------------------------------------------------------------------
  spacingButton.click(); await waitFor(() => !menu.hidden, "menu for the dialog");
  [...menu.querySelectorAll(".ui-menu-item")][6].click();
  const dialog = document.querySelector(".para-line-dialog-root");
  await waitFor(() => dialog.open, "the options dialog opens");
  const typeSelect = dialog.querySelector(".para-line-type"); const valueInput = dialog.querySelector(".para-line-value");
  assert(typeSelect.value === "multiple" && dialog.querySelector(".ui-dialog-title").textContent === "Options d’interligne", "the dialog starts on the current spacing");
  typeSelect.value = "multiple"; typeSelect.dispatchEvent(new Event("change"));
  valueInput.value = "99";
  const revisionBeforeInvalid = state().revision;
  dialog.querySelector(".para-line-apply").click();
  await sleep(100);
  assert(!dialog.querySelector(".para-line-invalid").hidden && dialog.open && state().revision === revisionBeforeInvalid, "an out-of-range value is refused in the dialog, nothing is sent");
  typeSelect.value = "exact"; typeSelect.dispatchEvent(new Event("change"));
  valueInput.value = "24";
  dialog.querySelector(".para-line-apply").click();
  await waitFor(() => paragraph()?.lineSpacing.value?.rule === "exact" && paragraph().lineSpacing.value.value === 480 && !dialog.open, "Exactly 24 pt is applied (480 twips) and the dialog closes");
  await waitFor(() => document.activeElement === editor.element, "the focus returns to the document after the dialog");
  await toTop(); await quiet();
  const exact = pitch();
  assert(Math.abs(exact[0] - 24 * zoom()) < 1.5 && Math.abs(exact[1] - 24 * zoom()) < 1.5, `the Canvas shows an exact 24 pt line pitch (${exact})`);
  await selectAll();
  lineKey("Digit1");
  await waitFor(() => paragraph()?.lineSpacing.value?.rule === "auto" && paragraph().lineSpacing.value.value === 240, "single spacing again before the marks");

  // ---- formatting marks (T243-C) --------------------------------------------------------------------------------------------------
  const { createParaFormat } = await import("/format-para.mjs");
  const { createI18n, parseCatalog } = await import("/i18n.mjs");
  const catalog = new Map([...parseCatalog(await (await fetch("/i18n/fr-FR.txt")).text()).entries].map(([k, entry]) => [k, entry.value]));
  const second = createParaFormat({ document, i18n: createI18n({ locale: "fr-FR", catalog }), controller, announce: () => {} });
  const probe = document.createElement("div"); document.body.append(probe);
  const probeButton = second.groups({ command: (id, label, tip, element) => ({ id, element }), future: () => ({}) })[0].commands.find(c => c.id === "para-marks").element;
  probe.append(probeButton);
  second.attachEditor(editor);
  assert(probeButton.getAttribute("aria-pressed") === "false" && second.marks.active === false && !document.querySelector(".para-marks-overlay"), "marks are hidden at first");
  await toTop();
  probeButton.click();
  await waitFor(() => second.marks.active && document.querySelector(".para-marks-overlay") && second.marks.drawn > 0, "the marks overlay is drawn");
  assert(probeButton.getAttribute("aria-pressed") === "true" && localStorage.getItem("atelier.formatMarks") === "true", "the toggle is pressed and remembered");
  const overlay = document.querySelector(".para-marks-overlay");
  assert(overlay.getAttribute("aria-hidden") === "true" && overlay.previousElementSibling === canvas && getComputedStyle(overlay).pointerEvents === "none", "the overlay sits over the Canvas and never takes the mouse");
  const reply = await controller.client.formatMarks({ first: 0, count: 1 });
  const inkAround = (mark, z) => {
    const ctx = overlay.getContext("2d"); const x = mark.x * z - scrollport.scrollLeft; const y = mark.y * z - scrollport.scrollTop;
    const { data } = ctx.getImageData(Math.max(0, Math.floor((x - 2) * ratio)), Math.max(0, Math.floor((y - 12 * z) * ratio)), Math.ceil((mark.width * z + 8) * ratio), Math.ceil(16 * z * ratio));
    let count = 0; for (let i = 3; i < data.length; i += 4) if (data[i] > 40) count += 1;
    return count;
  };
  const ends = reply.marks.filter(m => m.kind === "paragraphEnd"); const spaces = reply.marks.filter(m => m.kind === "space");
  assert(ends.length === 3 && spaces.length === 1, `the engine marks three paragraph ends and one space ${JSON.stringify(reply.marks.map(m => m.kind))}`);
  assert(ends.every(mark => inkAround(mark, zoom()) > 3) && inkAround(spaces[0], zoom()) > 0, "each mark is drawn at the position the engine gave, over the text");
  assert(inkAround({ x: 400, y: ends[0].y, width: 10 }, zoom()) === 0, "nothing is drawn where there is no mark");
  const zoomBefore = zoom();
  editor.setZoom(zoomBefore * 1.5);
  await waitFor(() => zoom() > zoomBefore, "zoom changed");
  await waitFor(() => ends.every(mark => inkAround(mark, zoom()) > 3), "the marks follow the zoom");
  editor.setZoom(zoomBefore);
  const revisionMarks = state().revision;
  key(editor.element, "ArrowDown");
  await quiet();
  assert(state().revision === revisionMarks && second.marks.drawn > 0, "moving the caret does not change the revision or the marks");
  await toTop(); typeLine("Mots "); await waitFor(() => state().revision > revisionMarks, "typed");
  await waitFor(async () => { const r = await controller.client.formatMarks({ first: 0, count: 1 }); return r.marks.filter(m => m.kind === "space").length === 2 && r.marks.every(m => inkAround(m, zoom()) > 0); }, "after typing the overlay follows the new text");
  probeButton.click();
  await waitFor(() => !document.querySelector(".para-marks-overlay") && probeButton.getAttribute("aria-pressed") === "false", "turning the marks off removes the overlay");
  assert(localStorage.getItem("atelier.formatMarks") === "false", "the preference is stored off");
  // The product's own toggle shortcut (Ctrl+Shift+8) and button.
  const productMarks = ribbonButton("para-marks");
  editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "*", code: "Digit8", ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true }));
  await waitFor(() => productMarks.getAttribute("aria-pressed") === "true", "Ctrl+Shift+8 turns the marks on");
  productMarks.click();
  await waitFor(() => productMarks.getAttribute("aria-pressed") === "false", "the ribbon button turns them off");
  second.detachEditor(); second.dispose(); probe.remove();

  // ---- states: read-only and plain text ----------------------------------------------------------------------------------------------
  bridge.nextOpen = { name: "essai.txt", format: "txt", bytes: encoder.encode("texte brut") };
  document.querySelector("#open-document").click();
  await waitFor(() => status.textContent === "Document ouvert." && controller.snapshot().plainText, "a plain text document is opened");
  assert(ribbonButton("para-align-center").disabled && ribbonButton("para-spacing").disabled && !ribbonButton("para-marks").disabled, "plain text: formatting is disabled, the marks stay available");
  bridge.nextOpen = { name: "revisions.docx", format: "docx", bytes: await fixture("revisions.docx") };
  document.querySelector("#open-docx").click();
  await waitFor(() => controller.snapshot().readOnly, "a document with revisions opens read-only");
  assert(ribbonButton("para-align-center").disabled && ribbonButton("para-indent-increase").disabled, "read-only: formatting is disabled");
  assert(pageErrors.length === 0, `no uncaught error or rejection during the run: ${pageErrors.join(" | ").slice(0, 300)}`);
  report = { status: "passed", checks, skipped, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, skipped, error: error.stack ?? String(error), userAgent: navigator.userAgent,
    context: { status: document.querySelector("#editor-status")?.textContent, errors: pageErrors } };
}
resultNode.textContent = JSON.stringify(report, null, 2);
try { app?.dispose?.(); } catch { /* the page is going away */ }
await fetch("/__rich_result", { method: "POST", body: JSON.stringify(report) });
