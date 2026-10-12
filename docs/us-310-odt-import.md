<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-310 — Ouvrir un fichier ODT

`atelier_fmt_odf::odt::import_odt` lit un document texte OpenDocument (`.odt`,
modèle `.ott`, document « à plat » `.fodt`) et renvoie un `ImportedOdt` : un
`RichDocument` validé, le `StyleEngine` de l'US-201, et des tables annexes typées
pour ce que le modèle riche ne peut pas porter. La forme de l'API reprend celle de
l'import DOCX ([US-301](us-301-docx-import.md)) afin que l'appelant traite les
formats de façon homogène. ODF reste en **lecture seule** (SPEC §5.2) : rien ici
n'écrit de l'ODF, et la proposition d'enregistrer en docx relève de l'interface
(US-322).

```rust,ignore
use atelier_fmt_odf::odt::{import_odt, import_odt_with_limits, OdtLimits};

let mut imported = import_odt(&bytes)?;
for id in imported.paragraph_ids().to_vec() {
    // Runs aux propriétés effectives (défauts, style de paragraphe, style de
    // caractère nommé, mise en forme directe) + propriétés de paragraphe.
    let (paragraph, resolved) = imported.resolved_paragraph(id)?;
}
for notice in &imported.notices { /* ce qui n'a pas pu être représenté */ }
```

| Élément | Rôle |
| --- | --- |
| `import_odt(&[u8])`, `import_odt_with_limits(&[u8], OdtLimits)` | points d'entrée ; le format (paquet ZIP ou XML plat) est détecté sur le contenu |
| `ImportedOdt::document` | `RichDocument` validé (`document.validate()` est appelé avant publication) |
| `ImportedOdt::styles` | `StyleEngine` alimenté par les styles communs, la valeur par défaut du document et leurs héritages |
| `resolved_paragraph`, `paragraph_ids` | même contrat que `ImportedDocx` ; `paragraph_ids` liste les paragraphes du corps (cellules et listes comprises, en-têtes et pieds exclus) |
| `run_styles` | styles de caractère nommés par plage (`CharRun` ne porte que des propriétés directes ; même technique que `ImportedRunStyle` du DOCX) |
| `sections` | une entrée par section : page maîtresse d'origine et géométrie de page (`PageSetup`, en twips) |
| `lists`, `list_items` | définitions de numérotation (puce, format, préfixe, suffixe, départ par niveau) et informations par paragraphe (en-tête de liste, valeur de départ, paragraphe de continuation) |
| `images` | métadonnées de chaque image (ancrage, taille, titre/description, état) ; les octets sont dans l'`AnchoredObject` (`kind` = type de média, `data` = octets d'origine) |
| `headings` | niveau de plan de chaque `text:h` |
| `style_names`, `next_styles` | nom affiché (`style:display-name`) et style suivant (`style:next-style-name`) par identifiant de style du modèle |
| `metadata` | titre, sujet, auteur, premier auteur, description, mots-clés, langue, générateur, dates (texte ISO 8601 tel quel) et statistiques de `meta:document-statistic` |
| `mime_type`, `is_template` | type lu dans `mimetype` (ou dans le manifeste) |
| `notices` | liste dédoublonnée et bornée d'`ImportNotice` |

Les erreurs sont typées (`OdtError`) : `Zip`, `NotOdf`, `UnsupportedDocumentType
{ mime_type }` (nomme le type : tableur, présentation, dessin, maître…),
`PasswordProtected` (le manifeste déclare des entrées chiffrées), `MissingPart`,
`Xml { part, message }`, `InvalidStructure`, `Limit`, `Style`, `Model`. Une
erreur ne renvoie jamais un document partiel. À l'inverse, un `styles.xml` ou
un `meta.xml` absent, mal formé ou illisible n'empêche pas l'import du texte :
c'est un avis (`MissingPart`, `UnreadablePart`).

## Paquet

