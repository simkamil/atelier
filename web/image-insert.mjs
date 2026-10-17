// SPDX-License-Identifier: AGPL-3.0-or-later
// Insertion d'images (US-270) : sélecteur de fichier, collage, glisser-déposer, lecture bornée,
// cache des bitmaps d'affichage et correspondance code d'erreur -> clé du catalogue i18n.
// Le moteur valide et décode ; ce module ne fait que transporter les octets d'origine.
import { EDITOR_LIMITS } from "./editor-protocol.mjs";

/** `image/*` laisse choisir un format refusé (WebP, TIFF…) pour obtenir un message clair. */
export const IMAGE_ACCEPT = "image/*,.png,.jpg,.jpeg,.jfif,.gif,.bmp";
const MAX_CACHE_BYTES = 256 * 1024 * 1024;

export class ImageInsertError extends Error {
  constructor(code, message = code) { super(message); this.name = "ImageInsertError"; this.code = code; }
}

const MESSAGE_KEYS = Object.freeze({
  EDITOR_IMAGE_UNSUPPORTED: "image-error-unsupported",
  EDITOR_IMAGE_CORRUPT: "image-error-corrupt",
  EDITOR_IMAGE_TOO_LARGE: "image-error-too-large",
  MESSAGE_TOO_LARGE: "image-error-too-large",
  EDITOR_IMAGE_LIMIT: "image-error-limit",
  EDITOR_IMAGE_READ: "image-error-read",
});

/** Clé du catalogue pour un code d'erreur d'insertion d'image, ou null si le code est étranger. */
export function imageMessageKey(code) {
  return Object.hasOwn(MESSAGE_KEYS, code) ? MESSAGE_KEYS[code] : null;
}

/** Lit un fichier image en refusant d'avance ce qui dépasse la limite du moteur. */
export async function readImageFile(file) {
  if (!file || typeof file.arrayBuffer !== "function" || !Number.isFinite(file.size)) throw new ImageInsertError("EDITOR_IMAGE_READ");
  if (file.size > EDITOR_LIMITS.maxImageBytes) throw new ImageInsertError("EDITOR_IMAGE_TOO_LARGE");
  let buffer;
  try { buffer = await file.arrayBuffer(); } catch { throw new ImageInsertError("EDITOR_IMAGE_READ"); }
  if (buffer.byteLength > EDITOR_LIMITS.maxImageBytes) throw new ImageInsertError("EDITOR_IMAGE_TOO_LARGE");
  if (buffer.byteLength === 0) throw new ImageInsertError("EDITOR_IMAGE_CORRUPT");
  return new Uint8Array(buffer);
}

/**
 * Insère les fichiers un par un à la position courante ; une erreur n'arrête pas les suivants.
 * `insertBytes(bytes)` envoie l'image au moteur ; `onError` reçoit chaque échec.
 */
export async function insertImageFiles(files, { insertBytes, onError = () => {} }) {
  let inserted = 0;
  let failed = 0;
  for (const file of files) {
    try {
      await insertBytes(await readImageFile(file));
      inserted += 1;
    } catch (error) {
      failed += 1;
      try { onError(error); } catch { /* le rapport d'erreur ne doit pas bloquer la suite */ }
    }
  }
  return { inserted, failed };
}

/** True quand le transfert (glisser ou presse-papiers) annonce des fichiers. */
export function transferHasFiles(transfer) {
  try { return Array.from(transfer?.types ?? []).includes("Files"); } catch { return false; }
}

/** Fichiers d'un transfert, dans l'ordre. À lire pendant l'événement : le transfert est protégé ensuite. */
export function filesFromTransfer(transfer) {
  try { return Array.from(transfer?.files ?? []); } catch { return []; }
}

/**
 * Collage d'une image : actif seulement quand le presse-papiers contient une image et aucun texte brut,
 * pour que le collage d'une cellule de tableur ou d'une page web garde son texte.
 */
