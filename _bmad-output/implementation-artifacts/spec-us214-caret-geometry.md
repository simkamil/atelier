---
title: 'US-214 — Géométrie du curseur riche'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: '13c605b19926bd08b439fc711d132d5a638f8e5e'
context: []
---

<frozen-after-approval reason="Poursuite autonome et parallèle des US autorisée par l’utilisateur">

## Intent

**Problem:** Le moteur riche publie des glyphes mais aucune conversion complète entre position de texte et coordonnées. La sélection riche et les déplacements verticaux ont besoin d’une géométrie commune aux lignes et pages.

**Approach:** Ajouter `caret::CaretLayout` sur des vues de lignes positionnées, indépendantes du paginateur, et produire curseurs, hit-tests, rectangles de sélection et déplacements conservant la colonne visuelle. Le parent raccordera ces vues aux sorties de pagination dans un test intégré.

## Boundaries & Constraints

**Always:** Unités 1/64 pt, offsets d’octets UTF-8 par paragraphe, arrêt aux frontières de graphèmes. `PositionedLine` fournit page_index, paragraph_index, texte original, &LayoutLine, top local à la page et translation horizontale explicite. Préserver la topologie des pages via index dans chaque résultat; hit-test prend un index de page (coordonnées locales), évitant l’ambiguïté entre pages superposées. Valider plages, frontières, ordre/continuité et géométrie sans overflow. Agréger les glyphes d’un même cluster; placer les arrêts internes d’une ligature sur ses frontières de graphèmes par interpolation uniforme, documentée comme approximation sans GDEF. Espaces terminaux sans glyphes = positions distinctes à avance nulle en fin de ligne. Biais/affinité explicite en frontière de ligne, ou règle aval documentée. Avant LF/CRLF = fin de la ligne précédente, après = début suivante. La colonne préférée est un état fourni par l’appelant et n’est pas écrasée quand la ligne intermédiaire est courte. Limites de lignes/stops/travail explicites pour entrées non fiables.

**Never:** Modifier lib.rs, Cargo, modèle/backlog ou modules des autres agents; ajouter dépendance/unsafe; bidi/écritures complexes, intégration UI, mesure Word ou positionnement GDEF exact.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat | Erreur |
|---|---|---|---|
| Position et point | glyphes connus, x/y et page connus | rectangle et position la plus proche déterministes | position hors document rejetée |
| Unicode | graphème combinant, ligature multigraphème, cluster multiglyphe | tous les stops sont frontières de graphèmes; aucune somme d’avance doublée | offset interne au graphème rejeté |
| Fin de ligne | espaces terminaux, LF/CRLF et ligne terminale vide | positions conservées; frontière aval cohérente | géométrie incomplète rejetée |
| Sélection | plage inversée, plusieurs paragraphes et pages | normalisation et rectangles indexés par page/ligne | bornes invalides rejetées |
| Vertical | ligne longue → courte → longue | x préféré initial conservé et récupéré | bord du document stable |
| Malformé | plages chevauchées, height nulle, dépassement i64, budget épuisé | aucun résultat partiel | erreur typée |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs` — LayoutLine, LayoutFragment, LayoutGlyph; x comporte x_offset; utiliser le pen x-x_offset pour les avances.
- `crates/unicode/src/segmentation.rs` — API grapheme_boundaries (repérer le chemin réel si modulaire).
- `crates/text-layout/src/caret.rs` — nouvelle API de vues et requêtes.
- `crates/text-layout/tests/caret.rs` — géométrie synthétique + vrais layouts/shaping.
- `docs/us-214-caret-geometry.md` — API, coordonnées et approximation des ligatures.

## Tasks & Acceptance

- [x] `crates/text-layout/src/caret.rs` — implémenter vues validées, curseur/hit-test/sélection/déplacement et budgets.
- [x] `crates/text-layout/tests/caret.rs` — couvrir toute la matrice, chiffres attendus et invariants Unicode.
- [x] `docs/us-214-caret-geometry.md` — exemple public et limites.

Given des lignes valides sur plusieurs pages, when position/point/selection sont interrogés, then les résultats gardent les pages et frontières UTF-8/graphèmes correctes.
Given une ligature, when le curseur la traverse, then ses graphèmes restent accessibles sans scinder un graphème combinant.
Given une ligne courte entre deux longues, when le curseur descend deux fois, then la colonne visuelle initiale reste la cible.

## Implementation Notes

Le paginateur publiera PlacedLine(page_index, paragraph_index, line_index, top, baseline_y), indépendant du type de vue; adaptation par root. Shell cmd.exe/login:false, tests ciblés; pas de commits/push. lib.rs exporté par root quand modules prêts.

## Spec Change Log

## Review Triage Log

Voir [revue triée](rich-page-geometry-review.md) : 12 constats indépendants, chacun conservé et réfuté ou corrigé; six constats complémentaires de l’intégrateur et régressions. Correctifs exécutés par les développeurs propriétaires des modules.

## Verification

- `cargo test -p atelier-text-layout --test caret --locked --offline`
- `cargo clippy -p atelier-text-layout --all-targets --locked --offline -- -D warnings`

Validation finale : 707 tests workspace, 57 tests text-layout release, 20 contrôles passés sur source immuable; [reçu](rich-page-geometry-validation.json) et [audit des matrices](rich-page-geometry-matrix.json). La clôture de cette spec fonctionnelle ne clôture pas les critères Word ouverts du backlog.
