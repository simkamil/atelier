# US-111/114 — WebView2 dans la fenêtre Win32

Le shell charge le contrôle WebView2 natif dans la fenêtre Win32 existante. La première livraison démarrait sur `about:blank`. Le lot [US-112](us-112-embedded-resources.md) charge maintenant la page embarquée `https://app.atelier/`; l’interface d’édition et le pont fichiers JavaScript restent à développer.

## ABI et liaison

`crates/desktop-win/src/webview.rs` déclare les seules méthodes COM nécessaires avec des pointeurs de fonctions `extern "system"` et les structures `repr(C)`. Les IID, types et positions de vtable ont été vérifiés dans le header officiel conservé dans `data/webview2/include/WebView2.h` pour Microsoft.Web.WebView2 1.0.4258.31. Les méthodes utilisées sont `ICoreWebView2Environment::CreateCoreWebView2Controller` (slot 3), `ICoreWebView2Controller::get_IsVisible` (3), `put_IsVisible` (4), `get_Bounds` (5), `put_Bounds` (6), `MoveFocus` (12), `Close` (24), `get_CoreWebView2` (25), `ICoreWebView2::get_Settings` (3) et les accesseurs DevTools de `ICoreWebView2Settings` (11–12).

Le chargeur officiel x64 MSVC est lié statiquement par `crates/desktop-win/build.rs`. Aucun chargement depuis le répertoire courant ou `PATH` et aucun export interne du runtime ne sont utilisés. `CoTaskMemFree` libère la version du runtime retournée par la sonde.

## Démarrage et runtime absent

`window::run(state_file, title, webview_data)` sonde `GetAvailableCoreWebView2BrowserVersionString` avant de créer le cache. En l’absence de runtime, une boîte de dialogue native en français affiche l’adresse officielle d’installation et indique que Ctrl+C copie le message et l’adresse; aucun navigateur ni installateur ne démarre automatiquement. La création d’environnement utilise ensuite exclusivement le dossier `webview_data` résolu par le démarrage Ready. Si le runtime disparaît entre la sonde et la création asynchrone, le HRESULT de rappel est propagé et la même information est présentée.

COM est initialisé en STA sur le thread de la fenêtre. Les rappels d’environnement et de contrôleur sont asynchrones. Chaque objet callback possède une référence `Arc` à son état partagé et ne conserve aucun pointeur vers `Context` ou vers le HWND dans une fermeture. Le résultat d’un contrôleur tardif après fermeture est fermé puis relâché. Les chemins de rappel convertissent les erreurs HRESULT et les pointeurs nuls en erreurs Rust; `catch_unwind` empêche une panique de traverser la frontière ABI. QueryInterface accepte `IUnknown` et l’IID exact du callback; AddRef et Release gèrent les références.

La fermeture normale masque la fenêtre, ferme le contrôleur et garde la pompe STA active jusqu’au drainage des rappels, avec un délai de 15 secondes. À l’expiration, l’état partagé détache le HWND avant la destruction. Un rappel tardif ne peut alors contacter aucune fenêtre ou donnée de contexte; son allocation de callback reste sous la référence détenue par WebView2 jusqu’à son Release final. Cette stratégie borne la fermeture et évite le use-after-free. Un runtime qui ne termine jamais un callback peut garder l’objet COM alloué jusqu’à la fin du processus; le self-check le détecte par le compteur de callbacks et échoue.

## Fenêtre, DPI et DevTools

Les dimensions WebView sont mises à jour depuis le rectangle client physique après `WM_SIZE` et `WM_DPICHANGED`. La visibilité est retirée pendant la minimisation et restaurée au retour; `WM_SETFOCUS` appelle `MoveFocus(PROGRAMMATIC)`. L’appel existant `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` active la conscience DPI avant la création du HWND; Windows transmet à WebView2 le DPI du parent. Le contrôle d’US-110 synthétise un `WM_DPICHANGED` à 144 DPI pour tester la réponse de la fenêtre; ce test ne simule pas le déplacement entre deux écrans. Le self-check WebView crée son parent sur l’écran courant et mesure le DPI réel du HWND.

