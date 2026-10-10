<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-072 — Historique annuler / rétablir

`atelier-model` fournit `History<O, Selection>` pour conserver en mémoire des
transactions `Transaction<O>` inversibles et naviguer entre leurs états. Le
contrôleur n'impose aucun type d'état ni de sélection : le type de sélection
est clonable, tandis que `Operation` et l'état n'ont pas besoin d'implémenter
`Clone`. L'historique ne participe pas au codec durable `ATOP`.

## Application, undo et redo

Construire l'historique avec le snapshot initial et un budget logique en octets.
Pour chaque édition, `apply` reçoit la transaction, le snapshot après édition,
l'horloge monotone en millisecondes, son type de regroupement et une borne de
coût. Il applique la transaction via `Transaction::apply`, puis ne publie
l'entrée, le snapshot, les évictions et l'invalidation de redo qu'après succès.
Une transaction vide est un no-op, même si redo est disponible.

Une entrée conserve une seule direction de transaction : après une édition,
c'est l'inverse; après undo, l'inverse rendu par `Transaction::apply` devient
la direction redo, et inversement. Les entrées sont empruntées pendant leur
application puis déplacées seulement après succès. Une erreur garde donc
l'entrée et le snapshot sur place. L'erreur garde le contexte `Apply`, `Undo`
ou `Redo` et la `TransactionError` d'origine, incluant l'index et l'erreur
d'opération.
Le checkpoint de `TransactionalState::restore` doit respecter son contrat de
restauration exacte et reste infallible.

```rust
use atelier_model::{EditKind, History, IndexedTree, IndexedTreeOperation, Transaction};

let mut tree = IndexedTree::new();
let mut history = History::new(None::<(atelier_model::NodeId, usize)>, 64 * 1024);
let edit = Transaction::new(vec![IndexedTreeOperation::Insert {
    position: 0,
    value: 42_i32,
}])?;
history.apply(&mut tree, edit, None, 10, EditKind::Other, 512)?;
history.undo(&mut tree)?;
history.redo(&mut tree)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Sélection et regroupement

`selection()` renvoie le snapshot courant. Chaque édition mémorise celui d'avant
et celui d'après; undo et redo le restaurent après le succès de la transaction.
`move_cursor` met à jour le snapshot sans créer d'entrée et ferme le groupe de
frappe actif.
Une annulation réussie ferme également le groupe de frappe de l’entrée devenue
accessible, afin qu’une nouvelle édition reste séparée après cette navigation.

Le caller signale `EditKind::Typing { segment, closes_after }`. Deux frappes
adjacentes partagent un groupe si leur segment correspond, si l'écart fourni
est inférieur ou égal à 1 000 ms et si la frappe précédente n'a pas fermé le
groupe. Un écart strictement supérieur, un changement de segment, un événement
`Other`, ou un déplacement explicite de curseur coupe le groupe. Pour l'espace
Unicode White_Space qui suit un mot, le caller passe `closes_after: true` :
l'espace joint le groupe courant puis le ferme. La détermination du mot et de
White_Space appartient au caller, sans dépendance Unicode dans ce module.

Les opérations d'une frappe suivante sont inversées avant celles du groupe
précédent, ce qui permet à une seule `Transaction::apply` de rendre l'annulation
du groupe atomique. Si le groupe dépasserait `MAX_TRANSACTION_OPERATIONS`, il
est fermé et l'édition commence une entrée séparée.

## Budget logique

La borne `cost_bound` doit couvrir au maximum les directions de transaction
futures (l'inverse peut être d'une taille différente), les deux snapshots, les
allocations variables propres aux opérations/snapshots, la copie du snapshot
courant conservée par le contrôleur, et les structures et capacités des piles.
Le contrôleur ajoute un plancher fixe par entrée pour ses métadonnées et son
slot de pile.
Cette comptabilité est une borne logique caller; elle ne mesure pas exactement
les allocations de l'allocateur Rust et ne correspond pas à la longueur `ATOP`.
Les additions sont vérifiées et un dépassement arithmétique est refusé.

Avant l'application, le contrôleur calcule le coût post-commit et réserve la
place des conteneurs nécessaires. Au commit, il évince les entrées undo les plus
anciennes nécessaires. Si une combinaison de frappes dépasse le budget mais
que l'entrée courante tient seule, le groupe est coupé; l'ancienne entrée peut
alors être évincée selon le budget. Une entrée individuelle trop grande est
refusée avant toute édition. Les coûts des piles redo sont libérés après une
nouvelle édition réussie; undo et redo déplacent des entrées sans changer le
coût retenu.

`retained_cost_bound`, `undo_len` et `redo_len` exposent les bornes logiques et
les tailles actuelles pour le diagnostic. Aucun historique ne survit à la
fermeture du processus.

## Validation — 2026-10-10

US développée, revue et validée. Le [suivi du lot](EDITING-PROGRESS.md) relie les tests, les comparaisons indépendantes, les correctifs de revue et leurs limites.
