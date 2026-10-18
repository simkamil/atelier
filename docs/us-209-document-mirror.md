# US-209 — Miroir DOM accessible du document

Tranche R5 du [plan de bascule](rich-editor-migration.md). Le document est dessiné sur un Canvas, que les lecteurs d'écran ne voient pas (SPEC §6.14, D13). `web/document-mirror.mjs` expose la portion visible dans le DOM, hors champ visuel, et suit le curseur et la sélection du modèle. Style : `web/document-mirror.css`. Chaînes : clés `mirror.*` de `web/i18n/fr-FR.txt`.

## Donnée d'entrée

Le module ne parle pas au moteur : il reçoit `getAccessibleWindow(start, end, { signal })`, qui renvoie (positions plates du protocole [US-203](us-203-rich-editor-protocol.md), BigInt) :

```
{ revision, items: [ { paraStart, paraEnd, role, level, label, text, objects: [ { offset, alt } ] } ] }
```

`role` : 0 paragraphe, 1 titre (`level` 1-9), 2 élément de liste (`level` 0-8, `label` « 1. », « • »), 3 cellule (mode dégradé : un paragraphe), 4 bloc opaque. Au plus 256 éléments et 256 Kio de texte ; la réponse est validée (`validateWindow`, erreur typée `DocumentMirrorError` `MIRROR_BAD_WINDOW`) avant d'atteindre le DOM. Les U+FFFC listés dans `objects` sont des objets ; les autres (sauts, symboles) sont retirés, remplacés par une espace s'ils séparaient deux mots.

**État à fournir à chaque State** (par l'intégrateur, depuis `EditorPageCache`) :

```js
mirror.update({ revision, docLen, selection: { anchor, focus }, visible: { start, end } })
```

`visible` est la portion affichée, en positions plates (à défaut : autour du focus) ; le module demande `visible` plus une marge (`margin`, 4096 octets par défaut) bornée à `[0, docLen]`. À rappeler quand la portion visible change (défilement, zoom).

## Architecture

- **DOM** : `div.document-mirror[role=document][aria-label]` contenant le contenu et une région `role=status`. Construit par `createElement`/`textContent`, jamais `innerHTML`. `h1`-`h6` pour les titres (niveaux 7-9 : `div[role=heading][aria-level]`), `div[role=list]` regroupant les éléments consécutifs, `div[role=listitem][aria-level=niveau+1]` avec l'étiquette en texte (« 1. »), `p` pour le reste, `span[role=img][aria-label=alt]` pour les objets (clé `mirror.image.noalt` sans texte alternatif), bloc opaque : « Contenu non textuel ». Un paragraphe vide contient un `br` pour être annoncé.
- **Mise à jour incrémentale** : chaque paragraphe a une signature de contenu sans position ; `planReconcile` (préfixe, suffixe, multi-ensemble) réutilise les nœuds inchangés même décalés par une insertion. Seuls les paragraphes modifiés sont recréés ; les positions des autres sont mises à jour sans toucher au DOM.
- **Requêtes périmées** : une requête par révision et par plage ; une nouvelle révision annule (`AbortSignal`) la précédente ; une réponse dont la révision n'est pas celle demandée, ou arrivée après remplacement, est ignorée (`stale`). Une seule requête sert plusieurs `update` de même révision dont la plage est couverte ; un changement de sélection seul, dans la fenêtre déjà chargée, ne rappelle pas le moteur.
- **Position plate ↔ nœud DOM** : chaque paragraphe est découpé en segments (texte, objet, vide) avec leur plage d'octets UTF-8 ; `flatToSegmentPoint`/`segmentPointToFlat` convertissent dans les deux sens (octets UTF-8 ↔ unités UTF-16, jamais au milieu d'un scalaire). Une position hors de la fenêtre est ramenée à son bord.

## Technique d'exposition

Hors champ : `position:absolute; 1×1 px; overflow:hidden; clip-path:inset(50%)`. Ni `display:none`, ni `visibility:hidden`, ni `aria-hidden` (qui retireraient le miroir de l'arbre d'accessibilité). Aucun `tabindex` : le miroir ne vole jamais le focus ; la zone de saisie (`text-input.mjs`, `textarea` vide qui garde le focus pour clavier et IME) reste le seul élément focalisable. En `forced-colors`, rien n'est peint (pas de bordure, pas de fond) et le miroir reste coupé.

Suivi du curseur et de la sélection, trois mécanismes combinés :

