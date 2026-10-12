<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-061 — Rasteriseur logiciel

`atelier-raster` fournit `Rasterizer`, une surface RGBA8 droite, et les primitives
`fill_path`, `stroke_path`, `draw_image`, `push_clip_rect`, `push_clip_path` et
`pop_clip`. Les coordonnées sont des pixels ; l'origine est en haut à gauche.
L'API accepte les chemins et matrices validés d'`atelier-geom`. `image()` emprunte
le résultat et `into_image()` le transfère sans copie. `encode_png` réutilise
l'encodeur PNG d'`atelier-image` ; le raster ne possède aucun codec supplémentaire.

Les remplissages non-zéro et pair-impair ferment implicitement les sous-chemins.
Les courbes sont aplaties avec une tolérance de 1/16 pixel. Une grille fixe de
8 × 8 échantillons par pixel détermine la couverture ; les intersections avec les
clips utilisent ces mêmes bits. Un clip et un dessin couvrant la même moitié
d'un pixel produisent donc une demi-couverture, sans la réduire au quart.
Les clips empilés sont intersectés ; `pop_clip` restaure exactement le précédent.
`clear` efface toute la surface, indépendamment des clips.

Les contours proposent Butt/Square/Round pour les extrémités et
Bevel/Miter/Round pour les jointures. La limite de pointe est le rapport de la
distance au centre à la demi-épaisseur, avec repli en biseau. Les polygones du
contour sont réunis avant composition : les chevauchements entre segments et
jointures ne multiplient pas l'opacité. Les cercles sont approchés par des
polygones dont l'écart radial est au plus 1/32 pixel, avec 4 096 sommets maximum.
Une épaisseur nulle ne peint rien. Un sous-chemin ouvert réduit à un point
produit un disque, un carré ou rien selon son extrémité.

La composition est source-over, calculée avec des couleurs prémultipliées puis
rendue en RGBA droit, arrondi au niveau entier le plus proche. Elle s'effectue
sur les valeurs sRGB, sans conversion gamma ni profil ICC. Les conventions de
remplissage et de contour suivent les [règles SVG](https://www.w3.org/TR/SVG/painting),
et l'alpha suit les [équations de composition W3C](https://www.w3.org/TR/compositing-1/).

`draw_image` transforme le rectangle source `[0,width] × [0,height]` vers la
surface. Les centres des pixels sont aux demi-entiers. L'interpolation bilinéaire
utilise les quatre pixels source autour du centre inverse, en couleurs
prémultipliées ; les bords sont prolongés jusqu'au bord géométrique de l'image,
puis deviennent transparents. La grille d'échantillons calcule séparément la
couverture du rectangle et du clip. Une transformation identité conserve les
pixels opaques exactement et une couleur cachée derrière un alpha nul ne crée
pas de frange. Une matrice singulière est refusée.

## Limites et validation

`RasterLimits` borne les pixels, les arêtes, le travail d'une opération, la
profondeur des clips et la mémoire de leurs masques. Les valeurs par défaut sont
16 777 216 pixels, 65 536 arêtes, 250 millions d'unités de travail, 16 clips et
128 Mio de masques temporaires et conservés. Les coordonnées de rasterisation
sont limitées à ±1 milliard de pixels. Une limite franchie renvoie une erreur
avant de changer les pixels ou le clip courant. L'aplatissement applique d'abord
les limites fixes de `atelier-geom` ; ces budgets ne constituent pas une limite
globale de RAM du processus. Le coût est estimé de façon conservatrice avant
chaque opération ; une scène complexe peut nécessiter un découpage ou une
augmentation explicite du budget par l'appelant.

Les tests de `crates/raster/tests/primitives.rs` vérifient des pixels analytiques
(rectangles fractionnaires, trous, alpha source-over), un oracle indépendant
de triangle, les jointures/extrémités, les clips corrélés, les courbes, les
transformations d'image et l'interpolation d'alpha. Le PNG produit est décodé par
le décodeur existant et comparé pixel pour pixel. Les tests de limites vérifient
que les opérations refusées ne changent pas l'état observable.

Le backend consommant une liste d'affichage complète, ses ressources de glyphes
et ses images correspond à US-082 ; ces primitives ne raccordent pas encore ce
backend à l'éditeur.

## Validation d’intégration — 2026-10-12

La [passe finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit 41 contrôles, 507 tests workspace et 55 tests natifs release,
avec sources et lock inchangés. Les 57 assertions de l’éditeur utilisent le vrai
Worker/WASM. Clippy Windows/WASM, format, licences et builds Windows/WASM/musl
passent ; les tests musl sont compilés localement et exécutés en CI Linux.
La [copie portable](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
fonctionne avec le seul atelier.exe et ses 18 ressources embarquées.
Les limites des US parentes et l’échec strict de handles US-111 restent ouverts.
