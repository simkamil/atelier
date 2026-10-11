// SPDX-License-Identifier: AGPL-3.0-or-later

const MAX_BUFFER_BYTES = 64 * 1024 * 1024;
const DESCRIPTOR_BYTES = 8;
const STATUS_NAMES = new Map([
  [1, "invalid input"],
  [2, "allocation or size limit reached"],
  [3, "unsupported command"],
]);

export class EngineInputError extends TypeError {
  constructor(message) {
    super(message);
    this.name = "EngineInputError";
  }
}

export class EngineAbiError extends Error {
  constructor(message) {
    super(message);
    this.name = "EngineAbiError";
  }
}

export class EngineStatusError extends Error {
  constructor(status) {
    super(`WASM dispatch failed with status ${status}${STATUS_NAMES.has(status) ? ` (${STATUS_NAMES.get(status)})` : ""}`);
    this.name = "EngineStatusError";
    this.status = status;
  }
}

export class EngineMemoryError extends Error {
  constructor(message) {
    super(message);
    this.name = "EngineMemoryError";
  }
}

export class EngineTrapError extends Error {
  constructor(cause, resetError = undefined) {
    super("The WASM engine trapped; its instance was discarded and recreated.", { cause });
    this.name = "EngineTrapError";
    this.resetError = resetError;
  }
}

export class EngineResetError extends Error {
  constructor(message, cause) {
    super(message, { cause });
    this.name = "EngineResetError";
  }
}

function abortError() {
  return new DOMException("The operation was aborted.", "AbortError");
}

function throwIfAborted(signal) {
  if (signal?.aborted) throw abortError();
}

function yieldToTaskQueue() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function normalizeModule(bytesOrModule) {
  if (bytesOrModule instanceof WebAssembly.Module) {
    return bytesOrModule;
  }
  if (bytesOrModule instanceof ArrayBuffer || ArrayBuffer.isView(bytesOrModule)) {
    return WebAssembly.compile(bytesOrModule);
  }
  throw new EngineInputError("Expected WebAssembly.Module, ArrayBuffer, or an ArrayBuffer view.");
}

function instantiate(module) {
  const instance = new WebAssembly.Instance(module);
  const { memory, alloc, free, dispatch } = instance.exports;
  if (!(memory instanceof WebAssembly.Memory)
      || typeof alloc !== "function"
      || typeof free !== "function"
      || typeof dispatch !== "function") {
    throw new EngineAbiError("The module must export memory, alloc, free, and dispatch.");
  }
  return { instance, memory, alloc, free, dispatch };
}

function isInBounds(ptr, len, memoryBytes) {
  return Number.isInteger(ptr)
    && Number.isInteger(len)
    && ptr >= 0
    && len >= 0
    && ptr <= memoryBytes
    && len <= memoryBytes - ptr;
}

/**
 * Compile/instantiate an ABI module. The instance's linear memory remains
 * private; dispatch returns only an independent copy of the response body.
 */
