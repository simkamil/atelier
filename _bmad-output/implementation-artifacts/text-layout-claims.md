# Relevé du lot US-012/054/055 — 2026-10-11

Le lot implémente trois primitives du démonstrateur, avec trois agents GPT-6 Luna moyen indépendants : coupures Unicode par défaut (UAX14 Unicode17), orchestrateur Latin LTR à métriques fixes, registre de faces empruntées et résolution de familles/styles/sources.

Les contrats complets ci-dessous sont les spécifications de chaque US. Les statuts restent ouverts jusqu'à la revue et la validation complète sur sources inchangées. Le corpus officiel LineBreakTest doit être exécuté intégralement (19 338 cas), les tables EAW comparées sur tous les scalaires, et l'orchestrateur comparé aux 54 716 sorties HarfBuzz indépendantes. Une preuve distincte utilise les quatre faces Arial réellement installées sur Windows, sans redistribuer leurs octets.

Le registre accepte les octets système par son API de provenance. Le pont Host/natif US-113 reste à développer, ainsi que le Worker US-092 et l'interface US-120. La table de substitutions applique les correspondances spécifiées ; elle ne constitue pas une mesure d'identité métrique avec les polices Microsoft.


---

---
title: 'Opportunités de coupure Unicode (US-012)'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
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
- [ ] `crates/unicode/src/line_break.rs` — calcul des frontières et types publics documentés.
- [ ] `crates/unicode/tests/line_break.rs` — corpus intégral, statut obligatoire et toutes lignes de la matrice.
- [ ] `docs/us-012-line-breaks.md` — API, version/révision, unités, complexité et limites fonctionnelles.

**Acceptance Criteria:**
- Given le corpus officiel figé, when l'API segmente chaque cas, then 19 338 cas concordent sans filtrage.
- Given des contextes longs adversariaux, when chaque frontière est décidée, then l'algorithme ne rescane pas les mêmes runs et les sorties restent ordonnées.

## Implementation Notes

Autorisation persistante de développement parallèle GPT-6 Luna moyen, validation puis push. Propriété exclusive : les trois fichiers des tâches et notes append-only de cette spec. Le coordinateur gère lib.rs, propriétés/générateur/tables, manifests, CI, preuves, Graphify et Git. Il fournira east_asian_width pendant le développement. Ne lancer ni skill, ni Graphify, ni sous-agent ; ne modifier aucun autre fichier. UTF-8 explicite. Cargo ciblé autorisé une fois la propriété ajoutée. rustfmt --edition 2024 sur fichiers possédés seulement ; format workspace final par coordinateur.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-unicode --test line_break --locked --offline` — corpus et cas ciblés passent.
- `cargo clippy -p atelier-unicode --test line_break --locked --offline -- -D warnings` — aucun avertissement.

---

---
title: 'Mise en forme du texte latin (US-054)'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
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
- [ ] `crates/shape/src/lib.rs` — façade, types, calculs bornés, options documentées.
- [ ] `crates/shape/tests/latin.rs` — matrice entière, marques/ligatures, séquences génériques, limites et erreurs.
- [ ] `docs/us-054-latin-shaping.md` — API, unités, arrondi, interlettrage et limites.

**Acceptance Criteria:**
- Given les fontes réelles et textes de référence, when cmap/GSUB/GPOS/échelle s'appliquent, then chaque glyphe, cluster et position concorde avec l'oracle indépendant.
- Given une séquence générique de glyphes, when shaping est appelé, then elle ne dépend pas de caractères reconstruits et conserve ses plages source.

## Implementation Notes

Autorisation persistante : agents GPT-6 Luna moyen en parallèle, validation et push. Propriété exclusive : trois fichiers des tâches et notes append-only ici. Root gère Cargo.toml et lock, exports/manifests, corpus d'oracle complet, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent ; aucun autre fichier modifié. Cargo ciblé après ajout des deux dépendances par root ; rustfmt --edition 2024 sur fichiers possédés uniquement. Les décisions d'unité/espacement sont techniques, compatibles avec la SPEC ; aucun nouveau choix utilisateur requis.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-shape --locked --offline` — cas ciblés et références passent.
- `cargo clippy -p atelier-shape --all-targets --locked --offline -- -D warnings` — aucun avertissement.

---

---
title: 'Gestionnaire de polices (US-055)'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
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
- [ ] `crates/font/src/manager.rs` — registre, résolution, métadonnées et limites.
- [ ] `crates/font/tests/manager.rs` — toutes lignes de la matrice, vrais TTF/TTC, vingt faces et aliases.
- [ ] `docs/us-055-font-manager.md` — API, règles exactes et frontière avec le futur Host.

**Acceptance Criteria:**
- Given les vingt fontes, when chaque alias et variante est demandé, then la face réellement sélectionnée est correcte et consultable.
- Given des octets système/document et plusieurs faces TTC, when l'appelant les inscrit, then leur source et index restent exacts sans dépendre de l'OS.
- Given une inscription refusée ou une suppression, when une nouvelle résolution survient, then les autres IDs restent stables et le budget cohérent.

## Implementation Notes

Autorisation persistante : dev parallèle GPT-6 Luna moyen puis validation/push. Propriété exclusive : trois fichiers des tâches et notes append-only de cette spec. Root gère export lib.rs, manifests, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent, ne modifier aucun autre fichier. Prévenir root quand manager.rs existe pour l'export avant tests. UTF-8 explicite ; Cargo ciblé autorisé ; rustfmt --edition 2024 sur fichiers possédés uniquement. US-092 reste ouverte : cette vague termine d'abord les primitives texte directement requises par US-120.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-font --test manager --locked --offline` — résolution et erreurs passent.
- `cargo clippy -p atelier-font --test manager --locked --offline -- -D warnings` — aucun avertissement.

Clôture du lot : 32 US terminées sur 158. Les 26 contrôles finaux passent, avec 291 tests workspace et 101 tests release sur sources inchangées. La revue triée ne laisse aucun constat différé. Les contrats reproduits plus haut sont ceux reçus en revue ; leurs statuts historiques in-review précèdent cette clôture.
