<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-216 — Mise en page et rendu incrémentaux de l'éditeur

Tranche R2 du [plan de bascule](rich-editor-migration.md) : `atelier-core-wasm::rich_layout::PageCache` implémente le trait `RichPageCache` figé par [US-203](us-203-rich-editor-protocol.md). Le cache ne possède pas le document : chaque appel qui lit des paragraphes reçoit un `&dyn LayoutContext` (le modèle R1 ; ici, pour les tests, le double `VecParagraphs`). Les positions sont les positions plates de `atelier_text_model::rich::flat`. Aucune signature du contrat n'a changé ; le type concret est `PageCache`, et `PageCache::selection_rects` (méthode propre au type, pas au trait) donne les rectangles de sélection d'une page.

## Périmètre du lot 1 (décision de l'intégrateur)

- Les paragraphes de cellule de tableau sont mis en page comme des paragraphes de corps pleine largeur (mode dégradé) ; les en-têtes et pieds ne sont pas dessinés.
- Une seule géométrie de page pour tout le document : celle de la première section (`page_setup`), en twips arrondis au 1/64 pt le plus proche, comme la mise en page des lignes. Profil TXT : A4, marges de 54 pt, Carlito 11 pt, interligne simple, sans espacement, sans attributs de run (comme `editor-layout.rs`).
- Les éléments en ligne (objets ancrés, sauts, symboles) occupent trois octets plats mais aucune place dans la ligne et ne sont pas dessinés ; les positions juste avant et juste après un élément ont la même géométrie. Les puces et numéros de liste ne sont pas dessinés. Les attributs de paragraphe que le modèle ne porte pas encore (garder avec le suivant, saut de page avant, veuves et orphelines au-delà des défauts) gardent les règles par défaut de la pagination.
- Dégradations qui évitent toute erreur : un caractère qu'aucune police enregistrée ne couvre devient autant de `?` que d'octets (les décalages restent valables) ; un paragraphe qui ne se met pas en page (limites de la mise en page des lignes) devient une ligne vide et toutes ses positions tombent au début de cette ligne ; une erreur interne de pagination ou de dessin met le cache en état « terminé » sans pages (`layout_complete` vrai, requêtes `NotReady`) au lieu de paniquer.

## Mise en page incrémentale

`atelier-text-layout` reçoit trois changements (ajouts demandés par le critère de latence ; les tests existants du crate passent sans modification) :

