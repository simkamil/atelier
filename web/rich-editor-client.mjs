// SPDX-License-Identifier: AGPL-3.0-or-later
// ATED v2 rich-editor client (US-203, R4) on top of an injectable transport with
// the shape of web/engine-client.mjs: `transport.request(frame, {signal, onProgress})`
// resolves with the raw v2 reply frame (the outer US-091 wrapping belongs to the
// transport). Commands run strictly one at a time in submission order; the
// layout pump and the fetch of visible pages only run when the queue is empty,
// so user commands always go first. No DOM access, no editor integration (R7).
import { decodeRichReply, encodeRichRequest, RichProtocolError } from "./rich-editor-protocol.mjs";
import { EditorPageCache, RichClientError } from "./editor-page-cache.mjs";
import { FORMAT_REPLY_TYPE } from "./rich-editor-format.mjs";
import { decodeMessage, encodeMessage, MessageType } from "./wasm-protocol.mjs";

export { EditorPageCache, RichClientError };

const REPLY_TYPE = {
  open: "state", insertText: "state", replaceRange: "state", delete: "state", move: "state", setSelection: "state",
  undo: "state", redo: "state", markSaved: "state", layout: "state", resync: "state", textWindow: "textWindow", pages: "pages", close: "closed",
  // US-217 / US-218: replies that carry no page data and never reach the page cache.
  accessibleWindow: "accessibleWindow", save: "saveData", makeEditableCopy: "saveData", insertImage: "state",
  // R7b-1: selection highlight, picture bitmaps, double and triple click.
  selectionRects: "selectionRects", imageBitmap: "imageBitmap", selectUnit: "state",
  // R7c: text drag and drop (State).
  moveText: "state",
  // T0: rich fragment. `copyFragment` carries no page (NOT_CACHE_INPUT); `pasteFragment` answers a State.
  copyFragment: "fragment", pasteFragment: "state",
  // R7b-2: search, replace, statistics, properties. `replaced` and `propertiesApplied` carry a State (applied to the cache).
  search: "searchResult", statistics: "statistics", searchRects: "searchRects", replace: "replaced", replaceAll: "replaced",
  propertiesRead: "properties", propertiesApply: "propertiesApplied",
  // F0: formatting. The commands that change the document answer a State; FormatState and StyleList carry no page.
  ...FORMAT_REPLY_TYPE,
};
const NOT_CACHE_INPUT = new Set(["accessibleWindow", "saveData", "fragment", "selectionRects", "imageBitmap", "searchResult", "statistics", "searchRects", "properties", "formatState", "styleList", "fontList", "formatMarks"]);
/** Replies whose result changes the document or the state the cache holds. */
const STATEFUL = new Set(["state", "replaced", "propertiesApplied"]);

const RICH_MAX_PAGES_PER_REQUEST = 16;

/**
 * Transport of the clients of every document of one engine (T0): one v2 frame in an `Editor` message of the outer
 * protocol, over the engine client (`request(bytes, {signal})` resolving with the outer reply). The outer request ids
 * come from ONE counter, so create it once per engine client and give it to every `createRichEditorClient`: the Worker
 * refuses two requests with the same id (`DUPLICATE_REQUEST_ID`), which two transports of their own would produce.
 * Errors carry the codes of `createTransport` of web/rich-editor.mjs (EDITOR_RESPONSE_MISMATCH, EDITOR_ENGINE_FAILED).
 */
export function createEngineTransport(engine) {
  if (!engine || typeof engine.request !== "function") throw new TypeError("engine must provide request().");
  let outer = 2n;
  return Object.freeze({
    async request(frame, { signal } = {}) {
      const id = outer;
      outer = outer === 0xffffffffffffffffn ? 2n : outer + 1n;
      const bytes = await engine.request(encodeMessage({ type: MessageType.Editor, requestId: id, payload: frame }), { signal });
      const message = decodeMessage(bytes, "event");
      if (message.requestId !== id) throw new RichClientError("EDITOR_RESPONSE_MISMATCH", "Reply of another request.");
      if (message.type === MessageType.Failed) throw new RichClientError("EDITOR_ENGINE_FAILED", new TextDecoder().decode(message.payload));
      if (message.type !== MessageType.EditorResult) throw new RichClientError("EDITOR_RESPONSE_MISMATCH", "Unexpected outer reply.");
      return message.payload;
    },
  });
}

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
 * @param {(highlight: {revision: bigint, rects: object[]}) => void} [options.onSelectionRects] the selection rectangles
 *   (points of the page, `{page, x, y, width, height}`) of the visible pages, asked after each State whose selection is not
 *   empty (one request per State, cancelled when a newer State arrives, a reply of another revision is dropped) and
 *   published empty as soon as the selection is
 * @param {(id: bigint) => boolean} [options.hasImage] true when the bitmap of a picture is already held (R7b-1)
 * @param {(bitmap: {objectId: bigint, width: number, height: number, rgba: Uint8Array}) => void} [options.onImageBitmap] a bitmap
 *   asked for by `setWantedImages` arrived (the pixels are a copy)
 */
