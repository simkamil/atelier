// SPDX-License-Identifier: AGPL-3.0-or-later
// Sélection de l'éditeur (v1 texte brut ou v2 riche) pour les suites navigateur de l'éditeur (US-219, R7).
//
// Toutes les suites concernées acceptent `--engine v1|v2` ; le défaut est `DEFAULT_ENGINE`.
// Pour une suite non prête en v2, `--engine v2` ne lance rien :
// la suite écrit un statut JSON « pending » et sort avec `V2_PENDING_EXIT_CODE` (75). Ce code est
// distinct de 0 (succès), 1 (échec d'une vérification) et 2 (usage) : un « en attente » n'est jamais
// un succès, et la CI ne doit pas l'accepter comme tel.
//
// R7a : l'interface sait créer l'éditeur v2 (`?engine=v2`). Le statut v2 est **par suite** : une suite n'est « prête »
// que si elle passe réellement en v2 (liste `V2_READY_SUITES`) ; les autres gardent le code 75 avec leur propre raison
// (`V2_PENDING_REASONS`). Une suite prête en v2 peut sauter des vérifications propres à v1 (recherche, aperçu DOCX,
// pixels d'images...) : elle les liste dans `skipped` de son rapport avec la raison (docs/r7-ui-parity.md §7).
// La bascule du défaut (`DEFAULT_ENGINE`) est R7b.

export const DEFAULT_ENGINE = "v1";
export const V2_PENDING_EXIT_CODE = 75;
export const V2_PENDING_MESSAGE = "v2 en attente de R7b (suite non portée)";
/** Suites qui passent réellement en v2 (R7a). */
export const V2_READY_SUITES = Object.freeze(["run-ui-browser", "run-image-insert-browser", "run-editor-browser"]);
/** Raison du code 75 des suites non portées, par nom de suite. */
export const V2_PENDING_REASONS = Object.freeze({
  "run-editor-utilities-browser": "v2 en attente de R7b : le menu contextuel et les symboles sont couverts en v2 par run-rich-editor-ui-browser ; cette suite pilote encore createEditor",
  "run-frontend-browser": "v2 en attente de R7b : la suite pilote le rendu Canvas et la saisie de createEditor ; l'atlas v2 a run-rich-atlas-render-browser",
  "mouse-selection": "v2 en attente de R7b : module partagé testé tel quel ; la souris v2 est dans run-rich-editor-ui-browser (double et triple clic absents)",
  "run-latency-browser": "v2 en attente de R7b : le banc de latence mesure l'éditeur v1 (la mesure v2 est le critère de R7b)",
});
export const ENGINES = Object.freeze({
  v1: Object.freeze({ ready: true }),
  v2: Object.freeze({ ready: false, pending: V2_PENDING_MESSAGE }),
});

export const isReady = (engine, suite) => ENGINES[engine].ready || (engine === "v2" && V2_READY_SUITES.includes(suite));
export const pendingReason = suite => V2_PENDING_REASONS[suite] ?? V2_PENDING_MESSAGE;

/** Extrait `--engine <v>` / `--engine=<v>` ; renvoie le moteur et les arguments restants. */
export function parseEngineArgs(args = process.argv.slice(2)) {
  const rest = [];
  let engine = DEFAULT_ENGINE;
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--engine") { engine = args[i + 1]; i += 1; }
    else if (arg.startsWith("--engine=")) engine = arg.slice(9);
    else rest.push(arg);
  }
  if (!Object.hasOwn(ENGINES, engine)) throw new RangeError("--engine attend v1 ou v2");
  return { engine, rest };
}

/**
 * Point d'entrée des lanceurs. `isValid(rest)` valide les autres arguments : usage invalide -> code 2,
 * toujours avant le statut « en attente » (code 75) de v2.
 */
export function selectEngine(suite, usage, isValid = rest => rest.length === 0, args = process.argv.slice(2)) {
  let parsed;
  try { parsed = parseEngineArgs(args); } catch (error) {
    console.error(`${suite} : ${error.message}`);
    process.exit(2);
  }
  if (!isValid(parsed.rest)) { console.error(`Usage : ${usage}`); process.exit(2); }
  if (!isReady(parsed.engine, suite)) {
    const reason = pendingReason(suite);
    console.log(JSON.stringify({ status: "pending", engine: parsed.engine, suite, reason, exitCode: V2_PENDING_EXIT_CODE }));
    console.error(`${suite} : ${reason} (code de sortie ${V2_PENDING_EXIT_CODE}, ni succès ni échec)`);
    process.exit(V2_PENDING_EXIT_CODE);
  }
  return parsed;
}
