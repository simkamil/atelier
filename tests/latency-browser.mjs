// SPDX-License-Identifier: AGPL-3.0-or-later
// Banc de latence de bout en bout (US-219) : événement clavier -> fin du dessin Canvas, pour la frappe, Entrée et Retour arrière
// au milieu d'un document déterministe, avec l'éditeur v1 (`createEditor`) ou v2 (`createRichEditor`, R7c).
// Instrumentation de test seulement : aucun code produit n'est modifié.
//   v1 : le dessin du Canvas est synchrone dans `publish()` de `web/editor.mjs` (setPages -> render, puis updateViewport) ;
//        le rappel `onState` s'exécute juste après, donc son horodatage marque la fin du dessin.
//   v2 : `syncPages()` de `web/rich-editor.mjs` appelle `CanvasRenderer.setPages` (dessin synchrone) avant `onState`. Le dessin
//        « de l'édition » est le premier `setPages` après la frappe où la révision du cache a changé et où aucune page de la
//        fenêtre visible ne manque encore (la `State` porte les pages touchées, sinon une requête `Pages` suit) ; le banc
//        enveloppe `EditorPageCache.apply` (pour lire le cache) et `CanvasRenderer.setPages` (pour l'horodatage).
// `untilNextFrameMs` ajoute l'attente de l'image d'animation suivante (présentation).
import { createEditor } from "../web/editor.mjs";
import { createRichEditor } from "../web/rich-editor.mjs";
import { EditorPageCache } from "../web/editor-page-cache.mjs";
import { CanvasRenderer } from "../web/canvas-renderer.mjs";
import { createEngineClient } from "../web/engine-client.mjs";
import { createI18n, parseCatalog } from "../web/i18n.mjs";

const params = new URL(location.href).searchParams;
const TARGET_PAGES = Number(params.get("pages") ?? 300);
const KEYS = Number(params.get("keys") ?? 300);
const PAIRS = Number(params.get("pairs") ?? 40); // paires Entrée / Retour arrière mesurées
// lines : TARGET_PAGES pages de lignes courtes ; dense : texte plein au plafond de l'éditeur v1.
const ENGINE = params.get("engine") === "v2" ? "v2" : "v1";
const V2 = ENGINE === "v2";
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
  if (values.length === 0) return { n: 0, mean: null, p50: null, p95: null, max: null };
  const sorted = [...values].sort((a, b) => a - b);
  const at = p => sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)];
  const r = x => Math.round(x * 1000) / 1000;
  return { n: values.length, mean: r(values.reduce((a, b) => a + b, 0) / values.length), p50: r(at(0.5)), p95: r(at(0.95)), max: r(sorted.at(-1)) };
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const frame = () => new Promise(resolve => requestAnimationFrame(() => resolve(performance.now())));

