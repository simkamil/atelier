---
title: 'US-001 — Initialiser le workspace Atelier'
type: 'feature'
created: '2026-10-09'
status: 'done'
baseline_commit: 'NO_VCS'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
---

<frozen-after-approval reason="Périmètre US-001 fourni par le backlog ; développement autorisé par la demande utilisateur du 2026-10-09">

## Intent

**Problem:** Le projet dispose d'une spec et d'un backlog, mais aucun workspace ni code compilable. Les autres stories V0 dépendent de ce socle.

**Approach:** Initialiser les crates décrites par la spec, partager les règles de qualité et fournir un contrôle de licence maison. Cette story fournit une base de développement, sans implémenter les fonctionnalités des futurs modules.

## Boundaries & Constraints

**Always:** Rust stable, édition 2024, uniquement `std`/`core` et dépendances locales du workspace. Licence AGPL-3.0-or-later ; en-tête SPDX dans chaque fichier source du projet. `#![forbid(unsafe_code)]` pour les crates hors FFI ; les trois exceptions autorisées sont `core-wasm`, `desktop-win` et `server`. Développement délégué à GPT-6 Luna, raisonnement moyen. Mettre à jour le backlog seulement après vérification des critères.

**Never:** Aucune dépendance téléchargée depuis un registre Cargo, aucun framework JS, aucune fonctionnalité fictivement déclarée terminée. Les outils BMAD et Graphify sont des données de travail de l'agent et ne font pas partie du code produit vérifié ni distribué. Pas de publication, de déploiement ou de commit requis ; ce dossier n'est pas encore un dépôt Git.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Gestion d'erreur |
|---|---|---|---|
| Sources conformes | Arborescence projet avec en-têtes corrects | Vérificateur de licences réussi | Code de sortie zéro |
| En-tête absent ou erroné | Un fichier source du projet non conforme | Chemin fautif signalé | Code de sortie non nul |
| Source illisible | Erreur de lecture / encodage | Erreur explicite | Aucun succès silencieux |
| Dossiers générés | `target`, Graphify ou runtime BMAD présents | Aucun contrôle des sources tierces de ces dossiers | Parcours limité aux racines sources du projet |

</frozen-after-approval>

## Code Map

- `SPEC.md` — architecture cible, liste des crates, invariants de sûreté et licence ; conserver le périmètre produit.
- `US-V0-V1.md` — critères US-001 ; US-002/004/005 dépendent de ce socle et restent ouvertes.
- `graphify-out/graph.json` — carte des dépendances déclarées ; US-001 est le point d'entrée des fondations.
- `Cargo.toml`, `crates/`, `tools/`, `web/`, `data/`, `tests/` — actuellement absents ; aucune API existante à préserver.

## Tasks & Acceptance

**Execution:**
- [x] `Cargo.toml`, `crates/*/Cargo.toml`, `crates/*/src/*.rs` — créer les 28 crates prévues, dont `calc`, avec métadonnées héritées, lints partagés, sources minimales et frontières FFI — rendre les futurs modules compilables isolément.
- [x] `rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml` — fixer les conventions communes et les composants standard — rendre les commandes locales reproductibles.
- [x] `tools/check-license/Cargo.toml`, `tools/check-license/src/*.rs` — ajouter un vérificateur sans dépendances et ses tests de cas limites — faire respecter les en-têtes du code projet.
- [x] `LICENSE`, `README.md`, `.gitignore`, `web/README.md`, `data/README.md`, `tests/README.md` — fournir licence officielle complète, commandes et limites du socle, conventions de ressources — documenter un démarrage réel.
- [x] `US-V0-V1.md`, `docs/DEVELOPMENT.md` — enregistrer les preuves de validation et cocher US-001 uniquement après les contrôles — rendre l'avancement vérifiable.

