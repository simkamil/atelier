# Front-end web

Le chargeur [wasm-loader.mjs](wasm-loader.mjs) appelle l’ABI manuelle de US-090 en JavaScript vanilla, sans framework ni étape de compilation. Il copie les réponses, libère les allocations et réinitialise l’instance après un trap ; voir le [contrat et les commandes de test](../docs/us-090-wasm-abi.md).

Le codec [wasm-protocol.mjs](wasm-protocol.mjs) fournit les dix messages binaires
versionnés de US-091, avec IDs BigInt, validation stricte et copies indépendantes.
Le [contrat Rust/JS](../docs/us-091-binary-protocol.md) décrit le format exact.
Les tests croisés exécutent un oracle Rust natif et transportent des trames
dans le véritable WASM, dont dispatch conserve son comportement d'écho.

Le [client et son Worker](../docs/us-092-worker.md) exécutent le transport WASM hors du fil principal. L’interface d’édition reste à développer. Les chaînes d’interface seront externalisées dans des catalogues fr-FR conformément à `SPEC.md`.

## Canvas et saisie

`display-list.mjs` décode ATDL 1.0 produit par Rust. `canvas-renderer.mjs`
rejoue les pages visibles avec DPR, régions sales, cache borné de glyphes et
overlay de composition. Le propriétaire fournit les contours/images résolus.
`text-input.mjs` traduit les événements d’une textarea invisible en commandes
et previews ; le propriétaire fournit la sélection et la géométrie du curseur.
Voir les [contrats Canvas](../docs/us-101-canvas-renderer.md) et
[saisie](../docs/us-103-text-input.md). Ces modules attendent encore leur
branchement au document, aux ressources et aux handlers métier US-120.
