// SPDX-License-Identifier: AGPL-3.0-or-later
// Point d'entrée de tests/editor-browser.html : le scénario v1 (editor-browser.mjs, inchangé) ou le scénario v2 (R7a).
const v2 = new URLSearchParams(location.search).get("engine") === "v2";
await import(v2 ? "./editor-browser-v2.mjs" : "./editor-browser.mjs");
