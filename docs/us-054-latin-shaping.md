<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-054 : mise en forme du texte latin

`atelier-shape` fournit `LatinShaper`, une façade réutilisable qui emprunte une `FontFace` et valide ses tables de mise en forme lors de sa construction. `LatinShaper::new` applique les `LayoutLimits` par défaut; `with_layout_limits` permet de conserver un budget spécifique. Le crate ne dépend que de `atelier-font` et `atelier-unicode`.

```rust
use atelier_font::FontFace;
use atelier_shape::{LatinShaper, ShapeOptions};

let face = FontFace::parse(font_bytes)?;
let shaper = LatinShaper::new(&face)?;
let run = shaper.shape_text("office", ShapeOptions::default())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`shape_text` mappe chaque scalaire via cmap (glyph 0 si absent), applique `liga` seulement quand l’interlettrage vaut zéro, puis les ajustements par paires GPOS/kern et les métriques des glyphes résultants. Les plages des clusters sont des indices de scalaires Unicode. La sortie fournit les glyphes positionnés, les avances X/Y totales et les offsets UTF-8 des frontières de graphèmes. Les substitutions fusionnent les plages selon les règles d’`atelier-font`.

`shape_glyphs` reçoit directement des `GlyphCluster`; il ne reconstruit aucun caractère et conserve leurs plages source. L’interlettrage générique est ajouté entre groupes consécutifs de plages de cluster identiques. Pour `shape_text`, il est ajouté au dernier glyphe de chaque graphème source, sauf au dernier graphème. Il ne coupe jamais un graphème, même pour une marque combinante ou une séquence emoji ZWJ. À espacement non nul, `liga` est désactivé pour que les frontières source restent disponibles. L’espacement est une distance finale et ne subit pas la chasse horizontale.

Toutes les positions, avances, offsets, taille et espacement utilisent des entiers `i64` en unités de 1/64 point. Les options par défaut sont 704 (11 pt), 0 et 1000 (100 %). Taille et chasse doivent être positives; l’espacement peut être signé. Les valeurs de police sont mises à l’échelle avec des intermédiaires `i128` contrôlés : X multiplie aussi par la chasse, Y ne le fait pas. L’arrondi est au plus proche, les égalités s’éloignant de zéro. Les placements restent séparés des avances; toute erreur d’option, de limite ou d’arithmétique renvoie une erreur sans run partiel.

Les limites `input_glyphs` plafonnent les scalaires avant la segmentation et l’allocation des glyphes, ainsi que la longueur d’entrée générique. `LayoutLimits` s’appliquent ensuite à GSUB et GPOS. Cette API ne fait ni bidi, ni fallback de police, ni positionnement des marques, ni césure ou pagination; les séquences génériques permettent aux futures étapes de réutiliser les glyphes sans prétendre à un shaping d’écriture complexe.

Le test indépendant `crates/shape/tests/reference.rs` réutilise les 54 716 sorties HarfBuzz sur vingt faces et compare chaque glyphe, cluster, placement et avance après mise à l'échelle fractionnaire (taille 721/64 pt et chasse 113,7 %). Le même test vérifie la désactivation de `liga` pour un interlettrage signé et son emplacement exact autour des marques combinantes et séquences emoji. Les tests ciblés vérifient les égalités d'arrondi signées, les limites et les overflows.

Les offsets sont des déplacements relatifs au stylo courant, avant de dessiner le glyphe ; les avances déplacent ensuite ce stylo. X positif va vers la droite et Y positif vers le haut, selon les coordonnées OpenType. Un moteur Canvas à Y descendant convertira cette convention au rendu. Les plages génériques non vides peuvent se chevaucher ou être non ordonnées ; l'ordre des glyphes fourni reste celui du run. L'espacement distingue les groupes identiques consécutifs, sans regrouper des plages égales éloignées.

Validation du lot : [26 contrôles](../_bmad-output/implementation-artifacts/text-layout-validation.json), 291 tests workspace et 101 tests release sur sources inchangées ; [revue indépendante triée](../_bmad-output/implementation-artifacts/text-layout-review.md), aucun constat différé.
