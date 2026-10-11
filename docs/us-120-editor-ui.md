# US-120 — Interface éditeur texte brut Canvas

La page embarquée fournit un éditeur de texte paginé. Le moteur WASM reste propriétaire du texte, des offsets UTF-8, des sélections, de l'historique, du rendu ATDL et de l'état modifié. Le navigateur possède le Canvas, le viewport, le focus et les événements d'entrée. Le host natif possède les dialogues et l'accès aux fichiers.

## Exercice local

Après compilation du WASM optimisé et préparation de l'hôte WebView, lancer les contrôles ciblés :

```powershell
node tests/editor-protocol.mjs
node tests/run-editor-browser.mjs
```

Le premier contrôle couvre les codecs ATED/ATES/ATEX, les rejets de tailles et de formats invalides, les octets UTF-8/BOM, la corrélation des réponses et la file host bornée. Le second démarre un navigateur Edge/Chrome headless, charge l'artefact WASM réel avec un Worker et un Canvas, puis injecte un host de fichiers contrôlé. Il exerce la saisie rapide, les sélections, undo/redo, suppression, annulation et erreur d'ouverture, préservation BOM/CRLF, pagination et défilement du caret, composition IME simulée, sauvegarde, rejet d'une confirmation de révision obsolète et hit-test Canvas. Il charge aussi le véritable `index.html` et `app.mjs` dans un DOM isolé avec un faux `chrome.webview` pour vérifier Ctrl+N/O/S sans dialogue système.

Le test navigateur conserve une sonde Ping exacte et valide le montage de l'éditeur par l'API `editorReady`. Il ne remplace pas le contrôle natif du bridge WebView ni les anciens contrôles du transport.

## Limites de preuve

Les événements de composition du navigateur sont synthétiques : ils ne prouvent pas le comportement d'un IME physique ni des touches mortes. Les contrôles headless ne valident pas la sensation de caret, le contraste ou l'accessibilité avec un lecteur d'écran. Le host utilisé par la suite JS est injecté ; les dialogues, permissions et erreurs du vrai bridge sont à vérifier dans le self-check natif. Cette tranche ne revendique pas les objectifs de latence à 300 pages, la mise en forme riche, le bidi ou la validation clavier/IME physique.

L'interface n'expose aucun chemin de fichier. Les erreurs affichées sont des clés traduites, les octets du document transitent entre le moteur et le bridge, et les notifications dirty ne bloquent pas l'ouverture ou la sauvegarde lorsque les notifications intermédiaires sont coalescées.

## Limites d'édition de ce prototype

La représentation accessible expose le texte courant comme contenu DOM lisible, mais le Canvas reste la surface d'édition et cette tranche ne revendique pas une expérience complète d'édition assistive. Copier/couper en texte brut, Ctrl+flèches par mots/paragraphes et la conservation de la colonne verticale sont disponibles ; voir les contrats [presse-papiers](us-225-plain-clipboard.md), [navigation](us-221-keyboard-editing.md) et [souris](us-222-mouse-selection.md). Le wrapping aux opportunités de coupure et les tabulations configurables restent à développer dans US-210 et US-211.

## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, 368 tests workspace et 55 tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.

## Édition TXT au clavier, à la souris et presse-papiers — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle la navigation, la
sélection souris et le presse-papiers, puis trois revues indépendantes ont été
triées. Les [contrats clavier](us-221-keyboard-editing.md),
[souris](us-222-mouse-selection.md) et [texte brut](us-225-plain-clipboard.md)
décrivent les comportements livrés : Ctrl+flèches par mot/paragraphe,
Ctrl+effacement par mot, colonne visuelle conservée, Ctrl+A, F8/Échap,
glisser avec défilement aux bords, double/triple-clic et copier/couper/coller.

La [validation finale](../_bmad-output/implementation-artifacts/editing-ergonomics-validation.json)
réussit **41 contrôles, 377 tests workspace et 55 tests natifs release**.
Les 57 assertions du navigateur exercent le vrai Worker/WASM ;
l’ABI réelle refuse onze SelectPoint invalides sans mutation. Les sources et le
lock sont inchangés pendant les contrôles. Le shell embarque **18 ressources**,
et la [copie isolée](../_bmad-output/implementation-artifacts/editing-ergonomics-portable.json)
fonctionne sans dossier web/data. La [revue](../_bmad-output/implementation-artifacts/editing-ergonomics-review.md)
conserve 17 constats et leurs décisions, sans nouveau report. Les anciens reports
copy/cut, mot et colonne sont résolus pour cette tranche TXT.
Un timeout d’initialisation WebView2 est conservé dans
`editing-ergonomics-retry-validation.json` ; les contrôles restants ont été repris
sur les mêmes sources, avec des dossiers de diagnostic natifs isolés.
La sonde portable a également réussi à la seconde tentative, avec le même
exécutable ; `editing-ergonomics-portable-first-attempt.json` conserve son timeout
ExecuteScript initial. Ces reprises ne prouvent pas l’absence d’instabilité du runtime.

Le backlog reste à **35/158 US complètes**. US-214/220/221/222/223/225 restent
ouvertes : objets riches, phrases/marge/bloc rectangulaire, formats HTML/interne
et images, ainsi que les essais physiques, ne sont pas tous livrés. US-111
conserve son échec strict de handles. Les événements synthétiques ne valident pas
AZERTY/IME physique ; les tests musl sont compilés localement et exécutés en CI Linux.
