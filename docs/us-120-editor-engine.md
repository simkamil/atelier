# Moteur de l’éditeur texte US-120

`atelier-core-wasm` traite un éditeur par instance WASM. Les commandes ATLM v1
`Editor` (`0x0020`) reçoivent une réponse `EditorResult` (`0x8020`) avec le même
`requestId`; une commande refusée renvoie `Failed` et un code ASCII `EDITOR_*`.
Les autres types et le comportement écho de l’ABI restent inchangés.

Chaque charge Editor contient `ATED`, version u16 little-endian `1`, opcode u16,
puis le corps décrit dans `editor-contract.md`. Les réponses d’état commencent
par `ATES`, version 1, indicateurs undo/redo/dirty/BOM, révision, sélection en
octets UTF-8, nombre de pages, caret, puis texte UTF-8 hors BOM, pages ATDL v1
longueur-préfixée et rectangles de sélection. L’opcode Export renvoie `ATEX`,
version 1, révision et octets du fichier exact. Mark-saved efface dirty seulement
si la révision exportée est toujours courante. Undo jusqu’au contenu de référence
redevient propre même si le numéro de révision a avancé.

Les modifications s’exécutent sur une copie candidate du document et de son
historique. La mise en page et la réponse sont préparées avant que la session soit
publiée; le dispatch WASM réserve aussi les allocations de réponse avant le
commit. Une commande rejetée conserve le contenu, la sélection, l’historique et
la référence de sauvegarde. Cette copie complète est volontairement simple et
n’a aucune garantie de latence.

Le moteur utilise Carlito Regular embarquée, LatinShaper à 11 pt, des pages A4
de 595,276 × 841,89 points, marges de 54 points et interligne fixe de 14 points.
Le wrapping s’effectue aux graphèmes; chaque glyphe produit un chemin ATDL
`FillPath` à partir de son contour TrueType. Les courbes sont aplaties avec une
tolérance de 0,02 point convertie en unités de police avant sérialisation. Les tabulations sont
rendues comme quatre espaces. Le layout conserve les positions de caret pour les
mouvements, le hit-test et les rectangles de sélection que l’interface affiche
comme overlay.

Les plafonds sont 128 Kio de texte hors BOM, 4 Mio d’historique, 32 Mio par
réponse d’état et 1024 pages. Les erreurs publiques sont stables : notamment
`EDITOR_INVALID_COMMAND`, `EDITOR_INVALID_UTF8`, `EDITOR_INVALID_SELECTION`,
`EDITOR_TEXT_LIMIT`, `EDITOR_HISTORY_LIMIT`, `EDITOR_PAGE_LIMIT`,
`EDITOR_RESPONSE_LIMIT` et `EDITOR_STALE_SAVE`.

Cette tranche ne comprend pas les dialogues et l’I/O natifs, le rendu des
overlays/clignotement, la composition IME, le bidi, les styles enrichis ni les
mesures de démarrage et de latence sur 300 pages. Le shaper est Latin gauche à
droite; les graphèmes sont façonnés séparément, donc les ligatures et le crénage
entre deux graphèmes voisins ne sont pas conservés. Les sauvegardes sont
coordonnées avec le pont natif via l’export et la confirmation de révision.

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
