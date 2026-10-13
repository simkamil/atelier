<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-331 — Écrivain PDF

`atelier-fmt-pdf` rejoue des pages d'une `DisplayList` d'`atelier-render` en un fichier PDF 1.7 :
contenu de page, polices incorporées en sous-ensemble avec `ToUnicode`, images, liens, signets,
destinations nommées et dictionnaire d'information. Il n'utilise que `std` et les crates de l'espace
de travail (`atelier-compress` pour Flate, `atelier-font` pour l'analyse et le sous-ensemble,
`atelier-image` pour le décodage), `#![forbid(unsafe_code)]`, aucune API système : le crate compile
pour `wasm32-unknown-unknown`. Il n'y a ni horloge ni aléa : mêmes entrées, mêmes octets (l'`/ID`
du trailer est une empreinte de 128 bits du contenu écrit). L'interface d'export est l'objet d'US-332.

```rust
use atelier_fmt_pdf::{render_pdf, PdfFont, PdfOptions, PdfResourceProvider, SvgResourceProvider};

// Le fournisseur étend celui des backends SVG et raster (contours de glyphes, images)
// avec le programme de police d'un `font_id` et le texte d'un glyphe.
// impl PdfResourceProvider for Resources {
//     fn font(&self, font_id: u64) -> Option<PdfFont<'_>> { /* programme, face, taille */ }
//     fn glyph_text(&self, font_id: u64, glyph_id: u32) -> Option<&str> { /* "fi" pour une ligature */ }
// }
// let out = render_pdf(&pages, &resources, &PdfOptions::default())?;
// std::fs::write("out.pdf", out.bytes)?; // out.notices : observations non fatales
```

## API

- `render_pdf(pages, provider, options) -> Result<PdfOutput { bytes, notices }, PdfError>` : un PDF
  pour une liste de pages (une liste d'affichage par page). `PdfWriter<W: Write>` fait la même chose
  page par page (`new`, `add_page`, `page_count`, `finish`) vers n'importe quel `Write` ; les objets
  sont écrits au fil de l'eau, seul le contenu des pages utilisant une police CFF attend la fin (voir
  Polices).
- `PdfResourceProvider: SvgResourceProvider` ajoute `font(font_id) -> Option<PdfFont>` (`program_id`,
  fichier de police complet, `face_index`, taille d'em en unités de page) et `glyph_text(font_id,
  glyph_id)`. Tous les `font_id` de même `(program_id, face_index)` partagent **une** police
  incorporée (une taille = un `font_id`, un programme). `glyph_outline` n'est appelé que pour les
  polices dessinées en chemins.
- `PdfOptions` : `info` (`PdfInfo` : titre, auteur, sujet, mots-clés, créateur, producteur `Atelier`
  par défaut, dates fournies par l'appelant), `language` (`/Lang`), `named_destinations`, `outlines`
  (`OutlineItem` : titre, `Destination { page, top }`, enfants, ouvert), `infer_spaces` (vrai par
  défaut), `compress` (vrai par défaut), `limits` (`PdfLimits`).
- Unités : celles de la liste d'affichage, 1 unité = 1 point PDF, origine en haut à gauche ; le contenu
  de page commence par `1 0 0 -1 −x₀ (y₀+h) cm`, donc les coordonnées de la liste sont écrites telles
  quelles. `MediaBox` = `[0 0 largeur hauteur]`, jusqu'à 14 400 unités par côté.
- `PdfError` : `InvalidList`, `CoordinateOutOfRange`, `PageTooLarge`, `InvalidOptions`, `MissingFont`,
  `UnknownGlyph`, `MissingOutline`, `MissingImage`, `Image`, `Font`, `Subset`, `UnsupportedFont`,
  `InvalidDestination`, `LimitExceeded`, `EmptyDocument`, `Io`, `Internal`. Aucun document partiel
  n'est renvoyé. `PdfNotice` (non fatal) : `FontDrawnAsPaths`, `FontEmbeddedWhole`, `MissingGlyphText`,
  `UnresolvedLink`, `NoticesTruncated` (au plus 1 000 avis).

## Commande de la liste d'affichage → opérateurs PDF

| Commande | Écriture |
| --- | --- |
| `FillPath` | `m l c h` puis `f` (non-zéro) ou `f*` (pair-impair) ; une quadratique devient une cubique exacte. |
| `StrokePath` | `w J j M` (largeur, extrémité, jointure, limite de pointe, écrits seulement s'ils changent), chemin, `S`. |
| `DrawGlyphRun` | `BT /Fn taille Tf` puis, par ligne de base, `1 0 0 -1 x y Tm [<gid>±décalage …] TJ` ; `ET`. Voir Polices. |
| `DrawImage` | `q largeur 0 0 −hauteur x bas cm /Imn Do Q` : l'image est étirée sur `bounds`, comme les backends SVG et raster. |
| `PushClip` / `PopClip` | `x y l h re W n` dans un `q` ; la pile est écrite en rectangles successifs, indépendants des transformations (même règle que le raster). |
| `PushTransform` / `PopTransform` | `a b c d e f cm` (matrice composée comme `DisplayTransform::then`), dans un `q`. |
| `Link` | annotation `/Link` `/Border[0 0 0]` : `/A<</S/URI/URI(…)>>` (octets hors `!`–`~` en `%XX`) ou `/Dest(nom)` pour `#nom`. Un lien non résolu (`#inconnu`, cible vide) est un avis, sans annotation. |
| `Structure` | ignorée (PDF balisé hors périmètre). |

