# Contrats du lot Worker et coque native

US-110 et US-116 sont candidates à clôture après validation. US-092 reste partielle : le transport existe, mais son critère métier ouverture/export annulable reste ouvert dans le backlog. Aucun éditeur ni WebView2 ne sont revendiqués.

---
title: 'US-092 — Worker WASM avec transfert et annulation'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
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

## Verification

- `node tests/wasm-worker.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm` — FIFO, transfert, nettoyage et progression/annulation réels.
- `node tests/wasm-abi.mjs target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm target/wasm-abi-hooks/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm` — aucune régression ABI.


---
title: 'US-110 — Fenêtre Win32 persistante et DPI'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
baseline_commit: 'c091f6a52aa58af94966592ff356750471200d44'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="intent supplied by persistent user authorization">

## Intent

Fournir la fenêtre native qui hébergera WebView2. La coque doit démarrer, recevoir les messages Win32, suivre le thème système et le DPI par écran, et restaurer une géométrie utilisable après redémarrage ou retrait d'un écran. Cette US ne prétend pas afficher un éditeur.

## Boundaries & Constraints

Rust std uniquement, FFI `extern system` manuelle isolée dans `win32.rs`, blocs unsafe avec justification de validité et de durée de vie. Tous les types externes ont repr(C), signatures conformes au SDK. API publique `window::initialize_dpi()->std::io::Result<()>` à appeler avant tout HWND ; `window::run(state_file:&Path,title:&str)->io::Result<()>` utilise le chemin déjà résolu par US-116, sans chercher une autre racine. Le coordinateur possède lib.rs/main.rs et les relie ; ne pas modifier ces fichiers. Tout HWND et contexte sont libérés une fois, le callback FFI ne laisse jamais une panique traverser l'ABI et ne garde pas de pointeur temporaire. Éviter état global mutable non protégé. Pas de dépendance crate/SDK généré/COM/WebView2 à ce stade.

Activer Per-Monitor V2 avant toute création. Suivre WM_DPICHANGED en appliquant le rectangle conseillé, traiter WM_SIZE et conserver la future surface cliente accessible. Barre système standard, mode clair/sombre via DwmSetWindowAttribute et réglage système lu en lecture seule, réactualisé sur WM_SETTINGCHANGE/WM_THEMECHANGED. Ne jamais écrire le registre. Boucle GetMessageW distingue erreur, quit et message ; WM_CLOSE/WM_DESTROY assurent la fermeture et la persistance sans boucler après quit.

État versionné borné : rectangle normal, DPI, maximisé. Ne pas mémoriser une géométrie minimisée comme état normal. Écrire dans le fichier fourni, remplacement atomique dans le même dossier, erreur explicite sans perdre le précédent. La restauration choisit l'écran concerné ou le plus proche, convertit correctement les coordonnées workspace/screen de WINDOWPLACEMENT, puis borne taille/position à son work area, y compris écrans négatifs et changement DPI. La taille minimale doit rester utilisable sans dépasser un petit écran. Un fichier absent/corrompu/trop grand utilise un défaut sûr ; ne pas allouer selon une longueur arbitraire.

Fournir `window::self_check(state_file:&Path)->io::Result<String>` sous Windows : crée une vraie fenêtre cachée, vérifie DPI et surface cliente, envoie un changement DPI contrôlé, teste sauvegarde/restauration et détruit la fenêtre sans interaction visible. L'état de test reste sous target. Les tests géométriques doivent couvrir plusieurs écrans synthétiques (ce n'est pas une preuve d'un vrai dispositif multi-écran).

## I/O & Edge-Case Matrix