1. **Prévalidation globale supprimée.** `paginate_recording` validait tous les paragraphes, construisait un préfixe de hauteurs par paragraphe et les chaînes « garder avec le suivant » de tout le document avant d'en paginer un seul. Il lit désormais ses paragraphes par une interface (`ParagraphSource`) et vérifie chaque paragraphe quand le flux l'atteint ; les chaînes sont mémorisées sur les seuls paragraphes regardés. `paginate`, `IncrementalSession` et leurs sorties sont inchangés (`pagination_preflight_work` vaut toujours 0).
2. **`stream::PageStream`** (nouveau) garde les mises en page de paragraphes, la pagination d'un préfixe de ces paragraphes, un point de reprise de la machine par paragraphe et une signature par page. `replace(first, removed, new, limit)` remplace une plage de paragraphes et repagine depuis la page qui contient le changement. Le flux s'arrête dès que la machine de pagination est dans un état que l'ancienne pagination a aussi traversé (même ordonnée dans la page, même page ouverte, même paragraphe précédent non touché par l'édition) : l'ancien suffixe est repris avec ses indices décalés. Quand l'état ne se retrouve pas (une ligne ajoutée pousse toutes les suivantes), il s'arrête après `RunLimit::Pages(2)` pages et deux paragraphes au-delà de l'édition ; `advance(n)` paginera la suite par tranches. Le résultat, une fois rien en attente, est exactement celui de `paginate` sur tout le document (tests de propriété : `crates/text-layout/tests/stream.rs`, 40 graines avec règles simples, 40 avec règles riches, comparaison après chaque remplacement et préfixes intermédiaires).
3. **Choix de la fin de ligne** (`layout_paragraph`) : les candidats de coupure sont testés par une recherche guidée (estimation avec l'avance de la mise en forme du paragraphe entier, vérification du candidat et de son voisin, dichotomie si l'estimation se trompe) au lieu d'une mise en forme du préfixe par candidat. L'hypothèse est celle que le code énonçait déjà (la largeur d'un préfixe ne décroît pas avec le préfixe). Sortie inchangée : 119 514 lignes sur 3 000 mises en page (largeurs de 10 à 480 pt, tabulations, ligatures, crénage, mots plus longs que la ligne), même empreinte avant et après. Coût d'un paragraphe de 574 caractères : 2,68 ms avant, 1,44 ms après.

Dans `PageCache` :

- `new` met en page des paragraphes par paquets de 32 jusqu'à deux pages complètes, puis rend la main ; `step(budget)` met en page puis pagine le reste (budget compté en paragraphes mis en page ou paginés, jamais d'horloge).
- `apply_edit` remet en forme les paragraphes de l'édition (au plus 256, le reste d'une grosse insertion est fini par `step`), repagine comme ci-dessus et rend un `PageDelta` : `changed` ce sont les pages dont la signature (géométrie des lignes et génération de chaque paragraphe) change à leur indice, `invalidated` les pages qui ne sont plus prêtes. Une édition au-delà du préfixe déjà mis en forme ne coûte rien ; `LayoutScope::All` reconstruit tout (changements de section ou de styles) et invalide toutes les pages.
- Tant que la suite n'est pas paginée, `page_count` garde le nombre de pages connu avant l'édition (pas de saut de barre de défilement) ; `page_display` d'une page non prête et les requêtes de position dans la zone non paginée rendent `NotReady`.
- `page_display` écrit la liste ATDL de la page à la demande et en garde les quatre dernières, validées par la signature de la page.

## Géométrie du curseur

Les requêtes construisent un `CaretLayout` sur des paragraphes entiers : celui du curseur (`caret`, `line_edges`), celui-ci et ses voisins (`move_vertical`), ceux d'une page (`hit_test`, `selection_rects`) ; ce n'est pas une validation globale. Les coordonnées sont en points de la page, marges comprises.

