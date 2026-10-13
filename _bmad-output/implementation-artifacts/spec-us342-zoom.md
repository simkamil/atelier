---
title: 'US-342 — Zoom du document'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: 94df23fb037145d75542f54dbc0215d0daf929bc
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="Reprise du backlog autorisée par l’utilisateur, avec développement parallèle">

## Intent

Permettre de lire le document avec un zoom de 10 à 500 %, commandé depuis la barre d’état et un dialogue. Le zoom agit sur la vue Canvas sans modifier la mise en page, le texte, la sélection ou l’historique. Les commandes fonctionnent dans le document texte et dans l’aperçu DOCX.

## Boundaries & Constraints

Toujours conserver la position de lecture : le point situé au centre de la fenêtre de lecture conserve son emplacement après changement de facteur, dans les limites du défilement disponible. Ctrl+molette conserve plutôt le point sous la souris. Les modes largeur, page entière et plusieurs pages suivent les changements de taille de leur conteneur. Plusieurs pages signifie deux pages consécutives dans la pile verticale actuelle, sans nouveau moteur de grille. Chaque vue garde son facteur et son mode pendant sa durée de vie ; un aller-retour entre texte et aperçu ne change pas le zoom du texte. Un chargement réussi d’un nouveau document initialise sa vue à 100 %, un dialogue annulé ou un import échoué conserve la vue précédente. Le pont natif ne fournit pas d’identité de fichier : la mémorisation après fermeture/réouverture du même fichier reste explicitement ouverte dans le backlog.

Réutiliser Carlito, les couleurs de thème et le dialogue accessible existants. Les libellés sont injectés depuis le catalogue courant. Ne pas implémenter US-102. Les commandes ne s’appliquent pas derrière un dialogue modal ; AltGr, IME et molette ordinaire restent disponibles. Pas de nouvelle dépendance npm, ni de changement ABI/WASM. Les agents ne commitent pas et ne relancent pas BMAD ou Graphify ; le parent assure l’intégration et la publication autorisée.

## I/O & Edge-Case Matrix

| Situation | Entrée | Résultat attendu |
|---|---|---|
| Limites | 10 %, 500 %, NaN, 9 %, 501 % | bornes acceptées, valeurs invalides rejetées sans changement |
| Lecture loin du curseur | défilement au milieu du document puis zoom | point de lecture conservé ; aucun recentrage sur le caret |
| Molette | Ctrl+molette ; molette seule ; AltGr | Ctrl zoome avec ancrage pointeur ; autres gestes inchangés |
| Modes ajustés | largeur, page, deux pages ; redimensionnement | calcul depuis les dimensions effectives, limites 10–500 % |
| Documents | texte → aperçu → retour ; ouverture annulée ou échouée | vue texte conservée ; échec sans perte de vue |
| Dialogue | saisie invalide, Entrée, Échap, focus | validation bornée, appliquer explicitement, annulation sans mutation, focus restitué |
| Fin de vie | dispose puis événements | aucun listener actif ni changement de vue |

</frozen-after-approval>

## Code Map

- `web/editor.mjs` — `setZoom` actuel 50–200 % republie l’état et recentre le caret ; les coordonnées sélection/drag utilisent `pageZoom`.
- `web/rich-preview.mjs` — aperçu distinct, géométrie A4 verticale et propre cycle de vie.
- `web/canvas-renderer.mjs` — sait déjà rendre tout facteur positif et défiler dans une pile verticale ; conserver ce contrat.
- `web/app.mjs`, `web/editor-shell.mjs`, `web/index.html` — intégration commune, contrôles désactivés pendant chargement, retour aperçu et onglet Affichage.
- `web/ui-components.mjs`, `web/theme.css` — dialogue, boutons et tokens existants à réutiliser.
- `crates/desktop-win/src/resources.rs`, `win32.rs` — routes embarquées explicites et sonde native.
- `tests/run-editor-browser.mjs`, `tests/editor-browser.mjs`, `tests/run-ui-browser.mjs` — vrais navigateurs, Worker/WASM et validation UI.

## Tasks & Acceptance

**Execution :**
- [x] Agent géométrie : `web/zoom.mjs`, `web/editor.mjs`, `web/rich-preview.mjs`, `tests/zoom.mjs`, `tests/rich-preview.mjs` — bornes, modes ajustés, ancrage, défilement horizontal, session et nettoyage.
- [x] Agent interface : `web/zoom-controls.mjs`, `web/zoom-controls.css` — curseur, pourcentage ouvrant dialogue, modes, validation, focus et contraste.
- [x] Parent : `web/app.mjs`, `web/editor-shell.mjs`, `web/index.html`, `web/i18n/fr-FR.txt` — relier les vues aux commandes et au ruban ; contrôle de statut visible sur tous les onglets.
- [x] Parent : routes natives et harnesses, tests réels couvrant la matrice, documentation et statut exact du backlog.

**Acceptance Criteria :**
- Given un document prêt, when l’utilisateur modifie le curseur ou applique une valeur du dialogue, then le pourcentage reflète le facteur réellement rendu, avec contrôles utilisables au clavier et à 420 px.
- Given plusieurs pages et une sélection, when le zoom change puis une édition est effectuée, then sélection, texte, annulation, clic et drag gardent leurs coordonnées documentaires correctes.
- Given un document ou une opération indisponible, when les commandes sont affichées, then aucun changement ne passe vers une vue inactive ou derrière un modal.

