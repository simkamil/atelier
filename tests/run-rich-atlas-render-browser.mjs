// SPDX-License-Identifier: AGPL-3.0-or-later
// R2b: rendu des pages de l'éditeur riche par atlas de glyphes dans Edge/Chromium headless, comparé au
// pixel près (tolérance zéro) à l'ancien dessin FillPath. Les pages viennent du vrai WASM
// (`node tests/run-rich-atlas-render-browser.mjs [engine.wasm]`).
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { crc32 } from "node:zlib";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { decodeRichReply, encodeRichRequest } from "../web/rich-editor-protocol.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const candidates = [process.env.ATELIER_BROWSER_EXECUTABLE,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/chromium", "/usr/bin/google-chrome"].filter(Boolean);
let executable;
for (const candidate of candidates) { try { await access(candidate); executable = candidate; break; } catch { /* suivant */ } }
assert(executable, "An installed Edge/Chromium browser is required for this check.");

// ---- Engine (real WASM) -------------------------------------------------------------------
const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const { memory, alloc, dispatch, free } = new WebAssembly.Instance(await WebAssembly.compile(await readFile(resolve(root, wasmPath)))).exports;
let outerId = 1n;
let innerId = 1;
function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const bodyPtr = descriptor.getUint32(0, true);
  const bodyLen = descriptor.getUint32(4, true);
  const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
  free(bodyPtr, bodyLen); free(descriptorPtr, 8);
  return body.slice(4);
}
/** Sends one command; returns the decoded response and records the raw reply frame in `frames`. */
function send(docId, command, frames) {
  const payload = encodeRichRequest({ docId, requestId: innerId++, command });
  const message = decodeMessage(invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload })), "event");
  assert.equal(message.type, MessageType.EditorResult, new TextDecoder().decode(message.payload));
  frames.push(Buffer.from(message.payload).toString("base64"));
  return decodeRichReply(message.payload).response;
}

// ---- A stored-ZIP DOCX with the formatting the editor draws ------------------------------------
function zipStored(files) {
  const enc = new TextEncoder();
  const locals = []; const centrals = []; let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const nameBytes = enc.encode(name); const data = enc.encode(text);
    const crc = crc32(data);
    const local = Buffer.alloc(30 + nameBytes.length + data.length);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(nameBytes.length, 26);
    nameBytes.forEach((b, i) => { local[30 + i] = b; }); data.forEach((b, i) => { local[30 + nameBytes.length + i] = b; });
    const central = Buffer.alloc(46 + nameBytes.length);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6); central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24); central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(offset, 42); nameBytes.forEach((b, i) => { central[46 + i] = b; });
    locals.push(local); centrals.push(central); offset += local.length;
  }
  const directory = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(centrals.length, 8); end.writeUInt16LE(centrals.length, 10);
  end.writeUInt32LE(directory.length, 12); end.writeUInt32LE(offset, 16);
  return new Uint8Array(Buffer.concat([...locals, directory, end]));
}
const run = (text, props = "") => `<w:r>${props ? `<w:rPr>${props}</w:rPr>` : ""}<w:t xml:space="preserve">${text}</w:t></w:r>`;
const para = (...runs) => `<w:p>${runs.join("")}</w:p>`;
const filler = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. ";
function styledDocx(custom) {
  const body = [];
  if (custom) body.push(...custom);
  else for (let i = 0; i < 6; i += 1) {
    body.push(para(
      run(filler), run("Gras. ", "<w:b/>"), run("Italique. ", "<w:i/>"), run("Gras italique. ", "<w:b/><w:i/>"),
      run("Souligné coloré. ", '<w:u w:val="single"/><w:color w:val="4472C4"/>'), run("Grand. ", '<w:sz w:val="40"/>'),
      run("Autre famille. ", '<w:rFonts w:ascii="No Such Family" w:hAnsi="No Such Family"/>'), run(filler),
    ));
    body.push(para(
      run("Accents é è ê ë à ù ç œ « » — ½ © ® ™ € £ ¥ § ¶ † ‡ • … ñ ü ö ä ß Ω Σ π ∑ √"),
      '<w:r><w:sym w:font="Symbol" w:char="F0B7"/></w:r>', run(" après le symbole. "),
      "<w:r><w:br/></w:r>", run("Après saut de ligne forcé. "),
    ));
    body.push(para(run("Avant saut de page. "), '<w:r><w:br w:type="page"/></w:r>', run("Après saut de page ", "<w:b/>")));
  }
  const ns = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
  return zipStored({
    "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document ${ns}><w:body>${body.join("")}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>`,
  });
}

/** Opens a document, lays it out, fetches every page, edits and fetches again; returns the replies in order. */
function capture(name, open, edit) {
  const frames = [];
  const docId = 1;
  let state = send(docId, { type: "open", ...open }, frames);
  assert.equal(state.type, "state", state.code);
  for (let i = 0; i < 4000 && !state.layoutComplete; i += 1) state = send(docId, { type: "layout", budget: 256 }, frames);
  assert.ok(state.layoutComplete, `${name}: layout complete`);
  const pageCount = state.pageCount;
  for (let first = 0; first < pageCount; first += 1) {
    assert.equal(send(docId, { type: "pages", first, count: 1 }, frames).type, "pages");
  }
  if (edit) {
    send(docId, { type: "setSelection", anchor: 0n, focus: 0n, upstream: false }, frames);
    let edited = send(docId, { type: "insertText", text: edit }, frames);
    assert.equal(edited.type, "state");
    for (let i = 0; i < 4000 && !edited.layoutComplete; i += 1) edited = send(docId, { type: "layout", budget: 256 }, frames);
    send(docId, { type: "pages", first: 0, count: 1 }, frames);
  }
  return { name, frames, pages: Array.from({ length: Math.min(pageCount, 8) }, (_, i) => i), pageCount };
}

