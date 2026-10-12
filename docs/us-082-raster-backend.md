<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-082 — Backend raster

`atelier_raster::display_list` rejoue une page d'une `DisplayList` d'`atelier-render`
sur le rasteriseur d'[US-061](us-061-software-rasterizer.md) et renvoie une image RGBA8
droite (`atelier_image::Image`) ou, par commodité, un PNG. Le module vit dans `atelier-raster`,
qui dépend désormais d'`atelier-render` ; `atelier-render` (donc le moteur WASM de l'éditeur,
qui ne dépend que de lui) ne dépend ni du raster ni du décodage d'images. Aucun cycle :
`geom ← render ← raster → image`. Le module n'accède à aucun fichier, aucune police système,
aucun réseau.

```rust
use atelier_raster::display_list::{
    render_image, render_png, RenderOptions, Resolution, ResourceProvider, SvgImage,
};

struct Resources;
impl ResourceProvider for Resources {
    fn glyph_outline(&self, _font_id: u64, _glyph_id: u32)
        -> Option<&atelier_render::DisplayPath> { None }
    fn image(&self, _image_id: u64) -> Option<SvgImage<'_>> { None }
}

// let options = RenderOptions { resolution: Resolution::Dpi(300.0), ..RenderOptions::default() };
// let pixels = render_image(&display_list, &Resources, &options)?;
// let png = render_png(&display_list, &Resources, &options)?;
```

## API

- `ResourceProvider` est exactement `atelier_render::SvgResourceProvider` (même trait, même
  `SvgImage { mime_type, bytes }`) : un seul fournisseur sert les deux backends. Les contours
  de glyphes sont des `DisplayPath` en unités de page relatifs à l'origine du glyphe ; les images
  sont fournies encodées (`image/png`, `image/jpeg`, `image/gif`, `image/bmp`) et décodées par
  `atelier-image`, une seule fois par identifiant. Le backend ne dépend pas du crate de polices.
- `RenderOptions` : `resolution` (`Scale(pixels par unité de page)` ou `Dpi(points par pouce)`,
  72 unités par pouce ; 96 dpi par défaut), `background` (RGBA droit ; blanc par défaut,
  `[0; 4]` pour un fond transparent), `region` (`PixelRegion` en pixels de l'image pleine
  résolution ; toute la page par défaut) et `limits` (`RenderLimits`).
- Taille : chaque côté vaut `ceil(taille de page × échelle)`, au moins 1 pixel
  (`page_pixel_size`). L'origine de la page est celle de `PageInfo`. À 96 dpi, une page de
  200 × 100 donne 267 × 134 ; la frange fractionnaire reste de la couleur de fond.
- Une sous-zone est **identique au bit près** au recadrage du rendu complet : les tuiles sont
  alignées sur la grille de l'image pleine page, quelle que soit la zone demandée. Une page
  gigantesque se rend donc par tuiles sans jamais allouer plus que la zone.
- `render_png` encode avec l'encodeur PNG d'`atelier-image` : mêmes entrées, mêmes octets.
- `RenderError` : `InvalidList`, `InvalidOptions`, `MissingResource`, `InvalidMime`,
  `LimitExceeded`, `Image(ImageError)`, `Raster(RasterError)`. Aucune image partielle.

## Commande de la liste d'affichage → rendu

| Commande | Rendu |
| --- | --- |
| `FillPath` | `Rasterizer::fill_path`, règle non-zéro ou pair-impair, couleur RGBA arrondie à l'octet le plus proche. |
| `StrokePath` | `stroke_path` avec largeur, extrémités, jointures et limite de pointe ; largeur mise à l'échelle (voir limites). |
| `DrawGlyphRun` | un remplissage non-zéro par glyphe : contour du fournisseur, translaté à la position du glyphe. L'avance n'est pas utilisée (comme le SVG). Un contour vide (espace) ne peint rien. |
| `DrawImage` | `draw_image` : l'image est étirée sur `bounds` (`preserveAspectRatio="none"`), interpolation bilinéaire du rasteriseur. |
| `PushClip` / `PopClip` | rectangles intersectés en coordonnées de page, indépendants de la pile de transformations, avec la même règle d'intersection vide que le SVG ; poussés dans la surface comme un clip rectangulaire (couverture à 8 × 8 échantillons, les bords fractionnaires ne sont pas assombris deux fois). |
| `PushTransform` / `PopTransform` | composition identique à `DisplayTransform::then` (transformation courante, puis la nouvelle), appliquée aux points avant rasterisation. |
| `Link`, `Structure` | ignorés : aucun effet visuel. |

Comme le SVG, le préambule résout et contrôle toutes les ressources référencées, y compris
celles d'un dessin masqué par un clip vide : une ressource manquante est toujours une erreur.

## Principe de rendu et déterminisme

La liste est compilée une fois (matrices composées, clips intersectés, contours et images résolus),
puis la page est peinte par tuiles de 64 × 64 pixels, chacune sur une petite surface du
rasteriseur, dans l'ordre de peinture. Le rasteriseur alloue des masques de la taille de sa
surface à chaque opération ; avec des tuiles, le coût d'un glyphe suit sa taille et non celle de
la page. Les tuiles sont décalées d'un nombre entier de pixels, ce qui garde la grille
d'échantillons alignée ; un test compare le rendu par tuiles à un rendu direct sur une seule
surface (pixels identiques). Tout est mono-thread et n'emploie que l'arithmétique IEEE
(+, −, ×, ÷, racine), sans temps ni aléa : même liste, mêmes options, mêmes octets, y compris
sur la cible WASM, **sauf** pour une image tournée ou cisaillée : l'API publique d'`AffineTransform`
n'offre que translation, échelle et rotation, la matrice générale est donc décomposée
(`R(φ)·diag(s₁, s₂)·R(θ)`) à l'aide de `rotation`, dont les fonctions trigonométriques de la
bibliothèque standard peuvent différer d'un bit entre plateformes. Les images alignées sur les axes,
les chemins, les contours et les glyphes ne sont pas concernés.

