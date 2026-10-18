// SPDX-License-Identifier: AGPL-3.0-or-later
// Installed Chromium and its debugging protocol, with no third-party driver.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { selectEngine } from "./engine-adapter.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { engine, rest: args } = selectEngine("run-ui-browser", "node tests/run-ui-browser.mjs [--forced-colors] [--screenshots] [--engine v1|v2]",
  rest => rest.every(arg => ["--forced-colors", "--screenshots"].includes(arg)));
const v2 = engine === "v2";
const engineQuery = `engine=${engine}`;
const skipped = [];
const forcedColors = args.includes("--forced-colors");
const screenshots = args.includes("--screenshots");
let executable;
for (const path of [process.env.ATELIER_BROWSER_EXECUTABLE, "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Microsoft/Edge/Application/msedge.exe", "/usr/bin/chromium"].filter(Boolean)) {
  try { await access(path); executable = path; break; } catch { /* try next installed browser */ }
}
assert(executable, "Installed Edge/Chromium required");
const files = new Map();
for (const folder of ["web", "tests"]) {
  for (const entry of await readdir(join(root, folder), { withFileTypes: true })) {
    if (!entry.isFile() || !/\.(?:mjs|css|html)$/.test(entry.name)) continue;
    const path = `${folder}/${entry.name}`;
    const mime = path.endsWith(".mjs") ? "text/javascript; charset=utf-8" : path.endsWith(".css") ? "text/css; charset=utf-8" : "text/html; charset=utf-8";
    files.set(`/${path}`, [path, mime]);
    if (folder === "web") files.set(`/${entry.name}`, [path, mime]);
  }
}
files.set("/i18n/fr-FR.txt", ["web/i18n/fr-FR.txt", "text/plain; charset=utf-8"]);
files.set("/fonts/Carlito-Regular.ttf", ["data/fonts/carlito/Carlito-Regular.ttf", "font/ttf"]);
files.set("/atelier_core_wasm.wasm", ["data/engine/atelier_core_wasm.wasm", "application/wasm"]);
let resolveReport, rejectReport;
let unavailableCatalog = false;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "POST" && path === "/__ui_result") {
      let body = "";
      for await (const chunk of request) { body += chunk; if (body.length > 100000) throw new Error("Report too large"); }
      response.writeHead(200).end("ok"); resolveReport(JSON.parse(body)); return;
    }
    const entry = files.get(path);
    if (path === "/i18n/fr-FR.txt" && unavailableCatalog === "unavailable") { response.writeHead(503).end(); return; }
    if (request.method !== "GET" || !entry) { response.writeHead(404).end(); return; }
    let bytes = await readFile(join(root, entry[0]));
    if (path === "/i18n/fr-FR.txt" && unavailableCatalog === "partial") bytes = Buffer.from(bytes.toString("utf8").replace(/^searchQuery=.*\r?\n/gm, ""));
    response.writeHead(200, { "Content-Type": entry[1], "Cache-Control": "no-store" }).end(bytes);
  } catch (error) { response.writeHead(500).end("UI harness failed"); rejectReport(error); }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const origin = `http://127.0.0.1:${server.address().port}`;
const profile = await mkdtemp(join(tmpdir(), "atelier-ui-browser-"));
const browser = spawn(executable, ["--headless=new", "--window-size=1280,900", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--disable-background-networking", "--disable-component-update", "--remote-debugging-port=0", ...(forcedColors ? ["--force-high-contrast"] : []), `--user-data-dir=${profile}`, `${origin}/tests/ui-browser.html`], { stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", bytes => { diagnostics = (diagnostics + bytes).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited ${code ?? signal}: ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`UI browser timed out: ${diagnostics}`)), 60000);

