---
title: 'Contours TrueType (US-051)'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: '20a6a05bd69d22f4f004b7a907d2c9696b24c4ad'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.6-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le rendu dispose des glyph IDs et métriques, mais ne peut pas dessiner les contours des polices embarquées. **Approach:** Lire loca/glyf dans FontFace et produire des chemins géométriques quadratiques, simples ou composites, bornés.

## Boundaries & Constraints

**Always:** Rust sûr, dépendance locale atelier-geom. API `FontFace::outline(glyph_id:u16)->Result<Path,OutlineError>` et `outline_with_limits(glyph_id,OutlineLimits)`. Limites explicites défaut : profondeur 32, 4096 composants, 1_000_000 points et 100_000 contours par appel, même limites cumulées dans l’arbre; profondeur publique plafonnée à 64 pour protéger la pile. Distinguer erreur de données, troncature, glyphe, cycle, limite et format non pris en charge. Lire FontFace.table/metrics, indexToLocFormat court/long; valider loca, bornes glyf et maxp 1.0, compte/flags/instructions avant allocation. Retour vide pour glyphe sans contours, jamais de résultat partiel.

Contours simples : flags répétés, deltas signés, fermeture, points hors courbe consécutifs et milieux implicites. Composites : translation XY signée, points d’attachement unsigned, transformations F2DOT14 (uniforme, axes, matrice), composants imbriqués/cycles. Ordre : transformer enfant, puis attacher/translater. SCALED_COMPONENT_OFFSET transforme l’offset, UNSCALED ou absence ne le transforme pas; drapeaux contradictoires rejetés. ROUND_XY_TO_GRID arrondit l’offset après transformation. Conserver les points de contour pour attacher des composants; des ancres phantom non prises en charge doivent produire une erreur Unsupported documentée, jamais une position inventée. Instructions de hinting bornées mais non exécutées. Coordonnées en unités de police, axe Y vers le haut; le consommateur applique taille et inversion Y.

**Never:** dépendance externe, unsafe, CFF ou variations, exécution bytecode, rasteriseur maison dans cette US, modification du parseur existant hors raccordement du module, assouplissement des contrôles de US-050.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Simple | points on/off curve, répétitions, vide | Path fermé conforme en unités de police | Succès |
| Composite | transforms, attachement points, profondeur admissible | tracé identique à l’expansion de référence | Succès |
| Hostile | loca/flags/troncature, cycle, ancre invalide | aucun panic ni résultat partiel | erreur typée |
| Budget | cumul points/composants/profondeur au seuil puis au-delà | seuil accepté, dépassement refusé avant allocation | LimitExceeded |

</frozen-after-approval>

## Code Map

- `crates/font/src/lib.rs` — FontFace.table, metrics, FontMetrics.number_of_glyphs/index_to_loc_format; export coordonné du module.
- `crates/font/src/outline.rs` — impl FontFace et types du décodeur, indépendants du backend.
- `crates/geom/src/lib.rs` — Path.move_to/line_to/quadratic_to/close, points vérifiés; à réutiliser sans modifier.
- `data/fonts/` — 20 TTF et licences existants; Carlito/Liberation pour références.
- `crates/font/tests/outlines.rs` — données synthétiques ciblées et polices réelles.

## Tasks & Acceptance

**Execution:**
- [x] `crates/font/src/outline.rs` — parser loca/glyf et convertir contours simples/composites avec budgets cumulés.
- [x] `crates/font/tests/outlines.rs` — valider cas normaux, transforms/attachments et erreurs/budgets.
- [x] `docs/us-051-truetype-outlines.md` — coordonnées, API, limites et absence de hinting/variations.

**Acceptance Criteria:**
- Given Carlito et Liberation, when contours exportés et rendus avec le même rasteriseur que les références FontTools, then images identiques.
- Given toutes les faces embarquées, when glyphes comparés à FontTools, then points/contours géométriques concordent indépendamment.
- Given données synthétiques invalides et mutations, when décodées, then erreurs propres sans panic; les limites sont respectées même dans les composites imbriqués.

## Verification

- `cargo test -p atelier-font --locked --offline` — parseur existant et contours passent.
- `cargo check -p atelier-font --target wasm32-unknown-unknown --locked --offline` — API portable.

## Implementation Notes

Autorisation persistante de développer, valider et publier les US en parallèle avec GPT-6 Luna moyen. Le coordinateur gère manifests partagés, lock, exports partagés, CI, preuves, Graphify et Git. L’agent ne lance ni skill, ni Graphify, ni sous-agent. UTF-8 explicite; aucune modification hors propriété indiquée.

## Spec Change Log

## Review Triage Log

Propriété exclusive : crates/font/src/outline.rs, crates/font/tests/outlines.rs et doc US-051. Le parent raccorde crates/font/src/lib.rs et Cargo.toml.


| ID | Constat | Verdict et preuve | Disposition |
|---|---|---|---|
| B8 | Erreurs de flags/coordonnées/transforms non ciblées | medium : corpus valide et oracle ne prouvent pas ces branches de rejet. | Mutations ciblées avec attentes d’erreurs typées dans outlines.rs. |
| B9 | Budgets imbriqués non testés aux frontières | medium : le test composant nul ne vérifie pas le cumul de points/contours/profondeur. | Composite imbriqué, totaux exacts acceptés et un de moins refusé par budget. |

Validation finale : 21 contrôles, 233 tests workspace et 43 tests release. Sources et lock inchangés durant les contrôles. Voir [relevé](prototype-foundations-validation.json) et [revue complète](prototype-foundations-review.md). Aucun constat différé.