- Couleurs : RVB 8 bits ; `g`/`G` quand les trois canaux sont égaux, sinon `rg`/`RG`. Un niveau
  intermédiaire est écrit à un quart de pas au-dessus de sa valeur (`(v+0,25)/255`) : MuPDF tronque
  `valeur × 255`, Acrobat arrondit, les deux retrouvent le même niveau (test sur 256 niveaux).
- Transparence : un `ExtGState` partagé par niveau d'alpha (`/ca` et `/CA`), page marquée
  `/Group /S /Transparency` ; un alpha nul ne dessine rien.
- Nombres : 3 décimales (6 pour les matrices), jamais d'exposant, `-0` jamais écrit ; une valeur non
  finie ou supérieure à 10⁹ est `CoordinateOutOfRange`. Les clips sont rectangulaires, comme dans la liste.

## Polices et `ToUnicode`

- Chaque programme de police est une police `Type0` / `Identity-H` avec un `CIDFontType2` (TrueType,
  `FontFile2`, `/CIDToGIDMap /Identity`) ou un `CIDFontType0` (CFF, `FontFile3` `/CIDFontType0C`).
  Les glyphes de toutes les pages sont réunis puis le sous-ensemble est calculé **à la fin**
  (`atelier-font::subset`, étiquette `ABCDEF+Nom`) ; le sous-ensemble TrueType garde les numéros de
  glyphe d'origine (les glyphes inutilisés sont vides), le CFF est renuméroté : le contenu des pages CFF
  est donc corrigé (codes à 4 chiffres hexadécimaux patchés) quand les CID définitifs sont connus,
  d'où `max_pending_bytes`.
- `/W` donne l'avance de chaque glyphe utilisé (en 1/1000 d'em, depuis `hmtx`) ; les décalages `TJ`
  sont calculés avec ces mêmes avances, donc la position de chaque glyphe est celle de la liste
  d'affichage à 0,0001 em près (le rendu a été comparé au raster, voir Preuves).
