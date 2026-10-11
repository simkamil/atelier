---
title: 'Mise en forme du texte latin (US-054)'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: '6f4b0b34ab39d5a15a73d21f615d2cbf75c7edf6'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.6-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le moteur sait lire et ajuster des glyphes mais ne transforme pas encore le texte en run positionné. **Approach:** Ajouter atelier-shape, orchestrateur Latin LTR qui réutilise cmap, GSUB/GPOS et graphèmes pour fournir glyphes, avances, décalages et clusters à la mise en page.

## Boundaries & Constraints

**Always:** Rust sûr et dépendances uniquement locales `atelier-font`/`atelier-unicode`. Façade réutilisable `LatinShaper` empruntant une FontFace et sa FontLayout validée une fois ; constructeurs normal et avec LayoutLimits. Entrées `shape_text(&str, ShapeOptions)` et `shape_glyphs(&[GlyphCluster], ShapeOptions)` renvoient `Result<GlyphRun, ShapeError>`. API générique glyphes conserve les plages de scalaires sans exiger du texte ni un caractère pour chaque glyphe. `GlyphRun` expose glyphes positionnés, avances totales X/Y et, pour l'entrée texte, offsets UTF-8 des frontières de graphèmes source. Chaque PositionedGlyph expose glyph_id, cluster Range<usize>, x/y_advance et x/y_offset ; clusters en indices de scalaires, indépendants des offsets octets de graphèmes.

Unité déterministe : toutes positions, taille et interlettrage en i64, 1 unité = 1/64 point typographique. `ShapeOptions { size: i64, letter_spacing: i64, horizontal_scale: u32 }` ; défaut taille 704 (11pt), espacement 0, chasse 1000 (100%). Taille >0 et chasse >0, interlettrage signé. Mise à l'échelle des unités de police avec intermédiaires i128 checked ; arrondi au plus proche, égalités en s'éloignant de zéro. X = valeur_font * size * horizontal_scale / (units_per_em *1000) ; Y = valeur_font * size / units_per_em. Une erreur d'overflow/option invalide ne donne aucune sortie partielle. Ne pas convertir en f64 dans le calcul ; le rendu convertira les unités plus tard.

Pipeline texte : cmap scalaire→glyph_id (.notdef 0 si absent), clusters i..i+1 ; ligatures si interlettrage nul ; puis GPOS/kern et métriques du glyphe substitué ; enfin échelle. À espacement non nul, désactiver `liga` optionnel pour préserver les unités de texte. Ajouter l'espacement après le dernier glyphe de chaque graphème source sauf le dernier ; jamais à l'intérieur d'un graphème ni après la fin. En entrée générique sans texte, l'ajouter entre groupes de clusters source identiques consécutifs, sans le modifier dans ces groupes ; documenter cette distinction. L'espacement est une distance finale en unités de sortie, non multipliée par la chasse. Un glyphe sans avance peut recevoir l'espacement de fin de groupe. Les ajustements de placement restent distincts des avances ; accumuler X/Y en checked.

Respecter limites LayoutLimits, plafonner scalaires avant allocation ; aucun gros texte contournant input_glyphs. La façade conserve le budget de la FontLayout pour chaque opération. Pas de repli de police ici : US-055 le gère.

**Never:** bidi, écritures complexes ou mark positioning, césure/pagination, géométrie arbitraire de curseur interne aux ligatures, nouvelles tables OpenType ou modification des APIs font/unicode existantes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Référence | Texte des 20 faces, espacement nul | Glyphes/clusters/deltas HarfBuzz puis échelle exacte | Succès |
| Taille/chasse | Fraction, valeurs négatives de kern, X/Y | Arrondi fixé et total cohérent | Overflow typé |
| Espacement | Ligature fi, accent combinant, emoji ZWJ | Liga coupé, espace entre graphèmes seulement | Aucun découpage de graphème |
| Générique | Glyphes sans texte, clusters partagés/ranges complexes | Préservation des clusters et groupes | Glyph IDs/plages invalides refusés |
| Bornes | vide, options invalides, limite exacte/+1 | Run vide ou erreur précise | Aucune panique/sortie partielle |

</frozen-after-approval>

## Code Map

- `crates/shape/src/lib.rs` — squelette à remplacer ; crate existant.
- `crates/font/src/lib.rs` — FontFace::glyph_index/horizontal_metric/metrics.
- `crates/font/src/layout.rs` — FontLayout::{apply_ligatures,pair_adjustments}, GlyphCluster, LayoutLimits.
- `crates/unicode/src/segmentation.rs` — grapheme_boundaries en offsets octets UTF-8.
- `tests/font-layout-reference/layout.tsv` — 54 716 sorties indépendantes HarfBuzz en unités de police, réutilisables par le coordinateur pour vérifier l'orchestrateur.

## Tasks & Acceptance

**Execution:**
- [x] `crates/shape/src/lib.rs` — façade, types, calculs bornés, options documentées.
- [x] `crates/shape/tests/latin.rs` — matrice entière, marques/ligatures, séquences génériques, limites et erreurs.
- [x] `docs/us-054-latin-shaping.md` — API, unités, arrondi, interlettrage et limites.

