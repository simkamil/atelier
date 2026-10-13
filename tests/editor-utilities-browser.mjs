// SPDX-License-Identifier: AGPL-3.0-or-later
import { run as checkEditorContextMenu } from "./editor-context-menu-browser.mjs";
import { run as checkAboutDialog } from "./about-dialog-browser.mjs";
import { run as checkSymbolDialog } from "./symbol-dialog-browser.mjs";

const checks = [];
function assert(condition, message) {
  if (!condition) throw new Error(message);
  checks.push(message);
}

let report;
try {
  const runSuite = async suite => {
    const fixture = document.createElement("section");
    document.querySelector("#fixture").append(fixture);
    try { await suite({ assert, fixture }); }
    finally { fixture.remove(); }
  };
  await runSuite(checkEditorContextMenu);
  await runSuite(checkAboutDialog);
  await runSuite(checkSymbolDialog);
  report = { status: "passed", userAgent: navigator.userAgent, checks };
} catch (error) {
  report = { status: "failed", message: error?.stack ?? String(error), userAgent: navigator.userAgent, checks };
} finally {
  document.querySelector("#result").textContent = JSON.stringify(report, null, 2);
  if (new URL(location.href).searchParams.get("automated") === "1") {
    await fetch("/__utilities_result", { method: "POST", body: JSON.stringify(report) });
  }
}
