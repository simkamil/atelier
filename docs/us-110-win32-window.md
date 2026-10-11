<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-110 — Fenêtre Win32 persistante et DPI

`atelier-desktop-win::window` expose `initialize_dpi()`, `run(state_file, title, webview_data)`, `self_check(state_file)` (Windows) et `show_error(message)` (Windows). Le coordinateur appelle l’initialisation PMv2 avant la création du moindre HWND et transmet les chemins d’état et de cache WebView2 déjà résolus par `data`. L’hébergement actuel et ses limites sont décrits dans [US-111/114](us-111-webview2.md).

Le fichier d’état est un texte UTF-8 versionné, borné à 128 octets :

```text
ATELIER-WINDOW
1
left
top
right
bottom
dpi
maximized (0 ou 1)
```

Les coordonnées du rectangle sont des coordonnées écran physiques; les valeurs négatives sont valides. Le DPI mémorisé permet de préserver une taille logique lors d’un déplacement vers un écran de densité différente. Une absence, une version inconnue, une entrée mal formée, une taille excessive ou des dimensions invalides donnent lieu à une géométrie par défaut. Les tailles par défaut et minimales sont exprimées à 96 DPI puis mises à l’échelle selon le DPI effectif, toujours limitées à la zone de travail disponible.

La sauvegarde écrit et synchronise un fichier temporaire créé dans le même dossier, puis le publie par `MoveFileExW` avec remplacement et écriture durable. Une erreur de publication est remontée et le fichier précédent reste intact. Le rectangle normal de `WINDOWPLACEMENT` est converti entre coordonnées workspace et écran; un état maximisé persiste le rectangle normal et le drapeau maximisé. Les événements DPI appliquent le rectangle recommandé, et les événements de thème relisent la préférence Windows sans écriture du registre. Le DPI initial est lu sur le HWND créé avec `GetDpiForWindow`, puis `WM_DPICHANGED` fournit le DPI de chaque déplacement. L’attribut de thème DWM courant est essayé en premier, avec l’identifiant historique pour Windows 10.

Références Microsoft : [SetProcessDpiAwarenessContext](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext), [WINDOWPLACEMENT et coordonnées workspace](https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-windowplacement), [GetDpiForWindow](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getdpiforwindow), [WM_DPICHANGED](https://learn.microsoft.com/windows/win32/hidpi/wm-dpichanged), [DWMWA_USE_IMMERSIVE_DARK_MODE](https://learn.microsoft.com/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute).

`self_check` crée une fenêtre cachée, valide la surface cliente, envoie un événement DPI contrôlé, teste la persistance et détruit la fenêtre. Son fichier doit être placé sous `target/`. Les tests de géométrie utilisent des écrans synthétiques : ils couvrent les règles de sélection et de bornage, sans remplacer une validation sur une machine multi-écran réelle.

Validation prévue sous Windows : `cargo test -p atelier-desktop-win --locked --offline` et `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings`.

Validation du lot : [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json), 312 tests workspace et 21 tests shell release sur sources inchangées ; [revue indépendante](../_bmad-output/implementation-artifacts/worker-shell-review.md). Les tests musl sont compilés localement et exécutés en CI Linux.
