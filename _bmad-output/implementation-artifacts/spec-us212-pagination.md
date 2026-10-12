---
title: 'US-212 — Pagination des lignes riches'
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

**Problem:** Les lignes riches d’US-210 ne sont pas encore réparties en pages. Cela bloque les règles de paragraphes solidaires et la géométrie de sélection entre pages.

**Approach:** Ajouter un paginateur déterministe sur les lignes déjà calculées, avec espacements résolus, contraintes de solidarité et sauts explicites. Conserver les indices des lignes et paragraphes d’origine pour l’intégration au curseur. US-212 restera partielle tant que la comparaison Word de 95 % n’est pas authentiquement mesurée.

## Boundaries & Constraints

**Always:** Unités 1/64 pt, coordonnées locales à chaque page, offsets UTF-8 conservés; aucune dépendance externe ni unsafe. API autonome `pagination::paginate` sur `&[ParagraphLayout]`, règles de même longueur, géométrie et limites explicites. Les règles fournissent les valeurs avant/après déjà incluses dans chaque layout; valider `height = before + somme(height lignes) + after` puis enlever ces espacements pour placer les lignes, sans double comptage. Exposer `PlacedLine` avec page/paragraph/line indices, top et baseline, et pages avec plages de placements. Espacement normal entre paragraphes = max(after précédent, before suivant); même style et espacement contextuel: soustraire du maximum chaque côté dont contextual=true, sans descendre sous zéro. En haut de page supprimer avant par défaut pour un saut automatique; conserver avant sur saut explicite. Les règles sont des entrées typées, sans modification de ParaProps dans ce lot. Sauts manuels entre lignes via indices explicites, plus avant/après paragraphe. Les contraintes impossibles sont relâchées avec un diagnostic explicite, en conservant toutes les lignes une fois.

**Never:** Modifier lib.rs, Cargo, le modèle, les autres modules ou le backlog (intégration par le parent); prétendre à une validation Word; tableaux/sections/en-têtes ou pagination incrémentale; UI riche.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat | Erreur |
|---|---|---|---|
| Espacements | after=200, before=240 twips convertis | maximum; contextual précédent seul: 40 twips; deux côtés: zéro | arithmétique vérifiée |
| Page suivante | lignes de hauteur connue, avant non nul | avant supprimé après saut auto, conservé après explicite; baseline correctement translatée | entrée incohérente rejetée |
| Solidarité | keep-next, keep-lines, veuves/orphelines | groupe déplacé si possible; ≥2 lignes à chaque extrémité si possible | diagnostic si impossible |
| Sauts | avant/après paragraphe et indices de lignes | saut à l’endroit exact, aucune ligne perdue ou doublée | indice invalide rejeté |
| Cas extrêmes | ligne plus haute qu’une page, chaîne keep trop longue | progression finie et dépassement signalé | limites explicites |
| Malformé | hauteur nulle, budget nul, i64 extrême, règles manquantes | aucun résultat partiel | erreur typée |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs` — US-210: baseline contient avant, height contient avant/après; ne pas modifier.
- `crates/text-layout/src/pagination.rs` — nouveau paginateur et types publics.
- `crates/text-layout/tests/pagination.rs` — tests numériques indépendants des polices.
- `docs/us-212-pagination.md` — contrat, exemple et limites Word.

## Tasks & Acceptance

- [x] `crates/text-layout/src/pagination.rs` — implémenter API, règles, diagnostics et limites.
- [x] `crates/text-layout/tests/pagination.rs` — couvrir chaque ligne de matrice et ordre/conservation des lignes.
- [x] `docs/us-212-pagination.md` — documenter coordonnées, policies et fidélité non mesurée.

Given des layouts valides, when paginate est appelé, then chaque ligne est placée exactement une fois et toutes les coordonnées sont vérifiées.
Given des règles impossibles, when la pagination avance, then elle termine et signale les contraintes relâchées.
Given un corpus Word absent, when les tests numériques passent, then les critères fonctionnels seuls sont documentés; US-212 reste partielle.

## Implementation Notes

Propriétés OOXML vérifiées dans Microsoft Learn (ContextualSpacing et SpacingBetweenLines); exemple contextuel 200/240 repris numériquement, pas de corpus Word simulé. API de règles séparée du modèle pour isoler les modules en parallèle. Shell cmd.exe/login:false; tests ciblés uniquement, root valide workspace. Aucun commit/push par cet agent.

## Spec Change Log

## Review Triage Log

Voir [revue triée](rich-page-geometry-review.md) : 12 constats indépendants, chacun conservé et réfuté ou corrigé; six constats complémentaires de l’intégrateur et régressions. Correctifs exécutés par les développeurs propriétaires des modules.

## Verification

- `cargo test -p atelier-text-layout --test pagination --locked --offline`
- `cargo clippy -p atelier-text-layout --all-targets --locked --offline -- -D warnings`

Validation finale : 707 tests workspace, 57 tests text-layout release, 20 contrôles passés sur source immuable; [reçu](rich-page-geometry-validation.json) et [audit des matrices](rich-page-geometry-matrix.json). La clôture de cette spec fonctionnelle ne clôture pas les critères Word ouverts du backlog.
