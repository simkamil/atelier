// SPDX-License-Identifier: AGPL-3.0-or-later

const MAX_U64 = (1n << 64n) - 1n;
const MAX_FILE_BYTES = 131075;
const MAX_DOCX_BYTES = 8 * 1024 * 1024;
const MAX_QUEUE = 4;
const MAX_QUEUED_BYTES = MAX_DOCX_BYTES + MAX_FILE_BYTES * MAX_QUEUE;
const PREFIX = "us120-file|1|";
const LAUNCH_NOTICE = "us115-launch|1";
const MAX_LAUNCH_SKIPPED = 99;
/** US-113 : taille d'un morceau d'écriture avant Base64 (le natif accepte jusqu'à 128 Kio). */
export const WRITE_CHUNK_BYTES = 96 * 1024;
/** US-113 : taille maximale d'un fichier écrit par le pont. */
export const MAX_WRITE_BYTES = 64 * 1024 * 1024;
const MAX_SHARED_BYTES = 1024 * 1024 * 1024;
const MAX_URL_LENGTH = 2048;
const MAX_FONT_LIST_BYTES = 2 * 1024 * 1024;
/** Formats que le natif sait proposer dans ses dialogues ; le moteur choisit le sous-ensemble utile. */
export const DIALOG_FORMATS = Object.freeze(["txt", "docx", "docm", "dotx", "odt", "doc"]);
const SHARED_OPERATIONS = new Set(["readTicket", "readFont", "listFonts", "launch"]);

/**
 * Clés d'erreur stables émises par le natif (catalogue `web/i18n/fr-FR.txt`). Le front les traduit avec
 * `i18n.t(key)` ; les codes `HOST_*` sont des erreurs locales du transport.
 */
export const BRIDGE_ERROR_KEYS = Object.freeze([
  "files.error.broker_full", "files.error.com_init_failed", "files.error.destination_changed",
  "files.error.destination_is_directory", "files.error.dialog_failed", "files.error.empty_interface",
  "files.error.empty_result", "files.error.invalid_dialog_options", "files.error.invalid_filter",
  "files.error.invalid_limit", "files.error.invalid_path", "files.error.invalid_request", "files.error.io",
  "files.error.not_found", "files.error.not_regular_file", "files.error.read_failed", "files.error.reparse_point",
  "files.error.save_failed", "files.error.shared_buffer_failed", "files.error.shared_buffer_unavailable",
  "files.error.ticket_exhausted", "files.error.unknown_ticket", "files.error.unsupported_format",
  "files.error.windows_only", "files.error.write_sequence", "files.error.wrong_capability",
  "fonts.error.changed", "fonts.error.read_failed", "fonts.error.too_large", "fonts.error.unknown_id",
  "url.error.access_denied", "url.error.invalid", "url.error.launch_failed", "url.error.no_handler",
  "url.error.scheme_not_allowed", "url.error.too_long",
]);

export class NativeHostError extends Error {
  constructor(code) { super(code); this.name = "NativeHostError"; this.code = code; }
}

