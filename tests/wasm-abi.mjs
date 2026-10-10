// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";
import {
  createWasmEngine,
  EngineAbiError,
  EngineInputError,
  EngineResetError,
  EngineTrapError,
} from "../web/wasm-loader.mjs";

const [productPath, hooksPath] = process.argv.slice(2).map((path) => resolve(path));
assert.ok(productPath && hooksPath, "usage: node tests/wasm-abi.mjs PRODUCT.wasm TEST_HOOKS.wasm");

const productBytes = await readFile(productPath);
const hooksBytes = await readFile(hooksPath);
const productModule = await WebAssembly.compile(productBytes);
const product = new WebAssembly.Instance(productModule);
const { memory, alloc, free, dispatch } = product.exports;
const moduleInitialMemoryBytes = memory.buffer.byteLength;
assert.ok(memory instanceof WebAssembly.Memory, "module exports memory");
assert.equal(typeof alloc, "function");
assert.equal(typeof free, "function");
assert.equal(typeof dispatch, "function");

async function withFakeInstance(factory, run) {
  const original = Object.getOwnPropertyDescriptor(WebAssembly, "Instance");
  Object.defineProperty(WebAssembly, "Instance", {
    ...original,
    value: function FakeInstance(module) {
      return factory(module);
    },
  });
  try {
    await run();
  } finally {
    Object.defineProperty(WebAssembly, "Instance", original);
  }
}

// Exercise loader guards against a module which returns a descriptor shorter
// than eight bytes and one whose payload range lies beyond linear memory.
const malformedFrees = [];
await withFakeInstance(() => {
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: () => fakeMemory.buffer.byteLength - 4,
      free: (ptr, len) => {
        malformedFrees.push([ptr, len]);
        return 0;
      },
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(7)), EngineAbiError, "short descriptor is rejected before reading");
});
assert.deepEqual(malformedFrees, [[65532, 8]], "short descriptor is still released exactly");

const outOfBoundsFrees = [];
await withFakeInstance(() => {
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  const descriptorPtr = 64;
  const descriptor = new DataView(fakeMemory.buffer, descriptorPtr, 8);
  descriptor.setUint32(0, fakeMemory.buffer.byteLength - 3, true);
  descriptor.setUint32(4, 4, true);
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: () => descriptorPtr,
      free: (ptr, len) => {
        outOfBoundsFrees.push([ptr, len]);
        return ptr === descriptorPtr && len === 8 ? 0 : -1;
      },
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(9)), EngineAbiError, "out-of-bounds payload is rejected before reading");
});
assert.deepEqual(
  outOfBoundsFrees,
  [[65533, 4], [64, 8]],
  "the invalid payload range is never read and the descriptor cleanup still runs",
);

// Both alloc and dispatch grow memory; their old ArrayBuffers are detached.
await withFakeInstance(() => {
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  const descriptorPtr = 64;
  const payloadPtr = 128;
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => {
        fakeMemory.grow(1);
        return 32;
      },
      dispatch: (ptr, len) => {
        const body = new Uint8Array(fakeMemory.buffer, ptr, len).slice();
        fakeMemory.grow(1);
        const payload = new DataView(fakeMemory.buffer, payloadPtr, body.length + 4);
        payload.setUint32(0, 0, true);
        new Uint8Array(fakeMemory.buffer, payloadPtr + 4, body.length).set(body);
        const descriptor = new DataView(fakeMemory.buffer, descriptorPtr, 8);
        descriptor.setUint32(0, payloadPtr, true);
        descriptor.setUint32(4, body.length + 4, true);
        return descriptorPtr;
      },
      free: () => 0,
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  const input = Uint8Array.of(0, 254, 12);
  assert.deepEqual(engine.dispatch(input), input, "fresh memory views are used after alloc and dispatch growth");
});

const sentinelError = new Error("ordinary export failure");
await withFakeInstance(() => {
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: () => { throw sentinelError; },
      free: () => 0,
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(1)), (error) => error === sentinelError);
});

