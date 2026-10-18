<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-307 — Enregistrer en docx

L'écrivain est dans `crates/fmt-ooxml/src/docx/write.rs` et `docx/write/` (`body.rs` corps et
paragraphes, `tbl.rs` tableaux, `props.rs` propriétés `w:rPr`/`w:pPr`, `parts.rs` styles, réglages,
polices, numérotation, `xml.rs` sortie XML). `docx.rs` n'a que les points de raccord : réexports,
`import_package` (l'import à partir d'un `Package` déjà lu) et l'acceptation des quatre types de
contenu principaux (docx, dotx, docm, dotm) par l'import. Le modèle n'est jamais modifié.

## API

* `write_docx(&ImportedDocx, &DocxWriteOptions) -> Result<WrittenDocx, WriteError>` : document
  importé, éventuellement modifié par des opérations du modèle.
* `write_new_docx(&NewDocument, &DocxWriteOptions)` : document riche créé de zéro. `NewDocument`
  porte le document, le `StyleEngine`, les noms des styles (le catalogue n'est pas énumérable),
  le style par défaut, la page. `NewDocument::from_default` prend le profil de US-202.
* `DocxWriteOptions` : `variant` (`DocxVariant::{Document, Template, MacroDocument, MacroTemplate}`,
  `None` = celui de la source), `save` (`SaveContext` de US-328 : `update_on_save` est appelé),
  `regenerate_all`, `added_styles` (styles ajoutés au moteur depuis l'import : identifiant, nom,
  suivant), `limits`, `max_part_bytes`.
* `WrittenDocx { bytes, notices, regenerated_parts, reused_parts, opaque_parts, save }`. `WriteError` : `Opc`,
  `Import`, `Limit`, `Invalid`. `WriteNotice` (au plus 512 + un décompte) liste ce qui n'a pas été
  écrit fidèlement : `MacrosRemoved`, `ParagraphRebuilt`, `EnvelopeDropped`,
  `AlternateContentFlattened`, `BlocksReordered`, `HighlightApproximated`,
  `UnsupportedCharacter`, `ThemeNotWritten`, `HeaderFooterNotWritten`, `ListDefinitionMissing`.

## Règle « partie touchée »

L'écrivain réimporte le paquet de l'`ImportedDocx` (coût linéaire) : c'est le **modèle de base**.
* `word/document.xml` est touchée si `imported.document != base.document` (ou `regenerate_all`) ;
* `styles.xml` si les valeurs par défaut ou l'un des styles du squelette diffère de la base, ou si
  `added_styles` n'est pas vide ; * `settings.xml` si `default_tab_stop_twips` diffère.
Sans `regenerate_all`, une partie non touchée est recopiée octet pour octet (c'est le cas de toutes les autres parties,
`fontTable.xml`, le thème, etc. : leur contenu n'est pas régénéré ; un thème modifié donne
`ThemeNotWritten`). Une partie touchée est régénérée à partir du modèle et du squelette de US-306.

## Régénération du corps

Les nœuds sont comparés à la base **par identifiant**.
* Paragraphe identique à la base : réémis depuis le squelette (attributs, enveloppes, propriétés
  dans l'ordre de la source, textes tirés du modèle par leurs plages) : XML identique à la source.
* Paragraphe modifié : le squelette est conservé, les plages de texte sont ramenées à travers
  l'édition (préfixe/suffixe communs, l'insertion est absorbée par le run qui précède) ; les
  familles de propriétés qui diffèrent (`b`, `i`, `strike`, `u`, `vertAlign`, `rFonts`, `sz`,
  `color`, `highlight`, `szCs` ; `pStyle`, `jc`, `ind`, `spacing`, `numPr`) sont reconstruites à
  leur place, ou au rang du schéma (`rpr_rank`, `ppr_rank`) s'il n'y en avait pas ; les autres
  enfants, attributs inconnus et objets ancrés restent. Un run est coupé là où les attributs du
  modèle changent. Texte venu d'un autre paragraphe (fusion) : propriétés neuves, sans `w:rStyle`.
* Si le texte ou les objets du modèle ne sont pas exactement retrouvés (champs, signets ou liens
  ajoutés, objet déplacé…), le paragraphe est écrit depuis le modèle seul en gardant ses attributs et
  `w:pPr` : avis `ParagraphRebuilt` (les enveloppes `w:hyperlink`, `w:ins`… sont alors perdues).
* Paragraphe sans squelette (créé, scindé) : écrit depuis le modèle : runs coupés aux frontières
  d'attributs, `w:tab`/`w:br`/`w:noBreakHyphen`/`w:softHyphen` pour `\t`, `\n`, U+2011, U+00AD,
  champs (`fldChar` begin/instrText/separate/end), signets, liens (`w:hyperlink`, relation externe
  `rIdLink…`), objets ancrés (enveloppés dans `w:r` si ce sont des éléments de run).
* Blocs : l'ordre du modèle décide ; les blocs nouveaux sont placés après le plus proche bloc
  précédent qui a un squelette ; si des blocs ont été déplacés, les enveloppes de la source sont
  abandonnées (`BlocksReordered`).
* Tableaux : si lignes, cellules, colonnes et fusions sont ceux de la base, le squelette est
  réémis (`tblPr`, `trPr`, `tcPr` pris dans les emplacements `opaque`) ; sinon la grille est
  reconstruite depuis le modèle (continuations `vMerge` recalculées), en gardant les emplacements
  `opaque`, `tblGrid` ajusté au nombre de colonnes, `gridSpan`/`vMerge` de `tcPr` rendus cohérents
  et le contenu des cellules du squelette. Une cellule ou un corps se termine toujours par un
  paragraphe (règle de Word) : un paragraphe vide est ajouté après un tableau final ; un corps vide
  reçoit un paragraphe vide.
* Sections : une section n'est écrite par `sectPr` que si le modèle en compte plusieurs (`sectPr`
  synthétique A4 dans le dernier paragraphe des premières) ; le `sectPr` final de la source reste.
  En-têtes/pieds du modèle : non écrits (`HeaderFooterNotWritten`, US-304).

