---
title: 'US-072 — Pile annuler / rétablir'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '3aa4d19bef7e8e917b90136f80252bd988e1f7b0'
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/docs/us-071-transactions.md'
  - '{project-root}/crates/model/src/lib.rs'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Les transactions US-071 peuvent être inversées atomiquement, mais aucun historique ne permet de naviguer entre les états précédents et suivants d’un document. Les frappes rapprochées doivent former des actions utiles sans sacrifier les limites de mémoire ni la restauration du curseur.

**Approach:** Ajouter à `atelier-model` un historique générique qui enregistre des transactions inversibles et des snapshots de sélection opaques, avec des piles undo/redo et un budget paramétrable. L’appelant fournit une horloge monotone déterministe, les indices de regroupement et un contrat conservateur de coût mémoire avant l’application.

## Boundaries & Constraints

**Always:** Réutiliser `Operation<S>`, `Transaction<O>` et leur garantie checkpoint/restore ; ne pas exiger `Clone` des opérations ou de l’état. Le type de snapshot de sélection est générique et clonable pour rendre les positions conservées ; aucune dépendance à US-073. L’historique est local, en mémoire et indépendant du codec durable `ATOP`. Toute erreur d’inversion ou d’application laisse état, historique, sélection et redo inchangés. Le contrat existant rend `TransactionalState::restore` infallible : chaque implémentation doit restaurer exactement son checkpoint ; aucun faux cas d’erreur de rollback ne sera ajouté à cette API. Coût calculé avant mutation : le contrat du mesureur fournit une borne supérieure déclarée pour opérations, inverses, snapshots, structures de pile et surcoûts. Ce compte est une borne logique définie par l’appelant, pas une mesure universelle des allocations Rust ni la longueur `ATOP`.

**Never:** Coupler la pile à `IndexedTree` ou à une sélection concrète, dépendre de US-073, abandonner silencieusement une modification, invalider redo sur une opération vide/échouée, ou changer le format et les octets `ATOP`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Edit and undo/redo | Transaction non vide, snapshots avant/après | Application atomique ; undo restaure l’état et le snapshot avant, redo restaure l’état et celui après | Échec de transaction conserve toutes les piles et le snapshot courant |
| Typing group | Frappes consécutives, temps monotone fourni | Continuer si pause ≤ 1 000 ms ; couper si pause strictement > 1 000 ms, déplacement explicite du curseur, ou espace Unicode après un mot. La séquence d’espaces ferme le groupe après l’espace | Les autres catégories d’édition ferment le groupe avant et après |
| Budget | Budget zéro, ajout qui tient, dépassement ou entrée seule trop grande | Précalcul avant application ; éliminer les entrées undo les plus anciennes nécessaires. Si l’entrée seule dépasse le budget, refuser sans édition ni perte d’historique | Erreur typée de budget ; débordements de calcul refusés |
| Empty/new edit | Transaction vide ou nouvelle édition après undo | Vide : aucun historique ni invalidation redo. Nouvelle édition réussie : vider redo ; transaction échouée : le conserver | Aucun effet pour vide ; erreur atomique pour échec |
| Undo/redo failure | Inversion en échec ou application qui échoue après une mutation intermédiaire | Transaction::apply restaure le checkpoint ; entrée et snapshot restent sur la même pile | Préserver l’erreur source et le contexte undo/redo ; `restore` doit respecter son contrat de restauration exacte sans erreur |

</frozen-after-approval>

## Code Map

- `crates/model/src/lib.rs` -- définitions existantes `Operation`, `Transaction`, `TransactionError`, `TransactionalState`, `NodeId` et exports publics.
- `crates/model/src/history.rs` -- nouveau contrôleur générique undo/redo, groupement, comptabilité mémoire et erreurs dédiées.
- `docs/us-071-transactions.md` -- contrat de réversibilité et d’atomicité déjà livré, à utiliser comme frontière d’intégration.

## Tasks & Acceptance

**Execution:**
- [x] `crates/model/src/history.rs` -- définir API générique, types d’événements de groupement, snapshots de sélection, budget vérifié avant mutation, erreurs et transitions atomiques.
- [x] `crates/model/src/lib.rs` -- déclarer le module et réexporter uniquement l’API publique nécessaire, sans modifier `ATOP`.
- [x] `crates/model/src/history.rs` -- tester apply/undo/redo, toutes les frontières temporelles et Unicode, budgets 0/trop petit/éviction, snapshots, invalidation redo et erreurs/rollback.

- [x] `docs/us-072-undo-history.md` -- documenter API, coût du budget, groupes, sélection, erreurs et exemple.

