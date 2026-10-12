<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-303 — Lecture docx : tableaux

`import_docx` lit désormais les `w:tbl` du corps du document et des cellules
(tableaux imbriqués) et les range dans le modèle riche (`Node::Table`, `Row`,
`Cell`). Avant US-303, les paragraphes d’un tableau étaient aplatis dans la
section. Le code est dans `crates/fmt-ooxml/src/docx/tables.rs` ; `docx.rs` ne
contient que les points d’appel (voir « Changements dans `docx.rs` »).

## Ce que le modèle porte, et où va le reste

Le modèle riche (`crates/text-model/`, non modifié) porte pour un tableau :
nombre de colonnes, lignes, cellules avec `col_span`/`row_span`, identifiant de
style de tableau, et un emplacement `opaque: Vec<OpaqueAttr>` sur `Table`,
`Row` et `Cell`. Il ne porte ni largeurs, ni bordures, ni trames, ni marges,
ni hauteurs de ligne, ni alignement vertical, ni options d’apparence.

Règle de rangement, appliquée à chaque propriété :

1. **Emplacement opaque du modèle** : les éléments d’origine `w:tblPr` et
   `w:tblGrid` (sur la table), `w:trPr` et `w:tblPrEx` (sur la ligne) et
   `w:tcPr` (sur la cellule) sont resérialisés tels quels dans `opaque`
   (`namespace` = espace WordprocessingML, `name` = nom local, `value` = XML
   UTF-8 avec ses déclarations `xmlns`). Rien n’est perdu pour une réécriture
   future (SPEC §5.2). Une cellule de continuation `vMerge` absorbée dans la
   cellule de départ perd son propre `tcPr`.
2. **Structure d’accompagnement `ImportedDocx::tables`** (`ImportedTables`),
   typée et indexée par identifiant de nœud, pour la mise en page (US-261/262) :
   `tables`, `rows`, `cells` (propriétés, position de grille, `synthetic`),
   `styles` (styles de tableau et conditionnels), `default_style` et
   `paragraph_cell` (paragraphe → cellule).
3. `crates/text-model/` n’est pas modifié ; les champs à y ajouter figurent
   plus bas.

## Éléments et propriétés pris en charge

| Élément | Traitement |
|---|---|
| `w:tblGrid` / `w:gridCol` | largeurs de colonnes (twips) dans `ImportedTable::grid` ; valeurs négatives ou invalides ignorées |
| `w:tr`, `w:tc` | lignes et cellules (aussi à travers `w:sdt`, `w:ins`, `w:customXml`…) ; ligne supprimée par suivi de modifications (`w:trPr/w:del`) abandonnée avec avis |
| contenu de cellule | paragraphes et tableaux imbriqués ; au moins un paragraphe par cellule (un paragraphe vide est ajouté sinon) |
| `w:gridSpan` | `col_span` ; borné à `max_table_columns` ; valeur < 1 traitée comme 1 |
| `w:vMerge` | `restart` + continuations → `row_span` de la cellule de départ ; les continuations sont supprimées du modèle |
| `w:hMerge` | ancienne fusion horizontale : la continuation s’ajoute au `col_span` de la cellule précédente |
| `w:gridBefore`, `w:gridAfter`, `w:wBefore`, `w:wAfter` | cellules vides `synthetic` pour garder la grille rectangulaire ; valeurs conservées dans `RowProps` |
| `w:tblPr` | `tblStyle`, `tblW` (auto, dxa, pct), `jc`, `tblInd`, `tblLayout` (fixe/auto), `tblCellMar`, `tblCellSpacing`, `tblBorders` (haut, gauche, bas, droite, intérieures H et V : style, épaisseur, espace, couleur RVB/auto/thème avec teinte et ombre), `shd`, `tblLook` (masque hexadécimal ou attributs), `tblStyleRowBandSize`, `tblStyleColBandSize`, `bidiVisual`, `tblpPr` (drapeau + avis) |
| `w:tblPrEx` | mêmes propriétés, rangées dans `RowProps::table_exceptions` |
| `w:trPr` | `trHeight` (auto, au moins, exacte), `tblHeader`, `cantSplit`, `jc`, `gridBefore/After`, `wBefore/After`, `tblCellSpacing`, `hidden` |
| `w:tcPr` | `tcW`, `gridSpan`, `vMerge`, `hMerge`, `tcBorders` (dont diagonales `tl2br`/`tr2bl`, avec avis), `shd`, `noWrap`, `tcMar`, `vAlign`, `textDirection` (avis si ≠ `lrTb`), `tcFitText` (avis si actif), `hideMark` |
| styles `w:type="table"` | `tblPr`, `trPr`, `tcPr`, héritage `basedOn`, `w:tblStylePr` pour `wholeTable`, `band1/2Horz`, `band1/2Vert`, `firstRow`, `lastRow`, `firstCol`, `lastCol`, `nwCell`, `neCell`, `swCell`, `seCell` (avec `pPr`, `rPr`, `tblPr`, `trPr`, `tcPr`) |

