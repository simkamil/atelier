<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-203 — Surbrillance de la sélection, images dessinées, double et triple clic (tranche R7b-1)

Trois écarts de l'éditeur v2 de l'interface (`docs/r7-ui-parity.md` §8, points 1 à 3) sont comblés. Les messages sont ceux de ce document ; les opcodes de la tranche sont **commandes `0x0120` à `0x012F`, réponses `0x8110` à `0x811F`, codes d'erreur 30 à 39** (la tranche R7b-2 : recherche, remplacement, statistiques, propriétés, prend le reste). Code : `crates/core-wasm/src/rich_protocol/sel.rs` (codec), `rich_editor/sel.rs` (session), `web/rich-editor-protocol.mjs` (miroir JS), `web/rich-editor-client.mjs`, `web/rich-image-cache.mjs`, `web/rich-editor.mjs`.

## Commandes

| Opcode | Nom | Corps | Réponse |
|---:|---|---|---|
| `0x0120` | SelectionRects | `first u32`, `count u32` (1 à 16) | SelectionRects ; aucun changement d'état |
| `0x0121` | ImageBitmap | `object_id u64` | ImageBitmap ; aucun changement d'état |
| `0x0122` | SelectUnit | `position u64`, `unit u8` (0 mot, 1 paragraphe), `extend u8` (drapeau), `0 u16` | State |

Contrôles du corps, dans l'ordre des champs : `SelectionRects` refuse `count == 0` (`RICH_INVALID_BODY`), `count > 16` (`RICH_TOO_MANY_PAGES`) et `first >= 2^20` (`RICH_PAGE_RANGE`) ; `SelectUnit` refuse une unité inconnue, un drapeau autre que 0 ou 1 et un réservé non nul (`RICH_INVALID_BODY`). Règles du moteur : voir ci-dessous.

## Réponses

| Opcode | Nom | Corps |
|---:|---|---|
| `0x8110` | SelectionRects | `revision u64`, `count u32` (au plus 4096), `count` x (`page u32`, `x f64`, `y f64`, `width f64`, `height f64`) |
| `0x8111` | ImageBitmap | `object_id u64`, `width u32`, `height u32`, `len u32`, `len` octets RGBA |

Le décodeur refuse une page `>= 2^20`, une largeur ou une hauteur négative ou non finie (rectangles), une largeur ou une hauteur de bitmap hors de 1 à 1600, et `len != width * height * 4`.