1. **Sélection DOM** (`Selection.setBaseAndExtent`, sens conservé) alignée sur la sélection du modèle. Vérifié dans Edge : elle ne déplace pas le focus. Les lecteurs d'écran en mode navigation (NVDA, Narrateur) y trouvent leur point de lecture.
2. **Région live polie** (`role=status`) : paragraphe courant à l'arrivée dans un nouveau paragraphe (avec « Titre de niveau n », « Élément de liste, niveau n », « Cellule de tableau »), texte sélectionné (« Sélection : … », 1000 caractères au plus) pour une sélection non vide, caractère suivant le curseur quand il bouge dans le paragraphe sans édition. Rien n'est annoncé pour la frappe (l'écho de frappe relève du lecteur) ni pour un curseur immobile. Option `announce:false` pour la couper.
3. **`aria-activedescendant`** sur la zone de saisie, vers le paragraphe courant, **désactivé par défaut** (option `activeDescendantTarget`).

Compromis, à confirmer par l'essai US-354 :

- Le miroir ne connaît pas les lignes visuelles (retours à la ligne automatiques), seulement les paragraphes : « annoncer la ligne » est approché par le paragraphe à l'entrée et par le caractère suivant dans le paragraphe. Un lecteur qui lit « ligne par ligne » dans le miroir lit des paragraphes.
- La région live peut doubler l'annonce propre au lecteur quand la sélection DOM change. Si l'essai montre des doublons, couper `announce` ou `domSelection` ; c'est un réglage, pas un changement d'architecture.
- `aria-activedescendant` sur un `textarea` est valide en ARIA 1.2 mais inégalement pris en charge ; il sert de recours si la sélection DOM ne suffit pas.
- Les listes imbriquées sont plates (`aria-level`), pas de `ul` imbriqués. Cellules : paragraphes, sans structure de tableau tant que la mise en page des tableaux n'existe pas. Sauts de ligne et symboles inline (tous en U+FFFC) sont indiscernables des objets sans `objects` : retirés.
- Les annonces de changement de mise en forme (SPEC §6.14) ne sont pas traitées ici.

## Tests

- `node --test tests/document-mirror.mjs` : logique pure sur faux DOM (conversion octets/UTF-16, segments et mappage plat ↔ nœud, réconciliation, validation, structure, sélection avec sens, requêtes périmées et annulées, annonces).
- `node tests/run-document-mirror-browser.mjs` : Edge headless, affichage normal puis `--force-high-contrast` (35 contrôles chacun) : structure, rôles et niveaux, alt, 1×1 px non masqué, sélection DOM suivant le modèle (UTF-8, sens inversé), focus conservé dans la zone de saisie réelle (`attachTextInput`), mise à jour incrémentale, requête périmée.

## Procédure d'essai NVDA / Narrateur (campagne US-354, non faite)

Prérequis : build de l'application avec le miroir branché sur les vraies données du moteur (`AccessibleWindow`) ; Windows 11 ; NVDA (version stable courante) puis Narrateur (Win+Ctrl+Entrée) ; document d'essai contenant un titre 1, un titre 2, un titre 8, une liste à deux niveaux numérotée, une image avec texte alternatif, une image sans, une cellule de tableau, un paragraphe vide, du texte accentué et un objet en milieu de phrase. Consigner version du lecteur, navigateur WebView2, date, testeur.

Pour chacun des deux lecteurs :

1. Ouvrir le document, focus dans la zone de document. Le lecteur annonce-t-il le nom de la zone (« Saisie du texte du document ») ?
2. Flèches haut/bas : annonce du paragraphe, du niveau de titre, de l'élément de liste avec son niveau et son étiquette. Noter si l'annonce est doublée.
3. Flèches gauche/droite, Ctrl+gauche/droite : caractère et mot annoncés ; comparer au caractère effectivement sous le curseur.
4. Maj+flèches, Ctrl+Maj+flèches, Maj+Fin : texte sélectionné annoncé (et « désélectionné » en relâchant) ; sélection inversée (vers la gauche).
5. Image avec et sans alt : annonce « graphique/image » + texte, ou « Image sans texte alternatif ».
6. Taper une phrase, supprimer, annuler (Ctrl+Z) : pas d'annonce parasite du miroir ; le curseur reste cohérent.
7. Mode navigation du lecteur (NVDA : Échap puis flèches ; Narrateur : mode Analyse) : lecture du miroir par titres (H, Maj+H), listes (L), images (G) : la structure est exposée.
8. Contraste élevé Windows (Alt gauche+Maj gauche+Impr écran) : rien de visible ne s'ajoute à l'écran ; l'annonce reste identique.
9. Document de plus de 256 paragraphes : défilement, le miroir suit la zone visible.
10. Si doublons ou silence : refaire l'essai avec `announce:false`, `domSelection:false`, puis avec `activeDescendantTarget`, et consigner la combinaison retenue.

Résultat attendu et consigné par lecteur : tableau (étape, annoncé, attendu, verdict), anomalies, réglage final. Cette campagne coche le second critère d'US-209, pas ce document.

## Intégration (à faire par l'intégrateur)

- Brancher `getAccessibleWindow` sur `rich-editor-client.mjs` (opcode `AccessibleWindow`) et appeler `mirror.update` après chaque State et chaque changement de portion visible.
- Créer le miroir dans le conteneur du Canvas (`position` non statique), lier `web/document-mirror.css`, passer `i18n.t` et la zone de saisie.
- Déclarer `/document-mirror.mjs` et `/document-mirror.css` dans les ressources embarquées (`crates/desktop-win/src/resources.rs`, sondes de `win32.rs`) et dans `tests/run-editor-browser.mjs` s'il sert l'éditeur ; ajouter les deux suites ci-dessus à la CI ; ajouter les clés `mirror.*` au repli `FALLBACK` de `web/app.mjs`.
