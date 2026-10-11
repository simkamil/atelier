---
title: 'Opportunités de coupure Unicode (US-012)'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: '6f4b0b34ab39d5a15a73d21f615d2cbf75c7edf6'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.2-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** La mise en page ne dispose pas des opportunités de coupure de ligne. **Approach:** Implémenter l'algorithme Unicode 17.0 UAX #14 révision 55 par défaut, sans adaptation locale, sur les propriétés déjà figées, afin de débloquer US-120.

## Boundaries & Constraints

**Always:** Module public `atelier_unicode::line_break` en Rust sûr, sans dépendance externe. API `line_breaks(text: &str) -> Vec<LineBreak>`, `LineBreak { offset: usize, kind: BreakKind }`, `BreakKind::{Allowed, Mandatory}`. Offsets octets UTF-8 strictement croissants et frontières de scalaires uniquement. Aucun offset initial 0 pour un texte non vide (LB2), fin toujours présente Mandatory (LB3), texte vide = un terminal Mandatory à 0. N'exposer que les coupures permises et obligatoires, jamais une coupure interdite. CR×LF est indivisible et sa fin obligatoire ; BK/CR/LF/NL distinguent les coupures obligatoires. Ne pas confondre une opportunité avec le choix d'une ligne ou une césure.

Appliquer les règles LB1–LB31 ordonnées, y compris les règles ajoutées dans Unicode17 (HH, guillemets dépendant du contexte EastAsian, orthographic syllables). Résolution LB1 par défaut : AI/SG/XX→AL, SA Mn/Mc→CM sinon AL, CJ→NS. Aucune résolution AI locale selon la largeur. Consommer `general_category`, `line_break_class`, `is_extended_pictographic` et la nouvelle propriété `east_asian_width` du coordinateur. U+00A0/U+202F suivent GL et toutes les exceptions LB12a (SP/BA/HY/HH) ; pas de tailoring français.

Temps O(n) et mémoire O(n) en scalaires ; précompter les contextes nécessaires (espaces, RI, combinaisons, nombres, guillemets), aucun rebalayage arrière/avant d'une longue séquence à chaque frontière. Ne jamais consulter les commentaires du corpus pour décider des résultats. Le texte `&str` est déjà UTF-8 valide ; aucune panique sur un scalaire valide.

**Never:** choisir les ruptures selon une largeur, hyphenation, dictionnaire thaï, shaping, bidi, insertion d'espaces françaises, modification des sources officielles ou du segmentateur US-011.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Conformité | 19 338 lignes LineBreakTest Unicode17 | Exactement les frontières attendues, sans exception | Échec du test au premier écart avec contexte |
| Contrôles | CRLF, BK, LF, CR, NEL, LS/PS | Coupures obligatoires, pas entre CRLF | Sans panique |
| Espaces | NBSP/NNBSP, SP précédant GL, WJ, ZW/ZWJ | Règles par défaut et exceptions | Sans adaptation |
| Contextes longs | espaces, marques, RI, numérique, guillemets | Résultats exacts en temps linéaire | Aucun rescannage quadratique |
| Bornes | vide, ASCII et multioctets | Terminal unique, offsets UTF-8 valides | Sans doublon |

</frozen-after-approval>

## Code Map

- `crates/unicode/src/properties.rs` — propriétés UCD ; le coordinateur ajoute east_asian_width(char) -> &'static str et sa table générée.
- `crates/unicode/src/segmentation.rs` — API existante en offsets octets ; ne pas modifier.
- `data/ucd/17.0.0/auxiliary/LineBreakTest.txt` — corpus intégral officiel déjà vérifié par manifeste.
- Norme primaire : https://www.unicode.org/reports/tr14/tr14-55.html ; lire §6 et l'ordre des règles.

## Tasks & Acceptance

**Execution:**
- [x] `crates/unicode/src/line_break.rs` — calcul des frontières et types publics documentés.
- [x] `crates/unicode/tests/line_break.rs` — corpus intégral, statut obligatoire et toutes lignes de la matrice.
- [x] `docs/us-012-line-breaks.md` — API, version/révision, unités, complexité et limites fonctionnelles.

**Acceptance Criteria:**
- Given le corpus officiel figé, when l'API segmente chaque cas, then 19 338 cas concordent sans filtrage.
- Given des contextes longs adversariaux, when chaque frontière est décidée, then l'algorithme ne rescane pas les mêmes runs et les sorties restent ordonnées.

## Implementation Notes

Autorisation persistante de développement parallèle GPT-6 Luna moyen, validation puis push. Propriété exclusive : les trois fichiers des tâches et notes append-only de cette spec. Le coordinateur gère lib.rs, propriétés/générateur/tables, manifests, CI, preuves, Graphify et Git. Il fournira east_asian_width pendant le développement. Ne lancer ni skill, ni Graphify, ni sous-agent ; ne modifier aucun autre fichier. UTF-8 explicite. Cargo ciblé autorisé une fois la propriété ajoutée. rustfmt --edition 2024 sur fichiers possédés seulement ; format workspace final par coordinateur.

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

- `cargo test -p atelier-unicode --test line_break --locked --offline` — corpus et cas ciblés passent.
- `cargo clippy -p atelier-unicode --test line_break --locked --offline -- -D warnings` — aucun avertissement.

Note de coordination : les agents ont développé les fichiers en parallèle. Après la limite d’usage Luna, le coordinateur a finalisé les corrections et les vérifications. Les statuts de fin attendent la revue et la validation du lot.

Validation finale : 26 contrôles, 291 tests workspace et 101 tests release. Sources et Cargo.lock identiques pendant les contrôles. Les critères sont couverts par les tests publics ciblés, les oracles indépendants et le [relevé complet](text-layout-validation.json). La [revue triée](text-layout-review.md) ne conserve aucun constat différé.
