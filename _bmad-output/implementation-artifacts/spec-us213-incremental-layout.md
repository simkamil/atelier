---
title: 'US-213 — Cache de paragraphes et pagination réutilisable'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 1
baseline_commit: '2ae5aecf73f7ff9b7849bebde7facbc54330d5b2'
context: []
---

<frozen-after-approval reason="Poursuite autonome et parallèle autorisée par l’utilisateur">

## Intent

**Problem:** Chaque appel recalcule tous les paragraphes et toute la pagination, même pour une modification locale. Un cache de texte seul serait invalide après un changement de propriétés calculées ou de polices.

**Approach:** Ajouter une session de layout incrémental et une couture de reprise dans le paginateur existant. Réutiliser les paragraphes identiques, reprendre suffisamment tôt pour les dépendances de solidarité et s’arrêter réellement à une frontière stable. Préserver l’égalité au calcul complet pour tous les cas; documenter les replis conservateurs et mesurer séparément un workload CPU de 300 pages.

## Boundaries & Constraints

**Always:** Agent propriétaire de incremental.rs, pagination.rs (couture de reprise), tests/incremental.rs, tests de pagination nécessaires, exemple benchmark et docs/us-213-incremental-layout.md. Parent propriétaire de lib.rs, Cargo et font/manager.rs. Entrées ordonnées identifiées par IDs stables uniques, paragraphes, propriétés résolues, LayoutOptions, TabSettings et ParagraphRules; options globales et FontManager explicitement fournis. Cache par identité + égalité sémantique complète (texte, runs, propriétés, defaults/options/tabs) sans collision de hash non vérifiée. Parent fournira `FontManager::cache_revision() -> u64`, un jeton global distinct entre managers et renouvelé après toute mutation de polices réussie; ne pas utiliser l’adresse du manager. Seuls les paragraphes nouveaux/modifiés ou globalement invalidés sont reshaped. Une erreur ne publie aucun état ni cache partiel; anciens résultats restent utilisables. API expose compteurs vérifiables (paragraphes reshaped/réutilisés, pages réellement recalculées/réutilisées, origine de reprise et motif de repli). Pour les éditions locales avec IDs stables et options compatibles: reprise à la première page susceptible de changer; rewind conservateur avant le paragraphe traversant la page et avant toute chaîne keep-with-next affectée, en incluant l’ajustement veuve/orpheline de la page précédente. La couture permet de reprendre le flux et d’arrêter son exécution quand page finalisée ET état d’entrée de la suite (position source, paragraphes précédents/espacement, saut en attente, indices) coïncident, au-delà de la dernière modification. Réutiliser la suite, sans exécuter son flux. Précalcul/validation O(n) acceptés et déclarés séparément, jamais comptés comme pages réutilisées s’ils ont été repaginés. Repli complet autorisé et explicitement compté pour insertion/suppression/réordonnancement, options globales/polices et cas de frontière non représentables; la forme des paragraphes inchangés reste cachée. Les diagnostics doivent être identiques en contenu ET ordre au calcul complet. Reprise in-paragraphe seulement si état complet sûr, sinon rewind au début du paragraphe et à sa première page. Budgets et arithmétique existants conservés sur le résultat total et le travail effectif. Implémentation de paginate complète et incrémentale partage la même logique de flux.

**Never:** Appeler paginate sur tout le document puis comparer et déclarer l’arrêt anticipé. Compteurs simulés, réplication divergente de l’algorithme, cache par indice ou FontId seul, dépendance externe/unsafe, modification du modèle ou des exports/Cargo/Graphify/backlog, commits. Pas de promesse UI <16ms sans mesure de frappe réelle sur la machine §6.16.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|---|---|---|---|
| Initial / identique | premier calcul puis entrées identiques | complet puis zéro shaping et zéro flux repaginé, résultat égal | aucune |
| Frappe locale | même nombre de lignes au milieu de 300 pages | un paragraphe reshaped, début conservé, arrêt effectif et suite réutilisée | aucune |
| Propagation | ajout/suppression de lignes ou changement de règles/espacement | reprise dépendances incluses, suite seulement si état stable | aucune |
| Contraintes | keep-next/together, veuves/orphelines, sauts manuels et paragraphes multipages | égalité stricte avec paginate, conservation source | aucune |
| Cache | changement de defaults/options/tabs/polices ou nouveau manager | invalidation appropriée, aucune géométrie périmée | aucune |
| Structure | insertion/suppression/réordre avec IDs stables | géométrie inchangée réutilisée, pagination complète déclarée | IDs doublons rejetés |
| Erreur | layout invalide, budgets insuffisants, arithmétique extrême | ancien résultat/cache inchangé, pas de panic | erreur typée |
| Mesure | benchmark release document réellement paginé en 300 pages | timings complets/incrémentaux, compteurs et workload publiés | aucune cible UI déclarée atteinte |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs` — layout, tabs et types publics; exports intégrés par le parent.
- `crates/text-layout/src/pagination.rs` — prévalidation/prefixes, chaînes keep et flux monolithique à rendre reprenable.
- `crates/font/src/manager.rs` — jeton de cache intégré par le parent.
- `crates/text-layout/src/incremental.rs` — session, cache, invalidation et résultats.
- `crates/text-layout/tests/incremental.rs` — scénarios et comparaison stricte complète.
- `crates/text-layout/examples/incremental_benchmark.rs` — workload release autonome et compteurs.

## Tasks & Acceptance

- [x] `crates/text-layout/src/{incremental,pagination}.rs` — API atomique et flux reprenable partagé.
- [x] `crates/text-layout/tests/incremental.rs` — couvrir la matrice et des séquences déterministes d’éditions comparées au calcul complet.
- [x] `crates/text-layout/examples/incremental_benchmark.rs` — mesurer un vrai document 300 pages sans assertion de timing fragile.
- [x] `docs/us-213-incremental-layout.md` — API, reprise, invalidations, replis, timings et limite UI.

L’édition locale démontrée évite effectivement la majorité de la pagination et du shaping. Tout scénario complexe donne le même résultat qu’un calcul complet, au besoin par un repli déclaré. US-213 reste partielle tant que la latence utilisateur sur la machine de référence n’est pas mesurée.

## Implementation Notes

Le parent a déjà chargé et rendu bmad-build pour cette session, puis exécuté les étapes de planification et de délégation. Cette tâche est la sous-étape d’implémentation déjà déléguée: lire cette spec et coder, sans relancer render_skill.py ni reprendre le workflow depuis le début.

Shell cmd.exe/login:false, cwd C:/mon-bureau. Lire Graphify avant les sources. Tests ciblés seulement; parent valide l’ensemble. Signaler tôt toute impossibilité de couture sûre, sans remplacer l’objectif par un faux cache. L’agent peut adapter les noms d’API aux types existants, sans changer ces garanties.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-text-layout --test incremental --test pagination --locked --offline`
- `cargo run -p atelier-text-layout --example incremental_benchmark --release --locked --offline`

Tranche API validée le 2026-10-12 : 739 tests workspace Windows, 78 text-layout release, 21 contrôles. L’US utilisateur reste partielle selon docs/backlog. Revue et matrice : `rich-incremental-borders-review.md`, `rich-incremental-borders-matrix.json`.