Le réglage `AreDevToolsEnabled` est écrit avec `cfg!(debug_assertions)` puis relu et comparé. Il est donc activé en debug et désactivé en release. Le self-check relit également la valeur sur chaque contrôleur créé.

## Self-check et limites de preuve

`window::webview_self_check(state_file, webview_data, cycles)` n’accepte que 1 à 100 cycles. Les deux chemins sont validés sous `target/` avant la création de leurs répertoires. Un cycle d’échauffement précède les mesures. Chaque cycle réel crée la WebView dans un HWND masqué, attend la fin asynchrone, vérifie DevTools, relit `get_Bounds` et `get_IsVisible`, redimensionne réellement le HWND, synthétise `WM_DPICHANGED` à 144 DPI et compare les bounds du contrôleur au rectangle client après chaque changement. Il vérifie les transitions de visibilité, ferme le contrôleur, pompe les messages STA jusqu’à 100 ms de quiescence (limite 5 s), puis vérifie le retour du nombre d’objets callback au niveau de départ. `GetProcessHandleCount` relève le compte réel après échauffement, aux cycles 1/10/100, le pic et la fin; une hausse du compte final échoue le contrôle sans seuil de tolérance.

Les tests du crate vérifient la barrière runtime absent/Ready et le refus de création avant Ready, les erreurs injectées, les IID/refcounts des callbacks, un résultat controller arrivé après fermeture, le texte de diagnostic et la valeur DevTools attendue pour le profil. `cargo test -p atelier-desktop-win --locked --offline` passe sur l’hôte Windows. Le self-check de 100 cycles requiert un runtime Evergreen installé et doit être exécuté sur une machine Windows x64 MSVC; il mesure le processus applicatif et ne prétend pas mesurer les handles des processus navigateur séparés.

## État de livraison — 2026-10-11

US-114 est implémentée et vérifiée par injection de sonde absente/présente, absence de création avant Ready, et diagnostic natif copiable. Le point d’entrée présente une seule boîte de dialogue ; le shell retourne le texte et le HRESULT au point d’entrée.

US-111 reste **en cours** : le contrôle strict de 100 cycles observe une hausse des handles du processus hôte. Le [journal exploratoire](../_bmad-output/implementation-artifacts/frontend-host-exploration.json) conserve les échecs, la sonde stable, les essais de drainage et de réutilisation d’environnement, ainsi que l’inspection PSS de handles visant des processus enfants terminés. L’essai de réutilisation a été retiré car il ne corrigeait pas le symptôme. Aucune fermeture forcée de handles de propriétaire inconnu ni tolérance numérique n’a été ajoutée.

