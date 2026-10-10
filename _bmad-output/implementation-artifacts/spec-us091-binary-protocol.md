---
title: 'Protocole binaire Rust/JS (US-091)'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: '76efdfee2124112eef5ceee21d67422b062efa80'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.10-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le futur Worker doit échanger des messages corrélés et versionnés avec Rust sans sérialisation JSON. **Approach:** Définir un codec binaire strict et son miroir JS, distinct du transport mémoire US-090 et des futurs handlers US-092.

## Boundaries & Constraints

**Always:** Rust sûr/std seulement et JavaScript ES modules sans dépendance. En-tête v1 de 24 octets little-endian : magic ATLM aux octets0..4; version u16=1 à4; type u16 à6; flags u16=0 à8; réservé u16=0 à10; request_id u64 à12; payload_len u32 à20; payload à24. Longueur totale exacte. IDs non nuls, Rust u64 et JS BigInt obligatoire (1..2^64-1), jamais Number. `MAX_MESSAGE_BYTES = super::MAX_BUFFER_BYTES - 4`, borne incluant l'en-tête, pour laisser le statut ABI. Vérifier tailles et additions avant toute copie/allocation.

Commandes : Ping=0x0001, LoadText=0x0010, ApplyOps=0x0011, Render=0x0012, Cancel=0x0013. Événements : Pong=0x8001, Completed=0x8010, Failed=0x8011, Progress=0x8012, Cancelled=0x8013. Ping/Pong/Cancel/Cancelled : payload vide obligatoire. Les six autres payloads sont des octets opaques, y compris UTF-8 invalide, leur schéma métier appartient aux futures US. Cancel réutilise l'ID de l'opération ciblée; événements réutilisent l'ID de la requête. Pas de simulation d'exécution ou de séquencement.

Rust expose `MessageType`, `Direction::{Command,Event}`, `Message { kind, request_id, payload:Vec<u8> }`, `encode(&Message)->Result<Vec<u8>,ProtocolError>`, `decode(&[u8],Direction)->Result<Message,ProtocolError>`. JS expose `MessageType` constantes, `encodeMessage({type,requestId,payload})`, `decodeMessage(bytes,direction)` avec direction obligatoire 'command'/'event', erreurs `ProtocolError` avec code stable. Payload et bytes d'entrée sont des Uint8Array seulement. Décodage retourne une copie indépendante, encodage copie aussi le payload; respecter byteOffset des sous-vues. API rejette les types JS incorrects, ID Number, ID hors bornes, mauvais sens, type/version/magic/flags/réservé inconnus, troncature/traîne, payload interdit et taille excessive. Catégories d'erreurs stables documentées, aucun panic Rust.

L'ABI dispatch reste l'écho d'octets exact US-090; le test peut transporter puis décoder une trame, sans modifier l'ABI ni inventer de réponse métier. Conserver sa taille de référence et tous ses tests.

**Never:** Worker/file d'attente, handlers documentaires, moteur de commandes, JSON wire, dépendance externe, casts u64 -> Number.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Types v1 | Les dix types, u64 >2^53 et u64::MAX | Octets identiques Rust/JS et décodages croisés | Succès |
| Payload | NUL, bytes arbitraires, accent UTF-8, sous-vue JS | Payload conservé et indépendant | Succès |
| Trame invalide | Chaque troncature, traîne, champ inconnu, longueur mensongère | Refus sans panique | ProtocolError |
| API invalide | Mauvaise direction/type JS/ID/limite | Refus avant allocation/copie inutile | ProtocolError |

</frozen-after-approval>

## Code Map

- `crates/core-wasm/src/lib.rs` — ABI à préserver, export module par coordinateur.
- `web/wasm-loader.mjs` — transport existant pour la preuve de round-trip.
- `tests/wasm-abi.mjs` — conserver toutes les validations US-090.

## Tasks & Acceptance

**Execution:**
- [x] `crates/core-wasm/src/protocol.rs` — types, codecs, erreurs et tests Rust.
- [x] `web/wasm-protocol.mjs` — codecs miroir stricts.
- [x] `crates/core-wasm/examples/protocol_oracle.rs` — petit CLI natif std-only encode/decode pour tests croisés, entrée/sortie hex textuelle uniquement dans cet outil de test.
- [x] `tests/wasm-protocol.mjs` — exécuter chaque type dans les deux directions via le vrai oracle Rust; fixtures golden hex explicites communes, erreurs, ownership, grandes tailles et transport ABI réel (chemin wasm argument optionnel).
- [x] `docs/us-091-binary-protocol.md` — tableau exact wire/types/API/erreurs et limites de portée.

**Acceptance Criteria:**
- Tous les types encodés d'un côté sont décodés à l'identique de l'autre, via processus Rust exécuté (aucun mock de codec).
- Rust/JS rejettent les trames inconnues/malformées, y compris tous préfixes tronqués et erreurs de direction.
- Régressions ABI, compilation WASM, MSVC et musl passent, sans augmentation de dépendances.

## Implementation Notes

Autorisation persistante : développement parallèle GPT-6 Luna moyen et push. Propriété exclusive de l'agent : les cinq fichiers des tâches et notes append-only de cette spec. Le coordinateur gère lib.rs, manifests, CI, preuves, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent; aucun autre fichier modifié. UTF-8 explicite. Cargo ciblé autorisé; rustfmt --edition 2024 uniquement sur fichiers possédés. Le coordinateur prépare l'oracle exécutable à `target/debug/examples/protocol_oracle[.exe]`; le test Node accepte un argument pour ce chemin puis un argument WASM optionnel.

## Spec Change Log

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B9 | Le relevé de validation n'empreinte pas les documents finalisés après les tests. | low | Le relevé d'exécution empreinte les sources et fixtures ; les documents sont clos ensuite. Correction directe : un relevé distinct text-bridge-documentation.json empreinte les quatorze documents finalisés et le relevé d'exécution associé, sans prétendre exécuter des tests sur la prose. |
| E3 | Une mutation SharedArrayBuffer entre la validation de l'en-tête et la copie de payload peut changer la trame décodée. | medium | Reproduit par mutation contrôlée pendant la lecture DataView. Patch interne : copie bornée de la trame entière avant toute validation de son contenu, puis lectures exclusives de cette copie. Le test déterministe SharedArrayBuffer et la propriété de copie Buffer sont conservés. |

Chaque constat est classé avant regroupement ; les patches internes sont validés par les contrôles finaux. La [revue commune](text-bridge-review.md) conserve les treize constats et les contrôles supplémentaires du coordinateur.

## Verification

- `cargo test -p atelier-core-wasm --locked --offline` — codecs et ABI passent.
- `cargo build -p atelier-core-wasm --example protocol_oracle --locked --offline` puis `node tests/wasm-protocol.mjs target/debug/examples/protocol_oracle.exe target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm` — tests croisés/transport passent.

Validation finale : 25 contrôles, 260 tests workspace, 70 tests release. Les sources et le lock sont restés identiques pendant les contrôles. Voir [relevé](text-bridge-validation.json) et [revue triée](text-bridge-review.md). Aucun constat différé.
