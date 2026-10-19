// SPDX-License-Identifier: AGPL-3.0-or-later
// US-320 : les modèles embarqués (data/templates/*.docx) s'ouvrent dans le vrai moteur de l'éditeur riche,
// comme documents modifiables et non modifiés, se mettent en page, et se réenregistrent en DOCX sans réserve.
// `node tests/file-templates.mjs [moteur.wasm]` (défaut : data/engine/atelier_core_wasm.wasm).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { decodeRichReply, encodeRichRequest } from "../web/rich-editor-protocol.mjs";

const root = new URL("../", import.meta.url);
const wasm = await readFile(process.argv[2] ?? new URL("data/engine/atelier_core_wasm.wasm", root));
const { memory, alloc, dispatch, free } = new WebAssembly.Instance(await WebAssembly.compile(wasm)).exports;
let outerId = 1n;
let innerId = 1;

function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const bodyPtr = descriptor.getUint32(0, true);
  const bodyLen = descriptor.getUint32(4, true);
  try {
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    return body.slice(4);
  } finally { free(bodyPtr, bodyLen); free(descriptorPtr, 8); }
}
function send(command, doc = 1) {
  const payload = encodeRichRequest({ docId: doc, requestId: innerId++, command });
  const message = decodeMessage(invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload })), "event");
  assert.equal(message.type, MessageType.EditorResult);
  return decodeRichReply(message.payload).response;
}

/** Marqueurs de contenu attendus par modèle (texte, ordre d'apparition). */
const TEMPLATES = {
  "lettre.docx": ["Prénom Nom", "Objet :", "Madame, Monsieur,", "salutations distinguées"],
  "cv.docx": ["Intitulé du poste recherché", "Expérience professionnelle", "Formation", "Compétences", "Langues"],
  "rapport.docx": ["Titre du rapport", "Résumé", "Introduction", "Résultats", "Conclusion et recommandations", "Annexes"],
  "compte-rendu.docx": ["Compte rendu de réunion", "Participants :", "Ordre du jour", "Décisions", "Responsable", "Prochaine réunion :"],
};
let checks = 0;
for (const [name, markers] of Object.entries(TEMPLATES)) {
  const bytes = new Uint8Array(await readFile(new URL(`data/templates/${name}`, root)));
  const opened = send({ type: "open", kind: "docx", profile: "document", data: bytes });
  assert.equal(opened.type, "state", `${name} : ouverture refusée (${opened.code ?? opened.type})`);
  assert.equal(opened.readOnly, false, `${name} doit être modifiable`);
  assert.equal(opened.modified, false, `${name} s'ouvre non modifié`);
  assert(opened.docLen > 100n, `${name} a du contenu`);
  const window = send({ type: "textWindow", start: 0n, len: 262144 });
  assert.equal(window.type, "textWindow");
  let from = 0;
  for (const marker of markers) {
    const at = window.text.indexOf(marker, from);
    assert(at >= from, `${name} : « ${marker} » absent ou dans le désordre`);
    from = at;
  }
  const saved = send({ type: "save", format: "docx", nowMs: 1_800_000_000_000n, user: "Test", version: "1" });
  assert.equal(saved.type, "saveData", `${name} : réenregistrement refusé`);
  assert.equal(saved.notices.length, 0, `${name} : réserves à l'enregistrement : ${JSON.stringify(saved.notices)}`);
  const again = send({ type: "open", kind: "docx", profile: "document", data: saved.bytes.slice() });
  assert.equal(again.type, "state", `${name} : le DOCX réenregistré se rouvre`);
  checks += 1;
}
assert.equal(checks, Object.keys(TEMPLATES).length);
console.log(`templates: ${checks} modèles ouverts, modifiables, réenregistrés sans réserve`);
