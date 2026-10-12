---
title: 'US-213 — réduire les copies du cache incrémental'
type: refactor
created: '2026-10-12'
status: done
route: dispatch
baseline_commit: 76b1ab0771ea32c8edf72a39e0a2cbb26c3c8997
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

Réduire le coût des copies profondes inutiles dans la session de pagination incrémentale, sans changer le résultat public ni ses garanties. Le dernier benchmark CPU mesurait 268.214ms pour une édition locale de 300 pages, donc le critère 16ms reste ouvert. Mesurer avant/après sur le même scénario et afficher séparément édition locale et mise à jour identique.

## Boundaries & Constraints

**Always:** égalité exacte avec le recalcul intégral; cache transactionnel si échec; invalidation géométrie/onglets/polices/règles; arrêt réel du flux au point stable; préflight document entier explicite; conservation des API publiques, absence de nouveaux crates. Optimiser d'abord stats en place et comparaisons empruntées. L'agent peut également adapter l'API interne paginate_recording dans pagination.rs pour emprunter les layouts sans copies, en conservant l'API publique paginate et les invariants. Mesure de référence avant édition du code, mesures release répétées après; conserver min/médiane et compteurs de travail, pas de seuil temporel fragile.

**Never:** changer le plafond WASM, masquer le préflight, déclarer 16ms/UI validés sur un benchmark CPU, supprimer les erreurs/diagnostics ou convertir la comparaison exacte en hash seul, modifier les fichiers d'autres agents.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| same | Session et inputs identiques | Zéro reshape/recalcul, résultat exact, statistiques actualisées | Aucun |
| local | Édition géométrique page 149/300 | 1 reshape/299 réutilisés, arrêt réel et suffixe réutilisé, équivalence fraîche | Échec atomique |
| global | Polices/règles/dimensions/onglets modifiés | Invalidation adéquate, équivalence fraîche | Erreurs préflight conservées |
| invalid | Entrées invalides/IDs dupliqués/budget | Cache et dernier résultat publiés inchangés | Erreur d'origine |
| benchmark | 300 pages × 52 lignes, same/local | Mesures release avant/après et compteurs, sans assertion 16ms | Hôte et protocole consignés |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/incremental.rs`: cache, stats, copie de layouts et comparaison du suffixe.
- `crates/text-layout/src/pagination.rs`: préflight, paginate_recording, checkpoints et arrêt.
- `crates/text-layout/examples/incremental_benchmark.rs`: retrouver le nom réel de l'exemple existant, mêmes entrées de mesure.

## Tasks & Acceptance

**Execution:**
- [x] `crates/text-layout/src/incremental.rs`, `crates/text-layout/src/pagination.rs`, tests et exemple benchmark text-layout — agent incremental_perf: mesurer référence, enlever clones inutiles et vérifier garanties. L'agent possède ces seuls fichiers. Écrire les mesures dans `_bmad-output/implementation-artifacts/editor-incremental-perf-benchmark.json`; ne pas modifier les autres docs. Les décisions locales d'ownership internes sont autorisées si elles conservent les API publiques.
- [x] `docs/PROTOTYPE-PROGRESS.md`, `US-V0-V1.md`, carte Graphify — parent: documenter limites et résultat mesuré, maintenir US-213 partielle si son critère UI 16ms reste non démontré.

**Acceptance Criteria:**
- Given les entrées de référence, when les optimisations sont actives, then toutes les géométries et diagnostics restent égaux au calcul frais.
- Given un échec d'update, when une mise à jour valide suit, then la session conserve les mêmes garanties que la référence.
- Given les mesures release, when elles sont publiées, then les chiffres distinguent préflight, shaping et pagination et ne certifient aucune latence UI.

## Implementation Notes

## Spec Change Log

## Review Triage Log

Every independent finding and its individual verdict is recorded in [the shared triage](editor-drag-spec.md#review-triage-log) and editor-drag-spacing-perf-review.json.

## Verification

- `cargo test --release --locked --offline -p atelier-text-layout` — tests ciblés existants et régressions ajoutées.
- Exemple benchmark release avec 300 pages, mêmes options et polices.
- Parent: validation workspace/WASM, taille et graphe sans dépendance ajoutée.

Final receipt: editor-drag-spacing-perf-validation.json; 23 gates, 749 Windows tests, 78 layout release tests, 82 browser assertions. Frozen matrices are traced by editor-drag-spacing-perf-matrix.json. Native external Windows drag and the 16 ms UI criterion remain open.
