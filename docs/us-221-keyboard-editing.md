# US-221 — Navigation rapide dans le texte brut

Le moteur garde les offsets en octets UTF-8 et applique UAX #29 Unicode 17 pour les mots. Les directions de déplacement du payload opcode 8 sont 10 (mot à gauche), 11 (mot à droite), 12 (paragraphe précédent) et 13 (paragraphe suivant). Le déplacement horizontal par mot traverse au moins un segment, puis saute les segments blancs vers la destination; la ponctuation reste une unité. Les fins CR, LF et CRLF sont des fins de paragraphe, CRLF étant indivisible.

Les opcodes 12 et 13 effacent respectivement le mot précédent et suivant. Leur corps est un horodatage little-endian u64. Une sélection existante est effacée en entier. Chaque action forme une opération d’historique et undo restaure le texte et la sélection dirigée. Les erreurs de budget ne publient pas le candidat.

La colonne X est mémorisée pour les déplacements haut/bas et page précédente/suivante et remise à zéro après mouvement horizontal, navigation de mot ou paragraphe, hit-test, sélection, édition, chargement, undo et redo.

Opcode 14 sélectionne au point. Son corps est page:u32, x:f64, y:f64, unité:u8, extend:u8, réservé:u16, anchor:u32, anchorEnd:u32. Unité 0 place un caret; unité 1 sélectionne un segment UAX #29; unité 2 sélectionne le paragraphe avec sa fin de ligne. Sans extension, les deux ancres doivent être nulles. Pour une extension caractère, anchorEnd doit être égal à anchor, qui doit être une frontière de graphème. Pour une extension mot/paragraphe, les ancres doivent être des frontières de graphèmes et anchor doit être inférieur ou égal à anchorEnd. Toute commande invalide échoue avant publication de l’état candidat.

Quand une suppression de mot part d’un caret, undo rétablit le texte et la sélection de plage enregistrée par `PlainTextDocument`; le caret peut donc réapparaître comme une sélection du mot supprimé. Ce comportement vient de l’API d’édition actuelle.

La validation couvre le maintien du X à travers une ligne courte, l’undo d’une sélection inversée multioctet et le rejet d’une commande invalide via l’enveloppe WASM. Cela ne valide pas encore les comportements clavier physiques AZERTY/IME ni la complétion des US parentes.

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
