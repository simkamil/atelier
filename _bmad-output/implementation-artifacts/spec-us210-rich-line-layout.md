---
title: 'US-210 — Mise en page des lignes riches'
type: 'feature'
created: '2026-10-12'
status: 'in-progress'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '512e3651dad18f19a9eb133d8658e3d19461ccd6'
context: []
---

<frozen-after-approval reason="US-210 explicitement demandée par l’utilisateur">

## Intent

Construire le moteur de lignes riches US-210 dans atelier-text-layout. Il consomme paragraphes et propriétés effectives US-201, forme le texte avec les polices US-055/054 et produit des lignes et fragments déterministes, sans remplacer implicitement l’éditeur de texte brut.

## Boundaries & Constraints

Toujours : Rust sûr et dépendances locales, unités explicites (propriétés twips, sortie 1/64 pt), offsets UTF-8 à frontières graphemiques, limites de texte/glyphes/lignes/travail avec erreurs typées, calculs vérifiés. Préserver shaping, ligatures/kerning et clusters; mesurer le texte réellement mis en ligne. Le mot trop long est coupé seulement aux frontières graphemiques sûres. Les coupes obligatoires restent obligatoires; la fin de paragraphe et la dernière ligne ne sont pas justifiées.

Périmètre : coupure gloutonne UAX #14, alignements gauche/centre/droite/justifié, retraits gauche/droit/première ligne/suspendu; interlignes simple, 1,5, double, multiple, exact/au moins; mélange de tailles et de polices, baseline et hauteur correctes. Sortie géométrique exploitable. Les tabulations avancées US-211, pagination US-212, édition géométrique US-214, RTL/césure linguistique et UI restent séparées; les contrôles non pris en charge sont signalés/documentés sans fausse fidélité.

Le critère original ≥95% de lignes identiques à Word reste obligatoire pour fermer US-210. Si aucun oracle Word authentique n’est disponible, livrer le code testé et conserver ce critère explicitement ouvert; ne jamais fabriquer de référence ou substituer notre moteur à l’oracle.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat | Erreur |
|---|---|---|---|
| Coupures | Espaces/NBSP/ZWSP, LF/CRLF, texte vide et mot large | UAX14, blancs finaux gérés, progrès garanti, pas de grapheme scindé | Budget dépassé : erreur |
| Géométrie | Quatre alignements, retraits positifs/négatifs, ligne finale | Positions et largeur vérifiables, justification répartie sans modifier la finale | Largeur/arithmétique invalide : erreur typée |
| Richesse | Runs/polices/tailles mixtes, ligature et combining mark | Glyphes/clusters cohérents, baseline commune et extents de faces réelles | Police/glyphe absent : politique explicite |
| Interligne | Auto240/360/480/autre, exact/atleast | Hauteur et baseline respectent les métriques et la règle | Valeur invalide : erreur |
| Référence | Corpus Word indépendant avec provenance | Comparateur exact des coupures, taux ≥95% pour clôture | Oracle absent : preuve indisponibilité et critère ouvert |

</frozen-after-approval>

## Code Map

- `crates/text-layout/src/lib.rs` : scaffold vide, point d’entrée à développer.
- `crates/text-model/src/rich` : Paragraph, CharAttrs/ParaProps/ResolvedProperties et StyleEngine; consommer propriétés résolues, préserver le modèle existant.
- `crates/unicode/src/line_break.rs` : opportunités en offsets UTF-8, Mandatory et Allowed. Segmentation : grapheme_boundaries.
- `crates/shape/src/lib.rs` : LatinShaper::shape_text, tailles 1/64 pt, clusters en indices scalaires à convertir en offsets UTF-8.
- `crates/font/src/{manager,lib}.rs` : FontManager, FontFace et métriques de faces.
- `docs/us-054-latin-shaping.md`, `docs/us-055-font-manager.md`, `docs/us-012-line-breaks.md` : contrats existants.

## Tasks & Acceptance

**Execution:**
- [x] `crates/text-layout/src` : API publique, moteur de lignes, métriques, limites/erreurs et sortie glyphes/fragments.
- [x] `crates/text-layout/tests` : géométrie chiffrée et cas limites de toute la matrice, sans oracle circulaire.
- [x] `docs/us-210-rich-line-layout.md` : usage/units/limites et état vérifié du critère Word, sans modifier le backlog global.

