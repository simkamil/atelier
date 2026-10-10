<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-060 — Géométrie 2D

`atelier-geom` fournit des primitives f64 sans dépendance pour les couches de rendu. Toutes les coordonnées doivent être finies et leur valeur absolue ne doit pas dépasser `MAX_COORDINATE` (`1e150`); cette limite porte sur les coordonnées, pas sur les dimensions dérivées. `Point::try_new` est le constructeur public des points; `x()` et `y()` exposent leurs coordonnées.

```rust
use atelier_geom::{AffineTransform, Path, Point, Rect};

fn example() -> Result<(), atelier_geom::GeometryError> {
    let origin = Point::try_new(0.0, 0.0)?;
    let mut path = Path::new();
    path.move_to(origin);
    path.cubic_to(
        Point::try_new(20.0, 0.0)?,
        Point::try_new(20.0, 20.0)?,
        Point::try_new(40.0, 20.0)?,
    )?;
    let bounds: Rect = path.bounds()?.expect("path has a move point");
    let outline = path.flatten(0.25)?;

    let translate = AffineTransform::translation(5.0, 2.0)?;
    let scale = AffineTransform::scale(2.0, 2.0)?;
    let translated_then_scaled = translate.then(scale)?;
    let mapped = translated_then_scaled.apply(origin)?;
    let _ = (bounds, outline, mapped);
    Ok(())
}
```

`Rect::new(a, b)` normalise les coins. `width()` et `height()` restent des accesseurs infallibles et peuvent atteindre `2 * MAX_COORDINATE` lorsque les coins opposés valent `-MAX_COORDINATE` et `MAX_COORDINATE`. `contains` inclut le bord, `union` couvre les deux rectangles et `intersection` renvoie `None` lorsqu'ils sont disjoints; un contact sur un bord renvoie un rectangle de largeur ou hauteur nulle. `Rect::from_xywh` accepte les dimensions signées et normalise le résultat.

Les matrices utilisent `x' = a*x + c*y + e` et `y' = b*x + d*y + f` avec des vecteurs colonnes. `first.then(next)` applique d'abord `first`, puis `next`. `inverse` renvoie `SingularTransform` pour une matrice non inversible. Les calculs qui débordent ou dépassent la plage coordonnée sont signalés, pas convertis silencieusement en `NaN`.

Un `Path` contient zéro ou plusieurs sous-chemins. `move_to` commence un nouveau sous-chemin; les opérations `line_to`, `quadratic_to`, `cubic_to` et `close` requièrent un sous-chemin actif. `close` ferme le sous-chemin en reliant son dernier point à son point de départ, sans ajouter de segment visible à la liste aplatie; l'appeler plusieurs fois est sans effet. Après un `close`, le prochain ajout de segment commence un nouveau sous-chemin ouvert au point de départ du sous-chemin fermé. Les bornes incluent les ancres et les extrema calculés analytiquement des courbes quadratiques et cubiques.

`flatten(tolerance)` exige une tolérance strictement positive, finie et au plus égale à `MAX_COORDINATE`. La subdivision de De Casteljau est bornée à 32 niveaux et à 1 048 576 segments pour l'appel entier. Une tolérance trop exigeante retourne `SubdivisionLimitExceeded`. L'aplatissement teste les points de contrôle contre le segment fini entre les extrémités; l'enveloppe convexe de Bézier garantit alors que chaque point de la courbe est à au plus la tolérance de ce segment. Les extrémités sont conservées, et chaque entrée de `Vec<FlattenedSubpath>` reste indépendante.

## Validation locale

Depuis la racine du dépôt :

```powershell
cargo test -p atelier-geom --locked --offline
cargo clippy -p atelier-geom --all-targets --locked --offline -- -D warnings
cargo fmt -p atelier-geom
```

Ces commandes vérifient le code localement; aucune exécution distante n'est présumée.
