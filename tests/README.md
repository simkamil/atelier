# Ressources de test

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