## Inventaire des parties et `regenerate_all`

Avec `regenerate_all`, aucune partie n'est lue dans les octets du fichier source :
* **Générées depuis le modèle** (`regenerated_parts`) : `word/document.xml` (modèle riche + squelette US-306),
  `styles.xml` (catalogue + squelette), `settings.xml` (réglages + squelette), `[Content_Types].xml`
  et tous les `.rels` (table des relations du paquet, triée, jamais relue du texte source).
* **Réémises depuis le sous-arbre opaque conservé** (`opaque_parts`) : toute autre partie du paquet. Le
  magasin opaque du modèle est `ImportedDocx::package` (US-306 : « parties non interprétées, octet pour
  octet »). Choix : c'est « depuis le modèle » au sens de la spec pour ce que le modèle ne représente pas.
* **Non régénérables aujourd'hui** (donc opaques) car le modèle les représente avec perte : `fontTable.xml`
  (`FontRecord` sans panose, charset ni signature), `theme` (`Theme` : couleurs et polices seulement),
  `numbering.xml` (non lu : US-302), `webSettings.xml`, en-têtes/pieds, notes, commentaires (US-304,
  non lus), `docProps/core.xml` et `app.xml` (le modèle porte leurs champs ; les enfants inconnus, ex.
  `HeadingPairs`/`TitlesOfParts`, ne vivent que dans la partie : `update_on_save` met les valeurs à jour
  dedans, voir US-328), `custom.xml`, glossaire, `customXml`, VBA, médias, polices intégrées.