async function captureShell() {
  const [port] = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).split(/\r?\n/);
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(20000) })).json();
  const target = targets.find(target => target.type === "page");
  assert(target, "CDP page available");
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((yes, no) => {
    const timeout = setTimeout(() => { socket.close(); no(new Error("CDP connection timed out")); }, 20000);
    socket.addEventListener("open", () => { clearTimeout(timeout); yes(); }, { once: true });
    socket.addEventListener("error", error => { clearTimeout(timeout); no(error); }, { once: true });
  });
  let id = 0;
  const pending = new Map();
  socket.addEventListener("message", event => {
    const reply = JSON.parse(event.data);
    const waiting = pending.get(reply.id);
    if (waiting) { pending.delete(reply.id); clearTimeout(waiting.timeout); reply.error ? waiting.reject(new Error(JSON.stringify(reply.error))) : waiting.resolve(reply.result); }
  });
  socket.addEventListener("close", () => {
    for (const waiting of pending.values()) { clearTimeout(waiting.timeout); waiting.reject(new Error("CDP connection closed")); }
    pending.clear();
  });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const current = ++id;
    const timeout = setTimeout(() => { pending.delete(current); reject(new Error(`CDP ${method} timed out`)); }, 20000);
    pending.set(current, { resolve, reject, timeout }); socket.send(JSON.stringify({ id: current, method, params }));
  });
  const evaluate = async expression => {
    const value = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    assert(!value.exceptionDetails, JSON.stringify(value.exceptionDetails));
    return value.result.value;
  };
  const output = join(root, "target/ui-screenshots");
  if (screenshots) await mkdir(output, { recursive: true });
  try {
    await send("Page.navigate", { url: `${origin}/index.html?engine=${engine}` });
    for (let tries = 0; tries < 100; tries++) {
      try { if (await evaluate("document.readyState === 'complete' && !!document.getElementById('ribbon')")) break; } catch { /* navigation context */ }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    const state = await evaluate("(async()=>{ const {editorReady}=await import('/app.mjs'); const app=await editorReady; window.atelierScreenshotApp=app; return app.status; })()");
    assert.equal(state, "ready", "real shell starts for screenshots");
    if (forcedColors) assert(await evaluate("['.mark','.ribbon-command.primary'].every(selector=>{const style=getComputedStyle(document.querySelector(selector)); return style.forcedColorAdjust==='none' && style.color!==style.backgroundColor;})"),
      "primary button and mark retain readable explicit system color pairs without automatic backplates");
    await evaluate("window.atelierScreenshotApp.theme.setTheme('system')");
    for (const palette of ["dark", "light"]) {
      await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: palette }] });
      let resolved;
      for (let tries = 0; tries < 100; tries++) {
        resolved = await evaluate("window.atelierScreenshotApp.theme.getResolvedTheme()");
        if (resolved === palette) break;
        await new Promise(resolve => setTimeout(resolve, 20));
      }
      assert.equal(resolved, palette, "real browser media change updates the system theme");
    }
    await evaluate("window.atelierScreenshotApp.editor.element.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertText',data:'Un document à lire. Un document à rechercher.',bubbles:true,cancelable:true}))");
    for (const [name, width, theme] of [["light", 1280, "light"], ["dark", 1280, "dark"], ["narrow", 420, "light"]]) {
      await send("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false });
      await evaluate(`window.atelierScreenshotApp.theme.setTheme('${theme}')`);
      await new Promise(resolve => setTimeout(resolve, 100));
      assert(await evaluate("document.documentElement.scrollWidth <= innerWidth"), `${name} shell has no horizontal overflow`);
      if (screenshots) {
        const shot = await send("Page.captureScreenshot", { format: "png" });
        await writeFile(join(output, `${forcedColors ? "forced-" : ""}${name}.png`), Buffer.from(shot.data, "base64"));
      }
      {
        await evaluate("(()=>{window.atelierScreenshotApp.navigation.openSearch(); const q=document.querySelector('[data-search-query]');q.value='document';q.dispatchEvent(new Event('input',{bubbles:true}));})()");
        for (let tries=0;tries<100;tries++) {
          if (await evaluate("window.atelierScreenshotApp.navigation.searchResult?.items.length===2")) break;
          await new Promise(resolve=>setTimeout(resolve,20));
        }
        assert(await evaluate("window.atelierScreenshotApp.navigation.searchResult?.items.length===2 && document.documentElement.scrollWidth<=innerWidth && document.body.scrollHeight<=innerHeight"), `${name} open search pane fits the real shell`);
        if (screenshots) {
          await evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
          const shot=await send("Page.captureScreenshot",{format:"png"});
          await writeFile(join(output,`${forcedColors ? "forced-" : ""}${name}-search.png`),Buffer.from(shot.data,"base64"));
        }
        await evaluate("document.querySelector('[data-search-close]').click(); window.atelierScreenshotApp.navigation.openStatistics();");
        await new Promise(resolve=>setTimeout(resolve,180));
        assert(await evaluate("(()=>{const d=document.querySelector('[data-statistics-dialog]');const r=d.getBoundingClientRect();return d.open&&r.left>=0&&r.right<=innerWidth&&r.top>=0&&r.bottom<=innerHeight;})()"),`${name} statistics dialog stays inside the viewport`);
        if (screenshots) {
          await evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
          const shot=await send("Page.captureScreenshot",{format:"png"});
          await writeFile(join(output,`${forcedColors ? "forced-" : ""}${name}-statistics.png`),Buffer.from(shot.data,"base64"));
        }
        await evaluate("document.querySelector('[data-statistics-dialog] .ui-dialog-close').click()");
        await evaluate("window.atelierScreenshotApp.navigation.openReplace();document.querySelector('[data-replace-text]').scrollIntoView({block:'nearest'})");
        await new Promise(resolve=>setTimeout(resolve,180));
        assert(await evaluate("(()=>{const c=document.querySelector('.replace-controls');const r=c.getBoundingClientRect();return !c.hidden&&r.left>=0&&r.right<=innerWidth&&document.documentElement.scrollWidth<=innerWidth&&document.body.scrollHeight<=innerHeight;})()"),`${name} replacement controls fit the actual viewport`);
        if (screenshots) {
          await evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
          const shot=await send("Page.captureScreenshot",{format:"png"});
          await writeFile(join(output,`${forcedColors ? "forced-" : ""}${name}-replace.png`),Buffer.from(shot.data,"base64"));
        }
      }
      await evaluate("document.querySelector('[data-search-close]').click();document.querySelector('.document-status-page').click()");
      assert(await evaluate("(()=>{const d=document.querySelector('[data-status-goto-dialog]');const r=d.getBoundingClientRect();return d.open&&r.left>=0&&r.right<=innerWidth&&r.top>=0&&r.bottom<=innerHeight;})()"),`${name} page dialog fits the viewport`);
      if (screenshots) {
        await evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
        const shot=await send("Page.captureScreenshot",{format:"png"});
        await writeFile(join(output,`${forcedColors ? "forced-" : ""}${name}-goto.png`),Buffer.from(shot.data,"base64"));
      }
      await evaluate("document.querySelector('[data-goto-cancel]').click();new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
      await evaluate("(()=>{const f=document.querySelector('footer');f.focus();f.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));})()");
      assert(await evaluate("(()=>{const m=document.querySelector('[data-status-visibility-menu]');const r=m.getBoundingClientRect();return !m.hidden&&m.querySelectorAll('[role=menuitemcheckbox]').length===6&&r.left>=0&&r.right<=innerWidth&&r.top>=0&&r.bottom<=innerHeight;})()"),`${name} six-field status menu fits the viewport`);
      if (screenshots) {
        await evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
        const shot=await send("Page.captureScreenshot",{format:"png"});
        await writeFile(join(output,`${forcedColors ? "forced-" : ""}${name}-status-menu.png`),Buffer.from(shot.data,"base64"));
      }
      await evaluate("document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}))");
      assert(await evaluate("document.querySelector('[data-status-visibility-menu]').hidden"), "status menu closes before the next layout scenario");
    }
    for (const catalogFailure of ["unavailable", "partial"]) {
    unavailableCatalog = catalogFailure;
    await send("Page.navigate", { url: `${origin}/index.html?catalog-${catalogFailure}&engine=${engine}` });
    for (let tries=0;tries<100;tries++) {
      try { if (await evaluate(`location.search==='?catalog-${catalogFailure}&engine=${engine}' && document.readyState==='complete' && !!document.getElementById('ribbon')`)) break; } catch { /* navigation context */ }
      await new Promise(resolve=>setTimeout(resolve,50));
    }
    assert(await evaluate("(async()=>{const app=await (await import('/app.mjs')).editorReady;if(app.status!=='ready'||app.navigation||document.querySelector('#search-command,#statistics-command,.search-pane,[data-statistics-status]'))return false;app.editor.element.dispatchEvent(new InputEvent('beforeinput',{inputType:'insertText',data:'secours',bubbles:true,cancelable:true}));await app.exportDocument();return "+(v2 ? "(await app.editor.readText())==='secours'" : "app.getState().text==='secours'")+";})()"),
      "catalog failure preserves legacy editing without exposing untranslated navigation controls");
    }
  } finally { socket.close(); }
}

