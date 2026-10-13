// SPDX-License-Identifier: AGPL-3.0-or-later
const checks = [];
const fixture = document.querySelector("#fixture");
const assert = (condition, name) => { if (!condition) throw new Error(name); checks.push(name); };
let report;
try {
  for (const name of ["ui-components", "theme", "ribbon"]) {
    const suite = await import(`/tests/${name}-browser.mjs`);
    await suite.run({ assert, fixture });
    fixture.replaceChildren();
  }
  report = { status: "passed", checks, forcedColors: matchMedia("(forced-colors: active)").matches, userAgent: navigator.userAgent };
} catch (error) {
  report = { status: "failed", checks, error: error.stack ?? String(error), userAgent: navigator.userAgent };
}
document.querySelector("#result").textContent = JSON.stringify(report, null, 2);
await fetch("/__ui_result", { method: "POST", body: JSON.stringify(report) });