**Acceptance Criteria:**
- Given une série de transactions non vides, when on applique puis annule/rétablit, then l’état et les snapshots avant/après reviennent exactement ; les erreurs n’altèrent pas piles ou sélection.
- Given une transaction d’annulation/rétablissement dont une opération échoue après une mutation partielle, when elle est appliquée, then le checkpoint restaure l’état exact et l’erreur conserve l’opération source ; `restore` reste infallible par contrat.
- Given une frappe et un horodatage caller, when l’écart dépasse strictement une seconde, qu’un déplacement explicite survient, ou qu’un espace Unicode après mot clôt la frappe, then une frontière undo existe ; un écart exactement égal à 1 000 ms continue le groupe.
- Given un budget et un coût supérieur conservateur, when l’édition est proposée, then le plan de réservation est vérifié avant mutation ; les entrées les plus anciennes sont évincées au commit. Une entrée unique trop grande est refusée sans perte silencieuse ni changement d’état.
- Given une édition nouvelle réussie après undo, when elle est enregistrée, then redo est effacé ; une transaction vide ou échouée ne l’efface pas.
- Given des snapshots construits avec `NodeId` et offset, when undo/redo réussit, then le snapshot correct est restauré sans supposer que US-073 est implémentée.

## Implementation Notes

Propriété exclusive : `crates/model/**`, `docs/us-072-undo-history.md` (à créer), ce spec. Le coordinateur gère backlog, Cargo.lock, Git, Graphify et preuves globales. Aucun sous-agent supplémentaire. Ceci est le handoff d'implémentation du workflow BMAD déjà rendu/suivi par le coordinateur; ne pas réinvoquer de skill ni refaire le planning. Lire le rapport Graphify sans relancer son écriture. Cargo `--locked --offline`, formatage atelier-model seulement.

Le contrat mémoire caller doit couvrir le coût maximal des directions futures (undo peut fabriquer des inverses différents); un simple coût courant ne suffit pas à garantir une borne. Stocker une seule direction d'opérations par étape, remplacée par l'inverse retournée après succès, évite Clone<O>. Vérifier que les transactions groupées restent atomiques en tant que groupe (un échec de la deuxième annulation restaure aussi la première). Ne pas perdre la dernière entrée en utilisant pop avant une opération fallible. Inclure les compteurs/capacités du stockage de la pile dans la documentation honnête du budget.

## Spec Change Log


## Review Triage Log

- B06 : low ; plafond exact et ajout suivant déjà testés, renforcer ordre undo/redo, valeurs et snapshots.
- B07 : false-positive ; obligation caller de borner toutes les directions futures déjà documentée.
- R01 : medium ; undo expose un ancien groupe de frappe ouvert, pouvant fusionner une édition après navigation. Fermer ce groupe après succès et tester.

Voir [revue complète](editing-review.md), triée après réception des trois rapports.


## Design Notes

Stocker une seule entrée avec transaction d’annulation et transaction de rétablissement, plus snapshots avant/après. Pour éviter `Clone<O>`, l’API consomme les transactions à l’enregistrement et l’inverse rendue par `Transaction::apply` devient la direction opposée. Le contrat du mesureur doit borner les deux directions et tous les snapshots retenus avant l’application ; ne pas prétendre connaître exactement l’allocation heap d’un type générique. Calculer les évictions sans les appliquer, exécuter la transaction, puis engager l’éviction et l’ajout seulement en cas de succès. Si l’entrée combinée à un groupe dépasse le budget mais tient seule, fermer le groupe précédent et démarrer une entrée séparée ; si elle dépasse seule, refuser. Le caller indique le segment de frappe et son statut de séparation après mot, calculé selon Unicode White_Space, afin de tester les espaces insécables et les séquences multi-espaces.

## Verification

**Commands:**
- `cargo test -p atelier-model --locked --offline` -- expected: tests existants et nouveaux invariants passent.
- `cargo fmt --check -p atelier-model` -- expected: format Rust conforme.

## Validation finale — 2026-10-10

Implémentation et correctifs ciblés validés ; 160 tests du workspace et 76 tests release passent. Les quatorze contrôles globaux et l’oracle indépendant réussissent, avec empreintes de sources inchangées pendant les contrôles. Revue triée sans constat différé. Preuves : [validation](editing-validation.json), [oracle](editing-oracle.json), [revue](editing-review.md) et [rendu SVG](editing-svg-visual.json). La vérification navigateur est indisponible ; le témoin SVG exact a été rasterisé localement et inspecté, avec huit points de contrôle.
