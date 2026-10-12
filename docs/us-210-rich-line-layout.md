<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-210 — Mise en page des lignes riches

`atelier-text-layout` transforme un `Paragraph` riche en lignes et fragments de glyphes déterministes. Il utilise les propriétés de paragraphe résolues de US-201, les attributs effectifs des runs (par exemple ceux produits après import DOCX US-301), les opportunités UAX #14 d’Unicode 17 et le shaping latin d’US-054.

```rust
use atelier_font::manager::FontManager;
use atelier_text_layout::{layout_paragraph, LayoutOptions};
use atelier_text_model::rich::{Paragraph, ResolvedProperties};

let fonts = FontManager::new();
let paragraph = Paragraph::with_text("Un paragraphe riche.");
let layout = layout_paragraph(
    &paragraph,
    &ResolvedProperties::default(),
    &fonts,
    LayoutOptions::new(40 * 64), // 40 pt de largeur
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

La largeur de `LayoutOptions`, les positions, les avances, les tailles et les métriques de ligne sont en 1/64 de point typographique. Les propriétés d’indentation, d’espacement avant/après et les règles exactes ou minimales restent en twips. La conversion utilise un arrondi au plus proche, les égalités s’éloignant de zéro. Les offsets `source` de paragraphe, ligne, fragment et glyphe sont des offsets d’octets UTF-8; chaque frontière publiée est aussi une frontière de graphème. Une ligature conserve son cluster entier. Les lignes consomment les LF et CRLF obligatoires; ces séparateurs ne produisent pas de glyphes. Les espaces blancs terminaux restent dans la plage source, mais ne contribuent pas à la largeur ni à la sortie glyphes.

Le moteur essaie d’abord les coupures UAX #14 qui tiennent dans la largeur disponible. Un mot trop long est ensuite coupé aux frontières de graphèmes sûres; un graphème ou cluster qui dépasse seul reste sur sa ligne et déborde. Chaque préfixe candidat est reshaped dans son contexte de ligne, et chaque face inscrite réutilise son shaper pendant la mise en page. Les limites par défaut sont 1 MiB de texte, 65 536 glyphes, 16 384 lignes et 4 000 000 unités de travail; une limite ou une erreur de police, shaping ou arithmétique renvoie `LayoutError` sans résultat partiel.

Les demandes utilisent les familles et propriétés caractère effectives, avec Calibri/Sans Serif par défaut. `FontManager` applique ses alias métriques et son ordre de repli déterministe. Un glyphe absent dans le premier scalaire d’un graphème renvoie `MissingGlyph`; un glyphe absent dans les autres scalaires du même graphème est représenté par glyph 0 (`.notdef`) selon le contrat de `LatinShaper`. Aucun gras ou italique synthétique, découverte de polices système, fallback d’écriture complexe, bidi ni césure linguistique n’est effectué. `ParagraphLayout.unsupported` signale les exposants/indices, objets ancrés, champs et décorations de liens présents.

Les [tabulations US-211](us-211-tabs.md) participent désormais à la coupure et au placement : l’API existante utilise les taquets par défaut; l’API explicite fournit les taquets personnalisés et décorations. Une tabulation est représentée par un glyphe de contrôle d’id zéro, identifié par sa plage source U+0009; les consommateurs conservent son avance et ne dessinent pas son contour. La [pagination](us-212-pagination.md) et la [géométrie du curseur](us-214-caret-geometry.md) consomment les mêmes lignes, sans modifier leurs offsets source.

Les lignes prennent le maximum des métriques hhea ascender, descender et line-gap des faces et tailles présentes sur cette ligne. Alignement gauche/centré/droite et justification sont calculés dans la largeur restante après retraits. La justification répartit le reliquat sur les espaces U+0020 des lignes intermédiaires; les lignes finales et celles qui terminent sur une coupure obligatoire ne sont pas justifiées. L’interligne automatique applique `value / 240` aux métriques de la ligne; l’interligne exact remplace sa hauteur, et `AtLeast` prend le maximum entre la métrique et le minimum demandé.

Les tests du crate couvrent la géométrie numérique, les quatre alignements, les retraits suspendus, les lignes LF/CRLF, NBSP/ZWSP, les mots trop longs, les graphèmes combinants, les clusters shaping, les tailles mixtes, les interlignes et les erreurs de budget. Le corpus de comparaison indépendant avec Word n’est pas disponible sur cet hôte. Le critère original d’au moins 95 % de coupures de lignes identiques à Word reste donc **ouvert**; aucune référence Word n’a été fabriquée et US-210 demeure partielle jusqu’à preuve authentique.

## Intégration au MVP

Un [aperçu DOCX en lecture seule](rich-docx-preview.md) utilise désormais cette API dans l’application Windows. Les limites du lot, les critères encore ouverts et les preuves intégrées sont consignés dans ce contrat. L’édition riche et la comparaison à Word restent séparées.
