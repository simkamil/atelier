<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-312 — Ouvrir un .doc : texte, mise en forme, styles

`atelier-fmt-legacy::doc::import_doc` lit un document Word 97–2003 (format binaire [MS-DOC]) et
renvoie un `ImportedDoc` de même forme que l'`ImportedDocx` de [US-301](us-301-docx-import.md) :
un `RichDocument` validé, le `StyleEngine` de [US-201](us-201-style-engine.md), des avis d'import,
des limites explicites et des erreurs typées. Le module `doc` est séparé : le crate recevra plus
tard les lecteurs `.xls` et `.ppt`. Aucune dépendance externe (seuls `atelier-cfb`,
`atelier-text-model`, `atelier-model`, `atelier-unicode`), `#![forbid(unsafe_code)]`, aucune API
système (compilable en WASM). Lecture seule, sans déchiffrement.

```rust,ignore
use atelier_fmt_legacy::doc::{import_doc, import_doc_with_limits, DocError, DocLimits};

match import_doc(&bytes) {
    Ok(mut doc) => {
        let id = doc.paragraph_ids()[0];
        let (paragraph, defaults) = doc.resolved_paragraph(id)?; // styles résolus
        for notice in &doc.notices { /* ce que le modèle ne sait pas représenter */ }
        let titre = doc.properties.title.as_deref();
    }
    Err(DocError::PasswordProtected { obfuscated }) => { /* US-315 */ }
    Err(e) => eprintln!("{e}"),
}
```

## API

