# Front-end web

Le chargeur [wasm-loader.mjs](wasm-loader.mjs) appelle l’ABI manuelle de US-090 en JavaScript vanilla, sans framework ni étape de compilation. Il copie les réponses, libère les allocations et réinitialise l’instance après un trap ; voir le [contrat et les commandes de test](../docs/us-090-wasm-abi.md).

Le codec [wasm-protocol.mjs](wasm-protocol.mjs) fournit les dix messages binaires
versionnés de US-091, avec IDs BigInt, validation stricte et copies indépendantes.
Le [contrat Rust/JS](../docs/us-091-binary-protocol.md) décrit le format exact.
Les tests croisés exécutent un oracle Rust natif et transportent des trames
dans le véritable WASM, dont dispatch conserve son comportement d'écho.

Le [client et son Worker](../docs/us-092-worker.md) exécutent le transport WASM hors du fil principal. L’interface d’édition reste à développer. Les chaînes d’interface seront externalisées dans des catalogues fr-FR conformément à `SPEC.md`.
