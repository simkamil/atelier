// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { MessageChannel, Worker } from "node:worker_threads";
import { resolve } from "node:path";
import { createEngineClient } from "../web/engine-client.mjs";
import { createWasmEngine, EngineInputError } from "../web/wasm-loader.mjs";
import { MessageType, ProtocolError, encodeMessage } from "../web/wasm-protocol.mjs";

const wasmPath = resolve(process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm");
const wasmBytes = await readFile(wasmPath);
const workerEntry = new URL("./worker-node-entry.mjs", import.meta.url);
const watchdog = setTimeout(() => {
  console.error("US-092 Worker checks exceeded the 120-second timeout.");
  process.exit(1);
}, 120_000);

const asyncEngine = await createWasmEngine(wasmBytes);
for (let attempt = 0; attempt < 1025; attempt += 1) {
  const input = new Uint8Array(64 * 1024);
  await assert.rejects(asyncEngine.dispatchAsync(input, {
    chunkBytes: 16 * 1024,
    onProgress(progress) {
      if (progress.phase === "input" && progress.copiedBytes === 16 * 1024) structuredClone(input.buffer, { transfer: [input.buffer] });
    },
  }), EngineInputError, "detaching an input between chunks is rejected after freeing its original allocation length");
}
assert.deepEqual(await asyncEngine.dispatchAsync(Uint8Array.of(4, 5, 6)), Uint8Array.of(4, 5, 6), "the engine remains usable after detached-input cleanup");
const lastChunkInput = new Uint8Array(16 * 1024);
await assert.rejects(asyncEngine.dispatchAsync(lastChunkInput, {
  chunkBytes: lastChunkInput.byteLength,
  onProgress(progress) {
    if (progress.phase === "input") structuredClone(lastChunkInput.buffer, { transfer: [lastChunkInput.buffer] });
  },
}), EngineInputError, "detaching in the final input progress callback is caught before dispatch");
let sawConcurrentProgress = false;
const concurrentDispatch = asyncEngine.dispatchAsync(new Uint8Array(1024 * 1024), {
  chunkBytes: 16 * 1024,
  onProgress(progress) { if (progress.phase === "input") sawConcurrentProgress = true; },
});
assert.equal(sawConcurrentProgress, true, "the async dispatch reached its first yielded chunk");
assert.throws(() => asyncEngine.dispatch(Uint8Array.of(1)), EngineInputError, "sync dispatch is rejected while async dispatch is yielding");
await assert.rejects(asyncEngine.dispatchAsync(Uint8Array.of(1)), EngineInputError, "a concurrent async dispatch is rejected");
await concurrentDispatch;

function command(type, requestId, payload = new Uint8Array()) {
  return encodeMessage({ type, requestId, payload });
}

function spawnWorker(workerOptions = {}) {
  const { port1, port2 } = new MessageChannel();
  const worker = new Worker(workerEntry, { workerData: { observer: port2, ...workerOptions }, transferList: [port2] });
  const listeners = new Map();
  const adapter = {
    postMessage: (...args) => worker.postMessage(...args),
    terminate: () => worker.terminate(),
    addEventListener(type, listener) {
      const wrapped = type === "message" ? (data) => listener({ data }) : (event) => listener(event);
      listeners.set(listener, [type, wrapped]);
      worker.on(type, wrapped);
    },
    removeEventListener(type, listener) {
      const item = listeners.get(listener);
      if (item) worker.off(item[0], item[1]);
      listeners.delete(listener);
    },
    get listenerCount() { return listeners.size; },
  };
  return { worker: adapter, observer: port1 };
}

const { worker, observer } = spawnWorker();
const transferObservations = [];
observer.on("message", (message) => transferObservations.push(message));
const client = await createEngineClient({ worker, bytes: wasmBytes });

const maxIdA = 9007199254740993n;
const maxIdB = (1n << 64n) - 1n;
const fifoOrder = [];
const payloadA = Uint8Array.from({ length: 1024 * 1024 }, (_, index) => (index * 19) & 0xff);
const payloadB = Uint8Array.from({ length: 128 * 1024 }, (_, index) => (index * 7) & 0xff);
const backing = new Uint8Array(payloadA.length + 3);
backing.set(payloadA, 1);
const firstFrame = command(MessageType.LoadText, maxIdA, backing.subarray(1, -2));
const secondFrame = command(MessageType.ApplyOps, maxIdB, payloadB);
const first = client.request(firstFrame, { chunkBytes: 64 * 1024, onProgress() { throw new Error("progress callbacks are isolated"); } })
  .then((result) => { fifoOrder.push(maxIdA); return result; });
const second = client.request(secondFrame)
  .then((result) => { fifoOrder.push(maxIdB); return result; });
const [firstResult, secondResult] = await Promise.all([first, second]);
assert.deepEqual(firstResult, firstFrame, "large subview command returns exact ABI echo");
assert.deepEqual(secondResult, secondFrame, "next queued command returns exact ABI echo");
assert.deepEqual(fifoOrder, [maxIdA, maxIdB], "u64 IDs remain correlated in FIFO order");
await new Promise((resolvePromise) => setTimeout(resolvePromise, 10));
assert.ok(transferObservations.length >= 2);
assert.ok(transferObservations.every((event) => event.detached === true), "the exact result buffer is detached by the real port transfer");

assert.throws(() => client.request(Uint8Array.of(1, 2, 3)), ProtocolError, "malformed frames are rejected at admission");
assert.throws(() => client.request(command(MessageType.Ping, 53n), {
  signal: { aborted: false, addEventListener() {} },
}), TypeError, "signal objects must support both listener methods before admission");
assert.deepEqual(
  await client.request(command(MessageType.Ping, 53n), { signal: new AbortController().signal }),
  command(MessageType.Ping, 53n),
  "a rejected signal object does not leave a pending entry",
);
const preAborted = new AbortController();
preAborted.abort();
await assert.rejects(client.request(command(MessageType.Ping, 54n), { signal: preAborted.signal }), { name: "AbortError" }, "an already-aborted signal is rejected before admission");
const duplicateController = new AbortController();
const duplicateIdFrame = command(MessageType.LoadText, 55n, new Uint8Array(8 * 1024 * 1024));
const activeDuplicate = client.request(duplicateIdFrame, { signal: duplicateController.signal, chunkBytes: 16 * 1024 });
assert.throws(() => client.request(command(MessageType.Ping, 55n)), (error) => error.name === "DuplicateRequestIdError");
await activeDuplicate;

async function cancelDuringCopy(phase, requestId, size) {
  const controller = new AbortController();
  const frame = command(MessageType.Render, requestId, new Uint8Array(size));
  let sawPartialProgress = false;
  const cancelled = client.request(frame, {
    signal: controller.signal,
    chunkBytes: 32 * 1024,
    onProgress(progress) {
      if (progress.phase === phase && progress.copiedBytes > 0 && progress.copiedBytes < progress.totalBytes) {
        sawPartialProgress = true;
        controller.abort();
      }
    },
  });
  await assert.rejects(cancelled, { name: "AbortError" });
  assert.equal(sawPartialProgress, true, `cancellation followed real partial ${phase} progress`);
}

await cancelDuringCopy("input", 56n, 8 * 1024 * 1024);
await cancelDuringCopy("output", 59n, 24 * 1024 * 1024);
assert.deepEqual(await client.request(command(MessageType.Ping, 57n)), command(MessageType.Ping, 57n), "engine remains usable after cancellation cleanup");

const activeBeforeQueuedCancel = client.request(command(MessageType.LoadText, 65n, new Uint8Array(8 * 1024 * 1024)), { chunkBytes: 16 * 1024 });
const queuedCancelController = new AbortController();
const queuedCancel = client.request(command(MessageType.Ping, 66n), { signal: queuedCancelController.signal });
queuedCancelController.abort();
await assert.rejects(queuedCancel, { name: "AbortError" }, "Cancel removes a request queued behind the active request");
await activeBeforeQueuedCancel;

const pressureController = new AbortController();
const activeUnderPressure = client.request(command(MessageType.LoadText, 60n, new Uint8Array(16 * 1024 * 1024)), { signal: pressureController.signal, chunkBytes: 16 * 1024 });
const queuedUnderPressure = Array.from({ length: 31 }, (_, index) => client.request(command(MessageType.Ping, BigInt(61 + index))));
assert.throws(() => client.request(command(MessageType.Ping, 100n)), (error) => error.name === "QueueLimitError", "queue cap applies before request admission");
await Promise.all([activeUnderPressure, ...queuedUnderPressure]);

const closeFrame = command(MessageType.LoadText, 58n, new Uint8Array(8 * 1024 * 1024));
const closePending = client.request(closeFrame, { chunkBytes: 16 * 1024 });
client.close();
await assert.rejects(closePending, { name: "WorkerClosedError" }, "close settles pending requests");
assert.equal(worker.listenerCount, 0, "close removes all client listeners");
observer.close();

const failedNodeWorker = new Worker(workerEntry, { workerData: {} });
const failedWorker = {
  postMessage: (...args) => failedNodeWorker.postMessage(...args),
  terminate: () => failedNodeWorker.terminate(),
  addEventListener(type, listener) { failedNodeWorker.on(type, type === "message" ? (data) => listener({ data }) : listener); },
  removeEventListener() {},
};
await assert.rejects(createEngineClient({ worker: failedWorker, bytes: Uint8Array.of(1, 2, 3) }), /WebAssembly|magic|compile/i);

const silentListeners = new Map();
let silentTerminated = false;
const silentWorker = {
  postMessage() {},
  terminate() { silentTerminated = true; },
  addEventListener(type, listener) { silentListeners.set(listener, type); },
  removeEventListener(_type, listener) { silentListeners.delete(listener); },
};
const originalSetTimeout = globalThis.setTimeout;
const originalClearTimeout = globalThis.clearTimeout;
let timeoutCallback;
let timeoutDelay;
let timeoutHandle;
let timerCleared = false;
globalThis.setTimeout = (callback, delay) => {
  timeoutCallback = callback;
  timeoutDelay = delay;
  timeoutHandle = { initTimer: true };
  return timeoutHandle;
};
globalThis.clearTimeout = (handle) => {
  if (handle === timeoutHandle) timerCleared = true;
  else originalClearTimeout(handle);
};
try {
  const silentInitialization = createEngineClient({ worker: silentWorker, bytes: wasmBytes });
  const observedTimeout = assert.rejects(silentInitialization, { name: "WorkerInitTimeoutError", code: "INIT_TIMEOUT" });
  assert.equal(timeoutDelay, 30_000, "initialization timeout is bounded at 30 seconds");
  timeoutCallback();
  await observedTimeout;
  assert.equal(timerCleared, true, "fatal timeout clears its timer");
  assert.equal(silentTerminated, true, "silent Worker is terminated after timeout");
  assert.equal(silentListeners.size, 0, "silent Worker listeners are removed after timeout");
} finally {
  globalThis.setTimeout = originalSetTimeout;
  globalThis.clearTimeout = originalClearTimeout;
}

const fatalHarness = spawnWorker({ fatalOnRequest: true });
const fatalClient = await createEngineClient({ worker: fatalHarness.worker, bytes: wasmBytes });
const fatalFirst = fatalClient.request(command(MessageType.LoadText, 80n, new Uint8Array(8 * 1024 * 1024)), { chunkBytes: 16 * 1024 });
const fatalSecond = fatalClient.request(command(MessageType.Ping, 81n));
await Promise.all([
  assert.rejects(fatalFirst, { name: "WorkerFatalError" }),
  assert.rejects(fatalSecond, { name: "WorkerFatalError" }),
]);
assert.equal(fatalHarness.worker.listenerCount, 0, "fatal Worker removes all client listeners");
fatalHarness.observer.close();

console.log(`US-092 Worker checks passed for ${wasmPath}; transfer detachment, FIFO, BigInt IDs, cancellation, cleanup, and close verified.`);
clearTimeout(watchdog);
