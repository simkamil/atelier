# US-113 — Pont natif étendu : SharedBuffer, dialogues, tickets, polices, URL

Cette page complète le [pont de fichiers texte](us-113-file-bridge.md) et le
[courtier d’accès aux fichiers](us-113-file-access.md). Le transport reste le
protocole `us120-file|1|ID|opération|charge` (source exacte `https://app.atelier/`,
identifiants décimaux croissants, file native de quatre opérations, validation et
copie seulement dans le callback WebView2, tout le reste sur la STA de la fenêtre).

## Principe de sécurité

Le JS ne fournit jamais de chemin. Les dialogues natifs (`IFileOpenDialog`,
`IFileSaveDialog`) renvoient un chemin au code de confiance, qui demande un ticket au
courtier et ne transmet au JS que `r<n>` (lecture) ou `w<n>` (écriture), le format
reconnu et le **nom de fichier** (jamais le dossier). Un ticket de lecture sert une
fois ; un ticket d’écriture sert à un fichier, publié atomiquement au dernier
morceau. Un numéro de ticket forgé, déjà utilisé ou jamais remis par le pont est
refusé avant d’atteindre le courtier (`files.error.unknown_ticket`). Au plus 8 tickets
de lecture et 4 d’écriture sont détenus en même temps.

## Opérations

| Opération (`ID\|op\|charge`) | Charge | Réponse `ok` |
|---|---|---|
| `pickOpen` | Base64 de la spécification (ci-dessous) | `r<n>;<format>;<octets>;<nom en Base64>` ou `cancelled` |
| `pickSave` | spécification + `name:<nom suggéré>` | `w<n>;<format>;<nom en Base64>` ou `cancelled` |
| `readTicket` | `r<n>` | `b64;<Base64>` (≤ 64 Kio) ou `sb;<octets>` par SharedBuffer |
| `writeTicket` | `w<n>;<seq>;<0\|1>;<Base64 ≤ 128 Kio>` | vide ; le dernier morceau (`1`) publie le fichier |
| `releaseTicket` | `r<n>` ou `w<n>` | vide |
| `listFonts` | vide | `b64;<TSV>` ou `sb;<octets>` : `id⇥famille⇥style⇥face⇥octets` par ligne |
| `readFont` | `f<génération>-<index>` | comme `readTicket` |
| `openUrl` | Base64 de l’URL | vide |
| `launch` (US-115) | vide | inchangé en ligne ; `<type>;<ignorés>;@sb` par SharedBuffer |

Spécification d’un dialogue (texte UTF-8, une ligne par élément) : le titre, puis
1 à 8 lignes `filter:<format>=<libellé>` et, à l’enregistrement, `name:<nom>`. Les
formats connus du natif sont `txt`, `docx`, `docm`, `dotx`, `odt`, `doc` (une
extension chacun) ; **le moteur décide du sous-ensemble qu’il sait lire ou écrire**
et le JS ne transmet que celui-là. Les libellés viennent du catalogue i18n du JS.
Titre ≤ 120 caractères, libellé ≤ 80, nom sans séparateur ni caractère réservé,
aucun caractère de contrôle ; tout écart est refusé par `files.error.invalid_request`.
Le filtre sélectionné à l’enregistrement est celui de l’extension du nom suggéré ;
une extension hors des filtres demandés est refusée (`files.error.unsupported_format`).

Une requête dont l’argument est invalide consomme son identifiant et reçoit une erreur
traduisible (file de refus bornée) au lieu d’un silence.

Les API JS correspondantes sont dans `web/native-host.mjs` : `pickOpen`, `pickSave`,
`readTicket`, `writeTicket`, `releaseTicket`, `listFonts`, `readFont`, `openUrl`,
`stats`. L’interface « Enregistrer / Enregistrer sous » reste à faire (US-322) ;
seul le pont et ses tests existent ici.

## SharedBuffer

