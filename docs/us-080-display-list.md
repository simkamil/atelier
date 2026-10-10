<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-080 — Liste d’affichage par page

`atelier-render` porte une représentation ordonnée et indépendante des backends.
Une `DisplayList` contient les bornes d’une page et les commandes dans leur
ordre de peinture. Elle ne charge aucune image ni police : `image_id` et
`font_id` sont des références opaques attribuées par le document.

## Unités et valeurs

Les points, rectangles, avances de glyphes, largeurs de trait et matrices sont
exprimés en unités de page. Les points et les rectangles réutilisent les types
validés de `atelier-geom`; leurs coordonnées sont des `f64` finis dans
`[-MAX_COORDINATE, MAX_COORDINATE]`. Les pages ont une largeur et une hauteur
strictement positives. Les matrices utilisent les vecteurs-colonnes et la
convention `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.

Les couleurs sont des composantes RGBA non prémultipliées finies dans `[0, 1]`.
L’alpha est conservé sans interprétation afin que chaque backend applique la
politique de composition qui lui convient. Les glyphes portent un identifiant
de glyphe, une position et une avance; aucune interprétation de police n’est
effectuée ici.

## État de rejeu

Chaque page démarre avec une transformation identité et un clip infini. Les
commandes de dessin sont peintes dans l’ordre du tableau. Les piles de clips et
de transformations sont indépendantes, chacune limitée à 128 niveaux. En v1,
les clips sont rectangulaires. Le rectangle fourni par `PushClip` est déjà en
coordonnées de page et est intersecté avec le clip courant; `PopClip` restaure
seulement le clip. `PushTransform` compose la transformation courante avec la
nouvelle matrice; `PopTransform` restaure
seulement la transformation. Les deux piles doivent être équilibrées à la fin
de la page. `DisplayTransform` est la représentation affine sérialisable de
cette crate : l’API publique actuelle de `atelier-geom::AffineTransform` ne
permet ni de lire ni de construire une matrice générale. Les liens portent un
rectangle déjà exprimé en coordonnées de page, et les marqueurs `Structure` ne
modifient pas l’état graphique.

## Format binaire ATDL

Tous les entiers et nombres flottants sont écrits en little-endian, sans écrire
la représentation mémoire des structures Rust. L’en-tête est la signature
ASCII `ATDL`, suivie de la version majeure `u16` (1) et mineure `u16` (0), des
deux coins du rectangle de page (quatre `f64`), puis du compte de commandes
(`u32`). Chaque commande commence par un opcode explicite (`u8`):

| Opcode | Commande |
| ---: | --- |
| 1 | Remplissage de chemin : règle, RGBA, chemin |
| 2 | Contour de chemin : RGBA, largeur, cap, jointure, limite de mitre, chemin |
| 3 | Séquence de glyphes : référence de police, RGBA, compte et glyphes positionnés |
| 4 | Image : référence opaque et rectangle |
| 5 / 6 | Push / pop du clip |
| 7 / 8 | Push / pop de transformation affine |
| 9 | Lien : rectangle et cible UTF-8 |
| 10 | Structure : rôle UTF-8 et identifiant opaque de nœud |

Les champs d’énumération sont des octets (`FillRule`: 0 non-zéro, 1 pair-impair;
`LineCap`: 0 bout, 1 rond, 2 carré; `LineJoin`: 0 mitre, 1 rond, 2 biseau).
Les chemins ont un compte `u32` d’éléments; chaque élément commence par son
opcode (`MoveTo` 0, `LineTo` 1, `QuadraticTo` 2, `CubicTo` 3, `Close` 4), suivi
de ses points `f64` lorsqu’il y en a. Les chaînes sont un compte d’octets `u32`
et des octets UTF-8. Il n’y a ni remplissage implicite, ni champ réservé.

La version, chaque opcode et chaque énumération inconnus sont refusés. Le
décodeur exige le flux complet : données tronquées ou suffixe surnuméraire
produisent une erreur typée, et aucun résultat partiel n’est retourné.

## Budgets et erreurs

Les limites publiques dans `atelier_render::limits` sont : 64 MiB par flux,
1 000 000 commandes, 1 000 000 éléments de chemin cumulés, 1 000 000 glyphes
cumulés, 4 096 octets par chaîne et 128 niveaux par pile. Le décodeur contrôle
les comptes avant toute allocation; additions et conversions vers `u32` sont
vérifiées. `DisplayListError` distingue valeur invalide, géométrie, état,
budget et format. L’encodeur calcule la taille exacte avec des additions et
multiplications vérifiées, refuse un flux de plus de 64 MiB avant d'allouer son
tampon, puis valide la liste avant d’écrire des octets.

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use atelier_geom::{Point, Rect};
use atelier_render::{Color, Command, DisplayList, DisplayPath, FillRule, PageInfo, PathElement};

let page = PageInfo::new(Rect::from_xywh(0.0, 0.0, 612.0, 792.0)?)?;
let path = DisplayPath::new(vec![
    PathElement::MoveTo(Point::try_new(10.0, 10.0)?),
    PathElement::LineTo(Point::try_new(80.0, 10.0)?),
    PathElement::LineTo(Point::try_new(80.0, 40.0)?),
    PathElement::Close,
])?;
let list = DisplayList::new(page, vec![Command::FillPath {
    path,
    color: Color::new(0.2, 0.4, 0.8, 1.0)?,
    rule: FillRule::NonZero,
}])?;
let bytes = list.encode()?;
assert_eq!(DisplayList::decode(&bytes)?, list);
# Ok(())
# }
```
