# US-051 — Contours TrueType

`FontFace::outline(glyph_id)` lit `loca` et `glyf` et renvoie un `atelier_geom::Path`. `outline_with_limits` permet d’ajuster les plafonds par appel avec `OutlineLimits`. Les chemins sont en unités de police, avec l’axe Y vers le haut. Le consommateur applique la taille et inverse Y si son système de coordonnées l’exige.

Les contours simples gèrent les flags répétés, les deltas signés, les points hors courbe consécutifs et leurs points on-curve implicites. Les contours composites appliquent d’abord la matrice du composant, puis l’attachement ou la translation. Les offsets XY sont transformés uniquement avec `SCALED_COMPONENT_OFFSET`; les offsets sans ce drapeau et ceux marqués `UNSCALED_COMPONENT_OFFSET` restent en unités de police. `ROUND_XY_TO_GRID` arrondit l’offset final. Une référence d’attachement à un point phantom ou inexistant échoue avec `OutlineError::Unsupported`.

Les limites par défaut sont une profondeur de 32 composants, 4096 composants, 1 000 000 points et 100 000 contours. Elles sont cumulées sur l’arbre de glyphes d’un appel. La profondeur configurable est plafonnée à 64. Les instructions de hinting sont vérifiées dans les limites de leur table et ignorées; elles ne sont jamais exécutées. CFF et variations restent hors périmètre. Les erreurs distinguent les données invalides, la troncature, les glyphes hors plage, les cycles, les limites dépassées, les cas non pris en charge et les erreurs géométriques. Aucun chemin partiel n’est renvoyé.

La suite compare les 300 contours de référence FontTools répartis sur les 20 faces embarquées après aplatissement à une tolérance de 0,01 unité. Elle ignore l’ordre des contours et leur point de départ cyclique, tout en préservant leur orientation.
