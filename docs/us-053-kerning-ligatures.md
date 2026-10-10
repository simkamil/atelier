<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-053: crénage et ligatures

`FontFace::layout()` crée une façade empruntée aux tables GSUB, GPOS, GDEF et `kern`. La construction sélectionne `latn` avec son `DefaultLangSys`; elle consulte `DFLT` uniquement si `latn` est absent. Seules les features `liga` et `kern` de ce système de langue sont retenues. Une feature required est traitée comme une feature ordinaire lorsqu’elle porte le tag concerné. Les indices de lookups retenus sont dédupliqués et appliqués dans l’ordre du `LookupList`.

```rust
use atelier_font::{FontFace, GlyphCluster};

let face = FontFace::parse(font_bytes)?;
let layout = face.layout()?;
let input = [
    GlyphCluster { glyph_id: face.glyph_index('f'), cluster: 0..1 },
    GlyphCluster { glyph_id: face.glyph_index('i'), cluster: 1..2 },
];
let ligatures = layout.apply_ligatures(&input)?;
let adjustments = layout.pair_adjustments(&ligatures)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Les bornes des clusters sont des indices de scalaires Unicode dans le texte source, avec une fin exclue. Chaque plage doit être non vide et chaque identifiant doit être inférieur à `numGlyphs`. Une substitution simple conserve sa plage. Une ligature fusionne les plages des composants par minimum et maximum; les marques traversées par un lookup `IgnoreMarks` restent dans le résultat avec leur plage d’origine. Cette API reçoit déjà des glyphes: elle ne fait ni cmap ni shaping, bidi, ni positionnement des marques.

`GlyphAdjustment` expose les placements X/Y et avances X/Y signés en unités de police. `pair_adjustments` renvoie exactement une entrée par glyphe. Les additions sont contrôlées. Une feature GPOS `kern` applicable choisit cette source pour tout le run, même lorsqu’aucune paire n’a de valeur; la table `kern` version 0, format 0 n’est utilisée qu’en son absence. Les sous-tables horizontales historiques ordinaires s’additionnent, et une couverture override remplace l’ajustement historique précédent. GPOS et `kern` ne sont jamais additionnés ensemble.

Les formats lus sont Coverage et ClassDef 1/2, GSUB SingleSubst 1/2, LigatureSubst 4/1 et son extension 7 vers 1/4, GPOS PairPos 2/1 et 2/2 et son extension 9 vers 2. Les lookups à flags autres que zéro ou `IgnoreMarks`, les formats actifs inconnus, ainsi que les Device/VariationIndex actifs sont signalés explicitement. Les offsets, glyphes, classes, paires, matrices et comptes sélectionnés sont validés à la création de la façade. Une table absente donne une identité ou zéro; une table sélectionnée mal formée renvoie `LayoutError`, sans fallback ni sortie partielle.

Les valeurs par défaut de `LayoutLimits` bornent la validation à 1 000 000 unités, le travail de chaque opération à 10 000 000 unités, l’entrée à 65 536 glyphes et les lookups sélectionnés à 1 024. `FontFace::layout_with_limits` permet de choisir d’autres seuils. `LayoutError` distingue troncature, données/formats invalides, fonctionnalité non prise en charge, dépassement de limite, identifiant invalide et plage de cluster invalide.
