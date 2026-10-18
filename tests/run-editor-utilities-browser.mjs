// Browser verification for context menu, About dialog, and Unicode symbol dialog.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { engineNote, selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { engine } = selectEngine("run-editor-utilities-browser", "node tests/run-editor-utilities-browser.mjs");
const candidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe", "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of candidates) {
  try { await access(candidate); executable = candidate; break; } catch { /* try next browser */ }
}
assert(executable, "An installed Edge/Chromium browser is required for this check.");

const mimeFor = path => path.endsWith(".mjs") ? "text/javascript; charset=utf-8"
  : path.endsWith(".css") ? "text/css; charset=utf-8" : "text/html; charset=utf-8";
const files = new Map();
for (const folder of ["web", "tests"]) {
  for (const entry of await readdir(join(root, folder), { withFileTypes: true })) {
    if (!entry.isFile() || !/\.(?:mjs|css|html)$/.test(entry.name)) continue;
    const path = `${folder}/${entry.name}`;
    files.set(`/${path}`, [path, mimeFor(path)]);
    if (folder === "web") files.set(`/${entry.name}`, [path, mimeFor(path)]);
  }
}
// Non-source resources are explicit routes; never expose arbitrary binary files.
for (const [url, path, mime] of [
  ["/i18n/fr-FR.txt", "web/i18n/fr-FR.txt", "text/plain; charset=utf-8"],
  ["/atelier_core_wasm.wasm", "data/engine/atelier_core_wasm.wasm", "application/wasm"],
  ["/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "application/wasm"],
  ["/fonts/Carlito-Regular.ttf", "data/fonts/carlito/Carlito-Regular.ttf", "font/ttf"],
  ["/fonts/Caladea-Regular.ttf", "data/fonts/caladea/Caladea-Regular.ttf", "font/ttf"],
]) files.set(url, [path, mime]);

let resolveReport, rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__utilities_result") {
      let body = "";
      for await (const chunk of request) { body += chunk; if (body.length > 100_000) throw new Error("Browser report is too large."); }
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
    response.writeHead(500).end("Editor utilities browser harness failed.");
    rejectReport(error);
  }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-editor-utilities-"));
const url = `http://127.0.0.1:${server.address().port}/tests/editor-utilities-browser.html?automated=1`;
const browser = spawn(executable, ["--headless=new", "--window-size=1280,900", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Editor utilities browser check timed out. ${diagnostics}`)), 120000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert(report.checks.length >= 30, "All editor utility browser suites must execute.");
  assert.match(report.userAgent, /(?:Chrome|Edg)\//);
  console.log(JSON.stringify({ ...report, engine, note: engineNote("run-editor-utilities-browser"), executable }));
} finally {
  clearTimeout(timer);
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(yes => browser.once("exit", yes)); browser.kill(); await exited;
  }
  server.closeAllConnections();
  await new Promise(yes => server.close(yes));
  assert(profile.startsWith(resolve(tmpdir()) + sep) && dirname(profile) === resolve(tmpdir()));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
