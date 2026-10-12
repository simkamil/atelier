---
title: 'Intégrer et valider les livraisons parallèles US-004/052/082/311'
type: 'feature'
created: '2026-10-12'
status: 'done'
baseline_commit: '39c928983092644f029f2054d96e1b55976ac8b4'
route: 'dispatch'
review_loop_iteration: 3
context: ['{project-root}/SPEC.md', '{project-root}/US-V0-V1.md']
---

<frozen-after-approval reason="human-owned intent — reprise du développement parallèle déjà autorisée">

## Intent

**Problem:** Quatre branches livrées par l'autre outil ne sont pas encore intégrées à main. Leurs tests et leurs affirmations doivent être confrontés au moteur actuel avant de compter les US comme terminées.

**Approach:** Reprendre les livraisons existantes, corriger les défauts avérés, puis valider le workspace et publier un lot cohérent. Conserver les critères réellement non satisfaits ouverts et mettre à jour les documents avec les preuves obtenues.

## Boundaries & Constraints

**Always:** Agents GPT-6 Luna en raisonnement moyen, tâches isolées par module ; Rust stable, dépendances Cargo locales uniquement, aucune nouvelle FFI hors exemptions de SPEC ; Graphify avant analyse et après changement structurel ; fichiers, branches et historiques de l'autre outil préservés. Le push main est autorisé par les instructions persistantes. Figer les sources pendant la validation finale. Distinguer tests locaux, compilation musl et exécution CI distante.

**Never:** Déclarer la mémoire réellement bornée à partir du seul output_size ; compter un test synthétique comme oracle extérieur ; revendiquer absence de bogues d'après une courte campagne de fuzz ; clore un critère Word, une intégration UI riche ou un résultat CI non exercé. Ne pas modifier les réglages de facturation GitHub.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Livraisons compatibles | Quatre branches propres, main contient DOCX/layout | Modules disponibles sans perte des fonctionnalités précédentes | Résoudre les seuls conflits documentés et garder les reçus de test |
| CFF | Charset prédéfini, FontMatrix, seac, sous-routines/CID | Contours cohérents et budgets appliqués | Erreur typée sur données invalides ou limites dépassées |
| Raster | Liste validée, clips, transformations, texte et images | Replay via US-061 à résolution choisie | Erreur typée, aucune sortie partielle revendiquée |
| CFB | V3/V4, FAT/DIFAT/MiniFAT, flux petits/grands | Répertoire et lecture bornée des flux | Rejeter cycles, références et tailles invalides |
| Fuzz | Graine, durée, corpus et cible | Mutations reproductibles, sauvegarde et réduction | Argument extrême diagnostiqué sans panique ; limite mémoire honnêtement documentée |

</frozen-after-approval>

## Code Map

- `crates/font/src/{cff.rs,cff_tables.rs,outline.rs}` : livraison de us-052-cff-completion ; contours appelés par font, shape et le moteur embarqué. Le nouvel interpréteur peut changer les octets WASM, vérifier et reconditionner si nécessaire.
- `crates/raster/src/display_list.rs` : livraison de us-082-raster-backend ; dépendance vers render, réutilise Rasterizer. Tests ont font/xml en dépendances locales de développement.
- `crates/cfb/src/lib.rs` : livraison us-311-cfb-container, crate jusque-là vide ; aucun import .doc complet fourni par cette API.
- `tools/fuzz/` : us-004-fuzzer ; nouveau membre workspace, 17 cibles, runner à thread surveillé, mémoire fondée sur sortie déclarée. Vérifier durées, conversions et minimisation des timeouts.
- `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml` : coordination root uniquement. Lock final 33 paquets attendus, sans source externe.
- `docs/us-004-fuzzer.md`, `docs/us-052-cff-outlines.md`, `docs/us-082-raster-backend.md`, `docs/us-311-cfb-container.md` : contrats à rapprocher des preuves.
- `tools/package-engine.py`, `data/engine/manifest.json`, `tests/check-embedded-engine.mjs` : provenance et taille WASM, plafond existant à conserver.

## Tasks & Acceptance