## Implementation Notes

Contrat entre agents : les vues exposent `setZoom(number)`, `fitZoom("width"|"page"|"pages")`, `getZoom()` (nombre), `getZoomMode()` (`custom` ou mode ajusté). Option de création `onZoom({zoom,mode})`, rappelée après changement/refit. Les commandes exposent `createZoomControls({document, labels, onChange})` retournant `element`, `setState({zoom,mode,disabled})`, `open({returnFocusTo})`, `dispose()`. `onChange({zoom,mode})` : mode custom avec facteur, sinon mode ajusté. Les nouveaux imports relatifs sont servis à la racine embarquée.

Le parent traite seul les fichiers partagés. Le développement est parallélisé selon la demande persistante de l’utilisateur ; la validation et la revue restent séquentielles après intégration. US-342 restera partielle tant que l’identité native des fichiers ne permet pas la mémorisation après réouverture.

## Spec Change Log

## Review Triage Log


| Source | Finding | Verdict et preuve | Suite |
|---|---|---|---|
| Blind 1 | Canvas texte sticky empêcherait le défilement horizontal | false — le renderer applique scrollX à chaque rendu ; Edge vérifie le pan réel et le canvas épinglé. | Rejeté |
| Blind 2 | Même risque dans l’aperçu DOCX | false — le renderer déplace les pixels en coordonnées de page, la largeur du canvas reste celle du viewport ; test DOCX de défilement horizontal passé. | Rejeté |
| Blind 3 | Le curseur step10 ne représente pas exactement 125 % | low — discrétisation du curseur réelle et volontaire ; bouton et aria-valuetext indiquent le facteur exact, testé à 125 %. Ajouter un contrôle continu distinct ne corrigerait aucun comportement métier. | Pas de complexité ajoutée |
| Blind 4 | Fit de la première page seulement, tailles hétérogènes | false — publish valide chaque page A4 avec la même tolérance avant toute installation. | Rejeté |
| Blind 5 | Même risque pour les pages DOCX de tailles différentes | false — prepareDocument valide toutes les pages A4 ; une dimension différente refuse l’aperçu. | Rejeté |
| Blind 6 | Le minimum 10 % peut dépasser une fenêtre minuscule | false — borne du produit 10–500 % ; le fit est explicitement borné et conserve le défilement si la taille ne permet pas tout afficher. | Rejeté |
| Blind 7 | Helper géométrique accepte NaN ou dimensions invalides | false — les consommateurs valident le facteur avant appel, pages décodées validées et géométrie DOM finie ; aucune entrée invalide atteignable montrée. | Rejeté |
| Blind 8 | Molette depuis une autre partie de l’interface | false — chaque listener wheel est attaché uniquement au scrollport de document, avec garde de vue active ; un autre élément de l’interface ne lui transmet pas ses événements. | Rejeté |
| Blind 9 | preventDefault aux bornes du zoom | false — laisser passer Ctrl+molette déclencherait le zoom navigateur après la borne du document ; interception nécessaire pour maintenir 10–500 %. | Rejeté |
| Blind 10 | Le statut ne signale pas le mode ajusté actif | low — mode consultable dans le dialogue mais absent du bouton ; title et nom accessible ajoutés avec le mode et le vrai pourcentage, test ciblé ajouté. | Corrigé |
| Blind 11 | Promesse de mémorisation malgré reset au chargement | false — backlog deuxième critère toujours ouvert ; docs distinguent clairement durée de vie de vue et réouverture de fichier, sans identité native. | Rejeté |
| Verification 1 | Ancrage du consommateur preview insuffisamment testé | medium — tests initialement limités au helper et à l’éditeur ; assertions indépendantes de centre et pointeur ajoutées sur une preview multi-page avec offsets non nuls. | Corrigé |
| Edge | Aucun finding retourné après reprise du shell cmd.exe | false — première tentative interrompue par PowerShell bloqué, revue reprise sans modification et résultat vide. | Couche exécutée |
| Intégrateur | Observer ajuste une vue masquée sur une taille nulle | medium — masquer le texte en mode largeur exposait dimensions zéro ; garde sur viewport nul dans les deux vues et test Edge aller-retour DOCX en mode ajusté. | Corrigé |

## Design Notes

Prolonger la barre inférieure Atelier : groupe compact aligné à droite, curseur natif, bouton de pourcentage avec un libellé explicite, dialogue court. Pas de nouvelle palette ni de décoration. Les deux pages restent verticales pour préserver la même géométrie dans le moteur, la souris et le rendu.

## Verification

- `node tests/zoom.mjs` et `node tests/rich-preview.mjs` : calculs, bornes, ancrage et cycle de vie.
- Harnesses navigateur éditeur/UI et vérification native : zoom visible, mode ajusté, molette, dialogue, sélection, thème et chargement réel.
- Format/lint Rust et tests desktop affectés : ressources complètes. Le moteur inchangé est comparé à l’artefact embarqué.

Validation initiale : 17 contrôles passés. Revue : trois agents GPT-6 Luna moyen indépendants, corrections ciblées réintégrées et validation finale passée après gel des sources : 17 contrôles, 57 tests Rust desktop, 152 assertions éditeur Edge, interface normale et contraste élevé, sonde native avec 31 ressources. Graphify retire trois anciens symboles US-313 uniquement après comparaison des deux commits sources ; aucun concept documentaire n’est supprimé.