Le [signalement Microsoft #5704](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5704), encore ouvert le 2026-10-11, décrit un symptôme similaire sur Windows 11 avec MSVC x64. Cette concordance suggère un problème du runtime ; elle n’établit pas la cause interne de notre mesure.

`tests/check-webview-host.ps1` lance le contrôle strict. Sans option, il conserve son échec. Avec `-AllowKnownRuntimeHandleGrowth`, utilisé pour la validation fonctionnelle CI, il accepte exclusivement le diagnostic final indiquant 100 cycles réussis et zéro callback restant, et publie explicitement `handle_acceptance_passed: false`. Toute autre erreur échoue la CI. Un job vert ne clôt donc pas US-111. Le binaire strict reste reproductible :

```powershell
.\target\debug\atelier.exe --webview-self-check target/webview-strict-check
```

L’environnement COM est retenu jusqu’à la fermeture du contrôleur et relâché sur le même STA. Les erreurs de `Close` sont propagées au diagnostic ; les tests des compteurs globaux de callbacks sont sérialisés pour éviter une course entre tests.

Validation finale du lot : [34 contrôles fonctionnels et qualité](../_bmad-output/implementation-artifacts/frontend-host-validation.json),
**318 tests workspace et 27 tests shell release**, sur sources et lock inchangés.
Les tests Chromium couvrent Canvas, saisie et leur composition intégrée.
Les deux profils WebView2 accomplissent 100 cycles fonctionnels avec callbacks équilibrés ;
le rapport conserve séparément l'échec strict des handles et US-111 reste ouverte.
La [revue indépendante](../_bmad-output/implementation-artifacts/frontend-host-review.md)
conserve un point IME natif de gravité moyenne non vérifiée à trancher sur matériel.
Les tests musl sont compilés localement ; leur exécution appartient à la CI Linux.

## Audit des références et caractérisation de la croissance — 2026-10-17

US-111 reste **en cours** ; le critère n'est pas coché.

**Nos références COM sont équilibrées.** Des compteurs compilés uniquement en test (`ledger` dans `webview.rs`, par thread STA) comptent chaque référence d'interface acquise (AddRef explicite ou pointeur sortant détenu : environnement, contrôleur, `get_CoreWebView2`, `get_Settings`, `ICoreWebView2_22`, requête, flux et réponse de ressource) et chaque `release_interface`, ainsi que AddRef/Release de nos objets callback (rappels d'initialisation et gestionnaires d'événements). Le test `com_reference_ledger_balances_over_100_cycles` exécute 100 cycles réels : 715 références acquises et 715 relâchées, 1310 AddRef et 1310 Release sur nos objets, objets callback vivants 0 puis 0. Aucune fuite de références de notre côté.

**La croissance concerne uniquement des handles `Process`.** La sonde ignorée `handle_growth_probe` (`webview/leak_probe.rs`, fenêtre parente masquée) classe les handles par type via `NtQuerySystemInformation`/`NtQueryObject`, relève GDI/USER (`GetGuiResources`) et décrit les handles Process :

| variante (100 cycles) | départ | fin | par cycle | type qui croît |
|---|---|---|---|---|
| `envonly` : environnement créé puis relâché, aucun contrôleur | 182 | 182 | 0 | aucun |
| `full` : notre cycle complet (réglages, événements, ressources, navigation, Close) | 227 | 330 | ~1 | Process 3 vers 103 |
| `noconfig` : contrôleur nu puis Close, sans aucun code de configuration de notre part | 228 | 528 | ~3 | Process 4 vers 301 |
| `--webview-self-check` (page embarquée chargée, 1/10/100) | 237 | 835 | ~6 | non typé |

La croissance est linéaire (par exemple `full` : +7 au cycle 10, +50 au cycle 50, +103 au cycle 100) ; aucun plafond jusqu'à 100 cycles. GDI reste à 0 et USER constant. Sur 30 cycles, tous les handles ajoutés ont le masque d'accès `0x1fffff` (PROCESS_ALL_ACCESS, celui du `hProcess` renvoyé par `CreateProcess`) et visent des processus déjà terminés ; notre processus n'appelle jamais `CreateProcess`.

**Lecture.** La croissance apparaît à la création du contrôleur, existe sans notre code de configuration, dépend de l'activité du runtime (plus de processus enfants avec la page embarquée) et consiste en handles de processus enfants terminés détenus dans notre processus par le code client du runtime. C'est cohérent avec le signalement #5704, sans en établir la cause interne. Aucun handle de propriétaire inconnu n'est fermé de force.

**Reproduction.**

```powershell
cargo test -p atelier-desktop-win --lib --locked --offline com_reference_ledger -- --nocapture
$env:ATELIER_PROBE_VARIANT='envonly'   # full | noconfig | envonly
$env:ATELIER_PROBE_CYCLES='100'
cargo test -p atelier-desktop-win --lib --locked --offline handle_growth_probe -- --ignored --nocapture
```

**Proposition de reformulation (décision du propriétaire du produit).** Remplacer le critère par : « Tables virtuelles des interfaces nécessaires déclarées en Rust ; comptage de références correct : après 100 ouvertures/fermetures, zéro objet callback vivant et chaque référence COM acquise relâchée (test de registre) ; la croissance du nombre de handles Process attribuée au runtime WebView2 est mesurée, documentée et suivie en amont (WebView2Feedback #5704). » Le contrôle strict des handles du processus hôte resterait un indicateur publié, non bloquant, tant que le runtime n'est pas corrigé.
