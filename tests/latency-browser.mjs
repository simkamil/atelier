// SPDX-License-Identifier: AGPL-3.0-or-later
// Banc de latence de frappe de bout en bout (US-219) : événement clavier -> fin du dessin Canvas.
// Instrumentation de test seulement : aucun code produit n'est modifié. Le dessin du Canvas est synchrone
// dans `publish()` de `web/editor.mjs` (setPages -> render, puis updateViewport) ; le rappel `onState`
// s'exécute juste après, donc son horodatage marque la fin du dessin. `untilNextFrameMs` ajoute l'attente
// de l'image d'animation suivante (présentation).
import { createEditor } from "../web/editor.mjs";
import { createEngineClient } from "../web/engine-client.mjs";

const params = new URL(location.href).searchParams;
const TARGET_PAGES = Number(params.get("pages") ?? 300);
const KEYS = Number(params.get("keys") ?? 300);
// lines : TARGET_PAGES pages de lignes courtes ; dense : texte plein au plafond de l'éditeur v1.
const SHAPE = params.get("shape") ?? "lines";
const MAX_TEXT = 128 * 1024; // plafond de l'éditeur v1 (MAX_TEXT dans crates/core-wasm/src/editor.rs)
const MARKER = "MILIEU";
const encoder = new TextEncoder();

// Générateur congruentiel : le document est identique à chaque exécution.
function rng(seed) { let s = seed >>> 0; return () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 2 ** 32; }
const WORDS = ["atelier", "page", "texte", "ligne", "mot", "carte", "bloc", "idee", "plan", "note", "ciel", "mer"];
function shortLines(count) {
  const next = rng(42), lines = [];
  for (let i = 0; i < count; i += 1) lines.push(WORDS[Math.floor(next() * WORDS.length)].slice(0, 3 + Math.floor(next() * 5)));
  lines[count >> 1] = MARKER;
  return lines.join("\n");
}
function denseText(bytes) {
  const next = rng(7); const parts = []; let size = 0;
  while (size < bytes) {
    const sentence = Array.from({ length: 8 + Math.floor(next() * 12) }, () => WORDS[Math.floor(next() * WORDS.length)]).join(" ") + ". ";
    parts.push(sentence); size += sentence.length;
  }
  parts[parts.length >> 1] = `${MARKER} ${parts[parts.length >> 1]}`;
  return parts.join("").slice(0, bytes);
}
function stats(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const at = p => sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)];
  const r = x => Math.round(x * 1000) / 1000;
  return { n: values.length, mean: r(values.reduce((a, b) => a + b, 0) / values.length), p50: r(at(0.5)), p95: r(at(0.95)), max: r(sorted.at(-1)) };
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const frame = () => new Promise(resolve => requestAnimationFrame(() => resolve(performance.now())));

