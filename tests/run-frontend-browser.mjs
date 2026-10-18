// SPDX-License-Identifier: AGPL-3.0-or-later
// Real installed Chromium, vanilla Node HTTP harness; no browser driver package.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { rest: suiteArgs } = selectEngine("run-frontend-browser", "node tests/run-frontend-browser.mjs [all|canvas|input] [--engine v1|v2]",
  rest => rest.length <= 1 && ["all", "canvas", "input"].includes(rest[0] ?? "all"));
const suite = suiteArgs[0] ?? "all";
const candidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of candidates) {
  try { await access(candidate); executable = candidate; break; } catch { /* next */ }
}
assert(executable, "An installed Edge/Chromium browser is required.");
const files = new Map([
  ["/tests/frontend-browser.html", "text/html; charset=utf-8"],
  ...["frontend-browser", "canvas-browser", "input-browser", "text-input-editing"].map(name =>
    [`/tests/${name}.mjs`, "text/javascript; charset=utf-8"]),
  ...["display-list", "canvas-renderer", "text-input"].map(name =>
    [`/web/${name}.mjs`, "text/javascript; charset=utf-8"]),
]);
let resolveReport, rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__frontend_result") {
      let body = "";
      for await (const chunk of request) {
        body += chunk;
        if (body.length > 65536) throw new Error("Browser report too large");
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
    response.writeHead(500).end("Frontend harness request failed");
    rejectReport(error);
  }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-frontend-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/frontend-browser.html?automated=1&suite=${suite}`;
const browser = spawn(executable, ["--headless=new", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-4000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited (${code ?? signal}): ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Frontend check timed out: ${diagnostics}`)), 60000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.deepEqual(report.suites.map(result => result.name),
    suite === "all" ? ["canvas", "input", "composition-integration"] : [suite]);
  assert(report.suites.every(suite => suite.checks.length > 0));
  assert.match(report.userAgent, /(?:Chrome|Edg)\//);
  console.log(JSON.stringify({ ...report, executable,
    environment: "headless software Canvas; synthetic input events are not native keyboard/IME evidence" }));
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
