# US-052 — Contours CFF

`FontFace::outline(glyph_id)` et `outline_with_limits` choisissent automatiquement le décodeur : si la police n’a pas de table `glyf` mais une table `CFF `, le contour vient de l’interpréteur Type 2 ; sinon le chemin TrueType de US-051 est inchangé. La sortie est le même `atelier_geom::Path` (unités de police, Y vers le haut), avec des courbes cubiques et des contours fermés implicitement, et la même `OutlineError`. Une table `CFF2` sans `glyf` renvoie `Unsupported("CFF2 variable outlines")`.

Le module `atelier_font::cff` expose aussi `CffFont::parse(table)` pour un usage direct : `outline`, `glyph` (contour et largeur d’avance du charstring), `glyph_count`, `is_cid`, `font_dict_count`, `font_dict_index`, `charset`, `glyph_id_in_charset` et `string`.

## Analyse

En-tête CFF 1, INDEX (Name, Top DICT, String, Global Subr, CharStrings, FDArray, Local Subr), DICT avec entiers (formes 1, 2, 3 et 5 octets) et réels (nibbles), Private DICT (`defaultWidthX`, `nominalWidthX`, `Subrs`), charsets formats 0, 1 et 2 ainsi que ISOAdobe prédéfini, polices CID (ROS, FDArray, FDSelect formats 0 et 3 validés en entier). Les INDEX vérifient `offSize`, le premier offset, la monotonie et les bornes à chaque accès.

## Interpréteur Type 2

Tous les opérateurs de tracé (`rmoveto`, `hmoveto`, `vmoveto`, `rlineto`, `hlineto`, `vlineto`, `rrcurveto`, `hhcurveto`, `vvcurveto`, `hvcurveto`, `vhcurveto`, `rcurveline`, `rlinecurve`, `flex`, `hflex`, `hflex1`, `flex1`), `callsubr`/`callgsubr` avec biais 107, 1131 ou 32768, `return`, `endchar`, largeur optionnelle, nombres 16.16 et opérateurs arithmétiques (`and`, `or`, `not`, `abs`, `add`, `sub`, `div`, `neg`, `eq`, `drop`, `put`, `get`, `ifelse`, `mul`, `sqrt`, `dup`, `exch`, `index`, `roll`). Les indices de hint (`hstem*`, `vstem*`, `hintmask`, `cntrmask`) sont comptés pour sauter les octets de masque ; le hinting n’est jamais appliqué.

## Limites

Pile de 48 valeurs, profondeur de sous-routines 10 (`max_depth` peut seulement la réduire ; au-delà de 64, `LimitExceeded("public depth cap")` comme pour TrueType), 1 000 000 opérations par glyphe, 96 hints. `OutlineLimits` est réutilisé : `max_components` borne le nombre total d’appels de sous-routines, `max_points` et `max_contours` s’appliquent comme pour TrueType. Un contour réduit à un `moveto` n’est pas émis. Les polices invalides donnent `Truncated`, `InvalidData`, `LimitExceeded`, `InvalidGlyph` ou `Geometry` ; il n’y a ni panique ni boucle possible (pas de saut, récursion plafonnée).

## Preuves

Le dépôt ne contient aucune police CFF. `tests/cff_outlines.rs` assemble des tables CFF synthétiques (constructeur d’en-tête, INDEX, DICT et charstrings) et vérifie chaque opérateur de tracé contre des chemins calculés à la main, les trois plages de biais (jusqu’à 33 900 sous-routines), la profondeur 10/11, les budgets, les largeurs, les masques de hints, les charsets, une police CID avec FDSelect 0 et 3, l’intégration via `FontFace` (TrueType non modifié, CFF2 rejeté) et une campagne à graine fixe de troncatures et de mutations (environ 50 000 tables et 20 000 charstrings aléatoires) sans panique.

Contrôle local hors dépôt : les 35 fichiers `.otf` de `C:\Windows\Fonts` (polices CFF à noms de glyphes, non CID) ont été décodés ; pour 8 171 glyphes, les boîtes englobantes concordent avec FontTools à 0,01 unité près et la largeur du charstring égale l’avance `hmtx`. Aucune police système n’est copiée ni utilisée par les tests.

## Non couvert

CFF2 (polices variables), charstrings Type 1, `endchar` à quatre arguments (accents `seac`, `Unsupported`), opérateur `random` (`Unsupported`), transformations `FontMatrix`, charsets Expert prédéfinis (`Unsupported`), table des chaînes standard (SID inférieurs à 391 non résolus en noms), hinting. Aucune police CID réelle n’a été testée, uniquement des fixtures synthétiques. US-052 reste ouverte pour les cas CFF1 valides encore refusés ; l’intégration du module ne vaut pas clôture complète de cette US.

La vérification d’intégration refuse désormais les sous-routines locales/globales qui arrivent en fin de données sans `return` ou `endchar`, conformément au [format Type 2, §2.3](https://partners.adobe.com/public/developer/en/font/5177.Type2.pdf). Le Top DICT accepte seulement une FontMatrix absente ou égale au défaut ; toute FontMatrix explicite de FD DICT est refusée jusqu’à prise en charge des transformations. Les six opérandes et le défaut Top DICT sont définis dans la [spécification CFF, table 9](https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf).
