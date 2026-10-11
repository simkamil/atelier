<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-200 — Modèle de document du traitement de texte

`atelier_text_model::rich` est le modèle riche du traitement de texte. Il vit dans
des modules séparés de `PlainTextDocument` (US-120), dont l'API et le comportement
sont inchangés : `lib.rs` ne reçoit que `pub mod rich;`. Le modèle n'est raccordé ni
à l'éditeur (`core-wasm`) ni au WASM. Il s'appuie sur `atelier-model` (arbre indexé,
`Operation`, `Transaction`, `History`, `PositionMap`) et sur `atelier-unicode`
(frontières de graphèmes), sans aucune autre dépendance.

## Structure des nœuds

```
RichDocument
└─ Section*                      (au moins une)
   ├─ corps : Bloc*
   ├─ en-têtes / pieds : HeaderFooter (par variante Default | First | Even)
   └─ propriétés opaques
Bloc = Paragraph | Table | OpaqueBlock
Table → Row* → Cell* → Bloc* (au moins un paragraphe par cellule)
```

- **Identifiants stables** : tout nœud et tout élément du texte (objet ancré, champ,
  signet, lien) porte un `NodeId` d'`atelier-model`, tiré d'un arbre indexé servant
  de distributeur ; un identifiant n'est jamais réattribué. Les ids survivent à
  annuler/rétablir (les branches supprimées sont restaurées avec leurs ids).
- **Stockage** : arène `BTreeMap<NodeId, Node>` ; chaque séquence de blocs
  (sections du document, corps de section, en-tête/pied, contenu de cellule) est un
  `IndexedTree<NodeId>` d'`atelier-model` (insertion, suppression et accès
  positionnel en O(log n)). Lignes et cellules d'un tableau sont de simples `Vec`.
- **Paragraphe** : texte UTF-8, plages d'attributs de caractère (`CharRun`),
  propriétés de paragraphe, objets ancrés, champs (plage du résultat), signets
  (un enregistrement début + fin, donc apparié par construction), liens. Tous les
  décalages sont des octets UTF-8 posés sur des frontières de graphème.
- **Tableau** : grille de `columns` colonnes ; une cellule porte `col_span` et
  `row_span` (fusion à la HTML : les lignes recouvertes n'ont pas de cellule à cet
  endroit). Une ligne liste ses cellules dans l'ordre des colonnes.
- **Bloc opaque** : `kind` + octets, conservé tel quel (SPEC §5.2). Les attributs de
  caractère, de paragraphe, de section, de ligne et de cellule ont aussi un
  emplacement `opaque: Vec<OpaqueAttr>` trié par clé.
- **Attributs** (`CharAttrs`, `ParaProps`) : gras, italique, soulignement, barré,
  exposant/indice, police, taille, couleur, surlignage ; alignement, retraits
  (gauche, droite, première ligne), espacements avant/après, interligne, référence de
  style, référence de liste + niveau (0 à 8). Chaque propriété est `Option` :
  `None` = non définie = héritée. La cascade (US-201) n'est pas faite.

## Opérations (`RichOp`)

Toutes implémentent `atelier_model::Operation<RichDocument>` et s'exécutent dans une
`Transaction` (atomique : un échec restaure le document) puis dans `History`.

| Famille | Opérations | Inverse |
| --- | --- | --- |
| Texte | `InsertText`, `DeleteText` | `DeleteText` ; restauration du paragraphe |
| Attributs | `SetCharAttrs` (retire `clear` puis pose `set` sur une plage, normalise les plages), `SetParaProps` | restauration du paragraphe |
| Éléments du texte | `InsertInline`, `ModifyInline`, `RemoveInline` (objet ancré, champ, signet, lien) | `RemoveInline`, `ModifyInline` (ancienne valeur), `InsertInline` |
| Paragraphes | `SplitParagraph`, `MergeParagraphs` | `CollapsePair`, `ExpandPair` |
| Blocs | `InsertBlock`, `RemoveBlock` (paragraphe, tableau, bloc opaque ; section si le parent est le document) | `RemoveBlock`, `InsertBlock` (branche complète) |
| En-têtes/pieds | `AttachHeaderFooter`, `DetachHeaderFooter` | l'une pour l'autre |
| Tableaux | `InsertRow`, `DeleteRow`, `InsertColumn`, `DeleteColumn`, `MergeCells`, `SplitCell` | `RestoreSubtree` (instantané du tableau) |
| Restauration | `RestoreSubtree`, `CollapsePair`, `ExpandPair` | entre elles |

Sémantique retenue :

- Le texte inséré hérite des attributs du caractère précédent (rien au début).
- La suppression fait glisser les repères situés dans la plage vers son début, retire
  les objets ancrés de la plage et les liens/champs rendus vides.
- `SplitParagraph` copie les propriétés de paragraphe vers la queue ; il est refusé
  à l'intérieur d'un champ ; liens et signets à cheval sont tronqués à la première
  partie. `MergeParagraphs` garde l'id et les propriétés du premier paragraphe et
  conserve les plages de caractères des deux.
