// SPDX-License-Identifier: AGPL-3.0-or-later
// Adaptateur de test côté navigateur : tout ce que les suites de l'éditeur savent des trames de l'éditeur
// (opcodes ATED v1 de `web/editor-protocol.mjs`, trames ATED v2 de `web/rich-editor-protocol.mjs`) passe ici.
// Le moteur est celui de l'URL de la page de test (`?engine=v2`, sinon v1), comme pour l'interface (R7a).
// R8 retire la partie v1 avec l'éditeur.
import { encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";
import { encodeRichRequest } from "../web/rich-editor-protocol.mjs";

export const engine = typeof location === "undefined" ? "v1" : new URLSearchParams(location.search).get("engine") === "v2" ? "v2" : "v1";

/** Identifiant de document de l'éditeur v2 des suites (`createRichEditor` : `docId` par défaut). */
export const V2_DOC_ID = 1;
/** Identifiant de requête des trames hors bande : loin de ceux du client (qui compte depuis 1). */
const OUT_OF_BAND_REQUEST_ID = 0x7ffffff0;

/** Octets de la commande « insérer du texte » hors bande (modification concurrente du document). */
export function insertTextCommand(text, nowMs = 999n) {
  if (engine === "v2") return encodeRichRequest({ docId: V2_DOC_ID, requestId: OUT_OF_BAND_REQUEST_ID, command: { type: "insertText", text } });
  return encodeEditorCommand(EditorOpcode.InsertText, { nowMs, text });
}
/** Commande de sélection (anchor/focus en octets UTF-8) pour les bancs qui posent le curseur. */
export function setSelectionCommand(anchor, focus) {
  if (engine === "v2") {
    return encodeRichRequest({ docId: V2_DOC_ID, requestId: OUT_OF_BAND_REQUEST_ID + 1,
      command: { type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false } });
  }
  return encodeEditorCommand(EditorOpcode.SetSelection, { anchor, focus });
}
/** Texte entier du document : dans l'état en v1, demandé au moteur en v2. */
export async function documentText(editor) {
  return engine === "v2" ? editor.readText() : editor.state.text;
}
