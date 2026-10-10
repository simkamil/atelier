---
title: 'US-005 — Bancs de performance'
type: 'feature'
created: '2026-10-09'
status: 'done'
baseline_commit: 'NO_VCS'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/README.md'
  - '{project-root}/docs/DEVELOPMENT.md'
---

<frozen-after-approval reason="Développement des US autorisé par la demande utilisateur du 2026-10-09">

## Intent

**Problem:** Le projet ne dispose d'aucun outil permettant de mesurer ses performances et de détecter une régression par rapport à une exécution de référence.

**Approach:** Fournir un outil Rust maison réutilisable, exécutable en ligne de commande, avec médiane sur N itérations, sortie CSV et comparaison à une référence. Les futurs moteurs pourront enregistrer leurs propres cas ; les cas initiaux mesurent seulement le socle réellement disponible.

## Boundaries & Constraints

**Always:** Uniquement Rust stable, `std`/`core` et dépendances locales. Licence et lints du workspace US-001. Refuser proprement les arguments, fichiers et données invalides. Une régression est strictement supérieure à 10 %, pas égale. Les résultats doivent identifier le cas et le nombre d'itérations ; les commandes de mesure utilisent le profil release. Mettre à jour les docs et critères US-005 seulement après validation.

**Never:** Aucune dépendance de registre, aucun moteur fictif, aucune affirmation que les objectifs fonctionnels du §6.16 sont déjà atteints. Ne modifier les autres crates que pour intégrer l'outil au workspace.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Gestion d'erreur |
|---|---|---|---|
| Mesure valide | Cas connu et N positif | Médiane calculée après N mesures, CSV exploitable | Succès |
| Limite de régression | Référence 100, mesure 110 puis 111 | 110 accepté, 111 signalé | Code non nul pour régression |
| Configuration invalide | N nul, argument inconnu, fichier illisible | Message explicite | Code non nul, sans panique |
| Référence invalide | CSV mal formé, cas manquant ou dupliqué, médiane nulle | Comparaison refusée explicitement | Aucun succès silencieux |
| Résultats futurs | Plusieurs cas enregistrés | Chaque cas comparé à son homologue | Noms conservés dans le CSV |

</frozen-after-approval>

## Code Map

- `Cargo.toml` — workspace US-001, membres locaux et métadonnées partagées.
- `tools/check-license/` — convention des outils internes et vérification de licence.
- `SPEC.md` §6.16, §8 — objectifs futurs et suivi de performance.
- `US-V0-V1.md` US-005 — deux critères : médiane/CSV puis référence/seuil.
- `docs/DEVELOPMENT.md` — avancement et commandes réellement vérifiées.

## Tasks & Acceptance

**Execution:**
- [x] `tools/bench/Cargo.toml`, `tools/bench/src/lib.rs` — fournir l'enregistrement de cas, la mesure avec `Instant` et `black_box`, une médiane correcte pour effectifs pairs et impairs, la sérialisation CSV et une comparaison réutilisable.
- [x] `tools/bench/src/main.rs` — exposer une CLI documentée (liste des cas, sélection, N itérations, fichier de sortie, référence) ; diagnostics explicites et code distinct d'une comparaison réussie lorsqu'une régression est détectée. Au moins deux cas réels de socle permettent de vérifier le parcours multi-cas.
- [x] `Cargo.toml` — intégrer seulement ce nouvel outil local, avec lints et licence hérités.
- [x] Tests unitaires et de CLI — couvrir chaque ligne de la matrice ; éviter les assertions de durée murale fragiles en utilisant des résultats contrôlés pour vérifier le seuil.
- [x] `README.md`, `docs/DEVELOPMENT.md`, `US-V0-V1.md` — documenter les commandes et preuves, cocher les critères seulement après les validations.

**Acceptance Criteria:**
- Étant donné N itérations, quand l'outil mesure un cas, alors il écrit un CSV qui nomme le cas, N et la médiane et peut être relu comme référence.
- Étant donné une référence complète et valide, quand l'outil compare les résultats, alors toute augmentation strictement supérieure à 10 % est signalée et provoque un code non nul ; exactement 10 % reste accepté.
- Étant donné une entrée invalide de la matrice, quand la CLI la traite, alors elle retourne un diagnostic et un code non nul sans panique.
- Étant donné le workspace, quand les contrôles de qualité s'exécutent, alors ils passent hors ligne sans dépendance externe ni `unsafe` supplémentaire.

## Implementation Notes

- Le workflow BMAD est piloté par l'orchestrateur ; le sous-agent implémente directement depuis cette spec, sans relancer un workflow ni créer de sous-agent. Graphify est également rafraîchi par l'orchestrateur.
- Le registre peut rester une API Rust simple de fonctions/closures ; pas de chargement dynamique ni d'abstraction de plugins.
- Employer des unités explicites pour le CSV et documenter chauffe, profil, machine et limites de reproductibilité. Les cas de démonstration ne remplacent pas les futurs scénarios documentaires du §6.16.

