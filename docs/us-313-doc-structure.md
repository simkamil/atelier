<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-313 — Ouvrir un .doc : listes, tableaux, sections, en-têtes et pieds, champs, liens, signets

Cette livraison étend l'import Word 97–2003 de [US-312](us-312-doc-import.md) : la structure du
document n'est plus aplatie en paragraphes. `import_doc` renvoie toujours un `ImportedDoc` ; les
API existantes ne changent pas (ajouts seulement : champs de `ImportedDoc`, de `DocLimits`,
variantes de `ImportNotice`). Le code est dans `crates/fmt-legacy/src/doc/` : `tables.rs`,
`lists.rs`, `sections.rs`, `headers.rs`, `fields.rs`, `bookmarks.rs`, `assemble.rs` (reconstruction
des blocs). `build.rs` ne garde que le parcours du texte et les points d'appel. Aucun autre crate
n'est modifié, en particulier pas `text-model` : ce que le modèle riche ne porte pas est rangé
dans des structures d'accompagnement, **avec les mêmes conventions que `ImportedDocx::tables`
(US-303) et `ImportedOdt::{sections, lists, list_items}` (US-310)**.

```rust,ignore
let doc = import_doc(&bytes)?;
for (id, table) in &doc.tables.tables { /* colonnes, largeurs, props */ }
for list in &doc.lists { /* list.list_id = ListRef::list_id */ }
for s in &doc.sections { /* géométrie de page, colonnes, en-têtes et pieds */ }
let h = doc.effective_header_footer(1, HfKind::Header, HfVariant::Default); // héritage
```

## Ce que reçoit l'appelant

| Champ de `ImportedDoc` | Contenu |
|---|---|
| `tables: ImportedDocTables` | `tables`, `rows`, `cells` (propriétés, position de grille, `synthetic`), `paragraph_cell` ; types `TableProps`, `RowProps`, `CellProps`, `Borders`/`Border` (noms OOXML), `Shading`, `TableWidth`… |
| `lists: Vec<ImportedList>` | une entrée par `LFO` ; `list_id` = indice 1-based de l'`LFO` = `ListRef::list_id` ; 9 niveaux (1 si liste simple) |
| `list_items` | par paragraphe : liste, niveau, `from_style` |
| `sections: Vec<ImportedSection>` | une par section du fichier, dans l'ordre des sections du modèle : saut, page (`PageSetup`), colonnes, numérotation de page, « première page différente », emplacements d'en-têtes et pieds (`HfState`) |
| `header_paragraphs` | paragraphes de chaque nœud d'en-tête/pied (le corps ne les contient pas) |
| `fields`, `bookmarks` | ce qu'est devenu chaque champ et chaque signet (`ImportedField`, `ImportedBookmark`) |
| `facing_pages` | `DopBase.fFacingPages` |

`paragraph_ids()` liste les paragraphes du **corps** (cellules comprises, en-têtes et pieds
exclus) dans l'ordre du document, relu dans le modèle. `resolved_paragraph` fonctionne pour tout
identifiant de paragraphe, en-têtes compris.

## Structures lues

