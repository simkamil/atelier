# US-330 — Sous-ensemble de polices

Le module `atelier_font::subset` réduit une police TrueType (`glyf`) ou CFF (nue ou dans un OpenType `OTTO`) à un ensemble de glyphes et produit un programme de police autonome, prêt à être incorporé par l’écrivain PDF (US-331), avec les informations du descripteur de police. Aucune dépendance externe, `#![forbid(unsafe_code)]`, aucune panique : toute erreur est un `SubsetError` typé, les entrées sont bornées avant allocation et la sortie est déterministe (mêmes entrées, mêmes octets).

## API

- `subset_font(octets, indice_de_face, &[u16])` : lit une police sfnt ou une collection TTC (la face choisie), puis appelle `subset_face`.
- `subset_face(&FontFace, &[u16])` : choisit le chemin selon les tables (`CFF ` → CFF, sinon `glyf` + `loca` → TrueType ; `CFF2` → `Unsupported`).
- `subset_cff(table_cff, &[u16])` : sous-ensemble d’une table CFF 1 nue (pas de `OS/2`, donc toujours incorporable ; avances lues dans les charstrings, 1000 unités par em).
- `embedding_rights(Option<u16>)` : interprétation de `OS/2.fsType`, utilisable avant de choisir une police.

Le glyphe 0 (`.notdef`) est toujours conservé ; les identifiants peuvent être dans n’importe quel ordre, avec doublons ; une liste vide donne `.notdef` seul. Un identifiant hors de `numGlyphs` donne `GlyphOutOfRange(id)`.

Résultat `SubsetFont` :

| Champ | Contenu |
|---|---|
| `kind` | `ProgramKind::TrueType` (octets sfnt, `FontFile2`, `CIDFontType2`) ou `ProgramKind::Cff` (table CFF nue, `FontFile3`, `/Subtype /CIDFontType0C`) |
| `data` | le programme de police |
| `glyphs` | `Vec<SubsetGlyph>` triée par `old_gid` : `old_gid`, `new_gid`, `cid` (valeur à écrire dans le flux de contenu), `advance` (unités de police, `f64`) |
| `metrics` | `units_per_em`, `ascent`, `descent`, `line_gap`, `bbox` (de la police entière), `italic_angle`, `cap_height`, `x_height`, `weight_class`, `fixed_pitch`, `serif`, `italic`, `bold` ; méthodes `descriptor_flags()` (Symbolic + FixedPitch, Serif, Italic) et `stem_v_estimate()` |
| `post_script_name` | nom `name` ID 6 (ou nom du Name INDEX CFF), limité aux caractères valides dans un nom PDF |
| `rights` | permission (`Installable`, `Editable`, `PreviewPrint`) et `subsetting_allowed` |
| `cid_keyed` | CFF : le programme est CID-keyed |

`SubsetFont::pdf_subset_tag()` donne six lettres majuscules (FNV-1a des octets du programme) pour le préfixe `ABCDEF+` de `BaseFont`. `SubsetFont::glyph(old_gid)` retrouve un glyphe conservé. L’écrivain PDF construit lui-même `ToUnicode` et le tableau `/W` (avances : `advance * 1000 / units_per_em`).

## Stratégie d’identifiants

**TrueType : conservation des identifiants d’origine.** Les glyphes absents deviennent vides (`loca[i] == loca[i+1]`), `numGlyphs` est ramené à `dernier glyphe conservé + 1`. Raisons : les références des composites restent valides sans réécriture, `CIDToGIDMap` est `/Identity` (aucune table à écrire), `ToUnicode` et `/W` sont indexés par le glyphe de la police source, que l’écrivain connaît déjà après mise en forme, et le test « même contour, même avance » est direct. Coût : `loca` (2 ou 4 octets) et `hmtx` (4 octets) par glyphe jusqu’au dernier conservé ; pour Carlito Regular et du texte ASCII (dernier glyphe 2782) cela fait 16,7 Ko sur 35,8 Ko. Une renumérotation compacte ferait disparaître ce coût mais impose la table `CIDToGIDMap` et la réécriture des composites ; elle reste possible plus tard sans changer l’API (`new_gid` et `cid` existent déjà).

**CFF : renumérotation compacte.** Le CharStrings INDEX n’a aucune table indexée par glyphe, donc la renumérotation est gratuite (`new_gid` = rang parmi les glyphes conservés, `.notdef` en tête). Le `cid` à écrire dans le PDF est : `new_gid` pour une police CFF à noms (pour un `CIDFontType0`, un CFF non CID-keyed adresse les glyphes par GID), et le CID d’origine du charset pour une police CID-keyed (le charset du sous-ensemble conserve les CID).