**Execution:**
- [x] `crates/font/`, `crates/raster/`, `crates/cfb/` — examiner les trois livraisons en parallèle et intégrer les correctifs prouvés.
- [x] `tools/fuzz/` — examiner puis durcir les cas limites réellement défaillants ; conserver les limites de mémoire non mesurée.
- [x] `Cargo.toml`, `Cargo.lock` — intégrer membres et dépendances locales sans effacer celles de DOCX/layout.
- [x] `_bmad-output/implementation-artifacts/` — revue indépendante du diff complet, triage individuel et validation finale avec hashes des sources.
- [x] `data/engine/` — reproduire et vérifier le moteur publié et sa provenance après changement font/lock.
- [x] `docs/`, `US-V0-V1.md`, `graphify-out/` — synchroniser contrats, statut exact des critères et dépendances ; ne clore que ce qui est validé.

**Acceptance Criteria:**
- Given les livraisons versionnées, when le lot est intégré, then les tests de DOCX, layout, CFF, raster, CFB et fuzz passent ensemble avec un lock local et immuable pendant les contrôles.
- Given une entrée CFF/CFB/raster invalide, when l'API la traite, then un résultat borné ou une erreur documentée est produit sans panique dans les cas couverts.
- Given des options numériques CLI extrêmes, when le fuzzer les reçoit, then elles sont refusées proprement sans panique ni écrasement implicite d'un fichier de rejeu.
- Given les résultats disponibles, when les docs sont actualisées, then chaque case cochée renvoie à une preuve vérifiable et les limites non exercées restent explicites.

## Implementation Notes

- Les quatre branches ont été fusionnées normalement depuis leur ancêtre commun, sans suppression des ajouts DOCX/layout de main. Les worktrees extérieurs sont conservés.
- Trois agents Luna moyen ont corrigé les marqueurs CFB, la composition seac et les contrôles du fuzzer. Références natives Windows CFB V3/V4 et FontTools CFF versionnées, testées depuis les mêmes fixtures par Rust.
- 17 cibles de fuzz incluent désormais DOCX et CFB ; limites mémoire réelles non mesurées, US-004 restera partielle. Dépendance locale CFB ajoutée volontairement et lock régénéré sous contrôle root.
- Le premier build WASM dépassait le plafond. La suppression des sections de symboles dans wasm-release ramène le moteur à 1 041 817 octets ; plafond conservé, profils ordinaires inchangés.

## Spec Change Log

Aucune modification de l’intention figée. Le plan d’exécution ajoute les régressions avérées des revues : entrée CFB étrangère, padding FAT/DIFAT, casse simple Unicode, UPEM nul et timeout pendant minimisation. La contrainte mémoire de US-004 reste explicitement non satisfaite.

## Review Triage Log

Les [33 constats séparés](foundation-integration-review.md) conservent origine, verdict et preuve. Neuf allégations sont réfutées ; les 24 constats retenus ont une correction de code, de test ou de documentation. Relectures ciblées des correctifs CFB, CFF et fuzz closes ; aucun défaut avéré de ce lot différé. Les critères ouverts hors lot et la mémoire réelle de US-004 restent ouverts.

## Verification

- Garde workspace, builds et tests Windows, Clippy hôte/WASM, rustfmt, SPDX, builds WASM/musl et compilation tests musl.
- Tests ciblés des quatre livraisons, campagne courte reproductible sur chaque cible, fixtures et oracles extérieurs selon leur disponibilité réelle.
- Reproduction du moteur embarqué, contrôle de taille existant, test réel ABI/Worker/navigateur si les octets moteur changent.
- Refresh Graphify, statuts des 158 US, absence de dépendances Cargo cycliques et arêtes pendantes.

## Résultat final

25 contrôles réussis, 671 tests workspace, 55 tests natifs release et 17 000 exécutions fuzz sans défaillance détectée. Références CFB/CFF reproduites ; sources et lock inchangés pendant la passe. Builds Windows/WASM/musl, tests musl seulement compilés ici. Moteur embarqué 1 041 817 octets ; plafond inchangé. US-052/082/311 closes, US-004 partielle, backlog 46/158. Graphify et publication sont tracés dans les reçus du lot.