| Partie | Source de la sortie avec `regenerate_all` |
|---|---|
| document, styles, settings | modèle + squelette (octets source ignorés ; test d'altération) |
| `[Content_Types].xml`, `.rels` | table du paquet, canonique (voir « Déterminisme ») |
| numbering, fontTable, webSettings, theme, en-têtes, pieds, notes, commentaires | opaque |
| core, app | opaque, valeurs du modèle appliquées (US-328) |
| custom, glossaire, customXml, VBA, signatures, médias, polices | opaque |

La référence qui dit ce que l'appelant a changé n'est plus une réimportation du paquet : l'import garde un
instantané du modèle (`Baseline`, clone du document, des styles, des réglages, du thème). Coût : une copie
du modèle en mémoire par document ouvert ; gain : l'écriture ne relit plus aucun octet, et `limits` n'est plus
utilisé. `WrittenDocx::opaque_parts` liste les parties opaques ; `regenerated_parts + reused_parts +
opaque_parts` couvrent le paquet.

## Cas délicats

* **Suivi des modifications** : l'enveloppe `w:ins` suit le texte ; si tout son texte est supprimé,
  elle disparaît (`EnvelopeDropped`). Les `w:del`/`w:moveFrom` sont des objets ancrés : ils suivent
  le décalage et disparaissent avec le texte autour s'ils sont supprimés du modèle.
* **`mc:AlternateContent`** : sa copie opaque est réémise tant que le contenu de sa branche
  (interprété dans le modèle, `shadow`) est inchangé. Si ce texte ou ces blocs sont modifiés, ou si
  la copie est supprimée ou séparée de sa branche par un bloc inséré, la copie est abandonnée et le
  contenu édité la remplace (autre branche perdue, `AlternateContentFlattened`).
* **Marques appariées** (début/fin de commentaire, signet, champ) : ce sont des objets ancrés
  indépendants ; si une moitié disparaît avec son texte, l'autre est réémise telle quelle (pas
  d'appariement imposé).
* **Numérotation** : `ListRef` devient `w:numPr` (`numId` = identifiant + 1) ; sans partie
  `numbering.xml`, une définition à puces à neuf niveaux est écrite. L'import ne la relit pas.

## Document créé de zéro

Paquet : `[Content_Types].xml`, `_rels/.rels`, `word/document.xml`, `styles.xml` (valeurs par défaut,
styles du profil US-202 avec noms intégrés de Word, `outlineLvl` des titres, bordures du style de
tableau), `settings.xml`, `fontTable.xml` (polices utilisées), `numbering.xml` si besoin,
`docProps` via `update_on_save`. Préfixes `w:`, `r:`, `mc:`, `w14:`, `mc:Ignorable="w14"`. Les
identifiants de style (`atelier.heading.1`…) sont écrits tels quels dans `w:styleId`.

## Variantes

Le type de contenu principal suit `variant`. `vbaProject.bin`, `vbaData` et signatures sont des parties
du paquet, conservées telles quelles pour docm/dotm ; pour docx/dotx elles sont retirées avec leurs
relations et un avis `MacrosRemoved`.

## Déterminisme et limites

Même modèle, mêmes octets, quel que soit l'ordre de construction de la source :
* parties triées (`BTreeMap<PartUri>`), membres ZIP dans cet ordre après `[Content_Types].xml` puis les `.rels` ;
* relations triées par identifiant à l'écriture (`Package::canonicalize` avec `regenerate_all`, plus
  d'ordre hérité du fichier source) ; identifiants des nouveaux liens `rIdAtelierLink<n>` dans l'ordre
  du document (préfixe élargi d'un `x` si le paquet en a déjà) ;
* types de contenu : un `Override` par partie, un seul `Default` (`rels`) ; les `Default` de la source sont
  abandonnés avec `regenerate_all` ;
* horodatage ZIP fixe (zéro DOS), compression Deflate déterministe, aucune horloge (dates fournies par
  l'appelant) ;
* aucun `HashMap`/`HashSet` dans `fmt-ooxml` et `zip` (recherche faite) ; attributs dans l'ordre de la source
  (squelette) ou du code de l'écrivain.
Sources de non-déterminisme trouvées : l'ordre des relations et les `Default` de la source, qui se
retrouvaient dans la sortie ; la dépendance aux octets source via la réimportation de référence. Corrigés.
`max_part_bytes` (512 Mio) vérifié à chaque bloc ; profondeur d'imbrication bornée (128).

## Différences admises à l'aller-retour

Zéro différence XML normalisée sur le corpus, partie par partie (voir Preuves). Hors aller-retour sans
modification : les commentaires XML et instructions de traitement des parties interprétées
(non préservés, voir US-306) ne sont pas restitués ; un `color auto`/`highlight none` à côté d'une
valeur explicite devient la valeur explicite ; un tableau terminant un corps ou une cellule reçoit un
paragraphe vide ; un corps vide en reçoit un.

## Preuves

Tests : `docx_write.rs` (aller-retour du corpus `basic.docx`, `libreoffice/*.docx`, trois documents
construits dont le « riche » de US-306 : parties interprétées comparées normalisées, parties non
interprétées identiques octet pour octet, réimport égal pour le document, les tableaux, les réglages,
le squelette), `docx_write_new.rs` (120 documents créés de zéro, relus égaux, ordre des enfants de
`rPr`, `pPr`, `tblPr`, `tcPr`, `sectPr` conforme au schéma, déterminisme), `docx_write_edit.rs`
(cas ciblés et campagne à graine fixe : 240 documents importés puis édités au hasard, écrits, relus ;
texte et structure égaux ; au plus 5 écarts de mise en forme tolérés, listés), `docx_write_variants.rs`.
Critère 1 (« toutes les parties générées depuis le modèle, sortie déterministe »), dans `docx_roundtrip_corpus.rs` (`determinism_problems`, appelé pour chacun des 67 documents : 4 du dépôt, 63 synthétiques) : avec `regenerate_all`, aucune partie réutilisée ; deux écritures identiques ; octets de `document.xml`, `styles.xml`, `settings.xml` du paquet source remplacés par du XML invalide après l'import : sortie identique ; même source reconstruite en ordre inverse (membres ZIP, relations, `Default` en trop) : sortie identique ; point fixe après un second cycle ouvrir/enregistrer ; `regenerated_parts` + `opaque_parts` = parties du paquet. Un test de contrôle montre que sans `regenerate_all` la disposition de la source se voit dans la sortie. `docx_write_new.rs` : 120 documents créés de zéro, construits deux fois indépendamment, mêmes octets, point fixe après aller-retour. Le test d'aller-retour US-306 (0 perte) passe avec `regenerate_all`.

`tools/check-docx-writer-libreoffice.py` : voir « Vérification avec LibreOffice ».

## Vérification avec LibreOffice

À compléter par le résultat de la commande `py tools/check-docx-writer-libreoffice.py` (voir le
rapport de livraison). LibreOffice lit de façon tolérante : son acceptation ne prouve pas la
conformité stricte que Word exige. **Aucune validation de schéma XSD** n'est disponible sur la
machine ; elle est remplacée par le test de l'ordre des enfants. **Word n'a pas été testé.**

## Réserve sur le critère 1 (non coché)

Déterministe et généré depuis le modèle pour document, styles, settings, types de contenu et relations ;
restent à générer les en-têtes et pieds (dette US-304 : import non lu ; le modèle les porte
(`Section.headers/footers`) mais ils sont recopiés, `HeaderFooterNotWritten`) et les définitions de
numérotation (`abstractNum`/`num` ne sont pas dans le modèle, qui n'a que des `ListRef`) ; les parties hors
modèle sont réémises depuis le magasin opaque.

* **Numérotation** : un `ListRef` dont l'identifiant (`numId` = identifiant + 1) n'est pas défini par le
  `numbering.xml` recopié donne `WriteNotice::ListDefinitionMissing { list_id }` (le modèle n'a pas de
  définition à fusionner). Sans `numbering.xml`, une définition à puces est écrite.
* **Notes et commentaires** : `footnotes.xml`, `endnotes.xml` et `comments.xml` sont recopiés. Après la
  suppression d'un texte qui portait une référence, la note ou le commentaire reste dans la partie sans
  être référencé ; le fichier reste valide (Word ignore les entrées orphelines). Test :
  `notes_and_comments_copied_keep_unreferenced_entries_after_a_deletion`.
* **`free_link_prefix`** dépend du contenu du paquet (les identifiants de relation du document principal) :
  le préfixe des nouveaux liens change si le paquet en contient déjà, donc deux sources aux relations
  différentes mais au même modèle peuvent nommer leurs liens différemment.
* **Dette mémoire** : l'instantané `Baseline` coûte environ deux fois le modèle par document ouvert ; à
  remplacer par une empreinte (hachage par nœud) qui suffise à détecter les modifications.

## Critère non vérifié : rendu identique dans Word

Procédure manuelle (deux minutes) : `cargo run -p atelier-fmt-ooxml --example write_docx_samples
-- <dossier>` écrit `<dossier>/word/sample-*.docx` ; ouvrir chaque fichier dans Word 365, vérifier
qu'il s'ouvre sans demande de réparation, puis comparer à la maquette décrite par le modèle (titre,
intertitre, texte mis en forme, tableau, lien, signet, section). Statut : **non vérifié**.

## Limites

Champs, liens et signets ne sont pas relus dans le modèle par l'import ; en-têtes/pieds, images,
numérotation fine : US-302/304/305. Les modifications de fusions de cellules d'un tableau importé
reconstruisent la grille (propriétés de cellule des continuations perdues). Un paragraphe réécrit
depuis le modèle perd ses enveloppes. Le thème modifié n'est pas écrit.
