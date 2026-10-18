// SPDX-License-Identifier: AGPL-3.0-or-later
// ATED v2 rich-editor client (US-203, R4) on top of an injectable transport with
// the shape of web/engine-client.mjs: `transport.request(frame, {signal, onProgress})`
// resolves with the raw v2 reply frame (the outer US-091 wrapping belongs to the
// transport). Commands run strictly one at a time in submission order; the
// layout pump and the fetch of visible pages only run when the queue is empty,
// so user commands always go first. No DOM access, no editor integration (R7).
import { decodeRichReply, encodeRichRequest, RichProtocolError } from "./rich-editor-protocol.mjs";
import { EditorPageCache, RichClientError } from "./editor-page-cache.mjs";

export { EditorPageCache, RichClientError };

const REPLY_TYPE = {
  open: "state", insertText: "state", replaceRange: "state", delete: "state", move: "state", setSelection: "state",
  undo: "state", redo: "state", markSaved: "state", layout: "state", textWindow: "textWindow", pages: "pages", close: "closed",
};

function abortError() { return new DOMException("The operation was aborted.", "AbortError"); }
const isAbort = (error) => error?.name === "AbortError";
const defaultYield = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Abort controller that follows several optional signals; `release()` detaches the listeners. */
function linkSignals(signals) {
  const controller = new AbortController();
  const hooks = [];
  for (const signal of signals) {
    if (!signal) continue;
    if (signal.aborted) { controller.abort(); continue; }
    const hook = () => controller.abort();
    signal.addEventListener("abort", hook, { once: true });
    hooks.push([signal, hook]);
  }
  return { signal: controller.signal, release() { for (const [s, h] of hooks) s.removeEventListener("abort", h); } };
}

/**
 * @param {object} options
 * @param {{request: Function}} options.transport
 * @param {number} options.docId non-zero u32
 * @param {EditorPageCache} [options.cache]
 * @param {number} [options.layoutBudget] paragraphs per Layout command (1 to 4096)
 * @param {AbortSignal} [options.signal] aborting it disposes the client
 * @param {() => Promise<void>} [options.yieldTurn] gives control back between two pump steps
 * @param {(error: Error) => void} [options.onError] background failures (pump, visible page fetch)
 */
