// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createWasmEngine } from "../web/wasm-loader.mjs";
import { encodeMessage, decodeMessage, MessageType } from "../web/wasm-protocol.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === "--compare-artifact"),
  "usage: node tests/check-embedded-engine.mjs [--compare-artifact]");
const manifest = JSON.parse(await readFile(resolve(root, "data/engine/manifest.json"), "utf8"));
assert.equal(manifest.schema_version, 1);
assert.equal(manifest.artifact, "atelier_core_wasm.wasm");
assert.equal(manifest.target, "wasm32-unknown-unknown");
assert.equal(manifest.profile, "wasm-release");
assert.deepEqual(manifest.features, []);
assert.match(manifest.sha256, /^[0-9a-f]{64}$/);
assert.match(manifest.rustc, /^rustc \d+\.\d+\.\d+ /);
assert.equal(manifest.source_hash_encoding, "UTF-8 bytes with CRLF normalized to LF");
assert.ok(Number.isSafeInteger(manifest.bytes) && manifest.bytes > 8 && manifest.bytes <= 64 * 1024 * 1024);

async function rustSources(relative) {
  const entries = await readdir(resolve(root, relative), { withFileTypes: true });
  const lists = await Promise.all(entries.map(async (entry) => {
    const path = `${relative}/${entry.name}`;
    if (entry.isDirectory()) return rustSources(path);
    return entry.isFile() && entry.name.endsWith(".rs") ? [path] : [];
  }));
  return lists.flat();
}
const sources = ["Cargo.toml", "crates/core-wasm/Cargo.toml", ...await rustSources("crates/core-wasm/src")].sort();
assert.deepEqual(Object.keys(manifest.sources_sha256).sort(), sources, "manifest covers exact Rust source inventory");
const cargo = await readFile(resolve(root, "crates/core-wasm/Cargo.toml"), "utf8");
assert.ok(!/^\s*\[(?:[^\]]*\.)?(?:build-|dev-)?dependencies(?:\.[^\]]+)?\]/m.test(cargo),
  "engine gained dependencies: extend the packager and verifier to their complete local source closure");
for (const source of sources) {
  assert.match(manifest.sources_sha256[source], /^[0-9a-f]{64}$/);
  const sourceText = await readFile(resolve(root, source), "utf8");
  assert.equal(sha256(sourceText.replace(/\r\n/g, "\n")), manifest.sources_sha256[source],
    `stale packaged engine source: ${source}; regenerate with tools/package-engine.py`);
}
const bytes = await readFile(resolve(root, `data/engine/${manifest.artifact}`));
assert.equal(bytes.length, manifest.bytes);
assert.equal(sha256(bytes), manifest.sha256, "packaged artifact digest");
const module = await WebAssembly.compile(bytes);
assert.deepEqual(WebAssembly.Module.imports(module), [], "standalone engine imports no host capabilities");
const exports = new Map(WebAssembly.Module.exports(module).map(({ name, kind }) => [name, kind]));
for (const name of ["alloc", "free", "dispatch"]) assert.equal(exports.get(name), "function");
assert.equal(exports.get("memory"), "memory");
assert.ok(![...exports.keys()].some((name) => /test|panic/i.test(name)), "test hooks must not be packaged");
const engine = await createWasmEngine(module);
const panicMarker = new TextEncoder().encode("__atelier_test_panic__");
assert.deepEqual(engine.dispatch(panicMarker), panicMarker,
  "packaged engine excludes the private input-triggered panic hook");
const request = { type: MessageType.Ping, requestId: 112n, payload: new Uint8Array() };
assert.deepEqual(decodeMessage(engine.dispatch(encodeMessage(request)), "command"), request,
  "packaged current engine transports Ping by exact ABI echo, not a business Pong");
if (args.length) {
  const artifact = await readFile(resolve(root, "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm"));
  assert.equal(sha256(artifact), manifest.sha256, "fresh local Cargo artifact equals packaged bytes");
}
console.log(JSON.stringify({ status: "passed", bytes: bytes.length, sha256: manifest.sha256,
  source_count: sources.length, exact_local_artifact: args.length === 1,
  dispatch: "current ABI echo; document handlers pending" }));