## Tables produites (TrueType)

`glyf`, `loca` (format court si possible, `indexToLocFormat` recalculé), `head` (copie, `checkSumAdjustment` recalculé, boîte englobante de la police source), `hhea` (`numberOfHMetrics` = nouveau `numGlyphs`), `hmtx` (avance et approche gauche d’origine pour les glyphes conservés, 0 pour les vides), `maxp` (copie, nouveau `numGlyphs`), `cvt `, `fpgm`, `prep` (recopiées si présentes), `OS/2` (copie, `fsType` conservé), `post` (format 3, en-tête seul), `name` (enregistrements 0 à 6 de la source) et `cmap`. Le `cmap` est un format 4 minimal sans aucune association : il n’est pas lu pour un PDF composite, mais `FontFace::parse` (et plusieurs outils) l’exige pour relire le sous-ensemble. `GSUB`, `GPOS`, `kern`, `gasp`, `hdmx`, `VDMX`, tables variables et bitmaps sont omis ; une police variable est sous-ensemblée à l’instance par défaut.

Répertoire trié par étiquette, `searchRange`/`entrySelector`/`rangeShift` calculés, tables alignées sur 4 octets (remplissage nul), somme de contrôle par table (celle de `head` calculée avec `checkSumAdjustment = 0`) et `0xB1B0AFBA` pour la somme de tout le fichier. Chaque glyphe est copié tel quel (instructions de hinting comprises) à un décalage multiple de 4. La fermeture sur les composites est une exploration en profondeur bornée à 32 niveaux, 4096 composants par glyphe : un composite cyclique donne `InvalidData("cyclic composite glyph")`, un composant hors bornes `InvalidData("component glyph id")`. Seuls les glyphes demandés (et leurs composants) sont examinés.

## Tables produites (CFF)

En-tête, Name INDEX (nom d’origine), Top DICT (entrées d’origine, `ROS` en premier pour une police CID, `charset`, `FDSelect`, `FDArray`, `CharStrings` et `Private` recalculés en entiers de 5 octets), String INDEX réduit aux chaînes encore référencées (SID renumérotés dans le Top DICT, les FD DICT et le charset), Global Subr INDEX, charset (format 0 ou 2, le plus court), FDSelect (format 0 ou 3, le plus court), CharStrings (octets d’origine des glyphes conservés), FDArray réduit aux Font DICT utilisés (indices de FDSelect renumérotés), Private DICT (opérandes d’origine, `Subrs` recalculé) et sous-routines locales. L’opérateur `Encoding` est abandonné (inutile pour un CID PDF). Les glyphes `seac` (`endchar` à quatre arguments) font ajouter leurs composants (code StandardEncoding → SID → charset) à l’ensemble, transitivement.

**Élagage des sous-routines** : les charstrings conservés sont exécutés (arithmétique et pile comprises, masques de hints comptés) pour relever les `callsubr`/`callgsubr`. Une sous-routine jamais appelée garde sa place mais son corps devient un seul `return` ; les sous-routines finales inutilisées sont supprimées tant que le biais d’appel (107, 1131, 32768) ne change pas ; une table sans appel disparaît. Le travail est borné (32 millions d’opérateurs au total, pile de 48, profondeur 10) : au-delà, `LimitExceeded`. Le résultat est relu par `CffFont::parse` avant d’être rendu (contrôle interne). Pas de FontMatrix modifiée : les matrices sont recopiées avec les DICT.

## Droits d’incorporation

`embedding_rights(fsType)` (`None` = pas de table `OS/2` = incorporable) applique la table OS/2 d’OpenType :

- bit 9 (`0x200`, bitmaps seulement) : `SubsetError::BitmapOnly` (les contours ne peuvent pas être incorporés) ;
- bits 1 à 3 (`0x2`, `0x4`, `0x8`) : exclusifs en théorie ; si plusieurs sont posés, le moins restrictif l’emporte. `0x8` → `Editable`, `0x4` → `PreviewPrint` (le document devrait être traité comme non modifiable), `0x2` seul → `SubsetError::EmbeddingRestricted` (l’appelant bascule vers un repli avec avertissement), aucun → `Installable` ;
- bit 8 (`0x100`, sous-ensemble interdit) : `subsetting_allowed = false` et `subset_font`/`subset_face` renvoient `SubsetError::SubsettingForbidden`. Choix documenté : le sous-ensemble n’est pas produit, c’est à l’appelant d’incorporer la police source entière, inchangée (identifiants identiques, `CIDToGIDMap` `/Identity`), après avoir appelé `embedding_rights` ; les polices CFF dans un OpenType s’incorporent en `FontFile3 /OpenType` et un TTC doit être extrait par l’appelant ;
- bits réservés (4 à 7, 10 à 15) ignorés.

