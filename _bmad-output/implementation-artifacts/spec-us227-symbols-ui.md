---
title: 'US-227 — Insérer des symboles dans le TXT'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: f419d339daef132a29fdbdeb68ca0093aaaa0a57
context: []
---

<frozen-after-approval reason="Poursuite autonome du backlog autorisée, composants isolés développés en parallèle">

## Intent

Insérer dans le texte éditable des symboles absents du clavier, depuis Insertion → Symbole. Le dialogue propose une grille par bloc Unicode, recherche par nom officiel anglais Unicode et code U+, symboles récents et un onglet des onze caractères spéciaux de l'US. Ctrl+Maj+Espace insère U+00A0. Le moteur actuel compose le TXT en Carlito : la grille annonce cette police et propose sa couverture réelle, sans prétendre offrir les polices et attributs riches encore absents. L'aperçu DOCX reste en lecture seule. L'insertion remplace la sélection courante par la voie d'édition existante, avec Undo/Redo.

## Boundaries & Constraints

Composant DOM autonome, textes visibles injectés depuis i18n, styles via tokens existants, dialogue et onglets partagés. Une seule police actuellement disponible : Carlito. Catalogue produit par le parent depuis le cmap réel et l'UCD 17 local, sans téléchargement ni dépendance externe. Au plus 256 cellules visibles, pagination explicite, recherche bornée ; récents limités à 16 pendant la session. Aucun changement direct du modèle depuis le dialogue. Tous les caractères spéciaux listés dans l'US sont conservés avec leur vrai scalaire, notamment espaces et trait d'union insécables. Ne pas présenter l'édition DOCX ou le choix de police riche comme terminé.

## I/O & Edge-Case Matrix

| État | Action | Résultat |
|---|---|---|
| TXT prêt avec sélection | Choisir puis insérer | Callback attendu avec le seul scalaire choisi ; sélection remplacée par le parent |
| Recherche vide ou sans résultat | Changer requête/bloc | Résultats bornés ou état vide annoncé, insertion désactivée sans choix |
| Dialogue actif | Flèches, Tab, Échap | Grille accessible, focus visible, fermeture et restauration |
| Derniers symboles | Réinsérer même symbole | Déduplication, plus récent en tête, maximum 16 |
| DOCX/busy/composition | Commande | Parent bloque sans modifier TXT |

</frozen-after-approval>

## Code Map

- `web/ui-components.mjs` : createDialog, attachTabs ; ne pas les modifier.
- `web/symbol-dialog.mjs`, `web/symbol-dialog.css` : nouveaux fichiers agent.
- `tests/symbol-dialog-browser.mjs` : tests navigateur exportant une fonction suite, réutilisable par le runner parent.
- `web/editor.mjs`, `web/app.mjs`, `web/editor-shell.mjs`, catalogue, ressources et génération du catalogue : exclusivement parent.

## Tasks & Acceptance

- [x] Implémenter `createSymbolDialog({labels,catalogue,onInsert})` où catalogue contient `{font:"Carlito",blocks:[{id,name,start,end}],symbols:[{codePoint,name,block}]}` ; retour `{element,open,close,dispose,getRecentSymbols}`.
- [x] Prévoir les clés labels title/close/symbols/specialCharacters/search/block/font/recent/insert/noResults/previous/next/page et noms localisés des caractères spéciaux ; exposer les clés requises.
- [x] Tester recherche, bloc, codepoint, symbole multi-octet, pagination, récents, contre-exemples, onglets/focus, dispose et callback asynchrone refusé sans perte du choix.
- Given un catalogue valide, when une recherche par nom/code est faite, then seuls les scalaires correspondants du bloc sélectionné sont proposés.
- Given une sélection, when Insertion est confirmée, then le callback asynchrone reçoit le texte exact une seule fois ; le symbole rejoint les récents après succès.

## Implementation Notes

L'agent possède seulement les nouveaux fichiers dialogue/CSS/tests ; le parent assure les raccordements et les preuves de bout en bout. Ne pas relancer Graphify sur la sortie canonique : le parent a déjà rafraîchi le graphe et en assurera la validation finale.

## Review Triage Log

| ID | Verdict | Vérification et décision |
|---|---|---|
| B1 | medium | Une recherche non préfixée composée de lettres A–F était interprétée comme code ; des fragments de noms tels que AC (ACUTE) perdaient leur sens. Codes désormais préfixés U+/0x, test FACE dans le catalogue synthétique. |
| B2 | false | Le focus initial de createDialog restait sur la cellule spéciale car focus() sur le champ caché est sans effet. Appel redondant supprimé ; réouverture spéciale testée, sans attribuer une perte de focus non démontrée. |
| B3 | medium | Le déplacement vertical était fixé à huit, alors que les grilles spéciales et étroites ont six/quatre colonnes. Le nombre de colonnes rendu pilote désormais la navigation. |
| B4 | medium | Le remplacement des cellules après clic supprimait la cible de focus. Focus restauré par scalaire dans la grille, y compris après redimensionnement. |
| B5 | medium | Un indice des récents était appliqué aux résultats. Les positions actives sont désormais indépendantes par grille et testées. |
| B6 | medium | Les cellules de grille n’étaient pas possédées par des lignes. Lignes ARIA, indices et nombres de lignes/colonnes ajoutés, recalculés selon le rendu. |
| B7 | low | L’option globale réutilisait le libellé du champ Bloc. Clé dédiée Tous les blocs ajoutée. |
| B8 | low | U+2011, spécial connu mais absent de la cmap, ne pouvait être trouvé par code. Les codes explicites consultent aussi les onze spéciaux connus, jamais les scalaires arbitraires non couverts. |
| B9 | false | Le TXT courant et l’aperçu de glyphes utilisent réellement Carlito ; aucune autre police n’est promise. Le catalogue est comparé à la cmap du moteur et le seul repli U+2011 est testé séparément sans modifier le texte. |
| B10 | medium | Le parent signalait déjà le refus hors du dialogue mais ce message était masqué par le modal. Le dialogue annonce maintenant le refus dans sa région de statut, en gardant le choix et les récents intacts. |
| E1 | medium | Même cause que B1 ; recherche de nom et recherche de code explicite sont distinguées. |
| E2 | medium | toLocaleLowerCase pouvait modifier les I des noms anglais Unicode selon la locale. Recherche désormais avec toLowerCase stable et régression dédiée. |

## Verification

Suite réelle Edge/Chromium ; insertion/Undo/Redo/shortcut et blocage aperçu validés par le parent avec Worker/WASM.

Le catalogue contient 2 107 scalaires nommés réellement couverts par la cmap Carlito et 36 blocs. Le trait d’union insécable U+2011 reste exact dans le modèle ; faute de glyphe, display_glyph_index choisit U+2010 puis U+002D. La cmap stricte reste inchangée. Les tests façonnage/font manager et aperçu DOCX vérifient le scalaire, les clusters et le vrai contour de tiret.

Validation finale du lot : Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.