**Acceptance Criteria:**
- Étant donné le dossier du projet, quand Cargo charge ses métadonnées hors ligne, alors les 28 crates de la spec et l'outil interne sont présents sans paquet externe.
- Étant donné le workspace, quand les builds, tests, Clippy et rustfmt sont exécutés, alors le socle passe sur Windows et le moteur se compile pour WASM, sans `unsafe` hors des exceptions autorisées.
- Étant donné les sources projet, quand le vérificateur inspecte les en-têtes, alors il accepte les fichiers conformes et refuse un en-tête absent ou une erreur de lecture selon la matrice.
- Étant donné la racine, quand un développeur suit le README, alors il trouve la licence complète et les commandes exécutées, avec la distinction entre crates vides et application fonctionnelle future.

## Implementation Notes

- Vérification indépendante réussie le 2026-10-09 : les 28 crates de SPEC §6.2 correspondent aux métadonnées Cargo ; 29 paquets locaux, aucun paquet externe et Cargo.lock cohérent. Build Windows, tests workspace, build WASM, Clippy, rustfmt et contrôle de licence réussis après les corrections de revue.
- Cinq tests unitaires et un test CLI passent. Le scénario réel de lien symbolique est conditionnel : ce poste refuse sa création (WinError 1314), donc cette branche doit aussi être exercée sur un hôte qui l'autorise.
- Chaque ligne de la matrice initiale est exercée : en-tête valide, absent/erroné, UTF-8 invalide, exclusions générées. Aucun élément de revue bloquant ni travail préexistant différé.

## Spec Change Log

## Review Triage Log

| Origine / constat | Verdict | Preuve et décision |
|---|---|---|
| Blind 1 — racines manquantes acceptées | medium | Reproduit : la CLI retourne 0 sur un chemin inexistant. Patch : les deux racines sources attendues doivent être lisibles. |
| Blind 2 — liens symboliques ignorés | medium | `file_type` ne suit pas les liens et aucune branche ne les signale. Patch : erreur explicite plutôt qu'exclusion silencieuse. |
| Blind 3 — SPDX parmi les trois premières lignes | low | La position reste limitée à l'en-tête ; une notice de copyright peut légitimement précéder SPDX. Cas de code placé avant SPDX peu probable, pas de perte d'identification de licence : rejet d'une règle plus complexe. |
| Blind 4 — exclusions par nom à toute profondeur | false | Les noms correspondent précisément aux répertoires générés exclus par la matrice ; aucun répertoire produit portant ces noms n'existe. Le test d'exclusion valide ce comportement voulu. |
| Blind 5 — absence de smoke test CLI | medium | Tests présents limités à `check_tree` ; le faux succès est observable via l'exécutable. Patch groupé avec Blind 1 : tester diagnostic et code de sortie réels. |
| Blind 6 — absence de test des racines manquantes | medium | Même cause que Blind 1 ; ajouter la couverture à son correctif. |
| Blind 7 — absence de test des liens | medium | Même cause que Blind 2 ; tester le rejet avec un lien réel lorsque la plateforme permet sa création. |
| Blind 8 — absence de test SPDX après du code | low | Même cas marginal que Blind 3 ; aucun critère n'impose une première ligne stricte. Rejet de l'ajout d'une politique différente. |
| Edge 1 — racines absentes ou inaccessibles | medium | Confirmé par la CLI et la lecture de `Path::exists` qui masque les erreurs. Correctif partagé avec Blind 1. |
| Edge 2 — sources liées ignorées | medium | Confirmé par la branche de parcours. Correctif partagé avec Blind 2. |
| Verification — roster non asserté automatiquement | low | Le roster est vérifié indépendamment depuis la spec via Cargo metadata pour cette livraison. Un test permanent recopiant la liste refléterait l'implémentation ; les instructions de développement demandent de ne pas ajouter ce type de test. Contrôle ponctuel et preuve consignés. |

## Verification

- `cargo metadata --offline --format-version 1 --no-deps` — uniquement les membres locaux attendus.
- `cargo build --workspace --offline` et `cargo test --workspace --offline` — réussite sur Windows.
- `cargo build -p atelier-core-wasm --target wasm32-unknown-unknown --offline` — compilation WASM réussie.
- `cargo clippy --workspace --all-targets --offline -- -D warnings` et `cargo fmt --all --check` — aucun avertissement ni écart.
- `cargo run -p atelier-check-license --offline -- .` — tous les en-têtes sources conformes.
