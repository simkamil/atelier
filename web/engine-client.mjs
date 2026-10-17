// SPDX-License-Identifier: AGPL-3.0-or-later
import { decodeMessage, encodeMessage, HEADER_BYTES, MessageType, MAX_MESSAGE_BYTES } from "./wasm-protocol.mjs";

export const MAX_CLIENT_REQUESTS = 32;
export const MAX_CLIENT_BYTES = 128 * 1024 * 1024;

export class WorkerClientError extends Error {
  constructor(message, name = "WorkerClientError", code = undefined) {
    super(message);
    this.name = name;
    if (code !== undefined) this.code = code;
  }
}

function abortError() { return new DOMException("The operation was aborted.", "AbortError"); }

function reviveError(value) {
  return new WorkerClientError(value?.message ?? "Worker request failed.", value?.name ?? "Error", value?.code);
}

function normalizeSource(module, bytes) {
  if ((module === undefined) === (bytes === undefined)) throw new TypeError("Provide exactly one of module or bytes.");
  if (module !== undefined) {
    if (!(module instanceof WebAssembly.Module)) throw new TypeError("module must be a WebAssembly.Module.");
    return { module };
  }
  if (!(bytes instanceof ArrayBuffer) && !ArrayBuffer.isView(bytes)) throw new TypeError("bytes must be an ArrayBuffer or view.");
  return { bytes: Uint8Array.from(new Uint8Array(bytes.buffer ?? bytes, bytes.byteOffset ?? 0, bytes.byteLength)).buffer };
}