Les `pPr`/`rPr` du style de tableau lui-même (hors conditionnels) restent gérés
par le moteur de styles US-201. Le style n’est posé sur `Table::style` que
s’il est utilisable (chaîne `basedOn` complète, type tableau, références de
thème résolubles) ; sinon un avis `tblStyle` est émis et l’identifiant reste
visible dans `ImportedTable::props.style`.

## Normalisation de la grille

Le modèle exige une grille exactement rectangulaire. Chaque règle émet un avis
`UnsupportedProperty` dédupliqué, jamais une erreur :

* Largeur d’une ligne = `gridBefore` + somme des `gridSpan` (+ `gridAfter`).
  Nombre de colonnes = largeur maximale des lignes ; si `tblGrid` en déclare
  plus, les colonnes en trop sont retirées ; s’il en déclare moins, la grille
  est étendue (largeur inconnue = `None`). Plafonné à `max_table_columns`.
* Avant la première cellule (`gridBefore`) et après la dernière (`gridAfter`,
  ligne courte) : une cellule vide `synthetic`, de `col_span` adapté.
* `vMerge` sans cellule de départ, ou dont l’étendue horizontale diffère de
  celle du départ : cellule ordinaire (avis `vMerge`). Contenu non vide d’une
  continuation : déplacé dans la cellule de départ (avis).
* Table sans ligne : abandonnée (avis `tbl`). Table seule dans le corps : le
  paragraphe vide final de la section est conservé.
* Une table imbriquée au-delà de `max_table_depth` est aplatie en paragraphes
  de la cellule englobante (avis `tbl`) ; le texte n’est pas perdu.

## Résolution des propriétés effectives

`ImportedTables::resolve_cell(cell, &theme)` renvoie un `ResolvedCell` ;
`resolve_table(table)` fait de même au niveau du tableau. Ordre d’application
(le suivant l’emporte) :

1. propriétés `tblPr`/`trPr`/`tcPr` du style de tableau (zone `wholeTable`
   comprise), de la base vers le dérivé le long de `basedOn` ;
2. zones conditionnelles applicables, dans l’ordre `ConditionalKind::APPLY_ORDER` :
   `wholeTable`, `band1Vert`/`band2Vert`, `band1Horz`/`band2Horz`, `firstRow`,
   `lastRow`, `firstCol`, `lastCol`, `nwCell`, `neCell`, `swCell`, `seCell`.
   Cet ordre suit la lecture d’ECMA-376 §17.7.6 mais n’est **pas vérifié contre
   Word**. Il n’a d’effet que là où une ligne spéciale croise une colonne
   spéciale sans coin défini. Endroit unique à modifier pour l’inverser :
   `ConditionalKind::APPLY_ORDER` dans `crates/fmt-ooxml/src/docx/tables.rs`.
   Pour chaque zone, la chaîne `basedOn` est fusionnée (le dérivé l’emporte) ;
3. propriétés directes du tableau (`tblPr`), puis exceptions de ligne
   (`tblPrEx`), puis propriétés directes de la cellule (`tcPr`).

Pour les bordures, chaque couche est projetée sur les bords de la cellule
(haut/bas : bords externes si la cellule touche le bord de la table, sinon
`insideH` ; gauche/droite avec `insideV`) ; dans une couche, `tcBorders`
l’emporte sur `tblBorders`. La résolution des conflits entre cellules voisines
relève d’US-261.

Applicabilité : `tblLook` (`firstRow`, `lastRow`, `firstColumn`, `lastColumn`,
`noHBand`, `noVBand` ; les attributs priment sur le masque `val` ; sans
`tblLook`, tout est faux : bandes actives, ni première ligne ni première
colonne). Les bandes sont comptées après la première ligne/colonne si elle est
activée, par blocs de `tblStyleRowBandSize`/`tblStyleColBandSize` (1 par
défaut), et ne s’appliquent pas aux lignes/colonnes d’en-tête ou de total.
Une cellule fusionnée utilise l’étendue qu’elle couvre (première ligne si elle
commence en ligne 0, dernière si elle finit à la dernière ligne). Les coins
exigent la ligne et la colonne correspondantes activées.