Un paquet sans `content.xml` (modèle de styles seul) s'ouvre comme un document
vide avec ses styles et l'avis `MissingPart`. `mimetype` est vérifié (`application/vnd.oasis.opendocument.text` et
`…text-template` acceptés ; tout autre type ODF ou `application/vnd.*` donne
`UnsupportedDocumentType`). Un `mimetype` absent est remplacé par le type de la
racine du manifeste. `META-INF/manifest.xml` fournit les types de média des
images et la détection du chiffrement (`manifest:encryption-data`). `styles.xml`,
`content.xml`, `meta.xml` sont lus indépendamment ; les images sont lues sous
`Pictures/` à la demande, avec contrôle de taille avant lecture. Un corps qui
n'est pas `office:text` (tableur, présentation…) est refusé même si `mimetype`
dit autre chose. Les liens vers l'extérieur (`xlink:href` avec schéma, `file:`,
`../`) ne sont jamais ouverts. Le document « à plat » `.fodt` est pris en
charge parce que c'est gratuit : même arbre XML, un seul fichier, images en
`office:binary-data` décodées en base64 (borné).

## Éléments pris en charge

| Domaine | Pris en charge | Traitement |
| --- | --- | --- |
| Texte | `text:p`, `text:h`, `text:span`, `text:s` (`text:c`), `text:tab`, `text:line-break`, `text:soft-page-break` | blancs normalisés selon ODF §6.1.2 (suite d'espaces/tab/retours → une espace, blancs de tête supprimés, `text:s` littéral) ; `\t` et `\n` dans le texte ; saut de page souple ignoré |
| Titres | niveau de plan, style `Heading_20_N` | style `HeadingN` si le fichier le définit ; niveau consigné dans `headings` |
| Liens | `text:a` | `LinkTarget::External`, ou `Bookmark` pour `#nom` (suffixe `\|outline` retiré) ; `office:title` → info-bulle ; liens qui se chevauchent : le second est ignoré (avis) |
| Signets | `text:bookmark`, `-start`/`-end` | `Bookmark` (point = plage vide) ; doublon ignoré (avis) ; plage qui traverse un paragraphe : fermée en fin de paragraphe (avis) |
| Sections | `text:section`, index et tables des matières | contenu aplati (avis) |
| Notes | `text:note` | appel (`note-citation`) conservé en exposant ; contenu ignoré (avis) |
| Modifications | `text:tracked-changes`, `change*` | ignorées (avis) : seul l'état final est importé |
| Listes | `text:list`, `list-item`, `list-header`, imbrication, `text:start-value`, `continue-numbering`, `continue-list` (+ `xml:id`) | `ParaProps.list = ListRef { list_id, level }` ; `list_id` identifie une liste (les listes qui continuent partagent l'identifiant) ; niveau limité à 8 ; définitions dans `lists` (avis `ListNumbering`) |
| Tableaux | `table:table`, colonnes (répétitions), lignes (répétitions), `table-header-rows`, cellules, `covered-table-cell`, fusions, tableaux imbriqués | grille toujours rectangulaire : fusions rognées à la grille, trous comblés par des cellules vides, cellule toujours terminée par un paragraphe ; largeurs de colonnes (twips) et nombre de lignes d'en-tête en attributs opaques `column-widths` / `header-rows` de l'espace de noms `urn:atelier:odf-import` ; propriétés de cellule et de tableau en attributs opaques (avis) |
| Images | `draw:frame` > `draw:image` (`xlink:href` ou `office:binary-data`) | `AnchoredObject` dont `kind` est le type de média et `data` les octets d'origine, sans transcodage ; taille, ancrage (`char`, `as-char`, `paragraph`, `page`, `frame`), nom, `svg:title`, `svg:desc` dans `images` ; image absente / externe / trop grande : objet `odf:image-reference` + avis |
| Autres cadres | `draw:text-box`, `draw:object`, `draw:object-ole`, plugins, formes `draw:*` | objet ancré opaque (`odf:text-box`, `odf:object`, `odf:shape`) dont `data` est le fragment XML d'origine, sérialisé et autonome ; image de repli d'un objet OLE importée comme image ; avis |
| Champs | `text:page-number` → `PAGE`, `text:page-count` → `NUMPAGES`, `text:date` → `DATE`, `text:time` → `TIME` | `Field` autour du texte du résultat ; date/heure `text:fixed="true"` → texte simple ; autres champs (`text:title`, `author-name`, `sequence`…) : texte du résultat conservé, avis `FieldReplacedByResult` |
| Mise en page | `style:master-page`, `style:page-layout` | changement de page maîtresse (`style:master-page-name` sur un style de paragraphe ou de tableau) → nouvelle `Section` ; format, orientation, marges dans `sections[i].page` et en attributs opaques de la section ; en-têtes/pieds (`style:header`, `-first`, `-left`) → `HfVariant::Default/First/Even` attachés à la section |
| Métadonnées | `meta.xml` | voir `ImportedOdt::metadata` |

## Styles

* **Styles communs** (`office:styles`, familles `paragraph` et `text`) deviennent des `Style` du modèle, avec
  `based_on` (`style:parent-style-name`), valeurs par défaut du document
  (`style:default-style` de la famille `paragraph` → `DocumentStyleDefaults`),
  taille relative (`fo:font-size="130%"`) résolue en demi-points sur la chaîne des
  parents, et liste associée (`style:list-style-name`). Un parent inconnu ou un
  cycle d'héritage coupe le lien et émet `UnknownStyle` / `StyleCycle` ; la
  résolution ne boucle jamais.
* **Styles automatiques** (`office:automatic-styles` de `content.xml` et de
  `styles.xml`, deux portées distinctes) deviennent de la **mise en forme
  directe** : propriétés de paragraphe sur le paragraphe, propriétés de texte
  d'un style de paragraphe automatique sur tout le texte du paragraphe, propriétés
  d'un style de texte automatique sur sa plage. Leur parent commun est la
  référence de style (`ParaProps.style`, `run_styles`). Pour des `text:span`
  imbriqués, les propriétés directes se cumulent de l'extérieur vers l'intérieur
  et la référence de style est le style commun le plus interne.
* **Équivalences nettes** avec les styles intégrés de LibreOffice (identifiant du
  modèle ← nom ODF) : `Normal` ← `Standard`, `Heading1`…`Heading9` ←
  `Heading_20_1`…`9`, `Title`, `Subtitle`, `BodyText` ← `Text_20_body`, `Header`,
  `Footer`, `Caption`, `Quote` ← `Quotations`, `List`, `Strong` ←
  `Strong_20_Emphasis`, `Emphasis`, `Hyperlink` ← `Internet_20_link`,
  `FollowedHyperlink` ← `Visited_20_Internet_20_Link`. Les autres styles gardent
  leur nom ODF (`style:name`) comme identifiant ; en cas de collision un suffixe
  `-2` est ajouté. Le nom affiché est dans `style_names`, le style suivant dans
  `next_styles`.

### Propriétés de texte (`style:text-properties`)

| ODF | Modèle |
| --- | --- |
| `fo:font-weight` (`bold`, `normal`, 100–900) | `bold` |
| `fo:font-style` (`italic`, `oblique`, `normal`) | `italic` |
| `style:text-underline-style/-type/-width` | `underline` (`Single`, `Double`, `Thick`, `Dotted`, `Dashed`, `Wave`, `None`) |
| `style:text-line-through-style` | `strike` |
| `style:text-position` (`super`, `sub`, pourcentage) | `vertical_align` (la taille réduite n'est pas lue) |
| `style:font-name` (résolu par `office:font-face-decls`, `svg:font-family`), `fo:font-family` | `font` |
| `fo:font-size` (longueur ou %) | `size_half_points` |
| `fo:color` | `color` |
| `fo:background-color` (valeur RVB) | `highlight` |

Sans équivalent dans `CharAttrs` : `fo:letter-spacing`, `fo:font-variant`,
`fo:text-transform`, `text:display` (masqué), `fo:language`/`fo:country`,
`style:text-underline-color`, `style:use-window-font-color`, `fo:background-color`
transparent (le modèle ne sait pas annuler une valeur héritée), ombre, relief,
etc. Chacune est conservée telle quelle en `OpaqueAttr` (espace de noms ODF
d'origine, nom local, valeur textuelle) ; un avis `UnsupportedProperty` est émis
pour celles qui ont un effet visible. Les variantes `*-asian` / `*-complex` et
quelques indications purement typographiques sont ignorées sans avis.

### Propriétés de paragraphe (`style:paragraph-properties`)

| ODF | Modèle |
| --- | --- |
| `fo:text-align` (`left`, `right`, `center`, `justify`, `start`, `end`) ; `style:writing-mode` `rl*` inverse `start`/`end` | `alignment` |
| `fo:margin-left`, `fo:margin-right`, `fo:margin` | `indent_left`, `indent_right` (twips) |
| `fo:text-indent` | `indent_first_line` |
| `fo:margin-top`, `fo:margin-bottom` | `space_before`, `space_after` (négatifs ramenés à 0) |
| `fo:line-height` en % | `LineSpacing { Auto, % × 2,4 }` (240 = simple) |
| `fo:line-height` en longueur | `LineSpacing { Exact, twips }` |
| `style:line-height-at-least` | `LineSpacing { AtLeast, twips }` |

Sans équivalent dans `ParaProps`, conservés en `OpaqueAttr` avec avis :
`style:tab-stops` (valeur `position_twips:type:remplissage;…`), `fo:keep-together`,
`fo:keep-with-next`, `fo:widows`, `fo:orphans`, `fo:break-before`, `fo:break-after`,
bordures, remplissage (`fo:padding`), fond de paragraphe, `style:line-spacing`
(interligne supplémentaire), `style:tab-stop-distance` (tabulation par défaut) et
`text:display` de paragraphe. Unités : `cm`, `mm`, `in`, `pt`, `pc`, `px` (96 dpi)
vers les twips (1/20 pt) ; pourcentages et unités relatives à la police pour les
longueurs sont refusés (valeur conservée en opaque).

## Avis émis

`MissingPart`, `UnreadablePart`, `RepairedArchive` (annuaire ZIP abîmé, entrées
récupérées), `UnsupportedElement`, `UnsupportedProperty`, `UnknownStyle`,
`StyleCycle`, `NestingLimit`, `FlattenedSection`, `NoteContentIgnored`,
`TrackedChangesIgnored`, `ListNumbering`, `ObjectKept`, `ImageUnavailable`,
`FieldReplacedByResult`, `DuplicateBookmark`, `BookmarkSpansParagraphs`,
`OverlappingLink`, `TooManyNotices`. Les avis sont dédoublonnés et bornés par
`OdtLimits::max_notices` ; `TooManyNotices` clôt une liste tronquée.

## Limites et robustesse

`OdtLimits` : limites ZIP et XML (`atelier-zip`, `atelier-xml`), taille d'une partie
XML (128 Mio), paragraphes (1 000 000), octets de texte (256 Mio), styles (100 000),
profondeur d'imbrication des listes, tableaux et sections (32, au-delà : avis
`NestingLimit` et contenu ignoré), cellules et lignes de tableaux (1 000 000, vérifié
sur les attributs de répétition **avant** toute allocation), images (10 000), octets
par image (64 Mio) et au total (256 Mio), avis (10 000). `text:s` ne peut pas dépasser
le budget de texte restant. Les chaînes de styles sont parcourues avec un compteur
de pas ; aucun parcours n'est récursif sur des données non bornées (la profondeur XML
est elle-même limitée par `atelier-xml`).

## Lacunes du modèle riche (aucune modification du modèle)

* pas de tabulations, de contrôles d'enchaînement, de bordures ni de fond de paragraphe ;
* pas d'interlettrage, petites capitales/majuscules, texte masqué ni langue ;
* pas de géométrie de page, de numérotation de pages par section ni de colonnes ;
* pas de largeur de colonne, de lignes d'en-tête ni de propriétés de cellule ;
* pas de définitions de numérotation (puces, formats, retraits) : seulement `ListRef` ;
* pas de style de caractère nommé sur une plage ni de style suivant ;
* pas d'objet ancré typé (`kind`/`data` seulement) ni de notes de bas de page ;
* les polices intégrées (`Fonts/`) ne sont pas lues (hors périmètre de cette livraison).

## Preuves

* Tests unitaires et d'intégration sur des paquets construits en mémoire avec
  `atelier-zip` (`crates/fmt-odf/tests/odt_import.rs`) : un test par construction
  ci-dessus, avec résultat attendu précis (texte, plages d'attributs, propriétés
  calculées après cascade par `StyleEngine`).
* Corpus de **fichiers réels produits par LibreOffice** (`tests/odt-reference/`,
  [README](../tests/odt-reference/README.md)) : 14 fichiers `.odt` + un `.fodt`,
  attendus relevés indépendamment par `tools/generate-odt-reference.py`, nombre de
  pages mesuré par LibreOffice sur chaque fichier. Test :
  `crates/fmt-odf/tests/odt_reference.rs`.
* Contrôle local **hors dépôt** (test `local_directory_imports_without_error`,
  ignoré par défaut, répertoire donné par `ATELIER_ODT_LOCAL_DIR`) sur les 80
  modèles `.ott`/`.odt` livrés avec LibreOffice (agendas, lettres, fax, CV,
  styles, index…) : 80 importés sans erreur, chaque paragraphe résolu par le moteur
  de styles ; seul un fichier de 0 octet est écarté. Ce contrôle a fait corriger
  deux cas réels : paquets sans `content.xml`, propriétés de paragraphe écrites dans
  `style:text-properties` par certains producteurs (honorées pour `fo:text-align`,
  `fo:line-height`, `fo:text-indent`).
* Campagne de mutations et troncatures à graine fixe (`odt_robustness.rs`) :
  mutations d'octets, troncatures et fragments XML sur paquets et sur `.fodt`,
  octets aléatoires, structures pathologiques (imbrication profonde, 50 000
  paragraphes, cycles de styles) ; aucune panique, aucune boucle infinie.

## Critères d'acceptation

1. **Contenu** (`content.xml`, `styles.xml`, `meta.xml` : texte, styles, listes,
   tableaux, images, en-têtes et pieds, mise en page, liens, signets, champs
   page/date) : couvert et vérifié sur paquets construits et fichiers LibreOffice,
   sous réserve des lacunes ci-dessus (notamment : définitions de numérotation et
   géométrie de page sont exposées en tables annexes, pas dans le modèle).
2. **Proposition d'enregistrer en docx** : hors périmètre de cette livraison
   (interface, US-322). Le crate n'expose aucune écriture ODF.
3. **Rendu conforme à LibreOffice sur 50 documents (pages identiques pour ≥ 90 %)** :
   *non vérifiable à ce stade*. Cela suppose la mise en page complète (composition
   des lignes, pagination, en-têtes/pieds, tableaux, images flottantes) qui n'existe
   pas encore. Ce qui est prêt : un corpus réel avec le nombre de pages attendu
   (`pageCount`, 14 fichiers) et l'import vérifié des textes et styles dont la mise
   en page aura besoin. Il manque : la mise en page, un corpus de 50 documents et
   l'harnais de comparaison.