| Structure | Lecture |
|---|---|
| `sprmPFInTable` 0x2416, `sprmPItap` 0x6649 | niveau d'imbrication ; `itap` ramené à `DocLimits::max_table_depth` |
| `sprmPFTtp` 0x2417 / `sprmPFInnerTtp` 0x244C | fin de ligne (paragraphe vide, supprimé ; sa propriété de ligne est lue). Un texte dans ce paragraphe devient une dernière cellule |
| marque 0x07 (niveau 1) / `sprmPFInnerTableCell` 0x244B (niveaux ≥ 2, marque 0x0D) | fin de cellule : ce paragraphe est le dernier de la cellule |
| `sprmTDefTable` 0xD608 | bornes `rgdxaCenter`, `TC80` (20 octets : `TCGRF`, largeur, 4 `Brc80`) |
| `sprmTJc` 0x548A / 0x5400, `sprmTDxaLeft` 0x9601, `sprmTDxaGapHalf` 0x9602, `sprmTDyaRowHeight` 0x9407, `sprmTTableHeader` 0x3404, `sprmTFCantSplit` 0x3403 / 0x3466, `sprmTTableWidth` 0xF614 | alignement, retrait, demi-espace, hauteur (> 0 au moins, < 0 exacte), ligne d'en-tête, ligne non fractionnable, largeur |
| `sprmTTableBorders80` 0xD605, `sprmTTableBorders` 0xD613, `sprmTSetBrc80` 0xD620, `sprmTSetBrc` 0xD62F | bordures du tableau (6) et de cellule (plages `itcFirst..itcLim`, côtés haut/gauche/bas/droite) |
| `sprmTDefTableShd80` 0xD609, `sprmTDefTableShd` 0xD612 (+0xD616/0xD617 au-delà de 22 cellules) | trames de cellule (`Shd` 10 octets, `Shd80` 2 octets) |
| `sprmTCellPaddingDefault` 0xD634, `sprmTCellPadding` 0xD632 | marges de cellule |
| autres sprms de tableau | avis `UnsupportedProperty("tableFormatting")` ; 0xD670 écrit par LibreOffice est ignoré sans avis |
| `PlfLst` (`LSTF` 28 octets, `LVL` : `LVLF` 28 octets, `grpprlPapx`, `grpprlChpx`, `xst`), `PlfLfo` (`LFO` 16 octets, `LFOData`, `LFOLVL` 8 octets + `LVL`) | listes ; **les `LVL` suivent le tableau `LSTF`** dans le flux de tables (`lcb` ne couvre que l'en-tête et les `LSTF`) |
| `sprmPIlfo` 0x460B, `sprmPIlvl` 0x260A | rattachement, dans le paragraphe ou hérité du style (chaîne `istdBase`) ; `sprmPAnld` 0xC63E / `sprmPNLvlAnm` 0x262A (listes Word 6) : avis `LegacyNumbering`. Valeurs spéciales de `ilfo` : 0x07FF (« était dans une liste, n'y est plus ») = paragraphe sans liste, sans avis ; 0xF801 (−2047, ancienne numérotation `ANLD`) = paragraphe sans liste, avis `LegacyNumbering`. Ni l'une ni l'autre n'est une référence cassée. **Prouvé seulement sur fixture construite, valeurs lues de mémoire de [MS-DOC]** (aucun fichier LibreOffice ne les contient) |
| `PlcfSed` (pair 6), `Sed`, `SEPX` | sections ; les sprms de section sont lus dans `WordDocument` |
| `PlcfHdd` (pair 11), `ccpHdd` | histoires d'en-têtes ; `Plcffld` d'en-têtes (pair 17) |
| `Plcffld` (pair 16) | marques de champ et type `flt` du début de champ |
| `SttbfBkmk` (21), `PlcfBkf` (22), `PlcfBkl` (23) | signets ; `Dop` (31) : `fFacingPages` ; `PlcfLst` (73) / `PlfLfo` (74) |

Les indices de paires de la FIB sont ceux de `FibRgFcLcb97` ; chacun a été vérifié sur un fichier
LibreOffice (les données lues aux emplacements 6, 11, 16, 17, 21–23, 31, 73 et 74 sont celles
attendues).

## Où sont rangées les propriétés

* **Tableaux** — `Table`/`Row`/`Cell` du modèle avec `col_span`/`row_span` ; tout le reste dans
  `ImportedDocTables` (indexé par `NodeId`, comme le DOCX) : `ImportedTable` (propriétés, largeurs
  de colonnes `grid`, `depth`), `ImportedRow` (hauteur, en-tête, non fractionnable, alignement),
  `ImportedCell` (largeur, bordures, trame, marges, alignement vertical, `noWrap`, `fitText`,
  `hideMark`, sens du texte, `synthetic`). Les largeurs de colonnes sont les différences entre
  bornes de cellules, regroupées à 3 twips près.
* **Listes** — `ParaProps::list = ListRef { list_id, level }`, `level` ≤ 8 (au-delà : 8, la valeur
  du fichier est perdue). Définitions de numérotation dans `ImportedDoc::lists`, comme l'ODT :
  `ListLevelKind::{Bullet(texte), Number { format, text, start }, None}`, où `text` écrit les
  emplacements des numéros `%1`…`%9` (`"%1.%2)"`). Chaque niveau garde alignement, caractère
  suivant, `fLegal`, `fNoRestart`, `ilvlRestartLim` brut, propriétés de paragraphe et de caractère
  du niveau, et deux drapeaux d'`LFO` (`start_overridden`, `overridden`). Les propriétés de
  paragraphe du niveau (retraits) ne sont **pas** recopiées sur les paragraphes : l'appelant les
  applique entre le style et la mise en forme directe. Deux paragraphes de même `LFO` partagent une
  numérotation ; deux `LFO` d'un même `LSTF` en ont une chacun sauf si l'`LFO` ne redémarre pas
  (pas de surcharge `fStartAt`) — la règle de continuité de Word reste à l'appelant
  (`shared_definition` signale le partage).
* **Sections** — une `Section` du modèle par section du fichier (`PlcfSed`). Géométrie de page,
  colonnes, numérotation et saut dans `ImportedSection` ; pas d'attribut opaque ajouté
  (`Section::opaque` reste vide).
* **En-têtes et pieds** — nœuds `HeaderFooter` attachés par `AttachHeaderFooter`, contenu importé
  par le même code que le corps (tableaux, champs, signets compris).
* **Champs** — `Field { instruction }` du modèle : `PAGE`, `NUMPAGES`, `DATE`, `TIME` (instruction
  réduite au mot-clé), tout autre champ non imbriqué avec son code en entier (nettoyé des
  caractères de contrôle). `HYPERLINK` → `Link` sur le résultat ; le code complet reste dans
  `ImportedField::code`.
* **Signets** — `Bookmark` du modèle.

## Tableaux de valeurs et niveau de preuve

« LO » = écrit par LibreOffice et relu par le lecteur Python indépendant de
`tools/generate-doc-reference.py`, puis par le test Rust `reference_structure.rs` ;
« fixture » = seulement fabriqué par `tests/common/mod.rs` / `tests/doc_structure.rs`, résultat
calculé à la main d'après [MS-DOC] (non prouvé par un fichier d'éditeur).

| Table | Valeurs | Preuve |
|---|---|---|
| `nfc` (format de numéro) | 0 décimal, 1 romain majuscule, 2 romain minuscule, 3 lettre majuscule, 4 lettre minuscule, 23 puce | **LO** (`struct-lists.doc` : 0, 1, 2, 3, 4, 23 ; un test de bout en bout pour chacun) |
| | 255 aucun, 5 ordinal, 6 texte cardinal, 7 texte ordinal, 8 hexadécimal, 22 décimal avec zéro, autre → `Other(n)` | fixture seulement |
| `ixchFollow` | 0 tabulation | **LO** ; 1 espace, 2 rien : fixture |
| `LVLF.jc` | 0 gauche | **LO** ; 1 centre, 2 droite : fixture |
| `brcType` | 1 `single`, 3 `double`, 6 `dotted`, 7 `dashed` (avec couleurs `ico` 6, 4, 2 et `cv` RVB) | **LO** (`struct-tables.doc`) |
| | 0/255 `nil`, 2 `thick`, 5 `hairline`, 8–27 (noms OOXML), 64+ → `single` | fixture / [MS-DOC] seulement |
| `TCGRF.vertAlign` | 0 haut, 1 centre, 2 bas | **LO** |
| `TCGRF` fusion verticale | 0x20 `fVertMerge` (continuation), 0x60 début (`fVertRestart` + `fVertMerge`) | **LO** (LibreOffice écrit 0x60 pour le début) |
| `TCGRF` fusion horizontale | `horzMerge` 1 `fFirstMerged`, 2 `fMerged` | fixture seulement : LibreOffice n'écrit pas de `TC` fusionné (une cellule large = une seule borne) |
| `Shd` | `ipat` 0 (`clear`) avec `cvBack` ; `ipat` 1 `solid`, 2–25 (noms OOXML) | `clear` **LO** (jaune `FFFF00`) ; autres motifs : fixture |
| `sprmTJc` | 1 centre | **LO** ; 0 gauche, 2 droite : fixture |
| hauteur de ligne | négatif = exacte, positif = au moins | **LO** (−850, 567) |
| `sprmTTableHeader`, `sprmTFCantSplit` | 1 = vrai | **LO** |
| `sprmTDxaLeft`, `sprmTDxaGapHalf` | retrait, demi-espace | fixture (retrait) |
| `sprmTTableWidth` | `ftsWidth` 2 (50ᵉ de pour cent) | **LO** (5000) ; 3 (twips), 1 (auto) : fixture |
| `sprmSBkc` | 0 continu | **LO** ; absent = nouvelle page (défaut) **LO** ; 1 colonne, 3 page paire, 4 page impaire : fixture |
| `sprmSNfcPgn` | 0 (absent), 2 romain minuscule | **LO** ; 1, 3 : fixture |
| `sprmSDmOrientPage` | 2 paysage (absent = portrait) | **LO** |
| `sprmSXaPage/YaPage/DxaLeft/DxaRight/DyaTop/DyaBottom` | A4, marges | **LO** (valeurs attendues vérifiées) |
| `sprmSDzaGutter`, `sprmSDyaHdrTop`, `sprmSDyaHdrBottom` | reliure, distances d'en-tête et de pied | fixture ; les deux dernières apparaissent dans les fichiers LO mais l'attendu ne les vérifie pas |
| colonnes | `sprmSCcolumns` 0x500B (n − 1), `sprmSDxaColumns` 0x900C, 0x3005, 0x3019 | **LO** (2 colonnes, 1 cm) ; largeurs inégales 0xF203/0xF204 : fixture non écrite — lues sans test |
| numérotation de page | `sprmSFPgnRestart` 0x3011, `sprmSPgnStart97` 0x501C | **LO** (redémarrage à 5) |
| `sprmSFTitlePage` | 0x300A | **LO** |
| histoires de `PlcfHdd` | 6 séparateurs, puis par section : pair, impair (défaut), pied pair, pied impair, première page en-tête, première page pied ; une histoire non vide se termine par une marque en trop | **LO** (impair, première page, section sans première page) ; pair et premier pied : fixture |
| `Dop.fFacingPages` | bit 0 du premier octet | LO l'écrit à 0 (lu) ; valeur 1 : fixture |
| `Fld.flt` | 88 `HYPERLINK`, 31 `DATE`, 32 `TIME`, 33 `PAGE`, 26 `NUMPAGES` | **LO** |
| `Fld` ch | 0x13, 0x14, 0x15 (octet bas 5 bits) | **LO** |
| `BKF`/`BKL` | `ibkl` désigne la fin dans `PlcfBkl`, point = début = fin | **LO** |

Écarts constatés entre la mémoire de la spécification et les fichiers LibreOffice, corrigés : les
sprms 0x5032 et 0x9031 sont écrits par LibreOffice dans toutes les sections mais ne sont **pas** le
nombre de colonnes (c'est 0x500B/0x900C) ; la marque de fin d'une section est le caractère 0x0C
qui termine le paragraphe ; LibreOffice n'exporte pas l'info-bulle des liens (pas de `\o`).

## Reconstruction des tableaux

Les paragraphes de l'histoire sont regroupés par `itap` : une cellule se termine à son paragraphe
de fin de cellule, une ligne à son paragraphe de fin de ligne, un tableau imbriqué commence à un
paragraphe d'`itap` supérieur. Le modèle exige une grille rectangulaire ; règles (chacune compte
un avis `TableRepaired { what, count }`) :

* Colonnes = union des bornes de toutes les lignes (à 3 twips près) ; une cellule couvre les
  colonnes entre ses deux bornes (`col_span`). Au-delà de `max_table_columns`, la grille suit
  l'ordre des cellules (`"too many grid columns"`).
* Groupes `fFirstMerged` + `fMerged` : une seule cellule (une marque de cellule pour le groupe).
* Nombre de marques différent du nombre de cellules définies : cellules vides ajoutées ou
  largeur par défaut (`"cell count differs from the row definition"`) ; ligne sans définition :
  largeur 1440 (`"row without cell definition"`) ; trou entre cellules ou fin de ligne courte :
  cellule `synthetic` (`"gap between cells filled"`, `"short row completed"`).
* Fusion verticale : les continuations disparaissent sous la cellule de départ (`row_span`) ;
  contenu non vide déplacé (`"content of a vertically merged cell moved"`) ; continuation sans
  départ de même étendue : cellule ordinaire (`"vertical merge without a start cell"`).
* Cellule sans paragraphe : un paragraphe vide est ajouté. Contrôle final de la grille ; en cas
  d'échec le tableau est aplati en paragraphes (`TableCellsFlattened { cells }`).
* Tableau sans marque de fin de ligne : fermé à la fin du texte.

## Champs, liens, signets

* Les marques 0x13/0x14/0x15 sont vérifiées contre le `Plcffld` de l'histoire : une marque absente
  du `Plcffld` (quand il existe) est ignorée (`FieldDegraded "field mark missing from Plcffld"`) ;
  sans `Plcffld`, les marques du texte sont prises telles quelles.
* Le code (entre 0x13 et 0x14) n'entre pas dans le texte. Un champ **imbriqué dans le code** d'un
  autre ne crée aucun champ du modèle : son code, rogné et entre accolades, est ajouté au code du
  champ englobant et son résultat est abandonné (`x{ IF { PAGE }2 > 1 }RES y` donne un seul champ,
  celui du `IF`, sur « RES », d'instruction `IF {PAGE} > 1`, texte `xRESy`). Un champ **à
  instruction vide** (code vide ou d'espaces) n'est pas un champ : son résultat reste du texte
  ordinaire (`FieldDegraded "field with empty instruction"`). Un champ **imbriqué dans le résultat** d'un autre : seul le champ intérieur
  est un `Field` (`"field containing a field"`). Un champ qui traverse une fin de paragraphe :
  résultat conservé en texte, ni champ ni lien (`"field spans paragraphs"`). Marques isolées :
  `"unmatched field mark"` ; champ non refermé : `"field not closed"` ; profondeur au-delà de
  `max_field_depth` : texte masqué (`"field nesting too deep"`). Champ sans résultat : conservé
  seulement pour `PAGE`/`NUMPAGES`/`DATE`/`TIME` (zone vide), sinon `"field without result"`.
* `HYPERLINK "url"` → `LinkTarget::External` ; `\l "nom"` seul → `LinkTarget::Bookmark` ; fichier
  et `\l` → `External("fichier#nom")` ; `\o "texte"` → info-bulle. Sans cible : `"hyperlink not
  kept"`.
* Les positions de signets (CP) deviennent des décalages d'octets du paragraphe, calés sur les
  frontières de graphèmes. Le modèle limite un signet à un paragraphe : **une plage qui traverse
  une fin de paragraphe est coupée à la fin du paragraphe de départ** (`ImportedBookmark::truncated`,
  `BookmarkAdjusted "range cut at the paragraph end"`). Un nom déjà pris (le modèle exige des noms
  uniques) est ignoré (`"duplicate name"`) ; une fin avant le début aussi (`"end before start"`).

## En-têtes et pieds

`PlcfHdd` donne les histoires ; les six premières (séparateurs de notes) sont ignorées. Par
section : les emplacements qui s'appliquent sont *par défaut* (toujours), *première page* (si
`sprmSFTitlePage`) et *pair* (si `fFacingPages`). Histoire de longueur 0 : `HfState::Inherited`
(rien n'est attaché ; `ImportedDoc::effective_header_footer` remonte aux sections précédentes) ;
une seule marque, ou un seul paragraphe vide : `HfState::Empty` (en-tête explicitement vide, arrête
l'héritage) ; sinon `HfState::Present(nœud)`. Les histoires non appliquées sont ignorées sans avis.
Un `PlcfHdd` absent ou inutilisable laisse l'avis `IgnoredText { HeadersFooters }` de US-312.

## Avis ajoutés

`TableRepaired { what, count }`, `FieldDegraded { what, count }`, `BookmarkAdjusted { what,
count }`, `LegacyNumbering { paragraphs }` ; `TableCellsFlattened` ne sert plus qu'au repli d'un
tableau inutilisable ; `FieldCodesDropped` n'est plus émis (les codes sont analysés). Disparus :
`UnsupportedProperty("listNumbering")` et le décompte provisoire des sprms de tableau
(`tableFormatting` ne compte plus que les sprms de tableau non lus).

## Limites (`DocLimits`, champs ajoutés)

| Champ | Défaut | Erreur |
|---|---|---|
| `max_table_rows` | 1 000 000 | `Limit("table rows")` |
| `max_table_cells` | 2 000 000 | `Limit("table cells")` |
| `max_table_columns` | 1024 | pas d'erreur : grille par ordre des cellules |
| `max_table_depth` | 32 | pas d'erreur : `itap` ramené à la limite |
| `max_lists` | 65 536 | `Limit("list count")`, `Limit("list instance count")` |
| `max_sections` | 100 000 | `Limit("section count")` |
| `max_fields` | 1 000 000 | `Limit("field count")` |
| `max_field_depth` | 64 | pas d'erreur : champ masqué |
| `max_bookmarks` | 1 000 000 | pas d'erreur : signets ignorés (`BookmarkAdjusted`) |

Tous les décalages sont contrôlés contre la taille des flux (`PlcfSed`, `SEPX`, `PlcfHdd`,
`Plcffld`, listes, signets) ; les récursions (blocs de tableaux) sont bornées par `max_table_depth`.
Une structure secondaire endommagée donne un avis `DamagedData` et un import partiel.

## Preuves

* `tests/doc-reference/struct-*.doc` : cinq fichiers produits par **LibreOffice** (filtre « MS Word
  97 ») à partir de documents ODT à plat décrits dans `tools/generate-doc-reference.py` :
  tableaux (fusions horizontale et verticale, tableau imbriqué, bordures, trame, hauteur, en-tête
  répété), listes (puces, numérotation sur trois niveaux, romain/lettres, redémarrage), sections
  (portrait/paysage, marges, deux colonnes continues, numérotation romaine avec redémarrage),
  en-têtes et pieds (numéro de page, nombre de pages, première page différente, deuxième section),
  champs et liens (externe, interne, date, heure, page, pages), signets (point, plage, sur deux
  paragraphes). `--check` (sans LibreOffice) vérifie hachages, relit les fichiers avec un lecteur
  Python indépendant (CFB, FKP, sprms, listes, sections, histoires, champs, signets) et compare à
  la description. Le test Rust `reference_structure.rs` compare l'import aux mêmes attendus
  (`expected.json`, clé `structure`). `--only-structure` régénère ces seuls fichiers.
* `tests/doc_structure.rs` : 26 tests sur fichiers fabriqués pour ce que LibreOffice n'écrit pas
  (voir les colonnes « fixture » ci-dessus) et pour les dégradations.
* `tests/robustness.rs` : campagne de mutations à graine fixe étendue aux cinq fichiers
  (5 × 2 500) et troncatures progressives de chaque flux.
* `tests/real_files.rs` : contrôle local optionnel (`ATELIER_DOC_SAMPLES`), maintenant avec le
  nombre de tableaux, listes, sections, champs et signets et une vérification de cohérence.

## Limites et restes

* Tabulations, bordures et trames de paragraphe, enchaînements : inchangé (US-312).
* Notes de bas de page, notes de fin, commentaires, zones de texte : texte ignoré avec avis.
* Images et objets (US-314) ; chiffrement (US-315, chemin de détection inchangé) ; suivi des
  modifications ; `Prm0` ; écriture `.doc` ; raccordement à l'éditeur.
* Styles de tableau et styles de liste (`sgc` 3 et 4) restent ignorés ; sprms de tableau de
  fusion/scission interactives (`sprmTMerge`, `sprmTSplit`, `sprmTInsert`, `sprmTDelete`) non lus.
* Continuité de la numérotation entre `LFO` : laissée à l'appelant (voir plus haut).

## Champs à ajouter au modèle riche (à l'équipe `text-model`)

`Table` : largeur, alignement, retrait, marges de cellule par défaut, bordures (6), largeurs de
colonnes. `Row` : hauteur et règle, ligne d'en-tête, ligne non fractionnable. `Cell` : largeur,
bordures, trame, marges, alignement vertical, `noWrap`, sens du texte. `Section` : type de saut,
géométrie de page, colonnes, numérotation de page, première page différente, héritage des
en-têtes/pieds. Listes : définitions de numérotation (niveaux, formats, textes, retraits) et
redémarrages. `Bookmark` : plage sur plusieurs paragraphes. `Link` : info-bulle déjà présente.

Références : [MS-DOC] §2.4 (tableaux), §2.9.147–2.9.150 (listes), §2.9.220–221 (sections),
§2.8.14 (`PlcfHdd`), §2.8.25 (`Plcffld`), §2.9.286 (signets), §2.6.3 (sprms de tableau).