- **`ToUnicode`** : une `CMap` `bfchar` par police, du CID vers le texte UTF-16BE que fournit
  `glyph_text` (`"fi"` pour une ligature, paires de substitution pour les caractères hors BMP). Un
  glyphe sans texte est un avis `MissingGlyphText` (non extractible). `infer_spaces` : si deux glyphes
  d'une ligne sont séparés de plus de 0,15 em sans glyphe d'espace, une espace est insérée (le glyphe
  d'espace de la police est ajouté au sous-ensemble) pour que l'extraction garde les mots.
- **Décision sur la limite « un glyphe, deux textes »** : l'entrée `ToUnicode` est par glyphe, le
  premier texte rencontré fait foi. Quand le fournisseur donne **un autre** texte pour le même glyphe du
  même programme (cas : le glyphe « ﬁ » est la ligature de « fi » sous un `font_id` et le caractère
  U+FB01 sous un autre `font_id` du même programme), le glyphe est écrit dans sa propre séquence
  `/Span <</ActualText …>> BDC … EMC` (dans le `BT`, séquence propre à ce glyphe). C'est la correction
  simple demandée : PyMuPDF et `pdftotext` rendent les deux textes, le lecteur de test aussi. Reste une
  limite d'API que le crate ne peut pas lever : `glyph_text(font_id, glyph_id)` ne renvoie qu'**un**
  texte par couple, donc deux occurrences d'un même glyphe sous le **même** `font_id` avec deux textes
  ne peuvent pas être distinguées (la liste d'affichage ne porte pas le texte source des glyphes). Le
  fournisseur de l'éditeur (US-332) doit soit n'avoir qu'un sens par glyphe, soit utiliser des
  `font_id` distincts ; une évolution propre est un texte de cluster dans `DrawGlyphRun`
  (`atelier-render`, hors périmètre). L'avis `ConflictingGlyphText` de la première version est supprimé.
- **Droits d'incorporation** (`OS/2.fsType`, voir [US-330](us-330-font-subsetting.md)) : installable,
  aperçu/impression et modifiable → sous-ensemble ; « pas de sous-ensemble » (bit 8) → police entière
  (`FontFile2` ou `FontFile3 /OpenType` identique au fichier, avis `FontEmbeddedWhole`) ; collection
  qui interdit le sous-ensemble → chemins (`CollectionNotEmbeddableWhole`) ; intégration restreinte
  (bit 1) ou bitmaps seuls (bit 9) → **chemins** : chaque glyphe est un `f` (contour fourni par
  `glyph_outline`, `MissingOutline` s'il manque), aucune police dans le fichier, texte non
  sélectionnable, avis `FontDrawnAsPaths` avec la raison.
- Refus : CFF2, polices sans contours TrueType ni CFF (`UnsupportedFont`), fichier non analysable
  (`Font`), glyphe hors de la police (`UnknownGlyph`), texte de glyphe de plus de 32 caractères.

## Images

- **JPEG** : copié tel quel (`/Filter /DCTDecode`, `BitsPerComponent 8`, `Interpolate true`) après un
  parcours des marqueurs (SOF, EOI, marqueur Adobe `APP14`). Espace colorimétrique selon les
  composantes : 1 → `DeviceGray`, 3 → `DeviceRGB`, 4 → `DeviceCMYK` avec `/Decode [1 0 1 0 1 0 1 0]`
  quand un marqueur Adobe est présent (CMYK et YCCK d'Adobe sont inversés ; sans marqueur, non inversé
  — même convention que le décodeur `atelier-image`, donc que le raster). Baseline, étendu et
  progressif passent. Sont décodés puis écrits comme un PNG : précision autre que 8 bits, JPEG codé
  arithmétiquement ou sans perte, trois composantes d'identifiants `R G B` sans marqueur Adobe (ce
  n'est pas du YCbCr), transformation Adobe incompatible. L'orientation EXIF n'est appliquée ni par le
  décodeur ni par les lecteurs PDF.
- **PNG, GIF, BMP** : décodés par `atelier-image`, écrits en Flate, 8 bits, `DeviceGray` si les trois
  canaux sont égaux partout sinon `DeviceRGB` ; un alpha non opaque donne un second objet image
  `DeviceGray` référencé par `/SMask` (pas de `/Matte` : les couleurs ne sont pas prémultipliées).
- Une image utilisée plusieurs fois (même `image_id`, sur une ou plusieurs pages) est écrite une seule
  fois. Le type MIME doit correspondre aux octets, sinon `Image` ; budgets d'octets et de pixels
  contrôlés avant décodage (`max_image_bytes`, `max_image_pixels`).

## Liens, signets, métadonnées

- Liens externes : `/URI`. Liens internes : cible `#nom`, résolue par `named_destinations`, écrite en
  `/Dest (nom)` et arbre de noms `/Names /Dests` (trié) ; destinations `[page /XYZ null y null]` avec
  `y = hauteur − top` (ou `null` sans position).
- Signets : arbre `/Outlines` à `/First /Last /Prev /Next /Parent /Count` (négatif pour un nœud
  fermé), `/PageMode /UseOutlines` ; profondeur et nombre bornés.
- `/Info` : chaînes en ASCII échappé (`( ) \`) ou UTF-16BE avec BOM ; dates `D:AAAAMMJJhhmmss` avec
  fuseau ; la validité d'une date n'est contrôlée que champ par champ (le 31 février est accepté).
  Arbre de pages équilibré (32 enfants au plus), `/Count` exact, `/Lang` si fourni.

## Limites et budgets

`PdfLimits` (défauts) : 100 000 pages, 1 Gio de sortie, 4 000 000 objets, 256 Mio de contenu par
page, 512 Mio de contenu en attente (pages à police CFF), images de 64 Mio et 64 Mpx, 4 096 polices,
100 000 signets sur 32 niveaux. Chaque budget est contrôlé avant l'allocation ou l'écriture
correspondante (`LimitExceeded`).

Limites de fonctionnement :

- Pas de PDF/A, PDF balisé, chiffrement, flux xref ou d'objets, formulaires (hors périmètre) ; pas de
  lecture de PDF dans le crate (le lecteur de test vit dans `tests/common/reader.rs`).
- Pas de pointillés (la liste n'en a pas), pas de dégradés, pas d'opacité d'image (comme le raster),
  clips rectangulaires. Un `Link` est écrit à ses coordonnées de page, sans appliquer les transformations
  actives (la liste les donne déjà transformées).
- Le sous-ensemble TrueType garde la table `loca` de la police entière (quelques Kio pour Carlito, 2 782
  glyphes) ; un CFF à nom de glyphe est écrit en `CIDFontType0C` avec les numéros de glyphe pour CID,
  ce que les lecteurs essayés acceptent.
- Un lecteur arrondit les couleurs et l'anticrénelage à sa façon : le rendu n'est pas identique au
  bit près à celui du raster (voir ci-dessous). La conversion `DeviceCMYK` → écran est propre à chaque
  lecteur (MuPDF désature par rapport à la formule simple du décodeur).

## Preuves

**Tests Rust** (`cargo test -p atelier-fmt-pdf --locked --offline` : 40 tests ; aucun ne dépend d'un
outil externe). Un lecteur PDF indépendant du rédacteur (`tests/common/reader.rs` : xref, références,
longueurs de flux, arbre de pages, Flate, `ToUnicode`, `ActualText`, extraction de texte) relit tout.

- `tests/document.rs` (11) : page type (structure, polices, texte sélectionnable, formes, images, clip,
  transparence, rotation), union des glyphes d'une police partagée entre pages, arbre de pages équilibré,
  déterminisme, `PdfWriter` = `render_pdf`, flux non compressés, métadonnées, liens, signets,
  caractères hors BMP et ligatures, espaces inférées, avis, police CFF (CID définitifs).
- `tests/features.rs` (28) : droits d'incorporation (`fsType` modifié dans une copie de Carlito :
  restreinte et bitmaps → chemins et avis, sans police ni `TJ` ; « pas de sous-ensemble » → police
  identique octet pour octet, identifiants de glyphe d'origine, texte extractible ; droits qui
  permettent le sous-ensemble → sans avis) ; images (JPEG baseline, progressif, gris, CMYK et YCCK
  Adobe avec `/Decode`, CMYK sans marqueur, JPEG non transmissible décodé, PNG alpha → `SMask` vérifié
  pixel par pixel, PNG opaque et gris, image réutilisée écrite une fois, images invalides) ; un glyphe
  à deux textes → `ActualText` ; chaque erreur et chaque budget (piles de clips et de transformations
  déséquilibrées, valeur non finie, valeurs hors plage, page trop grande, glyphe inconnu, police ou
  image absente ou invalide, pages, octets, objets, contenu, polices, pixels, signets, options,
  destination hors document, sortie en erreur à trois moments) ; échappement des chaînes, `/ID`
  propre à chaque document ; niveaux de couleur et d'alpha sous lecture par troncature et par arrondi.
- `tests/campaign.rs` (1) : 200 documents de listes aléatoires ou mutées à graine fixe (xorshift maison,
  comme `crates/raster/tests/display_list.rs`), quatre polices dont une restreinte, six images dont une
  invalide, clips, transformations, liens, mutations (commande supprimée, dupliquée, permutée, alpha
  `NaN`, coordonnée à 10¹²) : aucune panique ; chaque PDF produit est relu sans erreur (xref, pages,
  `q/Q`, `BT/ET`, `BDC/EMC` équilibrés, ressources déclarées et utilisées), rendu deux fois avec les
  mêmes octets ; chaque erreur est typée et reproductible (`InvalidList`, `CoordinateOutOfRange`,
  `UnknownGlyph`, `Image`, `MissingImage` dans la graine actuelle).

**Défauts corrigés dans la première version** : alerte clippy dans les tests ; `ToUnicode`
(un glyphe, deux textes) ; JPEG CMYK décodé inutilement (désormais copié, avec `/Decode`) ; niveaux de
couleur tronqués par MuPDF (le fond `0,96 0,96 0,94` était relu un niveau trop bas sur tout le
fond). Aucun défaut de xref, de longueur de flux, de repère de coordonnées, d'échappement ni de `/W` n'a
été trouvé : le rendu comparé au raster est aligné au pixel, les chaînes et noms sont relus.

**Oracles externes** (`tools/pdf-oracle.py`, local, bibliothèque standard + ce qui est présent :
PyMuPDF 1.28.2, poppler `pdftotext`, fontTools 4.63, LibreOffice ; rien n'est installé ; `cargo run -p
atelier-fmt-pdf --example oracle_sample -- <dossier> [police.otf]` écrit les PDF d'essai et le rendu du
raster à 96 dpi). Document type : 5 polices embarquées (Carlito ×3, Caladea, Liberation Mono), accents,
ligatures, formes, PNG alpha, JPEG, clip, transparence, rotation de 15°.

- Texte : PyMuPDF et `pdftotext` rendent les 241 caractères du texte source sans perte ni ajout ;
  PyMuPDF dans un autre ordre de lecture (mêmes 45 mots), `pdftotext` coupe « quinze » en « q » et
  « uinze » (texte tourné de 15°, analyse de mise en page). Document de 3 pages avec `ActualText`
  (« ﬁn » U+FB01 et « fi » ligature) : mots et ordre identiques dans les deux extracteurs, 3 signets, 6
  liens (URI et nommés), aucun avertissement MuPDF ni message poppler.
- Rendu comparé au raster (même liste, 794 × 1 123 px à 96 dpi, `tools/image-diff`) : à tolérance
  nulle 8,1 % des pixels diffèrent (bords anticrénelés des deux moteurs), écart moyen par canal
  0,47 / 0,44 / 0,42 ; au-delà de 32 niveaux 0,45 % (4 010 pixels), au-delà de 128 niveaux 0,043 % (381
  pixels), tous sur le bord d'un pixel de l'image JPEG et du rectangle de clip (placement
  fractionnaire). L'image de différence ne montre aucun décalage ni élément manquant ; l'essai de
  alignement (décalage de −1 à +1 px) donne le meilleur accord à (0, 0) pour le texte, les formes et le
  clip. **Tolérance proposée : canal 32, 0,6 % des pixels** (mesuré 0,45 %) ; un décalage d'un pixel
  du rendu externe donne 1,69 % et est refusé. Le texte MuPDF est un peu plus gras (écart moyen de 10
  niveaux dans le titre en gras, 3 dans le corps) : rendu différent de la même forme, pas de décalage.
  Les six sortes d'image (JPEG baseline, gris, progressif, CMYK, YCCK, PNG alpha) : écart moyen 1,0 à
  1,5 niveau, sauf le CMYK (18,9 : la conversion CMYK → RVB de MuPDF désature, même orientation et
  mêmes motifs, donc `/Decode` correct).
- fontTools relit chaque police incorporée (TrueType : `glyf` décompilés ; CFF nu : charstrings
  décompilées) sans erreur.
- LibreOffice (installation locale) importe les PDF en Draw sans erreur (`draw_pdf_import`) et exporte un PNG dont la
  page est conforme (textes, formes, transparence, images, clip, rotation).
- Firefox (pdf.js, capture en mode sans interface du PDF type) : la page s'affiche entièrement.
  **Non constaté** : sélection, copie et recherche de texte dans un navigateur, ouverture dans Acrobat
  Reader (absent) et dans Edge (la capture sans interface est blanche) : voir la procédure ci-dessous.

**Mesure** (`cargo run --release -p atelier-fmt-pdf --example bench_pages -- 100`) : 100 pages A4 de
46 lignes de texte Carlito (363 832 glyphes), de la liste d'affichage au fichier : 440 à 510 ms (critère
d'US-332 : moins de 3 s), 253 656 octets (2,5 Kio par page ; texte répétitif). Machine de développement
Windows, hors mise en page.

## À constater à la main (deux minutes)

1. Générer : `cargo run -p atelier-fmt-pdf --example oracle_sample -- out` (écrit `out/sample.pdf`,
   `out/multi.pdf`).
2. Ouvrir `out/sample.pdf` dans Acrobat Reader, Edge et Firefox : aucune erreur, page complète (texte
   noir, rectangles rouge et bleu translucides, étoile, courbe grise, deux images, forme verte rognée,
   texte incliné).
3. Dans chaque lecteur : sélectionner la ligne « Voilà un texte accentué… », copier, coller dans un
   éditeur : le texte doit être identique (accents, « Œuvre »). Rechercher « déjà » et « office » :
   une occurrence chacune. Dans `out/multi.pdf` : rechercher « ﬁn » (page 2) et « final » (page 3) ;
   cliquer le lien interne de chaque page (zone de la ligne de titre) : saut à la page 2 ; le panneau des signets montre
   trois entrées.