Liaisons COM manuelles dans `crates/desktop-win/src/webview/shared_buffer.rs` :
`ICoreWebView2Environment12::CreateSharedBuffer` (emplacement 24 de la table
virtuelle), `ICoreWebView2SharedBuffer` (`Buffer` 4, `Close` 7) et
`ICoreWebView2_17::PostSharedBufferToScript` (emplacement 116). Les emplacements et
les IID ont été relevés dans les liaisons générées du SDK
(`webview2-com-sys` 0.39.1, lu comme référence seulement ; aucune dépendance n’est
ajoutée). Les deux interfaces sont obtenues par `QueryInterface` à chaque envoi ;
`E_NOINTERFACE` signale un runtime plus ancien.

- Le natif copie les octets dans un tampon neuf, le poste **en lecture seule**
  (`COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY`) avec `additionalData` =
  `{"v":1,"reply":"us120-file|1|<id>|ok|sb;<octets>"}`, appelle `Close` puis relâche
  ses références. Le tampon et sa réponse arrivent ensemble : aucune corrélation entre
  deux canaux n’est nécessaire.
- Le JS écoute `chrome.webview` `sharedbufferreceived`, vérifie version, identifiant,
  opération attendue et taille annoncée, puis expose `bytes` (vue sans copie) et
  `release()` qui appelle `chrome.webview.releaseBuffer`. `{ copy: true }` copie et
  libère aussitôt (utilisé pour `launch`). Un tampon inattendu, tardif ou invalide est
  libéré immédiatement ; `close()` libère ceux qui restent.
- Repli explicite : si le runtime ne fournit pas les interfaces (ou si l’envoi échoue),
  le natif envoie à la place `fb;<Base64>` (jusqu’à 8 Mio, `result.shared === false`,
  compté dans `stats().fallbackReceived` et dans `SHARED_BUFFER_UNAVAILABLE` côté natif),
  sinon `files.error.shared_buffer_unavailable` ou `files.error.shared_buffer_failed`.
  Le diagnostic `--bridge-self-check` échoue clairement si le repli est utilisé.
- Les références COM (environnement 12, tampon, cœur, `ICoreWebView2_17`) passent par
  `note_acquired`/`release_interface` : le test `com_reference_ledger_balances_over_100_cycles`
  poste un tampon à chaque cycle et exige `acquired == released`.

## Mesure : fichier de 100 Mio

`target/debug/atelier.exe --bridge-self-check target/res-us113-bridge` (ou
`target/release/…`) charge la vraie page dans une WebView2 masquée et, avec
l’API publique du pont seulement, lit 7 fois un fichier de 100 Mio (104 857 600
octets) par tickets remis par le natif (comme le ferait un dialogue). Chaque essai
mesure dans le JS `performance.now()` de `readTicket` à la disponibilité de la vue sur
le tampon, et vérifie huit octets d’échantillon. Mesure sur cette machine (Windows 11,
WebView2 runtime 155.0.4283.62) :

| Build | Passes de 7 essais | Médiane des passes | Min / max d’un essai |
|---|---|---|---|
| release | 5 | **70,4 ms** (66,6 à 72,2) | 56,3 / 76,0 ms |
| debug | 8 | **70,3 ms** (66,6 à 74,2) | 54,9 / 77,3 ms |

La cible « < 300 ms » est atteinte avec une marge de plus de quatre fois. Le test
d’intégration `crates/desktop-win/tests/us113_bridge.rs` la fait respecter
(médiane < 300 ms, tampons reçus = libérés, aucune fuite d’objet de callback). Le coût
est dominé par deux copies de 100 Mio (lecture du fichier, puis tampon partagé) ; une
lecture directe dans le tampon n’a pas été nécessaire.

Le même diagnostic vérifie sans dialogue interactif : lecture en ligne, écriture
atomique par morceaux (300 000 octets vérifiés sur disque), liste de 568 polices et
lecture d’une police par SharedBuffer, refus d’un ticket consommé et d’un
identifiant de police inconnu, et **ouvre réellement les dialogues communs d’ouverture
et d’enregistrement** avec les six filtres de format avant de les fermer par `WM_CLOSE`
(minuterie réservée au diagnostic) : les appels `SetFileTypes`, `SetOptions`,
`SetTitle`, `SetDefaultExtension`, `SetFileName` réussissent et `Show` renvoie
l’annulation. Une fenêtre de dialogue apparaît donc brièvement pendant ce diagnostic.

## Polices système