Paragraphes et caractères : `ImportedDocx::resolved_paragraph` (signature
inchangée) tient compte des conditionnels pour les paragraphes situés dans une
cellule. Cascade : défauts → style de tableau → **conditionnels** → style de
paragraphe → style de caractère → formatage direct. Comme le moteur US-201 ne
peut pas intercaler une couche, les conditionnels ne remplissent que les
propriétés que ni le style de paragraphe/caractère ni le formatage direct ne
définissent (comparaison champ par champ). Limite : les bascules gras/italique
suivent cette règle de présence, pas la sémantique de bascule des styles de
caractère.

Couleurs : `TableColor::resolve(&Theme)` applique `themeTint` (mélange avec le
blanc) et `themeShade` (mélange avec le noir) ; `Shading::effective_fill`
donne la couleur de remplissage (`clear` → `fill`, `solid` → `color`, `pctNN`
→ mélange, autres motifs → 50 %).

## Avis émis

Tous sont des `ImportNotice::UnsupportedProperty` (espace WordprocessingML,
dédupliqués par nom et message, au plus 512) : `tblpPr` (tableau flottant),
`tcBorders` (diagonales), `textDirection`, `tcFitText`, `tblGrid` (grille
incohérente), `tr` (ligne courte), `gridSpan`, `hMerge`, `vMerge`, `tbl`
(tableau vide ou trop imbriqué), `tblStyle` (style absent ou inutilisable),
`trPr` (ligne supprimée), `tcW`/`gridCol` (largeur négative), `tblStylePr`
(type inconnu), ainsi que les largeurs invalides. Les éléments de tableau lus
ne produisent plus d’avis `UnsupportedElement` ; les éléments de tableau non lus
(par exemple `w:tblCaption`, `w:cnfStyle`) en produisent toujours.

## Limites (`DocxLimits`, champs ajoutés avec valeurs par défaut)

`max_table_depth` 32 (niveau 1 = tableau du corps), `max_table_rows` 1 000 000,
`max_table_columns` 1 024 (grilles et `gridSpan` bornés, sans erreur),
`max_table_cells` 2 000 000. Dépassement des lignes ou des cellules :
`DocxError::Limit("table rows")` / `Limit("table cells")`. Les nombres de
paragraphes, de runs et d’octets de texte restent bornés par les limites
existantes. Les valeurs numériques invalides ou négatives des propriétés de
tableau sont ignorées avec avis, sans échec de l’import.

## Champs à ajouter au modèle riche (à l’équipe `text-model`)

`Table` : largeur, alignement, retrait, disposition fixe/auto, marges de
cellule par défaut, espacement entre cellules, bordures (6), trame, options
`tblLook`, largeurs de colonnes. `Row` : hauteur et règle, ligne d’en-tête,
ligne non fractionnable, exceptions. `Cell` : largeur, bordures (4 + 2
diagonales), trame, marges, alignement vertical, `noWrap`, direction du texte,
ajuster le texte. Styles : conditionnels de style de tableau (aujourd’hui
seulement dans `ImportedTables::styles`).

## Preuves

* `crates/fmt-ooxml/tests/docx_tables.rs` : 29 tests sur des documents OPC
  montés en mémoire — grille et largeurs, `gridSpan`, `vMerge` (nominal,
  orphelin, contenu), `hMerge`, lignes décalées, lignes inégales, grille
  incohérente, imbrication et limite de profondeur, limites typées, valeurs
  absurdes, toutes les propriétés de tableau, de ligne et de cellule,
  `tblPrEx`, conservation opaque, résolution avec zones qui se recouvrent
  (coin, première ligne, première colonne, bandes, dernière ligne), `tblLook`,
  taille de bande, priorité du formatage direct et des styles de paragraphe,
  cycles de styles, teintes de thème, et campagne de mutations/troncatures à
  graine fixe (1 200 documents par exécution ; 24 000 avec une autre graine à la
  livraison) sans panique et avec modèle valide.
* Fidélité : **Microsoft Word n’est pas installé** sur la machine de
  livraison ; elle n’est vérifiée que sur des documents construits à la main.
  Le corpus `tests/docx-reference/` n’a donc pas été étendu (aucun attendu
  produit par Word). L’ordre de priorité des zones conditionnelles
  (lignes avant colonnes) suit la lecture de la norme et reste à confirmer
  contre Word.

## Changements dans `docx.rs`

Déclaration du sous-module et réexportations ; quatre champs de `DocxLimits` ;
champ `tables` de `ImportedDocx` ; `resolved_paragraph` (conditionnels) ;
lecture du corps par blocs (paragraphes et tableaux) via `ParagraphSink`, qui
reprend sans changement la comptabilité de runs, de texte et de métadonnées de
paragraphe ; lecture des styles de tableau ; exclusion des éléments de tableau
lus des avis `UnsupportedElement`.