// One family at several close sizes (the key of an atlas entry holds the size in 1/64 pt), and pages
// that do not have the same number of glyphs (one glyph, a few, a full page, a short last line).
const closeSizes = [20, 21, 22, 23, 24, 25, 26, 28, 32].flatMap((half) => [
  para(run(`Taille ${half / 2} pt : Wagons, Ægir &amp; fjords 0123456789 ${filler}`, `<w:sz w:val="${half}"/>`)),
  para(run(`Gras ${half / 2} pt : mmm www iii lll ${filler}`, `<w:b/><w:sz w:val="${half}"/>`)),
]);
const uneven = [
  para(run("A")), para('<w:r><w:br w:type="page"/></w:r>'),
  ...Array.from({ length: 40 }, () => para(run(filler + filler))),
  para(run("Fin.")), para('<w:r><w:br w:type="page"/></w:r>'), para(run("é")),
];
const letters = "mwgQ@&%$B8GRSgsy";
const dense = Array.from({ length: 60 }, (_, i) => Array.from({ length: 100 }, (_, k) => letters[(i * 7 + k * 5) % letters.length]).join("")).join("\n");
const captures = [
  capture("txt dense", { kind: "txt", profile: "txt", data: new TextEncoder().encode(dense) }),
  capture("txt edited", { kind: "txt", profile: "txt", data: new TextEncoder().encode("Bonjour le monde, ceci est un essai.\nDeuxième ligne avec des accents éàù.") }, "Zéro Æ — "),
  capture("docx styles", { kind: "docx", profile: "document", data: styledDocx() }),
  capture("docx close sizes", { kind: "docx", profile: "document", data: styledDocx(closeSizes) }),
  capture("docx uneven pages", { kind: "docx", profile: "document", data: styledDocx(uneven) }),
];
for (const c of captures) console.log(`captured ${c.name}: ${c.pageCount} pages, ${c.frames.length} replies`);

// ---- Browser ------------------------------------------------------------------------------
const MIME = { ".mjs": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".html": "text/html; charset=utf-8" };
function resolveFile(path) {
  const clean = path.replace(/^\/+/, "");
  if (clean.includes("..") || clean.includes("\\")) return null;
  if (clean.startsWith("tests/") && /^[\w.-]+\.(mjs|html)$/.test(clean.slice(6))) return clean;
  const web = clean.startsWith("web/") ? clean : `web/${clean}`;
  return /^web\/[\w-]+\.(?:mjs|css)$/.test(web) ? web : null;
}
let resolveReport; let rejectReport;
const reportPromise = new Promise((yes, no) => { resolveReport = yes; rejectReport = no; });
const server = createServer(async (request, response) => {
  try {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (request.method === "GET" && path === "/__captures") {
      response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(captures)); return;
    }
    if (request.method === "POST" && path === "/__atlas_result") {
      let body = "";
      for await (const chunk of request) { body += chunk; if (body.length > 1 << 20) throw new Error("Report too large."); }
      response.writeHead(200).end("ok"); resolveReport(JSON.parse(body)); return;
    }
    const relative = request.method === "GET" ? resolveFile(path) : null;
    if (!relative) { response.writeHead(404).end(); return; }
    response.writeHead(200, { "Content-Type": MIME[extname(relative)] ?? "application/octet-stream", "Cache-Control": "no-store" }).end(await readFile(join(root, relative)));
  } catch (error) { response.writeHead(500).end("harness failed"); rejectReport(error); }
});
await new Promise((yes, no) => { server.once("error", no); server.listen(0, "127.0.0.1", yes); });
const profile = await mkdtemp(join(tmpdir(), "atelier-atlas-browser-"));
const url = `http://127.0.0.1:${server.address().port}/tests/rich-atlas-render-browser.html`;
const browser = spawn(executable, ["--headless=new", "--window-size=1400,1000", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
  "--disable-background-networking", "--disable-component-update", `--user-data-dir=${profile}`, url],
{ stdio: ["ignore", "ignore", "pipe"], windowsHide: true });
let diagnostics = "";
browser.stderr.on("data", (chunk) => { diagnostics = (diagnostics + chunk).slice(-5000); });
browser.once("error", rejectReport);
browser.once("exit", (code, signal) => rejectReport(new Error(`Browser exited before reporting (${code ?? signal}). ${diagnostics}`)));
const timer = setTimeout(() => rejectReport(new Error(`Atlas render check timed out. ${diagnostics}`)), 240000);
try {
  const report = await reportPromise;
  for (const p of report.pages ?? []) console.log(`${p.name} page ${p.page} zoom ${p.zoom}: ${p.glyphs} glyphs, ${p.inked} inked pixels, ${p.different} different pixels (max channel difference ${p.max})`);
  for (const note of report.notes ?? []) console.log(`note: ${note}`);
  assert.equal(report.status, "passed", JSON.stringify(report).slice(0, 2000));
  console.log(JSON.stringify({ status: "passed", comparisons: report.pages.length, tolerance: 0, executable }));
} finally {
  clearTimeout(timer);
  if (browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise((yes) => browser.once("exit", yes));
    browser.kill(); await exited;
  }
  server.closeAllConnections();
  await new Promise((yes) => server.close(yes));
  assert(profile.startsWith(resolve(tmpdir()) + sep));
  await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