try {
  const report = await reportPromise;
  assert.equal(report.status, "passed", JSON.stringify(report));
  assert.equal(report.forcedColors, forcedColors, "forced-colors browser mode is real");
  assert(report.checks.length >= 20, "all UI suites must execute");
  await captureShell();
  report.checks.push("real packaged app follows live browser system-theme media changes", "real packaged shell fits 1280px and 420px without horizontal overflow");
  report.checks.push("real search pane and statistics dialog fit 1280px and 420px in light/dark themes");
  report.checks.push("replacement, page dialog and six-field status menu fit 1280px and 420px in light/dark themes");
  report.checks.push("missing or partial catalog keeps editing available and omits untranslated navigation tools");
  if (forcedColors) report.checks.push("primary button and mark preserve readable forced-colors system pairs");
  console.log(JSON.stringify({ ...report, engine, skipped: [...new Set(skipped.map(item => item.replace(/^[a-z]+: /, "")))], executable, screenshots, environment: "real DOM; synthetic keyboard events, not physical keyboard or screen-reader evidence" }));
} finally {
  clearTimeout(timer);
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(resolve => browser.once("exit", resolve)); browser.kill(); await exited;
  }
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  assert(profile.startsWith(resolve(tmpdir()) + sep) && dirname(profile) === resolve(tmpdir()));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