- `import_doc(&[u8])` et `import_doc_with_limits(&[u8], DocLimits)`.
- `ImportedDoc` : `document` (`RichDocument`), `styles` (`StyleEngine`), `info` (`DocInfo` : `nFib`,
  `nFibNew`, `DocVersion`, modèle `fDot`, fichier « fast-saved », flux de tables lu, `lid`,
  `ccpText`), `properties` (`DocumentProperties`), `fonts` (`Vec<DocFont>` : nom, nom alternatif,
  jeu de caractères, famille, pas, TrueType, graisse), `style_records` (une entrée par `STD`, y
  compris ceux qui ne sont pas importés), `breaks` (`ImportedBreak`), `run_styles`
  (`ImportedRunStyle`, styles de caractère par plage d'octets, comme pour le DOCX), `notices`,
  `paragraph_ids()` et `resolved_paragraph(id)` (mêmes règles que le DOCX : défauts du document →
  héritage des styles → style de caractère → mise en forme directe).
- Les sauts de ligne, de page et de colonne restent des `\n` dans le texte du paragraphe avec un
  `ImportedBreak` (`kind` `"br"` et `break_type` `None`, `"page"` ou `"column"`) ; une marque de
  section (0x0C déclarée par `PlcfSed`) termine le paragraphe avec un `ImportedBreak` de `kind`
  `"section"`. Même convention que le DOCX.
- `DocError` : `Cfb(CfbError)`, `NotWordDocument`, `MissingStream("WordDocument" | "0Table" |
  "1Table")`, `InvalidFib`, **`PasswordProtected { obfuscated }`**, **`UnsupportedVersion { n_fib }`**,
  `InvalidStructure`, `Limit(&'static str)`, `Style`, `Model`. Aucun document partiel n'est renvoyé
  avec une erreur ; les dommages locaux deviennent des avis (`DamagedData`).

## Ce qui est lu

| Structure | Lecture |
|---|---|
| Conteneur | `Compound::open` avec les limites `DocLimits::cfb` ; flux `WordDocument`, `0Table`/`1Table` selon `fWhichTblStm`, `Data` seulement si un `sprmPHugePapx` l'exige, `\x05SummaryInformation`. |
| `FibBase` | `wIdent` 0xA5EC, `nFib`, `lid`, drapeaux (`fDot`, `fComplex`, `cQuickSaves`, `fEncrypted`, `fWhichTblStm`, `fObfuscated`). Le chiffrement est testé **avant** la version et avant toute lecture du texte. |
| `FibRgW`, `FibRgLw` | tailles annoncées par `csw`/`cslw` ; `ccpText`, `ccpFtn`, `ccpHdd`, `ccpAtn`, `ccpEdn`, `ccpTxbx`, `ccpHdrTxbx`. |
| `FibRgFcLcb` | nombre de paires annoncé par `cbRgFcLcb` (93 en Word 97, 108, 136, 164, 183), borné par les octets disponibles et par 1024 ; une paire absente vaut `(0, 0)`, une paire en trop est ignorée. Paires utilisées : `Stshf` (1), `PlcfSed` (6), `PlcfBteChpx` (12), `PlcfBtePapx` (13), `SttbfFfn` (15), `Clx` (33). `nFibNew` de `FibRgCswNew` est lu (Word 2007 : 0x0112). |
| Table des pièces | `Clx` → `Prc*` + `Pcdt` → `PlcPcd`. `FcCompressed` : bit 30 = texte 8 bits (déplacement = `fc / 2`), sinon UTF-16LE. Les CP sont des unités UTF-16 ; paires de substitution reconstruites, substituts isolés → U+FFFD. Correspondance CP ↔ FC par pièce (`Piece::fc_of`), recherche dichotomique. |
| Texte 8 bits | Windows-1252 ; octets 0x80–0x9F selon le tableau du format (0x80 → €, 0x82–0x8C, 0x8E, 0x91–0x9C, 0x9E, 0x9F comme la page de code) ; les cinq positions non définies (0x81, 0x8D, 0x8F, 0x90, 0x9D) gardent leur point de code Latin-1. |
| CHPX / PAPX | `PlcfBteChpx` / `PlcfBtePapx` → pages FKP de 512 octets (`crun`, `rgfc`, `rgb` ou `rgbx` de 13 octets, `Chpx`, `PapxInFkp` avec ses formes `cb` et `0 + cb'`, `istd`). Une page référencée plusieurs fois n'est lue qu'une fois. `sprmPHugePapx` : le grpprl est lu dans le flux `Data` (`cb` sur 16 bits + sprms). |
| `Prm` des pièces | `Prm1` (`fComplex` = 1) : le grpprl du `Prc` indiqué est appliqué après le CHPX (caractère) et après le PAPX (paragraphe). `Prm0` (modification simple, tableau `isprm`) : **reconnu mais non appliqué**, avis `SimplePrmIgnored` (voir « Restes »). |
| `STSH` | `Stshi` (`cstd`, `cbSTDBaseInFile` variable, polices par défaut), `LPStd` (emplacements vides), `Stdf` (`sti`, `sgc`, `istdBase`, `istdNext`, `cupx`), `xstzName`, `UPX` de paragraphe (`istd` + grpprl) et de caractère. |
| `SttbfFfn` | `Ffn` : nom, nom alternatif (`ixchSzAlt`), `chs`, famille (`ff`), pas (`prq`), TrueType, graisse. L'index `ftc` des sprms de police désigne l'entrée. |
| Propriétés | jeu de propriétés OLE `\x05SummaryInformation` : page de code (propriété 1 : 1252, 65001, 28591, 437, 10000), `VT_LPSTR`, `VT_LPWSTR`, `VT_FILETIME`. Titre, sujet, auteur, mots-clés, commentaires, dernier auteur, application, dates de création, d'enregistrement et d'impression en FILETIME brut (`DocumentProperties::filetime_to_unix_seconds`). Flux absent ou illisible : propriétés vides et avis `SummaryInformation`. |

## Texte

Seul le document principal (`ccpText`) est importé. Les autres récits (notes de bas de page,
en-têtes et pieds, commentaires, notes de fin, zones de texte) donnent un avis
`IgnoredText { region, characters }`. Traitement des caractères :

| Caractère | Résultat |
|---|---|
| 0x0D | fin de paragraphe |
| 0x07 | fin de cellule : paragraphe simple, avis `TableCellsFlattened` ; la marque de fin de ligne (`sprmPFTtp`, paragraphe vide) est supprimée |
| 0x0B | `\n` + `ImportedBreak` ligne |
| 0x0C | fin de paragraphe si `PlcfSed` la déclare marque de section, sinon `\n` + saut de page |
| 0x0E | `\n` + saut de colonne |
| 0x09 | `\t` |
| 0x1E, 0x1F | U+2011 (trait d'union insécable), U+00AD (trait d'union conditionnel) |
| 0xA0 | espace insécable, conservée |
| 0x13 … 0x14 … 0x15 | champs : le code est ignoré, le résultat conservé, imbrication sur 64 niveaux, avis `FieldCodesDropped` ; un champ sans séparateur est entièrement du code |
| 0x01, 0x08, 0x02, 0x05 et autres contrôles | supprimés, avis `ObjectsDropped { kind }` (`inlinePicture`, `drawnObject`, `noteReference`, `commentReference`, `controlCharacter`) |

Un texte sans marque de paragraphe finale est conservé (avis `DamagedData`). Un `ccpText`
supérieur au texte réellement présent donne un import partiel et un avis.

## Sprms pris en charge

Le décodage est générique : la taille de l'opérande vient de `spra` (0–1 : 1 octet, 2/4/5 : 2,
3 : 4, 7 : 3, 6 : variable, avec les cas particuliers `sprmTDefTable` 0xD608 et `sprmPChgTabs`
0xC615 et sa forme 255). Tout sprm inconnu est donc sauté sans perdre la synchronisation et
compté dans l'avis `UnknownSprms`. Un grpprl coupé s'arrête proprement (`DamagedData`) ; un octet
nul final (remplissage des FKP) est ignoré. Les propriétés relatives au style (0x80 « comme le
style », 0x81 « inverse du style ») sont résolues avec la chaîne `istdBase` du style de paragraphe
puis du style de caractère.