export async function createWasmEngine(bytesOrModule) {
  const module = await normalizeModule(bytesOrModule);
  let state;
  let dispatchBusy = false;
  try {
    state = instantiate(module);
  } catch (error) {
    if (error instanceof WebAssembly.RuntimeError) {
      throw new EngineTrapError(error);
    }
    throw error;
  }

  function resetAfterTrap(cause) {
    try {
      state = instantiate(module);
      return new EngineTrapError(cause);
    } catch (resetError) {
      state = undefined;
      const error = new EngineTrapError(cause, resetError);
      error.message = "The WASM engine trapped and its instance could not be recreated.";
      return error;
    }
  }

  function dispatchCore(input) {
    if (!(input instanceof Uint8Array)) {
      throw new EngineInputError("dispatch expects a Uint8Array.");
    }
    if (input.byteLength > MAX_BUFFER_BYTES) {
      throw new EngineInputError(`Input exceeds ${MAX_BUFFER_BYTES} bytes.`);
    }
    if (!state) {
      throw new EngineResetError("The WASM engine has no usable instance.");
    }

    let inputPtr = 0;
    let inputOutstanding = false;
    let descriptorPtr = 0;
    let payloadPtr = 0;
    let payloadLen = 0;
    let instanceDiscarded = false;
    let result;
    let operationError;

    try {
      inputPtr = state.alloc(input.byteLength) >>> 0;
      if (input.byteLength > 0 && inputPtr === 0) {
        throw new EngineMemoryError("WASM could not allocate the input buffer.");
      }
      inputOutstanding = input.byteLength > 0;

      // alloc may grow linear memory, so make a fresh view after the call.
      if (input.byteLength > 0) {
        const inputMemory = new Uint8Array(state.memory.buffer);
        if (!isInBounds(inputPtr, input.byteLength, inputMemory.byteLength)) {
          throw new EngineAbiError("WASM returned an out-of-range input allocation.");
        }
        inputMemory.set(input, inputPtr);
      }

      descriptorPtr = state.dispatch(inputPtr, input.byteLength) >>> 0;
      inputOutstanding = false; // dispatch consumes a valid input allocation.
      if (descriptorPtr === 0) {
        const error = new EngineAbiError("WASM dispatch returned the fatal zero descriptor.");
        state = undefined;
        instanceDiscarded = true;
        try {
          state = instantiate(module);
        } catch (resetError) {
          state = undefined;
          throw new EngineResetError(
            "The WASM engine returned a fatal descriptor and its instance could not be recreated.",
            resetError,
          );
        }
        throw error;
      }

      // dispatch may grow memory; all views below are created from its current buffer.
      const descriptorMemory = new Uint8Array(state.memory.buffer);
      if (!isInBounds(descriptorPtr, DESCRIPTOR_BYTES, descriptorMemory.byteLength)) {
        throw new EngineAbiError("WASM returned an out-of-range descriptor.");
      }
      const descriptor = new DataView(state.memory.buffer, descriptorPtr, DESCRIPTOR_BYTES);
      payloadPtr = descriptor.getUint32(0, true);
      payloadLen = descriptor.getUint32(4, true);
      if (payloadPtr === 0 || payloadLen < 4 || payloadLen > MAX_BUFFER_BYTES) {
        throw new EngineAbiError("WASM returned an invalid payload pointer or length.");
      }

      const payloadMemory = new Uint8Array(state.memory.buffer);
      if (!isInBounds(payloadPtr, payloadLen, payloadMemory.byteLength)) {
        throw new EngineAbiError("WASM returned an out-of-range payload.");
      }
      const payloadView = new DataView(state.memory.buffer, payloadPtr, payloadLen);
      const status = payloadView.getUint32(0, true);
      if (status !== 0) {
        throw new EngineStatusError(status);
      }
      // slice copies the body; no view into the private WASM memory escapes.
      result = new Uint8Array(state.memory.buffer, payloadPtr + 4, payloadLen - 4).slice();
    } catch (error) {
      operationError = error;
      if (error instanceof WebAssembly.RuntimeError) {
        state = undefined;
        instanceDiscarded = true;
        operationError = resetAfterTrap(error);
      }
    } finally {
      if (!instanceDiscarded && state) {
        const cleanupErrors = [];
        if (payloadPtr !== 0 && payloadLen !== 0) {
          try {
            if (state.free(payloadPtr, payloadLen) !== 0) {
              cleanupErrors.push("WASM rejected freeing the response payload.");
            }
          } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
              state = undefined;
              instanceDiscarded = true;
              operationError = resetAfterTrap(error);
            } else {
              cleanupErrors.push("WASM threw while freeing the response payload.");
            }
          }
        }
        if (!instanceDiscarded && descriptorPtr !== 0) {
          try {
            if (state.free(descriptorPtr, DESCRIPTOR_BYTES) !== 0) {
              cleanupErrors.push("WASM rejected freeing the response descriptor.");
            }
          } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
              state = undefined;
              instanceDiscarded = true;
              operationError = resetAfterTrap(error);
            } else {
              cleanupErrors.push("WASM threw while freeing the response descriptor.");
            }
          }
        }
        if (!instanceDiscarded && inputOutstanding && inputPtr !== 0) {
          try {
            if (state.free(inputPtr, input.byteLength) !== 0) {
              cleanupErrors.push("WASM rejected freeing the unconsumed input.");
            }
          } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
              state = undefined;
              instanceDiscarded = true;
              operationError = resetAfterTrap(error);
            } else {
              cleanupErrors.push("WASM threw while freeing the unconsumed input.");
            }
          }
        }
        if (cleanupErrors.length > 0 && operationError === undefined) {
          operationError = new EngineMemoryError(cleanupErrors.join(" "));
        }
      }
    }

    if (operationError !== undefined) {
      throw operationError;
    }
    return result;
  }

  function dispatch(input) {
    if (dispatchBusy) throw new EngineInputError("This WASM engine already has a dispatch in progress.");
    dispatchBusy = true;
    try {
      return dispatchCore(input);
    } finally {
      dispatchBusy = false;
    }
  }

  async function dispatchAsync(input, options = {}) {
    if (!(input instanceof Uint8Array)) throw new EngineInputError("dispatchAsync expects a Uint8Array.");
    const inputLen = input.byteLength;
    if (inputLen > MAX_BUFFER_BYTES) throw new EngineInputError(`Input exceeds ${MAX_BUFFER_BYTES} bytes.`);
    if (options === null || typeof options !== "object") throw new EngineInputError("dispatchAsync options must be an object.");
    const { signal, onProgress, chunkBytes = 256 * 1024 } = options;
    if (signal !== undefined && (signal === null || typeof signal.addEventListener !== "function" || typeof signal.aborted !== "boolean")) {
      throw new EngineInputError("signal must be an AbortSignal.");
    }
    if (onProgress !== undefined && typeof onProgress !== "function") throw new EngineInputError("onProgress must be a function.");
    if (!Number.isSafeInteger(chunkBytes) || chunkBytes < 16 * 1024 || chunkBytes > 16 * 1024 * 1024) {
      throw new EngineInputError("chunkBytes must be an integer between 16384 and 16777216.");
    }
    if (dispatchBusy) throw new EngineInputError("This WASM engine already has a dispatch in progress.");
    if (!state) throw new EngineResetError("The WASM engine has no usable instance.");
    dispatchBusy = true;

    let inputPtr = 0;
    let inputOutstanding = false;
    let descriptorPtr = 0;
    let payloadPtr = 0;
    let payloadLen = 0;
    let instanceDiscarded = false;
    let operationError;
    let result;
    let inputCopied = 0;
    let outputCopied = 0;
    let outputBytes = 0;
    const report = (phase, copiedBytes, totalBytes) => {
      if (typeof onProgress === "function") {
        try { onProgress(Object.freeze({ phase, copiedBytes, totalBytes })); } catch { /* caller callbacks are isolated */ }
      }
    };
    const ensureInputAttached = () => {
      if (input.byteLength !== inputLen) throw new EngineInputError("Input buffer was detached or resized during dispatchAsync.");
    };
    try {
      throwIfAborted(signal);
      inputPtr = inputLen === 0 ? 0 : state.alloc(inputLen) >>> 0;
      if (inputLen > 0 && inputPtr === 0) throw new EngineMemoryError("WASM could not allocate the input buffer.");
      inputOutstanding = inputLen > 0;
      if (inputLen > 0) {
        for (let offset = 0; offset < inputLen; offset += chunkBytes) {
          ensureInputAttached();
          throwIfAborted(signal);
          const length = Math.min(chunkBytes, inputLen - offset);
          const memory = new Uint8Array(state.memory.buffer);
          if (!isInBounds(inputPtr + offset, length, memory.byteLength)) throw new EngineAbiError("WASM returned an out-of-range input allocation.");
          memory.set(input.subarray(offset, offset + length), inputPtr + offset);
          inputCopied += length;
          report("input", inputCopied, inputLen);
          if (offset + length < inputLen) await yieldToTaskQueue();
        }
      }
      ensureInputAttached();
      throwIfAborted(signal);
      descriptorPtr = state.dispatch(inputPtr, inputLen) >>> 0;
      inputOutstanding = false;
      if (descriptorPtr === 0) {
        const error = new EngineAbiError("WASM dispatch returned the fatal zero descriptor.");
        state = undefined;
        instanceDiscarded = true;
        try { state = instantiate(module); } catch (resetError) {
          state = undefined;
          throw new EngineResetError("The WASM engine returned a fatal descriptor and its instance could not be recreated.", resetError);
        }
        throw error;
      }
      const descriptorMemory = new Uint8Array(state.memory.buffer);
      if (!isInBounds(descriptorPtr, DESCRIPTOR_BYTES, descriptorMemory.byteLength)) throw new EngineAbiError("WASM returned an out-of-range descriptor.");
      const descriptor = new DataView(state.memory.buffer, descriptorPtr, DESCRIPTOR_BYTES);
      payloadPtr = descriptor.getUint32(0, true);
      payloadLen = descriptor.getUint32(4, true);
      if (payloadPtr === 0 || payloadLen < 4 || payloadLen > MAX_BUFFER_BYTES) throw new EngineAbiError("WASM returned an invalid payload pointer or length.");
      const memory = new Uint8Array(state.memory.buffer);
      if (!isInBounds(payloadPtr, payloadLen, memory.byteLength)) throw new EngineAbiError("WASM returned an out-of-range payload.");
      const status = new DataView(state.memory.buffer, payloadPtr, payloadLen).getUint32(0, true);
      if (status !== 0) throw new EngineStatusError(status);
      outputBytes = payloadLen - 4;
      result = new Uint8Array(outputBytes);
      const totalBytes = inputLen + outputBytes;
      for (let offset = 0; offset < outputBytes; offset += chunkBytes) {
        throwIfAborted(signal);
        const length = Math.min(chunkBytes, outputBytes - offset);
        const current = new Uint8Array(state.memory.buffer);
        if (!isInBounds(payloadPtr + 4 + offset, length, current.byteLength)) throw new EngineAbiError("WASM response moved out of bounds while copying.");
        result.set(current.subarray(payloadPtr + 4 + offset, payloadPtr + 4 + offset + length), offset);
        outputCopied += length;
        report("output", inputCopied + outputCopied, totalBytes);
        if (offset + length < outputBytes) await yieldToTaskQueue();
      }
      throwIfAborted(signal);
    } catch (error) {
      operationError = error;
      if (error instanceof WebAssembly.RuntimeError) {
        state = undefined;
        instanceDiscarded = true;
        operationError = resetAfterTrap(error);
      }
    } finally {
      if (!instanceDiscarded && state) {
        const cleanupErrors = [];
        for (const [ptr, len, label] of [[payloadPtr, payloadLen, "response payload"], [descriptorPtr, DESCRIPTOR_BYTES, "response descriptor"], [inputOutstanding ? inputPtr : 0, inputOutstanding ? inputLen : 0, "unconsumed input"]]) {
          if (!ptr || !len) continue;
          try {
            if (state.free(ptr, len) !== 0) cleanupErrors.push(`WASM rejected freeing the ${label}.`);
          } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
              state = undefined;
              instanceDiscarded = true;
              operationError = resetAfterTrap(error);
              break;
            }
            cleanupErrors.push(`WASM threw while freeing the ${label}.`);
          }
        }
        if (cleanupErrors.length && operationError === undefined) operationError = new EngineMemoryError(cleanupErrors.join(" "));
      }
      dispatchBusy = false;
    }
    if (operationError !== undefined) throw operationError;
    report("complete", inputLen + outputBytes, inputLen + outputBytes);
    return result;
  }

  return Object.freeze({ dispatch, dispatchAsync });
}
