# Epic 0.12 Context: Shell Windows

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Fournir le shell Windows natif qui héberge l’interface WebView2 et établit les bases de l’application portable : fenêtre Win32, intégration WebView2 et gestion cohérente du dossier de données. Le répertoire de planning demandé (`_bmad-output/planning-artifacts`) est absent ; ce contexte s’appuie donc sur l’epic source, `SPEC.md` et le crate existant.

## Stories

- US-110: Fenêtre Win32
- US-111: Liaisons WebView2
- US-112: Ressources embarquées et isolement réseau
- US-113: Pont natif ↔ JS
- US-114: Runtime WebView2 absent
- US-115: Instance unique et arguments
- US-116: Mode portable

## Requirements & Constraints

- Le shell est un exécutable Rust Windows x64 et utilise les API Win32/COM du système ; le produit reste sans dépendance tierce, conformément à la règle de dépendances du projet.
- La fenêtre doit gérer le DPI par écran et préserver sa géométrie sur plusieurs écrans.
- Toutes les données gérées par l’application doivent utiliser une racine de données résolue avant toute création de cache. En mode portable, cette racine est `atelier-data/` à côté de l’exécutable, sans écriture dans le registre.
- Si le stockage portable est inaccessible, l’utilisateur choisit explicitement entre déplacer l’exécutable, démarrer en mode local sous `%LOCALAPPDATA%\Atelier`, ou quitter. Aucun basculement automatique ; un emplacement inaccessible bloque le démarrage avec un message explicatif.
- Le dossier résolu regroupe préférences, récupération, documents récents, rapports de crash et cache WebView2. L’identification du mode local doit être visible. Les traces indépendantes de Windows sont hors de la garantie portable.
- Le runtime WebView2 Evergreen est attendu sur Windows 10/11 ; son absence doit être détectée avant la création de la WebView et expliquée clairement.

## Technical Decisions

- Shell maison Win32 + WebView2, sans Tauri. Les liaisons système sont écrites à la main en Rust (`extern "system"`) et les interfaces COM WebView2 déclarées avec leurs tables virtuelles.
- Le chargeur WebView2 est traité comme une API système Microsoft. La cible desktop est `x86_64-pc-windows-msvc`; le SDK Windows et l’éditeur de liens MSVC appartiennent à la chaîne de compilation.
- Les ressources front-end sont prévues pour être incorporées à l’exécutable et servies depuis l’origine virtuelle `https://app.atelier/`. La politique réseau interdit les requêtes sortantes.

## UX & Interaction Patterns

- Le shell suit le thème système dans sa barre de titre et restaure la taille, la position et l’état de la fenêtre.
- Un dialogue natif précède toute écriture si le dossier portable est inaccessible ; le mode local est indiqué dans l’interface. Les erreurs système doivent être présentées avec des messages traduisibles.

## Cross-Story Dependencies

- La fenêtre Win32 dépend de l’initialisation du workspace (US-001).
- Les liaisons WebView2 dépendent de la fenêtre et de la résolution du dossier de données (US-110, US-116) ; elles doivent résoudre ce dossier avant de créer le cache.
- Le service des ressources et le pont natif ↔ JS reposent sur les liaisons WebView2 (US-111). L’instance unique s’appuie sur le pont natif (US-113). La détection du runtime absent utilise la fenêtre native (US-110).
- Le mode portable dépend de la fenêtre native (US-110).
