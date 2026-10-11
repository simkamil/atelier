# Chargeur système WebView2

Les fichiers de ce dossier proviennent du package officiel Microsoft.Web.WebView2
**1.0.4258.31**, publié par Microsoft sur NuGet. Le [manifest](manifest.json)
conserve URL, taille et SHA-256 du package et des fichiers extraits.
Le [texte de licence Microsoft](LICENSE.txt) doit accompagner la distribution.

`x64/WebView2LoaderStatic.lib` est lié dans l'exécutable Windows x64 MSVC :
aucune DLL de chargeur n'est recherchée dans le répertoire courant ou le PATH.
Le runtime Evergreen installé sur la machine reste nécessaire.
Microsoft documente cette [distribution en exécutable unique](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/static).
Cette API système est explicitement autorisée par SPEC §6.8 / D11.

`include/WebView2.h` est l'oracle ABI officiel conservé sans modification pour
relire les GUID, les signatures et l'ordre des vtables COM manuelles ; il
n'est pas compilé et n'ajoute aucun générateur ou crate au build.

`tests/check-webview-assets.ps1` vérifie les trois fichiers avant compilation
dans la CI. Le build reste entièrement hors ligne après le clone du dépôt.
