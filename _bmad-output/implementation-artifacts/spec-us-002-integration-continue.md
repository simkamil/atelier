---
title: 'US-002 — Intégration continue'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: 'NO_VCS'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/docs/DEVELOPMENT.md'
  - '{project-root}/rust-toolchain.toml'
---

<frozen-after-approval reason="US-002 du backlog ; développement et documentation autorisés par l’utilisateur, puis demande de continuer">

## Intent

**Problem:** Aucun contrôle automatique ne protège les trois cibles ni l’interdiction des paquets externes.

**Approach:** Fournir une CI GitHub Actions et un vérificateur local du verrouillage, exécuté avant les autres commandes Cargo.

## Boundaries & Constraints

**Always:** Rust stable, édition 2024, uniquement std/core et membres locaux. AGPL-3.0-or-later, SPDX, aucun unsafe. Sous-agent GPT-6 Luna, raisonnement moyen. Builds MSVC, WASM et musl ; tests exécutés sur les hôtes natifs. Les tests communs au moteur s’exécutent nativement ; WASM est compilé et linté, sans prétendre exécuter des binaires WASM sur l’OS.

**Never:** Aucun crate, framework, analyseur ou outil tiers ajouté. Aucun push, déploiement ou modification système local. Ne pas déclarer une exécution GitHub/Linux réussie sans preuve. Les runtimes BMAD/Graphify restent hors produit.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat | Erreur |
|---|---|---|---|
| Workspace local | Lock cohérent, dépendances locales entre membres incluses | Acceptation | Code 0 |
| Source externe | Paquet registry/git, même avec nom/version d’un membre | Refus et paquet identifié | Code 1 |
| Chemin non membre | Paquet local exclu du workspace présent dans le lock | Refus | Code 1 |
| Lock invalide | Absent, UTF-8 invalide, malformé, format inconnu, doublon ou identité manquante | Diagnostic | Code 1, aucune panique |
| Cargo indisponible | Échec de metadata ou sortie incorrecte | Diagnostic | Code 1 |
| Préservation | Lock contenant un paquet extérieur ou nécessitant une résolution | Refus | Octets du lock inchangés |

</frozen-after-approval>

## Code Map

- `Cargo.toml`, `Cargo.lock` : 30 membres locaux, zéro dépendance ; ajouter un outil interne.
- `tools/check-license/` : conventions de CLI, tests réels et SPDX à réutiliser.
- `crates/core-wasm/` : moteur ciblé WASM ; `desktop-win/` : MSVC ; `server/` : musl.
- `README.md`, `docs/DEVELOPMENT.md`, `US-V0-V1.md:45` : commandes, preuves et critères US-002.
- `.github/` absent ; Graphify montre US-001 comme prérequis du socle CI, sans API produit affectée.

## Tasks & Acceptance

**Execution:**
- [x] `tools/check-workspace/Cargo.toml`, `src/*.rs`, `tests/*.rs`, `Cargo.toml`, `Cargo.lock` : ajouter `atelier-check-workspace`, sans dépendances ; comparer chaque nom/version verrouillé aux vrais `workspace_members` de Cargo metadata et refuser toute source externe. Tester la matrice avec tests unitaires et exécutable réel, dont une fixture avec dépendances locales.
- [x] `.github/workflows/ci.yml` : déclencher push, pull_request et workflow_dispatch ; jobs Windows/MSVC, WASM et Linux/musl. Compiler, tester, Clippy `-D warnings`, rustfmt et SPDX. Chaque job vérifie le lock avant Cargo ; commandes Cargo `--locked --offline`.
- [x] `README.md`, `docs/DEVELOPMENT.md` : documenter bootstrap, commandes équivalentes et portée réelle des validations. Le coordinateur coche US-002 seulement après vérification indépendante.

**Acceptance Criteria:**
- Étant donné une modification du dépôt, quand la CI démarre, alors les trois triples sont compilés et les erreurs de build, test, lint, format et licence rendent le job fautif non nul.
- Étant donné un paquet hors workspace dans Cargo.lock, quand le contrôle démarre avant les builds, alors il échoue sans normaliser ni réécrire le lock.
- Étant donné le dépôt, quand les commandes documentées sont suivies, alors elles reproduisent les contrôles et précisent les tests exécutés, compilés seulement ou restant à exécuter sur un autre hôte.

## Implementation Notes

