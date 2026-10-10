# US-090 — ABI WASM manuelle

`atelier-core-wasm` exposes `memory`, `alloc(len)`, `free(ptr, len)` and `dispatch(ptr, len)` from its `wasm32-unknown-unknown` `cdylib`. The boundary uses no `wasm-bindgen`. For this story `dispatch` is only a transport echo; command decoding and versioned messages belong to US-091.

## Memory and ownership

- `alloc` returns a zero-filled allocation, or zero on allocation/limit failure. `alloc(0)` returns zero without registering an allocation.
- `free` returns `0` on success and `-1` for a pointer/length mismatch, an unknown pointer, or a second free. `free(0, 0)` is a no-op. The registry compares the exact pair before reading or dropping the allocation.
- `dispatch` consumes a valid input allocation only when both pointer and length match. It returns the address of a separately allocated, 8-byte little-endian descriptor: `payload_ptr:u32`, `payload_len:u32`. A zero result is a fatal ABI failure.
- The payload begins with a little-endian `u32` status: `0` success, `1` invalid input allocation, `2` size/allocation limit, `3` unsupported command. On success the remaining bytes are an exact copy of the input. The descriptor and payload are separate allocations; free each with its exact original size.
- A `(0, 0)` dispatch represents the empty echo. It does not create or consume an input allocation.
- One allocation is limited to 64 MiB, aggregate live allocations to 128 MiB, and the registry to 1024 allocations. Arithmetic and vector reservation are checked. During echo dispatch, the consumed input stays charged until it is resized into the response payload, so request and output bytes are not double-counted or copied into a second body buffer.
- All returned data belongs to WASM until freed. The JS loader copies the response body before freeing both allocations and never exposes or transfers the WASM memory buffer.

## JavaScript loader and traps

`createWasmEngine(bytesOrModule)` asynchronously compiles bytes if needed and returns an object with synchronous `dispatch(Uint8Array)`. It validates descriptors and memory ranges, creates fresh views after calls which can grow memory, and returns a detached copy of the body. `EngineInputError`, `EngineAbiError`, `EngineMemoryError`, `EngineStatusError`, `EngineTrapError` and `EngineResetError` distinguish failures.

On `WebAssembly.RuntimeError`, the loader discards the instance and synchronously creates a fresh one from the already compiled module. It then raises `EngineTrapError`; it never retries the command. A zero descriptor is also fatal: the loader discards the instance before attempting a reset, then raises `EngineAbiError`. If that reset fails, it raises `EngineResetError` with the reset failure as its cause and keeps the engine unavailable, so later calls cannot reach the discarded instance. A panic in the optimized artifact uses `panic=abort`, so it is a WASM trap rather than a recoverable Rust unwind. The `abi-test-hooks` feature enables the reserved `__atelier_test_panic__` input for the separate test artifact; production builds omit it.

## Build and verification

Install the target once, then build the production artifact and a separate test-hook artifact:

```powershell
rustup target add wasm32-unknown-unknown
cargo build --locked --offline -p atelier-core-wasm --profile wasm-release --target wasm32-unknown-unknown
cargo build --locked --offline -p atelier-core-wasm --profile wasm-release --target wasm32-unknown-unknown --features abi-test-hooks --target-dir target/wasm-abi-hooks
node tests/wasm-abi.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
node tests/check-wasm-size.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
```

The hook build uses its own target directory so it cannot overwrite the production benchmark artifact. Record the production artifact size in bytes with:

```powershell
(Get-Item target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm).Length
```

Reference artifact size measured from the production `wasm-release` build: **52,323 bytes**, with a 10% ceiling of **57,555 whole bytes**. The measured baseline and threshold policy are recorded in [`tools/bench/references/wasm-size.json`](../tools/bench/references/wasm-size.json).
