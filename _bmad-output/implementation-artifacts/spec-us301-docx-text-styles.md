---
title: 'US-301 — Import DOCX : texte, propriétés, styles et polices'
type: 'feature'
created: '2026-10-12'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '512e3651dad18f19a9eb133d8658e3d19461ccd6'
context: []
---

<frozen-after-approval reason="US-301 explicitement demandée par l’utilisateur">

## Intent

Importer les paragraphes DOCX dans le modèle riche, avec leurs propriétés et références de styles, thèmes, paramètres et polices incorporées. Fournir une API utilisable par le futur éditeur et par US-210, sans prétendre que l’interface actuelle ouvre déjà le document riche.

## Boundaries & Constraints

Toujours : Rust sûr, dépendances locales uniquement, import déterministe, erreurs typées, limites de ressources explicites, aucun accès externe. Conserver le paquet original et les références/propriétés non interprétées pour les travaux ultérieurs. Respecter les frontières graphemiques du modèle, même quand un fichier découpe un grapheme entre plusieurs runs. Les attributs absents restent distincts des valeurs explicites fausses. Styles et thèmes sont résolus avec US-201.

Périmètre : paragraphes/runs, texte avec espaces, sauts et leurs types, tabs, symboles; propriétés de caractère/paragraphe représentées par le modèle; docDefaults, styles basés/liés et latents; thèmes couleurs/polices; paramètres de tabulation par défaut et compatibilité; fontTable et ODTTF. Les structures des US-302 à 307 restent opaques ou signalées explicitement, sans effacement silencieux du paquet. Aucun export sémantique, rendu UI ou changement des autres worktrees.

## I/O & Edge-Case Matrix

| Scénario | Entrée | Résultat attendu | Erreur |
|---|---|---|---|
| Texte | Runs, espaces, Unicode, br/cr/tab/sym | Texte fidèle, offsets UTF-8 valides et types de sauts conservés | Symbole mal formé : erreur contextualisée |
| Styles | Défauts, héritage, rStyle/pStyle, overrides faux, thèmes | Références conservées et propriétés effectives cohérentes avec US-201 | Cycle/référence absente : erreur ou avertissement explicite documenté |
| Annexes | Styles latents, settings, fontTable, polices embarquées | Métadonnées accessibles; ODTTF désobfusqué et validé par atelier-font | Clé/font/liaison invalide : erreur typée |
| XML | Préfixes arbitraires, éléments inconnus, faux namespace | Reconnaissance par URI, inconnus préservés, aucun faux texte | Structure invalide/ref externe nécessaire : refus clair |
| Budgets | XML/texte/runs/styles/fonts excessifs | Refus borné et déterministe, sans résultat partiel publié | Erreur de limite |

</frozen-after-approval>

## Code Map

- `crates/fmt-ooxml/src/lib.rs` : Package, PartUri, relations résolues, limites; garder la couche OPC intacte.
- `crates/xml/src/lib.rs` : lecteur namespace-aware et capture opaque, budgets XML.
- `crates/text-model/src/rich/{paragraph,document,styles,attrs}.rs` : Paragraph, RichDocument, RichOp et StyleEngine. Runs aux frontières graphemiques; pas de champ rStyle : utiliser métadonnées d’import au besoin.
- `crates/font/src/manager.rs` : FontManager emprunte les octets; le résultat d’import doit posséder les octets de polices et permettre leur inscription sans structure auto-référente.
- `docs/us-300-opc-package.md`, `docs/us-201-style-engine.md`, `US-V0-V1.md` : contrats et critères originaux.

## Tasks & Acceptance

**Execution:**
- [x] `crates/fmt-ooxml/src/docx*.rs` et sous-modules : API d’import, texte/propriétés/styles/thème/settings/fontTable/ODTTF.
- [x] `crates/fmt-ooxml/tests` : fixtures autonomes couvrant chaque ligne de la matrice, vrais octets de police libre du dépôt, erreurs et budgets.
- [x] `docs/us-301-docx-import.md` : API et limites honnêtes, sans cocher le backlog global (coordinateur après validation).

**Acceptance Criteria:**
- Un paquet DOCX valide produit un RichDocument validé, un moteur de styles et les métadonnées d’import, sans perdre les parties inconnues.
- Les trois critères originaux US-301 sont démontrés par tests exécutés; une police obfusquée chargée est réellement utilisable par FontManager/LatinShaper.
- Un paragraphe importé peut fournir ses propriétés effectives à US-210. Contrat suggéré : helper retournant Paragraph avec runs résolus et ResolvedProperties par défaut; coordonner avec l’autre agent, sans dépendance fmt-ooxml→text-layout.

## Implementation Notes

Le coordinateur gère les manifests Cargo, lockfile, docs globales, CI, Graphify et intégration. L’agent peut demander les dépendances locales nécessaires, mais ne modifie pas ces fichiers ni le modèle partagé sans coordination. Propriété de l’agent : modules DOCX, tests DOCX, doc US-301 et présente spec hors bloc gelé. Pas de commit/push. Lire graphify-out/GRAPH_REPORT.md avant l’analyse; rafraîchissement de session déjà fait par coordinateur, ne pas relancer en concurrence. Utiliser cargo --locked --offline après installation des dépendances coordonnées.

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

- `cargo test -p atelier-fmt-ooxml --locked --offline` : tous tests OPC et import passent.
- Coordinateur : fmt/clippy, workspace Windows/WASM/Linux, référence indépendante ODTTF, import→layout, revue indépendante et documentation.

### Validation finale — 2026-10-12

15 contrôles réussis, 540 tests workspace; Windows/WASM/musl, Clippy hôte/WASM, format, licences, garde et fixture indépendante. Sources et lock inchangés pendant la passe. Reçu : `docx-rich-layout-validation.json`. US-301 est validée au périmètre API; US-210 reste partielle, seuil Word ≥95 % ouvert. Les resets couleur/surlignage non représentables sont préservés et signalés, sans fidélité visuelle revendiquée.