**Acceptance Criteria:**
- Given les fontes réelles et textes de référence, when cmap/GSUB/GPOS/échelle s'appliquent, then chaque glyphe, cluster et position concorde avec l'oracle indépendant.
- Given une séquence générique de glyphes, when shaping est appelé, then elle ne dépend pas de caractères reconstruits et conserve ses plages source.

## Implementation Notes

Autorisation persistante : agents GPT-6 Luna moyen en parallèle, validation et push. Propriété exclusive : trois fichiers des tâches et notes append-only ici. Root gère Cargo.toml et lock, exports/manifests, corpus d'oracle complet, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent ; aucun autre fichier modifié. Cargo ciblé après ajout des deux dépendances par root ; rustfmt --edition 2024 sur fichiers possédés uniquement. Les décisions d'unité/espacement sont techniques, compatibles avec la SPEC ; aucun nouveau choix utilisateur requis.

## Spec Change Log

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B1 | Le TSV binaire HarfBuzz est omis du diff ; ses dimensions/empreinte manquent. | false | Le TSV ne change pas depuis la baseline. Son manifeste suivi contient déjà 54 716 cas, vingt SHA de faces et le SHA du corpus. Le garde check-ucd vérifie cette empreinte et les deux tests publics le consomment intégralement. Aucun nouveau corpus opaque n'est introduit. |
| B2 | Les sorties Graphify volumineuses masquent les changements fonctionnels. | low | Le diff complet conserve les artefacts et son index énumère tous les fichiers. Un résumé d'impact explicite est ajouté ci-dessous pour accélérer la navigation ; aucun fichier n'est retiré de la revue. |
| B3 | line_break_class n'a pas de contrôle indépendant sur tous les scalaires. | false | crates/unicode/tests/ucd_reference.rs inclut déjà line_break parmi les propriétés dont l'empreinte est recalculée sur tous les scalaires (déclaration ligne 48 et alimentation ligne 89). L'oracle figé vient des données UCD indépendantes ; EAW complète ce contrôle existant. |
| B4 | line_breaks alloue en proportion du texte sans plafond configurable. | low | L'API &str vers Vec et sa complexité O(n) sont explicites. La documentation précise maintenant l'absence de plafond intégré et le budget fixé par l'appelant ; aucun paramètre ni garde non demandé n'est ajouté. |
| B5 | La direction Y et l'origine des placements ne sont pas documentées. | medium | Un futur appelant Canvas pourrait inverser les placements. La doc fixe X droite, Y haut, offsets relatifs au stylo courant et avances appliquées ensuite, conformément aux valeurs OpenType transmises. |
| B6 | La doc pipeline ne précise pas l'absence de positionnement des marques. | false | Le dernier paragraphe excluait déjà explicitement le positionnement des marques. Le pipeline mentionne aussi désormais « ajustements par paires », clarification sans changement de comportement. |
| B7 | Les plages génériques chevauchantes/non ordonnées ne sont pas clarifiées. | low | Le code accepte toute plage non vide et traite les groupes identiques consécutifs. La doc l'explicite et un test public utilise 8..12, 3..10, 3..10, 1..2, vérifiant maintien de l'ordre et deltas [17,0,17,0]. |
| B8 | EmptyRegistry désigne aussi une famille absente d'un registre non vide. | low | Display disait déjà qu'aucune face ne satisfait la requête. Le commentaire public et la doc précisent cette signification et is_empty() pour distinguer les situations ; aucune nouvelle variante API. |
| B9 | Les tâches restent non cochées malgré les pages d'API implémentées. | false | Le relevé et les trois frontmatters indiquent in-review et attendent expressément la validation finale. Les cases ne doivent être cochées qu'après les contrôles immuables ; leur fermeture est conditionnée aux preuves, pas anticipée. |
| B10 | Les commandes des oracles shaping et Windows ne sont pas explicites dans le relevé. | low | Les sources et le harnais contenaient les commandes ; une section Reproduction les rend directement consultables avec le lien vers chaque résultat. |
| B11 | Les quatre variantes Arial ne disposent pas d'un résumé lisible. | low | La preuve JSON contient quatre lignes famille/poids/style/ID. La doc US-055 ajoute un tableau des quatre sélections System pour faciliter la lecture. |

[Revue commune et contrôles du coordinateur](text-layout-review.md). Aucun constat différé.

## Verification

- `cargo test -p atelier-shape --locked --offline` — cas ciblés et références passent.
- `cargo clippy -p atelier-shape --all-targets --locked --offline -- -D warnings` — aucun avertissement.

Note de coordination : les agents ont développé les fichiers en parallèle. Après la limite d’usage Luna, le coordinateur a finalisé les corrections et les vérifications. Les statuts de fin attendent la revue et la validation du lot.

Validation finale : 26 contrôles, 291 tests workspace et 101 tests release. Sources et Cargo.lock identiques pendant les contrôles. Les critères sont couverts par les tests publics ciblés, les oracles indépendants et le [relevé complet](text-layout-validation.json). La [revue triée](text-layout-review.md) ne conserve aucun constat différé.
