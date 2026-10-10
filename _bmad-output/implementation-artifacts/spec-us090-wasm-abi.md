---
title: 'ABI WASM manuelle (US-090)'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: '20a6a05bd69d22f4f004b7a907d2c9696b24c4ad'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.10-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le moteur WASM est un squelette sans point d’appel JavaScript. **Approach:** Fournir allocations bornées, dispatch binaire minimal et chargeur ES module avec propriété explicite des buffers et réinitialisation après trap.

## Boundaries & Constraints

**Always:** wasm32, exports `alloc(len:u32)->u32`, `free(ptr:u32,len:u32)->i32`, `dispatch(ptr:u32,len:u32)->u32`, memory; extern C et no_mangle édition 2024. free vaut 0 si succès, -1 sinon; (0,0) est no-op et alloc(0)=0. Registry d’allocations actives exactes pour rejeter pointeurs invalides/double free/tailles incorrectes sans déréférencer; préférer Box<[u8]> possédé dans registry et manipulation sûre aux raw_parts. Aucune référence ne survit à un appel JS. MAX_BUFFER_BYTES=64 MiB, MAX_TOTAL_BYTES=128 MiB, MAX_ALLOCATIONS=1024; arithmétique et allocation vérifiées.

Dispatch consomme/libère uniquement une entrée correspondant à une allocation active exacte; pointeur invalide ne libère rien. Retour : offset d’un descripteur 8 octets LE `{payload_ptr:u32,payload_len:u32}`. Payload commence par statut u32 LE (0 succès, 1 entrée invalide, 2 limite/alloc, 3 commande non prise en charge), suivi des données. Descriptor et payload libérés séparément avec leurs tailles exactes. Retour zéro = échec fatal à remonter avec reset. Pour cette US, seul comportement transport est l’écho des octets d’entrée (s’ils tiennent avec statut); les commandes métier et le protocole versionné sont US-091, sans prétendre les avoir implémentés.

Chargeur `createWasmEngine(bytesOrModule)` asynchrone puis `dispatch(Uint8Array)` synchrone renvoie une copie indépendante du body ou une erreur typée. Copier avant free; recréer DataView/Uint8Array après chaque appel pouvant memory.grow; valider descripteurs/ranges et free dans finally. Aucune vue de la mémoire privée exposée/transférée. Sur WebAssembly.RuntimeError, abandonner l’instance, en créer une neuve depuis le Module compilé, remonter EngineTrapError et ne jamais rejouer automatiquement la commande. Panic WASM abort est un trap, pas un catch_unwind récupérable. Test feature `abi-test-hooks` permet de provoquer un vrai panic dans dispatch avec entrée réservée `__atelier_test_panic__`; absente du binaire produit. Profil optimisé `wasm-release` coordonné par le parent; mesurer octets du .wasm et enregistrer référence benchmark.

**Never:** wasm-bindgen, framework JS, état métier factice, unsafe non nécessaire, promesse de récupération de l’état du moteur après trap, WebAssembly.Memory.buffer transféré, global profile release affectant les autres crates.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Écho | buffer valide, vide ou binaire | copie exacte; allocations libérées, appel suivant possible | Succès |
| Mémoire invalide | pointer/length/double-free, quota | rejet sans accès mémoire illégal ni libération étrangère | statut/retour défini |
| Croissance | allocation agrandit memory | vues recréées et résultat exact | Succès |
| Panic | vrai WASM test-hooks dans dispatch | erreur JS puis nouvel appel réussi sur instance réinitialisée | EngineTrapError |

</frozen-after-approval>

## Code Map

- `crates/core-wasm/src/lib.rs` et nouveaux modules — ABI et registry; crate déjà exempté de forbid unsafe, privilégier code sûr.
- `crates/core-wasm/tests/` — tests natifs des invariants; les offsets WASM se testent dans le vrai WASM, pas par conversion tronquée de pointeurs hôte 64 bits.
- `web/wasm-loader.mjs` — module ES autonome (Node et futur Worker).
- `tests/wasm-abi.mjs` — test Node avec vrais artefacts produit et test-hooks; aucun npm nécessaire.
- `Cargo.toml` et CI — profil/build cdylib/test JS raccordés par le coordinateur.

## Tasks & Acceptance

**Execution:**
- [x] `crates/core-wasm/src/**`, `tests/**` — registry, exports et tests bornes/ownership; hook sous feature seulement.
- [x] `web/wasm-loader.mjs`, `tests/wasm-abi.mjs` — contrat complet et tests vrais WASM, croissance, erreur et panic/reset.
- [x] `docs/us-090-wasm-abi.md` — ABI/ownership, limites, commandes build, taille et reset documentés.

**Acceptance Criteria:**
- Given un vrai WASM optimisé, when Node appelle écho/free puis dispatch suivant, then résultats exacts sans allocations résiduelles liées aux requêtes.
- Given des pointeurs invalides, doubles free et tailles/quota invalides, when exports appelés directement, then mémoire et autres allocations restent valides.
- Given un vrai panic de l’artefact test-hooks, when chargeur intercepte le trap, then erreur remontée et commande suivante réussit sur une nouvelle instance.

## Verification

- `cargo test -p atelier-core-wasm --locked --offline` — invariants natifs.
- `node tests/wasm-abi.mjs PRODUIT.wasm TEST_HOOKS.wasm` — vrais exports, buffers et trap/reset.

## Implementation Notes

Autorisation persistante de développer, valider et publier les US en parallèle avec GPT-6 Luna moyen. Le coordinateur gère manifests partagés, lock, exports partagés, CI, preuves, Graphify et Git. L’agent ne lance ni skill, ni Graphify, ni sous-agent. UTF-8 explicite; aucune modification hors propriété indiquée.

## Spec Change Log

## Review Triage Log

Propriété exclusive : crates/core-wasm/src/**, crates/core-wasm/tests/**, web/wasm-loader.mjs, tests/wasm-abi.mjs et doc US-090. Le parent raccorde manifests/profils Cargo et CI.


| ID | Constat | Verdict et preuve | Disposition |
|---|---|---|---|
| B10 | Gardes du chargeur sur descripteur/payload malformé non testées | medium : les exports normaux ne produisent pas ces cas, mais les branches protègent une frontière binaire. | Doubles contrôlés des exports pour erreurs/cleanup, croissance, reset raté ; tests réels WASM conservés. |
| R1 | Réinitialisation fatale conserve l’ancienne instance si instantiate échoue | medium : la réaffectation de state attendait le retour de instantiate ; une validation d’exports ratée laissait donc l’ancienne instance accessible. | Instance abandonnée avant la tentative, EngineResetError avec cause, tests des deux issues et absence de rejeu. |
| R2 | Seuil affiché arrondi au-dessus du seuil entier accepté | low : l’affichage ceil annonçait57556, mais le test exact +10% accepte jusqu’à57555. | Affichage floor cohérent avec le contrôle et documentation corrigée. |

Validation finale : 21 contrôles, 233 tests workspace et 43 tests release. Sources et lock inchangés durant les contrôles. Voir [relevé](prototype-foundations-validation.json) et [revue complète](prototype-foundations-review.md). Aucun constat différé.
