// SPDX-License-Identifier: AGPL-3.0-or-later
import { createWasmEngine } from "./wasm-loader.mjs";
import {
  decodeMessage, decodeStagedProgress, encodeStagedCancel, encodeStagedStep, MessageType, stagedOpcodeOf,
} from "./wasm-protocol.mjs";

export const MAX_QUEUED_REQUESTS = 32;
export const MAX_QUEUED_BYTES = 128 * 1024 * 1024;

function serializeError(error) {
  return { name: error?.name ?? "Error", message: String(error?.message ?? error), code: error?.code };
}

function yieldToTaskQueue() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function asAbortError() {
  return Object.assign(new Error("The operation was aborted."), { name: "AbortError" });
}

/** Install the production protocol runtime on a Worker-like message scope. */
export function installEngineWorker(scope) {
  let engine;
  let closed = false;
  let initializing = false;
  let active;
  const queue = [];
  const jobsById = new Map();
  let queuedBytes = 0;
  const send = (message, transfer = []) => scope.postMessage(message, transfer);

  function settleCancelled(job) {
    send({ kind: "cancelled", requestId: job.requestId });
  }

  /**
   * Drives a staged Begin frame: the engine does one bounded step per call, and
   * the Worker yields to its task queue after each so a pending cancel message
   * is seen before the next step. An abort or failure frees the engine job.
   */
  async function runStaged(job, options) {
    // Byte counters restart with every dispatch: report the Begin input copy and only the large final result's output copy.
    let reply = await engine.dispatchAsync(job.frame, { ...options, onProgress: (progress) => { if (progress.phase === "input") options.onProgress(progress); } });
    let jobId;
    let finished = false;
    try {
      for (;;) {
        const response = decodeMessage(reply, "event");
        const progress = response.type === MessageType.EditorResult ? decodeStagedProgress(response.payload) : undefined;
        if (!progress) { finished = true; return reply; }
        if (progress.state !== 0 || (jobId !== undefined && progress.jobId !== jobId)) {
          throw Object.assign(new Error("WASM staged job reported an unexpected state."), { name: "WorkerProtocolError", code: "STAGED_STATE" });
        }
        jobId = progress.jobId;
        send({ kind: "progress", requestId: job.requestId, phase: "step", copiedBytes: progress.done, totalBytes: progress.total, jobId });
        await yieldToTaskQueue();
        if (options.signal.aborted) throw asAbortError();
        reply = await engine.dispatchAsync(encodeStagedStep(job.requestId, jobId), {
          signal: options.signal,
          chunkBytes: options.chunkBytes,
          onProgress: (progress) => { if (progress.phase === "output" && progress.totalBytes > 256) options.onProgress(progress); },
        });
      }
    } finally {
      if (!finished) {
        // Id 0 drops whatever job is active: an abort during Begin happens before the id is read.
        try { await engine.dispatchAsync(encodeStagedCancel(job.requestId, jobId ?? 0n)); } catch { /* the engine drops the job on its next Begin */ }
      }
    }
  }

  async function pump() {
    if (active || closed || !engine) return;
    const job = queue.shift();
    if (!job) return;
    queuedBytes -= job.frame.byteLength;
    active = job;
    try {
      if (job.controller.signal.aborted) throw asAbortError();
      const dispatchOptions = {
        signal: job.controller.signal,
        chunkBytes: job.chunkBytes,
        onProgress(progress) {
          try { send({ kind: "progress", requestId: job.requestId, ...progress }); } catch { /* progress delivery must not stop dispatch */ }
        },
      };
      const bytes = job.staged ? await runStaged(job, dispatchOptions) : await engine.dispatchAsync(job.frame, dispatchOptions);
      if (job.controller.signal.aborted) throw asAbortError();
      if (!(bytes instanceof Uint8Array) || bytes.byteLength > 64 * 1024 * 1024 - 4) {
        throw Object.assign(new Error("WASM dispatch returned an invalid or oversized frame."), { name: "WorkerProtocolError", code: "INVALID_RESPONSE" });
      }
      if (job.decoded.type === MessageType.Editor) {
        const response = decodeMessage(bytes, "event");
        if ((response.type !== MessageType.EditorResult && response.type !== MessageType.Failed)
            || response.requestId !== job.requestId) {
          throw Object.assign(new Error("WASM Editor response does not match its request."), { name: "WorkerProtocolError", code: "EDITOR_RESPONSE_MISMATCH" });
        }
      } else {
        if (bytes.byteLength !== job.frame.byteLength) throw Object.assign(new Error("WASM dispatch changed the echoed command length."), { name: "WorkerProtocolError", code: "ECHO_MISMATCH" });
        for (let index = 0; index < bytes.byteLength; index += 1) {
          if (bytes[index] !== job.frame[index]) throw Object.assign(new Error("WASM dispatch changed the echoed command bytes."), { name: "WorkerProtocolError", code: "ECHO_MISMATCH" });
        }
      }
      send({ kind: "result", requestId: job.requestId, bytes }, [bytes.buffer]);
    } catch (error) {
      if (error?.name === "AbortError") settleCancelled(job);
      else send({ kind: "error", requestId: job.requestId, error: serializeError(error) });
    } finally {
      jobsById.delete(job.requestId);
      active = undefined;
      void pump();
    }
  }

  async function receive(message) {
    if (closed || !message || typeof message !== "object") return;
    if (message.kind === "init") {
      if (engine || initializing) {
        send({ kind: "fatal", error: serializeError(new Error("Worker is already initialized.")) });
        return;
      }
      initializing = true;
      try {
        const source = message.module ?? message.bytes;
        engine = await createWasmEngine(source);
        if (!closed) send({ kind: "ready" });
        void pump();
      } catch (error) {
        send({ kind: "fatal", error: serializeError(error) });
      } finally {
        initializing = false;
      }
      return;
    }
    if (message.kind === "close") {
      closed = true;
      active?.controller.abort();
      for (const job of queue.splice(0)) {
        jobsById.delete(job.requestId);
        settleCancelled(job);
      }
      queuedBytes = 0;
      return;
    }
    if (message.kind === "cancel") {
      let cancel;
      try {
        const frame = message.frame;
        if (!(frame instanceof Uint8Array)) throw new TypeError("Cancel frame must be a Uint8Array.");
        cancel = decodeMessage(frame, "command");
        if (cancel.type !== MessageType.Cancel || cancel.requestId !== message.requestId) throw new TypeError("Cancel envelope does not match its v1 frame.");
      } catch (error) {
        send({ kind: "fatal", error: serializeError(error) });
        return;
      }
      if (active?.requestId === cancel.requestId) active.controller.abort();
      const index = queue.findIndex((job) => job.requestId === cancel.requestId);
      if (index >= 0) {
        const [job] = queue.splice(index, 1);
        queuedBytes -= job.frame.byteLength;
        jobsById.delete(job.requestId);
        settleCancelled(job);
      }
      return;
    }
    if (message.kind !== "request") {
      send({ kind: "fatal", error: serializeError(new TypeError("Unknown worker control message.")) });
      return;
    }
    if (!engine) {
      send({ kind: "error", requestId: message.requestId, error: serializeError(new Error("Worker is not initialized.")) });
      return;
    }
    try {
      if (!(message.frame instanceof Uint8Array)) throw new TypeError("Request frame must be a Uint8Array.");
      if (message.frame.byteLength > 64 * 1024 * 1024 - 4) throw new TypeError("Request frame exceeds the protocol limit.");
      if (queue.length >= MAX_QUEUED_REQUESTS || queuedBytes + message.frame.byteLength > MAX_QUEUED_BYTES) {
        send({ kind: "error", requestId: message.requestId, error: serializeError(Object.assign(new Error("Worker request queue limit exceeded."), { name: "QueueLimitError", code: "QUEUE_LIMIT" })) });
        return;
      }
      const decoded = decodeMessage(message.frame, "command");
      if (decoded.type === MessageType.Cancel || decoded.requestId !== message.requestId) throw new TypeError("Request envelope does not match its v1 command frame.");
      if (jobsById.has(decoded.requestId)) {
        send({ kind: "error", requestId: decoded.requestId, error: serializeError(Object.assign(new Error("Request ID is already active."), { name: "DuplicateRequestIdError", code: "DUPLICATE_REQUEST_ID" })) });
        return;
      }
      const job = { requestId: decoded.requestId, decoded, frame: message.frame, controller: new AbortController(), chunkBytes: message.chunkBytes, staged: stagedOpcodeOf(decoded) === 23 };
      queue.push(job);
      jobsById.set(job.requestId, job);
      queuedBytes += message.frame.byteLength;
      void pump();
    } catch (error) {
      send({ kind: "error", requestId: message.requestId, error: serializeError(error) });
    }
  }

  const listener = (event) => { void receive(event.data); };
  scope.addEventListener("message", listener);
  return Object.freeze({ close() { closed = true; scope.removeEventListener("message", listener); active?.controller.abort(); } });
}

if (typeof WorkerGlobalScope !== "undefined" && globalThis instanceof WorkerGlobalScope) {
  installEngineWorker(globalThis);
}