/** Create an initialized FIFO client for the production WASM Worker. */
export async function createEngineClient({ worker, workerUrl, module, bytes } = {}) {
  const source = normalizeSource(module, bytes);
  if (worker !== undefined && (!worker || typeof worker.postMessage !== "function")) throw new TypeError("worker must provide postMessage().");
  const actualWorker = worker ?? new Worker(workerUrl ?? new URL("./engine-worker.mjs", import.meta.url), { type: "module" });
  if (typeof actualWorker.addEventListener !== "function" || typeof actualWorker.removeEventListener !== "function") {
    throw new TypeError("worker must support addEventListener/removeEventListener.");
  }

  let closed = false;
  let fatalError;
  let outstandingBytes = 0;
  const pending = new Map();
  let readyResolve;
  let readyReject;
  let initializationTimer;
  const ready = new Promise((resolve, reject) => { readyResolve = resolve; readyReject = reject; });

  function detachWorker() {
    actualWorker.removeEventListener("message", onMessage);
    actualWorker.removeEventListener("error", onError);
    actualWorker.removeEventListener("messageerror", onMessageError);
    try { actualWorker.postMessage({ kind: "close" }); } catch { /* Worker may already have stopped */ }
    actualWorker.terminate?.();
  }

  function removePending(entry) {
    pending.delete(entry.requestId);
    outstandingBytes -= entry.frame.byteLength;
    if (entry.abortListener) entry.signal.removeEventListener("abort", entry.abortListener);
  }

  function failAll(error) {
    if (fatalError || closed) return;
    fatalError = error;
    clearTimeout(initializationTimer);
    readyReject(error);
    for (const entry of pending.values()) {
      removePending(entry);
      entry.reject(error);
    }
    detachWorker();
  }

  function onMessage(event) {
    const message = event.data;
    if (!message || typeof message !== "object" || typeof message.kind !== "string") {
      failAll(new WorkerClientError("Worker sent a malformed control response.", "WorkerProtocolError"));
      return;
    }
    if (message.kind === "ready") { clearTimeout(initializationTimer); readyResolve(); return; }
    if (message.kind === "fatal") { failAll(reviveError(message.error)); return; }
    if (message.kind === "closed") return;
    const requestId = message.requestId;
    if (typeof requestId !== "bigint") {
      failAll(new WorkerClientError("Worker response has no valid request ID.", "WorkerProtocolError"));
      return;
    }
    const entry = pending.get(requestId);
    if (!entry) {
      failAll(new WorkerClientError(`Worker responded for unknown request ${requestId}.`, "WorkerProtocolError"));
      return;
    }
    if (message.kind === "progress") {
      if (!(["input", "output", "complete", "step"].includes(message.phase)
          && Number.isSafeInteger(message.copiedBytes) && message.copiedBytes >= (entry.lastProgress.get(message.phase) ?? 0)
          && Number.isSafeInteger(message.totalBytes) && message.totalBytes >= message.copiedBytes
          && message.copiedBytes <= MAX_MESSAGE_BYTES * 2)) {
        failAll(new WorkerClientError("Worker sent incompatible progress counters.", "WorkerProtocolError"));
        return;
      }
      entry.lastProgress.set(message.phase, message.copiedBytes);
      if (entry.onProgress) {
        try { entry.onProgress(Object.freeze({ phase: message.phase, copiedBytes: message.copiedBytes, totalBytes: message.totalBytes, ...(message.phase === "step" && typeof message.jobId === "bigint" ? { jobId: message.jobId } : {}) })); } catch { /* isolate user callbacks */ }
      }
      return;
    }
    if (message.kind === "cancelled") {
      removePending(entry);
      entry.reject(abortError());
      return;
    }
    if (message.kind === "error") {
      removePending(entry);
      entry.reject(entry.cancelRequested ? abortError() : reviveError(message.error));
      return;
    }
    if (message.kind !== "result" || !(message.bytes instanceof Uint8Array)) {
      failAll(new WorkerClientError("Worker returned an incompatible response.", "WorkerProtocolError"));
      return;
    }
    if (message.bytes.byteLength > MAX_MESSAGE_BYTES || message.bytes.byteLength < HEADER_BYTES) {
      failAll(new WorkerClientError("Worker response is outside the protocol size limit.", "WorkerProtocolError"));
      return;
    }
    let reply;
    try {
      reply = decodeMessage(message.bytes, entry.decoded.type === MessageType.Editor ? "event" : "command");
    } catch (error) {
      failAll(new WorkerClientError(error.message, "WorkerProtocolError", error.code));
      return;
    }
    const validType = entry.decoded.type === MessageType.Editor
      ? reply.type === MessageType.EditorResult || reply.type === MessageType.Failed
      : reply.type === entry.decoded.type && message.bytes.byteLength === entry.frame.byteLength;
    if (!validType || reply.requestId !== requestId) {
      failAll(new WorkerClientError("Worker response header does not match the submitted command.", "WorkerProtocolError"));
      return;
    }
    removePending(entry);
    if (entry.cancelRequested) entry.reject(abortError());
    else entry.resolve(message.bytes);
  }

  const onError = (event) => failAll(new WorkerClientError(event?.message ?? "Worker failed.", "WorkerFatalError"));
  const onMessageError = () => failAll(new WorkerClientError("Worker response could not be deserialized.", "WorkerFatalError"));
  actualWorker.addEventListener("message", onMessage);
  actualWorker.addEventListener("error", onError);
  actualWorker.addEventListener("messageerror", onMessageError);
  initializationTimer = setTimeout(
    () => failAll(new WorkerClientError("Worker initialization timed out after 30 seconds.", "WorkerInitTimeoutError", "INIT_TIMEOUT")),
    30_000,
  );
  try {
    actualWorker.postMessage({ kind: "init", ...source });
  } catch (error) {
    failAll(new WorkerClientError(error.message, "WorkerInitError"));
  }
  try { await ready; } catch (error) { actualWorker.terminate?.(); throw error; }

  function assertUsable() {
    if (closed) throw new WorkerClientError("Worker client is closed.", "WorkerClosedError");
    if (fatalError) throw fatalError;
  }

  return Object.freeze({
    request(frame, { signal, onProgress, chunkBytes } = {}) {
      assertUsable();
      if (!(frame instanceof Uint8Array)) throw new TypeError("request expects a Uint8Array command frame.");
      if (frame.byteLength > MAX_MESSAGE_BYTES) throw new WorkerClientError("Request exceeds the protocol message limit.", "QueueLimitError", "MESSAGE_TOO_LARGE");
      if (signal?.aborted) return Promise.reject(abortError());
      if (signal !== undefined && (signal === null
          || typeof signal.addEventListener !== "function"
          || typeof signal.removeEventListener !== "function")) throw new TypeError("signal must be an AbortSignal.");
      if (onProgress !== undefined && typeof onProgress !== "function") throw new TypeError("onProgress must be a function.");
      if (chunkBytes !== undefined && (!Number.isSafeInteger(chunkBytes) || chunkBytes < 16 * 1024 || chunkBytes > 16 * 1024 * 1024)) throw new TypeError("chunkBytes must be an integer between 16384 and 16777216.");
      if (pending.size >= MAX_CLIENT_REQUESTS || outstandingBytes + frame.byteLength > MAX_CLIENT_BYTES) {
        throw new WorkerClientError("Worker client request queue limit exceeded.", "QueueLimitError", "QUEUE_LIMIT");
      }
      const ownedFrame = Uint8Array.from(frame);
      const decoded = decodeMessage(ownedFrame, "command");
      if (decoded.type === MessageType.Cancel) throw new TypeError("Cancel frames are reserved for AbortSignal cancellation.");
      if (pending.has(decoded.requestId)) throw new WorkerClientError(`Request ID ${decoded.requestId} is already active.`, "DuplicateRequestIdError", "DUPLICATE_REQUEST_ID");
      return new Promise((resolve, reject) => {
        const entry = { requestId: decoded.requestId, decoded, frame: ownedFrame, resolve, reject, signal, onProgress, lastProgress: new Map(), cancelRequested: false };
        pending.set(entry.requestId, entry);
        outstandingBytes += ownedFrame.byteLength;
        if (signal) {
          entry.abortListener = () => {
            if (entry.cancelRequested || !pending.has(entry.requestId)) return;
            entry.cancelRequested = true;
            const cancelFrame = encodeMessage({ type: MessageType.Cancel, requestId: entry.requestId, payload: new Uint8Array() });
            try { actualWorker.postMessage({ kind: "cancel", requestId: entry.requestId, frame: cancelFrame }, [cancelFrame.buffer]); }
            catch (error) { failAll(new WorkerClientError(error.message, "WorkerFatalError")); }
          };
          signal.addEventListener("abort", entry.abortListener, { once: true });
        }
        try { actualWorker.postMessage({ kind: "request", requestId: entry.requestId, frame: ownedFrame, chunkBytes }); }
        catch (error) { removePending(entry); reject(new WorkerClientError(error.message, "WorkerSendError")); }
      });
    },
    close() {
      if (closed) return;
      closed = true;
      const error = new WorkerClientError("Worker client was closed.", "WorkerClosedError");
      for (const entry of pending.values()) { removePending(entry); entry.reject(error); }
      readyReject(error);
      detachWorker();
    },
  });
}
