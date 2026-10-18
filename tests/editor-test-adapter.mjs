// SPDX-License-Identifier: AGPL-3.0-or-later
// Adaptateur de test côté navigateur : tout ce que les suites de l'éditeur savent de l'éditeur v1
// (opcodes ATED v1 de `web/editor-protocol.mjs`) passe ici. R7 réécrit ce seul fichier pour v2 ;
// R8 retire la partie v1 avec l'éditeur.
import { encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";

export const engine = "v1";

/** Octets de la commande « insérer du texte » hors bande (modification concurrente du document). */
export function insertTextCommand(text, nowMs = 999n) {
  return encodeEditorCommand(EditorOpcode.InsertText, { nowMs, text });
}
/** Commande de sélection (anchor/focus en octets UTF-8) pour les bancs qui posent le curseur. */
export function setSelectionCommand(anchor, focus) {
  return encodeEditorCommand(EditorOpcode.SetSelection, { anchor, focus });
}
