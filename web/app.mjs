// SPDX-License-Identifier: AGPL-3.0-or-later
import { createEngineClient } from "/engine-client.mjs";
import { decodeMessage, encodeMessage, MessageType } from "/wasm-protocol.mjs";

const nodes = {
  title: document.querySelector("#status-title"),
  detail: document.querySelector("#status-detail"),
  indicator: document.querySelector("#status-indicator"),
  engine: document.querySelector("#engine-state"),
};

async function loadCatalog() {
  const response = await fetch("/i18n/fr-FR.txt", { cache: "no-store" });
  if (!response.ok) throw new Error("Le catalogue français embarqué est indisponible.");
  const entries = new Map();
  for (const line of (await response.text()).split(/\r?\n/)) {
    if (!line || line.startsWith("#")) continue;
    const separator = line.indexOf("=");
    if (separator > 0) entries.set(line.slice(0, separator), line.slice(separator + 1));
  }
  for (const element of document.querySelectorAll("[data-i18n]")) {
    const value = entries.get(element.dataset.i18n);
    if (value !== undefined) element.textContent = value;
  }
  return entries;
}

function showResult(ok, title, detail, entries) {
  document.documentElement.dataset.engineEcho = ok ? "ok" : "failed";
  nodes.title.textContent = title;
  nodes.detail.textContent = detail;
  nodes.indicator.className = `status-indicator ${ok ? "success" : "error"}`;
  nodes.indicator.setAttribute("aria-label", entries.get(ok ? "status-a11y-ok" : "status-a11y-failed"));
  nodes.engine.textContent = entries.get(ok ? "engine-ready" : "engine-error");
  nodes.engine.className = `engine-state ${ok ? "success" : "error"}`;
}

async function start() {
  let catalog;
  let client;
  try {
    catalog = await loadCatalog();
    const response = await fetch("/atelier_core_wasm.wasm", { cache: "no-store" });
    if (!response.ok) throw new Error("Le moteur WebAssembly embarqué est indisponible.");
    const bytes = await response.arrayBuffer();
    const worker = new Worker("/engine-worker.mjs", { type: "module", name: "atelier-engine" });
    client = await createEngineClient({ worker, bytes });
    const frame = encodeMessage({ type: MessageType.Ping, requestId: 1n, payload: new Uint8Array() });
    const reply = decodeMessage(await client.request(frame), "command");
    if (reply.type !== MessageType.Ping || reply.requestId !== 1n || reply.payload.byteLength !== 0) {
      throw new Error("Le moteur n’a pas renvoyé l’écho exact de la commande Ping.");
    }
    showResult(true, catalog.get("status-ok"), catalog.get("status-success-detail"), catalog);
  } catch {
    const messages = catalog ?? new Map([
      ["engine-error", "Indisponible"],
      ["status-failed", "Le contrôle du moteur a échoué."],
      ["status-failure-detail", "Le contrôle local n’a pas abouti. Réessayez plus tard."],
      ["status-a11y-ok", "Écho confirmé"],
      ["status-a11y-failed", "Échec"],
    ]);
    showResult(false, messages.get("status-failed"), messages.get("status-failure-detail"), messages);
  } finally {
    client?.close();
  }
}

document.addEventListener("securitypolicyviolation", (event) => {
  const blocked = event.blockedURI || "";
  const directive = event.effectiveDirective || "";
  const message = `us112-csp|${directive}|${blocked}`;
  if (message.length <= 768) window.chrome?.webview?.postMessage(message);
});

void start();