- `MergeCells` doit couvrir des cellules entières ; le contenu non vide des cellules
  absorbées est ajouté à la cellule de tête. `SplitCell` rend des cellules 1x1.
  `DeleteColumn` sur une cellule fusionnée réduit sa portée ; `DeleteRow` fait
  descendre une cellule qui commence sur la ligne supprimée.
- Les constructeurs `new_paragraph`, `new_table`, `new_section`, `new_header_footer`,
  `new_opaque`, `insert_row_op`, `insert_column_op`, `split_cell_op` fournissent les
  branches et les identifiants frais ; `alloc_id` donne un id pour un champ, signet…
- `RichOp::position_map` fournit le `PositionMap` d'`atelier-model` pour les
  opérations qui déplacent des positions (insertion, suppression, scission, fusion).

## Invariants (`RichDocument::validate`, `Paragraph::validate_local`)

- Au moins une section ; chaque cellule contient au moins un paragraphe.
- Aucun nœud orphelin ni partagé ; types de nœuds admis par chaque conteneur ;
  identifiants uniques (nœuds et éléments du texte dans un même espace) ; variantes
  d'en-tête/pied uniques.
- Plages d'attributs triées, disjointes, non vides, bornées par le texte, non
  vides d'attributs et normalisées (deux plages jointives égales sont fusionnées).
- Tous les décalages (plages, objets, champs, signets, liens) sont sur des frontières
  de graphème (`atelier-unicode`) ; signets : nom non vide et unique dans le document,
  `début <= fin` ; champs et liens triés et disjoints, liens non vides.
- Grille de tableau exactement rectangulaire : aucune case vide, aucun recouvrement,
  aucune cellule hors grille, fusions comprises ; au moins une ligne et une colonne.
- Index dérivés (élément du texte vers paragraphe, noms de signets) cohérents.

Toute opération invalide renvoie une `RichError` typée sans modifier le document : les
préconditions sont vérifiées avant la première mutation et les paragraphes/tableaux
sont édités sur une copie validée avant installation. Aucun chemin ne panique sur une
entrée d'opération.

## Preuves

`cargo test -p atelier-text-model --locked --offline` : 47 tests (12 de
`plain_text.rs`, inchangés ; 33 ciblés dans `tests/rich_ops.rs` ; 2 de propriété dans
`tests/rich_property.rs`).

- **Ciblés** : début/fin de paragraphe, plage vide, graphèmes multi-octets (é, emoji
  famille ZWJ, e + accent combinant, rejet d'un décalage à l'intérieur), insertion qui
  collerait deux graphèmes, fusion de paragraphes aux attributs différents, scission
  dans un champ, suppression d'une colonne traversant une cellule fusionnée, lignes
  traversant une fusion verticale, fusion coupant une cellule, dernier paragraphe de
  cellule, dernière section, dernière ligne/colonne, signets en double, `History`
  annuler/rétablir et transformation de sélection.
- **Propriété** : générateur LCG maison à graine fixe (64 graines, 100 pas chacune).
  À la dernière exécution : 5 597 transactions de 1 à 4 opérations, 10 727 opérations
  acceptées, 5 295 refusées. Après chaque opération acceptée, `validate()` ; après
  chaque refus, égalité structurelle avec l'état précédent ; pour chaque transaction,
  annuler restitue exactement le document (égalité structurelle complète, hors
  distributeur d'ids) puis rejouer redonne l'état d'arrivée ; 1 381 transactions
  terminées par une opération invalide ont été vérifiées sans effet. Toutes les
  variantes d'opération sont exercées avec succès. Certaines opérations valides par
  construction (attributs de caractère, propriétés de paragraphe, insertion de bloc
  bien placé, insertion de ligne/colonne) ne doivent jamais être refusées, ce qui
  détecte une implémentation qui refuserait trop. Contrôle par mutation : désactiver
  la normalisation des plages fait échouer ce test.

## Hors périmètre et limites

- Cascade de styles (US-201), mise en page, lecture/écriture docx, raccordement à
  l'éditeur ou au WASM.
- **Codec binaire du journal de récupération** : `encode`/`decode` renvoient
  `CodecError::InvalidData` (comme `decode` de `PlainTextDocument`). Les opérations
  riches ne sont donc pas encore sérialisables dans le journal.
- Signets limités à un paragraphe (pas de signet à cheval sur plusieurs paragraphes).
- Un lien ou un signet à cheval sur une scission est tronqué ; l'annulation restitue l'original exact.
- `Transaction::apply` copie tout le document comme point de contrôle, et plusieurs
  inverses sont des instantanés de paragraphe ou de tableau : correct mais pas
  optimal pour de gros documents ; aucune mesure de performance n'est faite.
- Besoin constaté dans `atelier-model` (non modifié) : un `NodeId` ne peut être créé
  que par un `IndexedTree`, d'où l'arbre distributeur d'identifiants ; un arbre
  hiérarchique natif ou un constructeur public d'identifiants simplifierait le modèle.
