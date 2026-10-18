<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# R7a — Parité d'API entre l'éditeur v1 (`createEditor`) et l'éditeur v2 (`createRichEditor`)

Tranche R7a du [plan de bascule](rich-editor-migration.md) : l'interface (`web/app.mjs`) peut créer l'éditeur v2 derrière un choix de moteur. Ce document est le contrat : pour chaque membre de l'objet renvoyé par `createEditor` (`web/editor.mjs`) et chaque appel des modules qui l'utilisent, l'équivalent v2 (`web/rich-editor.mjs`, au-dessus de `web/rich-editor-client.mjs` et `web/editor-page-cache.mjs`) ou « absent en v2 » avec la conséquence. Aucune case d'US-219 n'est cochée par R7a.

Convention : `v2 = couvert` (même nom, même forme de résultat) ; `v2 = adapté` (même nom, forme documentée) ; `v2 = désactivé` (la commande est grisée avec une info-bulle i18n, l'appel direct rejette `EditorError("EDITOR_UNSUPPORTED_V2")`, jamais un échec silencieux) ; `v2 = absent` (rien à faire, sans conséquence visible).

## 1. Choix du moteur

`v1` par défaut. `v2` si l'URL de la page contient `?engine=v2` ; sinon si la préférence persistée `localStorage["atelier.engine"]` vaut `v2` ; `?engine=v1` force v1 même si la préférence demande v2. Une valeur inconnue retombe sur v1. La préférence se règle depuis la console (`localStorage.setItem("atelier.engine", "v2")`) tant que R7b n'a pas livré de réglage visible ; la bascule par défaut est R7b.

## 2. Options de `createEditor` -> `createRichEditor`

| Option v1 | v2 |
|---|---|
| `client` (client Worker : `request(frame, {signal})`) | couvert : habillage des trames ATED v2 dans `MessageType.Editor` (transport de `createRichEditorClient`) |
| `host` (`open`, `save`, `notifyDirty` obligatoires) | adapté : `notifyDirty` obligatoire ; `pickOpen`/`readTicket`/`pickSave`/`writeTicket`/`releaseTicket` (US-113) utilisés s'ils existent, sinon repli sur `open()`/`save(bytes)` (hôtes de test) |
| `canvas`, `scrollport`, `spacer` | couvert |
| `textRepresentation` | adapté : le texte entier n'est plus recopié dans un nœud (limite 1 Mio, R5) ; le miroir DOM accessible (`document-mirror.mjs`) le remplace, créé dans le parent du Canvas. L'option est acceptée et vidée (le nœud reçoit `hidden`) |
| `frenchSpacing`, `setFrenchSpacing` | couvert (contexte lu dans la fenêtre de texte de l'État) |
| `inputLabel`, `confirmDiscard` | couvert |
| `onState`, `onError`, `onBusy`, `onZoom`, `onViewport`, `onImages`, `imageAlt` | couvert, mêmes formes |
| (nouveau) `t`, `user`, `version`, `onReadOnly`, `screenReaderMode`, `onScreenReaderMode`, `docId`, `layoutBudget`, `dialogLabels`, `mirrorContainer` | propres à v2 ; sans `t` (i18n du miroir) il n'y a pas de miroir DOM ; `dialogLabels` : titres et libellés des dialogues natifs ; `user` vide si inconnu, `version` : version de l'application |

## 3. Membres de l'objet renvoyé

Forme de l'état publié (`state`, `getState()`, argument de `onState`) : v1 `{kind:"state", text, textBytes, anchor, focus, revision (BigInt), dirty, canUndo, canRedo, caret, selectionRects, pages, imageIds...}`. v2 : `{kind:"state", revision (BigInt), anchor, focus (Number, octets plats), docLen, dirty, canUndo, canRedo, readOnly, hasRevisions, pageCount, layoutComplete, caret, upstream}`. **Absents en v2 : `text`, `textBytes`, `selectionRects`, `pages`, `imageIds`** (pas de texte entier côté interface). **La surbrillance de la sélection n'est pas dessinée en v2** : le protocole ne renvoie que le caret du foyer (`selection_rects` existe dans le moteur, `PageCache::selection_rects`, mais aucune commande ni champ d'`State` ne l'expose) ; la sélection reste perceptible pour les lecteurs d'écran (miroir DOM) et par le texte copié, pas à l'écran. À livrer côté protocole avant la bascule par défaut (R7b). L'objet d'état reste le même tant que rien de ce qu'il porte ne change (identité conservée par un rafraîchissement de pages, un zoom). Lecteurs touchés : voir §5.

| Membre v1 | Équivalent v2 | Statut |
|---|---|---|
| `state`, `getState()` | idem, forme ci-dessus | adapté |
| `busy` | idem | couvert |
| `element` | idem (zone de saisie `attachTextInput`) | couvert |
| `focus()` | idem | couvert |
| `setFrenchSpacing(bool)` | idem | couvert |
| `setSuspended(bool)` | idem (désactive la saisie, annule souris et glissement) | couvert |
| `previewDocx(bytes, {signal})` | **aperçu DOCX en lecture seule remplacé** : en v2 un DOCX s'ouvre dans l'éditeur (lecture seule s'il porte des révisions ou des commentaires). L'appel rejette `EDITOR_UNSUPPORTED_V2` ; `app.mjs` n'appelle pas l'aperçu en v2 (le bouton « Ouvrir DOCX… » ouvre dans l'éditeur) | désactivé |
| `setZoom(z)`, `fitZoom(mode)`, `getZoom()`, `getZoomMode()` | idem (échelle du Canvas ; `fitZoom` utilise la géométrie de la première page connue, A4 à défaut) ; `onZoom` idem | couvert |
| `getViewportState()` | idem (`{page, totalPages}` depuis `pageCount` du moteur) | couvert |
| `goToPage(n)` | idem (défile ; les pages sont demandées par `setVisibleRange`) | couvert |
| `initialize()` | ouvre un document vierge (`Open` blank, profil TXT) et résout l'état | couvert |
| `canEdit` | idem, faux aussi en lecture seule | couvert |
| `editSelection(text, snapshot)` | `ReplaceRange` sur la sélection du cliché ; refusée par `RICH_READ_ONLY` | couvert |
| `selectionAction(action, snapshot)` | `selectAll` = `SetSelection(0, docLen)`, `undo`, `redo` | couvert |
| `search(q, options)` | `Search` (R7b-2) -> `{kind:"search", revision (BigInt), total, truncated, totalCapped, items:[{start, end (Number, octets plats), excerpt:{text, start, end, truncatedBefore, truncatedAfter}}]}` ; **les éléments ne portent plus `rects`** : le surlignage les demande à part (`SearchRects`). Pas de caractères génériques ni de direction (absents de v1) | adapté |
| `statistics()` | `Statistics` -> `{kind:"statistics", revision, anchor, focus (Number), layoutComplete, document, selection}` avec `{words, characters, charactersWithSpaces, paragraphs, lines, pages}` ; lignes et pages provisoires tant que `layoutComplete` est faux | couvert |
| `replaceCurrent(values)`, `replaceAll(values)` | `Replace` et `ReplaceAll` -> `{kind:"replacement", count, state}` ; Tout remplacer en une étape d'annulation, refusé en lecture seule (`EDITOR_READ_ONLY`) et au-delà de 5000 occurrences (`EDITOR_TOO_MANY_MATCHES`) | couvert |
| `setSearchHighlights(result, index)` | retient les plages ; les rectangles des pages visibles viennent de `SearchRects`, le Canvas peint toutes les occurrences visibles (la courante d'une autre couleur) ; `null` efface ; résultat d'une autre révision : `false` | couvert |
| `selectSearchResult(result, index)` | `SetSelection` sur l'occurrence ; révision périmée : `EDITOR_STALE_QUERY` | couvert |
| (nouveau) `readProperties()`, `applyProperties(changes)` | `PropertiesRead` -> `{kind:"snapshot", …}` (forme de `properties-protocol.mjs`) ; `PropertiesApply` -> `{kind:"applied", changed, state}` ou `{kind:"tooLong", field, max, length}` ; sans aperçu DOCX | v2 seulement |
| `insertImages(files, {revision, target})` | `InsertImage` par fichier (octets + texte alternatif, `target` = `SetSelection` avant) ; `onImages({inserted, failed})` idem. Limite : le moteur v2 compte l'image (3 octets plats, enregistrée dans le DOCX) mais **la page ne la dessine pas encore** (aucune commande `drawImage` dans les pages v2) | adapté |
| `exportDocx()` | `Save` DOCX -> `{bytes, noticeCount, pictureCount}` (nombre de `word/media/*` du paquet) | couvert |
| `exportDocument()` | `Save` TXT -> `{revision, bytes, notices}` | couvert |
| `newDocument()` | confirmation, `Open` blank | couvert |
| `openDocument(source, {formats})` | confirmation, puis `pickOpen` + `readTicket` (ou `source()`), `Open` TXT/DOCX selon le format ; renvoie `{kind:"ok", state}` / `cancelled` / `error` | couvert |
| `saveAs()` | `pickSave` -> `Save` (format du dialogue) -> `writeTicket` -> `MarkSaved(revision)` ; `{kind:"ok", state}` / `cancelled` / `error` ; `files.error.queue_full` si occupé | couvert |
| `undo()`, `redo()` | `Undo`, `Redo` ; `EDITOR_BUSY` si occupé ; `RICH_READ_ONLY` en lecture seule | couvert |
| `confirmReplace()` | idem | couvert |
| `dispose()` | idem + client v2 + miroir ; une opération en cours est rejetée `CLIENT_CLOSED`, les tickets d'écriture ouverts sont libérés | couvert |
| (nouveau) `engine` | `"v2"` (`"v1"` n'existe pas sur l'objet v1 : `app.mjs` lit sa propre constante) | v2 seulement |
| (nouveau) `capabilities` | `{search:true, replace:true, statistics:true, textDragDrop:false, preview:false, properties:true, wordSelect:false}` (R7b-2 : recherche, remplacement, statistiques et propriétés levées) : `app.mjs` grise et explique ce qui reste à `false` | v2 seulement |
| (nouveau) `readText(start, end)` | `TextWindow` (tests, presse-papiers, remplace `state.text`) | v2 seulement |
| (nouveau) `selectedText(snapshot)` | texte de la sélection du cliché (fenêtre de l'État, sinon `TextWindow`) pour le menu contextuel | v2 seulement |
| (nouveau) `readOnlyInfo` / `onReadOnly` | `{readOnly, hasRevisions}` pour le bandeau | v2 seulement |
| (nouveau) `makeEditableCopy()` | `MakeEditableCopy` -> `Open` de la copie ; avis exposés | v2 seulement |
| (nouveau) `input`, `setScreenReaderMode(bool)` | poignée `attachTextInput` (bouton du mode lecteur d'écran, Alt+Maj+F1) | v2 seulement |
| (nouveau) `whenLayoutComplete()` | résolu quand la mise en page est terminée | v2 seulement |
| (nouveau) `mirror` | miroir DOM accessible | v2 seulement |

## 4. Événements et rappels

| Rappel v1 | v2 |
|---|---|
| `onState(state, {documentReplaced})` | à chaque `State` appliqué par le cache (frappe, déplacement, annulation, ouverture) ; `documentReplaced: true` après `Open`. Les rafraîchissements de pages seules (`Pages`, pompe de mise en page) ne le déclenchent pas |
| `onError(error)` | `RichClientError` (codes `RICH_*`, `CLIENT_*`) convertie en `EditorError` : `RICH_READ_ONLY` -> `EDITOR_READ_ONLY`, `RICH_STALE_REVISION` -> `EDITOR_STALE_EDIT`, `RICH_IMAGE_INVALID`/`RICH_LIMIT` -> codes d'image existants (`imageMessageKey`) ; les autres gardent leur code (`app.mjs` les affiche par le message générique) |
| `onBusy(bool)` | idem (enregistrement, ouverture, nouveau) |
| `onZoom({zoom, mode})` | idem |
| `onViewport({page, totalPages})` | idem, aussi quand `pageCount` change pendant la mise en page |
| `onImages({inserted, failed})` | idem |
| (nouveau) `onReadOnly({readOnly, hasRevisions})` | quand le drapeau de l'État change |

## 5. Modules consommateurs, simulés un par un

Chaque ligne cite la donnée utilisée et le comportement en v2, chemins d'erreur compris.

| Module | Données lues | v2 |
|---|---|---|
| `app.mjs` (`onState`, `updateControls`) | `state.dirty`, `canUndo`, `canRedo` ; `editor.busy` | identique ; en lecture seule `canUndo`/`canRedo` valent `false` (le moteur refuse) ; boutons Annuler/Rétablir désactivés |
| `app.mjs` ouverture/enregistrement | `openDocument()`, `saveAs()` -> `kind` | voir §6 |
| `app.mjs` mode aperçu (`openDocx`, `closePreview`, `previewActive`) | `previewDocx`, `setSuspended` | en v2 le bouton « Ouvrir DOCX… » appelle `openDocument` filtré DOCX : pas de mode aperçu, pas de bouton « Retour au texte » |
| `app.mjs` insertion d'image | `canEdit`, `insertImages(files)` | idem ; en lecture seule `canEdit` est faux : bouton grisé |
| `app.mjs` raccourci Ctrl+Maj+Espace | `editSelection(" ", getState())` | idem ; refus lecture seule : message d'erreur `status-readonly-refused` |
| `navigation-tools.mjs` | `search`, `statistics`, `replace*`, `setSearchHighlights`, `selectSearchResult`, `state.textBytes`, `state.anchor/focus/revision`, `editor.element` | **créé en v2 depuis R7b-2** (`editor.capabilities.search`) ; mêmes appels qu'en v1 ; sans `state.textBytes` le texte sélectionné qui amorce la requête vient de `readText` ; en lecture seule le volet « Remplacer » n'est pas proposé ; contrat et chemins d'erreur : [us-340-search.md](us-340-search.md) |
| `editor-shell.mjs` | `setNavigationEnabled`, `setPropertiesEnabled`, `setSymbolsEnabled`, `setImageEnabled`, `setZoomEnabled` | ajout `setUnavailable(reason)` : pose `title` et `aria-description` sur les commandes désactivées par le moteur |
| `properties-dialog.mjs` / `properties-protocol.mjs` | octets de l'aperçu DOCX + client v1 | **v2 (R7b-2)** : la boîte lit le document ouvert (`editor.readProperties()`, `editor.applyProperties(changes)`), sans aperçu ; `properties-protocol.mjs` reste celui de v1 ; commande active quand l'éditeur est au repos |
| `zoom-controls.mjs` | `getZoom`, `getZoomMode`, `setZoom`, `fitZoom` | identique |
| `document-status.mjs` | `getViewportState()`, `state.dirty` ; `goToPage` | identique ; avant le premier `State`, `busy` |
| `symbol-dialog.mjs` | `editor.canEdit`, `editor.getState()` (cliché), `editSelection(text, cliché)` | identique ; cliché périmé -> `EDITOR_STALE_SELECTION` ; lecture seule : bouton grisé |
| `editor-context-menu.mjs` | cliché : `revision, anchor, focus, text, canUndo, canRedo, enabled, busy` ; copier/couper lisent `snapshot.text` | v2 : le cliché porte `docLen`, la sélection, et le menu reçoit `hasText` (ajouté) ; copier/couper appellent `selectedText(cliché)` (fenêtre de l'État ou `TextWindow` de 256 Kio au plus ; au-delà `EDITOR_SELECTION_TOO_LARGE`) ; couper = `ReplaceRange` vide ; tout sélectionner = `SetSelection(0, docLen)` |
| `text-input.mjs` | `getSelection`, `getCaret`, `getClipboardSnapshot` (`text`, `revision` BigInt, sélection) | v2 : instantané construit sur la fenêtre de texte de l'État, sinon sur la dernière `TextWindow` préchargée pour la sélection ; absent -> `onError(EDITOR_CLIPBOARD_UNAVAILABLE)` (jamais une copie vide silencieuse) |
| `mouse-selection.mjs` | `selectPoint(values)` -> `{anchor, focus}` entiers | v2 : `HitTest` `setCaret` ou `extend` (le moteur calcule l'affinité) ; unités mot et paragraphe (double et triple clic) **absentes** : traitées comme un clic (`capabilities.wordSelect=false`) |
| `text-drag-drop.mjs` | `probePoint`, `onDrop`, `onFilesDrop`, `getSelectionSnapshot` (avec `text` entier) | **non branché en v2** : glisser-déposer de texte absent (le texte déposé est refusé par `EDITOR_UNSUPPORTED_V2`, la sélection ne se déplace pas) ; dépôt de fichiers image : géré directement par `rich-editor.mjs` (`HitTest` probe puis `InsertImage`) |
| `french-spacing.mjs` | cliché `{text, anchor, focus}` après l'insertion | v2 : fenêtre de l'État (2 Kio autour du curseur), décalages ramenés à des positions plates |
| `image-insert.mjs` | `insertImageFiles(files, {insertBytes, onError})` | identique (`insertBytes` = `InsertImage`) ; le magasin JS `imageCache` ne concerne que v1 |
| `document-mirror.mjs` | `getAccessibleWindow(start, end, {signal})`, `update({revision, docLen, selection, visible})` | v2 seulement ; fenêtre fournie par `accessibleWindow` du client **après conversion** (`mirrorWindow` : le codec nomme les rôles et genres d'objets, le miroir attend leurs numéros ; l'écart a été trouvé par la suite navigateur) ; `update` après chaque `State` ; `visible` = positions plates du haut et du bas de la fenêtre, obtenues par deux `HitTest` de sonde 150 ms après le dernier défilement |
| tests (`editor-test-adapter.mjs`) | `insertTextCommand`, `setSelectionCommand` (octets ATED v1) | côté v2 : trames v2 (`InsertText`, `SetSelection`) |

## 6. Ouverture, enregistrement, lecture seule, images (côté interface)

- **Ouvrir** : `host.pickOpen({title, filters:[txt, docx, docm, dotx]})` -> `{ticket, format, name}`, `host.readTicket(ticket, {copy:true})` -> octets, `Open` (`txt`/profil TXT ; `docx|docm|dotx`/profil document). Annulation du dialogue : `cancelled`. Octets plus grands que `RICH_LIMITS.maxOpenBytes`, ou ticket refusé : `{kind:"error", key}` avec la clé du pont.
- **Enregistrer sous** : `host.pickSave({title, filters, suggestedName})` -> `{ticket, format}` ; `Save` avec `now_ms`, `user`, `version` -> `SaveData` ; `host.writeTicket(ticket, bytes)` ; puis `MarkSaved(revision)`. Échec d'écriture : le ticket est libéré, le document reste modifié. Document modifié entre `Save` et `MarkSaved` : `RICH_STALE_REVISION` -> `EDITOR_STALE_SAVE` (message `status-stale-save`). Fermeture pendant l'enregistrement : le client rejette `CLIENT_CLOSED`, le ticket est libéré, aucun `MarkSaved`.
- **Avis d'enregistrement** (`notices`) : exposés dans le résultat (`notices`) ; `app.mjs` n'affiche pas encore de liste (R7b).
- **DOCX à révisions ou commentaires** : le moteur ouvre en lecture seule (`readOnly` + `hasRevisions`). Bandeau `role="status"` : « Document en lecture seule — modifications acceptées affichées. » avec le bouton « Créer une copie modifiable » (`MakeEditableCopy` -> `Open` de la copie, état éditable, titre conservé). Refus `RICH_REVISIONS_UNSUPPORTED` : message `status-copy-unsupported`. Pendant la lecture seule : saisie, collage, images, annuler et rétablir sont désactivés ou refusés (`EDITOR_READ_ONLY`, message `status-readonly-refused`) ; déplacement, sélection, copie, enregistrement restent permis.
- **Images du magasin JS v1** : sans objet en v2. Migration d'un document v1 ouvert en v2 : les images insérées en v1 vivent dans un magasin JS indexé par un U+FFFC privé et ne sont pas exportées dans le TXT ; une session v1 ne se « bascule » pas vers v2. L'ouverture d'un DOCX produit par v1 (`exportDocx`) en v2 relit ses `word/media/*` comme objets ancrés (moteur). Une réinsertion par `InsertImage` à la bascule n'est pas faite : **limite documentée**, à traiter en R7b si nécessaire.

## 7. Fonctions désactivées en v2 (liste)

1. ~~Rechercher, Remplacer, Statistiques~~ : levé par R7b-2 (2026-10-18), commandes du ruban et Ctrl+F/Ctrl+H actives.
2. ~~Propriétés du document~~ : levé par R7b-2, la boîte lit le document ouvert.
3. Aperçu DOCX en lecture seule distinct (remplacé par l'ouverture dans l'éditeur).
4. Glisser-déposer de texte (interne et externe) ; le dépôt d'images fonctionne.
5. Sélection du mot et du paragraphe à la souris (double et triple clic).
6. Affichage des images insérées (le moteur v2 ne les dessine pas encore).
7. Texte entier de l'état (`state.text`), géométrie de sélection (`selectionRects`).

## 8. Écarts connus de R7a (à lever en R7b ou par le protocole)

1. **Sélection invisible à l'écran** (voir §3) : exposer `selection_rects` (commande ou champ d'`State`).
2. Images insérées : comptées, enregistrées, lues par le miroir, **non dessinées** (pas de `drawImage` dans les pages v2) ; la suite d'images v2 saute géométrie et pixels et le dit.
3. Double et triple clic : un clic simple (le protocole n'a pas de test de position par mot ou paragraphe).
4. Glisser-déposer de texte : refusé avec `EDITOR_UNSUPPORTED_V2` ; le dépôt de fichiers image fonctionne.
5. ~~Recherche, remplacement, statistiques, propriétés~~ : levé par R7b-2 (commandes `0x0130-0x0136`). Restent : en-têtes et pieds non cherchés (hors texte plat), Tout remplacer limité à 5000 occurrences, lignes et pages des statistiques provisoires tant que la mise en page n'est pas terminée, propriétés d'un document non DOCX écrites seulement dans un DOCX (la boîte le dit).
6. Encodage : un fichier TXT n'est jamais refusé pour UTF-8 invalide (le moteur détecte l'encodage ; v1 répondait `EDITOR_INVALID_UTF8`).
7. Aperçu de composition IME : largeur estimée à 7 px par caractère comme en v1 (la géométrie viendra du moteur en R7b).
8. Presse-papiers : copie d'une sélection d'au plus 256 Kio ; au-delà `EDITOR_SELECTION_TOO_LARGE` (message `status-selection-too-large`) ; le texte d'une sélection hors de la fenêtre de l'État est demandé dès que la sélection change, une copie immédiate peut donc répondre `EDITOR_CLIPBOARD_UNAVAILABLE` (réessayer).
9. `initialize()` ouvre un document vierge (en v1 il relisait l'état) ; `previewDocx` rejette.
10. Avis d'enregistrement (`notices`) renvoyés par `saveAs`, `exportDocument` et `makeEditableCopy` mais non affichés par l'interface.
11. Un `Open` refusé par le moteur (`RICH_OPEN_FAILED`) garde le document précédent côté moteur : le client se resynchronise (`Resync`) au lieu de rester vide (correctif de `rich-editor-client.mjs`, testé).
