# Front-end web

Le chargeur [wasm-loader.mjs](wasm-loader.mjs) appelle l’ABI manuelle de US-090 en JavaScript vanilla, sans framework ni étape de compilation. Il copie les réponses, libère les allocations et réinitialise l’instance après un trap ; voir le [contrat et les commandes de test](../docs/us-090-wasm-abi.md).

Le codec [wasm-protocol.mjs](wasm-protocol.mjs) fournit les dix messages binaires
versionnés de US-091, avec IDs BigInt, validation stricte et copies indépendantes.
Le [contrat Rust/JS](../docs/us-091-binary-protocol.md) décrit le format exact.
Les tests croisés exécutent un oracle Rust natif et transportent des trames
dans le véritable WASM, dont dispatch conserve son comportement d'écho.

Le [client et son Worker](../docs/us-092-worker.md) exécutent le transport WASM hors du fil principal. L’interface d’édition reste à développer. La page de socle charge ses textes depuis `i18n/fr-FR.txt`; un message français de secours permet de signaler un catalogue indisponible.

## Canvas et saisie

`display-list.mjs` décode ATDL 1.0 produit par Rust. `canvas-renderer.mjs`
rejoue les pages visibles avec DPR, régions sales, cache borné de glyphes et
overlay de composition. Le propriétaire fournit les contours/images résolus.
`text-input.mjs` traduit les événements d’une textarea invisible en commandes
et previews ; le propriétaire fournit la sélection et la géométrie du curseur.
Voir les [contrats Canvas](../docs/us-101-canvas-renderer.md) et
[saisie](../docs/us-103-text-input.md). Ces modules attendent encore leur
branchement au document, aux ressources et aux handlers métier US-120.

## Page embarquée

`index.html`, `app.mjs` et `app.css` présentent le socle local avec Carlito. La table [native de ressources](../docs/us-112-embedded-resources.md) incorpore les modules, le catalogue et le [vrai WASM optimisé](../data/engine/README.md). Le contrôle de la page vérifie l’écho exact de Ping par le Worker ; les handlers d’ouverture/export et l’éditeur restent à construire. Le relais de violations CSP sert uniquement au journal de diagnostics.