let fakeInstanceCount = 0;
await withFakeInstance(() => {
  fakeInstanceCount += 1;
  if (fakeInstanceCount === 2) throw new WebAssembly.RuntimeError("reset instance failure");
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: () => { throw new WebAssembly.RuntimeError("dispatch trap"); },
      free: () => 0,
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(1)), (error) => {
    assert.ok(error instanceof EngineTrapError);
    assert.ok(error.cause instanceof WebAssembly.RuntimeError);
    assert.ok(error.resetError instanceof WebAssembly.RuntimeError);
    return true;
  }, "trap and failed reset remain visible as a typed error");
  assert.throws(() => engine.dispatch(Uint8Array.of(2)), EngineResetError, "failed reset leaves the engine unavailable");
});

let validFatalResetCount = 0;
let validFatalDispatchCalls = 0;
await withFakeInstance(() => {
  validFatalResetCount += 1;
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: validFatalResetCount === 1
        ? () => { validFatalDispatchCalls += 1; return 0; }
        : (ptr, len) => {
          const descriptorPtr = 64;
          const payloadPtr = 128;
          const payload = new DataView(fakeMemory.buffer, payloadPtr, len + 4);
          payload.setUint32(0, 0, true);
          new Uint8Array(fakeMemory.buffer, payloadPtr + 4, len)
            .set(new Uint8Array(fakeMemory.buffer, ptr, len));
          const descriptor = new DataView(fakeMemory.buffer, descriptorPtr, 8);
          descriptor.setUint32(0, payloadPtr, true);
          descriptor.setUint32(4, len + 4, true);
          return descriptorPtr;
        },
      free: () => 0,
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(3)), EngineAbiError, "fatal zero is reported after a valid reset");
  assert.equal(validFatalDispatchCalls, 1, "fatal command is not replayed");
  assert.deepEqual(engine.dispatch(Uint8Array.of(4)), Uint8Array.of(4), "next call uses the recreated instance");
  assert.equal(validFatalResetCount, 2);
});

let invalidFatalResetCount = 0;
let invalidFatalDispatchCalls = 0;
await withFakeInstance(() => {
  invalidFatalResetCount += 1;
  const fakeMemory = new WebAssembly.Memory({ initial: 1 });
  if (invalidFatalResetCount === 2) {
    return { exports: { memory: fakeMemory, alloc: () => 32, dispatch: () => 1 } };
  }
  return {
    exports: {
      memory: fakeMemory,
      alloc: () => 32,
      dispatch: () => { invalidFatalDispatchCalls += 1; return 0; },
      free: () => 0,
    },
  };
}, async () => {
  const engine = await createWasmEngine(productModule);
  assert.throws(() => engine.dispatch(Uint8Array.of(5)), (error) => {
    assert.ok(error instanceof EngineResetError);
    assert.ok(error.cause instanceof EngineAbiError, "invalid replacement exports remain the reset cause");
    return true;
  });
  assert.throws(() => engine.dispatch(Uint8Array.of(6)), EngineResetError);
  assert.equal(invalidFatalDispatchCalls, 1, "discarded fatal instance is never called again");
  assert.equal(invalidFatalResetCount, 2, "failed reset does not retry or replay the command");
});

function directDispatch(input) {
  const ptr = input.length === 0 ? 0 : alloc(input.length);
  if (input.length > 0) {
    assert.notEqual(ptr, 0, "input allocation succeeds");
    new Uint8Array(memory.buffer).set(input, ptr);
  }
  const descriptorPtr = dispatch(ptr, input.length);
  assert.notEqual(descriptorPtr, 0, "dispatch produces a descriptor");
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const payloadPtr = descriptor.getUint32(0, true);
  const payloadLen = descriptor.getUint32(4, true);
  const payload = new DataView(memory.buffer, payloadPtr, payloadLen);
  const status = payload.getUint32(0, true);
  const body = new Uint8Array(memory.buffer, payloadPtr + 4, payloadLen - 4).slice();
  assert.equal(free(payloadPtr, payloadLen), 0, "payload freed exactly");
  assert.equal(free(descriptorPtr, 8), 0, "descriptor freed exactly");
  return { status, body };
}

