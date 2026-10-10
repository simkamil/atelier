---
title: 'US-050 — Tables de base OpenType'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '77622b0c410ff9f5686325e81bca1e71cf383642'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.6-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le crate `atelier-font` est un scaffold ; le moteur ne peut ni ouvrir les fichiers de polices courants ni calculer les métriques et correspondances caractères-glyphes nécessaires aux stories de contours et de mise en forme.

**Approach:** Ajouter au crate une lecture sûre des conteneurs sfnt/TrueType, OpenType/CFF et TTC, puis exposer les tables de base et une API de correspondance Unicode et de métriques horizontales. Les données sont empruntées au buffer source lorsque possible.

## Boundaries & Constraints

**Always:** Accepter TTF, OTF et TTC avec sélection explicite d'un index de face (par défaut face 0). Prendre en charge `cmap` formats 4, 6, 12 et 14 ainsi que `head`, `hhea`, `hmtx`, `maxp`, `name`, `OS/2` et `post`. Valider version, décalages, longueurs, comptes, dépassements d'entiers et cohérence inter-tables avant accès. Les offsets de sous-tables `cmap` sont relatifs à `cmap`; offsets sfnt/TTC suivent leur origine prescrite. Les scalaires Unicode hors plage et glyphes hors `numGlyphs` ne doivent jamais conduire à un accès invalide. Les glyphes absents donnent `.notdef` (0); format 14 expose la résolution des séquences de variation avec distinction défaut/non-défaut. Pas de dépendance tierce, pas de panique sur entrée hostile, erreurs explicites et distinctes (format non pris en charge, signature/version invalide, entrée tronquée, offset/plage invalide, contenu incohérent).

**Never:** Décoder les contours `glyf`/CFF, appliquer GPOS/GSUB ou réaliser le shaping : ces travaux relèvent de US-051/052/053/054. Ne pas accepter silencieusement des tables critiques manquantes ou incohérentes. Ne pas charger ni redistribuer de fichiers de police dans cette story.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|----------------------------|----------------|
| SFNT single face | TTF/OTF bytes, face 0 | `FontFace` avec tables, noms, métriques et `glyph_index(char)` | `Result` sans panic |
| Collection | TTC bytes, valid face index | Face sélectionnée isolée avec offsets résolus sur l'origine TTC | Index hors collection = erreur |
| Unicode lookup | BMP/supplémentaire ou variation sequence | cmap 4/6 ou 12; cmap 14 signale défaut/non-défaut/absence | Codepoint invalide/lookup absent = 0 ou `None` selon API documentée |
| Malformed | fichier tronqué, offset overflow, length hors table, comptes/segments invalides, glyphe >= numGlyphs | Rejet au parsing ou lookup contrôlé | erreur de format précise, aucune panic/allocation basée sans borne sur données |

</frozen-after-approval>


## Code Map

