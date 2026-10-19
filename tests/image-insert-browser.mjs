// SPDX-License-Identifier: AGPL-3.0-or-later
// US-270 dans le vrai produit : ruban, sélecteur, collage, glisser-déposer, rendu Canvas, EXIF, refus, annulation.
import { decodeDisplayList } from "/display-list.mjs";

// R7a : `?engine=v2` lance le même scénario avec l'éditeur v2. R7b-1 : les pages v2 dessinent les images (`drawImage`,
// bitmaps demandés au moteur) : géométrie, pixels, EXIF, annuler et rétablir, réduction sont vérifiés comme en v1 (aux
// arrondis de 1/64 pt près pour v2) ; plus rien n'est sauté.
const v2 = new URLSearchParams(location.search).get("engine") === "v2";
const checks = [];
const skipped = [];
function assert(value, name) { if (!value) throw new Error(name); checks.push(name); }
// Tolerances: v2 lays out in 1/64 pt, v1 in exact floats.
const EPS = v2 ? 0.05 : 1e-6;
const RATIO_EPS = v2 ? 2e-3 : 1e-6;
async function waitFor(predicate, name, timeoutMs = 8000) {
  const end = performance.now() + timeoutMs;
  while (performance.now() < end) { if (await predicate()) return; await new Promise(r => setTimeout(r, 10)); }
  throw new Error(`Timed out waiting for ${name}`);
}
const inputText = (el, text) => el.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: text, bubbles: true, cancelable: true }));
const key = (el, name, mods = {}) => el.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...mods }));
const PICTURE = v2 ? /\ufffc/gu : /[\u{100000}-\u{10fffd}]/gu;
const pictureCount = text => (text.match(PICTURE) ?? []).length;

async function canvasBytes(width, height, paint, type = "image/png") {
  const canvas = new OffscreenCanvas(width, height);
  paint(canvas.getContext("2d"), width, height);
  return new Uint8Array(await (await canvas.convertToBlob({ type, quality: 1 })).arrayBuffer());
}
const halves = (left, right) => (ctx, w, h) => { ctx.fillStyle = left; ctx.fillRect(0, 0, w / 2, h); ctx.fillStyle = right; ctx.fillRect(w / 2, 0, w / 2, h); };
const solid = color => (ctx, w, h) => { ctx.fillStyle = color; ctx.fillRect(0, 0, w, h); };
function withExifOrientation(jpeg, orientation) {
  const tiff = [0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, orientation, 0, 0, 0, 0, 0, 0, 0];
  const payload = [0x45, 0x78, 0x69, 0x66, 0, 0, ...tiff];
  const length = payload.length + 2;
  return Uint8Array.from([...jpeg.subarray(0, 2), 0xff, 0xe1, length >> 8, length & 255, ...payload, ...jpeg.subarray(2)]);
}
const file = (bytes, name, type) => new File([bytes], name, { type });

