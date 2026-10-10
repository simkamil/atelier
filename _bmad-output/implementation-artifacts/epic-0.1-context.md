# Epic 0.1 Context: Socle du dépôt et outillage

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Établir un dépôt Rust compilable et vérifiable qui serve de fondation commune aux modules d’Atelier. Cela comprend l’organisation Cargo, les conventions de qualité et de licence, ainsi que les outils locaux de test visuel, fuzzing et mesure de performance. Le dossier standard de planification est absent ; ce contexte s’appuie sur les stories du backlog et sur la spécification globale existante.

## Stories

- US-001 : Initialiser le workspace
- US-002 : Intégration continue
- US-003 : Tests par images de référence
- US-004 : Fuzzer maison
- US-005 : Bancs de performance

## Requirements & Constraints

- La chaîne Rust stable est la seule dépendance de développement autorisée. N’ajouter aucun crate externe, framework ou bibliothèque JavaScript ; s’appuyer sur `std`/`core`, les API système et navigateur autorisées et des outils maison sous `tools/`.
- Le workspace doit prendre en charge les trois cibles : `x86_64-pc-windows-msvc`, `wasm32-unknown-unknown` et `x86_64-unknown-linux-musl`. La CI compile et teste ces cibles et exécute tests, Clippy, rustfmt et les contrôles de licence. Elle échoue si `Cargo.lock` contient un paquet hors workspace.
- Les outils de référence comparent les PNG pixel à pixel avec des tolérances configurables par canal et en pourcentage de pixels, produisent une image de différence en cas d’échec et ne régénèrent les références qu’en mode de mise à jour explicite.
- Le fuzzer mutationnel expose `FuzzTarget`, prend en charge inversion de bits, insertion, suppression, épissure du corpus et dictionnaire de jetons. Il conserve les cas de panique, dépassement de temps ou de mémoire, propose une minimisation simple et lance une cible pour une durée donnée en ligne de commande.
- Les bancs sont exécutables en ligne de commande, calculent la médiane sur N itérations, produisent du CSV et signalent une régression supérieure à 10 % face à une exécution de référence.
- Les entrées externes sont non fiables : aucun analyseur ne doit paniquer ; bornes et erreurs propres sont attendues. Aucun `unsafe` hors des crates FFI ; chaque bloc `unsafe` doit être commenté. Les fichiers de référence et corpus doivent respecter les licences compatibles.
- Le code est sous AGPL-3.0-or-later, avec en-tête dans chaque fichier source vérifié par outil maison. Fournir `LICENSE` et un README minimal. Les éventuelles ressources distribuées avec le produit doivent conserver leur licence compatible.
- La définition générale du travail terminé exige les tests unitaires passants, `cargo clippy -D warnings`, `cargo fmt --check`, aucune dépendance externe, aucune panique sur les entrées externes et aucune régression aux références visuelles ou aux bancs. Les analyseurs concernés doivent être ajoutés au fuzzer.

## Technical Decisions

- Organiser le dépôt comme un workspace Cargo selon la structure de crates du §6.2 de la spécification ; garder les modules isolés et sans dépendances externes. Les crates `core-wasm`, `desktop-win` et `server` sont les seules exceptions à `#![forbid(unsafe_code)]` ; les autres crates l’appliquent.
- Partager la configuration rustfmt et Clippy. Mettre les scripts et utilitaires internes dans `tools/`, avec des commandes utilisables localement et en CI.
- Garder les tests et bancs reproductibles : corpus initial versionné par cible pour le fuzzing, paramètres de tolérance explicites pour les images et comparaison des performances à une référence enregistrée.
- L’intégration continue suit la chaîne Rust stable et vérifie les trois cibles, les tests et les conventions de dépôt. Aucun environnement de build JavaScript distinct n’est prévu.

## Cross-Story Dependencies

US-001 établit le workspace requis par US-002, US-004 et US-005. US-003 dépend des capacités de rendu introduites par US-041 et US-082 ; ces stories relèvent de fondations ultérieures, de sorte que son intégration complète peut attendre leur disponibilité.
