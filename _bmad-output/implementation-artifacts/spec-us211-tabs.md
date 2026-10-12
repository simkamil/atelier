---
title: 'US-211 — Tabulations des lignes riches'
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

**Problem:** Les tabulations sont ignorées par US-210, ce qui fait disparaître l’alignement des textes qui en contiennent.

**Approach:** Calculer les taquets et leur avance pendant la coupure et le placement des lignes, exposer des réglages explicites et les décorations des points de suite/barres. Garder l’API existante avec tabulations par défaut. La fidélité du dépassement de marge à Word reste non mesurée sans Word; documenter ce critère ouvert plutôt que fermer toute l’US.

## Boundaries & Constraints

**Always:** Unités 1/64 pt. Taquets explicites gauche/centre/droite/décimal virgule/barre; points de suite points/tirets/trait. Intervalle par défaut 1,25 cm arrondi au plus proche dans les unités du moteur (=2268). Taquets mesurés depuis la marge gauche de la zone de contenu, avec retraits de paragraphe pris en compte par la position courante. Les barres sont des décorations à position fixe, ne déplacent pas le texte et ne consomment pas une tabulation. Un taquet d’alignement est strictement après le pen courant; taquets triés et positions uniques validés. Les réglages sont une API séparée de ParaProps dans ce lot. L’alignement centre/droite/décimal utilise le segment suivant jusqu’à la prochaine tabulation ou coupure obligatoire. La coupure et le placement utilisent la même mesure; un alignement ne fait pas revenir le pen derrière le contenu précédent. Une tabulation terminale garde son avance et sa source, contrairement aux espaces terminaux ordinaires. Décorations exposées séparément des glyphes de texte, avec positions/lignes et limites explicites. Aucun caractère artificiel ajouté au modèle. API existante layout_paragraph conserve son type ParagraphLayout; une API avec réglages retourne layout + décorations sans changer LayoutLine/ParagraphLayout (agents voisins construisent ces types). Intégrer les avances des tabulations dans des glyphes de contrôle identifiables ou une autre représentation compatible avec le curseur; documenter le choix.

**Never:** Modifier modèle/Cargo/backlog/pagination/caret; dépendance externe/unsafe; copier le moteur entier; rendu UI ou import OOXML des taquets (travaux ultérieurs). Ne pas affirmer que le comportement observé de Word a été testé. Pas de modifications Graphify par l’agent.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat | Erreur |
|---|---|---|---|
| Défaut | a\tb et retraits | prochain multiple de2268, après le pen; avance source visible au curseur | calcul vérifié |
| Alignements | segments de largeur connue, centre/droite/décimale | géométrie numérique au taquet; virgule alignée; sans virgule comme droite | réglages invalides rejetés |
| Décorations | barre et leaders points/tirets/trait | barre fixe sans déplacement; leader sur espace réellement consommé | budget décorations |
| Sources | plusieurs tabs, tabs terminaux, graphèmes et ligatures | plages UTF-8 conservées, wrapping cohérent et progression | budget travail/glyphes/lignes |
| Marge | taquet au-delà de la largeur ou segment trop long | règle déterministe documentée, aucun blocage; comparer à Word plus tard | overflow typé |
| Malformé | intervalle nul, doublons, positions i64 extrêmes | aucun résultat partiel | erreur typée |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs` — shape_range/glyph_advance/make_fragments et boucle de coupure à modifier ensemble; tabs actuellement ignorées et trim_trailing_space les supprime.
- `crates/text-layout/src/tabs.rs` — réglages, résolution, sortie de décorations.
- `crates/text-layout/tests/tabs.rs` — tests de géométrie et wrapping.
- `crates/text-layout/tests/layout.rs` — remplacer seulement le test qui attend tabs unsupported, préserver les autres attentes.
- `docs/us-211-tabs.md` — API, exemple, coordonnées, overflow non comparé Word.

## Tasks & Acceptance

- [x] `crates/text-layout/src/lib.rs`, `src/tabs.rs` — implémenter mesure/placement/décorations; déclarer `pub mod tabs;` uniquement, root exporte pagination/caret.
- [x] `crates/text-layout/tests/tabs.rs`, `tests/layout.rs` — couvrir matrice avec attentes numériques et garder US-210 valide.
- [x] `docs/us-211-tabs.md` — contrat et limites sans surévaluation du statut.

Given les mêmes entrées, when un préfixe est mesuré puis placé, then ses avances et alignements sont identiques.
Given des tabulations, when elles sont mises en page, then elles ne disparaissent pas des positions et les autres lignes ne régressent pas.
Given les tests locaux passent, when le backlog est actualisé, then le comportement de dépassement de marge Word reste ouvert jusqu’à comparaison réelle.

## Implementation Notes

L’agent possède lib.rs/tabs.rs et ses tests/docs seulement. Shell cmd.exe/login:false. Tests ciblés, aucun commit/push; root coordonne export des modules voisins et validation totale. Propositions de représentation à coordonner avec l’agent du curseur via root avant modification de types publics.

## Spec Change Log

## Review Triage Log

Voir [revue triée](rich-page-geometry-review.md) : 12 constats indépendants, chacun conservé et réfuté ou corrigé; six constats complémentaires de l’intégrateur et régressions. Correctifs exécutés par les développeurs propriétaires des modules.

## Verification

- `cargo test -p atelier-text-layout --test tabs --test layout --locked --offline`
- `cargo clippy -p atelier-text-layout --all-targets --locked --offline -- -D warnings`

Validation finale : 707 tests workspace, 57 tests text-layout release, 20 contrôles passés sur source immuable; [reçu](rich-page-geometry-validation.json) et [audit des matrices](rich-page-geometry-matrix.json). La clôture de cette spec fonctionnelle ne clôture pas les critères Word ouverts du backlog.
