---
title: 'US-202 — Fabrique de document riche par défaut'
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

**Problem:** `RichDocument::new()` crée un arbre vide sans les réglages ni le catalogue de styles demandés par US-202. Le modèle ne possède pas de champs typés pour la géométrie de section, la langue ou le style suivant.

**Approach:** Fournir une fabrique autonome renvoyant le document, son moteur de styles et des métadonnées typées de présentation. Préserver les structures publiques existantes. Cette tranche rend les valeurs disponibles au moteur; US-202 reste partielle jusqu’au raccordement du bouton Nouveau à l’éditeur riche.

## Boundaries & Constraints

**Always:** Ajouter `rich/defaults.rs` et un test d’intégration dédié. Le parent intègre les exports dans rich/mod.rs. Utiliser les types de styles existants; document valide, un paragraphe vide en style Normal, résolvable immédiatement. Géométrie exacte en centièmes de millimètre: A4 21000×29700, marges 2500 sur quatre côtés, portrait. Langue `fr-FR`. Police Carlito, 22 demi-points, 160 twips après. Métadonnée d’interligne exacte 108/100; valeur Auto du modèle 259/240, arrondie au plus proche, limitation explicitement documentée. Aucune sémantique inventée dans les attributs opaques. Catalogue complet avec identifiants stables, noms français et métadonnées de style suivant dont chaque cible existe. Styles exigés: Normal, Titre, Sous-titre, Titre 1–9, Citation, Citation intense, Accentuation, Élevé, Légende, En-tête, Pied de page, Lien hypertexte, Paragraphe de liste, Sans interligne, Grille du tableau. Respecter les catégories paragraphe/caractère/tableau. Normal et titres ont pour suivant Normal; en-tête/pied et paragraphe de liste peuvent se suivre eux-mêmes. Les autres valeurs de style sont des choix explicites du modèle de document Atelier, sans prétendre reproduire une version non identifiée de Word. Style Sans interligne: Auto240, après0. Styles Accentuation/Élevé de caractère et Grille du tableau de tableau; lien hypertexte de caractère. Fabriques indépendantes et déterministes.

**Never:** Modifier les champs des structures existantes, libérer les invariants de validation, ajouter des dépendances, écrire lib.rs/rich/mod.rs/Cargo/backlog/Graphify ou effectuer des commits. Pas de raccordement UI ou import/export OOXML dans cette tranche. Ne pas présenter 259/240 comme exactement 1,08.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|---|---|---|---|
| Fabrique | document neuf | arbre valide, un paragraphe vide Normal et moteur utilisable | aucune |
| Géométrie | paramètres initiaux | A4 portrait et quatre marges exactes 25 mm, langue fr-FR | aucune |
| Résolution | paragraphe Normal sans format direct | Carlito 11 pt, après8pt, Auto259 et ratio exact108/100 disponible | aucune |
| Catalogue | tous les noms de l’US | chaque nom présent une fois, catégorie correcte, références existantes sans cycle | aucune |
| Suite | styles paragraphe | métadonnées suivantes résolubles, Normal après les titres | aucune |
| Sans interligne | résolution du style | Auto240 et après0, sans modifier Normal | aucune |
| Isolation | deux fabriques puis mutation de l’une | seconde inchangée, identifiants/catalogue initiaux déterministes | aucune |

</frozen-after-approval>

## Code Map

- `crates/text-model/src/rich/{document,styles,attrs,node}.rs` — arbre, styles et unités existants; Section n’a pas de géométrie et Style n’a pas de suivant.
- `crates/text-model/src/rich/defaults.rs` — nouvelle fabrique et métadonnées typées.
- `crates/text-model/tests/default_document.rs` — contrat public et matrice.
- `docs/us-202-default-document.md` — valeurs, arrondis, exemple compilable ou testé et limite UI.

## Tasks & Acceptance

- [x] `crates/text-model/src/rich/defaults.rs` — fabrique, métadonnées et catalogue complet.
- [x] `crates/text-model/tests/default_document.rs` — couvrir chaque ligne de matrice, résolution et isolation.
- [x] `docs/us-202-default-document.md` — documenter API, choix et limites.

La fabrique est utilisable sans configuration supplémentaire par un appelant Rust, sans casser les consommateurs existants. L’état du backlog distingue la fabrique validée du flux utilisateur Nouveau encore absent.

## Implementation Notes

Shell cmd.exe/login:false, cwd C:/mon-bureau. Lire le rapport Graphify avant les sources. Tests ciblés seulement; validation générale par le parent. Si une surface de code diffère, adapter au contrat des types existants et signaler toute impossibilité réelle.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-text-model --test default_document --locked --offline`
- `cargo clippy -p atelier-text-model --all-targets --locked --offline -- -D warnings`

Tranche API validée le 2026-10-12 : 739 tests workspace Windows, 78 text-layout release, 21 contrôles. L’US utilisateur reste partielle selon docs/backlog. Revue et matrice : `rich-incremental-borders-review.md`, `rich-incremental-borders-matrix.json`.