let report, editor, client;
try {
  const wasm = await fetch("/target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm");
  if (!wasm.ok) throw new Error("WASM introuvable");
  const worker = new Worker("/web/engine-worker.mjs", { type: "module" });
  client = await createEngineClient({ worker, bytes: await wasm.arrayBuffer() });
  let responseAt = 0, resolveState = null, lastError = null;
  const timedClient = { request: async (...args) => { const out = await client.request(...args); responseAt = performance.now(); return out; } };
  const canvas = document.querySelector("#document-canvas"), scrollport = document.querySelector("#document-scroll");
  scrollport.style.height = "900px"; canvas.style.width = "100%";
  const host = { async open() { return { kind: "cancelled" }; }, async save() { return { kind: "ok" }; }, async notifyDirty() { return { kind: "ok" }; }, close() {} };
  editor = createEditor({ client: timedClient, host, canvas, scrollport, spacer: document.querySelector("#page-spacer"),
    textRepresentation: document.querySelector("#document-text"),
    onError: error => { lastError = error; },
    onState: () => { if (resolveState) { const done = resolveState; resolveState = null; done(performance.now()); } } });
  await editor.initialize();

  async function load(text) {
    const bytes = encoder.encode(text);
    const result = await editor.openDocument(() => ({ kind: "ok", bytes }));
    if (result.kind !== "ok") throw new Error(`ouverture refusée : ${JSON.stringify(result)}`);
    return { bytes: bytes.length, pages: editor.state.pages.length };
  }
  // Le plus grand document qui tient. v1 renvoie toutes les pages à chaque réponse : au-delà de 32 Mio
  // (MAX_RESPONSE) le moteur répond RESPONSE_LIMIT ; on cherche alors le plus grand document accepté.
  const refusal = error => /RESPONSE_LIMIT|TOO_LARGE|LIMIT/.test(String(error.message) + String(error.code));
  async function tryLines(pagesWanted) {
    let count = 46 * pagesWanted, last;
    for (let attempt = 0; attempt < 6; attempt += 1) {
      try { last = await load(shortLines(count)); } catch (error) { if (refusal(error)) return { refused: true }; throw error; }
      if (last.pages === pagesWanted) break;
      count = Math.max(1, Math.round(count * pagesWanted / last.pages) + (last.pages < pagesWanted ? 1 : -1));
    }
    return last;
  }
  async function tryDense(bytes) {
    try { return await load(denseText(bytes)); } catch (error) { if (refusal(error)) return { refused: true }; throw error; }
  }
  // Recherche dichotomique de la plus grande taille acceptée (lo accepté, hi refusé), puis marge de 10 %.
  async function largest(attempt, hi, min) {
    let lo = min;
    while (hi - lo > Math.max(2, min / 10)) {
      const mid = Math.floor((lo + hi) / 2);
      if ((await attempt(mid)).refused) hi = mid; else lo = mid;
    }
    return { accepted: lo, margin: Math.max(min, Math.floor(lo * 0.9)) };
  }
  let doc, note = "";
  if (SHAPE === "lines") {
    doc = await tryLines(TARGET_PAGES);
    if (doc.refused) {
      const { accepted, margin } = await largest(tryLines, TARGET_PAGES, 1);
      doc = await tryLines(margin);
      note = `${TARGET_PAGES} pages refusées par le moteur v1 (réponse > 32 Mio) ; plus grand accepté env. ${accepted} pages ; mesuré sur ${doc.pages} pages (marge de 10 % : la frappe ajoute du texte)`;
    } else if (doc.pages !== TARGET_PAGES) note = `${TARGET_PAGES} pages non atteintes (${doc.pages} pages)`;
  } else {
    doc = await tryDense(MAX_TEXT - 4096);
    if (doc.refused) {
      const { accepted, margin } = await largest(tryDense, MAX_TEXT - 4096, 1024);
      doc = await tryDense(margin);
      note = `texte plein de ${MAX_TEXT - 4096} octets refusé par le moteur v1 (réponse > 32 Mio) ; plus grand accepté env. ${accepted} octets ; mesuré sur ${doc.bytes} octets / ${doc.pages} pages`;
    } else note = `texte plein au plafond v1 de 128 Kio : ${doc.pages} pages seulement`;
  }

  // Curseur au milieu : sélectionne le marqueur, puis ArrowRight replie la sélection juste après lui.
  const found = await editor.search(MARKER);
  if (!found.items.length) throw new Error("marqueur introuvable");
  await editor.selectSearchResult(found, 0);
  editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  for (let i = 0; i < 200 && editor.state.anchor !== editor.state.focus; i += 1) await sleep(10);
  await sleep(100);
  if (editor.state.anchor !== editor.state.focus) throw new Error("caret non replié");
  const caretPage = editor.state.caret.page;

  async function type(kind, data) {
    const done = new Promise(resolve => { resolveState = resolve; });
    const enter = kind === "enter";
    const t0 = performance.now();
    editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: enter ? "Enter" : data, bubbles: true, cancelable: true }));
    editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: enter ? "insertParagraph" : "insertText", data: enter ? null : data, bubbles: true, cancelable: true }));
    const drawn = await Promise.race([done, sleep(15000).then(() => { throw new Error(`pas de mise à jour après la frappe (${lastError?.code ?? lastError?.message ?? "sans erreur"})`); })]);
    const framed = await frame();
    return { total: drawn - t0, engine: responseAt - t0, draw: drawn - responseAt, frame: framed - t0, enter };
  }
  const alphabet = "abcdefghijklmnopqrstuvwxyz";
  for (let i = 0; i < 5; i += 1) { await type("char", "w"); await sleep(20); } // échauffement, non compté
  const samples = [];
  for (let i = 0; i < KEYS; i += 1) { samples.push(await type("char", alphabet[i % 26])); await sleep(20); }
  samples.push(await type("enter")); await sleep(20);
  for (let i = 0; i < 20; i += 1) { samples.push(await type("char", alphabet[i % 26])); await sleep(20); }
  samples.push(await type("enter"));
  const pick = key => stats(samples.map(s => s[key]));
  const total = pick("total");
  report = { status: "passed", engine: "v1", userAgent: navigator.userAgent, shape: SHAPE, document: { ...doc, note, caretPage },
    keystrokes: samples.length, enterKeys: samples.filter(s => s.enter).length,
    endToEndMs: total, engineRoundTripMs: pick("engine"), decodeAndDrawMs: pick("draw"), untilNextFrameMs: pick("frame"),
    enterOnlyMs: stats(samples.filter(s => s.enter).map(s => s.total)),
    budget16ms: { p95: total.p95 < 16, max: total.max < 16 } };
} catch (error) {
  report = { status: "failed", message: error?.stack ?? String(error) };
} finally {
  editor?.dispose(); client?.close();
  document.querySelector("#result").textContent = JSON.stringify(report, null, 2);
  await fetch("/__editor_result", { method: "POST", body: JSON.stringify(report) });
}
