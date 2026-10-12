---
title: 'Intégration des tabulations, pages et curseurs riches'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: '13c605b19926bd08b439fc711d132d5a638f8e5e'
context: []
---

<frozen-after-approval reason="L’utilisateur demande de poursuivre plusieurs US en parallèle avec Luna moyen, validation et docs">

## Intent

**Problem:** US-210 produit des lignes riches isolées. Pour préparer l’éditeur riche, leurs tabulations, pages et positions de curseur doivent partager une géométrie cohérente.

**Approach:** Développer US-211, US-212 et US-214 dans trois modules parallèles, puis fournir un adaptateur et des tests communs. Actualiser les critères réellement validés et pousser le lot sur main. La poursuite multi-US et le push sont autorisés dans la conversation; aucun changement du périmètre produit des specs.

## Boundaries & Constraints

**Always:** Réutiliser US-210 et les polices embarquées; garder coordonnées 1/64pt et offsets UTF-8 par paragraphe; aucun paquet externe/unsafe. Préserver les travaux et worktrees d’autres outils. Agents GPT-6 Luna moyen, fichiers possédés séparément; root seul intègre et publie. Les contrats détaillés sont les trois specs US de ce lot. Valider l’interopérabilité avec un vrai layout riche et des attentes numériques, plus build/tests/lints natifs et compilation WASM/musl. Actualiser Graphify selon les directives utilisateur; contrôler les dépendances Cargo et les extrémités du graphe. Conserver les preuves d’échecs et leur correction.

**Never:** Clôturer le critère Word d’US-212 ou le dépassement Word d’US-211 sans référence authentique; changer la limite de taille WASM pour faire passer un contrôle; annoncer une UI riche disponible alors que seul TXT est raccordé; modifier les limitations historiques US-004/111/210 non résolues par le lot.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat attendu | Erreur |
|---|---|---|---|
| Chaîne complète | paragraphes riches, tabulations et hauteur de page réduite | sources conservées, plusieurs pages, curseurs/hit-tests sur la bonne page | erreur typée si entrée incompatible |
| Espacement | before/after déjà inclus par layout | aucun double comptage par pagination ou vue de curseur | géométrie incohérente rejetée |
| Sélection | plage sur pages successives et lignes courtes | rectangles page-local et colonne préférée conservée | bornes invalides rejetées |
| Régression | tests US-210, DOCX et éditeur TXT existants | restent valides, artefact embarqué cohérent | correction avant publication |
| Publication | tests locaux et Word absent | US-214 peut être close si tous critères passent; US-211/212 partielles et preuves explicites | CI distante non déclarée verte sans exécution |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs`, `tabs.rs`, `pagination.rs`, `caret.rs` — modules agents; root ajoute exports sans écraser leur travail.
- `crates/text-layout/src/flow.rs` — adaptateur de placements vers vues de curseur.
- `crates/text-layout/tests/flow.rs` — preuve de chaîne intégrée.
- `docs/us-211-tabs.md`, `docs/us-212-pagination.md`, `docs/us-214-caret-geometry.md` — contrats agents.
- `US-V0-V1.md`, `README.md`, `docs/PROTOTYPE-PROGRESS.md`, `docs/DEVELOPMENT.md` — statuts et limites root.
- `_bmad-output/implementation-artifacts/refresh-graph.py` — actualisation canonique après CLI Graphify, préserve sémantique et AST/Cargo.

## Tasks & Acceptance

- [x] Trois specs US et modules — implémentés en parallèle et tests de chaque matrice passés.
- [x] `crates/text-layout/src/flow.rs`, `tests/flow.rs` — API d’adaptation et régressions intégrées.
- [x] Artefacts de revue — trois lectures indépendantes Luna moyen, chaque constat trié, correctifs vérifiés.
- [x] Validation immuable — workspace natif, WASM, lints, SPDX, guard, musl compilé et régression UI/ABI existante.
- [x] Docs/backlog/Graphify — critères exacts et preuve de dépendances sans cycle Cargo.
- [x] Git main — commit et push, conserver les limitations CI distantes observées.

Given les trois modules, when le test de chaîne s’exécute, then les avances de tabs, placements de pages et positions du curseur sont cohérents.
Given la source gelée, when la validation se termine, then le reçu enregistre les hashes inchangés et tous les contrôles exécutés.
Given une limite non mesurable ici, when la documentation est publiée, then cette limite reste explicitement ouverte.

## Implementation Notes

Le graphe relie les trois US à US-210; US-212 débloque US-213/215/262/272/280/284/285, US-214 les interactions riches US-220/221/222/350. Aucun changement de dépendance Cargo prévu. Les espacements contextuels suivent l’exemple numérique Microsoft Learn; la fidélité Word n’en est pas déduite.

## Spec Change Log

## Review Triage Log

Voir [revue triée](rich-page-geometry-review.md) : 12 constats indépendants, chacun conservé et réfuté ou corrigé; six constats complémentaires de l’intégrateur et régressions. Correctifs exécutés par les développeurs propriétaires des modules.

## Verification

- `py _bmad-output/implementation-artifacts/validate-rich-page-geometry.py` — contrôles passés sur source immuable.
- `py -m graphify . --code-only`, puis refresh canonique et contrôle d’intégrité — nouveaux modules référencés, aucun cycle Cargo.

Validation finale : 707 tests workspace, 57 tests text-layout release, 20 contrôles passés sur source immuable; [reçu](rich-page-geometry-validation.json) et [audit des matrices](rich-page-geometry-matrix.json). La clôture de cette spec fonctionnelle ne clôture pas les critères Word ouverts du backlog.

Publication : code et docs poussés sur `origin/main`, commit `9b1bcd249fc12702b5e92220e60af03809b6afc3`. La [CI de ce commit](https://github.com/simkamil/mon-bureau/actions/runs/38267149521) refuse les trois jobs avant affectation de runner pour paiement/plafond Actions; aucun contrôle distant exécuté. [Reçu](rich-page-geometry-ci.json). Les validations locales restent établies; aucun réglage de facturation modifié.