**Acceptance Criteria:**
- Tous les critères fonctionnels originaux US-210 sont démontrés par tests exécutés.
- L’entrée intègre les résultats de StyleEngine et de US-301 sans dépendance croisée entre importeur et layout. Contrat suggéré : `Paragraph` aux runs résolus, `ResolvedProperties` par défaut et `FontManager`; coordonner avec l’agent DOCX.
- Le corpus Word et le seuil sont prouvés, ou le suivi conserve US-210 partielle avec ce seul critère ouvert et une preuve explicite de l’absence d’oracle.

## Implementation Notes

Le coordinateur gère manifests/lock, docs globales, Graphify, CI et intégration. Demander les dépendances locales nécessaires sans toucher ces fichiers ni le modèle partagé sans coordination. Propriété agent : crates/text-layout/src et tests, doc US-210, présente spec hors bloc gelé. Un agent oracle prépare tests/text-layout-reference; coordonner sans écraser ses fichiers. Pas de commit/push. Lire graphify-out/GRAPH_REPORT.md avant analyse; refresh initial de session fait par coordinateur, ne pas lancer en concurrence. Cargo --locked --offline.

## Spec Change Log

## Review Triage Log

Trois revues indépendantes Luna moyen puis revue ciblée des correctifs. Tous les constats sont conservés séparément; décisions et preuves détaillées dans [le reçu](docx-rich-layout-review.md).

