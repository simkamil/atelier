// SPDX-License-Identifier: AGPL-3.0-or-later
// Run the production browser entry with installed Chromium; no npm dependencies.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const candidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of candidates) {
  try { await access(candidate); executable = candidate; break; } catch { /* try next */ }
}
assert(executable, "An installed Edge/Chromium browser is required for this check.");
const files = new Map([
  ["/tests/worker-browser.html", "text/html; charset=utf-8"],
  ["/tests/worker-browser.mjs", "text/javascript; charset=utf-8"],
  ...["engine-client", "engine-worker", "wasm-loader", "wasm-protocol"].map(name =>
    [`/web/${name}.mjs`, "text/javascript; charset=utf-8"]),
  ["/tests/docx-reference/basic.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
  ["/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "application/wasm"],
]);
let resolveReport;
let rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__worker_result") {
      let body = "";
      for await (const chunk of request) {
        body += chunk;
        if (body.length > 65536) throw new Error("Browser report is too large.");
      }
      const report = JSON.parse(body);
      response.writeHead(200).end("ok");
      resolveReport(report);
      return;
    }
    if (request.method !== "GET" || !files.has(path)) {
      response.writeHead(404).end();
      return;
    }
    const bytes = await readFile(join(root, path.slice(1)));
    response.writeHead(200, { "Content-Type": files.get(path), "Cache-Control": "no-store" }).end(bytes);
  } catch (error) {
    response.writeHead(500).end("Browser harness request failed.");
    rejectReport(error);
  }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-worker-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/worker-browser.html?automated=1`;
const browser = spawn(executable, ["--headless=new", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-4000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Browser check timed out. ${diagnostics}`)), 60000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.equal(report.checks.length, 9, "Every browser scenario must complete.");
  assert.match(report.userAgent, /(?:Chrome|Edg)\//);
  assert.equal(report.crossOriginIsolated, false, "Worker must work without SharedArrayBuffer isolation.");
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