- Validation indépendante : 25 tests workspace réussis, dont 19 du banc ; build Windows/WASM, Clippy, rustfmt et SPDX passent. Métadonnées et lockfile : 28 crates produit + deux outils locaux, aucune dépendance externe. Chaque ligne de la matrice est couverte par des tests exécutés et passants.
- CLI release réellement exercée : mesures multi-cas sur 100 itérations, référence relue, code 0 sur comparaison valide et code 2 sur régression contrôlée. stdout CSV propre, référence identique préservée, entrées extrêmes/non Unicode refusées avec le code 1. Preuves dans `us005-validation.json` et ../../docs/DEVELOPMENT.md.
- Tous les constats conservés de la revue ont été corrigés ; aucun travail préexistant différé. Correction finale du nom d'un test et d'un chemin Windows dans un test pour conserver sa portabilité.

## Spec Change Log

## Review Triage Log

| Origine / constat | Verdict | Preuve et décision |
|---|---|---|
| Blind 1 — TOML invalide | false | `cargo build -p atelier-bench --offline` charge correctement Cargo.toml. L'ancienne ligne sans saut final était mal représentée par le diff NO_VCS ; corriger l'artefact de diff. |
| Blind 2 — référence écrasée avant comparaison | high | Reproduit sur un fichier temporaire : référence 1 ns, même chemin pour sortie et référence, code 0 et contenu écrasé. Patch : charger et valider la référence avant toute écriture et comparer les données originales. |
| Blind 3 — stdout CSV pollué | medium | Reproduit avec `--output - --reference` : message de succès après les lignes CSV. Patch : tous les diagnostics vont sur stderr ; test de la sortie capturée. |
| Blind 4 — barrières d'optimisation des closures | low | Les deux cas fournis utilisent déjà black_box. Une closure FnMut() doit conserver son travail observable ; préciser cette responsabilité dans la documentation API et un exemple, sans promettre de protéger du code mort interne à une closure. |
| Blind 5 — --list ignore --iterations | low | Lecture du parseur : option acceptée mais inutilisée. Correction directe de la vérification des options incompatibles. |
| Blind 6 — cas sans scénarios §6.16 | false | L'intention fournit un banc réutilisable pour les futurs moteurs. Les moteurs sont encore vides et les docs excluent explicitement toute affirmation d'atteinte des objectifs ; implémenter leurs scénarios appartient aux US fonctionnelles. |
| Edge 1 — allocation trop grande | high | CLI reproduite avec usize::MAX : code 101 et capacity overflow. Patch : borner le nombre de mesures et retourner une erreur avant l'allocation. |
| Edge 2 — alias sortie/référence | high | Même défaut vérifié que Blind 2 ; charger la référence originale avant écriture couvre également les chemins équivalents. |
| Verification 1 — sélection valide non vérifiée | medium | Couverture existante limitée aux cas inconnus et à l'ensemble par défaut. Patch : test de la CLI réelle sélectionnant un seul cas. |
| Verification 2 — noms de --list non vérifiés | medium | Le test ne vérifie que le retour interne. Patch : capturer stdout de l'exécutable et vérifier les deux noms. |
| Verification 3 — CSV stdout non vérifié | medium | Aucun test existant de stdout ; défaut confirmé pour la comparaison. Patch : test de la CLI réelle qui relit stdout comme CSV, avec et sans référence. |
| Audit — médiane pair/impair non vérifiée | medium | Le test chronométré ne peut pas détecter le remplacement de la médiane par la moyenne. Patch : cas déterministes autour d'un helper privé de calcul de médiane, dont valeurs proches de u128::MAX. |
| Audit — nombre de crates README erroné | low | Metadata : 28 crates produit + 2 outils = 30 membres, pas 29 crates produit. Correction directe de la phrase. |
| Audit — CSV de test laissé dans les sources | low | tools/bench/bench.csv créé par les tests d'erreur utilisant la sortie par défaut. Supprimer cet artefact et éviter l'écriture avant validation des entrées. |
| Audit — arguments non Unicode | medium | `env::args` convertit les arguments sans gestion d'erreur ; un argument Windows contenant un surrogate isolé provoque une panique. Patch : collecter via args_os puis retourner un diagnostic de conversion. |

## Verification

- `cargo test -p atelier-bench --offline` — tests de matrice, y compris la CLI réelle.
- Exécuter en release les commandes documentées de mesure de deux cas, produire un CSV et le relire pour comparer ; vérifier aussi une régression contrôlée via CLI.
- `cargo build --workspace --offline`, `cargo test --workspace --offline`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`.
- `cargo run -p atelier-check-license --offline -- .` — en-têtes conformes.
- `cargo metadata --offline --format-version 1 --no-deps` — uniquement les membres locaux attendus.
