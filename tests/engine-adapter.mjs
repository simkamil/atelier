// SPDX-License-Identifier: AGPL-3.0-or-later
// Sélection de l'éditeur (v1 texte brut ou v2 riche) pour les suites navigateur de l'éditeur (US-219, R7).
//
// Toutes les suites concernées acceptent `--engine v1|v2` ; le défaut est `DEFAULT_ENGINE`.
// Tant que l'interface n'est pas branchée sur l'éditeur v2 (lot 3), `--engine v2` ne lance rien :
// la suite écrit un statut JSON « pending » et sort avec `V2_PENDING_EXIT_CODE` (75). Ce code est
// distinct de 0 (succès), 1 (échec d'une vérification) et 2 (usage) : un « en attente » n'est jamais
// un succès, et la CI ne doit pas l'accepter comme tel.
//
// Passage à v2 (R7) : passer `ENGINES.v2.ready` à true, fournir `tests/editor-test-adapter.mjs` côté v2
// (commandes de test) et basculer `DEFAULT_ENGINE`. Les suites elles-mêmes ne changent pas.

export const DEFAULT_ENGINE = "v1";
export const V2_PENDING_EXIT_CODE = 75;
export const V2_PENDING_MESSAGE = "v2 en attente de R7 (lot 3)";
export const ENGINES = Object.freeze({
  v1: Object.freeze({ ready: true }),
  v2: Object.freeze({ ready: false, pending: V2_PENDING_MESSAGE }),
});

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
  if (!ENGINES[parsed.engine].ready) {
    console.log(JSON.stringify({ status: "pending", engine: parsed.engine, suite, reason: ENGINES[parsed.engine].pending,
      exitCode: V2_PENDING_EXIT_CODE }));
    console.error(`${suite} : ${ENGINES[parsed.engine].pending} (code de sortie ${V2_PENDING_EXIT_CODE}, ni succès ni échec)`);
    process.exit(V2_PENDING_EXIT_CODE);
  }
  return parsed;
}