- `crates/font/src/lib.rs` -- scaffold propriétaire ; implémenter le parseur public ici ou dans des modules privés de `crates/font/**`.
- `crates/font/Cargo.toml` -- crate sans dépendances externes.
- `crates/zip/src/lib.rs` -- précédent local pour plages contrôlées, `checked_add` et erreurs explicites.
- `SPEC.md` -- moteur partagé, polices, robustesse et licences applicables.
- `US-V0-V1.md` -- US-050 et dépendances immédiates US-051 à US-056.
- [OpenType font file](https://learn.microsoft.com/en-us/typography/opentype/spec/otff) -- liste des tables de base communes aux outlines TrueType et CFF.
- [cmap](https://learn.microsoft.com/en-us/typography/opentype/spec/cmap) -- records d'encodage, formats, règles de lookup et séquences de variation.
- [head](https://learn.microsoft.com/en-us/typography/opentype/spec/head), [hhea](https://learn.microsoft.com/en-us/typography/opentype/spec/hhea), [hmtx](https://learn.microsoft.com/en-us/typography/opentype/spec/hmtx), [maxp](https://learn.microsoft.com/en-us/typography/opentype/spec/maxp), [name](https://learn.microsoft.com/en-us/typography/opentype/spec/name), [OS/2](https://learn.microsoft.com/en-us/typography/opentype/spec/os2), [post](https://learn.microsoft.com/en-us/typography/opentype/spec/post) -- layouts et champs normatifs des tables prises en charge.

## Tasks & Acceptance

**Execution:**
- [x] `crates/font/**` -- parser conteneurs et tables, API `FontFace`, `FontMetrics`, `NameRecord`, `Cmap`/variation lookup, erreurs -- fournir les besoins des stories en aval.
- [x] `docs/us-050-opentype-tables.md` -- documenter API, sélections cmap, validations, limites et erreurs -- rendre le contrat utilisable par les consommateurs.
- [x] `crates/font/**` -- tests unitaires des scénarios de matrice, notamment limites TTC/cmap et tables tronquées -- verrouiller les garanties de parsing.

**Acceptance Criteria:**
- Given des fixtures valides TTF, OTF et TTC, when une face est ouverte, then toutes les tables demandées et métriques requises sont consultables sans lire hors buffer.
- Given des cmap formats 4, 6, 12 et 14 conformes, when un caractère ou une séquence variationnelle est résolu, then le résultat suit OpenType et les glyphes hors `maxp` sont rejetés proprement.
- Given des données tronquées, débordantes ou contradictoires, when le parsing/lookup est demandé, then une erreur documentée est retournée sans panic ni allocation non bornée.

## Implementation Notes

Propriété : crates/font/**, docs/us-050-opentype-tables.md et ce spec seulement. Le coordinateur gère preuves, lock, CI, Graphify et Git. Ne pas réinvoquer un skill ni créer d’agent. Graphe déjà rafraîchi : lecture uniquement. Préférence cmap 12 puis 4/6 ; noms Unicode décodés avec remplacement documenté des séquences invalides.

## Spec Change Log

## Review Triage Log

Chaque ligne constitue un verdict individuel avant regroupement.

| ID | Constat | Verdict | Preuve et route |
|---|---|---|---|
| B1 | Nom UTF-16 de longueur impaire tronqué | medium | `chunks_exact(2)` perd le dernier octet. Correction locale : refuser la longueur impaire, conserver U+FFFD pour les unités invalides. patch |
| B2 | Records de langues `name` version 1 non validés | medium | `parse_names` ne lit que les records ordinaires ; une version 1 sans langTagCount passe. Valider les plages et le stockage des tags. patch |
| B3 | Sélecteurs format 14 non triés, dupliqués ou invalides | medium | `validate14` ne contrôle que le plafond Unicode. Les sélecteurs ordinaires ou doublons rendent la résolution ambiguë. Gardes locales. patch |
| B4 | Fin de plage UVS hors Unicode | medium | Seul le début est contrôlé ; `additionalCount` peut prolonger U+10FFFF. Contrôler fin et ordre des plages. patch |
| B5 | Offset UVS pointant dans les records | medium | Une valeur non nulle peut référencer l'en-tête tant que la lecture du compte réussit. Contrôler l'origine des données et leurs bornes. patch |
| B6 | Mapping non-default sans cmap de base | false | La spécification OpenType autorise explicitement l'absence du caractère de base dans cmap pour un non-default UVS. Rejeter ce cas casserait des polices conformes. Aucune correction. |
| B7 | Formats cmap non implémentés ignorés | low | L'API n'expose que 4/6/12/14 et ne lit pas les autres formats. Aucun accès non borné ; préciser la documentation trop générale plutôt que décoder des formats supplémentaires. patch documentaire |
| B8 | Format 14 accepté sous une plateforme incorrecte | medium | `parse_cmap` affecte variation avant le classement plateforme. Exiger Unicode plateforme 0, encodage 5 pour le format 14. patch |
| B9 | Tableau d'offsets TTC incomplet accepté | medium | Seul l'offset de la face demandée est lu ; le compte complet peut déborder du buffer. Valider le tableau complet avant sélection. patch |
| B10 | Archive CI datée prétendument périmée | false | `github-ci-editing.json` conserve le SHA exact 77622b0 et les conclusions de son exécution. C'est une preuve historique immuable du lot précédent, pas le statut courant. La documentation identifie les commits. |
| E1 | Caractère ordinaire accepté comme sélecteur | medium | Même défaut que B3, vérifié à `validate14` : comparaison au seul plafond Unicode. patch |
| E2 | Octet UTF-16 final perdu | medium | Même défaut que B1, vérifié à `parse_names` : `chunks_exact(2)`. patch |
| G1 | Absence de test public de format 14 en CI | medium | Aucun des cinq tests initiaux n'appelle `variation_glyph`. Le comparateur externe confirme huit rejets de polices conformes. Ajouter des fixtures synthétiques et un test public exécuté par Cargo. patch |
| R1 | Longueur format 14 lue au mauvais offset | high | Le champ uint32 commence à +2, le code le lit à +4 ; les huit faces produites par FontTools sont rejetées. Correction locale du champ, test de non-régression. patch |
| R2 | Temps de validation UVS amplifiable par réutilisation | high | Un grand nombre de sélecteurs peut référencer le même grand tableau ; la limite de chaque compte ne borne pas la somme des itérations. Décompter un budget global et éviter de revalider un même sous-tableau cmap. patch |
| R3 | Tables maxp/post tronquées acceptées | medium | maxp 1.0 à six octets et post à quatre octets passent. Vérifier minima par version et les longueurs des variantes post exposées comme tables valides. patch |
| R4 | Cmap secondaires utilisés après absence dans le préféré | medium | `glyph_index` parcourt les autres formats si le préféré ne contient pas le caractère. OpenType demande de sélectionner un seul cmap de base. Garder le choix 12 puis 4 puis 6 et retourner zéro sans mélange. patch |
| R5 | Noms Windows legacy 3/4/5 traités comme UTF-16 | medium | La spécification name impose les pages 936/950/949 pour ces trois encodages ; le décodeur UTF-16 peut perdre ou refuser leurs octets. Utiliser la représentation octet-vers-caractère déjà documentée pour ces encodages non interprétés. patch |
| R6 | Autonomie des ressources vérifiée seulement hors CI | medium | Les sept tests initiaux du journal utilisent un codec d’arbre sans ressource ; l’oracle externe vérifie un payload avec ressource. Ajouter un test Rust qui conserve ressources et état de base malgré remplacement de l’état source. patch |
| G2 | Mutations externes limitées aux sfnt | low | La revue complémentaire des outils confirme que le glob excluait TTC et que le pilote appelait seulement parse. Correction directe : inclure two-faces.ttc et exercer parse_collection aux indices 0 et 1 pour chaque entrée. patch appliqué |

## Verification

**Commands:**
- `cargo test -p atelier-font --locked --offline` -- expected: tous les tests du crate passent.
- `cargo clippy -p atelier-font --all-targets --locked --offline -- -D warnings` -- expected: aucune alerte.
- `cargo fmt -p atelier-font -- --check` -- expected: format conforme.
- `py -m graphify .` -- expected: uniquement par le coordinateur après intégration ; l'agent d'implémentation ne relance pas le skill ni Graphify.

## Design Notes

### API de départ

`FontFace::parse(bytes: &[u8]) -> Result<FontFace<'_>, FontError>` et `FontFace::parse_collection(bytes: &[u8], face_index: usize) -> Result<FontFace<'_>, FontError>`. La face garde des tranches/table records validés. `glyph_index(char) -> u16` retourne 0 si absent; `variation_glyph(base: char, selector: char) -> VariationGlyph` différencie `Default`, `NonDefault(u16)`, `Unsupported`. `horizontal_metric(glyph_id: u16) -> Result<HorizontalMetric, FontError>` utilise les métriques longues jusqu'à `numberOfHMetrics`, puis la dernière avance avec le LSB correspondant aux glyphes restants conformément au format. Les noms sont exposés comme records avec plateforme/encodage/langue/id et octets ou chaîne décodée; OS/2 expose ses versions/longueurs sans supposer les champs absents des anciennes versions.

TTC face offsets sont lus depuis l'en-tête collection; les tables partagées ou propres à chaque face sont résolues par rapport au fichier entier. Le parser vérifie chaque vue avant de créer les sous-lecteurs. Les longueurs/counts ont des plafonds défensifs explicites documentés; une longueur qui ne tient pas dans la table engendre une erreur. Format 14 se limite à l'énumération des sélecteurs et à la résolution de la séquence; il ne remplace pas cmap de base. Les données non requises restent ignorées.

## Validation finale — 2026-10-10

Les corrections de revue sont appliquées ; B6 et B10 sont réfutés et aucun constat n’est différé. Les quatorze contrôles locaux réussissent : 187 tests workspace, 51 tests modèle/polices release, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, SPDX, garde et métadonnées des 31 paquets locaux. Lock et sources identiques avant/après. Le comparateur indépendant valide 8 faces, 25 272 mutations/troncatures de polices (TTC compris), 12 001 sélections et 267 cas de journal. Voir model-font-validation.json, model-font-oracle.json et model-font-review.md. Les tests musl sont exécutés par le job Linux distant, pas sur cet hôte Windows.
