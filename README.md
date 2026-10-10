# Atelier

Socle de développement de la suite bureautique Atelier, sous AGPL-3.0-or-later. Le workspace comprend 28 crates produit et trois outils internes (garde du lock, vérification de licence et bancs de performance). Les API produit disponibles couvrent les [encodages de texte](docs/us-014-encodings.md), la [compression DEFLATE/zlib](docs/us-020-021-compression.md), la [géométrie 2D](docs/us-060-geometry.md), l'[arbre de nœuds indexé](docs/us-070-indexed-tree.md), les [transactions atomiques](docs/us-071-transactions.md), l'[historique annuler/rétablir](docs/us-072-undo-history.md), la [liste d'affichage](docs/us-080-display-list.md), son [backend SVG](docs/us-081-svg-backend.md), les [archives ZIP](docs/us-022-023-zip.md), le [XML en flux](docs/us-030-031-xml.md) et les [sous-arbres XML opaques](docs/us-032-opaque-xml.md). Les suivis [ZIP/XML](docs/FORMATS-PROGRESS.md) et [XML opaque, historique et SVG](docs/EDITING-PROGRESS.md) rassemblent les preuves des lots. Les autres crates sont des squelettes ; les éditeurs et formats décrits dans `SPEC.md` restent à développer.

## Prérequis

- Rust stable avec `rustfmt` et `clippy` (`rust-toolchain.toml`).
- Pour la cible WASM : `rustup target add wasm32-unknown-unknown`.
- Pour WASM : installer aussi le composant Clippy (`rustup component add clippy`) pour reproduire le lint CI.
- Pour Windows MSVC : toolchain MSVC et éditeur de liens correspondants.
- Pour Linux musl : Rust stable, la cible `x86_64-unknown-linux-musl` et `musl-tools` (`rustup target add x86_64-unknown-linux-musl`; Ubuntu/Debian : `sudo apt-get install musl-tools`). Les tests musl doivent s'exécuter sur Linux.

## Commandes

Avant toute commande Cargo, compiler et lancer le garde directement avec `rustc`. Cela inspecte le lock avant qu'une résolution Cargo puisse le modifier :

```powershell
New-Item -ItemType Directory -Force target | Out-Null
rustc --edition=2024 tools/check-workspace/src/main.rs -o target/check-workspace.exe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
.\target\check-workspace.exe .
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

Sur Linux/macOS, créer d'abord `target` (`mkdir -p target`), utiliser `-o target/check-workspace` puis `./target/check-workspace .`. Si le garde réussit, compiler l'outil et lancer les commandes suivantes :

Le garde prend en charge le sous-ensemble du `Cargo.lock` format 4 généré par Cargo. Il refuse les variantes manuelles non prises en charge, dont les tableaux inline non vides et les escapes TOML Unicode. Après l'ajout volontaire de membres ou de dépendances locales, mettre à jour le lock avec `cargo generate-lockfile --offline`, puis relancer le bootstrap et le garde. La CI vérifie le lock existant et ne le régénère jamais.

```powershell
cargo build -p atelier-check-workspace --locked --offline
cargo test -p atelier-check-workspace --locked --offline
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo build -p atelier-core-wasm -p atelier-unicode -p atelier-geom -p atelier-model -p atelier-compress -p atelier-render -p atelier-zip -p atelier-xml --target wasm32-unknown-unknown --locked --offline
cargo clippy -p atelier-core-wasm -p atelier-unicode -p atelier-geom -p atelier-model -p atelier-compress -p atelier-render -p atelier-zip -p atelier-xml --all-targets --target wasm32-unknown-unknown --locked --offline -- -D warnings
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
cargo run --locked --offline -p atelier-check-license -- .
cargo run --locked --offline --release -p atelier-bench -- --list
cargo run --locked --offline --release -p atelier-bench -- --iterations 100 --output baseline.csv
cargo run --locked --offline --release -p atelier-bench -- --iterations 100 --output current.csv --reference baseline.csv
cargo run --release -p atelier-model --example bench_indexed_tree --locked --offline
```

Sur un hôte Linux avec `musl-tools`, les commandes du job Linux dans [`.github/workflows/ci.yml`](.github/workflows/ci.yml) compilent et exécutent les tests musl des crates compatibles avec cette cible :

```sh
cargo build --workspace --exclude atelier-desktop-win --exclude atelier-core-wasm --target x86_64-unknown-linux-musl --locked --offline
cargo test --workspace --exclude atelier-desktop-win --exclude atelier-core-wasm --target x86_64-unknown-linux-musl --locked --offline
```

La CI déclenche ces contrôles sur `push`, `pull_request` et `workflow_dispatch`. Le job Windows compile et teste tout le workspace sur MSVC, puis exécute Clippy, rustfmt et SPDX. Le job WASM compile et lint `atelier-core-wasm`, `atelier-unicode`, `atelier-geom`, `atelier-model`, `atelier-compress`, `atelier-render`, `atelier-zip` et `atelier-xml` pour `wasm32-unknown-unknown`, puis teste ces crates sur l'hôte Windows natif ; il ne prétend pas exécuter un binaire WASM sous Windows. Le job Ubuntu compile et exécute les tests des crates compatibles sur `x86_64-unknown-linux-musl`. Ces jobs valident des hôtes et configurations distincts ; un passage local Windows ne prouve pas le résultat d'un job distant Linux.

Aucun paquet externe n'est déclaré. `atelier-bench` mesure les cas de socle `utf8_scan` et `sort_u64`, et produit un CSV en nanosecondes. `--case NOM` sélectionne un cas, `--output -` écrit le CSV seul sur la sortie standard, et `--reference FICHIER` compare les mêmes cas. Les mesures acceptent de 1 à 1 000 000 itérations. Le code de sortie vaut 2 pour une régression strictement supérieure à 10 %, 1 pour une erreur d'entrée et 0 en cas de succès. Ces cas ne prétendent pas que les objectifs de `SPEC.md` §6.16 sont atteints.

Pour des mesures comparables, utiliser le profil release, consigner la machine (CPU, mémoire, système), la même charge et laisser le processus au repos avant plusieurs exécutions. Faire une première exécution de chauffe puis l'écarter. Le CSV conserve le nombre d'itérations et la médiane par cas ; il ne compense pas les variations de fréquence, les tâches de fond ou la résolution du chronomètre (plancher d'affichage : 1 ns). Le banc autonome `bench_indexed_tree` mesure 10 000 insertions dans 100 000 blocs, après chauffe, sur cinq échantillons ; sa médiane doit être inférieure à 50 ms sur l'hôte mesuré. Son [protocole et relevé matériel](docs/us-070-indexed-tree.md) précisent la portée de ce résultat. L'avancement et les preuves de validation sont suivis dans `US-V0-V1.md` et [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
