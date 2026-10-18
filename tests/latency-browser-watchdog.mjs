// SPDX-License-Identifier: AGPL-3.0-or-later
// Comme editor-browser-watchdog.mjs, sans délai de 90 s : le banc est long ; le lanceur borne la durée (9 min).
let reported = false;
async function send(message) {
  if (reported) return;
  reported = true;
  try { await fetch("/__editor_result", { method: "POST", body: JSON.stringify({ status: "failed", message }) }); } catch { /* lanceur absent */ }
}
addEventListener("error", event => { void send(event.error?.stack ?? event.message ?? "Latency page error"); });
addEventListener("unhandledrejection", event => { void send(event.reason?.stack ?? String(event.reason)); });