export function createRichEditorClient({
  transport, docId, cache = new EditorPageCache(), layoutBudget = 16, signal, yieldTurn = defaultYield,
  onError = () => {}, maxLayoutRetries = 4096,
} = {}) {
  if (!transport || typeof transport.request !== "function") throw new TypeError("transport must provide request().");
  if (!Number.isInteger(docId) || docId < 1 || docId > 0xffffffff) throw new TypeError("docId must be a non-zero u32.");
  if (!Number.isInteger(layoutBudget) || layoutBudget < 1 || layoutBudget > 4096) throw new TypeError("layoutBudget must be 1 to 4096.");
  if (typeof yieldTurn !== "function" || typeof onError !== "function") throw new TypeError("yieldTurn and onError must be functions.");

  let disposed = false;
  let opened = false;
  let nextRequestId = 1;
  let draining = false;
  let wake = false;
  let needsResync = false;
  let pumpHalt = null; // null | "stopped" | "error"
  let visible = null;
  let fillBlockedVersion = -1;
  const queue = [];
  const idleWaiters = [];
  const layoutWaiters = [];
  const lifetime = new AbortController();
  let pumpController = new AbortController();
  const clientLink = linkSignals([signal]);

  const closedError = () => new RichClientError("CLIENT_CLOSED", "Rich editor client is closed.");
  const report = (error) => { try { onError(error); } catch { /* isolate user callbacks */ } };

  function settleLayoutWaiters(error) {
    if (!error && !cache.layoutComplete) return;
    for (const waiter of layoutWaiters.splice(0)) (error ? waiter.reject(error) : waiter.resolve());
  }

  async function exchange(command, callerSignal) {
    if (disposed) throw closedError();
    const requestId = nextRequestId;
    nextRequestId = nextRequestId === 0xffffffff ? 1 : nextRequestId + 1;
    const frame = encodeRichRequest({ docId, requestId, command });
    const link = linkSignals([lifetime.signal, callerSignal]);
    let bytes;
    try {
      if (link.signal.aborted) throw abortError();
      bytes = await transport.request(frame, { signal: link.signal });
    } catch (error) {
      if (isAbort(error) || error instanceof RichClientError) throw error;
      throw new RichClientError("CLIENT_TRANSPORT", "Transport request failed.", error);
    } finally { link.release(); }
    if (disposed) throw closedError();
    let decoded;
    try {
      if (!(bytes instanceof Uint8Array)) throw new RichClientError("CLIENT_BAD_REPLY", "Transport returned no frame.");
      decoded = decodeRichReply(bytes);
    } catch (error) {
      if (error instanceof RichClientError) throw error;
      const code = error instanceof RichProtocolError ? error.code : "unknown";
      throw new RichClientError("CLIENT_BAD_REPLY", `Reply frame rejected: ${code}`, error);
    }
    if (decoded.docId !== docId || decoded.requestId !== requestId) {
      throw new RichClientError("CLIENT_MISMATCH", "Reply does not match the submitted request.");
    }
    const { response } = decoded;
    if (response.type === "error") throw new RichClientError(response.code, `Engine refused the command: ${response.code}`);
    const expected = command.type === "hitTest" ? (command.mode === "probe" ? "hitResult" : "state") : REPLY_TYPE[command.type];
    if (response.type !== expected) throw new RichClientError("CLIENT_BAD_REPLY", `Unexpected ${response.type} reply to ${command.type}.`);
    if (response.type === "closed") {
      opened = false; cache.reset(); pumpHalt = "stopped";
      settleLayoutWaiters(closedError());
      return { response, change: null };
    }
    const change = cache.apply(response);
    settleLayoutWaiters();
    return { response, change };
  }

  /** Command whose reply carries a State: when it is cancelled the lost invalidations force a resync. */
  async function guarded(command, callerSignal) {
    try { return await exchange(command, callerSignal); } catch (error) {
      // The engine may have applied the command whatever went wrong, except for its own RICH_* refusals.
      const refused = typeof error?.code === "string" && error.code.startsWith("RICH_");
      if (!refused && REPLY_TYPE[command.type] === "state" && !disposed && error?.code !== "CLIENT_CLOSED") { cache.dropPages(); needsResync = true; }
      throw error;
    }
  }

  async function runPagesUntilReady(first, end, callerSignal) {
    let notReady = 0;
    for (let guard = 0; guard < maxLayoutRetries + 64; guard += 1) {
      const run = cache.missing(first, end)[0];
      if (!run) return;
      try {
        await exchange({ type: "pages", first: run.first, count: run.count }, callerSignal);
      } catch (error) {
        if (error?.code !== "RICH_NOT_READY" || cache.layoutComplete || notReady >= maxLayoutRetries) throw error;
        notReady += 1;
        await guarded({ type: "layout", budget: layoutBudget }, callerSignal);
        await yieldTurn();
      }
    }
    throw new RichClientError("CLIENT_RETRY_LIMIT", "Pages stayed unavailable.");
  }

  function pumpWanted() { return opened && !pumpHalt && !cache.layoutComplete; }

  async function idleStep() {
    if (needsResync && opened) {
      needsResync = false;
      try { await guarded({ type: "layout", budget: 1 }, undefined); } catch (error) {
        if (disposed) return false;
        needsResync = true;
        report(error);
        return false;
      }
      return true;
    }
    if (visible && opened && fillBlockedVersion !== cache.version) {
      const run = cache.missing(visible.first, visible.end)[0];
      if (run) {
        const version = cache.version;
        try {
          const { change } = await exchange({ type: "pages", first: run.first, count: run.count }, undefined);
          if (change.ignored) fillBlockedVersion = version; // stale revision: wait for the next State
        } catch (error) {
          if (disposed) return false;
          if (isAbort(error)) { fillBlockedVersion = version; return true; }
          if (error?.code !== "RICH_NOT_READY" || cache.layoutComplete) report(error);
          fillBlockedVersion = version;
        }
        return true;
      }
    }
    if (pumpWanted()) {
      try {
        await guarded({ type: "layout", budget: layoutBudget }, pumpController.signal);
      } catch (error) {
        if (disposed) return false;
        if (isAbort(error)) { pumpHalt ??= "stopped"; return true; }
        pumpHalt = "error";
        settleLayoutWaiters(error);
        report(error);
        return true;
      }
      await yieldTurn();
      return true;
    }
    return false;
  }

  async function drain() {
    if (draining) return;
    draining = true;
    try {
      while (!disposed) {
        const task = queue.shift();
        if (task) {
          if (needsResync && opened) {
            needsResync = false;
            try { await exchange({ type: "layout", budget: 1 }, undefined); } catch (error) {
              if (disposed) break;
              needsResync = true;
              task.reject(error);
              continue;
            }
          }
          try {
            if (task.signal?.aborted) throw abortError();
            const result = await task.run();
            if (pumpHalt === "error") pumpHalt = null;
            task.resolve(result);
          } catch (error) { task.reject(error); }
          continue;
        }
        wake = false;
        if (!(await idleStep()) && queue.length === 0 && !wake) break;
      }
    } finally {
      draining = false;
      for (const resolve of idleWaiters.splice(0)) resolve();
      if (queue.length && !disposed) void drain();
    }
  }

  function kick() {
    wake = true; // a drain that is finishing re-checks it, so no request is lost
    if (!draining && !disposed) void drain();
  }

  function enqueue(run, taskSignal) {
    if (disposed) return Promise.reject(closedError());
    if (taskSignal !== undefined && taskSignal !== null && typeof taskSignal.addEventListener !== "function") {
      return Promise.reject(new TypeError("signal must be an AbortSignal."));
    }
    return new Promise((resolve, reject) => {
      queue.push({ run, resolve, reject, signal: taskSignal });
      kick();
    });
  }

  function dispose() {
    if (disposed) return;
    disposed = true;
    clientLink.release();
    lifetime.abort();
    pumpController.abort();
    const error = closedError();
    for (const task of queue.splice(0)) task.reject(error);
    settleLayoutWaiters(error);
    for (const resolve of idleWaiters.splice(0)) resolve();
    cache.reset();
  }
  if (signal) {
    if (signal.aborted) dispose();
    else signal.addEventListener("abort", dispose, { once: true });
  }

  return Object.freeze({
    cache,
    get opened() { return opened; },
    /** Open a document (replaces the active one) and start the layout pump. */
    open(spec, { signal: callerSignal } = {}) {
      return enqueue(async () => {
        cache.reset();
        opened = false;
        pumpHalt = null;
        fillBlockedVersion = -1;
        needsResync = false;
        pumpController = new AbortController();
        let result;
        try { result = await exchange({ type: "open", ...spec }, callerSignal); } catch (error) {
          settleLayoutWaiters(error);
          throw error;
        }
        opened = true;
        return result;
      }, callerSignal);
    },
    /** Send one command; resolves `{response, change}`, rejects with RichClientError for RICH_* refusals. */
    command(command, { signal: callerSignal } = {}) {
      if (command?.type === "open") return Promise.reject(new TypeError("Use open() to open a document."));
      return enqueue(() => guarded(command, callerSignal), callerSignal);
    },
    /** Fetch the unknown pages of [first, end), retrying after Layout on RICH_NOT_READY. */
    ensurePages(first, end, { signal: callerSignal } = {}) {
      if (!Number.isInteger(first) || !Number.isInteger(end) || first < 0 || end < first) throw new TypeError("Invalid page range.");
      return enqueue(() => runPagesUntilReady(first, end, callerSignal), callerSignal);
    },
    /** Declare the visible pages; unknown ones are fetched in the background, one reply at a time. */
    setVisibleRange(first, end) {
      if (!Number.isInteger(first) || !Number.isInteger(end) || first < 0 || end < first) throw new TypeError("Invalid page range.");
      visible = { first, end };
      cache.setVisible(first, end);
      fillBlockedVersion = -1;
      kick();
    },
    closeDocument({ signal: callerSignal } = {}) { return enqueue(() => exchange({ type: "close" }, callerSignal), callerSignal); },
    /** Stops the pump; a Layout in flight is cancelled, which empties the cached pages (asked again later). whenLayoutComplete() then waits for startPump(). */
    stopPump() { pumpHalt = "stopped"; pumpController.abort(); },
    startPump() { if (pumpHalt) { pumpHalt = null; pumpController = new AbortController(); } kick(); },
    /** Resolves when the State carries the layout-complete flag; rejects if the pump fails, the document closes, open fails or the client is disposed (a request in flight is then rejected with AbortError). Stays pending while the pump is stopped. */
    whenLayoutComplete() {
      if (disposed) return Promise.reject(closedError());
      if (opened && cache.layoutComplete) return Promise.resolve();
      return new Promise((resolve, reject) => layoutWaiters.push({ resolve, reject }));
    },
    /** Resolves when the queue is empty and no background work remains. */
    idle() { return draining ? new Promise((resolve) => idleWaiters.push(resolve)) : Promise.resolve(); },
    dispose,
  });
}
