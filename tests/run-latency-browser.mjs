// SPDX-License-Identifier: AGPL-3.0-or-later
// Lanceur du banc de latence de frappe (tests/latency-browser.mjs) dans Edge/Chromium headless.
// Usage : node tests/run-latency-browser.mjs [--shape lines|dense] [--pages N] [--keys N]
// v2 (R7c) : l'éditeur riche, par `createRichEditor` (document de 300 pages sans limite de 128 Kio). Lancer le banc SEUL, sans autre test ni
// processus lourd : définir ATELIER_BENCH_ALONE=1 pour l'inscrire dans le JSON (`machine.ranAlone`).
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import os, { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const options = { shape: "lines", pages: "300", keys: "300", pairs: "40" };
const usage = "node tests/run-latency-browser.mjs [--shape lines|dense] [--pages N] [--keys N] [--pairs N]";
function parseOptions(rest) {
  const parsed = { ...options };
  for (let i = 0; i < rest.length; i += 2) {
    const name = rest[i]?.replace(/^--/, "");
    if (!Object.hasOwn(options, name) || rest[i + 1] === undefined) return null;
    parsed[name] = rest[i + 1];
  }
  return ["lines", "dense"].includes(parsed.shape) && /^\d+$/.test(parsed.pages) && /^\d+$/.test(parsed.keys) ? parsed : null;
}
const { engine, rest } = selectEngine("run-latency-browser", usage, rest => parseOptions(rest) !== null);
Object.assign(options, parseOptions(rest));
let executable;
for (const candidate of [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe", "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean)) {
  try { await access(candidate); executable = candidate; break; } catch { /* next */ }
}
assert(executable, "An installed Edge/Chromium browser is required for this check.");
const mimes = { ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".html": "text/html; charset=utf-8" };
const files = new Map([["/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm",
  ["target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm", "application/wasm"]],
  ["/i18n/fr-FR.txt", ["web/i18n/fr-FR.txt", "text/plain; charset=utf-8"]]]);
for (const folder of ["web", "tests"]) {
  for (const entry of await readdir(join(root, folder), { withFileTypes: true })) {
    const extension = entry.name.slice(entry.name.lastIndexOf("."));
    if (!entry.isFile() || !mimes[extension]) continue;
    files.set(`/${folder}/${entry.name}`, [`${folder}/${entry.name}`, mimes[extension]]);
    if (folder === "web") files.set(`/${entry.name}`, [`web/${entry.name}`, mimes[extension]]);
  }
}
let resolveReport, rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__editor_result") {
      let body = "";
      for await (const chunk of request) { body += chunk; if (body.length > 65536) throw new Error("Browser report is too large."); }
      response.writeHead(200).end("ok");
      resolveReport(JSON.parse(body));
      return;
    }
    const entry = files.get(path);
    if (request.method !== "GET" || !entry) { response.writeHead(404).end(); return; }
    response.writeHead(200, { "Content-Type": entry[1], "Cache-Control": "no-store" }).end(await readFile(join(root, entry[0])));
  } catch (error) {
    response.writeHead(500).end("Latency harness failed.");
    rejectReport(error);
  }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-latency-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/latency-browser.html?automated=1&engine=${engine}&shape=${options.shape}&pages=${options.pages}&keys=${options.keys}&pairs=${options.pairs}`;
const browser = spawn(executable, ["--headless=new", "--window-size=1280,1000", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Latency check timed out. ${diagnostics}`)), 540000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.match(report.userAgent, /(?:Chrome|Edg)\//);
  console.log(JSON.stringify({ ...report, machine: { ranAlone: process.env.ATELIER_BENCH_ALONE === "1", note: process.env.ATELIER_BENCH_ALONE === "1" ? "banc lancé seul : aucun autre test ni processus lourd en parallèle" : "isolation non déclarée", cpus: os.cpus().length, cpuModel: os.cpus()[0]?.model, platform: `${os.platform()} ${os.release()}`, node: process.version, date: new Date().toISOString() }, executable }));
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