- `caret(position, upstream)` : à un retour automatique, `upstream` choisit la fin de la ligne précédente ; ailleurs les deux affinités coïncident.
- `line_edges` : début et fin de la ligne visuelle (la fin d'une ligne coupée est le début de la suivante, avec l'affinité amont pour l'afficher à la fin de la ligne).
- `hit_test(page, x, y)` : position la plus proche de la ligne la plus proche verticalement ; au-delà de la fin d'une ligne coupée, la fin de la ligne avec `upstream` vrai.
- `move_vertical(position, lignes, x)` : ligne visuelle par ligne visuelle, en gardant le x souhaité à travers les lignes courtes ; au-delà de la première ligne, début du document ; au-delà de la dernière, fin du document une fois la mise en page terminée (`NotReady` avant).
- `move_page(position, pages, x)` : même ordonnée sur la page voisine ; au-delà des pages, début ou fin du document.
- `selection_rects(page, début, fin)` : un rectangle par ligne visuelle coupée par la sélection sur cette page (extrémités inversées acceptées).

## Dessin

`rich_layout/rich_paint.rs` contient le dessin extrait de `rich_preview.rs` : `paint_line` (les glyphes d'une ligne placée en commandes `FillPath` et `StrokePath`) et `encode_page`. L'aperçu DOCX les appelle : ses pages sont identiques octet pour octet à celles d'avant l'extraction (empreinte du corpus de référence et tests `rich_preview*` inchangés).

L'éditeur redessine une page par frappe ; `paint_line` construit une `Command` par glyphe que `DisplayList::encode` valide deux fois (environ 5 µs par glyphe, 25 ms par page). `paint_line_fast` fait les mêmes calculs (contour du glyphe converti une fois, à l'origine, puis translaté à l'écriture : mêmes opérations flottantes dans le même ordre) et écrit les commandes ATDL directement dans les octets de la page ; le contour mis en cache est validé une fois par `DisplayPath::new`. Le test `the_editor_writer_is_byte_identical_to_the_preview_painter` compare page par page ses octets à ceux de `paint_line` + `encode_page` (gras, italique, souligné, couleur, tabulations, accents).

## Mesures

Banc hôte (`cargo run --release --locked --offline -p atelier-core-wasm --example rich_layout_bench`), document de 304 pages (4 100 paragraphes de 20 à 100 mots, profil document), frappe d'un caractère à la fois dans le paragraphe du milieu (400 frappes, le paragraphe grossit de 400 caractères), mise à jour = `apply_edit` + liste d'affichage de chaque page modifiée + `caret` :

| Mesure (400 frappes) | moyenne | p50 | p95 | maximum |
|---|---|---|---|---|
| Frappe complète (`apply_edit` + listes des pages modifiées + `caret`) | 2,48 ms | 2,40 ms | 3,05 ms | 4,97 ms |
| dont `apply_edit` (mise en page du paragraphe + pagination) | 1,22 ms | 1,17 ms | 1,62 ms | 2,76 ms |
| dont listes d'affichage des pages modifiées | 1,23 ms | 1,19 ms | 1,48 ms | 3,05 ms |
| 398 frappes qui ne changent pas le flux (arrêt sur page stabilisée) | 2,47 ms | 2,40 ms | 3,00 ms | 4,45 ms |
| 2 frappes qui ajoutent une ligne (suite laissée à `step`) | 4,22 ms | | | 4,97 ms |
| Entrée au milieu du paragraphe (30 essais, scinde puis refusionne) | 2,84 ms | 2,81 ms | 3,19 ms | 3,37 ms |

Compteurs joints : pages parcourues par la pagination (pages recalculées, page ouverte comprise) 2,02 en moyenne, 4 au plus, sur 304 pages (303 reprises du calcul précédent à l'arrêt typique) ; pages renvoyées au client par frappe : 1,01 en moyenne. Le critère « moins de 4 ms » tient en moyenne et au 95e centile ; les frappes qui ajoutent une ligne repaginent deux pages de plus et en redessinent deux, et dépassent 4 ms (jusqu'à 4,97 ms) : le maximum de la série n'est pas sous la barre. La mise en page d'un paragraphe de 574 caractères passe de 2,68 ms à 1,44 ms avec la recherche guidée de fin de ligne, le dessin d'une page de 25 ms à environ 1,1 ms avec l'écriture directe.

La suite non paginée après une ligne ajoutée est achevée par `step` entre deux frappes (hors mesure). Le banc ne mesure ni le modèle (le double reconstruit son index à chaque édition, hors chrono) ni la copie dans la réponse du protocole. Le poste de mesure n'est pas le PC de référence du §6.16 : le critère de campagne reste à US-362.

## Budget WASM

`python tools/package-engine.py` puis `node tests/check-wasm-size.mjs --measure …` et `node tests/check-wasm-size.mjs` : code **1 948 076 octets** (limite 2 111 059, référence 1 919 145), soit +4 026 octets sur les 1 944 050 d'avant. Ce delta sous-estime le coût réel : `lib.rs` n'appelle pas encore `PageCache` (R3a le fera) et l'éditeur de liens retire le code mort. Une sonde temporaire, non commitée, qui appelait les méthodes du trait et `selection_rects` à travers un modèle factice a donné **2 032 826 octets** de code (+88 776 octets sur l'état précédent, 96 % de la limite, 78 233 octets de marge) : c'est l'ordre de grandeur que le cache ajoutera une fois branché, avant le code de l'éditeur lui-même. `data/engine/` n'est pas modifié par cette branche.

## Ce qui n'est pas fait

Sections à géométries différentes, en-têtes et pieds, tableaux (cellules en pleine largeur), éléments en ligne dessinés, listes, comparaison de la pagination à Word (US-308) et latence de bout en bout dans le navigateur (US-219, US-362).