Types JS décodés (`decodeRichReply(...).response`, contrat fixé par l'intégrateur) :

```js
{ type: "selectionRects", revision: 4n, rects: [{ page: 0, x: 72, y: 90.5, width: 120.25, height: 13 }] }
{ type: "imageBitmap", objectId: 9n, width: 2, height: 1, rgba: Uint8Array /* vue dans la trame, 8 octets */ }
// commandes : { type: "selectionRects", first: 0, count: 3 }, { type: "imageBitmap", objectId: 9n },
//             { type: "selectUnit", position: 12n, unit: "word" | "paragraph", extend: false }
```

## Surbrillance de la sélection

`SelectionRects` renvoie, pour la sélection de la session (`anchor`, `focus` : l'ordre ne compte pas) et les pages `first .. first + count` (bornées au nombre de pages), les rectangles de `PageCache::selection_rects` : un par ligne et par page, en points de page (mêmes coordonnées que la liste d'affichage). Une sélection vide donne zéro rectangle. `revision` est celle du document au moment de la réponse. Un `first` au-delà du nombre de pages répond `RICH_PAGE_RANGE`. La mise en page n'est avancée que jusqu'à la page demandée : une sélection qui dépasse les paragraphes déjà posés est coupée à la dernière ligne de la page (`selection_rects` borne ses extrémités à la fenêtre de la page), un début au-delà ne sélectionne rien ici ; une page non encore posée répond `RICH_NOT_READY` (le client redemande quand le cache change). **Limite** : au plus 4096 rectangles par réponse (de l'ordre de 16 pages denses) ; le reste de la sélection n'est pas surligné, le client demande moins de pages.

Client (`rich-editor-client.mjs`, option `onSelectionRects({revision, rects})`) :

- après chaque `State` appliqué (pas un `State` périmé) dont la sélection n'est pas vide, une seule requête, pour les pages visibles (`setVisibleRange`, au plus 16), lancée quand la file de commandes est vide ; elle est **annulée** (signal) quand une commande est mise en file ou qu'un nouveau `State` arrive, et redemandée si aucun `State` ne suit ;
- une réponse dont la révision n'est pas celle du dernier `State` est ignorée ; le changement des pages visibles redemande la surbrillance ;
- une sélection vide publie tout de suite un tableau vide, sans requête ; ouvrir ou fermer un document publie `{revision: null, rects: []}` ;
- `RICH_NOT_READY` et `RICH_PAGE_RANGE` ne sont pas des erreurs (pages pas encore posées) : nouvelle tentative au prochain changement du cache.

`rich-editor.mjs` convertit les rectangles en coordonnées de fenêtre (zoom, défilement) et les passe à `CanvasRenderer.setEditorOverlay({selectionRects})`, même forme que la v1 ; ils sont redessinés à chaque défilement ou zoom.

## Images dessinées

Mise en page (`crates/core-wasm/src/rich_layout/prepare.rs`, `crates/text-layout` : `layout_paragraph_boxes`). Un objet ancré de genre `atelier:image` est une **boîte en ligne** posée sur la ligne de base : U+FFFC dans le texte posé, trois octets comme dans le texte plat, donc les positions plates et les correspondances (`Mark`) ne changent pas ; la boîte prend sa largeur, remonte l'ascendante de la ligne à sa hauteur et garde la descendante du texte (comme `inline_object::line_box` en v1) ; elle passe à la ligne comme un mot, seule sur sa ligne si elle dépasse. Taille : pixels x 0,75 pt (96 ppp), largeur et hauteur échangées pour l'orientation EXIF 5 à 8, réduite proportionnellement à la zone de texte de la page (largeur) et à sa hauteur moins 3 pt de descendante, jamais agrandie (`inline_object::natural_size` et `fit`, les fonctions de la v1). Les autres objets ancrés restent sans place. Le curseur, les rectangles de sélection et le test de position prennent la hauteur de la ligne ; un clic dans l'image donne la position avant ou après elle.

Liste d'affichage. L'image est une commande `DrawImage` (opcode ATDL 4) : `image_id` = **identité du nœud de l'objet** (`NodeId::value()`, `u64`, stable tant que l'objet existe, différente pour deux objets même identiques), rectangle = `x` du glyphe de la boîte, bas sur la ligne de base. Le peintre de pages l'écrit à l'endroit de la boîte dans l'ordre de peinture. Une image inconnue du moteur (supprimée) n'est plus référencée par les pages.

`ImageBitmap { object_id }` renvoie le bitmap d'affichage de l'image (celui de l'opcode 29 de la v1 : orientation EXIF appliquée, réduit à 1600 pixels au plus sur le grand côté par moyenne de surface, RGBA droit, de haut en bas). Erreurs : `RICH_UNKNOWN_OBJECT` (30) si l'identité n'est celle d'aucune image posée du document (objet supprimé par un annulé, identité inventée, objet d'un autre genre), `RICH_IMAGE_INVALID` si les octets ne se décodent plus, `RICH_LIMIT` au-delà des bornes d'US-270.

Client. `client.setWantedImages(ids)` (ids `BigInt` des `drawImage` des pages affichées) : les images que `hasImage` ne détient pas sont demandées une à une, en arrière-plan, quand la file est vide, et remises par `onImageBitmap({objectId, width, height, rgba})` (copie des pixels) ; une image disparue (`RICH_UNKNOWN_OBJECT`) n'est pas une erreur. `web/rich-image-cache.mjs` (`createRichImageCache`) transforme les pixels en source dessinable (`createImageBitmap`) et les garde dans une borne de **128 Mio** (dernier utilisé d'abord évincé, jamais une image que les pages affichées dessinent) ; il est vidé au chargement d'un document et au changement d'époque de l'atlas. `rich-editor.mjs` passe `resolveImage` au `CanvasRenderer` : une image dont les pixels n'ont pas encore été reçus est un rectangle gris clair de sa taille, remplacé et redessiné à leur arrivée.

## Double et triple clic

`SelectUnit` : `position` est une position plate valide (une sonde `HitTest` la donne ; `RICH_POSITION_OUT_OF_RANGE`, `RICH_POSITION_NOT_BOUNDARY` sinon, sélection inchangée). Mot : `FlatIndex::word_at` (au bout du document, le dernier mot ; un objet est un mot à lui seul ; document vide : curseur). Paragraphe : du début du paragraphe au début du suivant, donc avec la coupure qui le termine (comme en v1), sauf le dernier. La sélection est dirigée (ancre au début, foyer à la fin), sans affinité amont, et la réponse est un `State` avec le curseur.

`extend` agrandit la sélection par unités, comme Word : la session retient l'étendue de l'unité du clic qui a commencé le geste (`unit_base`) ; avec `extend`, la sélection va de cette étendue à l'unité sous `position`, ancrée du côté que l'unité cible n'atteint pas (cible avant : ancre à la fin de l'étendue ; après : au début ; qui la recoupe : l'étendue seule). Sans geste en cours (Maj + double clic), l'étendue est la sélection courante ; un curseur dans l'unité cible donne l'unité. Toute autre commande de sélection ou d'édition oublie l'étendue. Aucune vérification de lecture seule : sélectionner n'est pas éditer.

Interface : `mouse-selection.mjs` (inchangé : il compte les clics, `unit` = `character`, `word`, `paragraph`) ; côté v2, `rich-editor.mjs` fait `HitTest` en sonde puis `SelectUnit` avec `values.extend` (premier appel : Maj ; ensuite : vrai pendant le glisser). la clé `wordSelect` a disparu de `capabilities` (la liste ne contient que ce que v2 n'offre pas).

## Erreurs ajoutées

| Code | Nom | Quand |
|---:|---|---|
| 30 | `RICH_UNKNOWN_OBJECT` | `ImageBitmap` d'une identité qui n'est pas une image du document |

Les codes 31 à 39 sont réservés à la tranche R7b-1.

## Vecteurs et tests

`tests/rich-editor-selection-vectors.txt` (même format que `rich-editor-save-vectors.txt` : 5 requêtes, 4 réponses, 11 requêtes et 13 réponses refusées), régénéré par `ATELIER_REGEN_VECTORS=1 cargo test -p atelier-core-wasm --lib rich_protocol` et vérifié par `crates/core-wasm/src/rich_protocol/sel_tests.rs` et `node tests/rich-editor-selection.mjs`. Autres tests : `cargo test -p atelier-core-wasm --test rich_selection` (rectangles sur plusieurs pages, sélection vide, image dans la page, EXIF, mot et paragraphe aux bornes, objets en ligne), `-p atelier-text-layout --test inline_box`, `-p atelier-core-wasm --test rich_layout` (sélection au-delà du posé) ; `node tests/rich-editor-selection.mjs` (client : révision périmée, annulation, une requête par `State`, cache d'images borné ; moteur réel) ; navigateur : `node tests/run-rich-editor-ui-browser.mjs` (surbrillance au pixel, image dessinée, double et triple clic) et `node tests/run-image-insert-browser.mjs --engine v2` (géométrie et pixels des images).
