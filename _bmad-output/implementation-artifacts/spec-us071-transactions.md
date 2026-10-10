---
title: 'US-071 — Opérations et transactions atomiques'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/docs/us-070-indexed-tree.md'
baseline_commit: 'b9297cd3a94293a913b2b79ba66c06e3acf4024c'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le modèle possède l'arbre indexé US-070, mais aucune frontière commune ne garantit qu'une modification est annulable ou qu'une commande composée reste indivisible après erreur. US-072 et US-074 dépendent d'un contrat d'opérations exploitable par le futur modèle de texte.

**Approach:** Ajouter une abstraction générique d'état transactionnel et d'opération inversible, puis une transaction ordonnée dont l'application valide ou restaure l'état exact. Le contrat reste indépendant de `IndexedTree<T>` et des futurs types de nœuds ; un adaptateur concret sur l'arbre doit démontrer son utilité dès cette story.

## Boundaries & Constraints

**Always:** Rust stable, `core`/`std` uniquement, `#![forbid(unsafe_code)]`, SPDX AGPL-3.0-or-later. Après échec atomique, restaurer le checkpoint exact, compteur compris : les identités provisoires ne sont jamais publiées. Après une transaction réussie puis son inverse, restaurer le contenu logique (ordre, valeurs et IDs préexistants), tout en conservant le compteur monotone pour ne jamais réattribuer une identité publiée. Les opérations ne produisent aucun effet externe au modèle. Toute erreur est un résultat explicite. Sérialisation déterministe, versionnée, bornée en taille et validée avant application.

Propriété exclusive : `crates/model/**`, `docs/us-071-transactions.md` et ce spec. Le coordinateur gère les fichiers partagés, le backlog, Graphify/BMAD, Cargo.lock et Git ; pas d'agent supplémentaire ni de modification hors périmètre. Décision interne : checkpoint par clone exact pour l'adaptateur `IndexedTree<T: Clone>` seulement ; l'API générique laisse chaque état définir son checkpoint. Documenter le coût proportionnel à l'arène allouée et aux valeurs copiées ; les mutations AVL restent O(log n). Toutes les commandes Cargo utilisent `--locked --offline`, formatage limité à la crate.

**Never:** Coupler l'API au seul `IndexedTree`, exiger que ses valeurs implémentent `Clone` pour toutes les opérations, exposer les champs internes de `NodeId`, ajouter une dépendance, ou promettre journal de récupération durable, UI undo/redo ou coédition (US-072/074 et suivantes).

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|----------|--------------|-----------------|--------|
| Transaction valide | opérations applicables sur état initial | Toutes appliquées dans l'ordre ; inverses composées permettent le retour exact à l'état initial | Aucune |
| Échec d'application | une opération échoue après des opérations réussies | Transaction restaurée exactement à son checkpoint ; aucune mutation partielle visible | Erreur de l'opération |
| Échec de préparation / inversion | inverse indisponible ou invalide avant mutation | État inchangé ; transaction refusée | Erreur typée |
| État de départ absent | opération référant un ID ou nœud absent | Aucun état fabriqué implicitement ; résultat inchangé | Erreur typée de précondition |
| Données sérialisées invalides | version inconnue, longueur excessive, données tronquées/incohérentes | Rejet avant application, consommation bornée | Erreur de format/version/limite |

</frozen-after-approval>

## Code Map

- `crates/model/src/lib.rs` — `IndexedTree<T>`, `NodeId` opaque, erreurs et point d'intégration de l'API générique.
- `docs/us-070-indexed-tree.md` — contrats d'identité, complexités AVL/radix et exigences de non-régression.
- `SPEC.md` §6.3 — chaque changement passe par une opération inversible et une commande est une transaction.
- `US-V0-V1.md` US-071 à US-074 — critères de story, dépendances undo/redo et journal.

## Tasks & Acceptance