function readAndFreeResponse(descriptorPtr) {
  assert.notEqual(descriptorPtr, 0, "dispatch produces a descriptor");
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const payloadPtr = descriptor.getUint32(0, true);
  const payloadLen = descriptor.getUint32(4, true);
  const payload = new DataView(memory.buffer, payloadPtr, payloadLen);
  const response = {
    status: payload.getUint32(0, true),
    body: new Uint8Array(memory.buffer, payloadPtr + 4, payloadLen - 4).slice(),
  };
  assert.equal(free(payloadPtr, payloadLen), 0);
  assert.equal(free(descriptorPtr, 8), 0);
  return response;
}

// Pointer/length checks never release an allocation unless both are exact.
const guarded = alloc(9);
assert.notEqual(guarded, 0);
new Uint8Array(memory.buffer, guarded, 9).set([10, 11, 12, 13, 14, 15, 16, 17, 18]);
const emptyEchoResult = directDispatch(new Uint8Array()); // empty echo remains usable
assert.equal(emptyEchoResult.status, 0);
const invalidPointerResult = readAndFreeResponse(dispatch(guarded + 1, 9));
assert.equal(invalidPointerResult.status, 1, "invalid pointer is rejected");
const invalidLengthResult = readAndFreeResponse(dispatch(guarded, 8));
assert.equal(invalidLengthResult.status, 1, "invalid length is rejected");
assert.deepEqual([...new Uint8Array(memory.buffer, guarded, 9)], [10, 11, 12, 13, 14, 15, 16, 17, 18]);
assert.equal(free(guarded, 8), -1, "wrong length rejected without freeing");
const guardedEcho = readAndFreeResponse(dispatch(guarded, 9));
assert.equal(guardedEcho.status, 0, "original allocation remains valid after invalid requests");
assert.deepEqual(guardedEcho.body, Uint8Array.from([10, 11, 12, 13, 14, 15, 16, 17, 18]));
assert.equal(free(guarded, 9), -1, "dispatch consumed the input allocation");
assert.equal(free(0, 0), 0, "zero free is a no-op");
assert.equal(free(0, 1), -1, "null pointer with nonzero size rejected");
assert.equal(alloc(64 * 1024 * 1024 + 1), 0, "oversized allocation rejected");

const binary = Uint8Array.from([0, 255, 1, 0, 128, 42]);
assert.deepEqual(directDispatch(binary), { status: 0, body: binary });
assert.deepEqual(directDispatch(new Uint8Array()), { status: 0, body: new Uint8Array() });

// Fill the slot quota with tiny buffers and confirm the next request is rejected.
const quotaAllocations = [];
for (let index = 0; index < 1024; index += 1) {
  const ptr = alloc(1);
  assert.notEqual(ptr, 0, `allocation ${index + 1} succeeds`);
  quotaAllocations.push(ptr);
}
assert.equal(alloc(1), 0, "allocation count is capped at 1024");
for (const ptr of quotaAllocations) assert.equal(free(ptr, 1), 0);

// Force linear-memory growth in the real artifact, then refresh the view.
const growthSize = memory.buffer.byteLength * 2 + 17;
const beforeGrowth = memory.buffer.byteLength;
const growthPtr = alloc(growthSize);
assert.notEqual(growthPtr, 0, "large allocation succeeds and can grow memory");
assert.ok(memory.buffer.byteLength > beforeGrowth, "WASM linear memory grew");
const growthInput = new Uint8Array(growthSize);
for (let index = 0; index < growthInput.length; index += 1) growthInput[index] = (index * 31) & 0xff;
new Uint8Array(memory.buffer).set(growthInput, growthPtr);
const growthDescriptorPtr = dispatch(growthPtr, growthSize);
assert.notEqual(growthDescriptorPtr, 0);
const growthDescriptor = new DataView(memory.buffer, growthDescriptorPtr, 8);
const growthPayloadPtr = growthDescriptor.getUint32(0, true);
const growthPayloadLen = growthDescriptor.getUint32(4, true);
const growthPayload = new DataView(memory.buffer, growthPayloadPtr, growthPayloadLen);
assert.equal(growthPayload.getUint32(0, true), 0);
assert.deepEqual(
  new Uint8Array(memory.buffer, growthPayloadPtr + 4, growthPayloadLen - 4),
  growthInput,
);
assert.equal(free(growthPayloadPtr, growthPayloadLen), 0);
assert.equal(free(growthDescriptorPtr, 8), 0);

