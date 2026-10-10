---
title: 'US-070 — Arbre de nœuds indexé'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
baseline_commit: 'NO_VCS'
review_loop_iteration: 0
context: ['C:/mon-bureau/Cargo.toml']
---

<frozen-after-approval reason="Intention autorisée par la demande de développer les US">

## Intent

Implémenter US-070 : conteneur générique de blocs dans un arbre équilibré, indexé par position et identifiant stable. Insertion, suppression, accès par position et identifiant doivent rester O(log n) ; mesurer 10 000 insertions aléatoires dans 100 000 blocs, objectif <50 ms.

## Boundaries & Constraints

Toujours : Rust stable, std/core uniquement, zéro dépendance externe, SPDX AGPL-3.0-or-later, forbid(unsafe_code), portabilité Windows/WASM/Linux. Le coordinateur orchestre BMAD/Graphify : implémenter directement sans workflow/agent supplémentaire. Propriété exclusive : `crates/model/**`, `docs/us-070-indexed-tree.md`, Implementation Notes de ce fichier. Ne pas modifier les fichiers partagés, Cargo.lock ou backlog.

API générique publique `IndexedTree<T>`, `NodeId` opaque, new/len/is_empty, insertion par position retournant l'identifiant, suppression par position, get(position), get_by_id(id), position_of(id). Les opérations invalides sont signalées sans panique ni mutation partielle. Identifiants jamais recyclés pendant la vie du conteneur, y compris après suppression de tous les éléments ; débordement signalé. Une insertion/suppression déplace les positions sans changer les identités des survivants. Arbre équilibré garantissant O(log n), par exemple AVL avec tailles de sous-arbres et index d'identifiants BTreeMap ; un Vec déplacé ou treap simplement aléatoire ne satisfait pas la garantie. Aucun parcours linéaire caché dans les opérations demandées. Réutiliser les emplacements libres pour borner la mémoire par le pic d'éléments vivants.

Banc autonome en exemple Cargo : fixture de 100 000 blocs hors chronométrage, 10 000 positions pseudo-aléatoires déterministes calculées hors mesure et valides à chaque insertion, travail observable via black_box. Chauffe puis médiane d'au moins cinq mesures release ; afficher taille, nombre d'opérations, médiane et seuil réussi/échoué. Décrire matériel/OS et protocole, sans promettre un seuil universel. Le coordinateur mesure indépendamment sur cet hôte.

Hors périmètre : transactions US-071, undo, sérialisation, IDs persistants entre documents, UI et types de blocs métier.

## I/O & Edge-Case Matrix

| Situation | Résultat attendu |
|---|---|
| Vide, insertion début/milieu/fin | Ordre et positions exacts, identifiants uniques |
| Suppression feuille/racine/deux enfants | Identités survivantes conservées, index cohérent |
| Position hors limites, ID absent ou supprimé | Erreur/None documenté, pas de mutation |
| Supprimer tout puis insérer | Aucun ID réutilisé ; mémoire d'arène réutilisée |
| Longue séquence mixte déterministe | Résultats égaux à un Vec oracle ; tailles, parents, hauteur et balance valides |
| Insertion monotone et rotations | Hauteur logarithmique, accès position/ID correct |
| Compteur d'ID épuisé | Erreur explicite et état conservé (test interne) |

</frozen-after-approval>

## Code Map

- `crates/model/src/lib.rs` : squelette vide, conteneur et API à créer.
- `crates/model/examples/bench_indexed_tree.rs` : nouveau banc std-only, aucune dépendance atelier-bench nécessaire.
- `crates/model/Cargo.toml` : pas de changement nécessaire.
- `SPEC.md` §6.3 : IDs stables et arbre équilibré positionnel, condensés ici ; aucun consommateur existant.

## Tasks & Acceptance

- [x] `crates/model/**` — conteneur équilibré, index d'identifiants, erreurs et invariants.
- [x] Tests — matrice complète avec oracle indépendant, invariants après mutations et cas de suppression à deux enfants.
- [x] Exemple de banc — protocole exact et mesure reproductible.
- [x] `docs/us-070-indexed-tree.md` — API, complexité argumentée, mémoire, protocole et limites ; ne pas annoncer de validation distante.

Critères : O(log n) démontré structurellement et par invariants ; stabilité d'identité prouvée ; banc exécuté en release et résultat documenté honnêtement. Si >=50 ms, signaler le critère incomplet au coordinateur pour optimisation avant clôture.

## Implementation Notes

## Spec Change Log

## Review Triage Log

- B6 — low, patch : Les IDs sont uniques dans leur arbre, conformément au périmètre document. La précondition d’origine est désormais explicite sur NodeId, get_by_id, position_of et dans le guide ; test d’IDs locaux ajouté. Une unicité globale ne fait pas partie de l’US.
- B7 — false, rejected : Le test deleting_a_two_child_node_preserves_successor_identity contient déjà assert_eq!(tree.remove(position), Ok(removed_value)) et assert_eq!(tree.get_by_id(removed_id), None), avant de vérifier tous les survivants.
- B8 — false, rejected : La documentation initiale précise déjà la conservation de capacité au pic historique. Le stockage radix conserve maintenant ses pages et réutilise les slots, avec espace O(pic de slots), pas une promesse de libération immédiate de toute mémoire après suppression.
- R1 — medium, patch : Revue indépendante du coordinateur : les croissances Vec pouvaient recopier O(n) slots. Remplacement par pages radix fixes, accès borné par usize::BITS et liste libre intrusive ; revue ciblée sans défaut concret, 240 000 opérations oracle et benchmark release après correction réussis.

## Verification

`cargo test -p atelier-model --locked --offline`, `cargo clippy -p atelier-model --all-targets --locked --offline -- -D warnings`, `cargo fmt -p atelier-model`, `cargo run --release -p atelier-model --example bench_indexed_tree --locked --offline`. Validation globale par coordinateur après livraison.

Validation indépendante finale : 67 tests du workspace passent ; builds MSVC/WASM/musl, Clippy hôte/WASM, fmt et SPDX réussis. Tests musl compilés seulement ; CI distante non exécutée. Preuves : foundations-validation.json et foundations-review.md.