| ID | Observation | Verdict | Preuve et traitement |
|---|---|---|---|
| B1 | Offsets UAX14 potentiellement scalaires, indexation incorrecte | false | `atelier_unicode::line_break::BreakOpportunity.offset` et `line_breaks` utilisent des offsets d'octets UTF-8, bornés par `text.len()`. Le vecteur `len + 1` convient. |
| B2 | Limite glyphes appliquée au paragraphe entier | false | `LayoutOptions.max_glyphs` borne la sortie du paragraphe entier, et `total_glyphs` applique le même contrat. Il n'existe pas de budget par ligne promis. |
| B3 | Exactement max_lines lignes échouent | false | La dernière ligne interrompt la boucle avant le prochain contrôle. Ajouter la ligne vide d'un séparateur terminal devra conserver cette propriété; assertion ajoutée dans le correctif R1. |
| B4 | Clusters vides/non graphemiques rejetés à tort | false | `shape_range` convertit les indices scalaires du shaper puis arrondit aux frontières graphemiques avant `boundary_index`. Le LatinShaper ne publie pas de cluster vide pour un glyphe. |
| B5 | Largeur du paragraphe oublie les retraits | false | L'agrégat utilise `x + line_width`; `x` inclut les retraits et l'alignement. La largeur occupée n'est pas la largeur de contrainte. |
| B6 | Justification de la ligne finale/obligatoire | false | `justify = alignment == Justify && !final_line && !mandatory_break` protège les deux cas. Assertion de coupure obligatoire ajoutée avec R1. |
| B7 | Repli de police par graphème incorrect | medium | `build_spans` sélectionne via `resolve_for_char`, puis `build_shapers` refait `resolve` sans couverture : IDs différents et erreur malgré une face couvrante. Patch ciblé et régression par l'agent US-210. |
| B8 | Spans de shaping scindent les graphèmes | false | `Paragraph::validate_local` vérifie les runs; les spans sont construits avec `boundaries.windows(2)`, les séparateurs sont exclus, les candidats sont reshaped sur des frontières validées. R2 concerne une autre cause en amont dans l'importeur. |
| B9 | Le collecteur Word peut écraser un JSON arbitraire | medium | Le chemin JSON fourni était transmis à `Set-Content` sans confinement ni contrôle d'existence. Correction : répertoires de résultats autorisés, extension JSON, refus de l'existant et écriture atomique `CreateNew`. Le mode de collecte Word reste non exécuté ici. |
| B10 | Le collecteur retire le marqueur de paragraphe | false | Les lignes sont explicitement locales au texte du paragraphe; le modèle `Paragraph` n'inclut pas le marqueur structurel Word. La liste de paragraphes, leurs longueurs et les positions Word distinguent leurs fins. Ne pas ajouter un CR synthétique au corpus comparé. |
| V1 | Espacement avant jamais appliqué | false | `vertical` est initialisé avec `twips_to_units(para.space_before)` avant la boucle. `space_after` est ensuite ajouté à la hauteur. Ajout d'une assertion numérique avant/après pour protéger ce contrat. |
| R1 | Une coupure obligatoire terminale perd la ligne vide suivante | medium | `final_line = end_slot == boundaries.len() - 1` assimilait `x\n` à une seule ligne finale. Conserver une ligne vide finale avec métriques, indicateurs et budget cohérents. Patch par l'agent US-210. |
| R2 | Runs formatés qui partagent un graphème : mutation/déduplication incorrecte | high | `dedup_by` avançait le début du run précédent jusqu'à la fin du suivant, pouvant supprimer ses attributs ou créer une plage inversée. Normaliser selon le propriétaire du premier scalaire et préserver les portions ultérieures. Patch par l'agent US-301. |
| R3 | Texte visible des conteneurs inline ignoré | high | La boucle des seuls enfants directs `w:r` omettait les runs de `w:hyperlink`. Parcourir les conteneurs transparents connus en ordre source, sans importer instructions, suppressions ou faux namespaces; garder notices et paquet opaque. Patch par l'agent US-301. |
| R4 | Taille complexe écrase la taille latine; nombres mal formés ignorés | medium | Les branches `sz | szCs` partageaient le champ latin et plusieurs `parse().ok()` effaçaient les erreurs. Garder `w:sz`, conserver/signaler `szCs`, refuser les lexèmes invalides des propriétés prises en charge. Patch par l'agent US-301. |
| R5 | Type de relation d'une police reconnu par suffixe | medium | `ends_with("/font")` acceptait `urn:fake/font`; reconnaissance par les deux URI OOXML authentiques requise. Patch et assertion par l'agent US-301. |
| R6 | Réinitialisations couleur automatique/surlignage aucun non représentables par le modèle actuel | medium | Les marqueurs opaques étaient conservés mais le cascadeur ne les interprète pas. Le modèle distingue l'absence d'un booléen de faux, mais pas l'absence d'une couleur de sa réinitialisation. Rendre cette limite explicite par notice et documentation, préserver le marqueur et protéger par test; aucun rendu fidèle de ces resets n'est revendiqué. Une représentation de reset dans le modèle partagé relève d'un travail ultérieur coordonné. |
| R7 | Recherche des runs quadratique lors de la normalisation/résolution | medium | Pour chaque graphème, `source_runs.iter().find` repartait du début et `resolved_paragraph` parcourait tous les runs du document. Curseurs monotones sur les plages triées et filtrage unique du paragraphe conservent le propriétaire du premier scalaire sans ces rescans. Patch et régression de styles alternés par l'agent US-301. |
| R8 | Reproduction binaire de la fixture dépendante de zlib | low | `--check` compare les octets exacts du ZIP, alors que DEFLATE dépend du runtime zlib du générateur et de la CI. Les entrées ZIP stockées suppriment cette dépendance; métadonnées, contenu et contrôle indépendant restent déterministes. Correction directe du générateur et régénération vérifiée. |

## Verification

- `cargo test -p atelier-text-layout --locked --offline` : réussi (5 tests d’intégration DOCX et 16 tests de layout).
- `cargo clippy -p atelier-text-layout --lib --locked --offline -- -D warnings` : réussi.
- Disponibilité de l’oracle : `_bmad-output/implementation-artifacts/us-210-word-availability.json` et `us-210-word-com-check.json` confirment que Word n’est pas installé/enregistré; aucune ligne de référence n’a été collectée. Le critère ≥95 % reste ouvert et US-210 partielle.
- Coordinateur : import→layout, fmt/clippy, workspace Windows/WASM/Linux, revue indépendante et preuve Word disponible ou limite explicite.

### Validation finale — 2026-10-12

15 contrôles réussis, 540 tests workspace; Windows/WASM/musl, Clippy hôte/WASM, format, licences, garde et fixture indépendante. Sources et lock inchangés pendant la passe. Reçu : `docx-rich-layout-validation.json`. US-301 est validée au périmètre API; US-210 reste partielle, seuil Word ≥95 % ouvert. Les resets couleur/surlignage non représentables sont préservés et signalés, sans fidélité visuelle revendiquée.
