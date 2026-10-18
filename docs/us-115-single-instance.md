# US-115 — Instance unique et arguments

## Instance unique

`main.rs` résout d'abord la racine de données (US-116), puis `instance_win32::acquire(racine, chemins)` :

1. `CreateMutexW` sur `Local\Atelier.SingleInstance.<clé>` ; `GetLastError() == ERROR_ALREADY_EXISTS` signale une instance existante (aucune course entre ouverture et création).
2. La clé est un FNV-1a 64 bits de la racine de données (insensible à la casse, `\?\` ignoré) : une instance par utilisateur (espace `Local\` de la session), par mode (portable ou local) et par copie portable. Deux copies portables dans deux dossiers coexistent.
3. La fenêtre principale utilise la classe `AtelierWindow.<clé>` ; la seconde instance la retrouve avec `FindWindowW`.
4. Envoi par `SendMessageTimeoutW(WM_COPYDATA, SMTO_BLOCK | SMTO_ABORTIFHUNG, 3 s)`. La mémoire de l'émetteur n'est valide que pendant un envoi synchrone ; `PostMessageW` ne convient pas. `AllowSetForegroundWindow(pid cible)` est appelé côté émetteur. Le récepteur répond 1 (accepté) ou 0 (refusé) ; un refus n'est pas réessayé.
5. Repli : tant que la fenêtre n'existe pas (première instance en cours de démarrage) ou ne répond pas, 40 tentatives espacées de 125 ms (5 s). Ensuite la seconde instance affiche une erreur et se termine. Elle n'ouvre jamais une seconde fenêtre sur le même dossier de données (préférences, cache WebView2 partagés).

## Protocole `WM_COPYDATA` (`instance.rs`, pur et testé)

- `dwData = 0x41544C31` (« ATL1 »), `cbData` ≤ 16 × 32 768 × 2 octets.
- Charge : chemins absolus en UTF-16 little-endian, chacun terminé par NUL ; au plus 16 chemins de 32 767 unités. Charge vide valide : simple mise au premier plan.
- Rejet sans panique : protocole inconnu, taille impaire ou excessive, NUL final absent, chemin vide, trop long, non absolu (`X:\`, `\serveur\partage`, `\?\`), UTF-16 invalide, trop de chemins.
- Provenance : `wParam` porte le PID de l'émetteur ; le récepteur n'accepte que si ce processus exécute le même fichier exécutable (`QueryFullProcessImageNameW`). UIPI bloque déjà les émetteurs de moindre intégrité.
- Les arguments relatifs sont rendus absolus par rapport au dossier courant de l'émetteur ; les options `--…` sont ignorées.

## Côté fenêtre et front

- `WM_COPYDATA` valide : les chemins vont dans `FileRuntime.launch` (borné à 16), puis `PostMessageW(WM_ATELIER_LAUNCH)` (le travail n'est pas fait dans le message envoyé) : restauration si réduite, `SetForegroundWindow`, notification `us115-launch|1` au front.
- Le front réclame les documents par l'opération `launch` du pont de fichiers (`us120-file|1|<id>|launch|`). Aucun chemin ne vient de JavaScript : le natif lit le fichier (`.txt` ≤ 131 075 octets, `.docx` ≤ 8 Mio) via le courtier. Réponse `ok|<txt|docx>;<ignorés>;<base64>`, `ok|` si rien n'attend, `error|files.error.*` sinon (les fichiers suivants restent en attente).
- Au démarrage, `app.mjs` réclame une fois les chemins de la ligne de commande (front prêt). Une notification ultérieure déclenche une nouvelle réclamation. `.txt` : `editor.openDocument(source)` (confirmation si le document est modifié) ; `.docx` : aperçu existant.
- Aucun onglet de documents n'existe : le premier fichier s'ouvre, les suivants sont comptés et signalés (`status-launch-skipped`, pluriel i18n). Type inconnu : `files.error.unsupported_type`.

## Diagnostic et tests

- `atelier.exe --instance-probe-primary <racine> <sortie>` : prend le mutex, crée une fenêtre cachée de la classe de l'application, écrit les chemins reçus, se termine (20 s de garde). `--instance-probe-secondary <racine> [chemins…]` se comporte comme une seconde instance et affiche `forwarded`, `became-primary` ou `unreachable`.
- `crates/desktop-win/tests/us115_single_instance.rs` lance deux vrais processus sur des dossiers isolés sous `target/tmp`.
- Tests unitaires : `instance.rs` (encodage, décodage hostile, `process_copydata`, clé), `file-bridge.rs` (opération `launch`), `tests/editor-protocol.mjs` (pont JS), `tests/editor-browser.mjs` (application complète).
- Vérifié à la main (exe debug copié dans un dossier isolé, WebView2 réel, lu via le protocole de débogage) : `atelier.exe "a.txt" b.txt` ouvre `a.txt` et signale 1 fichier ignoré ; une seconde exécution avec `c.txt` se termine en 38 ms et `c.txt` s'ouvre dans la première fenêtre.

## Limites

- La mise au premier plan (`ShowWindow`/`SetForegroundWindow` après `AllowSetForegroundWindow`) n'est pas vérifiée automatiquement : Windows peut se limiter à faire clignoter la barre des tâches selon le contexte.
- Le sélecteur de mode (US-116) précède le mutex : deux lancements simultanés avec dossier non inscriptible affichent deux dialogues.
- Fichiers `.txt`/`.docx` uniquement ; pas d'onglets (voir US-307 et suivantes).