export function attachImagePaste({ target, onFiles, isEnabled = () => true, doc = globalThis.document }) {
  if (!target || typeof onFiles !== "function" || !doc?.addEventListener) throw new TypeError("image paste requires a target and a callback");
  function onPaste(event) {
    if (event.target !== target) return;
    const data = event.clipboardData;
    if (!data) return;
    let types;
    try { types = Array.from(data.types ?? []); } catch { return; }
    if (types.includes("text/plain")) return;
    const files = filesFromTransfer(data).filter(file => typeof file.type === "string" && file.type.startsWith("image/"));
    if (files.length === 0) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (isEnabled()) onFiles(files);
  }
  // Capture sur le document : passe avant le traitement du texte de la zone de saisie.
  doc.addEventListener("paste", onPaste, true);
  return Object.freeze({ dispose() { doc.removeEventListener("paste", onPaste, true); } });
}

/** Sélecteur de fichiers : `open()` rend les fichiers choisis, ou [] si le dialogue est annulé. */
export function createImagePicker({ doc = globalThis.document, accept = IMAGE_ACCEPT } = {}) {
  const input = doc.createElement("input");
  input.type = "file";
  input.accept = accept;
  input.multiple = true;
  input.hidden = true;
  input.tabIndex = -1;
  input.id = "image-file-input";
  input.setAttribute("aria-hidden", "true");
  doc.body.append(input);
  let pending = null;
  function settle(files) {
    const resolve = pending;
    pending = null;
    input.value = "";
    resolve?.(files);
  }
  const onChange = () => settle(Array.from(input.files ?? []));
  const onCancel = () => settle([]);
  input.addEventListener("change", onChange);
  input.addEventListener("cancel", onCancel);
  return Object.freeze({
    input,
    open() {
      if (pending) return Promise.resolve([]);
      return new Promise(resolve => { pending = resolve; input.value = ""; input.click(); });
    },
    dispose() {
      settle([]);
      input.removeEventListener("change", onChange);
      input.removeEventListener("cancel", onCancel);
      input.remove();
    },
  });
}

async function drawable({ width, height, rgba }) {
  const data = new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.byteLength), width, height);
  if (typeof createImageBitmap === "function") return createImageBitmap(data);
  const canvas = globalThis.document.createElement("canvas");
  canvas.width = width; canvas.height = height;
  canvas.getContext("2d").putImageData(data, 0, 0);
  return canvas;
}

/**
 * Bitmaps d'affichage des images du document, indexés par identifiant d'image de la liste d'affichage.
 * `prepare` les demande au moteur avant la publication d'un état ; `get` sert le moteur de rendu.
 * Les bitmaps non référencés par l'état courant sont retirés au-delà de la borne mémoire.
 */
export function createImageCache({ maxBytes = MAX_CACHE_BYTES } = {}) {
  const items = new Map();
  let total = 0;
  const release = entry => { try { entry.drawable.close?.(); } catch { /* déjà libéré */ } };
  return Object.freeze({
    get(id) { return items.get(Number(id))?.drawable ?? null; },
    get size() { return items.size; },
    async prepare(ids, fetchBitmap) {
      const wanted = new Set(ids);
      for (const id of wanted) {
        if (items.has(id)) continue;
        const response = await fetchBitmap(id);
        if (response?.kind !== "imageBitmap" || response.id !== id) throw new ImageInsertError("EDITOR_INVALID_RESPONSE");
        items.set(id, { drawable: await drawable(response), bytes: response.rgba.byteLength });
        total += response.rgba.byteLength;
      }
      for (const [id, entry] of items) {
        if (total <= maxBytes) break;
        if (wanted.has(id)) continue;
        items.delete(id); total -= entry.bytes; release(entry);
      }
    },
    clear() {
      for (const entry of items.values()) release(entry);
      items.clear(); total = 0;
    },
  });
}
