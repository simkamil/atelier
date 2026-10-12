<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-003 — Tests par images de référence

`atelier-image-diff` (`tools/image-diff`) compare une image obtenue à un PNG de référence versionné,
avec une tolérance à deux niveaux, et fournit en cas d'échec l'image obtenue et une image de
différence. Il offre une bibliothèque, à utiliser en `dev-dependencies` depuis n'importe quel crate,
et un binaire. Il dépend uniquement d'`atelier-image` (décodage et encodage PNG, [US-040](us-040-png.md),
[US-041](us-041-png-encoder.md)) ; il ne dépend pas du rasteriseur, donc aucun cycle n'est possible.
Le rendu d'une liste d'affichage ([US-082](us-082-raster-backend.md)) se fait côté appelant, en deux lignes.

```toml
[dev-dependencies]
atelier-image-diff = { path = "../../tools/image-diff" }
```

```rust
use atelier_image_diff::{Tolerance, assert_image_matches};

let image = atelier_raster::display_list::render_image(&list, &resources, &options).unwrap();
assert_image_matches!(&image, "tests/reference/page.png", Tolerance::exact());
```

La macro résout le chemin relativement au crate appelant (`CARGO_MANIFEST_DIR` évalué chez l'appelant).
La fonction `assert_image_matches(&image, chemin, &tolerance)` prend le chemin tel quel.
`tools/image-diff/tests/demo_raster.rs` montre le flux complet (rendu d'une petite page, comparaison,
régression simulée).

## API

- `compare(attendue, obtenue, &Tolerance) -> Result<Report, DiffError>` ; ne panique jamais.
  Des dimensions différentes donnent `DiffError::DimensionMismatch`, un tampon de pixels de mauvaise
  taille `InvalidImage`, un pourcentage hors 0–100 `InvalidTolerance`.
- `Tolerance { channel, max_different }` : `channel` est l'écart maximal par canal (0–255) en dessous
  duquel (inclus) un pixel est identique ; `max_different` est le nombre (`MaxDifferent::Count`) ou le
  pourcentage (`MaxDifferent::Percent`) maximal de pixels au-delà de cet écart. Constructeurs :
  `Tolerance::exact()`, `Tolerance::channel(n)`, `Tolerance::new(n, pourcentage)`.
- `Report` : dimensions, pixels comparés, pixels différents et leur proportion, écart maximal et moyen
  par canal (R, V, B, A), boîte englobante des pixels différents, tolérance, `passed`. Il s'affiche en texte.
- `diff_image(attendue, obtenue, &Tolerance) -> Image` : l'image obtenue composée sur blanc et éclaircie
  à un quart d'intensité, opaque ; chaque pixel au-delà de la tolérance par canal est rouge vif
  (`DIFF_MARK`, 255 0 0 255). Aucun autre pixel n'est rouge vif.
- `check_image_matches(&image, &chemin, &tolerance, &Config) -> Result<Outcome, MatchError>` : la
  forme sans panique des assertions. `MatchError` est typée : `ReferenceMissing`, `Decode` (PNG de
  référence corrompu), `Io`, `Incomparable` (dimensions), `Mismatch` (avec chemins et rapport), `UpdateInCi`.
- `load_png`, `save_png`, `artifact_paths` : utilitaires de fichiers.

### Alpha

Les images sont en RGBA droit. Les canaux de couleur sont comparés **prémultipliés par l'alpha**
(`arrondi(c × a / 255)`), l'alpha tel quel ; l'écart d'un pixel est le plus grand des quatre écarts.
Raison : la couleur d'un pixel transparent est invisible, deux rendus qui ne diffèrent que par elle
sont visuellement identiques (`255 0 0 0` et `0 0 255 0` sont identiques) ; en revanche tout changement
d'alpha compte. À alpha faible, un petit écart de couleur droite est atténué par la prémultiplication,
ce qui correspond à ce que voit l'utilisateur.

## En cas d'échec

Sont écrits `<nom>.actual.png` (image obtenue) et `<nom>.diff.png` (différence) à côté de la référence
`<nom>.png`, ou dans le dossier de `ATELIER_REFERENCE_OUT_DIR` (ou `Config::output_dir`). Le message
d'erreur donne les deux chemins, le nombre et la proportion de pixels différents, les écarts maximal
et moyen et la boîte englobante. Une réussite ultérieure supprime ces fichiers périmés. Avec des
dimensions différentes, seule l'image obtenue est écrite.

Ces fichiers ne doivent pas être versionnés ; ligne proposée pour `.gitignore` :

```
*.actual.png
*.diff.png
```

## Mettre à jour les références

Jamais par défaut, jamais en CI. Une référence absente fait échouer le test avec la marche à suivre.

```
ATELIER_UPDATE_REFERENCES=1 cargo test -p mon-crate
```

Avec `ATELIER_UPDATE_REFERENCES=1`, une référence absente est créée, une référence hors tolérance ou
illisible est réécrite (une référence dans la tolérance n'est pas touchée, pour éviter un bruit dans
les diffs). Si la variable `CI` est définie (non vide, ni `0` ni `false`), la mise à jour est refusée
(`MatchError::UpdateInCi`). Relire visuellement les PNG modifiés avant de les valider.
Le binaire offre aussi `atelier-image-diff update <obtenue.png> <référence.png>`.

## Binaire

```
atelier-image-diff compare <a.png> <b.png> [--channel-tolerance N] [--max-different PCT] [--diff sortie.png]
atelier-image-diff update <obtenue.png> <référence.png>
```

`compare` affiche le rapport ; code de sortie 0 si identique dans la tolérance, 1 sinon (y compris
dimensions différentes), 2 en cas d'erreur d'usage ou d'E/S (fichier absent, PNG corrompu).
`N` vaut 0 par défaut, `PCT` 0 par défaut.

## Tolérances conseillées

- Rendu déterministe (rasteriseur d'US-082 sans rotation, texte, formes, images droites) :
  `Tolerance::exact()`. Le rendu est identique au bit près sur toutes les plateformes.
- Image tournée ou cisaillée : la trigonométrie de `AffineTransform::rotation` peut différer du dernier
  bit selon la plateforme (réserve de la doc US-082). Commencer par `Tolerance::new(2, 0.5)` (écart 2
  par canal, 0,5 % des pixels) et resserrer si possible.
- Comparaison avec un rendu externe (US-308, export PDF d'US-332) : tolérance par canal de 8 à 32 et
  pourcentage à déterminer sur le corpus, en lisant l'écart moyen du rapport.

## Limites

- Comparaison pixel à pixel : pas de recalage, pas de mesure perceptuelle ; un décalage d'un pixel
  d'un contour compte comme différence.
- Pas de prise en compte des profils colorimétriques (aucune conversion), PNG 8 bits uniquement
  (limites de décodage par défaut : 16 Mpixels).
- La variable `CI` est la seule détection de la CI ; la mise à jour reste possible si elle n'est pas définie.
- Les références sont des PNG binaires versionnés : les garder minuscules.
- Pas de variante prenant une liste d'affichage : elle obligerait la bibliothèque à dépendre
  d'`atelier-raster`, qui pourrait à son tour l'utiliser en `dev-dependencies` (cycle de crates).
- Les tests existants d'`atelier-raster` (somme de contrôle FNV des PNG) n'ont pas été modifiés ; ils
  peuvent migrer vers `assert_image_matches!` pour obtenir l'image de différence au lieu d'un simple
  hachage.
