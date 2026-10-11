# Stockage portable et racine des données

Le shell résout sa racine de données avant de créer une fenêtre ou un environnement WebView2. Toutes les données gérées par Atelier utilisent cette racine : préférences, état de la fenêtre, récupération, documents récents, rapports de crash et cache WebView2. Les accesseurs de `DataPaths` construisent des chemins sans créer les fichiers ou répertoires correspondants.

## Résolution au démarrage

`data::resolve_data_paths(executable, local_appdata, choose)` vérifie d’abord que le chemin de l’exécutable est absolu, puis prépare `atelier-data/` dans son dossier. La préparation crée seulement la racine, vérifie qu’elle est bien un dossier et réalise une sonde d’écriture exclusive : création avec `create_new`, écriture, synchronisation, fermeture et suppression. Les noms de sonde sont uniques et les collisions sont bornées ; un fichier déjà présent n’est jamais ouvert ou supprimé.

Si la résolution portable échoue, `choose` est appelé une seule fois avec une erreur exploitable. `Local` valide et prépare ensuite `%LOCALAPPDATA%\Atelier`. `Move` renvoie `MoveRequired`, et `Quit` renvoie `Quit` ; ces deux choix n’examinent ni ne créent le chemin local. Un échec du chemin local est définitif et ne redemande pas de choix. Aucun sous-dossier de cache ou de récupération n’est créé par la résolution.

Sur Windows, `choose_data_mode` affiche un dialogue français : Oui choisit explicitement le mode local, Non demande de déplacer manuellement l’exécutable puis renvoie `MoveRequired`, et Annuler quitte. `show_startup_error` présente une erreur finale. `show_move_instructions` explique l’action manuelle ; le shell ne déplace pas l’exécutable.

## API et chemins

`DataPaths` expose `root`, `mode` et les accesseurs `preferences_file`, `window_state_file`, `recovery_dir`, `recent_file`, `crash_dir` et `webview_dir`. Tous sont sous la même racine. `mode_label()` renvoie `Portable` ou `Mode local` pour que le shell identifie le mode local dans son titre.

Le coordinateur de démarrage fournit le chemin réel de l’exécutable et la valeur de `LOCALAPPDATA`, appelle le dialogue de choix après échec portable, puis ne poursuit que sur `StartupOutcome::Ready`. Le cache WebView2 peut être créé ultérieurement uniquement sous `webview_dir()` après cette réussite. Atelier n’écrit pas dans le registre ; les traces produites indépendamment par Windows ne font pas partie de la garantie portable.

## Couverture

Les tests utilisent des dossiers temporaires uniques. Ils couvrent les chemins portables et locaux, l’absence de création anticipée des sous-dossiers, les choix Move/Quit sans accès local, l’échec local final, les chemins relatifs et la préservation d’un fichier en collision. Le mapping des réponses Win32 vers les choix est testé sans afficher de dialogue.

Validation du lot : [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json), 312 tests workspace et 21 tests shell release sur sources inchangées ; [revue indépendante](../_bmad-output/implementation-artifacts/worker-shell-review.md). Les tests musl sont compilés localement et exécutés en CI Linux.
