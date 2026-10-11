# US-222 — Sélection souris et défilement du texte brut

Le contrôleur isolé `web/mouse-selection.mjs` transforme les événements du
pointeur en appels `selectPoint` asynchrones. Le Canvas garde la capture pendant
le glisser; l'ancre et l'unité choisie restent stables et le point final du
relâchement est traité même si un appel moteur est encore en attente. Le module
convertit les coordonnées CSS en points de page sans appliquer le DPR, choisit
la page la plus proche dans un espace inter-page et borne le défilement sur les
dimensions réelles du scrollport.

Le test ciblé `node tests/mouse-selection.mjs` couvre la capture jusqu'au
relâchement hors Canvas, Shift sur une sélection dirigée, double-clic mot,
triple-clic paragraphe, mouvement et relâchement pendant un appel asynchrone,
libération implicite de capture après pointerup, annulation d'un appel pendant,
désactivation pendant un appel avec mouvement en attente, conversion avec zoom
et espace entre pages, et arrêt du RAF à la limite du scroll. Résultat local :
succès.

Cette preuve est synthétique. Elle ne valide pas le ressenti d'un pointeur réel,
les gestes du système d'exploitation ni l'interaction physique avec AZERTY/IME.
L’intégration avec la queue de commandes, le vrai Worker/WASM, l’undo et les ressources natives est vérifiée dans la suite globale ci-dessous. Les erreurs du callback isEnabled annulent capture et RAF et sont rapportées une fois.

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