function toBase64(bytes) {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function fromBase64(value, maxBytes) {
  if (typeof value === "string" && value.length > 4 * Math.ceil(maxBytes / 3)) throw new NativeHostError("HOST_FILE_TOO_LARGE");
  if (typeof value !== "string" || value.length % 4 !== 0
      || /[^A-Za-z0-9+/=]/.test(value)) {
    throw new NativeHostError("HOST_INVALID_RESPONSE");
  }
  let binary;
  try { binary = atob(value); } catch { throw new NativeHostError("HOST_INVALID_RESPONSE"); }
  if (binary.length > maxBytes) throw new NativeHostError("HOST_FILE_TOO_LARGE");
  const bytes = Uint8Array.from(binary, char => char.charCodeAt(0));
  if (toBase64(bytes) !== value) throw new NativeHostError("HOST_INVALID_RESPONSE");
  return bytes;
}

function hasControl(text) { return /[\p{Cc}\p{Zl}\p{Zp}]/u.test(text); }
function plainText(text, max) {
  return typeof text === "string" && text.length > 0 && [...text].length <= max && !hasControl(text) && text.trim() === text;
}

/**
 * Valide une URL avant envoi (miroir de `url_open.rs`) : http/https seulement, ASCII visible, bornée,
 * hôte simple sans identifiants. Renvoie une clé d'erreur `url.error.*` ou `null`.
 */
export function validateUrl(url) {
  if (typeof url !== "string" || url.length === 0) return "url.error.invalid";
  if (url.length > MAX_URL_LENGTH) return "url.error.too_long";
  if (/[^\x21-\x7e]|["<>\\^`{}|]/.test(url)) return "url.error.invalid";
  const match = /^(https?):\/\/([^/?#]*)/i.exec(url);
  if (!match) {
    const scheme = /^([A-Za-z][A-Za-z0-9+.-]*):/.exec(url);
    return scheme && !url.startsWith("//") ? "url.error.scheme_not_allowed" : "url.error.invalid";
  }
  return /^[A-Za-z0-9.\-:[\]]+$/.test(match[2]) && !match[2].startsWith(":") ? null : "url.error.invalid";
}

/**
 * Texte de spécification d'un dialogue (titre, filtres `{format, label}`, nom suggéré à l'enregistrement),
 * validé comme le natif. Renvoie `null` si la spécification est invalide.
 */
export function buildDialogSpec({ title, filters, suggestedName } = {}, save = false) {
  if (!plainText(title, 120) || !Array.isArray(filters) || filters.length < 1 || filters.length > 8) return null;
  const seen = new Set();
  const lines = [title];
  for (const filter of filters) {
    if (!filter || !DIALOG_FORMATS.includes(filter.format) || seen.has(filter.format) || !plainText(filter.label, 80)) return null;
    seen.add(filter.format);
    lines.push(`filter:${filter.format}=${filter.label}`);
  }
  if (suggestedName !== undefined) {
    if (!save || !plainText(suggestedName, 120) || /[\\/:*?"<>|]/.test(suggestedName) || suggestedName.endsWith(".") || suggestedName === "..") return null;
    lines.push(`name:${suggestedName}`);
  }
  const text = lines.join("\n");
  return new TextEncoder().encode(text).length <= 4096 ? text : null;
}

function decodeName(encoded) {
  const bytes = fromBase64(encoded, 1024);
  try { return new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { throw new NativeHostError("HOST_INVALID_RESPONSE"); }
}

/** Bounded, request-correlated adapter for the native US-113 file bridge. */
export function createNativeHost({ webview = globalThis.chrome?.webview, timeoutMs = 10 * 60 * 1000 } = {}) {
  if (!webview || typeof webview.postMessage !== "function"
      || typeof webview.addEventListener !== "function" || typeof webview.removeEventListener !== "function") {
    throw new NativeHostError("HOST_UNAVAILABLE");
  }
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) throw new TypeError("timeoutMs must be a positive safe integer.");
  let nextId = 1n;
  let closed = false;
  let queuedBytes = 0;
  const pending = new Map();
  const launchListeners = new Set();
  const heldBuffers = new Set();
  const sharedSupported = typeof webview.releaseBuffer === "function";
  const stats = { sharedReceived: 0, sharedReleased: 0, inlineReceived: 0, fallbackReceived: 0 };

  function releaseBuffer(buffer) {
    if (!heldBuffers.delete(buffer)) return;
    stats.sharedReleased += 1;
    try { webview.releaseBuffer(buffer); } catch { /* le tampon est déjà libéré côté page */ }
  }

  function parseReply(reply) {
    if (typeof reply !== "string" || !reply.startsWith(PREFIX)) return null;
    const fields = reply.split("|");
    if (fields.length !== 5 || fields[0] !== "us120-file" || fields[1] !== "1"
        || !/^[1-9][0-9]{0,19}$/.test(fields[2])) return null;
    let id;
    try { id = BigInt(fields[2]); } catch { return null; }
    if (id > MAX_U64) return null;
    return { id, status: fields[3], payload: fields[4] };
  }

  /** Données `b64;…` / `fb;…` renvoyées en ligne par `readTicket`, `readFont` et `listFonts`. */
  function inlineBytes(payload, maxBytes) {
    const match = /^(b64|fb);([A-Za-z0-9+/=]*)$/.exec(payload);
    if (!match) throw new NativeHostError("HOST_INVALID_RESPONSE");
    const bytes = fromBase64(match[2], maxBytes);
    if (match[1] === "fb") stats.fallbackReceived += 1; else stats.inlineReceived += 1;
    return bytes;
  }

  function finishData(id, entry, bytes, release) {
    if (entry.operation === "listFonts") {
      try { settle(id, { kind: "ok", fonts: parseFontList(bytes) }); }
      catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
      release();
      return;
    }
    const result = { kind: "ok", bytes, shared: release !== noop, release };
    if (entry.copy && release !== noop) {
      result.bytes = bytes.slice();
      release();
      result.release = noop;
    }
    settle(id, result);
  }

  function noop() {}

  function receive(event) {
    const data = event?.data;
    if (data === LAUNCH_NOTICE) {
      // US-115 : le natif signale des chemins en attente ; le front les réclame par takeLaunch().
      for (const listener of [...launchListeners]) { try { listener(); } catch { /* un abonné défaillant ne bloque pas les autres */ } }
      return;
    }
    const parsed = parseReply(data);
    if (!parsed) return;
    const { id, status, payload } = parsed;
    const entry = pending.get(id);
    if (!entry) return;
    if (status === "cancelled" && payload === "") settle(id, { kind: "cancelled" });
    else if (status === "error" && /^[A-Za-z][A-Za-z0-9_.]{0,63}$/.test(payload)) settle(id, { kind: "error", key: payload });
    else if (status === "ok" && ["open", "openDocx"].includes(entry.operation)) {
      try { settle(id, { kind: "ok", bytes: fromBase64(payload, entry.operation === "openDocx" ? MAX_DOCX_BYTES : MAX_FILE_BYTES) }); }
      catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && entry.operation === "launch" && payload !== "") {
      const match = /^(txt|docx);([0-9]{1,2});([A-Za-z0-9+/=]*)$/.exec(payload);
      try {
        if (!match || Number(match[2]) > MAX_LAUNCH_SKIPPED) throw new NativeHostError("HOST_INVALID_RESPONSE");
        settle(id, { kind: "ok", document: { type: match[1], skipped: Number(match[2]), bytes: fromBase64(match[3], match[1] === "docx" ? MAX_DOCX_BYTES : MAX_FILE_BYTES) } });
      } catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && entry.operation === "pickOpen") {
      const match = /^(r[1-9][0-9]{0,19});([a-z]{2,5});([0-9]{1,10});([A-Za-z0-9+/=]*)$/.exec(payload);
      try {
        if (!match || !DIALOG_FORMATS.includes(match[2])) throw new NativeHostError("HOST_INVALID_RESPONSE");
        settle(id, { kind: "ok", ticket: match[1], format: match[2], size: Number(match[3]), name: decodeName(match[4]) });
      } catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && entry.operation === "pickSave") {
      const match = /^(w[1-9][0-9]{0,19});([a-z]{2,5});([A-Za-z0-9+/=]*)$/.exec(payload);
      try {
        if (!match || !DIALOG_FORMATS.includes(match[2])) throw new NativeHostError("HOST_INVALID_RESPONSE");
        settle(id, { kind: "ok", ticket: match[1], format: match[2], name: decodeName(match[3]) });
      } catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && ["readTicket", "readFont", "listFonts"].includes(entry.operation)) {
      try { finishData(id, entry, inlineBytes(payload, entry.operation === "listFonts" ? MAX_FONT_LIST_BYTES : MAX_DOCX_BYTES), noop); }
      catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && payload === "" && ["save", "dirty", "launch", "writeTicket", "releaseTicket", "openUrl"].includes(entry.operation)) {
      settle(id, { kind: "ok" });
    } else settle(id, { kind: "error", key: "HOST_INVALID_RESPONSE" });
  }

  /** US-113 : le natif a posté un SharedBuffer en lecture seule ; `additionalData.reply` porte la réponse corrélée. */
  function receiveBuffer(event) {
    let buffer;
    try { buffer = event?.getBuffer?.(); } catch { buffer = undefined; }
    if (!(buffer instanceof ArrayBuffer)) return;
    heldBuffers.add(buffer);
    const drop = () => releaseBuffer(buffer);
    const extra = event.additionalData;
    const parsed = extra && typeof extra === "object" && extra.v === 1 ? parseReply(extra.reply) : null;
    const entry = parsed ? pending.get(parsed.id) : undefined;
    if (!parsed || !entry || parsed.status !== "ok" || !SHARED_OPERATIONS.has(entry.operation)) { drop(); return; }
    stats.sharedReceived += 1;
    const { id, payload } = parsed;
    if (entry.operation === "launch") {
      const match = /^(txt|docx);([0-9]{1,2});@sb$/.exec(payload);
      if (!match || Number(match[2]) > MAX_LAUNCH_SKIPPED || buffer.byteLength > MAX_DOCX_BYTES) {
        drop(); settle(id, { kind: "error", key: "HOST_INVALID_RESPONSE" }); return;
      }
      // Le document d'ouverture est copié : les appelants le conservent, le tampon est libéré aussitôt.
      const bytes = new Uint8Array(buffer).slice();
      drop();
      settle(id, { kind: "ok", document: { type: match[1], skipped: Number(match[2]), bytes } });
      return;
    }
    const match = /^sb;([0-9]{1,10})$/.exec(payload);
    if (!match || Number(match[1]) !== buffer.byteLength || buffer.byteLength === 0 || buffer.byteLength > MAX_SHARED_BYTES) {
      drop(); settle(id, { kind: "error", key: "HOST_INVALID_RESPONSE" }); return;
    }
    let released = false;
    const release = () => { if (!released) { released = true; drop(); } };
    finishData(id, entry, new Uint8Array(buffer), release);
  }

  function parseFontList(bytes) {
    let text;
    try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { throw new NativeHostError("HOST_INVALID_RESPONSE"); }
    const fonts = [];
    for (const line of text.split("\n")) {
      if (line === "") continue;
      const fields = line.split("\t");
      if (fields.length !== 5 || !/^f[0-9]{1,20}-[0-9]{1,6}$/.test(fields[0])
          || !/^[0-9]{1,3}$/.test(fields[3]) || !/^[0-9]{1,12}$/.test(fields[4])) throw new NativeHostError("HOST_INVALID_RESPONSE");
      fonts.push({ id: fields[0], family: fields[1], style: fields[2], face: Number(fields[3]), size: Number(fields[4]) });
    }
    return fonts;
  }

  function settle(id, result) {
    const entry = pending.get(id);
    if (!entry) {
      // Réponse tardive portant un tampon : ne rien retenir.
      if (result?.release) result.release();
      return;
    }
    pending.delete(id);
    clearTimeout(entry.timer);
    queuedBytes -= entry.bytes;
    entry.resolve(result);
  }

  function request(operation, payload = "", bytes = 0, options = {}) {
    if (closed) return Promise.reject(new NativeHostError("HOST_CLOSED"));
    if (operation === "open") bytes = MAX_FILE_BYTES;
    if (operation === "openDocx") bytes = MAX_DOCX_BYTES;
    if (pending.size >= MAX_QUEUE || queuedBytes + bytes > MAX_QUEUED_BYTES) return Promise.reject(new NativeHostError("HOST_QUEUE_LIMIT"));
    if (nextId > MAX_U64) return Promise.reject(new NativeHostError("HOST_ID_EXHAUSTED"));
    const id = nextId++;
    return new Promise(resolve => {
      const timer = setTimeout(() => settle(id, { kind: "error", key: "HOST_TIMEOUT" }), timeoutMs);
      pending.set(id, { resolve, timer, operation, bytes, copy: options.copy === true });
      queuedBytes += bytes;
      try { webview.postMessage(`${PREFIX}${id}|${operation}|${payload}`); }
      catch { settle(id, { kind: "error", key: "HOST_UNAVAILABLE" }); }
    });
  }

  function notifyDirty(dirty) {
    if (closed) return Promise.reject(new NativeHostError("HOST_CLOSED"));
    if (typeof dirty !== "boolean") return Promise.reject(new TypeError("dirty must be boolean."));
    if (nextId > MAX_U64) return Promise.reject(new NativeHostError("HOST_ID_EXHAUSTED"));
    const id = nextId++;
    try { webview.postMessage(`${PREFIX}${id}|dirty|${dirty ? "1" : "0"}`); }
    catch { return Promise.reject(new NativeHostError("HOST_UNAVAILABLE")); }
    return Promise.resolve({ kind: "ok" });
  }

  const invalid = key => Promise.resolve({ kind: "error", key });

  function pick(operation, options) {
    const save = operation === "pickSave";
    const spec = buildDialogSpec(options ?? {}, save);
    if (spec === null) return invalid("files.error.invalid_request");
    return request(operation, toBase64(new TextEncoder().encode(spec)));
  }

  async function writeTicket(ticket, bytes) {
    if (typeof ticket !== "string" || !/^w[1-9][0-9]{0,19}$/.test(ticket)) return { kind: "error", key: "files.error.unknown_ticket" };
    if (!(bytes instanceof Uint8Array)) throw new TypeError("bytes must be a Uint8Array.");
    if (bytes.byteLength > MAX_WRITE_BYTES) return { kind: "error", key: "files.error.too_large" };
    let seq = 0;
    let offset = 0;
    for (;;) {
      const end = Math.min(offset + WRITE_CHUNK_BYTES, bytes.byteLength);
      const last = end >= bytes.byteLength;
      const result = await request("writeTicket", `${ticket};${seq};${last ? 1 : 0};${toBase64(bytes.subarray(offset, end))}`, end - offset);
      if (result.kind !== "ok" || last) return result;
      seq += 1;
      offset = end;
    }
  }

  webview.addEventListener("message", receive);
  if (sharedSupported) webview.addEventListener("sharedbufferreceived", receiveBuffer);
  return Object.freeze({
    open() { return request("open"); },
    openDocx() { return request("openDocx"); },
    save(bytes) {
      if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_FILE_BYTES) return Promise.reject(new NativeHostError("HOST_FILE_TOO_LARGE"));
      return request("save", toBase64(bytes), bytes.byteLength);
    },
    /** US-115 : prochain document demandé par la ligne de commande ou une seconde instance ; `{ kind: "ok" }` sans `document` si rien n'attend. */
    takeLaunch() { return request("launch"); },
    /** Appelle `callback` quand des chemins arrivent ; renvoie la fonction de désabonnement. */
    onLaunch(callback) {
      if (typeof callback !== "function") throw new TypeError("callback must be a function.");
      launchListeners.add(callback);
      return () => { launchListeners.delete(callback); };
    },
    notifyDirty,
    /**
     * US-113 : dialogue d'ouverture natif. `{ title, filters: [{ format, label }] }` ; résultat
     * `{ kind: "ok", ticket, format, size, name }` (ticket de lecture opaque, jamais de chemin).
     */
    pickOpen(options) { return pick("pickOpen", options); },
    /** US-113 : dialogue d'enregistrement natif ; `{ kind: "ok", ticket, format, name }` avec un ticket d'écriture opaque. */
    pickSave(options) { return pick("pickSave", options); },
    /**
     * US-113 : lit (une fois) le fichier d'un ticket `r…`. Au-delà du seuil natif, `bytes` est une vue en
     * lecture seule d'un SharedBuffer : appeler `result.release()` quand les octets ne servent plus
     * (`{ copy: true }` copie et libère aussitôt). `result.shared` indique le mode de transfert.
     */
    readTicket(ticket, { copy = false } = {}) {
      if (typeof ticket !== "string" || !/^r[1-9][0-9]{0,19}$/.test(ticket)) return invalid("files.error.unknown_ticket");
      return request("readTicket", ticket, 0, { copy });
    },
    /** US-113 : écriture atomique par morceaux sur un ticket `w…` ; le fichier n'apparaît qu'au dernier morceau. */
    writeTicket,
    /** US-113 : abandonne un ticket `r…` ou `w…` sans l'utiliser. */
    releaseTicket(ticket) {
      if (typeof ticket !== "string" || !/^[rw][1-9][0-9]{0,19}$/.test(ticket)) return invalid("files.error.unknown_ticket");
      return request("releaseTicket", ticket);
    },
    /** US-113 : familles installées ; `{ kind: "ok", fonts: [{ id, family, style, face, size }] }`. */
    listFonts() { return request("listFonts"); },
    /** US-113 : octets du fichier d'une police par identifiant opaque (mêmes règles que `readTicket`). */
    readFont(fontId, { copy = false } = {}) {
      if (typeof fontId !== "string" || !/^f[0-9]{1,20}-[0-9]{1,6}$/.test(fontId)) return invalid("fonts.error.unknown_id");
      return request("readFont", fontId, 0, { copy });
    },
    /** US-113 : ouvre une URL http(s) dans le navigateur par défaut ; refus local sans aller-retour natif. */
    openUrl(url) {
      const key = validateUrl(url);
      if (key !== null) return invalid(key);
      return request("openUrl", toBase64(new TextEncoder().encode(url)));
    },
    /** Compteurs de diagnostic (tampons reçus/libérés, réponses en ligne, replis Base64). */
    stats() { return { ...stats, held: heldBuffers.size, sharedBufferSupported: sharedSupported }; },
    close() {
      if (closed) return;
      closed = true;
      launchListeners.clear();
      webview.removeEventListener("message", receive);
      if (sharedSupported) webview.removeEventListener("sharedbufferreceived", receiveBuffer);
      for (const id of [...pending.keys()]) settle(id, { kind: "error", key: "HOST_CLOSED" });
      for (const buffer of [...heldBuffers]) releaseBuffer(buffer);
    },
  });
}