// Exercise the real aggregate-byte quota with two bounded 64 MiB allocations.
// The finally block releases both even if an assertion or a later allocation fails.
let totalQuotaFirst = 0;
let totalQuotaSecond = 0;
try {
  totalQuotaFirst = alloc(64 * 1024 * 1024);
  assert.notEqual(totalQuotaFirst, 0, "first 64 MiB allocation succeeds");
  totalQuotaSecond = alloc(64 * 1024 * 1024);
  assert.notEqual(totalQuotaSecond, 0, "second 64 MiB allocation reaches the 128 MiB quota");
  assert.equal(alloc(1), 0, "aggregate allocation quota is capped at 128 MiB");
} finally {
  if (totalQuotaSecond !== 0) assert.equal(free(totalQuotaSecond, 64 * 1024 * 1024), 0);
  if (totalQuotaFirst !== 0) assert.equal(free(totalQuotaFirst, 64 * 1024 * 1024), 0);
}

const engine = await createWasmEngine(productModule);
const panicMarker = new TextEncoder().encode("__atelier_test_panic__");
assert.deepEqual(engine.dispatch(panicMarker), panicMarker, "production artifact does not contain the panic hook");
const loaderInput = Uint8Array.from([0, 1, 2, 255, 0, 17]);
const loaderOutput = engine.dispatch(loaderInput);
assert.deepEqual(loaderOutput, loaderInput);
loaderInput.fill(0);
assert.deepEqual(loaderOutput, Uint8Array.from([0, 1, 2, 255, 0, 17]), "output is an independent copy");
assert.deepEqual(engine.dispatch(new Uint8Array()), new Uint8Array(), "subsequent dispatch still succeeds");
const utf8Bytes = Buffer.from("café", "utf8");
const utf8View = utf8Bytes.subarray(1, utf8Bytes.length - 1);
assert.deepEqual(engine.dispatch(utf8View), Uint8Array.from(utf8View), "Buffer views preserve exact UTF-8 bytes");
assert.throws(() => engine.dispatch("café"), EngineInputError, "strings must be explicitly encoded as bytes");
for (let index = 0; index < 1100; index += 1) {
  assert.deepEqual(engine.dispatch(binary), binary, `repeated dispatch ${index + 1} succeeds without leaked allocations`);
}
const loaderGrowthSize = moduleInitialMemoryBytes + 17;
assert.ok(loaderGrowthSize + 4 <= 64 * 1024 * 1024, "initial module memory fits the echo payload cap");
const loaderGrowthInput = new Uint8Array(loaderGrowthSize);
for (let index = 0; index < loaderGrowthInput.length; index += 1) loaderGrowthInput[index] = (index * 17) & 0xff;
assert.deepEqual(engine.dispatch(loaderGrowthInput), loaderGrowthInput, "loader refreshes views after memory growth");

const hookEngine = await createWasmEngine(hooksBytes);
assert.throws(
  () => hookEngine.dispatch(new TextEncoder().encode("__atelier_test_panic__")),
  EngineTrapError,
  "test hook raises a real WASM trap",
);
assert.deepEqual(hookEngine.dispatch(binary), binary, "engine instance is recreated after trap");

const optimizedBytes = (await stat(productPath)).size;
console.log(`US-090 WASM ABI checks passed; optimized artifact: ${optimizedBytes} bytes.`);
