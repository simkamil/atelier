# Front-end web

Le chargeur [wasm-loader.mjs](wasm-loader.mjs) appelle l’ABI manuelle de US-090 en JavaScript vanilla, sans framework ni étape de compilation. Il copie les réponses, libère les allocations et réinitialise l’instance après un trap ; voir le [contrat et les commandes de test](../docs/us-090-wasm-abi.md).

Le client, son Worker et l’interface d’édition restent à développer. Les chaînes d’interface seront externalisées dans des catalogues fr-FR conformément à `SPEC.md`.