`crates/desktop-win/src/fonts.rs` lit `%WINDIR%\Fonts` puis
`%LOCALAPPDATA%\Microsoft\Windows\Fonts` (`.ttf`, `.otf`, `.ttc`, `.otc`, sans suivre
les liens), extrait famille et style de la table `name` (identifiants 16/17 puis 1/2,
Windows en-US d’abord ; en-tête, répertoire de tables et table bornés à 256 Kio ;
toutes les faces d’une collection) sans GDI ni DirectWrite. Les identifiants
`f<génération>-<index>` n’ont de sens que pour le dernier balayage. `readFont` refuse
un fichier modifié depuis le balayage (`fonts.error.changed`) ou de plus de 64 Mio
(`fonts.error.too_large`) et renvoie le fichier entier (une collection reste une
collection ; le JS choisit la face avec `face`). Les balayages suivants ne relisent que
les fichiers dont la taille ou la date a changé.

## Ouverture d’URL

`crates/desktop-win/src/url_open.rs` n’accepte que `http:` et `https:` (`mailto:`
est volontairement refusé : il lance un client de messagerie et ouvre la porte aux
paramètres d’URL). L’URL est bornée à 2048 octets, en ASCII visible déjà encodé, sans
guillemet, chevron, antislash, accent circonflexe, apostrophe inverse, accolade ou
barre, avec un hôte non vide sans identifiants (`@`). Le JS applique la même règle avant
l’envoi (aucun aller-retour pour un refus). `ShellExecuteW("open")` s’exécute sur la
STA ; son code de retour est traduit en `url.error.no_handler`,
`url.error.access_denied` ou `url.error.launch_failed`.

## Erreurs

Chaque erreur (HRESULT, `GetLastError`, code `ShellExecuteW`, erreur du courtier)
est transformée en clé stable `files.error.*`, `fonts.error.*` ou `url.error.*`
définie dans le bloc « US-113 » de `web/i18n/fr-FR.txt` ; les valeurs numériques
(HRESULT) restent dans `FileError` côté natif et ne sont jamais envoyées au script.
Deux garde-fous : le test Rust `every_native_error_key_exists_in_the_french_catalogue`
parcourt les sources natives, et `BRIDGE_ERROR_KEYS` dans `web/native-host.mjs` rend les
clés visibles de `node tests/check-i18n.mjs --strict`. L’affichage de ces messages
dans l’interface relève des écrans qui appelleront le pont (US-322).

## Tests

- Rust : `cargo test -p atelier-desktop-win --locked --offline` (analyse des
  opérations, spécifications de dialogue, filtres par format, tickets contre le courtier
  réel avec un hôte de dialogue factice, écriture atomique par morceaux, polices
  synthétiques et réelles, URL, clés du catalogue, `com_reference_ledger`, tests
  US-115) et le test d’intégration `us113_bridge`.
- JS : `node tests/native-bridge-protocol.mjs` (WebView factice : spécifications,
  tickets, tampons reçus/copiés/tardifs/invalides, libération, écriture par morceaux,
  polices, URL, `launch`, catalogue) ; `node tests/editor-protocol.mjs` et
  `node tests/rich-preview-protocol.mjs` pour la compatibilité.
- Navigateur : `node tests/run-editor-browser.mjs` (inchangé, passe).

## Reste à vérifier à la main

- Choisir réellement un fichier dans le dialogue d’ouverture et un nom dans le
  dialogue d’enregistrement avec les filtres du moteur, vérifier le filtre
  présélectionné, l’extension ajoutée et l’invite de remplacement (les dialogues
  s’ouvrent et s’annulent automatiquement, le choix d’un chemin est simulé).
- Ouvrir une URL https dans le navigateur par défaut réel (`ShellExecuteW` n’est
  jamais appelé par les tests) et un poste sans navigateur par défaut.
- Un runtime WebView2 sans `ICoreWebView2Environment12`/`ICoreWebView2_17` (repli
  Base64 vérifié par les tests de protocole, pas contre un vrai runtime ancien).
- L’US reste à relier à l’interface (US-322) : aucune commande de l’éditeur n’appelle
  encore `pickOpen`/`pickSave`.