// v2 : lecture du cache de pages et horodatage du dessin (voir l'en-tête).
let cacheRef = null;
let pendingDraw = null;
if (V2) {
  const apply = EditorPageCache.prototype.apply;
  EditorPageCache.prototype.apply = function (response) { cacheRef = this; return apply.call(this, response); };
  const setPages = CanvasRenderer.prototype.setPages;
  CanvasRenderer.prototype.setPages = function (pages) {
    const out = setPages.call(this, pages);
    if (pendingDraw && cacheRef && cacheRef.revision !== pendingDraw.revision) {
      const visible = cacheRef.visible ?? { first: 0, end: 1 };
      if (cacheRef.missing(visible.first, visible.end).length === 0) { const done = pendingDraw.done; pendingDraw = null; done(performance.now()); }
    }
    return out;
  };
}

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
  const common = { client: timedClient, host, canvas, scrollport, spacer: document.querySelector("#page-spacer"),
    textRepresentation: document.querySelector("#document-text"), onError: error => { lastError = error; } };
  if (V2) {
    // Configuration du produit : textes i18n (donc miroir DOM accessible), mise en page par tranches de 16 paragraphes.
    const catalog = new Map([...parseCatalog(await (await fetch("/i18n/fr-FR.txt")).text()).entries].map(([k, entry]) => [k, entry.value]));
    const i18n = createI18n({ locale: "fr-FR", catalog });
    editor = createRichEditor({ ...common, confirmDiscard: "confirm", t: (key, values) => i18n.t(key, values) });
  } else {
    editor = createEditor({ ...common, onState: () => { if (resolveState) { const done = resolveState; resolveState = null; done(performance.now()); } } });
  }
  await editor.initialize();

  const pagesOf = () => (V2 ? editor.state.pageCount : editor.state.pages.length);
  async function load(text) {
    const bytes = encoder.encode(text);
    const result = await editor.openDocument(() => ({ kind: "ok", bytes }));
    if (result.kind !== "ok") throw new Error(`ouverture refusée : ${JSON.stringify(result)}`);
    if (V2) await editor.whenLayoutComplete();
    return { bytes: bytes.length, pages: pagesOf() };
  }
  // v1 : le plus grand document qui tient. v1 renvoie toutes les pages à chaque réponse : au-delà de 32 Mio
  // (MAX_RESPONSE) le moteur répond RESPONSE_LIMIT ; on cherche alors le plus grand document accepté.
  const refusal = error => /EDITOR_RESPONSE_LIMIT|TOO_LARGE/.test(String(error.message) + String(error.code));
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
    } else note = V2 ? `texte plein de ${doc.bytes} octets : ${doc.pages} pages` : `texte plein au plafond v1 de 128 Kio : ${doc.pages} pages seulement`;
  }

  // Curseur au milieu : sélectionne le marqueur, puis ArrowRight replie la sélection juste après lui.
  const found = await editor.search(MARKER);
  if (!found.items.length) throw new Error("marqueur introuvable");
  await editor.selectSearchResult(found, 0);
  editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  for (let i = 0; i < 200 && editor.state.anchor !== editor.state.focus; i += 1) await sleep(10);
  await sleep(300);
  if (editor.state.anchor !== editor.state.focus) throw new Error("caret non replié");
  const caretPage = editor.state.caret.page;
  const initialPages = pagesOf();

  // kind : "char" | "enter" | "backspace".
  async function type(kind, data) {
    const t0 = performance.now();
    let done;
    if (V2) {
      done = new Promise(resolve => { pendingDraw = { revision: cacheRef.revision, done: resolve }; });
    } else {
      done = new Promise(resolve => { resolveState = resolve; });
    }
    const keyName = kind === "enter" ? "Enter" : kind === "backspace" ? "Backspace" : data;
    editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: keyName, bubbles: true, cancelable: true }));
    editor.element.dispatchEvent(new InputEvent("beforeinput", {
      inputType: kind === "enter" ? "insertParagraph" : kind === "backspace" ? "deleteContentBackward" : "insertText",
      data: kind === "char" ? data : null, bubbles: true, cancelable: true }));
    const drawn = await Promise.race([done, sleep(15000).then(() => { pendingDraw = null; throw new Error(`pas de mise à jour après la frappe (${kind} ; ${lastError?.code ?? lastError?.message ?? "sans erreur"})`); })]);
    const respondedAt = responseAt;
    const framed = await frame();
    return { total: drawn - t0, engine: Math.min(respondedAt, drawn) - t0, draw: drawn - Math.min(respondedAt, drawn), frame: framed - t0, kind };
  }
  const alphabet = "abcdefghijklmnopqrstuvwxyz";
  for (let i = 0; i < 5; i += 1) { await type("char", "w"); await sleep(20); } // échauffement, non compté
  const samples = [];
  for (let i = 0; i < KEYS; i += 1) { samples.push(await type("char", alphabet[i % 26])); await sleep(20); }
  for (let i = 0; i < PAIRS; i += 1) {
    samples.push(await type("enter")); await sleep(20);
    samples.push(await type("backspace")); await sleep(20);
  }
  const by = kind => samples.filter(s => s.kind === kind);
  const pick = (key, list = samples) => stats(list.map(s => s[key]));
  const total = pick("total");
  report = { status: "passed", engine: ENGINE, synthetic: true, gpu: "disabled (rendu logiciel, --disable-gpu)",
    isolatedMachine: false, caveat: "événements clavier synthétiques ; machine non isolée", userAgent: navigator.userAgent, shape: SHAPE,
    document: { ...doc, note, caretPage, pagesBefore: initialPages, pagesAfter: pagesOf(), target: TARGET_PAGES },
    keystrokes: samples.length, enterKeys: by("enter").length, backspaceKeys: by("backspace").length,
    endToEndMs: total, byKindMs: { char: pick("total", by("char")), enter: pick("total", by("enter")), backspace: pick("total", by("backspace")) },
    engineRoundTripMs: pick("engine"), decodeAndDrawMs: pick("draw"), untilNextFrameMs: pick("frame"),
    budget16ms: { p95: total.p95 < 16, max: total.max < 16 } };
} catch (error) {
  report = { status: "failed", engine: ENGINE, message: error?.stack ?? String(error) };
} finally {
  editor?.dispose(); client?.close();
  document.querySelector("#result").textContent = JSON.stringify(report, null, 2);
  await fetch("/__editor_result", { method: "POST", body: JSON.stringify(report) });
}
