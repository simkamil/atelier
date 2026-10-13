# Ressources de test

Les [composants, thèmes et ruban](../docs/us-104-105-106-interface.md) sont vérifiés par `node tests/theme.mjs`, `node tests/run-ui-browser.mjs` et son mode `--forced-colors`. Le runner utilise Edge/Chromium installé, le DOM réel et le WASM emballé; il contrôle aussi les media queries système et les vues 1280/420 px via CDP. `--screenshots` écrit les captures sous `target/ui-screenshots/`. Les événements synthétiques ne prouvent pas la validation physique clavier ou lecteur d’écran. Les suites UI tournent dans le job Windows de CI.

`format-reference/` contient des petits corpus ZIP et XML indépendants, leurs
sémantiques attendues et leurs empreintes SHA-256. Les tests Rust consomment les
fichiers versionnés sans dépendance Python. Le générateur de références est un
outil d'agent sous `_bmad-output/implementation-artifacts/`.

Les octets sont conservés tels quels par `.gitattributes`, notamment les
encodages malformés, BOM UTF-16 et fins de ligne utilisés pour tester XML.
Les corpus de documents complets restent à ajouter avec leurs stories.
Le corpus d’images de référence est décrit ci-dessous.

Deux archives Writer classiques/Zip64 ont été validées par Python zipfile (noms, contenus et CRC); leurs octets figés sont comparés aux sorties du writer dans les tests Rust CI. Le manifeste conserve les entrées, options et SHA-256, et le générateur reproduit ces références.

Les tests Rust de XML opaque, historique et SVG vivent dans leurs crates et
s’exécutent en CI sans runtime externe. Les comparaisons à ElementTree et à un
modèle d’historique, ainsi que la rasterisation SVG indépendante, sont des
contrôles complémentaires d’agent : voir le [suivi du lot](../docs/EDITING-PROGRESS.md).

Les fixtures synthétiques OpenType vivent dans `crates/font/tests/fixtures/` ;
les tests Rust du parseur, des sélections et du journal s’exécutent dans la CI
sans Python. Le contrôle complémentaire `check-model-font-oracle.py` compare
les faces à FontTools, les positions à un modèle Python et les enveloppes
binaires à `struct`/`zlib.crc32`. Ces outils restent réservés aux agents ; voir
le [suivi du lot](../docs/MODEL-FONT-PROGRESS.md).

Le [corpus d’images](image-reference/README.md) fournit PngSuite complet et des
GIF/BMP avec pixels RGBA de référence. Le test Cargo du corpus tourne dans les
jobs natifs de CI; les oracles Python sont des outils d’agent uniquement.

`font-outline-reference/` contient 300 contours de référence FontTools issus de
vingt faces. Les tests Rust comparent les contours aplatis à une tolérance de
0,01 unité de police. `unicode-reference/` conserve les empreintes indépendantes
des 29 consultations sur chaque scalaire et des règles de casse conditionnelle ;
le test natif les recalcule en CI. Le garde `check-ucd.ps1` vérifie les sources
Unicode, leur licence et les corpus. `wasm-abi.mjs` exerce les vrais artefacts
optimisés produit/test-hooks dans Node, notamment allocations, croissance,
quota mémoire et panic/réinitialisation. Voir le [suivi](../docs/PROTOTYPE-PROGRESS.md).

Les corpus officiels GraphemeBreakTest (766 cas) et WordBreakTest (1 944 cas)
sont exécutés intégralement. Un second test vérifie navigation et itération
dans les deux sens depuis chaque position UTF-8 de ces 2 710 cas, y compris
les offsets invalides. Le [corpus typographique](font-layout-reference/README.md)
contient 54 716 sorties indépendantes HarfBuzz sur les vingt faces, consommées
par Cargo en CI sans installer l'oracle Python.

`wasm-protocol.mjs` compare les dix messages dans les deux directions entre
Rust et JavaScript, avec golden frames et erreurs. `wasm-protocol-boundaries.mjs`
vérifie la capacité maximale exacte et son dépassement d'un octet. Reproduction :

```powershell
cargo build -p atelier-core-wasm --example protocol_oracle --locked --offline
node tests/wasm-protocol.mjs target/debug/examples/protocol_oracle.exe target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm
node tests/wasm-protocol-boundaries.mjs
```

Le lot US-012/054/055 ajoute LineBreakTest complet (19 338 cas), ses contextes LB9 enrichis, le contrôle EAW sur tous les scalaires, et la façade de façonnage confrontée aux 54 716 sorties HarfBuzz à une taille/chasse fractionnaire. Les tests du registre vérifient familles, poids, styles, sources, couverture, TTC et atomicité. L'oracle de fontes système est un outil Windows local ; ses fontes ne sont pas distribuées et les tests Cargo restent autonomes.

Les tests [Worker](wasm-worker.mjs) utilisent un vrai Worker Node et le vrai WASM :
FIFO, octets exacts, transfert/détachement, progression/annulation et reprise.
`cargo test -p atelier-desktop-win --locked --offline` couvre stockage et géométrie ;
la CI lance aussi `atelier.exe --self-check target/shell-check/window-state.txt`
pour exercer une fenêtre Win32 cachée sans interaction. Voir les [preuves](../_bmad-output/implementation-artifacts/worker-shell-validation.json).

## Front-end et hébergement natif