const resultNode = document.querySelector("#result");
let app;
try {
  // Même méthode que les autres suites : le produit réel, WebView natif simulé.
  const product = await fetch("/index.html");
  assert(product.ok, "product entry document available");
  const doc = new DOMParser().parseFromString(await product.text(), "text/html");
  for (const source of doc.querySelectorAll('link[rel="stylesheet"]')) {
    const sheet = document.createElement("link");
    sheet.rel = "stylesheet"; sheet.href = source.getAttribute("href");
    const loaded = new Promise((yes, no) => { sheet.onload = yes; sheet.onerror = no; });
    document.head.append(sheet); await loaded;
  }
  document.body.replaceWith(document.importNode(doc.body, true));
  const listeners = new Set();
  Object.defineProperty(window, "chrome", { configurable: true, value: { webview: {
    postMessage(message) {
      const match = /^us120-file\|1\|([1-9][0-9]*)\|(\w+)\|/.exec(message);
      if (!match || match[2] === "dirty") return;
      queueMicrotask(() => { for (const l of listeners) l({ data: `us120-file|1|${match[1]}|cancelled|` }); });
    },
    addEventListener(type, l) { if (type === "message") listeners.add(l); },
    removeEventListener(type, l) { if (type === "message") listeners.delete(l); },
  } } });
  localStorage.setItem("atelier.frenchSpacing", "false");
  window.confirm = () => true;
  const { editorReady } = await import("/app.mjs");
  app = await editorReady;
  assert(app.status === "ready", "product initializes with Worker and optimized WASM");
  // US-327 : le document affiché change quand un document s'ouvre dans un nouvel onglet ; ces références désignent toujours celui de l'onglet actif.
  const liveNode = id => new Proxy({}, {
    get: (_, key) => { const node = document.getElementById(id); const value = node[key]; return typeof value === "function" ? value.bind(node) : value; },
    set: (_, key, value) => { document.getElementById(id)[key] = value; return true; },
  });
  const canvasNode = () => document.getElementById("document-canvas");
  const editor = new Proxy({}, { get: (_, key) => { const target = app.editor; const value = target[key]; return typeof value === "function" ? value.bind(target) : value; } });
  const status = document.querySelector("#editor-status");
  const canvas = liveNode("document-canvas");
  const state = () => app.getState();
  // v1 keeps the whole text in the state; v2 asks the engine for it.
  const textOf = async () => (v2 ? editor.readText() : state().text);
  const picturesIn = async () => pictureCount(await textOf());

  // --- ruban ------------------------------------------------------------------
  app.ribbon.selectTab("insert");
  const command = document.querySelector("#image-command");
  assert(command && command.tagName === "BUTTON" && command.textContent === "Image" && !command.disabled,
    "Insertion ribbon exposes an enabled, labelled Image button (keyboard reachable)");
  const fileInput = document.querySelector("#image-file-input");
  assert(fileInput && fileInput.type === "file" && fileInput.accept.includes("image/") && fileInput.getAttribute("aria-hidden") === "true",
    "hidden file input is created for the picker");
  let pickerFiles = [];
  fileInput.click = () => {
    const transfer = new DataTransfer();
    for (const f of pickerFiles) transfer.items.add(f);
    fileInput.files = transfer.files;
    fileInput.dispatchEvent(new Event("change"));
  };
  const pixelAt = (page, x, y) => {
    const ratio = devicePixelRatio || 1;
    const view = document.createElement("canvas");
    view.width = canvas.width; view.height = canvas.height;
    const ctx = view.getContext("2d"); ctx.drawImage(canvasNode(), 0, 0);
    return [...ctx.getImageData(Math.round(x * ratio), Math.round(y * ratio), 1, 1).data];
  };
  const pictures = () => {
    if (v2) return editor.pictureBoxes().filter(p => p.page === 0).map(p => p.bounds);
    const out = [];
    for (const page of state().pages) {
      for (const c of decodeDisplayList(page).commands) if (c.type === "drawImage") out.push(c.bounds);
    }
    return out;
  };
  // v2 asks the engine for the bitmaps after the page arrives: the pixels show a moment later.
  const showsColor = (box, fx, fy, expected, tolerance = 40) => near(pixelAt(0, box.min.x + box.width * fx, box.min.y + box.height * fy), expected, tolerance);
  const waitForColor = (getBox, fx, fy, expected, name, tolerance) => waitFor(() => { scrollTop(); const box = getBox(); return box && showsColor(box, fx, fy, expected, tolerance); }, name);
  const near = (rgb, expected, tolerance = 40) => expected.every((v, i) => Math.abs(rgb[i] - v) <= tolerance);
  const scrollTop = () => { document.querySelector("#document-scroll").scrollTop = 0; };

  // --- insertion par le sélecteur, au curseur, alignée sur le texte ----------------
  inputText(editor.element, "A"); inputText(editor.element, "B");
  await waitFor(async () => await textOf() === "AB", "typed text");
  key(editor.element, "ArrowLeft");
  await waitFor(() => state().focus === 1, "caret between A and B");
  const red = await canvasBytes(200, 100, solid("rgb(255,0,0)"));
  pickerFiles = [file(red, "rouge.png", "image/png")];
  command.click();
  await waitFor(async () => await picturesIn() === 1, "picture inserted through the ribbon picker");
  const chars = [...await textOf()];
  assert(chars.length === 3 && chars[0] === "A" && chars[2] === "B", "picture sits exactly at the caret between the letters");
  if (v2) assert(state().dirty && state().canUndo, "state is undoable and dirty after the picture");
  else assert(state().imageIds.size === 1 && state().dirty && state().canUndo, "state lists the picture and is undoable and dirty");
  await waitFor(() => pictures().length === 1, "the page draws the picture");
  {
    const [box] = pictures();
    assert(Math.abs(box.width - 150) < EPS && Math.abs(box.height - 75) < EPS, "200x100 px is laid out at 96 dpi (150 x 75 pt)");
    // The picture sits on the line of the letters: it starts after the "A" and no further left than the margin (25 mm, the default document profile of US-202).
    assert(box.min.x > 70.8 && box.min.x < 70.8 + 12, "the picture is placed right after the letter A");
    await waitForColor(() => pictures()[0], 0.5, 0.5, [255, 0, 0], "the Canvas shows the decoded picture pixels");
  }
  assert(status.textContent.includes("image insérée") && !status.classList.contains("error"), "success is announced in the status line");
  if (v2) {
    await waitFor(() => document.querySelectorAll(".document-mirror [role=img]").length === 1, "mirror exposes the picture");
    assert(document.querySelector("#document-text").hidden, "v2 hides the plain text region: the DOM mirror carries the accessible content");
  } else assert(document.querySelector("#document-text").textContent.includes("￼") && !PICTURE.test(document.querySelector("#document-text").textContent),
    "accessible text exposes a replacement character, never a private-use scalar");
  assert(document.activeElement === editor.element, "focus returns to the document after the picker");

  // --- annuler / rétablir ----------------------------------------------------------
  document.querySelector("#undo").click();
  await waitFor(async () => await picturesIn() === 0 && await textOf() === "AB", "undo removes the picture");
  if (v2) await waitFor(() => document.querySelectorAll(".document-mirror [role=img]").length === 0, "mirror drops the picture");
  await waitFor(() => pictures().length === 0, "undo removes the picture from the display list");
  document.querySelector("#redo").click();
  await waitFor(async () => await picturesIn() === 1, "redo restores the picture");
  await waitForColor(() => pictures()[0], 0.5, 0.5, [255, 0, 0], "redo repaints the picture");
  assert(true, "redo repaints the picture");

  // --- collage ---------------------------------------------------------------------
  const pasteEvent = (types, files, text = "") => {
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { types, files, getData: () => text, setData() {} } });
    editor.element.dispatchEvent(event);
    return event;
  };
  const blue = await canvasBytes(80, 40, solid("rgb(0,0,255)"));
  const before = await picturesIn();
  const pasted = pasteEvent(["Files"], [file(blue, "image.png", "image/png")]);
  await waitFor(async () => await picturesIn() === before + 1, "pasted picture inserted");
  assert(pasted.defaultPrevented, "image paste is handled by the editor");
  const revision = state().revision;
  const textPaste = pasteEvent(["text/plain", "Files"], [file(blue, "cellule.png", "image/png")], "texte");
  await waitFor(async () => (await textOf()).includes("texte"), "text keeps priority when the clipboard has text and an image");
  assert(textPaste.defaultPrevented && await picturesIn() === before + 1 && state().revision > revision, "mixed clipboard pastes text only");

  // --- glisser-déposer -------------------------------------------------------------
  document.querySelector("#new-document").click();
  await waitFor(async () => await textOf() === "", "new document");
  inputText(editor.element, "premiere ligne");
  await waitFor(async () => await textOf() === "premiere ligne", "line typed");
  const green = await canvasBytes(60, 60, solid("rgb(0,200,0)"));
  const transfer = new DataTransfer();
  transfer.items.add(file(green, "vert.png", "image/png"));
  const rect = canvas.getBoundingClientRect();
  const MARGIN = 1417 / 20; const TEXT_WIDTH = (11906 - 2 * 1417) / 20; // A4 with 25 mm margins: default document profile (US-202)
  const target = { clientX: rect.left + MARGIN + 2, clientY: rect.top + MARGIN + 6 };
  canvas.dispatchEvent(new DragEvent("dragover", { dataTransfer: transfer, bubbles: true, cancelable: true, ...target }));
  canvas.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true, ...target }));
  await waitFor(async () => await picturesIn() === 1, "dropped picture inserted");
  const dropText = await textOf();
  assert(v2 ? [...dropText][0] === "\ufffc" : [...dropText][0].codePointAt(0) >= 0x100000, "drop inserts at the pointed text offset (start of the line)");
  assert(dropText.endsWith("premiere ligne"), "dropped file leaves the text intact");
  await waitForColor(() => pictures()[0], 0.5, 0.5, [0, 200, 0], "dropped picture is painted");
  assert(true, "dropped picture is painted");

  // --- réduction à la largeur de la zone de texte -----------------------------------
  const wide = await canvasBytes(2000, 1000, solid("rgb(10,10,10)"));
  pickerFiles = [file(wide, "large.png", "image/png")];
  command.click();
  await waitFor(async () => await picturesIn() === 2, "wide picture inserted");
  await waitFor(() => pictures().length === 2, "the page draws both pictures");
  {
    const widest = pictures().reduce((a, b) => (b.width > a.width ? b : a));
    assert(Math.abs(widest.width - TEXT_WIDTH) < Math.max(0.01, EPS) && Math.abs(widest.width / widest.height - 2) < RATIO_EPS,
      "a picture wider than the text area shrinks proportionally to its width");
    assert(Math.abs(widest.height - TEXT_WIDTH / 2) < Math.max(1e-6, EPS), "the reduced picture keeps its aspect ratio in height");
  }

  // --- EXIF -------------------------------------------------------------------------
  document.querySelector("#new-document").click();
  await waitFor(async () => await textOf() === "" && (v2 || state().imageIds.size === 0), "document reset");
  const jpeg = await canvasBytes(60, 30, halves("rgb(255,0,0)", "rgb(0,0,255)"), "image/jpeg");
  pickerFiles = [file(withExifOrientation(jpeg, 6), "photo.jpg", "image/jpeg")];
  command.click();
  await waitFor(async () => await picturesIn() === 1, "EXIF picture inserted");
  await waitFor(() => pictures().length === 1, "the page draws the EXIF picture");
  {
    const [photo] = pictures();
    assert(Math.abs(photo.width - 22.5) < EPS && Math.abs(photo.height - 45) < EPS, "orientation 6 swaps the laid-out dimensions (60x30 -> 30x60 px)");
    await waitForColor(() => pictures()[0], 0.5, 0.2, [255, 0, 0], "orientation 6 puts the left half on top", 60);
    const lower = () => showsColor(pictures()[0], 0.5, 0.8, [0, 0, 255], 60);
    assert(lower(), "orientation 6 is applied to the rendered pixels (left half becomes the top)");
  }

  // --- cache invalidé au changement de document : l'identifiant 0 est réutilisé ----------
  document.querySelector("#new-document").click();
  await waitFor(async () => await textOf() === "", "second reset");
  const orange = await canvasBytes(40, 40, solid("rgb(255,128,0)"));
  pickerFiles = [file(orange, "orange.png", "image/png")];
  command.click();
  await waitFor(async () => await picturesIn() === 1, "picture in the new document");
  await waitForColor(() => pictures()[0], 0.5, 0.5, [255, 128, 0], "a new document never shows the previous document's picture for the same id");
  assert(true, "a new document never shows the previous document's picture for the same id");

  // --- refus --------------------------------------------------------------------------
  const refused = async (files, fragment, name) => {
    const snapshot = { revision: state().revision, text: await textOf() };
    pickerFiles = files;
    command.click();
    await waitFor(() => status.textContent.includes(fragment), name);
    assert(status.classList.contains("error") && state().revision === snapshot.revision && await textOf() === snapshot.text, `${name}: document unchanged`);
  };
  const webp = await canvasBytes(10, 10, solid("rgb(1,2,3)"), "image/webp");
  await refused([file(webp, "image.webp", "image/webp")], "non pris en charge", "WebP is refused with a clear message");
  await refused([file(Uint8Array.from([0x49, 0x49, 42, 0, 8, 0, 0, 0, 0, 0]), "scan.tif", "image/tiff")], "non pris en charge", "TIFF is refused with a clear message");
  await refused([file(new TextEncoder().encode("pas une image"), "note.png", "image/png")], "non pris en charge", "a text file renamed .png is refused");
  await refused([file(red.subarray(0, Math.floor(red.length / 2)), "cassee.png", "image/png")], "corrompue", "a truncated PNG is reported as corrupt");
  await refused([file(new Uint8Array(9 * 1024 * 1024), "enorme.png", "image/png")], "volumineuse", "an oversized file is refused before reaching the engine");
  pickerFiles = [file(red, "rouge.png", "image/png")];
  command.click();
  await waitFor(() => status.textContent.includes("image insérée"), "editor still usable after refusals");

  // --- enregistrement DOCX : octets d'origine -----------------------------------------
  const exported = await editor.exportDocx();
  const head = new TextDecoder("latin1").decode(exported.bytes.subarray(0, 2));
  assert(head === "PK" && exported.pictureCount === 2, "DOCX export is a package carrying both pictures");
  const raw = new TextDecoder("latin1").decode(exported.bytes);
  assert(raw.includes("word/media/image1.png") && raw.includes("word/media/image2.png"), "media parts are named word/media/imageN.png");
  assert(raw.includes("[Content_Types].xml"), "package has its content types part");

  resultNode.textContent = "passed";
  await fetch("/__image_result", { method: "POST", body: JSON.stringify({ status: "passed", engine: "v2", checks, skipped, userAgent: navigator.userAgent }) });
} catch (error) {
  resultNode.textContent = String(error?.stack ?? error);
  await fetch("/__image_result", { method: "POST", body: JSON.stringify({ status: "failed", message: String(error?.stack ?? error) + " | status: " + document.querySelector("#editor-status")?.textContent, checks }) });
}
