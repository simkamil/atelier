# Atelier

Socle de développement de la suite bureautique Atelier, sous AGPL-3.0-or-later. Le workspace comprend 28 crates produit et cinq outils internes (garde du lock, vérification de licence, bancs de performance, générateur Unicode et fuzzer). Les API produit disponibles couvrent les [encodages de texte](docs/us-014-encodings.md), la [compression DEFLATE/zlib](docs/us-020-021-compression.md), la [géométrie 2D](docs/us-060-geometry.md), l'[arbre de nœuds indexé](docs/us-070-indexed-tree.md), les [transactions atomiques](docs/us-071-transactions.md), l'[historique annuler/rétablir](docs/us-072-undo-history.md), les [sélections](docs/us-073-selection.md), le [journal de récupération](docs/us-074-recovery-journal.md), les [tables OpenType](docs/us-050-opentype-tables.md), la [liste d'affichage](docs/us-080-display-list.md), son [backend SVG](docs/us-081-svg-backend.md), les [archives ZIP](docs/us-022-023-zip.md), le [XML en flux](docs/us-030-031-xml.md) et les [sous-arbres XML opaques](docs/us-032-opaque-xml.md). Les [décodeurs PNG](docs/us-040-png.md), [GIF/BMP](docs/us-043-gif-bmp.md) et les [polices embarquées](docs/us-056-embedded-fonts.md) sont également disponibles. Les [tables Unicode](docs/us-010-ucd-tables.md), [contours TrueType](docs/us-051-truetype-outlines.md) et [ABI WASM](docs/us-090-wasm-abi.md) ainsi que la [segmentation](docs/us-011-segmentation.md), le [crénage/ligatures](docs/us-053-kerning-ligatures.md) et le [protocole Rust/JS](docs/us-091-binary-protocol.md) ainsi que les [coupures de ligne](docs/us-012-line-breaks.md), le [façonnage latin](docs/us-054-latin-shaping.md) et le [gestionnaire de polices](docs/us-055-font-manager.md) préparent le [démonstrateur](docs/PROTOTYPE-PROGRESS.md). Les suivis [ZIP/XML](docs/FORMATS-PROGRESS.md), [XML opaque, historique et SVG](docs/EDITING-PROGRESS.md), [modèle/polices](docs/MODEL-FONT-PROGRESS.md) et [images/fontes embarquées](docs/IMAGES-FONTS-PROGRESS.md) rassemblent les preuves des lots. Les autres crates sont des squelettes ; les éditeurs et formats décrits dans `SPEC.md` restent à développer.

## Prérequis

- Rust stable avec `rustfmt` et `clippy` (`rust-toolchain.toml`).
- Pour la cible WASM : `rustup target add wasm32-unknown-unknown`.
- Pour WASM : installer aussi le composant Clippy (`rustup component add clippy`) pour reproduire le lint CI.
- Pour Windows MSVC : toolchain MSVC et éditeur de liens correspondants ; runtime Microsoft Edge WebView2 Evergreen installé pour lancer le shell. Le chargeur officiel est lié statiquement et vérifié par empreinte.
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
cargo build -p atelier-core-wasm -p atelier-text-model -p atelier-unicode -p atelier-geom -p atelier-model -p atelier-compress -p atelier-render -p atelier-zip -p atelier-xml -p atelier-font -p atelier-image -p atelier-shape -p atelier-raster -p atelier-fmt-ooxml -p atelier-text-layout -p atelier-cfb --target wasm32-unknown-unknown --locked --offline
cargo clippy -p atelier-core-wasm -p atelier-text-model -p atelier-unicode -p atelier-geom -p atelier-model -p atelier-compress -p atelier-render -p atelier-zip -p atelier-xml -p atelier-font -p atelier-image -p atelier-shape -p atelier-raster -p atelier-fmt-ooxml -p atelier-text-layout -p atelier-cfb --all-targets --target wasm32-unknown-unknown --locked --offline -- -D warnings
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

La CI déclenche ces contrôles sur `push`, `pull_request` et `workflow_dispatch`. Le job Windows compile et teste tout le workspace sur MSVC, puis exécute Clippy, rustfmt et SPDX. Le job WASM compile et lint `atelier-core-wasm`, `atelier-text-model`, `atelier-unicode`, `atelier-geom`, `atelier-model`, `atelier-compress`, `atelier-render`, `atelier-zip`, `atelier-xml`, `atelier-font`, `atelier-image`, `atelier-shape`, `atelier-raster`, `atelier-fmt-ooxml`, `atelier-text-layout` et `atelier-cfb` pour `wasm32-unknown-unknown`, puis teste ces crates sur l'hôte Windows natif ; il ne prétend pas exécuter un binaire WASM sous Windows. Le job Ubuntu compile et exécute les tests des crates compatibles sur `x86_64-unknown-linux-musl`. Ces jobs valident des hôtes et configurations distincts ; un passage local Windows ne prouve pas le résultat d'un job distant Linux.

Aucun paquet externe n'est déclaré. `atelier-bench` mesure les cas de socle `utf8_scan` et `sort_u64`, et produit un CSV en nanosecondes. `--case NOM` sélectionne un cas, `--output -` écrit le CSV seul sur la sortie standard, et `--reference FICHIER` compare les mêmes cas. Les mesures acceptent de 1 à 1 000 000 itérations. Le code de sortie vaut 2 pour une régression strictement supérieure à 10 %, 1 pour une erreur d'entrée et 0 en cas de succès. Ces cas ne prétendent pas que les objectifs de `SPEC.md` §6.16 sont atteints.

Pour des mesures comparables, utiliser le profil release, consigner la machine (CPU, mémoire, système), la même charge et laisser le processus au repos avant plusieurs exécutions. Faire une première exécution de chauffe puis l'écarter. Le CSV conserve le nombre d'itérations et la médiane par cas ; il ne compense pas les variations de fréquence, les tâches de fond ou la résolution du chronomètre (plancher d'affichage : 1 ns). Le banc autonome `bench_indexed_tree` mesure 10 000 insertions dans 100 000 blocs, après chauffe, sur cinq échantillons ; sa médiane doit être inférieure à 50 ms sur l'hôte mesuré. Son [protocole et relevé matériel](docs/us-070-indexed-tree.md) précisent la portée de ce résultat. L'avancement et les preuves de validation sont suivis dans `US-V0-V1.md` et [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

La [fenêtre Win32](docs/us-110-win32-window.md) et le [stockage portable](docs/us-116-portable-data.md)
sont disponibles : `cargo run -p atelier-desktop-win --locked --offline` démarre
la coque native sur la [page locale embarquée](docs/us-112-embedded-resources.md) `https://app.atelier/`, avec un contrôle du vrai Worker/WASM. Le [Worker WASM](docs/us-092-worker.md) assure le transport
hors du fil principal. Les modules [Canvas](docs/us-101-canvas-renderer.md) et
[saisie clavier/IME](docs/us-103-text-input.md) disposent de tests Chromium réels.
Le [test strict WebView2](docs/us-111-webview2.md) conserve un échec de handles
sur cet hôte ; US-111 reste ouverte. Le premier [éditeur intégré de texte brut](docs/us-120-editor-ui.md) permet de saisir, sélectionner, annuler/rétablir et utiliser Ouvrir/Enregistrer sous, avec des pages A4 et Carlito 11 pt. Le [pont natif](docs/us-113-file-bridge.md) conserve les octets UTF-8, BOM et fins de ligne. Le périmètre actuel est limité à 128 Kio de texte, latin gauche à droite ; les essais physiques AZERTY/IME, les dialogues manuels et les objectifs de performance restent ouverts.

L’éditeur ajoute la [navigation par mot et paragraphe](docs/us-221-keyboard-editing.md), Ctrl+A, F8/Échap, le [glisser de sélection et le double/triple-clic](docs/us-222-mouse-selection.md), ainsi que [copier/couper/coller en texte brut](docs/us-225-plain-clipboard.md). La colonne visuelle est conservée à travers les lignes courtes. Ces tranches ne clôturent pas les US complètes.

Pour le tester : `cargo run -p atelier-desktop-win --release --locked --offline`, puis Ouvrir un `.txt`, modifier le texte et Enregistrer sous. Le binaire construit est `target/release/atelier.exe`. **Ouvrir DOCX…** affiche désormais un [aperçu riche en lecture seule](docs/rich-docx-preview.md) ; **Retour au texte** retrouve le TXT conservé. L’édition DOCX reste à développer. Le [suivi](docs/PROTOTYPE-PROGRESS.md) distingue les modules livrés des US complètes.
Le [lot US-104/105/106](docs/us-104-105-106-interface.md) ajoute les composants accessibles, les six onglets du ruban et **Affichage → Apparence** pour choisir Clair/Sombre/Système. Les commandes futures sont désactivées. Le lot est validé localement dans Edge et WebView2, y compris le contraste élevé; les essais clavier/lecteur d’écran physiques restent distincts.

Les livraisons parallèles ajoutent l’[encodeur PNG](docs/us-041-png-encoder.md), la [casse et normalisation Unicode](docs/us-013-case-normalization.md), un [décodeur CFF1](docs/us-052-cff-outlines.md) et le [modèle de document riche](docs/us-200-rich-text-model.md). Ce dernier expose ses opérations inversibles aux développeurs ; son raccordement à l’éditeur reste à réaliser.

Les [styles calculés](docs/us-201-style-engine.md), le [paquet OPC](docs/us-300-opc-package.md), le [décodeur JPEG](docs/us-042-jpeg.md) et les [primitives du rasteriseur logiciel](docs/us-061-software-rasterizer.md) complètent ces API. L’[import DOCX](docs/us-301-docx-import.md) fournit désormais texte, styles, thème, paramètres et polices incorporées. Le [moteur de lignes riches](docs/us-210-rich-line-layout.md) est développé et testé; US-210 attend la comparaison avec Word. Le [backend de liste d’affichage raster](docs/us-082-raster-backend.md), le [conteneur CFB](docs/us-311-cfb-container.md) et le [fuzzer de 17 cibles](docs/us-004-fuzzer.md) sont intégrés ; le fuzzer reste partiel pour la mémoire réelle. Le raccordement à l’éditeur riche reste à développer. Les [tabulations](docs/us-211-tabs.md), la [pagination](docs/us-212-pagination.md) et la [géométrie du curseur riche](docs/us-214-caret-geometry.md) partagent désormais les mêmes lignes et pages. US-214 est validée; US-211/212 attendent les comparaisons Word. La [fabrique de document initial](docs/us-202-default-document.md), la [pagination incrémentale](docs/us-213-incremental-layout.md) et les [bordures/trames](docs/us-215-paragraph-borders.md) sont maintenant testées en API. Leur raccordement UI reste ouvert; le benchmark CPU local reste au-dessus de 16 ms. Le [suivi](docs/PROTOTYPE-PROGRESS.md) compte 51/158 US complètes.
