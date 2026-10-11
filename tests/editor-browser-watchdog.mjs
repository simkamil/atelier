// SPDX-License-Identifier: AGPL-3.0-or-later
let reported = false;
async function send(message) {
  if (reported || new URL(location.href).searchParams.get("automated") !== "1") return;
  reported = true;
  try { await fetch("/__editor_result", { method: "POST", body: JSON.stringify({ status: "failed", message }) }); } catch { /* runner may already be gone */ }
}
addEventListener("error", event => { void send(event.error?.stack ?? event.message ?? "Editor page error"); });
addEventListener("unhandledrejection", event => { void send(event.reason?.stack ?? String(event.reason)); });
setTimeout(() => { void send("Editor test script did not finish within 90 seconds."); }, 90_000);
