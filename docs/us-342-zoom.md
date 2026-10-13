# US-342 — Zoom du document

Le document texte et l’aperçu DOCX disposent d’un zoom de 10 à 500 %. Le curseur et le bouton de pourcentage restent visibles dans la barre inférieure. Le bouton et la commande **Affichage → Zoom…** ouvrent le même dialogue : pourcentage personnalisé, largeur de page, page entière ou deux pages consécutives dans la pile verticale.

Ctrl+molette zoome autour du point sous la souris. Le curseur et le dialogue conservent le point de lecture au centre de la fenêtre, dans les limites du défilement disponible. Le calcul tient compte des espaces fixes entre pages, des bordures et de l’apparition des barres de défilement. Les modes ajustés suivent la taille réelle du conteneur. Le zoom ne change ni le texte, ni les coordonnées du modèle, ni la sélection, ni l’historique.

Chaque vue conserve son facteur et son mode pendant sa durée de vie. Ouvrir puis quitter un aperçu DOCX conserve le zoom du texte en cours. Un nouveau document ou un chargement réussi réinitialise la vue à 100 %. Une annulation ou un import refusé conserve la vue précédente.

La mémorisation après fermeture et réouverture du même fichier reste à développer : le pont natif transmet les octets sans identité de fichier. Le deuxième critère d’US-342 reste donc ouvert. Le mode deux pages est vertical ; il n’introduit pas de grille côte à côte.

Les commandes utilisent les composants accessibles, les couleurs de thème et le catalogue de libellés existants. Le dialogue valide les pourcentages entiers de 10 à 500, permet l’annulation sans modification et restitue le focus. Les gestes ordinaires, AltGr et l’IME restent disponibles ; un dialogue modal bloque les commandes de zoom derrière lui. Pendant l’ouverture d’un fichier, les contrôles sont indisponibles.

## Contrats

- `web/zoom.mjs` : limites, calcul des modes ajustés et transformation des points de lecture entre facteurs.
- `web/editor.mjs`, `web/rich-preview.mjs` : `setZoom(facteur)`, `fitZoom(mode)`, `getZoom()`, `getZoomMode()`, callback `onZoom({zoom,mode})`. Les facteurs sont compris entre `0.1` et `5` ; les modes sont `custom`, `width`, `page`, `pages`.
- `web/zoom-controls.mjs` : `createZoomControls({document,labels,onChange})` avec `element`, `setState`, `open`, `dispose`. Les valeurs du contrat sont des facteurs ; les pourcentages appartiennent à l’interface.
- Les trois nouvelles ressources sont servies explicitement par l’origine native. Aucun changement ABI/WASM ; l’artefact optimisé reconstruit est identique à celui du lot précédent.

## Validation

Le script [validate-zoom.py](../_bmad-output/implementation-artifacts/validate-zoom.py) vérifie le format et Clippy, les tests desktop affectés, les ressources et licences, les helpers, le moteur embarqué, les suites Canvas/éditeur/UI dans Edge et la sonde native de ressources. Les résultats et les empreintes exactes des entrées sont consignés dans `zoom-validation.json` et `zoom-validation.log` à côté du script.

Les tests couvrent les bornes, le point de lecture loin du curseur, le point sous la souris, le défilement horizontal, les modes et le redimensionnement, les coordonnées de clic et l’annulation après édition, les erreurs et le focus du dialogue, les vues TXT/DOCX indépendantes, le contraste élevé et l’absence de débordement à 420 px. Les événements de clavier/souris du harness sont synthétiques dans un vrai navigateur ; ils ne constituent pas une validation de clavier physique ou de lecteur d’écran.