Caractère (`CharAttrs`) :

| Sprm | Opcode | Modèle |
|---|---|---|
| `sprmCFBold`, `sprmCFItalic`, `sprmCFStrike` | 0x0835, 0x0836, 0x0837 | `bold`, `italic`, `strike` (bascules) |
| `sprmCKul` | 0x2A3E | `underline` : 0 aucun ; 1, 2 simple (2 « mots seuls » approché) ; 3 double ; 4, 0x0A pointillé ; 5 épais ; 6–8, 0x0B–0x0D, 0x0F, 0x10 tirets ; 9, 0x0E, 0x11 ondulé ; autre → simple (avis) |
| `sprmCIss` | 0x2A48 | `vertical_align` |
| `sprmCHps` | 0x4A43 | `size_half_points` (2 à 3276, sinon ignoré) |
| `sprmCIco`, `sprmCCv` | 0x2A42, 0x6870 | `color` (table `Ico` de 16 couleurs ; `Cv` RGB ; le dernier gagne ; automatique → avis `automaticColor`) |
| `sprmCHighlight` | 0x2A0C | `highlight` (table `Ico`) |
| `sprmCShd`, `sprmCShd80` | 0xCA71, 0x4866 | `highlight` si aucun surlignage (le modèle n'a pas de trame de caractère : avis `characterShadingAsHighlight`) |
| `sprmCRgFtc0/1/2`, `sprmCFtcDefault` | 0x4A4F, 0x4A50, 0x4A51, 0x4A3D | `font` : ASCII, sinon « autre », sinon Extrême-Orient ; index inconnu → avis |
| `sprmCIstd` | 0x4A30 | style de caractère (`ImportedRunStyle`) |
| `sprmCPlain` | 0x2A33 | réinitialise la mise en forme directe du grpprl |
| `sprmCFSmallCaps`, `sprmCFCaps`, `sprmCFVanish` | 0x083A, 0x083B, 0x083C | **sans équivalent** : attribut opaque `urn:atelier:doc` (`smallCaps`, `caps`, `hidden`) + avis |
| `sprmCFDStrike` | 0x2A53 | `strike` (approché) + opaque `doubleStrike` + avis |
| `sprmCDxaSpace`, `sprmCHpsPos` | 0x8840, 0x4845 | opaques `spacing`, `position` (i16 petit-boutiste) + avis |
| `sprmCFOutline/Shadow/Emboss/Imprint` | 0x0838, 0x0839, 0x0858, 0x0854 | avis seulement |

Paragraphe (`ParaProps`) :

| Sprm | Opcode | Modèle |
|---|---|---|
| `sprmPIstd` (grpprl de pièce) et `istd` du PAPX | 0x4600 | `style` (identifiant du moteur ; référence invalide → Normal + avis) |
| `sprmPJc80`, `sprmPJc` | 0x2403, 0x2461 | `alignment` : 0 gauche, 1 centre, 2 droite, 3 justifié ; 4–9 (réparti…) → justifié (avis) |
| `sprmPDxaLeft80/Left`, `sprmPDxaRight80/Right`, `sprmPDxaLeft1_80/Left1` | 0x840F/0x845E, 0x840E/0x845D, 0x8411/0x8460 | `indent_left`, `indent_right`, `indent_first_line` (twips signés) |
| `sprmPDyaBefore`, `sprmPDyaAfter` | 0xA413, 0xA414 | `space_before`, `space_after` |
| `sprmPDyaLine` | 0x6412 | `line_spacing` : `fMult` = 1 → `Auto` (240ᵉ de ligne) ; sinon `dyaLine` > 0 → `AtLeast`, < 0 → `Exact` (valeur absolue), 0 → absent |
| `sprmPFKeep`, `sprmPFKeepFollow`, `sprmPFPageBreakBefore`, `sprmPFWidowControl` | 0x2405, 0x2406, 0x2407, 0x2431 | **sans équivalent** : opaques `keepLines`, `keepNext`, `pageBreakBefore`, `widowControl` + avis |
| `sprmPOutLvl` | 0x2640 | opaque `outlineLevel` (0–8) + avis |
| `sprmPChgTabsPapx`, `sprmPChgTabs` | 0xC60D, 0xC615 | opaque `tabs` (opérande brut, octet de taille compris) + avis `tabStops` |
| `sprmPHugePapx` | 0x6646 | grpprl lu dans `Data` |
| `sprmPFInTable`, `sprmPFTtp` | 0x2416, 0x2417 | tableaux : cellules aplaties (voir plus haut) |
| `sprmPIlvl`, `sprmPIlfo` | 0x260A, 0x460B | listes : avis `listNumbering` (US-313) |
| Bordures et trames de paragraphe, sprms de tableau (`sgc` 5) | 0x6424–0x6428, 0xC64E–0xC653, 0x442D, 0xC64D, … | avis `paragraphBorders`, `paragraphShading`, `tableFormatting` |

## Feuille de styles

- Types (`sgc`) : paragraphe et caractère sont importés ; tableau et liste (numérotation) sont notés
  et ignorés (`UnsupportedStyleKind`). Un paragraphe qui référence un tel style, ou un `istd`
  inexistant, retombe sur Normal (avis `DamagedData`).
- Identifiants : `Normal` (`sti` 0), `Heading1`…`Heading9` (`sti` 1–9), puis `FootnoteText`,
  `Header`, `Footer`, `Caption`, `FootnoteReference`, `Title`, `DefaultParagraphFont`, `BodyText`,
  `Subtitle`, `Hyperlink`, `FollowedHyperlink`, `Strong`, `Emphasis` : les mêmes que ceux de
  l'import DOCX. Les autres styles prennent leur nom sans les caractères qui ne sont ni
  alphanumériques ni `-` `_` (`Mon Style` → `MonStyle`), suffixé par `_istd` en cas de doublon,
  `Style{istd}` si le nom est vide. Le nom d'origine (localisé) est dans `style_records`.
- `istdBase` : doit exister, être importé et du même type, sinon la base est supprimée (avis
  `InvalidStyleReference`) ; les **cycles** sont coupés par un parcours itératif avec marquage
  (avis `base style cycle`). Aucune boucle n'est donc possible, à l'import comme à la résolution.
- Valeurs par défaut du document : police ASCII de `Stshif` (`rgftcStandardChpStsh`) et taille de
  10 pt (valeur par défaut du format quand aucun style ne la précise).
- `istdNext` est conservé dans `style_records` (le modèle n'a pas de style suivant).

## Avis d'import

`UnsupportedProperty { name, count }`, `ApproximatedProperty { name, count }`,
`UnknownSprms { count }`, `IgnoredText { region, characters }`, `TableCellsFlattened { cells }`,
`FieldCodesDropped { fields }`, `ObjectsDropped { kind, count }`,
`UnsupportedStyleKind { istd, kind }`, `InvalidStyleReference { istd, reason }`,
`SimplePrmIgnored { count }`, `SummaryInformation { problem }`, `DamagedData { what, count }`.
Les compteurs sont des nombres d'occurrences ; l'ordre est celui de production (structures, puis
récits ignorés, tableaux, champs, objets, propriétés, sprms, dommages).

Propriétés lues **sans équivalent dans le modèle riche** (non ajoutées au modèle, conservées en
attributs opaques `urn:atelier:doc` quand c'est utile) : petites majuscules, majuscules, texte
masqué, double barré, interlettrage, position haute/basse, effets (contour, ombre, relief,
gravé), couleur automatique (réinitialisation), enchaînements (lignes solidaires, paragraphes
solidaires, saut de page avant, veuves et orphelines), niveau de plan, taquets de tabulation,
bordures et trames de paragraphe, trame de caractère, numérotation, style suivant.

## Limites (`DocLimits`, vérifiées avant allocation)

| Champ | Défaut | Erreur `Limit` |
|---|---|---|
| `cfb` | limites de `atelier-cfb` | `Cfb(LimitExceeded)` |
| `max_text_chars` | 64 Mi unités | `"text characters"` (`ccpText`) |
| `max_pieces` | 1 000 000 | `"piece count"`, `"Prc count"` |
| `max_fkp_pages` | 1 000 000 | `"FKP page count"` |
| `max_styles` | 4096 | `"style count"` |
| `max_fonts` | 4096 | `"font count"` |
| `max_paragraphs` | 1 000 000 | `"paragraph count"` |
| `max_runs` | 4 000 000 | `"character runs"` (segments de mise en forme et entrées FKP) |
| `max_sprms` | 64 Mi | `"sprm count"` |

Tous les décalages et longueurs sont vérifiés contre la taille des flux ; l'allocation du texte est
bornée par la taille de `WordDocument`. Une structure secondaire endommagée (polices, feuille de
styles, tables de pages) donne un avis et un import partiel ; une `Clx` illisible est fatale. Les
sommes sont saturantes (cibles 32 bits).

## Versions et cas particuliers

- **Chiffré ou obscurci** (`fEncrypted`, avec `fObfuscated` pour l'obscurcissement XOR) :
  `DocError::PasswordProtected { obfuscated }`, sans lire le texte ni les tables. US-315 s'appuie
  sur cette variante.
- **Word 6/95** (`nFib` < 0x00C1) : `DocError::UnsupportedVersion { n_fib }`. Leur FIB et leur
  structure de texte (`fcMin`/`fcMac`, sprms sur un octet) sont différents ; ils ne sont pas lus.
- **Word 97 à 2007** (`nFib` 0x00C1, 0x00D9, 0x0101, 0x010C, `nFibNew` 0x0112) : lus. Les formats
  Word 2010 et suivants gardent ces structures (le `.doc` écrit par Word 2010+ a `nFib` 0x0112).
- **Fichiers « fast-saved » / complexes** (`fComplex`, `cQuickSaves` > 0) : pris en charge par la
  table des pièces (pièces dans un ordre quelconque du fichier, texte supprimé mais encore
  présent ignoré, `Prm1`). Signalés par `DocInfo::fast_saved`.
- Un document sans `\x05SummaryInformation` s'ouvre (avis).

## Preuves

- `crates/fmt-legacy/tests/doc_import.rs` : 42 tests sur des fichiers fabriqués par le
  constructeur déterministe `tests/common/mod.rs` (FIB, `Clx`, FKP, `STSH`, `SttbfFfn`, flux
  `Data`, propriétés OLE, au-dessus du constructeur CFB de `atelier-cfb` inclus par chemin).
  Chaque sprm pris en charge a un test au résultat calculé à la main ; texte 8 bits/Unicode,
  pièces dans le désordre, héritage de styles et bascules 0x80/0x81, cycles et références
  invalides, chiffrement, versions, champs, tableaux, sections, limites de forme.
- `crates/fmt-legacy/tests/reference.rs` : quatre vrais `.doc` produits par **LibreOffice** (filtre
  « MS Word 97 ») dans [`tests/doc-reference`](../tests/doc-reference/README.md) : texte accentué et
  Unicode, mise en forme de caractère, de paragraphe, titres et styles. Les attendus (`expected.json`)
  sont décrits une fois dans `tools/generate-doc-reference.py` ; `--check` redécode les fichiers
  stockés avec un lecteur Python indépendant (CFB, FIB, table des pièces, propriétés). **Microsoft
  Word n'était pas installé** : le corpus ne vient pas de Word.
- `crates/fmt-legacy/tests/robustness.rs` : 7 tests — chaque limite, tailles absurdes, 20 000
  paragraphes, 9 000 mutations à graine fixe sur trois fichiers fabriqués, 6 000 sur les quatre
  fichiers de référence (octets, mots de 32 bits à valeurs limites, remises à zéro, copies de
  fragments, troncatures, FIB seul, flux `Data`/propriétés), troncatures de tous les flux, 1 500
  flux aléatoires à FIB valide ; aucune panique ni boucle (plafond de 5 s par itération, et
  `resolved_paragraph` appelé sur chaque paragraphe accepté).
- `crates/fmt-legacy/tests/real_files.rs` : contrôle local optionnel (`ATELIER_DOC_SAMPLES`,
  répertoires ou fichiers séparés par `;`), aucun fichier copié. Résultat du 12 octobre 2026 sur les
  deux fichiers Word trouvés sur la machine du développeur (un `.doc` Word 2000 de 561 paragraphes
  et un `.dot` Word 2003) : importés sans erreur, longueur de texte = `ccpText` moins une marque
  par paragraphe moins les ancres supprimées.

## Ce qui reste

- **US-313** : listes (`PlcfLst`, `PlfLfo`, `sprmPIlvl/PIlfo`), tableaux (`sprmTDefTable`, TAP,
  fusions), sections (`PlcfSed`/`Sepx`), en-têtes et pieds (`ccpHdd`), notes, champs et liens,
  signets. L'architecture les accueille : `walk` isole la lecture du texte, les ancres (cellules,
  champs, sections) sont déjà reconnues et comptées, et `Stats` collecte les sprms qui manquent.
- **US-314** : images en ligne (`PICF`, `sprmCPicLocation`) et flottantes (OfficeArt) ; les ancres
  0x01 et 0x08 sont déjà repérées (avis `ObjectsDropped`).
- **US-315** : déchiffrement (RC4, XOR) ; la détection et l'erreur existent.
- `Prm0` (modification simple de la table des pièces, 128 valeurs `isprm` → sprm) n'est pas
  appliqué faute de table de référence vérifiée ; il est signalé par `SimplePrmIgnored`.
- Styles de tableau et de liste, `StdfPost2000`, texte bidirectionnel et d'Extrême-Orient
  (`rgftcBi`, `sprmCRgFtc1` n'est utilisé qu'en repli), langue des caractères.
- Raccordement à l'éditeur et écriture `.doc` : hors périmètre.

Références : [MS-DOC] Word Binary File Format (§2.5 FIB, §2.8.25 Clx, §2.9 structures, §2.6 sprms),
[MS-OLEPS] pour le jeu de propriétés, [MS-CFB] pour le conteneur.
