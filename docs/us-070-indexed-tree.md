<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-070 — Arbre de nœuds indexé

`atelier-model` fournit `IndexedTree<T>`, une séquence générique dont chaque valeur
reçoit un `NodeId` opaque. Une insertion ou suppression change les positions des
éléments concernés sans changer l’identité des éléments survivants. L’API publique
se trouve dans `crates/model/src/lib.rs`.

## API et erreurs

- `new`, `len`, `is_empty`
- `insert(position, value) -> Result<NodeId, IndexedTreeError>` insère avant la
  position ; `len()` désigne la fin.
- `remove(position) -> Result<T, IndexedTreeError>` enlève et rend la valeur.
- `get(position) -> Option<&T>`, `get_by_id(id) -> Option<&T>` et
  `position_of(id) -> Option<usize>` retournent `None` si la cible n’existe pas.
- Une position d’insertion supérieure à `len()`, une position de suppression
  supérieure ou égale à `len()`, l’épuisement de l’espace d’identifiants et
  l’épuisement de l’espace des slots sont des erreurs explicites. Ces erreurs ne
  modifient pas l’arbre.

Les identifiants sont des compteurs monotones `u64`. Ils ne sont jamais recyclés,
même quand le conteneur redevient vide. L’identifiant maximal peut être délivré
une fois ; l’insertion suivante renvoie `IdentifierExhausted`.
Un identifiant n’est unique qu’au sein de l’arbre qui l’a émis. Deux arbres
indépendants peuvent produire des `NodeId` de même valeur. `get_by_id` et
`position_of` attendent donc un identifiant issu de l’instance appelée ; un ID
d’un autre arbre peut coïncider avec un élément local et ne peut pas être détecté
comme étranger.

## Structure et complexités

Les nœuds occupent une arène radix à fan-out fixe de 256. Chaque page est une
allocation de taille fixe : les pages internes contiennent 256 liens, les pages
feuilles 256 pointeurs vers des nœuds alloués séparément. Un slot est atteint par
au plus `ceil(usize::BITS / 8)` pages (8 sur une cible 64 bits). Cette borne ne
dépend pas du nombre de nœuds, et aucun agrandissement ne déplace les nœuds ou
les pages existantes. L’espace de pages est O(pic de slots attribués) ; chaque
nœud vivant occupe un slot et les slots libérés sont réutilisés.

Les nœuds forment un AVL ordonné par parcours infixe et conservent la hauteur,
la taille du sous-arbre et le parent. Les tailles permettent de descendre vers un
rang en O(log n) ; les liens parent permettent de calculer le rang d’un nœud en
O(log n). Une `BTreeMap<NodeId, slot>` fournit le slot en O(log n). Les insertions,
suppressions, accès par position, accès par identifiant et recherche de position
sont O(log n) au pire cas : le parcours AVL fait O(log n) lectures de slot, chacune
en O(1) borné par la largeur de `usize`. La liste des slots libres est chaînée
dans les slots eux-mêmes ; reprendre ou libérer un slot coûte O(1), même après
une forte baisse de la taille vivante. Les rotations AVL rétablissent une
différence de hauteur d’au plus un ; elles ne parcourent pas l’arbre.

Une suppression avec deux enfants échange uniquement les identités et valeurs
avec le successeur infixe, met à jour l’index d’identifiants, puis enlève le
successeur ; les valeurs et identités survivantes restent associées. Le compteur
de slots est borné par `usize` ; une insertion qui ne peut plus en attribuer
renvoie `CapacityExhausted` sans modifier l’arbre.

## Tests

Les tests unitaires couvrent les insertions en tête/milieu/fin, les positions
invalides sans mutation, la suppression et les identités survivantes, la
réutilisation de slots après suppression totale, les frontières de pages radix,
une longue séquence mixte
comparée à un `Vec` oracle indépendant, les insertions monotones et l’épuisement
interne des identifiants. Après chaque mutation de la séquence oracle, les tests
vérifient les liens parent, tailles, hauteurs, facteur AVL, index d’identifiants
et résultats de recherche.

## Banc de performance

Lancer depuis la racine du workspace :

```text
cargo run --release -p atelier-model --example bench_indexed_tree --locked --offline
```

Le banc construit 100 000 éléments avant chaque chronométrage, calcule hors
mesure 10 000 positions pseudo-aléatoires déterministes valides, effectue une
chauffe, puis cinq mesures de série d’insertions. Il affiche la médiane, le nombre
d’opérations, la taille de la fixture, la chauffe, la cible de 50 ms, le résultat
et l’OS/architecture rapportés par Rust. Les valeurs insérées, les identifiants et
le checksum sont passés par `black_box`. Le processus échoue si la médiane atteint
ou dépasse 50 ms.

La cible dépend du matériel, de la charge système, du compilateur et de son profil
release ; elle ne constitue pas une garantie universelle. Compléter le relevé
ci-dessous avec le matériel précis de la machine d’exécution avant de comparer
des résultats entre hôtes.

### Relevé local

- Machine / processeur : ASUS System Product Name, Intel Core i9-13900KF
  (24 cœurs / 32 processeurs logiques), 64 Gio de mémoire.
- OS / architecture : Windows 11 Professionnel, build 26300, x86-64.
- Rust : `rustc 1.99.0 (b940084d7 2026-09-28)`, profil release.
- Protocole : commande ci-dessus ; fixture hors mesure ; une chauffe et cinq
  échantillons ; médiane.
- Résultat après revue et stockage radix : chauffe 16.4412ms ; médiane 16.6464ms ; cible <50 ms réussie sur
  cet hôte. La mesure n’est pas une garantie sur d’autres machines ou charges.
