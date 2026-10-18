// SPDX-License-Identifier: AGPL-3.0-or-later
// Lanceurs des suites navigateur de l'éditeur (US-219). Depuis R8 l'éditeur de l'interface est unique (v2, riche) : l'option
// `--engine` et le moteur v1 ont disparu avec lui. Ce module garde le point d'entrée commun des lanceurs
// (`selectEngine`) pour valider les arguments (usage invalide : code 2) ; `engine` vaut toujours "v2", valeur que les pages de
// test reçoivent encore dans `?engine=v2` et que l'interface rapporte dans `app.engine`.

export const ENGINE = "v2";
const SHARED_NOTES = Object.freeze({
  "run-editor-utilities-browser": "modules isolés (menu contextuel, À propos, symboles) sans éditeur ; leur intégration avec l'éditeur est dans run-rich-editor-ui-browser (editor-utilities-integration-v2, document-status-integration-v2)",
  "run-frontend-browser": "CanvasRenderer et text-input sans éditeur ; le rendu des pages (atlas) est dans run-rich-atlas-render-browser et run-rich-editor-ui-browser",
  "mouse-selection": "module de souris sans éditeur ; la souris de l'éditeur (HitTest, SelectUnit, glisser-déposer MoveText) est dans run-editor-browser et run-rich-editor-ui-browser",
});
export const engineNote = suite => SHARED_NOTES[suite] ?? null;

/**
 * Point d'entrée des lanceurs. `isValid(rest)` valide les arguments : usage invalide -> code 2.
 * `--engine` n'existe plus : il est rejeté comme tout argument inconnu.
 */
export function selectEngine(suite, usage, isValid = rest => rest.length === 0, args = process.argv.slice(2)) {
  if (!isValid(args)) { console.error(`${suite} : usage : ${usage}`); process.exit(2); }
  return { engine: ENGINE, rest: args };
}
