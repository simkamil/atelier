// SPDX-License-Identifier: AGPL-3.0-or-later
// US-270 dans le vrai produit : ruban, sélecteur, collage, glisser-déposer, rendu Canvas, EXIF, refus, annulation.
import { decodeDisplayList } from "/display-list.mjs";

const checks = [];
function assert(value, name) { if (!value) throw new Error(name); checks.push(name); }
async function waitFor(predicate, name, timeoutMs = 8000) {
  const end = performance.now() + timeoutMs;
  while (performance.now() < end) { if (predicate()) return; await new Promise(r => setTimeout(r, 10)); }
  throw new Error(`Timed out waiting for ${name}`);
}
const inputText = (el, text) => el.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: text, bubbles: true, cancelable: true }));
const key = (el, name, mods = {}) => el.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...mods }));
const PICTURE = /[\u{100000}-\u{10fffd}]/gu;
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
  const editor = app.editor;
  const status = document.querySelector("#editor-status");
  const canvas = document.querySelector("#document-canvas");
  const state = () => app.getState();

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
    const ctx = view.getContext("2d"); ctx.drawImage(canvas, 0, 0);
    return [...ctx.getImageData(Math.round(x * ratio), Math.round(y * ratio), 1, 1).data];
  };
  const pictures = () => {
    const out = [];
    for (const page of state().pages) {
      for (const c of decodeDisplayList(page).commands) if (c.type === "drawImage") out.push(c.bounds);
    }
    return out;
  };
  const near = (rgb, expected, tolerance = 40) => expected.every((v, i) => Math.abs(rgb[i] - v) <= tolerance);
  const scrollTop = () => { document.querySelector("#document-scroll").scrollTop = 0; };

  // --- insertion par le sélecteur, au curseur, alignée sur le texte ----------------
  inputText(editor.element, "A"); inputText(editor.element, "B");
  await waitFor(() => state().text === "AB", "typed text");
  key(editor.element, "ArrowLeft");
  await waitFor(() => state().focus === 1, "caret between A and B");
  const red = await canvasBytes(200, 100, solid("rgb(255,0,0)"));
  pickerFiles = [file(red, "rouge.png", "image/png")];
  command.click();
  await waitFor(() => pictureCount(state().text) === 1, "picture inserted through the ribbon picker");
  const chars = [...state().text];
  assert(chars.length === 3 && chars[0] === "A" && chars[2] === "B", "picture sits exactly at the caret between the letters");
  assert(state().imageIds.size === 1 && state().dirty && state().canUndo, "state lists the picture and is undoable and dirty");
  const [box] = pictures();
  assert(Math.abs(box.width - 150) < 1e-6 && Math.abs(box.height - 75) < 1e-6, "200x100 px is laid out at 96 dpi (150 x 75 pt)");
  scrollTop();
  assert(near(pixelAt(0, box.min.x + box.width / 2, box.min.y + box.height / 2), [255, 0, 0]), "the Canvas shows the decoded picture pixels");
  assert(status.textContent.includes("image insérée") && !status.classList.contains("error"), "success is announced in the status line");
  assert(document.querySelector("#document-text").textContent.includes("￼") && !PICTURE.test(document.querySelector("#document-text").textContent),
    "accessible text exposes a replacement character, never a private-use scalar");
  assert(document.activeElement === editor.element, "focus returns to the document after the picker");

  // --- annuler / rétablir ----------------------------------------------------------
  document.querySelector("#undo").click();
  await waitFor(() => pictureCount(state().text) === 0 && state().text === "AB", "undo removes the picture");
  assert(pictures().length === 0, "undo removes the picture from the display list");
  document.querySelector("#redo").click();
  await waitFor(() => pictureCount(state().text) === 1, "redo restores the picture");
  scrollTop();
  const [again] = pictures();
  assert(near(pixelAt(0, again.min.x + again.width / 2, again.min.y + again.height / 2), [255, 0, 0]), "redo repaints the picture");

  // --- collage ---------------------------------------------------------------------
  const pasteEvent = (types, files, text = "") => {
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { types, files, getData: () => text, setData() {} } });
    editor.element.dispatchEvent(event);
    return event;
  };
  const blue = await canvasBytes(80, 40, solid("rgb(0,0,255)"));
  const before = pictureCount(state().text);
  const pasted = pasteEvent(["Files"], [file(blue, "image.png", "image/png")]);
  await waitFor(() => pictureCount(state().text) === before + 1, "pasted picture inserted");
  assert(pasted.defaultPrevented, "image paste is handled by the editor");
  const revision = state().revision;
  const textPaste = pasteEvent(["text/plain", "Files"], [file(blue, "cellule.png", "image/png")], "texte");
  await waitFor(() => state().text.includes("texte"), "text keeps priority when the clipboard has text and an image");
  assert(textPaste.defaultPrevented && pictureCount(state().text) === before + 1 && state().revision > revision, "mixed clipboard pastes text only");

  // --- glisser-déposer -------------------------------------------------------------
  document.querySelector("#new-document").click();
  await waitFor(() => state().text === "", "new document");
  inputText(editor.element, "premiere ligne");
  await waitFor(() => state().text === "premiere ligne", "line typed");
  const green = await canvasBytes(60, 60, solid("rgb(0,200,0)"));
  const transfer = new DataTransfer();
  transfer.items.add(file(green, "vert.png", "image/png"));
  const rect = canvas.getBoundingClientRect();
  const target = { clientX: rect.left + 54 + 2, clientY: rect.top + 54 + 6 };
  canvas.dispatchEvent(new DragEvent("dragover", { dataTransfer: transfer, bubbles: true, cancelable: true, ...target }));
  canvas.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true, ...target }));
  await waitFor(() => pictureCount(state().text) === 1, "dropped picture inserted");
  assert([...state().text][0].codePointAt(0) >= 0x100000, "drop inserts at the pointed text offset (start of the line)");
  assert(state().text.endsWith("premiere ligne"), "dropped file leaves the text intact");
  scrollTop();
  const [dropped] = pictures();
  assert(near(pixelAt(0, dropped.min.x + dropped.width / 2, dropped.min.y + dropped.height / 2), [0, 200, 0]), "dropped picture is painted");

  // --- réduction à la largeur de la zone de texte -----------------------------------
  const wide = await canvasBytes(2000, 1000, solid("rgb(10,10,10)"));
  pickerFiles = [file(wide, "large.png", "image/png")];
  command.click();
  await waitFor(() => pictureCount(state().text) === 2, "wide picture inserted");
  const widest = pictures().reduce((a, b) => (b.width > a.width ? b : a));
  assert(Math.abs(widest.width - (595.276 - 108)) < 0.01 && Math.abs(widest.width / widest.height - 2) < 1e-6,
    "a picture wider than the text area shrinks proportionally to its width");
  assert(Math.abs(widest.height - (595.276 - 108) / 2) < 1e-6, "the reduced picture keeps its aspect ratio in height");

  // --- EXIF -------------------------------------------------------------------------
  document.querySelector("#new-document").click();
  await waitFor(() => state().text === "" && state().imageIds.size === 0, "document reset");
  const jpeg = await canvasBytes(60, 30, halves("rgb(255,0,0)", "rgb(0,0,255)"), "image/jpeg");
  pickerFiles = [file(withExifOrientation(jpeg, 6), "photo.jpg", "image/jpeg")];
  command.click();
  await waitFor(() => pictureCount(state().text) === 1, "EXIF picture inserted");
  scrollTop();
  const [photo] = pictures();
  assert(Math.abs(photo.width - 22.5) < 1e-6 && Math.abs(photo.height - 45) < 1e-6, "orientation 6 swaps the laid-out dimensions (60x30 -> 30x60 px)");
  const topPixel = pixelAt(0, photo.min.x + photo.width / 2, photo.min.y + photo.height * 0.2);
  const bottomPixel = pixelAt(0, photo.min.x + photo.width / 2, photo.min.y + photo.height * 0.8);
  assert(near(topPixel, [255, 0, 0], 60) && near(bottomPixel, [0, 0, 255], 60), "orientation 6 is applied to the rendered pixels (left half becomes the top)");

  // --- cache invalidé au changement de document : l'identifiant 0 est réutilisé ----------
  document.querySelector("#new-document").click();
  await waitFor(() => state().text === "", "second reset");
  const orange = await canvasBytes(40, 40, solid("rgb(255,128,0)"));
  pickerFiles = [file(orange, "orange.png", "image/png")];
  command.click();
  await waitFor(() => pictureCount(state().text) === 1, "picture in the new document");
  scrollTop();
  const [fresh] = pictures();
  assert(near(pixelAt(0, fresh.min.x + fresh.width / 2, fresh.min.y + fresh.height / 2), [255, 128, 0]),
    "a new document never shows the previous document's picture for the same id");

  // --- refus --------------------------------------------------------------------------
  const refused = async (files, fragment, name) => {
    const snapshot = { revision: state().revision, text: state().text };
    pickerFiles = files;
    command.click();
    await waitFor(() => status.textContent.includes(fragment), name);
    assert(status.classList.contains("error") && state().revision === snapshot.revision && state().text === snapshot.text, `${name}: document unchanged`);
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
  await fetch("/__image_result", { method: "POST", body: JSON.stringify({ status: "passed", checks, userAgent: navigator.userAgent }) });
} catch (error) {
  resultNode.textContent = String(error?.stack ?? error);
  await fetch("/__image_result", { method: "POST", body: JSON.stringify({ status: "failed", message: String(error?.stack ?? error) + " | status: " + document.querySelector("#editor-status")?.textContent, checks }) });
}
