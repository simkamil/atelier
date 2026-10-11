// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEngineClient } from "../web/engine-client.mjs";
import { encodeMessage, MessageType } from "../web/wasm-protocol.mjs";

const result = document.querySelector("#result");
let client;
let report;
const checks = [];
function assert(value, name) {
  if (!value) throw new Error(name);
  checks.push(name);
}
function frame(id, size = 0) {
  return encodeMessage({ type: size ? MessageType.LoadText : MessageType.Ping,
    requestId: id, payload: Uint8Array.from({ length: size }, (_, i) => i % 251) });
}
try {
  const response = await fetch("../target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm");
  assert(response.ok, "artefact WASM accessible");
  client = await createEngineClient({ bytes: await response.arrayBuffer() });
  const first = frame(9007199254740993n, 1024 * 1024);
  const order = [];
  const [actual, second] = await Promise.all([
    client.request(first).then(bytes => { order.push(1); return bytes; }),
    client.request(frame(2n)).then(bytes => { order.push(2); return bytes; }),
  ]);
  assert(actual.length === first.length && actual.every((byte, i) => byte === first[i]), "octets exacts, ID u64 conservé");
  assert(order.join() === "1,2" && second.length === 24, "FIFO dans un vrai Worker ES navigateur");
  const controller = new AbortController();
  let partial = false;
  try {
    await client.request(frame(3n, 8 * 1024 * 1024), {
      signal: controller.signal, chunkBytes: 32768,
      onProgress(progress) {
        if (progress.phase === "input" && progress.copiedBytes < progress.totalBytes) {
          partial = true;
          controller.abort();
        }
      },
    });
    throw new Error("annulation non exécutée");
  } catch (error) {
    assert(partial && error.name === "AbortError", "annulation après progression partielle réelle");
  }
  for (let id = 4n; id < 45n; id++) {
    const expected = frame(id);
    const bytes = await client.request(expected);
    if (!bytes.every((byte, i) => byte === expected[i])) throw new Error("reprise après annulation");
  }
  assert(true, "mémoire WASM réutilisable et budget libéré sur 41 commandes");
  report = { status: "passed", userAgent: navigator.userAgent,
    crossOriginIsolated: globalThis.crossOriginIsolated, checks };
} catch (error) {
  report = { status: "failed", message: String(error), checks };
} finally {
  client?.close();
  result.textContent = JSON.stringify(report, null, 2);
  if (new URL(location.href).searchParams.get("automated") === "1") {
    await fetch("/__worker_result", { method: "POST", body: JSON.stringify(report) });
  }
}