**Execution:**
- [x] `crates/model/src/lib.rs` — définir `Operation<S>` avec application, inversion et sérialisation versionnée, plus erreur typée ; définir une transaction générique réutilisable et un checkpoint/restore exact, sans supposer que chaque `S` ou valeur est clonable.
- [x] `crates/model/src/lib.rs` — démontrer une opération réelle sur `IndexedTree` (insertion/suppression par identité ou rang), en restaurant l'association ID/valeur et le rang ; garder les erreurs d'inversion et de précondition sans mutation.
- [x] `crates/model/src/lib.rs` — ajouter codec borné avec limites explicites, rejet version inconnue/troncature/données invalides avant mutation ; aucun `unwrap`/panique sur entrée.
- [x] `crates/model/src/lib.rs` — tests déterministes et pseudo-aléatoires : apply puis inverse égale état initial exact ; échec au milieu, échec de préparation/inversion et décodage invalide conservent l'état.
- [x] `docs/us-071-transactions.md` — décrire le contrat public, les garanties, limites de codec et exemples concrets.

**Acceptance Criteria:**
- Given une transaction de plusieurs opérations valides, when elle est appliquée puis inversée, then le contenu logique et les IDs préexistants sont restaurés ; une identité nouvellement publiée n'est jamais attribuée à un autre nœud. Un échec restaure aussi le compteur du checkpoint, dont les IDs provisoires n'ont pas été publiés.
- Given une opération qui échoue après une mutation précédente de la transaction, when la transaction est exécutée, then le checkpoint restaure l'état exact et retourne l'erreur sans panique.
- Given une insertion ou suppression réelle dans `IndexedTree`, when l'opération est inversée, then ordre, valeurs et IDs d'origine sont restaurés, y compris après suppression d'un nœud à deux enfants.
- Given des octets dépassant une limite, tronqués ou d'une version inconnue, when ils sont décodés, then le décodeur retourne une erreur bornée sans modifier l'état.
- Given les plateformes Windows, WASM et Linux, when la crate modèle est compilée, then l'API n'exige aucune dépendance externe ni code `unsafe`.

## Implementation Notes


## Spec Change Log


## Review Triage Log

| ID | Source | Constat | Évaluation | Résolution / preuve |
| --- | --- | --- | --- | --- |
| B9 | Aveugle | « charges utiles vérifiées » pourrait impliquer les préconditions de l'état dès decode. | `low`, précision utile ; `patch`. | Corrigé : decode valide le format sans état ; invert/apply contrôlent les préconditions sur l'état. |
| R1 | Coordinateur | Le test aléatoire de transactions repartait d'un état vide après chaque annulation, sans suppression ni vraie séquence. | `medium`, confirmé ; `patch`. | Test renforcé : arbre peuplé, 200 transactions de 3–6 opérations, Insert/Remove, codec, modèle Vec, ordre/valeurs/IDs, compteur monotone et non-réutilisation après annulation. |

Revue croisée du lot : [journal complet](next-batch-review.md). Aucun constat différé.


## Design Notes

Le rollback s'appuie sur un checkpoint restaurable à coût/stratégie définis par l'état, car appliquer une suite d'inverses fallibles ne garantit pas une restauration exacte. L'abstraction peut utiliser des inverses possédant les données retirées, mais le contrôleur de transaction restaure depuis le checkpoint en cas d'erreur. Un adaptateur d'arbre peut employer une copie `T: Clone` au départ, tant que cette contrainte reste locale ; si elle est inacceptable, fournir un snapshot interne exact sans rendre les opérations de test la seule implémentation utilisable. Pour `IndexedTree`, les mutations structurelles restent O(log n) ; checkpoint complet par clone coûte O(n), explicitement distinct du coût de l'opération. Évaluer un journal interne ultérieurement si ce coût est bloquant.

## Verification

**Manual checks:**
- Inspecter les signatures pour confirmer que `Operation<S>`/transaction acceptent des états définis par le modèle, que le codec impose des limites avant allocation, et qu'aucun chemin d'erreur ne panique.
- Vérifier que les tests d'intégration couvrent une mutation concrète de `IndexedTree` et comparent aussi le prochain ID après rollback.

Validation finale du lot : 91 tests du workspace, builds MSVC/WASM/musl, Clippy hôte/WASM sans avertissement, rustfmt, SPDX et garde du lock réussis. Les tests musl sont compilés localement ; leur exécution Linux relève de la CI distante du commit publié. Preuves : [validation](next-batch-validation.json), [contrôles complémentaires et banc](next-batch-extra-checks.json), [oracle indépendant](compression-oracle.json). Aucun constat de revue différé.