```powershell
cargo build -p atelier-render --example display_list_oracle --locked --offline
node tests/display-list.mjs
node tests/run-frontend-browser.mjs
pwsh -NoProfile -File tests/check-webview-assets.ps1
cargo build -p atelier-desktop-win --locked --offline
.\target\debug\atelier.exe --webview-self-check target/webview-strict-check
```

Le dernier contrôle est strict et reste en échec sur cet hôte : US-111 n’est pas
clôturée. La CI appelle `check-webview-host.ps1 -Executable ... -TargetDirectory ...
-AllowKnownRuntimeHandleGrowth` pour distinguer les 100 cycles fonctionnels et
les callbacks équilibrés de `handle_acceptance_passed: false`. Une erreur
fonctionnelle, un timeout, un callback restant ou un autre diagnostic échoue
ce wrapper. Sans option, il conserve également l’échec de handles.

`run-frontend-browser.mjs` lance Edge/Chrome headless sur un serveur local,
avec profil temporaire isolé et suppression de son propre profil. Il vérifie
Canvas, saisie et composition intégrée. `canvas` ou `input` limite les suites.
Le benchmark software n’est pas une validation de fluidité matérielle ; les
événements synthétiques ne remplacent pas les tests AZERTY/IME natifs.

## Ressources embarquées, fichiers et texte brut

`node tests/mouse-selection.mjs` vérifie capture, coalescence asynchrone, zoom, pages, scroll, annulation et erreurs de callbacks avec une horloge contrôlée. `run-frontend-browser.mjs input` ajoute les cas copy/cut du modèle (UTF-8, U+FEFF de contenu, sélection inversée et échec d’écriture). `run-editor-browser.mjs` couvre les raccourcis Ctrl par mot/paragraphe, F8/Ctrl+A/Échap, suppression par mot et undo, double/triple-clic et glisser via le vrai Worker/WASM. Les événements synthétiques et captures simulées ne remplacent pas les essais natifs. `editor-abi.mjs` injecte onze commandes SelectPoint invalides directement dans le vrai WASM et vérifie l’absence de mutation.

Le premier éditeur intégré se vérifie avec `node tests/editor-protocol.mjs`,
`node tests/editor-abi.mjs` et `node tests/run-editor-browser.mjs` après un build
WASM optimisé. Le test ABI exerce la limite réelle de 128 Mio d’allocations et
vérifie qu’un candidat refusé conserve document, révision, sélection et historique.
Le navigateur utilise le vrai Worker/WASM/Canvas, avec Host factice et événements
synthétiques. Le diagnostic natif des ressources vérifie aussi saisie accentuée,
pixels de glyphes et undo/redo dans le WebView2 réel ; il n’ouvre pas de dialogue.

`node tests/check-embedded-engine.mjs` vérifie provenance, empreintes, imports/exports et écho du moteur livré, sans compilation préalable. Après un build optimisé avec la même toolchain, `--compare-artifact` compare les octets à l’artefact Cargo. `pwsh -NoProfile -File tests/check-embedded-resources.ps1 -Executable target/release/atelier.exe -TargetDirectory target/embedded-resources` attend la fin du diagnostic réel caché, y compris avec le sous-système Windows release. WebView2 Evergreen exposant `ICoreWebView2_22` est requis.

`cargo test -p atelier-text-model --locked --offline` couvre UTF-8, sélection, graphèmes et historique. `cargo test -p atelier-desktop-win --locked --offline` couvre le broker fichiers, les pannes de publication et les ressources ; aucun dialogue interactif n’est ouvert. Les erreurs simulées vérifient la préservation des fichiers avant publication, sans promettre de CAS atomique face à un processus extérieur.

## DOCX et lignes riches

Le [corpus DOCX indépendant](docx-reference/README.md) est généré et relu par
Python ZIP/XML, avec un vrai Carlito ODTTF, des parties relocalisées et une partie
opaque. `py tools/generate-docx-reference.py --check` contrôle ses octets exacts;
les jobs Cargo consomment les fixtures versionnées sans dépendance Python.
`cargo test -p atelier-fmt-ooxml -p atelier-text-layout --locked --offline`
exerce import, contrôles malformés, styles, polices et géométrie riche intégrée.

Le [collecteur de lignes Word](text-layout-reference/README.md) nécessite Word
desktop. Sa collecte reste non exécutée ici; aucun golden Word ni taux ≥95 %
n’est simulé. US-210 demeure partielle. Les tests numériques et les références
HarfBuzz existantes ne remplacent pas cet oracle indépendant de mise en page.
# Glisser-déposer et espaces françaises — 2026-10-12

`node --test tests/text-drag-drop.mjs tests/french-spacing.mjs` couvre les contrôleurs purs, positions UTF-8, événements périmés, lecture synchrone du DataTransfer, marqueurs, modificateurs et corrections typographiques. `cargo test --locked --offline -p atelier-text-model -p atelier-core-wasm` couvre les transactions atomiques et le protocole; les cas supplémentaires sont dans leurs fichiers `tests/drag_spacing.rs`.

`node tests/run-editor-browser.mjs` vérifie déplacements/copies/dépôts, export exact, Annuler/Rétablir, guillemets, ponctuation et l'option réelle dans le navigateur avec Worker/WASM. Les événements de dépôt et pointeur sont synthétiques: l'essai natif interapplication Windows reste une validation manuelle distincte. `node tests/editor-abi.mjs` exerce directement les commandes sur le WASM optimisé et leurs refus sans mutation.
