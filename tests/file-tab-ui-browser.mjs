// SPDX-License-Identifier: AGPL-3.0-or-later
// US-320 : l'onglet Fichier du produit réel (`app.mjs`, éditeur riche v2, vrai Worker et vrai WASM) : nouveau et
// modèles embarqués, récents du natif (liste, épingler, retirer, introuvable), commandes sans moteur désactivées avec
// leur raison. Le pont natif est simulé ; la logique native (persistance de recent.ini, ordre, introuvable) est
// vérifiée par les tests Rust de `bridge_ops.rs` et `recent.rs`.
const checks = [];
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
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8");
const b64 = bytes => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s); };
const fromB64 = text => Uint8Array.from(atob(text), c => c.charCodeAt(0));

/** Pont natif simulé : dialogues, tickets, et la liste des récents telle que le natif la tient (épinglés d'abord, puis récents). */
function createFakeBridge() {
  const listeners = new Set();
  const bridge = {
    calls: [], files: [], reads: new Map(), paths: new Map(), readable: new Map(), writes: new Map(), serial: 1, nextOpen: { cancelled: true }, nextSave: {}, lastSaveSpec: null,
    recents: [], recentSerial: 1, clock: 1_770_000_000,
    addRecent(path, { pinned = false, present = true, bytes = new Uint8Array(), format = "txt", used } = {}) {
      const entry = { id: bridge.recentSerial++, path, pinned, present, bytes, format, used: used ?? bridge.clock++ };
      bridge.recents.push(entry); return entry;
    },
    touch(path) { const entry = bridge.recents.find(item => item.path === path); if (entry) entry.used = bridge.clock++; else bridge.addRecent(path); },
    ordered: () => [...bridge.recents].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.used - a.used || b.id - a.id),
    webview: {
      postMessage(message) {
        bridge.calls.push(message);
        const match = /^us120-file\|1\|([1-9][0-9]*)\|(\w+)\|(.*)$/.exec(message);
        if (!match || match[2] === "dirty") return;
        const [, id, operation, payload] = match;
        const reply = (status, body = "") => queueMicrotask(() => { for (const listener of listeners) listener({ data: `us120-file|1|${id}|${status}|${body}` }); });
        const grant = (bytes, format, name, path) => { const ticket = `r${bridge.serial++}`; bridge.reads.set(ticket, bytes); bridge.paths.set(ticket, path); reply("ok", `${ticket};${format};${bytes.length};${b64(encoder.encode(name))}`); };
        if (operation === "pickOpen") {
          const next = bridge.nextOpen;
          if (next.cancelled) return reply("cancelled");
          return grant(next.bytes, next.format, next.name, next.path);
        }
        if (operation === "readTicket") {
          const bytes = bridge.reads.get(payload); bridge.reads.delete(payload);
          if (bytes) bridge.readable.set(payload, bridge.paths.get(payload));
          return bytes ? reply("ok", `b64;${b64(bytes)}`) : reply("error", "files.error.unknown_ticket");
        }
        if (operation === "pickSave") {
          bridge.lastSaveSpec = decoder.decode(fromB64(payload));
          if (bridge.nextSave.cancelled) return reply("cancelled");
          const name = /^name:(.*)$/m.exec(bridge.lastSaveSpec)?.[1] ?? "Document.txt";
          const ticket = `w${bridge.serial++}`; bridge.writes.set(ticket, { name, chunks: [] });
          return reply("ok", `${ticket};${name.split(".").pop()};${b64(encoder.encode(name))}`);
        }
        if (operation === "writeTicket") {
          const [ticket, , last, data] = payload.split(";");
          const write = bridge.writes.get(ticket);
          if (!write) return reply("error", "files.error.unknown_ticket");
          write.chunks.push(fromB64(data));
          if (last === "1") {
            const bytes = new Uint8Array(write.chunks.reduce((n, c) => n + c.length, 0)); let at = 0;
            for (const c of write.chunks) { bytes.set(c, at); at += c.length; }
            bridge.writes.delete(ticket); bridge.files.push({ name: write.name, bytes });
          }
          return reply("ok");
        }
        if (operation === "recentList") {
          const rows = bridge.ordered().map(e => `${e.id}\t${Number(e.pinned)}\t${e.used}\t${Number(e.present)}\t${e.path}`).join("\n") + "\n";
          return reply("ok", `b64;${b64(encoder.encode(bridge.recents.length ? rows : ""))}`);
        }
        if (operation === "recentOpen") {
          const entry = bridge.recents.find(item => item.id === Number(payload));
          if (!entry) return reply("error", "files.error.unknown_recent");
          if (!entry.present) return reply("error", "files.error.not_found");
          return grant(entry.bytes, entry.format, entry.path.split("\\").pop(), entry.path);
        }
        if (operation === "recentNote") {
          const path = bridge.readable.get(payload); bridge.readable.delete(payload);
          if (!path) return reply("error", "files.error.unknown_ticket");
          bridge.touch(path); return reply("ok");
        }
        if (operation === "recentPin") {
          const [number, flag] = payload.split(";"); const entry = bridge.recents.find(item => item.id === Number(number));
          if (!entry) return reply("error", "files.error.unknown_recent");
          entry.pinned = flag === "1"; return reply("ok");
        }
        if (operation === "recentRemove") {
          const before = bridge.recents.length; bridge.recents = bridge.recents.filter(item => item.id !== Number(payload));
          return bridge.recents.length === before ? reply("error", "files.error.unknown_recent") : reply("ok");
        }
        if (operation === "launch") return reply("ok");
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
  // Trois documents récents déjà connus du natif : un épinglé, un introuvable.
  const docsDir = "C:\\Users\\Moi\\Documents\\";
  const mail = bridge.addRecent(`${docsDir}Courrier é.txt`, { bytes: encoder.encode("Bonjour\nle monde"), format: "txt", used: 1_760_000_100 });
  const gone = bridge.addRecent(`${docsDir}Disparu.docx`, { present: false, format: "docx", used: 1_760_000_200 });
  const pinned = bridge.addRecent(`D:\\Archives\\Épinglé.txt`, { pinned: true, bytes: encoder.encode("archive"), format: "txt", used: 1_700_000_000 });
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: bridge.webview } });
  let confirms = 0;
  let confirmAnswer = true;
  window.confirm = () => { confirms += 1; return confirmAnswer; };
  localStorage.setItem("atelier.frenchSpacing", "false");
  await productPage();
  const { editorReady } = await import("/app.mjs?first");
  app = await editorReady;
  assert(app.status === "ready" && app.fileTab, "the product starts with the file tab controller");
  const editor = app.editor;
  const status = document.querySelector("#editor-status");
  const text = () => editor.readText();
  const state = () => app.getState();

  // --- structure de l'onglet ------------------------------------------------------------------------------------------
  const fileTabButton = document.querySelector('[data-ribbon-tab-id="file"]');
  assert(fileTabButton && fileTabButton.getAttribute("aria-selected") === "true", "the File tab is the initial tab");
  const panel = document.querySelector('[data-ribbon-panel-id="file"]');
  const groups = [...panel.querySelectorAll(".ribbon-group")].map(group => group.dataset.ribbonGroupId);
  assert(JSON.stringify(groups) === JSON.stringify(["new", "files", "document", "recent"]), `file tab groups: ${groups}`);
  const commands = [...panel.querySelectorAll("[data-ribbon-command-id]")].map(node => node.dataset.ribbonCommandId);
  for (const id of ["new", "template-letter", "template-cv", "template-report", "template-minutes", "open", "open-docx", "save", "save-as",
    "export-pdf", "print", "properties", "options", "about", "recent"]) assert(commands.includes(id), `file tab command ${id}`);
  const label = id => panel.querySelector(`[data-ribbon-command-id="${id}"]`).textContent.trim();
  assert(label("template-letter") === "Lettre" && label("template-cv") === "CV" && label("template-report") === "Rapport" && label("template-minutes") === "Compte rendu",
    "the four templates are Lettre, CV, Rapport and Compte rendu");
  const keyTips = [...panel.querySelectorAll("[data-ribbon-command-id]")].length;
  assert(keyTips >= 15, "no key tip collision: the ribbon accepted the file tab");

  // --- commandes sans moteur : désactivées avec leur raison, sans rien faire ----------------------------------------
  const callsBefore = bridge.calls.length;
  for (const [id, expected] of [["save", /Enregistrer sous/], ["export-pdf", /PDF.*pas encore disponible/], ["print", /impression.*pas encore disponible/], ["options", /options.*pas encore disponibles/]]) {
    const button = panel.querySelector(`[data-ribbon-command-id="${id}"]`);
    assert(button.getAttribute("aria-disabled") === "true" && !button.disabled, `${id} is announced as unavailable yet focusable`);
    const reason = document.getElementById(button.getAttribute("aria-describedby"));
    assert(reason && expected.test(reason.textContent) && button.title === reason.textContent, `${id} carries its reason as description and tooltip`);
    button.click();
    assert(expected.test(status.textContent) && !status.classList.contains("error"), `${id} says why in the status region instead of pretending`);
  }
  assert(bridge.calls.slice(callsBefore).every(call => call.includes("|recentList|") || call.includes("|dirty|")), "unavailable commands never reach the native side");

  // --- récents : liste ---------------------------------------------------------------------------------------------------
  const items = () => [...document.querySelectorAll(".file-recent-item")];
  await waitFor(() => items().length === 3, "the recents list is shown");
  assert(items().map(item => item.querySelector(".file-recent-path").textContent).join("|")
    === [`D:\\Archives\\Épinglé.txt`, `${docsDir}Disparu.docx`, `${docsDir}Courrier é.txt`].join("|"), "recents show their full path, pinned first then most recent first");
  assert(items()[0].dataset.pinned === "true" && items()[0].querySelector(".file-recent-pin").getAttribute("aria-pressed") === "true", "the pinned entry is marked pinned");
  assert(items()[0].querySelector(".file-recent-name").textContent === "Épinglé.txt", "the name is the file name");
  assert(/2023/.test(items()[0].querySelector(".file-recent-date").textContent) && /2025/.test(items()[2].querySelector(".file-recent-date").textContent), "each recent shows its date");
  assert(items()[1].dataset.missing === "true" && items()[1].querySelector(".file-recent-missing").textContent === "Introuvable"
    && /introuvable/.test(items()[1].querySelector(".file-recent-open").getAttribute("aria-label")), "a missing file is flagged in text, not by colour alone");
  assert(items()[2].dataset.missing === "false" && !items()[2].querySelector(".file-recent-missing"), "a present file is not flagged");

  const recentBox = document.querySelector("#file-recent").getBoundingClientRect();
  const ribbonBox = document.querySelector(".ribbon-body").getBoundingClientRect();
  assert(recentBox.width > 200 && recentBox.left >= ribbonBox.left - 1 && recentBox.right <= ribbonBox.right + 1, "the recents list fits inside the ribbon");
  assert(document.querySelector("#file-recent").scrollWidth <= document.querySelector("#file-recent").clientWidth + 1, "the recents list does not overflow horizontally");
  assert(items().every(item => item.querySelector(".file-recent-open").getBoundingClientRect().width > 100), "each recent keeps a readable open target");

  // --- récents : ouvrir (présent) -------------------------------------------------------------------------------------------
  items()[2].querySelector(".file-recent-open").click();
  await waitFor(async () => await text() === "Bonjour\nle monde", "the recent opens in the editor");
  assert(status.textContent === "Document ouvert." && !state().dirty, "the recent opens as a clean document");
  assert(!bridge.calls.some(call => /\|recentOpen\|.*[\\:]/.test(call)), "the recent is designated by its number, never by a path");
  await waitFor(() => items()[0].querySelector(".file-recent-name").textContent === "Épinglé.txt" && items()[1].querySelector(".file-recent-name").textContent === "Courrier é.txt", "the list is reread after the open (opened file moves up)");

  // --- récents : introuvable ------------------------------------------------------------------------------------------------
  confirmAnswer = true;
  const typed = new InputEvent("beforeinput", { inputType: "insertText", data: "X", bubbles: true, cancelable: true });
  editor.element.dispatchEvent(typed);
  await waitFor(() => state().dirty, "the document is dirty");
  const confirmsBefore = confirms;
  const textBefore = await text();
  const missingItem = items().find(item => item.dataset.missing === "true");
  missingItem.querySelector(".file-recent-open").click();
  await waitFor(() => /introuvable/.test(status.textContent), "a missing recent is reported");
  assert(status.classList.contains("error") && status.textContent.includes("Disparu.docx"), "the missing file is named in an error message");
  assert(confirms === confirmsBefore && await text() === textBefore && state().dirty, "a missing recent never asks to discard nor changes the document");
  assert(items().some(item => item.dataset.missing === "true"), "a missing recent stays in the list until removed");

  // --- récents : épingler et désépingler ---------------------------------------------------------------------------------------
  const courrier = () => items().find(item => item.querySelector(".file-recent-name").textContent === "Courrier é.txt");
  courrier().querySelector(".file-recent-pin").click();
  await waitFor(() => courrier().dataset.pinned === "true", "pinning marks the entry");
  assert(bridge.calls.some(call => call.endsWith(`|recentPin|${mail.id};1`)), "pin sends the entry number and the flag");
  assert(/épinglé en haut/.test(status.textContent), "pinning is announced");
  assert(items().findIndex(item => item === courrier()) < items().findIndex(item => item.dataset.missing === "true"), "a pinned entry sorts before unpinned ones");
  courrier().querySelector(".file-recent-pin").click();
  await waitFor(() => courrier().dataset.pinned === "false", "unpinning clears the mark");
  assert(bridge.calls.some(call => call.endsWith(`|recentPin|${mail.id};0`)) && /n’est plus épinglé/.test(status.textContent), "unpin sends flag 0 and is announced");
  assert(document.activeElement === courrier().querySelector(".file-recent-pin"), "the focus stays on the pin button after the list is redrawn");

  // --- récents : retirer ---------------------------------------------------------------------------------------------------------
  const goneItem = () => items().find(item => item.dataset.missing === "true");
  const position = items().indexOf(goneItem());
  goneItem().querySelector(".file-recent-remove").click();
  await waitFor(() => !goneItem(), "removing drops the entry");
  assert(bridge.calls.some(call => call.endsWith(`|recentRemove|${gone.id}`)) && /retiré de la liste/.test(status.textContent), "remove sends the entry number and is announced");
  assert(document.activeElement === items()[Math.min(position, items().length - 1)].querySelector(".file-recent-open"), "the focus moves to the neighbouring entry after a removal");
  assert(items().length === 2 && bridge.recents.some(item => item.id === pinned.id), "removing one entry keeps the others");

  // --- l'ouverture et l'enregistrement par dialogue alimentent la liste ------------------------------------------------------------
  const openButton = document.querySelector("#open-document");
  bridge.nextOpen = { path: "E:\\Nouveau.txt", name: "Nouveau.txt", format: "txt", bytes: encoder.encode("neuf") };
  openButton.click();
  await waitFor(async () => await text() === "neuf", "open through the dialog");
  await waitFor(() => items().some(item => item.querySelector(".file-recent-path").textContent === "E:\\Nouveau.txt"), "a document opened through the dialog appears in the recents");

  // Un fichier lu mais refusé par l'éditeur n'entre pas dans les récents (pas de recentNote).
  bridge.nextOpen = { path: "E:/Refusé.docx", name: "Refusé.docx", format: "docx", bytes: encoder.encode("pas un docx") };
  openButton.click();
  await waitFor(() => /pas pu être ouvert/.test(status.textContent), "a corrupt DOCX is refused");
  assert(!bridge.recents.some(item => item.path === "E:/Refusé.docx"), "a refused document is not added to the recents");

  // --- modèles -----------------------------------------------------------------------------------------------------------------------
  const MARKERS = { letter: "Madame, Monsieur,", cv: "Expérience professionnelle", report: "Résumé", minutes: "Ordre du jour" };
  const templateBytes = {};
  for (const [id, file] of [["letter", "lettre"], ["cv", "cv"], ["report", "rapport"], ["minutes", "compte-rendu"]]) {
    const response = await fetch(`/templates/${file}.docx`);
    templateBytes[id] = new Uint8Array(await response.arrayBuffer());
    assert(response.ok && templateBytes[id][0] === 0x50 && templateBytes[id][1] === 0x4b, `template ${id} is served as a DOCX (zip)`);
  }
  for (const id of ["letter", "cv", "report", "minutes"]) {
    document.querySelector(`#file-template-${id}`).click();
    await waitFor(async () => (await text()).includes(MARKERS[id]), `template ${id} opens`);
    assert(/^Modèle « .+ » ouvert comme document sans nom/.test(status.textContent) && !state().dirty && !state().readOnly,
      `template ${id}: editable, clean, announced as an unnamed document`);
    document.querySelector("#save-document").click();
    await waitFor(() => bridge.lastSaveSpec !== null && bridge.files.length > 0, `save as after template ${id}`);
    const specLines = bridge.lastSaveSpec.split("\n");
    assert(specLines.at(-1) === "name:Document.docx" && specLines[1].startsWith("filter:docx="), `template ${id}: Save As asks for a location, suggests Document.docx first (not the template name)`);
    const saved = bridge.files.pop();
    assert(saved.name === "Document.docx" && saved.bytes[0] === 0x50, `template ${id}: the saved copy is a new DOCX`);
    bridge.lastSaveSpec = null;
    await waitFor(() => !editor.busy && !state().dirty, `template ${id}: saved and clean`);
    assert(!bridge.calls.some(call => /lettre|rapport|compte-rendu|templates/.test(call) && call.includes("|writeTicket|")), "no write targets a template");
  }
  const again = new Uint8Array(await (await fetch("/templates/lettre.docx")).arrayBuffer());
  assert(again.length === templateBytes.letter.length && again.every((value, index) => value === templateBytes.letter[index]), "the embedded template is unchanged after saving");

  // --- un document modifié demande confirmation avant d'être remplacé par un modèle -----------------------------------------------
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: "Z", bubbles: true, cancelable: true }));
  await waitFor(() => state().dirty, "dirty before the template");
  const withText = await text();
  confirmAnswer = false;
  const asked = confirms;
  document.querySelector("#file-template-cv").click();
  await waitFor(() => confirms === asked + 1 && status.textContent === "Opération annulée.", "a declined confirmation cancels the template");
  assert(await text() === withText && state().dirty, "declining keeps the edited document");
  confirmAnswer = true;

  // --- blocage pendant une opération -------------------------------------------------------------------------------------------------------
  app.fileTab.setBlocked(true);
  assert(document.querySelector("#file-template-letter").disabled && [...document.querySelectorAll(".file-recent-item button")].every(button => button.disabled), "templates and recents are disabled while the document is busy");
  app.fileTab.setBlocked(false);

  // --- sans hôte natif -----------------------------------------------------------------------------------------------------------------------
  const { createFileTab } = await import("/file-tab.mjs");
  const { createI18n, parseCatalog } = await import("/i18n.mjs");
  const catalogue = parseCatalog(await (await fetch("/i18n/fr-FR.txt")).text());
  const i18n = createI18n({ locale: "fr-FR", catalog: new Map([...catalogue.entries].map(([key, entry]) => [key, entry.value])) });
  const bare = createFileTab({ document, i18n });
  await bare.attach({ host: { open: async () => ({ kind: "error", key: "HOST_UNAVAILABLE" }) }, openDocument: async () => ({ kind: "cancelled" }) });
  assert(/ne sont disponibles que dans l’application/.test(bare.element.textContent), "without the native host the recents say they are unavailable instead of staying empty");
  const failing = createFileTab({ document, i18n });
  await failing.attach({ host: { recentList: async () => ({ kind: "error", key: "HOST_TIMEOUT" }) }, openDocument: async () => ({ kind: "cancelled" }) });
  assert(/indisponible/.test(failing.element.textContent) && failing.element.querySelector(".file-recent-note.error"), "a failing list is reported as an error");
  const empty = createFileTab({ document, i18n });
  await empty.attach({ host: { recentList: async () => ({ kind: "ok", entries: [] }) }, openDocument: async () => ({ kind: "cancelled" }) });
  assert(/Aucun document récent/.test(empty.element.textContent), "an empty list says so");
  for (const view of [bare, failing, empty]) view.dispose();

  assert(pageErrors.length === 0, `no page error: ${pageErrors.join(" | ")}`);
  report = { status: "passed", checks, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, error: String(error?.stack ?? error), pageErrors, userAgent: navigator.userAgent };
} finally {
  try { app?.dispose?.(); } catch { /* arrêt du produit */ }
}
resultNode.textContent = JSON.stringify(report, null, 2);
await fetch("/__file_tab_result", { method: "POST", body: JSON.stringify(report) });
