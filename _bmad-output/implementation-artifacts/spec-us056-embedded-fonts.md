---
title: 'US-056 — Polices embarquées'
type: 'feature'
created: '2026-10-10'
status: 'done'
route: 'dispatch'
review_loop_iteration: 0
baseline_commit: '6e09261054a7cbb91bc63d270762ae15fea1131b'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.6-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-us050-opentype-tables.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le rendu partagé doit fournir les polices métriquement compatibles prévues même lorsque les polices système ne sont pas disponibles, et les redistribuer légalement avec l’application. Sans inventaire figé ni notices visibles, les builds peuvent diverger et la provenance/licence des binaires est difficile à vérifier.

**Approach:** Fournir dans `data/fonts/` les cinq familles redistribuables Carlito, Caladea, Liberation Sans, Liberation Serif et Liberation Mono, chacune avec Regular, Bold, Italic et Bold Italic (20 TTF). Chaque binaire sera repris sans altération d’une révision amont officielle figée, accompagné des textes de licence intégraux, d’une notice/provenance et des empreintes SHA-256 permettant une vérification reproductible.

## Boundaries & Constraints

**Always:** Chaque face doit être OFL 1.1 et la redistribution doit conserver le copyright original et la licence dans les notices livrées. Ne modifier, convertir, sous-ensembler ni renommer aucun binaire. Enregistrer famille, graisse (400/700), style normal/italique, fichier, URL/repository, révision amont immuable et SHA-256 dans le manifeste. L’exposition Rust est une tranche statique `EMBEDDED_FONTS` de `EmbeddedFont { family: &'static str, weight: u16, style: EmbeddedFontStyle, data: &'static [u8], license: &'static str }`, avec `EmbeddedFontStyle::{Normal, Italic}` et `include_bytes!` des fichiers sous `data/fonts/`. Les références `license` doivent pointer vers les textes inclus et consultables depuis cette API sans UI.