| État | Comportement | Échec |
|---|---|---|
| Première ouverture | fenêtre normale, PMv2, surface cliente non vide | io::Error |
| Rectangle/maximisé mémorisés | état normal et maximisé restaurés | état corrompu → défaut |
| Écran retiré, coordonnées négatives, DPI différent | fenêtre bornée dans work area | aucune panique |
| Minimise puis ferme | rectangle normal préservé | persistance explicite |
| Changement DPI/thème | rectangle conseillé et barre système adaptés | erreur API contrôlée |
| Propriétés/format invalides, fichier énorme | défaut borné, lecture limitée | aucune panique |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/main.rs` — squelette, intégration par coordinateur.
- `crates/desktop-win/src/lib.rs` — exports data/window par coordinateur.
- `SPEC.md` §6.8, §7.2 — FFI système et racine portable commune.
- `crates/desktop-win/src/data.rs` — autre agent, ne pas modifier ; le chemin d'état arrive en paramètre.

## Tasks & Acceptance

- [x] `crates/desktop-win/src/window.rs` — état versionné, géométrie sûre, orchestration, API et tests purs.
- [x] `crates/desktop-win/src/win32.rs` — liaisons, boucle de messages et propriétaire HWND, PMv2/thème, persistance native et self_check.
- [x] `docs/us-110-win32-window.md` — APIs, format d'état, validation et limites matérielles.

Given Windows, when la coque démarre puis ferme, then la boucle est saine et taille/position/maximisé sont persistés. Given un écran disparu ou un état hostile, when la fenêtre est restaurée, then la barre et surface restent dans une work area. Given un test natif caché, when DPI change, then la fenêtre suit le rectangle demandé et ferme proprement.

## Implementation Notes

Propriété exclusive : tâches et notes append-only. Coordinateur possède main/lib/CI/Graphify/Git. Pas de sous-agent/skill/Graphify/commit/push. UTF-8/LF explicite, commentaires SPDX. Agent voisin data peut être encore en cours : attendre son fichier avant Cargo ou utiliser rustc isolé si nécessaire. Le coordinateur appelle initialize_dpi avant la résolution portable ; local est identifié dans le titre transmis à run.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — tests état/géométrie et vrai HWND caché sous Windows.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — aucun avertissement.


---
title: 'US-116 — Résolution explicite du stockage portable'
type: 'feature'
created: '2026-10-11'
status: 'in-review'
baseline_commit: 'c091f6a52aa58af94966592ff356750471200d44'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="intent supplied by persistent user authorization">

## Intent

Résoudre le stockage avant de lancer la fenêtre ou WebView2. Le défaut est atelier-data à côté de l'exe ; un stockage portable inaccessible exige un choix utilisateur explicite et ne doit jamais créer automatiquement de cache dans LOCALAPPDATA.

## Boundaries & Constraints

Rust std seulement ; FFI des dialogues dans `data_win32.rs` avec justification de tous unsafe. Pas d'écriture au registre. API publique `data::resolve_data_paths(executable:&Path,local_appdata:Option<&Path>,choose:impl FnOnce(&DataError)->StartupChoice)->Result<StartupOutcome,DataError>`. Exposer `StartupChoice::{Local,Move,Quit}`, `StartupOutcome::{Ready(DataPaths),MoveRequired,Quit}`, `DataMode::{Portable,Local}`. `DataPaths` expose root/mode et accesseurs `preferences_file`, `window_state_file`, `recovery_dir`, `recent_file`, `crash_dir`, `webview_dir` tous sous une seule racine. Exposer `mode_label()` pour le titre utilisateur. Le coordinateur possède main/lib et relie le démarrage.

Résolution portable : chemin absolu de l'exe validé, racine parent/atelier-data, create_dir_all puis sonde unique create_new, écrire/sync/fermer/supprimer la sonde ; tester une écriture réelle, pas seulement un flag permissions. Ne pas effacer un fichier préexistant ni suivre un nom prévisible de sonde. La sonde et création de la seule racine ne sont pas des données utilisateur/cache ; aucun sous-dossier cache/récupération ne précède le choix. Échec : appeler choose une seule fois, transmettre raison exploitable ; Move et Quit terminent sans consulter/créer le chemin local. Local demande un LOCALAPPDATA absolu disponible, prépare LOCALAPPDATA/Atelier avec la même sonde ; échec local final bloque et ne boucle/replie jamais. Ne jamais créer de WebView avant Ready. Les accesseurs sont purs, le cache sera créé ultérieurement par US-111 à ce seul chemin.

Dialogues natifs français : expliquer stockage portable inaccessible et trois issues (déplacer l'exe vers un dossier inscriptible, choisir explicitement local, quitter). MessageBoxW Oui/Non/Annuler admis si le texte associe sans ambiguïté Oui=local, Non=déplacer et Annuler=quitter ; déplacer indique ensuite comment déplacer et quitte, sans déplacement automatique. `choose_data_mode(&DataError)->StartupChoice`, `show_startup_error(&DataError)` et `show_move_instructions()` disponibles sous Windows pour main. Aucun test ne déclenche de dialogue interactif : choix injecté et couverture pure de l'association des réponses Win32. Mode local visible dans le titre via mode_label. Toute erreur finale donne un message clair.

## I/O & Edge-Case Matrix

| Précondition / choix | Résultat | Effet local |
|---|---|---|
| Portable inscriptible | Ready Portable et chemins cohérents | aucun |
| Portable inaccessible / Quit | Quit | aucun |
| Portable inaccessible / Move | MoveRequired puis instructions et arrêt | aucun |
| Portable inaccessible / Local explicite | Ready Local sous LOCALAPPDATA/Atelier | préparation seulement après choix |
| Local absent/relatif/inaccessible | erreur finale claire | pas de repli |
| Collision sonde, échec I/O, chemin hostile | pas de fichier existant détruit, nettoyage et erreur | borné |

</frozen-after-approval>

## Code Map

- `SPEC.md` §7.2 et US-116 — une racine portable/local pour toutes les données gérées.
- `crates/desktop-win/src/main.rs` — intégration par coordinateur, initialize_dpi avant HWND/dialogues.
- `crates/desktop-win/src/lib.rs` — export data par coordinateur.
- `crates/desktop-win/src/window.rs` — agent US110, aucune dépendance requise : reçoit window_state_file et titre.

## Tasks & Acceptance

- [x] `crates/desktop-win/src/data.rs` — types/API, résolution/probe, chemins, messages et tests isolés sous target/temp.
- [x] `crates/desktop-win/src/data_win32.rs` — dialogues natifs sans autre écriture et correspondance des choix testable.
- [x] `docs/us-116-portable-data.md` — contrat de démarrage, choix, accesseurs, données gérées et tests.

Given un dossier portable inscriptible, when la résolution termine, then tous les chemins restent dans cette racine et aucun cache local n'apparaît. Given un échec portable, when Local n'est pas explicitement choisi, then aucune donnée locale n'est créée. Given Local inaccessible, when la résolution échoue, then le démarrage s'arrête avant tout cache et l'erreur est explicite. Given Windows, when la coque démarre en local, then le titre identifie ce mode.

## Implementation Notes

Propriété exclusive : tâches et notes append-only. Le coordinateur main/lib/CI/validation/Graphify/Git ; pas de sous-agent/skill/Graphify/commit/push. UTF-8/LF explicite. Tests en dossiers temporaires uniques, sans changer les ACL utilisateur ni manipuler vrais LOCALAPPDATA/exe du produit. Pour simuler portable inaccessible de manière reproductible, placer un fichier à la place du dossier atelier-data. Agent window peut être en cours : Cargo attendra les modules voisins.

## Spec Change Log

## Review Triage Log

## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — branches portable/local/quit/move, chemins et erreurs.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — zéro avertissement.