/** Paragraphs per Layout command while a selection waits for its caret (about 10 ms in release). */
const JUMP_LAYOUT_BUDGET = 64;
/** Moves the engine refuses while their target is not laid out (no caret): replayed once the caret is there. */
const REPLAYED_MOVES = new Set(["up", "down", "pageUp", "pageDown", "home", "end"]);
const CARET_COMMANDS = new Set(["move", "setSelection", "selectUnit", "moveText", "insertText", "delete", "replaceRange", "undo", "redo", "insertImage"]);

export function createRichEditorClient({
  transport, docId, cache = new EditorPageCache(), layoutBudget = 16, signal, yieldTurn = defaultYield,
  onError = () => {}, maxLayoutRetries = 4096, onSelectionRects = () => {}, hasImage = () => false, onImageBitmap = () => {},
} = {}) {
  if (!transport || typeof transport.request !== "function") throw new TypeError("transport must provide request().");
  if (!Number.isInteger(docId) || docId < 1 || docId > 0xffffffff) throw new TypeError("docId must be a non-zero u32.");
  if (!Number.isInteger(layoutBudget) || layoutBudget < 1 || layoutBudget > 4096) throw new TypeError("layoutBudget must be 1 to 4096.");
  if (typeof yieldTurn !== "function" || typeof onError !== "function") throw new TypeError("yieldTurn and onError must be functions.");
  if (typeof onSelectionRects !== "function" || typeof hasImage !== "function" || typeof onImageBitmap !== "function") {
    throw new TypeError("onSelectionRects, hasImage and onImageBitmap must be functions.");
  }

  let disposed = false;
  let opened = false;
  let nextRequestId = 1;
  let draining = false;
  let wake = false;
  let needsResync = false;
  let overflows = 0; // consecutive resyncs caused by the client atlas byte bound
  let pumpHalt = null; // null | "stopped" | "error"
  let visible = null;
  let fillBlockedVersion = -1;
  const queue = [];
  const idleWaiters = [];
  const layoutWaiters = [];
  const lifetime = new AbortController();
  let pumpController = new AbortController();
  const clientLink = linkSignals([signal]);
  // R7b-1: the highlight job of the latest State, and the pictures wanted by the pages shown.
  let selectionJob = null; // {revision, done, controller}
  let selectionBlockedVersion = -1;
  let highlighted = false; // the last published highlight had rectangles
  let wantedImages = [];
  const imageBlocked = new Map(); // id -> cache version at which the fetch failed

  const closedError = () => new RichClientError("CLIENT_CLOSED", "Rich editor client is closed.");
  const report = (error) => { try { onError(error); } catch { /* isolate user callbacks */ } };

  function publishSelection(revision, rects) {
    if (rects.length === 0 && !highlighted) return;
    highlighted = rects.length > 0;
    try { onSelectionRects({ revision, rects }); } catch { /* isolate user callbacks */ }
  }

  /** A State was applied: the highlight of the previous one is out of date. */
  function noteState(state) {
    selectionJob?.controller?.abort();
    selectionBlockedVersion = -1;
    if (state.anchor === state.focus) {
      selectionJob = { revision: state.revision, empty: true, done: true, controller: null };
      publishSelection(state.revision, []);
    } else {
      selectionJob = { revision: state.revision, empty: false, done: false, controller: null };
    }
  }

  function dropSelection() {
    selectionJob?.controller?.abort();
    selectionJob = null;
    publishSelection(null, []);
  }

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
      opened = false; awaitingCaret = false; cache.reset(); pumpHalt = "stopped";
      dropSelection();
      settleLayoutWaiters(closedError());
      return { response, change: null };
    }
    if (NOT_CACHE_INPUT.has(response.type)) { settleLayoutWaiters(); return { response, change: null }; }
    let change;
    try { change = cache.apply(response.type === "replaced" || response.type === "propertiesApplied" ? response.state : response); } catch (error) {
      // Glyph atlas entries were lost (an abandoned reply the engine counted as sent): the pages that
      // refer to them cannot be trusted, so a `Resync` (new atlas epoch) is requested.
      if (error?.cause?.atlas === true && !disposed) { cache.dropPages(); needsResync = true; }
      if (error?.cause?.atlasBytes === true && !disposed) {
        cache.dropPages(); needsResync = true;
        overflows += 1;
        if (overflows >= 2) {
          // The atlas filled the client bound right after a resync: another resync would loop.
          needsResync = false; pumpHalt = "error";
          const fatal = new RichClientError("CLIENT_ATLAS_FULL", "Glyph atlas exceeds the client byte bound after a resynchronisation.", error);
          settleLayoutWaiters(fatal); report(fatal);
          throw fatal;
        }
      }
      throw error;
    }
    if (command.type !== "resync") overflows = 0;
    if (response.type === "state" && !change.ignored) noteState(response);
    settleLayoutWaiters();
    return { response, change };
  }

  /**
   * Command whose reply carries a State or Pages: when it is cancelled the lost invalidations (State)
   * or the lost atlas entries (State, Pages) force a resync.
   */
  async function guarded(command, callerSignal) {
    try { return await exchange(command, callerSignal); } catch (error) {
      // The engine may have applied the command whatever went wrong, except for its own RICH_* refusals.
      // RICH_FRAME_TOO_LARGE is not a refusal: the engine applied the command, then lost the reply (mark_resync).
      const refused = typeof error?.code === "string" && error.code.startsWith("RICH_") && error.code !== "RICH_FRAME_TOO_LARGE";
      const type = REPLY_TYPE[command.type];
      if (!refused && (STATEFUL.has(type) || type === "pages") && !disposed && error?.code !== "CLIENT_CLOSED" && error?.code !== "CLIENT_ATLAS_FULL") {
        if (STATEFUL.has(type)) cache.dropPages();
        needsResync = true;
      }
      throw error;
    }
  }

  async function runPagesUntilReady(first, end, callerSignal) {
    let notReady = 0;
    for (let guard = 0; guard < maxLayoutRetries + 64; guard += 1) {
      const run = cache.missing(first, end)[0];
      if (!run) return;
      try {
        await guarded({ type: "pages", first: run.first, count: run.count }, callerSignal);
      } catch (error) {
        if (error?.code !== "RICH_NOT_READY" || cache.layoutComplete || notReady >= maxLayoutRetries) throw error;
        notReady += 1;
        await guarded({ type: "layout", budget: layoutBudget }, callerSignal);
        await yieldTurn();
      }
    }
    throw new RichClientError("CLIENT_RETRY_LIMIT", "Pages stayed unavailable.");
  }

  /** Close: no resync before it; a document the engine no longer has counts as closed, whatever the resync state. */
  async function closeExchange(callerSignal) {
    try { return await exchange({ type: "close" }, callerSignal); } catch (error) {
      if (error?.code === "RICH_UNKNOWN_DOCUMENT") {
        opened = false; awaitingCaret = false; cache.reset(); pumpHalt = "stopped"; needsResync = false;
        settleLayoutWaiters(closedError());
      }
      throw error;
    }
  }

  /**
   * The layout is sequential (the tail cannot be laid out before what precedes it), so a focus that the layout has not
   * reached cannot be served first; its page is reached sooner with larger steps (still about 10 ms each). Commands
   * queued meanwhile run before the next step, so no keystroke waits for the pump.
   */
  function pumpBudget() {
    if (cache.caret || cache.layoutComplete) awaitingCaret = false;
    return opened && awaitingCaret ? Math.max(layoutBudget, JUMP_LAYOUT_BUDGET) : layoutBudget;
  }

  let awaitingCaret = false;
  /** Last Up/Down/PageUp/PageDown/Home/End that found no caret to start from; replayed when the caret arrives. */
  let replayMove = null;

  function pumpWanted() { return opened && !pumpHalt && !cache.layoutComplete; }

  async function idleStep() {
    if (needsResync && opened) {
      needsResync = false;
      try { await guarded({ type: "resync" }, undefined); } catch (error) {
        if (disposed) return false;
        needsResync = true;
        report(error);
        return false;
      }
      return true;
    }
    if (replayMove && opened && cache.caret) {
      const move = replayMove;
      replayMove = null;
      try { await guarded(move, undefined); } catch (error) { if (disposed) return false; report(error); }
      return true;
    }
    if (visible && opened && fillBlockedVersion !== cache.version) {
      const run = cache.missing(visible.first, visible.end)[0];
      if (run) {
        const version = cache.version;
        try {
          const { change } = await guarded({ type: "pages", first: run.first, count: run.count }, undefined);
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
    if (selectionJob && !selectionJob.done && visible && opened && selectionBlockedVersion !== cache.version) {
      const job = selectionJob;
      const version = cache.version;
      job.controller = new AbortController();
      try {
        const count = Math.min(RICH_MAX_PAGES_PER_REQUEST, Math.max(1, visible.end - visible.first));
        const { response } = await exchange({ type: "selectionRects", first: visible.first, count }, job.controller.signal);
        // A newer State (or a changed viewport) replaced the job while the request ran: the reply is stale.
        if (selectionJob === job && job.controller && response.revision === job.revision && cache.revision === job.revision) {
          job.done = true;
          publishSelection(response.revision, response.rects);
        } else if (selectionJob === job) selectionBlockedVersion = version;
      } catch (error) {
        if (disposed) return false;
        if (isAbort(error)) return true; // superseded
        if (selectionJob === job) selectionBlockedVersion = version;
        // The pages of the viewport are not laid out yet: asked again when the cache changes.
        if (error?.code !== "RICH_NOT_READY" && error?.code !== "RICH_PAGE_RANGE") report(error);
      }
      return true;
    }
    const missingImage = opened ? wantedImages.find((id) => !hasImage(id) && imageBlocked.get(id) !== cache.version) : undefined;
    if (missingImage !== undefined) {
      const version = cache.version;
      try {
        const { response } = await exchange({ type: "imageBitmap", objectId: missingImage }, undefined);
        const rgba = response.rgba.slice();
        try { onImageBitmap({ objectId: response.objectId, width: response.width, height: response.height, rgba }); } catch (error) { report(error); }
        // Held now, or the owner refused it: do not ask again until the cache changes.
        imageBlocked.set(missingImage, version);
      } catch (error) {
        if (disposed) return false;
        if (isAbort(error)) return true;
        imageBlocked.set(missingImage, version);
        // A picture that is gone (a page about to be replaced) is no failure.
        if (error?.code !== "RICH_UNKNOWN_OBJECT") report(error);
      }
      return true;
    }
    if (pumpWanted()) {
      try {
        await guarded({ type: "layout", budget: pumpBudget() }, pumpController.signal);
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
          // An Open replaces the whole state: no resync before it.
          if (needsResync && opened && !task.replaces) {
            needsResync = false;
            try { await guarded({ type: "resync" }, undefined); } catch (error) {
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

  function enqueue(run, taskSignal, replaces = false) {
    if (disposed) return Promise.reject(closedError());
    if (taskSignal !== undefined && taskSignal !== null && typeof taskSignal.addEventListener !== "function") {
      return Promise.reject(new TypeError("signal must be an AbortSignal."));
    }
    // A command is waiting: a highlight request in flight is dropped (asked again if no State follows).
    selectionJob?.controller?.abort();
    return new Promise((resolve, reject) => {
      queue.push({ run, resolve, reject, signal: taskSignal, replaces });
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
    /** Epoch of the glyph atlas the pages refer to (null before the first reply). */
    get atlasEpoch() { return cache.atlasEpoch; },
    /** `resolveGlyph` of CanvasRenderer for the pages of this document. */
    glyphResolver: cache.glyphResolver,
    /** Open a document (replaces the active one) and start the layout pump. */
    open(spec, { signal: callerSignal } = {}) {
      return enqueue(async () => {
        const wasOpened = opened;
        cache.reset();
        opened = false;
        pumpHalt = null;
        fillBlockedVersion = -1;
        awaitingCaret = false;
        replayMove = null;
        needsResync = false;
        pumpController = new AbortController();
        dropSelection();
        imageBlocked.clear();
        let result;
        try { result = await exchange({ type: "open", ...spec }, callerSignal); } catch (error) {
          // The engine keeps the previous document when it refuses an Open (and holds either one when the
          // transport failed): the cache was emptied, so a Resync rebuilds it instead of leaving a blank editor.
          if (wasOpened && !disposed && error?.code !== "CLIENT_CLOSED") { opened = true; needsResync = true; pumpHalt = null; }
          settleLayoutWaiters(error);
          throw error;
        }
        opened = true;
        return result;
      }, callerSignal, true);
    },
    /** Send one command (or a function that returns it when its turn comes); resolves `{response, change}`, rejects with RichClientError for RICH_* refusals. */
    command(requested, { signal: callerSignal } = {}) {
      if (requested?.type === "open") return Promise.reject(new TypeError("Use open() to open a document."));
      return enqueue(async () => {
        // A function builds the command when its turn comes (F0: a revision guard must be the one of that moment).
        const command = typeof requested === "function" ? requested() : requested;
        if (command?.type === "open") throw new TypeError("Use open() to open a document.");
        if (command?.type === "close") { replayMove = null; return closeExchange(callerSignal); }
        replayMove = null; // a newer command supersedes a move that waited for its caret
        const result = await guarded(command, callerSignal);
        if (command?.type === "move" && REPLAYED_MOVES.has(command.direction) && cache.caret === null && !cache.layoutComplete) replayMove = command;
        // A selection command that left the caret unplaced (target not laid out yet): the pump steps faster until it shows.
        if (CARET_COMMANDS.has(command?.type)) awaitingCaret = cache.caret === null && !cache.layoutComplete;
        return result;
      }, callerSignal, requested?.type === "close");
    },
    /**
     * Accessible window of [start, end) (flat positions, BigInt) for the DOM mirror (US-209):
     * resolves `{revision, items}`. It changes neither the cache nor the document, so a failure never forces a resync.
     */
    accessibleWindow(start, end, { signal: callerSignal } = {}) {
      return enqueue(async () => {
        const { response } = await exchange({ type: "accessibleWindow", start, end }, callerSignal);
        return { revision: response.revision, items: response.items };
      }, callerSignal);
    },
    /**
     * Rich fragment of [start, end) (flat positions, BigInt) of the document at `revision` (T0): the revision the caller
     * saw the range in (BigInt, required: the engine answers RICH_STALE_REVISION when the document moved on). Resolves
     * `{revision, lossy, text, bytes}`; `bytes` (a copy) goes to `pasteFragment` of any document of the engine, `text`
     * is what the system clipboard receives. Changes neither the cache nor the document.
     */
    copyFragment(start, end, { revision, signal: callerSignal } = {}) {
      if (typeof revision !== "bigint") return Promise.reject(new TypeError("copyFragment needs the expected revision (BigInt)."));
      return enqueue(async () => {
        const { response } = await exchange({ type: "copyFragment", revision, start, end }, callerSignal);
        return { revision: response.revision, lossy: response.lossy, text: response.text, bytes: response.bytes.slice() };
      }, callerSignal);
    },
    /** Replaces the selection by a fragment, one undo step (T0); resolves like any State-answered command. */
    pasteFragment(bytes, { signal: callerSignal } = {}) {
      return enqueue(async () => { replayMove = null; return guarded({ type: "pasteFragment", bytes }, callerSignal); }, callerSignal);
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
      // Other pages are shown: the highlight is asked again for them.
      if (selectionJob && !selectionJob.empty) {
        selectionJob.controller?.abort();
        selectionJob = { revision: selectionJob.revision, empty: false, done: false, controller: null };
      }
      selectionBlockedVersion = -1;
      kick();
    },
    /**
     * Declare the pictures the pages shown draw (`bigint` object ids); the ones `hasImage` does not hold are fetched
     * in the background, one reply at a time, and handed to `onImageBitmap`.
     */
    setWantedImages(ids) {
      wantedImages = [...ids];
      for (const id of wantedImages) if (typeof id !== "bigint") throw new TypeError("Picture identities must be BigInt.");
      kick();
    },
    closeDocument({ signal: callerSignal } = {}) { return enqueue(() => closeExchange(callerSignal), callerSignal, true); },
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
