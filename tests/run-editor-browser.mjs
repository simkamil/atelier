// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { engine } = selectEngine("run-editor-browser", "node tests/run-editor-browser.mjs");
const browserCandidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of browserCandidates) {
  try { await access(candidate); executable = candidate; break; } catch { /* try next installed browser */ }
}
assert(executable, "An installed Edge/Chromium browser is required for this check.");
const files = new Map([
  ...["symbol-dialog", "symbol-catalogue", "editor-context-menu", "about-dialog", "about-licenses", "build-info", "properties-dialog"].flatMap(name =>
    [`/${name}.mjs`, `/web/${name}.mjs`].map(url => [url, [`web/${name}.mjs`, "text/javascript; charset=utf-8"]])),
  ...["symbol-dialog", "about-dialog", "properties-dialog", "file-tab", "format-char", "format-para", "document-mirror"].map(name => [`/${name}.css`, [`web/${name}.css`, "text/css; charset=utf-8"]]),
  ["/tests/editor-utilities-integration-browser.mjs", ["tests/editor-utilities-integration-browser.mjs", "text/javascript; charset=utf-8"]],
  ["/page-navigation.mjs", ["web/page-navigation.mjs", "text/javascript; charset=utf-8"]],
  ["/replace-controls.mjs", ["web/replace-controls.mjs", "text/javascript; charset=utf-8"]],
  ["/replace-controls.css", ["web/replace-controls.css", "text/css; charset=utf-8"]],
  ["/document-status.mjs", ["web/document-status.mjs", "text/javascript; charset=utf-8"]],
  ["/document-status.css", ["web/document-status.css", "text/css; charset=utf-8"]],
  ["/web/page-navigation.mjs", ["web/page-navigation.mjs", "text/javascript; charset=utf-8"]],
  ["/web/replace-controls.mjs", ["web/replace-controls.mjs", "text/javascript; charset=utf-8"]],
  ["/web/document-status.mjs", ["web/document-status.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/replacement-browser.mjs", ["tests/replacement-browser.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/document-status-integration-browser.mjs", ["tests/document-status-integration-browser.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/editor-browser.html", ["tests/editor-browser.html", "text/html; charset=utf-8"]],
  ["/tests/editor-browser-entry.mjs", ["tests/editor-browser-entry.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/editor-browser-v2.mjs", ["tests/editor-browser-v2.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/editor-find-browser.mjs", ["tests/editor-find-browser.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/editor-test-adapter.mjs", ["tests/editor-test-adapter.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/zoom-browser.mjs", ["tests/zoom-browser.mjs", "text/javascript; charset=utf-8"]],
  ["/tests/editor-browser-watchdog.mjs", ["tests/editor-browser-watchdog.mjs", "text/javascript; charset=utf-8"]],
  ["/web/app.css", ["web/app.css", "text/css; charset=utf-8"]],
  ["/index.html", ["web/index.html", "text/html; charset=utf-8"]],
  ["/app.mjs", ["web/app.mjs", "text/javascript; charset=utf-8"]],
  ["/app.css", ["web/app.css", "text/css; charset=utf-8"]],
  ["/ui-components.css", ["web/ui-components.css", "text/css; charset=utf-8"]],
  ["/ribbon.css", ["web/ribbon.css", "text/css; charset=utf-8"]],
  ["/theme.css", ["web/theme.css", "text/css; charset=utf-8"]],
  ["/zoom-controls.css", ["web/zoom-controls.css", "text/css; charset=utf-8"]],
  ["/search-pane.css", ["web/search-pane.css", "text/css; charset=utf-8"]],
  ["/statistics-controls.css", ["web/statistics-controls.css", "text/css; charset=utf-8"]],

  ["/i18n/fr-FR.txt", ["web/i18n/fr-FR.txt", "text/plain; charset=utf-8"]],
  ["/atelier_core_wasm.wasm", ["target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "application/wasm"]],
  ["/tests/docx-reference/basic.docx", ["tests/docx-reference/basic.docx", "application/octet-stream"]],
  ...["i18n", "navigation-tools", "search-pane", "statistics-controls", "zoom", "zoom-controls", "editor-protocol", "engine-client", "engine-worker", "wasm-loader", "wasm-protocol", "text-input", "mouse-selection", "text-drag-drop", "french-spacing", "canvas-renderer", "display-list", "image-insert", "native-host", "file-tab", "editor-shell", "ui-components", "ribbon", "theme", "rich-editor", "rich-editor-client", "rich-editor-format", "format-controller", "format-char", "format-para", "rich-image-cache", "rich-editor-protocol", "editor-page-cache", "document-mirror"].map(name =>
    [`/${name}.mjs`, [`web/${name}.mjs`, "text/javascript; charset=utf-8"]]),
  ...["i18n", "navigation-tools", "search-pane", "statistics-controls", "zoom", "zoom-controls", "editor-protocol", "engine-client", "engine-worker", "wasm-loader", "wasm-protocol", "text-input", "mouse-selection", "text-drag-drop", "french-spacing", "canvas-renderer", "display-list", "image-insert", "rich-editor", "rich-editor-client", "rich-editor-format", "format-controller", "format-char", "format-para", "rich-image-cache", "rich-editor-protocol", "editor-page-cache", "document-mirror"].map(name =>
    [`/web/${name}.mjs`, [`web/${name}.mjs`, "text/javascript; charset=utf-8"]]),
  ["/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", ["target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "application/wasm"]],
]);
let resolveReport;
let rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__editor_result") {
      let body = "";
      for await (const chunk of request) {
        body += chunk;
        if (body.length > 65536) throw new Error("Browser report is too large.");
      }
      response.writeHead(200).end("ok");
      resolveReport(JSON.parse(body));
      return;
    }
    const entry = files.get(path);
    if (request.method !== "GET" || !entry) { response.writeHead(404).end(); return; }
    const [relative, mime] = entry;
    const bytes = await readFile(join(root, relative));
    response.writeHead(200, { "Content-Type": mime, "Cache-Control": "no-store" }).end(bytes);
  } catch (error) {
    response.writeHead(500).end("Editor browser harness failed.");
    rejectReport(error);
  }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-editor-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/editor-browser.html?automated=1&engine=${engine}`;
const browser = spawn(executable, ["--headless=new", "--window-size=1280,900", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Editor browser check timed out. ${diagnostics}`)), 120000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert(report.checks.length >= 60, "Editor integration must complete all required scenarios.");
  assert.match(report.userAgent, /(?:Chrome|Edg)\//);
  console.log(JSON.stringify({ ...report, executable }));
} finally {
  clearTimeout(timer);
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(yes => browser.once("exit", yes));
    browser.kill();
    await exited;
  }
  server.closeAllConnections();
  await new Promise(yes => server.close(yes));
  assert(profile.startsWith(resolve(tmpdir()) + sep) && dirname(profile) === resolve(tmpdir()));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
