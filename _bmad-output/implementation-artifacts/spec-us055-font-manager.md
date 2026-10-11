---
title: 'Gestionnaire de polices (US-055)'
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

**Problem:** Le moteur ne sait pas sélectionner une police réelle depuis la famille demandée et ses replis. **Approach:** Ajouter un registre déterministe, empruntant les fontes embarquées, système et document pour exposer la face effective à la mise en page et à l'interface.

## Boundaries & Constraints

**Always:** Module `atelier_font::manager` en Rust sûr, sans dépendance nouvelle. `FontManager<'a>` conserve les FontFace empruntées, identifiants stables non réutilisés, métadonnées de famille/poids/style et source. `new()` charge les vingt faces EMBEDDED_FONTS ; variante vide et variante avec limites pour les tests/appelants. `register(bytes: &'a [u8], face_index: usize, source: FontSource) -> Result<FontId, ManagerError>` accepte de vrais octets TTF/TTC ; System est alimenté par les octets fournis par le futur Host/pont natif, Document par l'importeur et Embedded par le catalogue. Aucune énumération système factice ni accès OS/disque dans ce module. `remove(FontId)` retire une entrée sans invalider les autres IDs et sans réutiliser l'ID ; registre et budget inchangés si inscription échoue.

Types publics FontSource::{Embedded,System,Document}, FontStyle::{Normal,Italic,Oblique}, GenericFamily::{SansSerif,Serif,Monospace}, FontRequest avec liste ordonnée de familles, poids u16 (1..=1000), style et générique. `resolve(&FontRequest)` retourne une référence à la face avec FontId, famille effective, poids/style, source et face_index ; `resolve_for_char(...,char)` ajoute la sélection de couverture par cmap. Si rien ne couvre, erreur explicite MissingGlyph, jamais prétendre qu'un .notdef couvre le caractère. Registre vide ou requête invalide donnent erreurs typées.

Noms de familles : préférer ID16 typographique, sinon ID1 ; reconnaître les noms Unicode localisés comme clés. Sélection de la famille affichée déterministe (anglais Unicode Windows en priorité, puis autre Unicode selon clé plateforme/langue/valeur stable). Normalisation des clés : trim/collapse des blancs et minuscules Unicode via Rust ; pas de dépendance Unicode supplémentaire. Poids OS/2 usWeightClass, sinon 400 ; refuser hors1..1000. Style depuis OS/2 fsSelection (italic bit0, oblique bit9 version>=4), sinon head macStyle italic ; ne pas déduire d'un nom arbitraire. FontFace::table donne les octets validés, tout champ encore lu doit être borné.

Sélection : familles demandées dans l'ordre ; pour chacune famille exacte puis alias métrique. Dans une famille, sources Document→System→Embedded ; dans une source, style exact, puis proximité Normal→Oblique→Italic, Italic→Oblique→Normal, Oblique→Italic→Normal. Poids selon CSS Fonts : demande <400 cherche vers le bas puis vers le haut ; >500 vers le haut puis vers le bas ; 400..500 cherche vers500 puis vers le bas puis au-dessus500. Égalité départagée par ordre d'inscription. Pour resolve_for_char, filtrer les faces non couvrantes avant cette sélection puis essayer la famille suivante/alias/générique.

Alias exacts insensibles à la casse : Calibri→Carlito, Cambria→Caladea, Arial→Liberation Sans, Times New Roman→Liberation Serif, Courier New→Liberation Mono. Génériques finaux : SansSerif→Carlito, Serif→Caladea, Monospace→Liberation Mono. Si famille générique absente, choisir la première autre famille couvrante disponible de façon stable ; si aucune face, erreur. La table applique les correspondances métriques spécifiées ; ne pas prétendre mesurer l'équivalence aux fontes Microsoft non redistribuées.

Limites publiques par défaut : 1024 faces et 256MiB de tailles de fichiers cumulées par inscription (un TTC inscrit deux fois compte deux fois). Valider avant parsing et allocation ; incréments checked. Supprimer une face rend son budget disponible. Exposer métadonnées et FontFace sans dupliquer ses octets.

**Never:** charger les fontes système automatiquement, modifier les TTF, inventer le pont US-113, synthèse bold/italic, shaping ou cache glyphes, preuve de comparaison aux fontes Microsoft absentes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Aliases | Cinq familles Microsoft, quatre variantes | Face embarquée correcte et source consultable | Succès |
| Sources/TTC | Vrais octets TTF/TTC, sources distinctes | Priorité stable, bonne face_index et ID | Index invalide refusé |
| Proximité | Poids intermédiaires/styles absents | Ordre CSS fixé, style puis poids | Requête hors bornes refusée |
| Couverture | Glyphe absent de la première famille | Face couvrante suivante/générique | MissingGlyph si aucune |
| Atomicité | Données malformées, limite exacte/+1, suppression | Registre conservé et IDs stables | Erreurs typées, aucune panique |

</frozen-after-approval>

## Code Map

- `crates/font/src/lib.rs` — FontFace::parse_collection/table/names/metrics/glyph_index ; NameRecord expose plateforme, langue et IDs.
- `crates/font/src/embedded.rs` — vingt TTF statiques OFL, EMBEDDED_FONTS.
- `crates/font/tests/fixtures/` — vrais TTF/TTC synthétiques déjà générés, ne pas modifier.
- Références primaires : Microsoft OpenType name et OS/2 ; CSS Fonts4 font-matching-algorithm.

## Tasks & Acceptance

**Execution:**
- [x] `crates/font/src/manager.rs` — registre, résolution, métadonnées et limites.
- [x] `crates/font/tests/manager.rs` — toutes lignes de la matrice, vrais TTF/TTC, vingt faces et aliases.
- [x] `docs/us-055-font-manager.md` — API, règles exactes et frontière avec le futur Host.

**Acceptance Criteria:**
- Given les vingt fontes, when chaque alias et variante est demandé, then la face réellement sélectionnée est correcte et consultable.
- Given des octets système/document et plusieurs faces TTC, when l'appelant les inscrit, then leur source et index restent exacts sans dépendre de l'OS.
- Given une inscription refusée ou une suppression, when une nouvelle résolution survient, then les autres IDs restent stables et le budget cohérent.

## Implementation Notes

Autorisation persistante : dev parallèle GPT-6 Luna moyen puis validation/push. Propriété exclusive : trois fichiers des tâches et notes append-only de cette spec. Root gère export lib.rs, manifests, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent, ne modifier aucun autre fichier. Prévenir root quand manager.rs existe pour l'export avant tests. UTF-8 explicite ; Cargo ciblé autorisé ; rustfmt --edition 2024 sur fichiers possédés uniquement. US-092 reste ouverte : cette vague termine d'abord les primitives texte directement requises par US-120.

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

- `cargo test -p atelier-font --test manager --locked --offline` — résolution et erreurs passent.
- `cargo clippy -p atelier-font --test manager --locked --offline -- -D warnings` — aucun avertissement.

Note de coordination : les agents ont développé les fichiers en parallèle. Après la limite d’usage Luna, le coordinateur a finalisé les corrections et les vérifications. Les statuts de fin attendent la revue et la validation du lot.

Validation finale : 26 contrôles, 291 tests workspace et 101 tests release. Sources et Cargo.lock identiques pendant les contrôles. Les critères sont couverts par les tests publics ciblés, les oracles indépendants et le [relevé complet](text-layout-validation.json). La [revue triée](text-layout-review.md) ne conserve aucun constat différé.
