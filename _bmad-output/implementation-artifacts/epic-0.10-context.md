# Epic 0.10 Context: Pont WASM ↔ JS

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Établir le pont manuel entre le moteur Rust compilé en WASM et le JavaScript, puis permettre au worker de traiter les échanges sans bloquer l’interface. Le backlog et la spécification fournissent le contexte disponible ; aucun artefact de planification dédié n’a été trouvé.

## Stories

- US-090 : ABI WASM
- US-091 : Protocole de messages
- US-092 : Moteur dans un Web Worker

## Requirements & Constraints

- Cibler `wasm32-unknown-unknown` avec des exports `extern "C"` écrits à la main, sans `wasm-bindgen`; le front-end reste constitué de modules ES sans étape de build.
- Le JavaScript du worker écrit les entrées dans des allocations WASM et lit les sorties depuis la mémoire linéaire. Cette mémoire reste privée au worker.
- Le pont expose allocation, libération et dispatch. Les erreurs récupérables doivent remonter au JS; après une défaillance fatale, le moteur doit pouvoir être réinitialisé proprement. Le build de release est optimisé pour la taille, suivie par les bancs.
- Les commandes et événements relèvent d’un protocole binaire versionné, avec encodage/décodage symétrique Rust/JS et rejet propre des messages inconnus ou malformés.
- Le worker associe les réponses aux commandes, copie une fois les sorties nécessaires dans un `ArrayBuffer` indépendant, puis peut transférer ce tampon au fil principal. Ne jamais transférer `WebAssembly.Memory.buffer`; V0/V1 ne dépendent pas de `SharedArrayBuffer`.
- Préserver le workspace de production sans dépendance externe et ses lints Cargo; tout octet entrant reste non fiable et ne doit pas provoquer de panique sur une entrée malformée.

## Technical Decisions

- `crates/core-wasm` porte la frontière ABI et dépendances moteur autorisées; le JavaScript correspondant communique via les exports WASM et les modules ES natifs.
- Le moteur WASM est chargé et utilisé dans un Web Worker. Le fil principal ne reçoit que des tampons JS autonomes transférables.
- L’ABI et le protocole sont des couches distinctes : la première transporte les régions mémoire et l’appel, le second définit la signification et la version des messages.
- La chaîne stable compile la cible WASM; la CI compile et lint le crate WASM et exécute les tests du moteur sur la cible native.

## Cross-Story Dependencies

US-090 dépend du workspace initialisé par US-001 et fournit les bases d’appel à US-091. US-091 définit les échanges consommés par US-092. L’interface Host, la capture de saisie et le rendu canvas en aval dépendent du worker de US-092.