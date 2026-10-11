---
title: 'US-092 — Worker WASM avec transfert et annulation'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'c091f6a52aa58af94966592ff356750471200d44'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.10-context.md'
---

<frozen-after-approval reason="intent supplied by persistent user authorization">

## Intent

Le moteur doit fonctionner dans un Worker ES pour garder le fil principal disponible. Construire le transport FIFO corrélé au protocole v1 et des transferts réellement annulables avec progression. Les handlers métier restent à US-120 : l'ABI conserve son écho exact, sans prétendre charger un modèle ou exporter un document.

## Boundaries & Constraints

JavaScript natif sans dépendance et sans SharedArrayBuffer. Réutiliser le loader et le codec existants. Un appel client reçoit une trame commande v1, déduit son ID BigInt par le codec et retourne les octets exacts du dispatch. L'écho ABI n'est pas présenté comme un événement métier Completed. Cancel utilise l'ID de l'opération ciblée conformément au contrat US-091. Les enveloppes Worker de contrôle ne remplacent pas le codec métier.

Le loader ajoute `dispatchAsync(input,{signal,onProgress,chunkBytes})` : copie réelle par blocs de l'entrée vers l'allocation WASM, cession à la file des tâches entre blocs, dispatch synchrone, copie par blocs vers un unique tampon JS exact. Chaque octet de sortie est copié une seule fois depuis WASM, puis ce même buffer est transféré. Le transfert détache l'émetteur. Ne jamais transférer la mémoire WASM ni recopier la réponse via encode/decode. Signaler des compteurs de progression monotones correspondant aux octets effectivement copiés ; annuler avant dispatch ou pendant copie sortie libère les allocations. L'appel WASM synchrone n'est pas préemptible ; ce contrat constitue la base coopérative pour les futures opérations métier. Interdire les dispatch concurrents sur une même instance pour protéger les vues après await. Préserver `dispatch` synchrone, tous tests ABI et taille WASM.

Le client fournit une API documentée pour init (module/bytes), requête, AbortSignal et fermeture. Rejeter les IDs actifs dupliqués, entrées invalides, réponses d'ID inconnu ou incompatibles ; la clôture et les erreurs fatales règlent toutes les promesses. Le runtime initialise une seule instance privée, sérialise les commandes, traite Cancel immédiatement même pendant une requête en cours et autorise la suite après une annulation. Borner la file (nombre et octets) avant son admission et documenter les limites. Les exceptions de callbacks progression ne doivent pas bloquer la file. Ne pas présenter les tests Node comme une validation de clavier/IME ou de WebView2.

## I/O & Edge-Case Matrix

| Situation | Résultat | Erreur |
|---|---|---|
| Plusieurs IDs u64 au-delà de 2^53 | FIFO, réponse exacte et corrélée | aucune |
| Grande trame / sous-vue | octets exacts, sortie détachée côté Worker, commande suivante réussit | aucune |
| Cancel actif ou en attente | pas de succès tardif, nettoyage, commande suivante réussit | AbortError |
| Abort avant appel / double ID / trame invalide | refus sans fuite ni désorganisation | erreur stable |
| Init/Worker fatal ou fermeture | toutes promesses réglées et listeners retirés | erreur explicite |
| Callback qui lève / progression | progression réelle bornée et monotone, autres requêtes fonctionnent | callback isolé |

</frozen-after-approval>

## Code Map

- `web/wasm-loader.mjs` — alloc/dispatch/free, statut, trap/reset, copie indépendante ; conserver les garanties de nettoyage.
- `web/wasm-protocol.mjs` — codec v1 strict, `decodeMessage` copie son payload ; réservé à l'admission, pas à la sortie transférée.
- `tests/wasm-abi.mjs` — vraie ABI et hooks panic à préserver.
- `.github/workflows/ci.yml` — intégration par coordinateur.

## Tasks & Acceptance

- [x] `web/wasm-loader.mjs` — ajouter le dispatch coopératif réel sans modifier l'ABI Rust.
- [x] `web/engine-worker.mjs` — runtime Worker et point d'entrée navigateur.
- [x] `web/engine-client.mjs` — client FIFO/ID, annulation et cycle de vie.
- [x] `tests/worker-node-entry.mjs` et `tests/wasm-worker.mjs` — Worker threads réel utilisant les modules de production et les artefacts WASM, erreurs, transfert/détachement, annulation en cours, FIFO et fermeture. Observer le détachement dans le port de test après le vrai postMessage, sans recopier.
- [x] `docs/us-092-worker.md` — API, limites exactes, étapes d'intégration et preuves.

Given des trames valides, when le Worker réel les traite, then tous les octets et IDs sont conservés dans l'ordre sans mémoire WASM transférée. Given une grande entrée, when Cancel arrive après une progression non finale, then le traitement est annulé, les tampons libérés et une nouvelle requête réussit. Given un fatal/close, when des requêtes attendent, then aucune promesse ne reste pendante.

## Implementation Notes

Propriété exclusive : fichiers des tâches et notes append-only de cette spec. Coordinateur : CI, manifestes, validation globale, Graphify, Git et clôture docs. Pas de sous-agent/skill/Graphify/commit/push. UTF-8/LF explicite. Les autres agents implémentent le shell Windows dans d'autres fichiers. Tester Node ciblé sur `target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm`, hook optionnel à `target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm`.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Constat | Preuve et décision | Route |
|---|---|---|---|---|
| B7 | medium | Signal incomplet laisse une promesse non réglée | request vérifie addEventListener seul ; removePending appelle removeEventListener après suppression de la Map et avant resolve/reject. Un objet admis sans cette méthode reproduit le blocage. | patch Worker |
| B9 | medium | Banc navigateur absent de CI | Le banc existe mais seul worker_threads est invoqué. Le bootstrap WorkerGlobalScope navigateur peut régresser sans échec Node ; même cause que V2. | patch navigateur |
| E1 | medium | Worker silencieux : initialisation sans limite | await ready ne comporte aucun délai. Un Worker vivant qui ignore init laisse createEngineClient pendante ; un délai privé doit utiliser failAll et retirer les listeners. | patch Worker |
| V2 | medium | Bootstrap navigateur par défaut non automatisé | Constat pré-vérifié : le point d’entrée Node installe explicitement le runtime ; il ne couvre pas le guard WorkerGlobalScope du navigateur. Automatiser le banc dans un vrai Edge headless local et CI. | patch navigateur |


## Verification

- `node tests/wasm-worker.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm` — FIFO, transfert, nettoyage et progression/annulation réels.
- `node tests/wasm-abi.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm` — aucune régression ABI.

Les routes patch du journal sont résolues et couvertes par les tests ciblés. Le journal complet et ses preuves sont dans [worker-shell-review.md](worker-shell-review.md). Aucun constat différé ; clôture soumise à la validation globale.

Le transport est implémenté et validé par les contrôles du lot. US-092 reste ouverte : le critère backlog de progression/annulation d’une ouverture ou d’un export métier attend les handlers documentaires. Les copies coopératives seules ne le clôturent pas.
