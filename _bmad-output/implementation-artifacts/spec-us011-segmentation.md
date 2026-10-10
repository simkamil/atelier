---
title: 'Segmentation Unicode (US-011)'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: '76efdfee2124112eef5ceee21d67422b062efa80'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.2-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le curseur et le futur façonnage ont besoin de frontières de graphèmes et de mots conformes aux données Unicode déjà embarquées. **Approach:** Implémenter la segmentation étendue par défaut UAX #29 révision 47, Unicode 17.0.0, au-dessus des propriétés US-010.

## Boundaries & Constraints

**Always:** Rust sûr, aucune dépendance externe. Consommer `properties.rs`, notamment InCB et Extended_Pictographic. Appliquer toutes les règles EG et WB, dont GB9c, GB11, parité RI et exceptions WB4; conserver les caractères ignorés et leurs offsets. Source normative : https://www.unicode.org/reports/tr29/tr29-47.html.

API publique dans `segmentation` : `grapheme_boundaries(&str) -> BoundaryIndex<'_>`, `word_boundaries(&str) -> BoundaryIndex<'_>`. L'index emprunte le texte, expose `boundaries() -> &[usize]` (0 et len, une seule valeur 0 si vide), `segments()` comme itérateur double sens de `Range<usize>` demi-ouvertes. Une chaîne vide a zéro segment. Les plages couvrent exactement le texte, espaces et ponctuation compris. Chaque position est un offset d'octets UTF-8, jamais un indice de scalaire ou UTF-16.

Méthodes sur l'index : `is_boundary`, `floor_boundary`, `ceil_boundary`, `previous_boundary`, `next_boundary`, toutes avec offset `usize` et `Result<..., OffsetError>`. Floor/ceil sont inclusifs; previous/next stricts renvoient `Option<usize>` aux extrémités. Rejeter `offset > len` (OutOfBounds) et les positions au milieu d'un scalaire (NotCharBoundary), sans arrondi implicite. `forward_from(offset)` produit les segments entiers dont la fin > offset; `backward_from(offset)` produit en ordre inverse ceux dont le début < offset. Ces méthodes renvoient une erreur pour les mêmes offsets invalides. Si l'offset est dans un segment, ce segment est donc le premier dans les deux directions; à une frontière, la direction choisit le segment suivant/précédent.

Construire l'index en O(n) temps et O(n) mémoire (n scalaires/octets); conserver des états de contexte plutôt que rescanner un préfixe pour chaque frontière. Itération O(1) par segment, navigation arbitraire O(log k), k frontières. Pas de cache global. Toutes additions/indexations sûres.

**Never:** segmentation de phrases, tailoring linguistique/dictionnaires, dépendance tierce, régénération des tables UCD, édition du modèle ou du moteur shape.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Corpus officiels | Toutes lignes GraphemeBreakTest et WordBreakTest 17.0.0 | Toutes frontières ÷/× exactes en UTF-8 | Échec de test détaillé par ligne |
| Contexte | Indic linker, emoji ZWJ, RI, lettres/nombres avec ignorables | Règles normatives et itération inverse identiques | Succès |
| Navigation | Vide, 0, len, milieu d'un segment | Contrat inclusif/strict et segments entiers | Succès |
| Offset invalide | len+1, intérieur d'un scalaire multioctet | Aucune panique ni déplacement silencieux | OffsetError distincte |

</frozen-after-approval>

## Code Map

- `crates/unicode/src/properties.rs` — API statiques US-010 à consommer.
- `data/ucd/17.0.0/auxiliary/` — corpus normatifs figés.
- `crates/unicode/src/lib.rs` — raccordement du module par le coordinateur.

## Tasks & Acceptance

**Execution:**
- [x] `crates/unicode/src/segmentation.rs` — index, règles EG/WB, navigation et itérateurs.
- [x] `crates/unicode/tests/segmentation.rs` — corpus complets, navigation à toutes positions valides, inversions et mélange next/next_back, cas ciblés, longues suites de marques/RI/ignorables.
- [x] `docs/us-011-segmentation.md` — API, version, complexité et offsets.

**Acceptance Criteria:**
- Les deux corpus officiels passent à 100 %, sans filtrage ni exceptions.
- L'index peut servir le curseur depuis chaque frontière de scalaire sans recalculer la segmentation, avec un résultat inverse cohérent.
- Des suites de contexte longues ne déclenchent pas de balayages quadratiques.

## Implementation Notes

Autorisation persistante : développement parallèle avec GPT-6 Luna moyen et publication. Propriété exclusive de l'agent : les trois fichiers des tâches et cette spec (notes append-only). Le coordinateur gère exports de lib.rs, manifests, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent; ne modifier aucun autre fichier. UTF-8 explicite. Les tests peuvent utiliser Cargo ciblé; ne pas lancer cargo fmt --all pendant les autres éditions, utiliser rustfmt --edition 2024 sur les seuls fichiers possédés. Le coordinateur exécutera le format workspace final.

## Spec Change Log

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B7 | Les vecteurs de segmentation multiplient la mémoire linéaire. | low | Plusieurs vecteurs par scalaire existent, tout en respectant le contrat O(n). Aucun dépassement d'une limite annoncée n'est démontré. Réduire ce facteur constant demanderait de réécrire les états de l'algorithme ; effet négligeable pour l'usage courant. Rejeté selon la règle des optimisations mineures complexes. |
| B9 | Le relevé de validation n'empreinte pas les documents finalisés après les tests. | low | Le relevé d'exécution empreinte les sources et fixtures ; les documents sont clos ensuite. Correction directe : un relevé distinct text-bridge-documentation.json empreinte les quatorze documents finalisés et le relevé d'exécution associé, sans prétendre exécuter des tests sur la prose. |

Chaque constat est classé avant regroupement ; les patches internes sont validés par les contrôles finaux. La [revue commune](text-bridge-review.md) conserve les treize constats et les contrôles supplémentaires du coordinateur.

## Verification

- `cargo test -p atelier-unicode --locked --offline` — corpus et régressions passent.
- `cargo check -p atelier-unicode --target wasm32-unknown-unknown --locked --offline` — compile portable.

Validation finale : 25 contrôles, 260 tests workspace, 70 tests release. Les sources et le lock sont restés identiques pendant les contrôles. Voir [relevé](text-bridge-validation.json) et [revue triée](text-bridge-review.md). Aucun constat différé.
