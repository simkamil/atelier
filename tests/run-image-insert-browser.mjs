// SPDX-License-Identifier: AGPL-3.0-or-later
// US-270 : insertion d'images dans le vrai produit (Edge/Chromium, Worker, WASM optimisé).
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { engine } = selectEngine("run-image-insert-browser", "node tests/run-image-insert-browser.mjs");
const candidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of candidates) { try { await access(candidate); executable = candidate; break; } catch { /* suivant */ } }
assert(executable, "An installed Edge/Chromium browser is required for this check.");
const WASM = "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const MIME = { ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".html": "text/html; charset=utf-8",
  ".txt": "text/plain; charset=utf-8", ".wasm": "application/wasm" };
/** Fichiers servis : web/ à la racine (comme l'application), puis tests/ et le WASM optimisé. */
function resolveFile(path) {
  if (path === WASM || path === `/${WASM}`) return WASM;
  if (path === "/atelier_core_wasm.wasm") return WASM;
  const clean = path.replace(/^\/+/, "");
  if (clean.includes("..") || clean.includes("\\")) return null;
  if (clean.startsWith("tests/") && /^[\w.-]+\.(mjs|html)$/.test(clean.slice(6))) return clean;
  const web = clean.startsWith("web/") ? clean : `web/${clean || "index.html"}`;
  return /^web\/(?:[\w-]+\.(?:mjs|css|html)|i18n\/fr-FR\.txt)$/.test(web) ? web : null;
}
let resolveReport; let rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__image_result") {
      let body = "";
      for await (const chunk of request) { body += chunk; if (body.length > 65536) throw new Error("Report too large."); }
      response.writeHead(200).end("ok"); resolveReport(JSON.parse(body)); return;
    }
    const relative = request.method === "GET" ? resolveFile(path) : null;
    if (!relative) { response.writeHead(404).end(); return; }
    const bytes = await readFile(join(root, relative));
    response.writeHead(200, { "Content-Type": MIME[extname(relative)] ?? "application/octet-stream", "Cache-Control": "no-store" }).end(bytes);
  } catch (error) { response.writeHead(500).end("harness failed"); rejectReport(error); }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-image-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/image-insert-browser.html?automated=1&engine=${engine}`;
const browser = spawn(executable, ["--headless=new", "--window-size=1280,900", "--disable-gpu", "--no-first-run",
  "--no-default-browser-check", "--disable-background-networking", "--disable-component-update",
  `--user-data-dir=${profile}`, url], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Image browser check timed out. ${diagnostics}`)), 120000);
try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert(report.checks.length >= 25, "all image scenarios must complete");
  console.log(JSON.stringify({ ...report, executable }));
} finally {
  clearTimeout(timer);
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(yes => browser.once("exit", yes));
    browser.kill(); await exited;
  }
  server.closeAllConnections();
  await new Promise(yes => server.close(yes));
  assert(profile.startsWith(resolve(tmpdir()) + sep));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