- Le workflow BMAD est déjà orchestré : implémenter directement cette spec, sans relancer BMAD ni créer d’agent. Graphify est géré par le coordinateur.
- Bootstrap du garde via `rustc --edition=2024 tools/check-workspace/src/main.rs -o ...`, puis exécuter avant toute commande Cargo. `cargo run` pourrait sinon retirer un paquet interdit avant inspection. Le tool doit donc aussi compiler seul, sans env Cargo à la compilation.
- CLI : un chemin racine optionnel, args_os ; code 0/1 et diagnostic stderr. Lire le lock directement ; metadata `--no-deps --format-version 1 --locked --offline`, sans décoder les IDs opaques. Un parseur maison strict du sous-ensemble Cargo.lock généré est acceptable s’il échoue proprement sur un format non pris en charge et accepte les dépendances locales. JSON de metadata : lecture structurée, pas recherche de sous-chaînes ; traiter erreurs et borner la profondeur.
- Actions : checkout officiel épinglé `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1 vérifiée). Aucun setup action tiers. Rustup stable + fmt/clippy/cibles, runners windows-2025 et ubuntu-24.04. Installer musl-tools sur le runner Linux pour le linker.
- Windows : workspace complet. WASM : atelier-core-wasm et dépendances transitives. Linux musl : workspace hors atelier-desktop-win et atelier-core-wasm ; tests exécutés avec cible musl. Qualité fmt/SPDX sur un hôte natif. Ne pas compiler les outils std/CLI pour WASM.
- Ce poste Windows compile déjà atelier-server pour musl via le rust-lld livré avec Rust ; il ne peut pas exécuter les tests Linux. Aucun dépôt Git ni hôte CI distant disponible.
- Correction de revue : l'appel metadata complet `--format-version 1 --locked --offline` remplace l'option `--no-deps` prescrite initialement. Il détecte les collisions entre membres et chemins extérieurs ; les sources registry/git sont refusées avant cet appel. Le garde ne modifie jamais volontairement le lock ; la maintenance explicite est documentée séparément.
- Validation finale indépendante : 43 tests Windows, 18 pour le garde ; bootstrap, trois builds, Clippy natif/WASM, rustfmt et SPDX réussis. Tests musl compilés seulement ; aucun job distant exécuté. Revue à trois sous-agents Luna moyen terminée, correctifs vérifiés, aucun défaut confirmé laissé ouvert. Preuves dans `us002-validation.json` ; graphe et backlog actualisés.

## Spec Change Log

## Review Triage Log

| Origine / constat | Verdict | Preuve et décision |
|---|---|---|
| Contrôle initial — erreurs natives PowerShell masquées | medium | Des commandes natives successives peuvent laisser seulement le dernier code de sortie. Corrigé avant revue : une étape par contrôle ; bootstrap rustc vérifie explicitement son code. |
| Contrôle indépendant — TOML malformé accepté | medium | Reproduit code 0 pour dependencies répété, guillemet non échappé et virgule manquante. Corrigé ; tests unitaires et CLI refusent ces entrées et vérifient les octets inchangés. |
| Contrôle indépendant — boucle ne parcourant qu’un élément | medium | Clippy échouait sur never_loop après le contrôle des membres absents. Corrigé avec if let et next ; suite complète et Clippy repassés. |
| Blind 1 — chemin Unicode supplémentaire rejeté | false | Une fixture réelle dans un chemin contenant U+1F4C1 passe le garde. Cargo émet l’UTF-8 directement, sans paire de surrogates JSON ; la conséquence annoncée ne se produit pas. |
| Blind 2 — arrays TOML inline non vides refusés | low | Réel uniquement pour un lock édité manuellement. Le sous-ensemble généré par Cargo est explicitement autorisé par la conception ; préciser la restriction dans les docs, sans ajouter un second analyseur TOML général. |
| Blind 3 — escapes TOML Unicode refusés | low | Même restriction de syntaxe manuelle ; le refus reste sûr et explicite, les sources générées par Cargo utilisent leur texte normal. Documentation de la limite ; extension du parseur rejetée. |
| Blind 4 — nom/version vide produit un diagnostic de non-membre | low | Le code 1 refuse bien le lock ; seul le libellé pourrait être plus spécifique. État manuel marginal, aucune branche supplémentaire justifiée. |
| Blind 5 — deux membres workspace de même identité confondus | false | Fixture réelle : Cargo refuse avant extraction des identités avec « two packages named same in this workspace », puis le garde retourne 1. Distinct du cas path extérieur de la revue Edge. |
| Blind 6 — contrôles de chaîne et structure JSON incomplets | false | Le parseur partagé rejette explicitement les octets 0 à 31 dans toutes les chaînes, clés incluses ; la racine, les types des champs utilisés et les données finales sont vérifiés. Aucun échec concret supplémentaire n’est fourni. |
| Blind 7 — fixtures TOML manuel valides manquantes | low | Ces syntaxes sont volontairement non prises en charge. Le happy path réel avec dépendance locale et les tableaux générés par Cargo sont exécutés ; les restrictions sont documentées. Pas de test qui imposerait une capacité exclue de cette conception. |
| Edge 1 — collision membre/path extérieur de même nom et version | medium | Reproduit : garde 0, metadata complet verrouillé 101 (« package collision in the lockfile »), lock inchangé. Corrigé par metadata complet et refus des sources externes avant résolution ; fixture réelle de collision passée. |
| Contrôle indépendant — entier TOML version = 04 accepté | medium | Reproduit code 0. Corrigé par la validation Cargo complète ; le test CLI refuse cette syntaxe et conserve exactement les octets. |

## Verification

- Build/tests workspace Windows, build WASM, build musl et compilation de ses tests via rust-lld, fmt, Clippy et SPDX.
- Bootstrap rustc puis contrôle valide et fixtures de refus ; confirmer la préservation des octets du lock.
- Relire le YAML, comparer toutes les commandes à celles exécutées localement, consigner la limite GitHub/Linux.

Audit de matrice : `executable_accepts_workspace_and_local_dependency_packages` couvre le workspace conforme ; les tests registry/git couvrent les sources externes ; `executable_rejects_excluded_path_dependency_without_changing_lock` couvre le chemin non membre. Les tests de lock absent/UTF-8 invalide, malformed arrays et format inconnu couvrent les entrées invalides ; `rejects_bad_json_and_missing_workspace_fields` et `executable_reports_cargo_metadata_failure_without_changing_lock` couvrent les erreurs metadata. Les tests de préservation, de collision et de lock nécessitant une résolution couvrent l'absence d'écriture. Tous ont été exécutés et passent dans la preuve finale.
