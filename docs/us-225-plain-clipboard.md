# US-225 — Presse-papiers texte brut

La surface de saisie reçoit les événements natifs `copy`, `cut` et `paste`. Pour copier ou couper, elle demande un instantané au glue de l’éditeur, extrait la plage sélectionnée selon ses offsets UTF-8 et appelle synchroniquement `clipboardData.setData("text/plain", ...)`. La coupe transmet ensuite `deleteSelection` avec la plage et la révision de l’instantané. Le glue vérifie cette révision et cette plage avant de modifier le document.

Le presse-papiers ne reçoit que `text/plain`. Les formats HTML, riches, image et internes ne sont ni lus ni écrits. Le collage garde le chemin natif `paste` et `Ctrl+Maj+V` suit le même chemin; le dédoublonnage existant empêche une seconde insertion par `beforeinput`. `Ctrl+C` et `Ctrl+X` restent pilotés par les événements clipboard natifs.

La copie et la coupe restent inactives sans instantané stable, sur une sélection vide, pendant une composition ou si l’écriture échoue. Quand le callback d’instantané du document est configuré, l’événement est annulé dès l’entrée pour empêcher une copie du tampon transitoire du textarea invisible ; aucun texte ni commande de suppression n’est émis dans ces cas. L’adaptateur sans callback conserve le comportement natif. `Ctrl+A`, `F8` et `Échap` émettent respectivement `selectAll`, l’activation et la désactivation du mode d’extension.

Les vérifications ciblées sont exécutées dans le navigateur réel via `node tests/run-frontend-browser.mjs input` après intégration de `tests/text-input-editing.mjs` au runner. Elles couvrent les offsets UTF-8, la sélection inversée, l’ordre écriture/suppression, l’échec de `setData`, les instantanés absents ou invalides, la composition, les raccourcis et le nettoyage des écouteurs. Ces événements synthétiques ne valident pas la disposition AZERTY ni une saisie IME physique. L’US reste ouverte jusqu’à validation des critères complets et des preuves manuelles prévues.

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