Sources officielles : [Carlito, commit amont](https://github.com/googlefonts/carlito/tree/3a810cab78ebd6e2e4eed42af9e8453c4f9b850a) et [OFL Carlito](https://github.com/googlefonts/carlito/blob/3a810cab78ebd6e2e4eed42af9e8453c4f9b850a/OFL.txt); [Caladea, commit amont](https://github.com/googlefonts/caladea/tree/336a529cfad3d103d6527752686f8331d13e820a) et [OFL Caladea](https://github.com/googlefonts/caladea/blob/336a529cfad3d103d6527752686f8331d13e820a/OFL.txt); [Liberation Fonts 2.1.5 et son commit](https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5), avec son [texte de licence](https://github.com/liberationfonts/liberation-fonts/blob/2.1.5/LICENSE). Ces trois sources confirment OFL 1.1 pour les fichiers retenus. SHA release Liberation `4b01920` à résoudre en SHA complet dans le manifeste. Les deux dépôts Google Fonts publient dans leurs métadonnées les quatre styles visés. Identifiants : `googlefonts/carlito` commit `3a810cab78ebd6e2e4eed42af9e8453c4f9b850a`; `googlefonts/caladea` commit `336a529cfad3d103d6527752686f8331d13e820a`; `liberationfonts/liberation-fonts` release `2.1.5` (release page commit prefix `4b01920`, à résoudre en SHA complet dans le manifeste). Les métadonnées Google Fonts confirment les quatre styles et OFL pour Carlito/Caladea; l’amont Liberation confirme OFL 1.1.

**Never:** Aucune dépendance d’exécution, téléchargement réseau lors du build, intégration UI ou inventaire de polices système dans cette story. Ne pas modifier `crates/font/src/lib.rs`, la configuration Git, le manifeste du workspace ou la CI, appartenant au coordinateur.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Catalogue complet | Module embarqué compilé | 20 faces uniques, les 5 familles × 4 couples poids/style, données non vides | Test échoue avec famille/style manquant ou doublon |
| Métadonnées | Chaque TTF analysé par `FontFace` (US-050) | Noms internes, famille, graisse et italique concordent avec l’entrée du catalogue | Échec explicite du test pour une métadonnée divergente ou un parseur rejetant la face |
| Notices et provenance | Manifeste des ressources | Licence/copyright de chaque source accessibles; chaque empreinte recalculée correspond | Échec si notice absente, source non épinglée ou hash différent |

</frozen-after-approval>

## Code Map

- `data/fonts/` -- emplacement prévu dans l’architecture pour les ressources distribuables.
- `crates/font/src/lib.rs` -- API `FontFace::parse` servant à vérifier les faces; module d’intégration défini par le coordinateur.
- `crates/font/tests/opentype_fixtures.rs` -- exemple de tests d’intégration du parseur de polices.
- `SPEC.md` -- exigences de compatibilité métrique, distribution sans dépendance tierce et licence OFL.
- `US-V0-V1.md` -- inventaire et critères d’acceptation de US-056.

## Tasks & Acceptance

**Execution:**
- [x] `data/fonts/**` -- ajouter les 20 TTF amont inchangés et leurs notices/licences OFL complètes -- permettre une redistribution conforme et traçable.
- [x] `data/fonts/manifest.json` et `data/fonts/README.md` -- enregistrer chemins, familles/styles, versions, URL, SHA complets des commits et empreintes SHA-256, avec instructions de vérification -- rendre l’origine et l’intégrité auditables.
- [x] `crates/font/src/embedded.rs` -- exposer le catalogue typé statique prescrit par l’API du coordinateur -- rendre les faces consommables sans accès disque/réseau.
- [x] `crates/font/tests/embedded.rs` -- vérifier cardinalité, unicité, métadonnées OpenType et présence des notices -- détecter les erreurs d’assemblage.
- [x] `docs/us-056-embedded-fonts.md` -- documenter API, familles, licences, provenance et limites -- guider les consommateurs du crate.

**Acceptance Criteria:**
- Given un build offline du crate, when le catalogue est compilé, then exactement 20 faces (5 familles × regular/bold/italic/bold italic) sont incorporées et parsables.
- Given le manifeste et les assets, when leurs SHA-256 et leurs révisions officielles sont vérifiés, then chaque binaire correspond à la source amont annoncée sans modification.
- Given une redistribution du produit, when les notices sont consultées, then la licence OFL 1.1 et les copyrights de toutes les familles sont disponibles en texte intégral.

## Implementation Notes

Propriété exclusive : data/fonts/**, crates/font/src/embedded.rs, crates/font/tests/embedded.rs, docs/us-056-embedded-fonts.md et notes de ce spec. Le coordinateur gère exports du module, docs globales, vérification externe, CI, Graphify et Git. Ne pas invoquer de skill, sous-agent ou Graphify. Les autorisations utilisateur de développement/validation sont persistantes; résoudre les choix techniques ordinaires autonomement.


## Spec Change Log


## Review Triage Log

Chaque constat est trié individuellement avant regroupement. Les corrections sont locales, sans nouvelle API produit. Les trois agents de revue sont GPT-6 Luna moyen. La revue Edge Case et la revue Verification Gap n’ont signalé aucun constat.

| ID | Constat | Verdict | Preuve et résolution |
|---|---|---|---|
| B1 | Virgule absente entre XML et font dans README | low | Constat exact sur le diff initial; correction directe de la liste. |
| B2 | Énumération des suivis avec deux et | low | Constat exact dans README; énumération corrigée directement. |
| B3 | Annonce obsolète du futur corpus images | low | tests/README annonce un futur corpus déjà livré; phrase remplacée. |
| B4 | Graphify pourrait traiter les fichiers RGBA | false | Les .rgba ne sont pas des extensions prises en charge : la CLI les classe explicitement non classifiés et ne les soumet ni à AST ni au LLM. Exclure leur extension ne corrige aucun traitement réel. |
| B5 | Comptage du corpus trop permissif | medium | valid >= 180 laisserait disparaître sept références; assert_eq!(valid,187) détecte désormais toute perte. |
| B6 | Empreintes du manifeste absentes de CI | medium | Le test de pixels ne validait pas les empreintes documentées. tests/check-assets.ps1 vérifie les 201 entrées et 187 références, l’égalité TSV/manifeste, et les 20 polices; exécuté dans le job Windows. |
| B7 | Mention des agents inutile au suivi | false | Le suivi est daté et cette information documente la préférence explicite de l’utilisateur pour GPT-6 Luna moyen. Aucun comportement ni invariant de produit ne diverge. |
| B8 | Rapport de revue exclu du diff/lien absent | false | prepare-images-fonts-review.py exclut le préfixe images-fonts-review- ; le rapport images-fonts-review.md ne correspond pas à ce motif. Il est produit après les revues et livré à ce chemin; il ne pouvait exister avant leur résultat. |
| B9 | PngSuite distribué depuis HTTP | low | Les deux hôtes HTTPS échouent à TLS; conserver le lien officiel, documenter le SHA figé et le corpus versionné sans téléchargement CI. Correction documentaire. |
| B10 | Validation de schéma annoncée trop générale | low | Commentaire remplacé par les contrôles réels; garde natif ajoutant types, valeurs, cardinalité, unicité, commits et chemins. Onze inventaires volontairement altérés doivent échouer. |
| R1 | Portée GCE après extension Plain Text | medium | Test ajouté d’abord en échec confirmé par l’agent : la transparence était héritée après un bloc graphique Plain Text. Nettoyer control après en-tête 12 octets et données ignorées; préserver contrôle après commentaire/application. Test Cargo passe. |
| R2 | Pixels sautés par RLE rendus transparents | medium | Le corpus indépendant échouait sur bmp-rle4-delta.bmp : alpha 0 aux pixels non peints. Initialisation avec palette[0] opaque; références RLE4/RLE8 et test dédié passent. |
| R3 | biSizeImage tronqué silencieusement | medium | Le min(file_end) masquait une taille déclarée excessive; garde renvoie Truncated. Régression : flux RLE réel de six octets annoncé à sept. |
| R4 | Entiers PNG dépassant 31 bits | medium | Les premières sources ne refusaient pas explicitement les champs supérieurs à 2^31-1. Guardes chunk et IHDR avant allocation; test max_pixels=u64::MAX couvre largeur, hauteur et longueur. |
| R5 | Signature de test &mut Vec inutile | low | Clippy all-targets refusait ptr_arg; set_file_size prend &mut [u8]. |
| R6 | matches!(Err(_)) redondant en test | low | Clippy all-targets refusait redundant_pattern_matching; assertion .is_err(). |



## Verification

**Commands:**
- `cargo test -p atelier-font --locked --offline` -- expected: tests du catalogue et des métadonnées passent sans réseau.
- `cargo fmt -p atelier-font -- --check` -- expected: code Rust formaté.
- Vérification indépendante SHA-256 des assets contre `data/fonts/manifest.json` -- expected: 20 empreintes exactes.


## Validation finale — 2026-10-10

Les quinze contrôles de `images-fonts-validation.json` passent : 210 tests workspace, 34 tests release images/polices, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, licences, garde, métadonnées et intégrité des ressources. Les sources et le lock sont inchangés pendant les contrôles. `images-fonts-oracle.json` confirme 201 entrées, 187 références régénérées à l’identique, 14 PNG invalides rejetés, 149 728 mutations sans panic, onze inventaires altérés rejetés et vingt faces FontTools. `images-fonts-upstream.json` confirme les vingt polices identiques aux sources officielles. Tous les constats sont triés, aucun différé. Le relevé `github-ci.json` identifie le commit et les jobs distants ; les tests musl sont exécutés sur Linux par la CI.