## Limites et budgets

`RenderLimits` (valeurs par défaut) : côté de page 2²⁴ pixels, 16 777 216 pixels en sortie,
1 000 000 opérations (glyphes compris), 16 777 216 affectations opération/tuile, 2 · 10⁹ unités
de travail estimées, 65 536 arêtes par opération et par tuile, images de 8 Mio encodés,
16 777 216 pixels et 128 Mio décodés au total. Chaque budget est contrôlé avant l'allocation
correspondante (taille de sortie avant le tampon, opérations et tuiles avant la distribution) ;
une page gigantesque à haute résolution renvoie `LimitExceeded` sans allocation. Il n'y a pas de
garantie de durée : le travail estimé borne le coût, pas l'horloge.

Approximations et refus documentés :

- Un contour est élargi en pixels. Sous une transformation qui n'est pas une similitude
  (échelle anisotrope, cisaillement), l'épaisseur utilise l'échelle moyenne `√|det|` au lieu de
  déformer le trait comme le SVG.
- Une transformation singulière ou d'aire relative inférieure à 10⁻⁹ ne peint rien (remplissages
  ne couvrant aucune aire, images dégénérées), sans erreur. Un rectangle d'image de largeur ou
  hauteur nulle ne peint rien.
- Les coordonnées d'une forme peinte au-delà de ±10⁹ pixels sont refusées par le rasteriseur
  (`Raster(InvalidInput)`) ; une forme entièrement hors de la zone est écartée sans erreur.
  Une forme qui chevauche la zone avec de telles coordonnées est donc une erreur, pas un rendu
  tronqué.
- `DrawImage` n'a pas de champ d'opacité dans la liste d'affichage v1 : aucune opacité d'image
  n'est appliquée. L'alpha propre de l'image est respecté.
- Les glyphes sont composés un par un, comme les `<use>` du SVG ; la composition est celle du
  rasteriseur (sRGB, source-over, sans gamma ni profil ICC), pas celle d'un navigateur.

## Preuves

`crates/raster/tests/display_list.rs` (30 tests dont un chronométrage `#[ignore]`) :

- dimensions proportionnelles à la résolution (échelles et dpi), origine de page, fond blanc,
  transparent ou coloré ;
- pixels attendus à des positions connues (rectangles à deux résolutions, bord fractionnaire,
  alpha, règles de remplissage, contours et extrémités, rotation d'un quart de tour) ;
- zones hors clip strictement inchangées, clip fractionnaire sans double assombrissement, clips
  imbriqués et disjoints, piles de clips et de transformations croisées ;
- images placées, étirées, rognées, tournées, transformées par une matrice générale ;
- couverture d'encre du texte (police Carlito embarquée, via `atelier-font` en dépendance de test)
  cohérente à 3 % près entre 1× et 3×, boîtes d'encre concordantes ;
- boîtes englobantes de l'encre identiques (à un pixel près) à celles que décrit le SVG produit par
  `render_svg` pour les mêmes listes (remplissage, glyphes, clip, image, transformation) : le test
  relit le SVG avec `atelier-xml` ;
- sous-zone égale au recadrage, rendu par tuiles égal à un rendu direct du rasteriseur,
  déterminisme et aller-retour PNG ;
- erreurs typées (ressources manquantes ou invalides, MIME, options, budgets, listes
  incohérentes, coordonnées hors plage) ;
- trois images de référence PNG de 64 × 48 dans `crates/raster/tests/reference/`, avec la somme de
  contrôle FNV-1a 64 de chaque fichier dans le test ; les octets PNG générés et
  les pixels décodés sont comparés. Ces images ont été produites par le backend
  et constituent des références de régression, sans oracle visuel indépendant
  (régénération :
  `ATELIER_UPDATE_REFERENCE=1 cargo test -p atelier-raster --test display_list reference_images`) ;
- campagne de 400 listes aléatoires ou mutées, à graine fixe (générateur xorshift maison) :
  aucune panique, rendus déterministes, erreurs typées variées.

Mesure indicative historique de la livraison extérieure, non rejouée lors de
l'intégration (page Lettre 612 × 792, 48 lignes de texte Carlito 11 pt, 3 008 glyphes,
`--release`, médiane de 5 rendus, machine de développement Windows) : 92 ms à 96 dpi
(816 × 1 056), 238 ms à 300 dpi (2 550 × 3 300) ; avec encodage PNG, 172 ms et 810 ms.
Commande : `cargo test -p atelier-raster --release --test display_list timing -- --ignored --nocapture`.

## Besoins constatés (hors périmètre)

- `atelier-geom` : un accesseur de segments de `Path` (conversion police → `DisplayPath` sans
  aplatir) et un constructeur général d'`AffineTransform::new(a, b, c, d, e, f)` supprimeraient la
  décomposition trigonométrique des images tournées.
- `atelier-raster` : masques de couverture bornés à la zone de l'opération (le rendu par tuiles
  contourne le coût en surface pleine page), contours anisotropes, clip par chemin.
- `atelier-render` : opacité d'image dans `DrawImage`.

Validation d’intégration du 2026-10-12 : [reçu complet](../_bmad-output/implementation-artifacts/foundation-integration-validation.json), [revue et limites](../_bmad-output/implementation-artifacts/foundation-integration-review.md).
