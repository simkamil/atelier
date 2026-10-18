# Front-end web

Le [lot US-104/105/106](../docs/us-104-105-106-interface.md) ajoute `ui-components.mjs`, `theme.mjs`, `ribbon.mjs` et leur intégrateur `editor-shell.mjs`. Le ruban réutilise les nœuds des commandes TXT/DOCX; Affichage → Apparence permet de changer le thème. Les contrôles futurs sont désactivés. Les textes viennent du catalogue existant, compatible avec le futur service US-102.

Le chargeur [wasm-loader.mjs](wasm-loader.mjs) appelle l’ABI manuelle de US-090 en JavaScript vanilla, sans framework ni étape de compilation. Il copie les réponses, libère les allocations et réinitialise l’instance après un trap ; voir le [contrat et les commandes de test](../docs/us-090-wasm-abi.md).

Le codec [wasm-protocol.mjs](wasm-protocol.mjs) fournit les messages binaires
versionnés de US-091, avec IDs BigInt, validation stricte et copies indépendantes.
Le [contrat Rust/JS](../docs/us-091-binary-protocol.md) décrit le format exact.
Les tests croisés exécutent un oracle Rust natif et transportent des trames
dans le véritable WASM. Les anciens types gardent leur écho de transport ; Editor/EditorResult portent la session métier.

Le [client et son Worker](../docs/us-092-worker.md) exécutent le transport WASM hors du fil principal. Le [contrôleur d’édition](../docs/us-120-editor-ui.md) relie la saisie, la session WASM et le Canvas. La page charge ses textes depuis `i18n/fr-FR.txt`; un message français de secours permet de signaler un catalogue indisponible.

## Canvas et saisie

`display-list.mjs` décode ATDL 1.0 produit par Rust. `canvas-renderer.mjs`
rejoue les pages visibles avec DPR, régions sales, cache borné de glyphes et
overlay de composition. Le propriétaire fournit les contours/images résolus.
`text-input.mjs` traduit les événements d’une textarea invisible en commandes
et previews ; le propriétaire fournit la sélection et la géométrie du curseur.
Voir les [contrats Canvas](../docs/us-101-canvas-renderer.md) et
[saisie](../docs/us-103-text-input.md). `rich-editor.mjs` les raccorde au document et aux handlers métier US-120 ; la validation physique du clavier/IME et les performances restent ouvertes.

## Page embarquée

`mouse-selection.mjs` traduit les points du Canvas en coordonnées de page et gère capture, glisser, double/triple-clic et défilement aux bords. Le moteur WASM choisit les frontières de graphèmes, mots et paragraphes. `rich-editor.mjs` fournit à `text-input.mjs` un instantané stable de la sélection pour copier/couper uniquement du texte brut ; les commandes restent dans la même queue. Ctrl+A quitte F8 et sélectionne tout, F8 active l’extension et Échap l’arrête. Voir les contrats [clavier](../docs/us-221-keyboard-editing.md), [souris](../docs/us-222-mouse-selection.md) et [presse-papiers](../docs/us-225-plain-clipboard.md).

`index.html`, `app.mjs` et `app.css` présentent l’éditeur texte brut avec Carlito. La table [native de ressources](../docs/us-112-embedded-resources.md) incorpore les modules, le catalogue et le [vrai WASM optimisé](../data/engine/README.md). Le contrôle natif vérifie Ping, la saisie réelle via Worker/WASM, des pixels de glyphes, undo/redo et le retour à un document propre. `native-host.mjs` implémente les requêtes fichiers bornées ; le relais CSP conserve son journal distinct. Les tests utilisent aussi un Host factice sans dialogue.
