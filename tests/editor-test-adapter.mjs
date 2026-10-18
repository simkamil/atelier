// SPDX-License-Identifier: AGPL-3.0-or-later
// Adaptateur de test côté navigateur : tout ce que les suites de l'éditeur savent des trames de l'éditeur
// (trames ATED de `web/rich-editor-protocol.mjs`) passe ici.
// R8 : la partie v1 est retirée avec l'éditeur ; il ne reste que les trames de l'éditeur riche.
import { encodeRichRequest } from "../web/rich-editor-protocol.mjs";

export const engine = "v2";

/** Identifiant de document de l'éditeur v2 des suites (`createRichEditor` : `docId` par défaut). */
export const V2_DOC_ID = 1;
/** Identifiant de requête des trames hors bande : loin de ceux du client (qui compte depuis 1). */
const OUT_OF_BAND_REQUEST_ID = 0x7ffffff0;

/** Octets de la commande « insérer du texte » hors bande (modification concurrente du document). */
export function insertTextCommand(text, nowMs = 999n) {
  return encodeRichRequest({ docId: V2_DOC_ID, requestId: OUT_OF_BAND_REQUEST_ID, command: { type: "insertText", text } });
}
/** Commande de sélection (anchor/focus en octets UTF-8) pour les bancs qui posent le curseur. */
export function setSelectionCommand(anchor, focus) {
  return encodeRichRequest({ docId: V2_DOC_ID, requestId: OUT_OF_BAND_REQUEST_ID + 1,
    command: { type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false } });
}
/** Texte entier du document, demandé au moteur. */
export async function documentText(editor) {
  return editor.readText();
}