## Limites

- Pas de CFF2, de glyphes Type 1, de polices sans `glyf` ni `CFF `, ni de bitmaps.
- Les tables de mise en forme (`GSUB`, `GPOS`, `kern`, `cmap` utile) sont omises : le texte doit être mis en forme avant le sous-ensemble.
- La boîte englobante, `maxp`, `hhea` (extrêmes) gardent les valeurs de la police entière.
- Pas de renumérotation TrueType (voir le coût plus haut). Pas de fusion de sous-ensembles ni de sous-ensemble incrémental.
- Le moteur WASM grossit d’environ 40 Ko (opt-level `z`, LTO) dès que `subset` y est lié (mesure sur une bibliothèque de test appelant `subset_font` contre `FontFace::parse` seul : 109 190 → 150 205 octets). `core-wasm` n’appelle pas encore le module : son artefact ne change pas.

## Preuves

Tests (`cargo test -p atelier-font`) :

- `tests/subset.rs` (10 tests) : sur les 20 polices embarquées (Carlito, Caladea, Liberation), ensembles ASCII et lettres accentuées composites ; relecture par `FontFace`, contour (`outline`) et métrique d’avance identiques pour chaque glyphe conservé, glyphes abandonnés vides, tables de programme recopiées, boîte globale identique, structure de l’sfnt (tri, alignement, remplissage, somme de contrôle de chaque table, `checkSumAdjustment`), taille de l’ASCII < 1/3 de l’original ; glyphe unique, ensemble vide, doublons, toutes les glyphes (3 polices), déterminisme ; les 1024 valeurs de `fsType` de 0 à `0x3FF` sur une police modifiée ; composites cycliques et hors bornes ; campagne à graine fixe (1500 mutations × 4 fixtures, 400 mutations de tables d’une vraie police, troncatures, identifiants aléatoires) sans panique.
- `tests/subset_cff.rs` (13 tests) avec le constructeur CFF de `tests/cff_outlines.rs` copié dans `tests/common/cff_builder.rs` (les tests existants ne sont pas modifiés) : police à noms avec sous-routines, noms de glyphes et String INDEX réduit, élagage et appels imbriqués, biais conservé, masques de hints, `seac` (composants ajoutés, composant manquant), police CID avec FDSelect formats 0 et 3 et FD inutilisés retirés, OpenType/CFF (avances `hmtx`, métriques `head`, `fsType`), l’OTF synthétique FontTools de `tests/font-cff-reference/` (seac imbriqué), police de 3000 glyphes (INDEX sur 16 bits, charset par plages), charstrings hostiles, campagne à graine fixe (4 polices × 4000 mutations, enveloppées aussi dans un OpenType).

Contrôle local hors dépôt avec FontTools 4.63 (script et petit exécutable dans le répertoire de travail temporaire, rien de copié dans le dépôt) : sous-ensembles TrueType ouverts avec `TTFont(checkChecksums=2)` (aucune erreur de somme), glyphes octet pour octet identiques (`glyf`), `hmtx`, boîte `head` et `fsType` identiques, recompilation complète sans erreur, sur les 239 `.ttf` de `C:\Windows\Fonts` (ensembles ASCII, accentué, épars) et les polices embarquées (plus le mode « tous les glyphes » sur trois d’entre elles) ; sous-ensembles CFF des 35 `.otf` de `C:\Windows\Fonts` (ASCII, accentué, épars, tous les glyphes) décompilés par `CFFFontSet` : `FontBBox` identique, contours (`RecordingPen`), largeurs et noms de glyphes identiques pour chaque glyphe conservé. Ces 35 polices n’ont ni police CID ni `seac` : ces deux chemins ne sont vérifiés que sur fixtures synthétiques.

Tailles mesurées (octets) : Carlito Regular 628 032 → 35 848 (ASCII, 106 glyphes), 4 280 (un glyphe, 2 glyphes conservés) ; Liberation Serif Italic 375 632 → 26 072 ; Caladea Bold 84 492 → 23 604 ; CFF : FrankRuehlCLM-Medium (OTF de 151 340) → 15 887 (ASCII), 3 177 (10 glyphes).
