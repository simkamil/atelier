<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-071 — Opérations et transactions atomiques

`atelier-model` expose `Operation<S>`, `TransactionalState` et `Transaction<O>`.
Une opération connaît son état, sait se préparer une inverse à partir de l'état
qui la précède, puis sait s'appliquer. Chaque type d'état choisit son propre
checkpoint et sa stratégie de restauration ; le contrat générique n'impose donc
ni `Clone` à l'état ni à ses valeurs.

## Application atomique

`Transaction::apply` enregistre un checkpoint, puis traite les opérations dans
l'ordre. Pour chaque opération, elle prépare d'abord l'inverse, puis applique
l'opération. Toute erreur de préparation ou d'application restaure le checkpoint
et retourne l'index ainsi que l'erreur d'origine. Les inverses sont rendues dans
l'ordre inverse afin qu'une seconde transaction les applique correctement.
Cette restauration ne dépend pas d'inverses eux-mêmes fallibles.

L'adaptateur `IndexedTreeOperation<T>` fournit `Insert`, `Remove` et la primitive
de restauration d'identité nécessaire à l'inverse d'une suppression. Il vérifie
les positions et les identités avant mutation. L'inverse d'une suppression
réinsère la même valeur au même rang avec le même `NodeId`, même lorsque le nœud
retiré avait deux enfants. Une insertion annulée retire l'ID qu'elle vient de
publier. Le compteur d'identifiants est monotone après une transaction réussie
et son inverse. Lorsqu'une transaction échoue, le checkpoint restaure aussi ce
compteur : un ID provisoire qui n'a pas été publié peut être réutilisé.

Le checkpoint de `IndexedTree<T>` est volontairement local à l'adaptateur et
requiert `T: Clone`. Il clone l'arène, les index, les liens, le compteur et les
valeurs. Son coût est proportionnel à l'arène allouée et aux valeurs copiées ;
il est distinct du coût des mutations AVL, qui reste O(log n). Les autres types
d'état peuvent fournir un checkpoint constant ou une stratégie différente sans
modifier `Operation<S>`.

## Codec binaire

Le codec est déterministe et utilise le cadrage `ATOP`, version little-endian
`u16`, nombre d'opérations `u32`, puis des enregistrements préfixés par leur
longueur `u32`. La version prise en charge est `1`. Les limites sont :

- 1 Mio pour une transaction complète ;
- 4 096 opérations ;
- 64 Kio par charge utile d'opération.

Le décodeur vérifie les limites, la version, les longueurs, le format des charges
utiles et les octets finaux avant de rendre la transaction. Il ne dépend
d'aucune entrée d'état et n'applique rien pendant le décodage. Les préconditions
sur l'état, comme l'existence d'un ID, sont vérifiées par `invert` et `apply`. Une
valeur invalide, une version inconnue, une troncature ou un dépassement de limite
produit un `CodecError`. Les opérations fournissent leur encodage de valeur via
`BinaryValue`; les codecs spécifiques peuvent implémenter `Operation<S>` avec
leurs propres formats déterministes.

Exemple :

```rust
use atelier_model::{IndexedTree, IndexedTreeOperation, Transaction};

let mut tree = IndexedTree::new();
let command = Transaction::new(vec![IndexedTreeOperation::Insert {
    position: 0,
    value: 42_i32,
}])?;
let undo = command.apply(&mut tree)?;
assert_eq!(tree.get(0), Some(&42));
undo.apply(&mut tree)?;
assert!(tree.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Cette story fournit des opérations inversibles et un codec borné. Elle ne
fournit ni journal durable, ni récupération après crash, ni pile undo/redo, ni
coédition ; ces contrats appartiennent aux stories suivantes.
