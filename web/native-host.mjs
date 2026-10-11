// SPDX-License-Identifier: AGPL-3.0-or-later

const MAX_U64 = (1n << 64n) - 1n;
const MAX_FILE_BYTES = 131075;
const MAX_QUEUE = 4;
const MAX_QUEUED_BYTES = MAX_FILE_BYTES * MAX_QUEUE;
const PREFIX = "us120-file|1|";

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

function fromBase64(value) {
  if (typeof value !== "string" || value.length % 4 !== 0
      || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    throw new NativeHostError("HOST_INVALID_RESPONSE");
  }
  let binary;
  try { binary = atob(value); } catch { throw new NativeHostError("HOST_INVALID_RESPONSE"); }
  if (binary.length > MAX_FILE_BYTES) throw new NativeHostError("HOST_FILE_TOO_LARGE");
  const bytes = Uint8Array.from(binary, char => char.charCodeAt(0));
  if (toBase64(bytes) !== value) throw new NativeHostError("HOST_INVALID_RESPONSE");
  return bytes;
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

  function receive(event) {
    const data = event?.data;
    if (typeof data !== "string" || !data.startsWith(PREFIX)) return;
    const fields = data.split("|");
    if (fields.length !== 5 || fields[0] !== "us120-file" || fields[1] !== "1"
        || !/^[1-9][0-9]{0,19}$/.test(fields[2])) return;
    let id;
    try { id = BigInt(fields[2]); } catch { return; }
    if (id > MAX_U64) return;
    const entry = pending.get(id);
    if (!entry) return;
    const [, , , status, payload] = fields;
    if (status === "cancelled" && payload === "") settle(id, { kind: "cancelled" });
    else if (status === "error" && /^[A-Za-z][A-Za-z0-9_.]{0,63}$/.test(payload)) settle(id, { kind: "error", key: payload });
    else if (status === "ok" && entry.operation === "open") {
      try { settle(id, { kind: "ok", bytes: fromBase64(payload) }); }
      catch (error) { settle(id, { kind: "error", key: error.code ?? "HOST_INVALID_RESPONSE" }); }
    } else if (status === "ok" && payload === "") settle(id, { kind: "ok" });
    else settle(id, { kind: "error", key: "HOST_INVALID_RESPONSE" });
  }

  function settle(id, result) {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    clearTimeout(entry.timer);
    queuedBytes -= entry.bytes;
    entry.resolve(result);
  }

  function request(operation, payload = "", bytes = 0) {
    if (closed) return Promise.reject(new NativeHostError("HOST_CLOSED"));
    if (pending.size >= MAX_QUEUE || queuedBytes + bytes > MAX_QUEUED_BYTES) return Promise.reject(new NativeHostError("HOST_QUEUE_LIMIT"));
    if (nextId > MAX_U64) return Promise.reject(new NativeHostError("HOST_ID_EXHAUSTED"));
    const id = nextId++;
    if (operation === "open") bytes = MAX_FILE_BYTES;
    return new Promise(resolve => {
      const timer = setTimeout(() => settle(id, { kind: "error", key: "HOST_TIMEOUT" }), timeoutMs);
      pending.set(id, { resolve, timer, operation, bytes });
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

  webview.addEventListener("message", receive);
  return Object.freeze({
    open() { return request("open"); },
    save(bytes) {
      if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_FILE_BYTES) return Promise.reject(new NativeHostError("HOST_FILE_TOO_LARGE"));
      return request("save", toBase64(bytes), bytes.byteLength);
    },
    notifyDirty,
    close() {
      if (closed) return;
      closed = true;
      webview.removeEventListener("message", receive);
      for (const id of [...pending.keys()]) settle(id, { kind: "error", key: "HOST_CLOSED" });
    },
  });
}
