# US-340 — Rechercher du texte (partie bibliothèque)

Module `atelier_text_model::search` : un seul moteur, utilisable sur le document riche (`RichDocument`, US-200) et sur le texte brut (`PlainTextDocument`, US-120). Le volet Ctrl+F, le surlignage et les raccourcis sont désormais raccordés au TXT ; voir [la livraison UI](us-340-343-navigation-ui.md). Le raccordement au document riche reste ouvert. Aucune dépendance externe ; réutilise `atelier-unicode` (graphèmes et mots UAX #29, `nfc`, `nfd`, `to_case_folded`).

## API

- `trait ParagraphSource { type Id; fn paragraphs(&self) -> Vec<SearchParagraph<'_, Id>> }` : liste des paragraphes dans l'ordre de lecture (identifiant, zone, indice de section, texte). Implémenté pour `RichDocument` (`Id = NodeId`), `PlainTextDocument` et `TextLines<'a>(&str)` (`Id = LineId`). Le moteur ne connaît que ce trait : l'algorithme n'est écrit qu'une fois.
- `SearchOptions { match_case, whole_word, ignore_accents, match_nbsp, context_graphemes }` (défaut : tout à `false`, 30 graphèmes de contexte, `0` désactive les extraits).
- `Searcher::new(&source, requête, options)` : capture les paragraphes (références, pas de copie du texte) et compile la requête.
  - `iter()` : itérateur **paresseux** (`Matches`), un paragraphe analysé à la fois.
  - `find_all(max) -> SearchResults { items, truncated }` : plafond configurable (`DEFAULT_MAX_RESULTS = 10_000` proposé) ; `truncated` dit s'il y en avait davantage. `count(max) -> (n, coupé)` sans construire d'extraits. `SearchResults::index_of(&occ)` donne l'indice (« 3 sur 17 » = indice + 1).
  - `next_from(&Position) -> Option<Step>` : première occurrence dont le début est **≥** la position ; `previous_before(&Position)` : dernière occurrence dont le début est **<** la position. `Step { occurrence, wrapped }` : `wrapped` vaut `true` quand la recherche a bouclé en fin (ou début) de document, pour afficher « recherche reprise au début ». `None` s'il n'y a aucune occurrence. Un paragraphe inconnu vaut début de document (suivant) ou fin (précédent).
  - `paragraph_index(id)`, `is_empty_query()`.
- `Occurrence<Id> { paragraph, paragraph_index, section, zone, range, excerpt }` : `range` en **octets UTF-8 du texte d'origine du paragraphe** ; `Zone` ∈ `Body`, `Table`, `Header`, `Footer`. Pour le texte brut, `Occurrence<LineId>::global_range()` rend la plage dans tout le texte ; `TextLines::position(décalage_global)` convertit un curseur en `Position`.
- `Excerpt { text, start, end, truncated_before, truncated_after }` : N graphèmes avant et après, bornés au paragraphe ; `start..end` situe l'occurrence dans `text` (octets) pour la mettre en gras dans la liste du volet.

## Sémantique des options

La comparaison se fait sur des **clés** : chaque graphème du texte (et de la requête, d'abord mise en NFC) est transformé en NFD ; puis, si `match_case` est faux, repli de casse complet (`to_case_folded`) suivi d'un NFD ; si `ignore_accents`, suppression des caractères de catégorie `Mn` ; si `match_nbsp` est faux, U+00A0 et U+202F deviennent U+0020. Les clés sont concaténées et la requête est cherchée dans cette chaîne (KMP).

- **Casse** (`match_case = false`, défaut) : repli complet Unicode, donc « STRASSE » trouve « straße » et inversement, « fi » trouve la ligature « ﬁ », « i̇ » trouve « İ ». Avec `match_case`, seule la normalisation canonique s'applique : « ß » ne trouve pas « ss ».
- **Mot entier** : début **et** fin de l'occurrence doivent être des frontières de mots UAX #29 du texte d'origine. L'apostrophe entre lettres ne coupe pas (« eau » n'est pas trouvé dans « l'eau », « l'eau » l'est), le trait d'union coupe (« trait » est trouvé dans « trait-d'union », « union » non car « d'union » est un mot). Une requête réduite à une espace est acceptée si ses deux bouts sont des frontières.
- **Accents** : `ignore_accents = false` : « e » ne trouve pas « é », ni « e » + U+0301 ; les formes composée et décomposée se trouvent mutuellement. `true` : « eleve » trouve « élève » (composé ou décomposé) et réciproquement. Seules les marques `Mn` après décomposition sont ignorées (donc « ø » ≠ « o », les voyelles indiennes `Mc` sont conservées).
- **Espaces insécables** : par défaut (`match_nbsp = false`) U+0020, U+00A0 et U+202F sont équivalents, dans la requête comme dans le texte. Avec l'option, ils sont distincts. Les autres espaces (U+2007, etc.) restent distincts.
- Les options se combinent librement (tests croisés dans `tests/search.rs`).

## Plages exactes

Une occurrence démarre et finit toujours sur une frontière de graphème du texte d'origine : un graphème est atomique pour la comparaison (une occurrence ne peut pas se terminer au milieu de « ß », de « ﬁ », de « e » + accent ou d'une séquence émoji ZWJ). Pour chaque graphème dont la clé est non vide, on retient la position de sa clé dans la chaîne repliée et ses octets d'origine ; une occurrence repliée n'est acceptée que si elle commence au début d'une clé et finit à la fin d'une clé. C'est ce tableau qui traduit position repliée → position d'origine malgré les changements de longueur (ß → ss, ﬁ → fi, İ → i̇, marques supprimées).

Cas limite : un graphème de clé vide (marque seule en début de paragraphe avec `ignore_accents`) n'est jamais début d'une occurrence, et n'est pas absorbé en fin ; une requête dont la clé est vide ne trouve rien. Les marques combinantes de classe non nulle qui ouvriraient un graphème (après un caractère de contrôle) ne sont pas réordonnées avec ce qui précède.

## Portée et ordre

- Ordre de lecture : par section, **en-têtes**, **corps**, **pieds** ; dans le corps, tableaux ligne par ligne et cellule par cellule (tableaux imbriqués inclus, pile explicite sans récursion). Les cellules fusionnées ne sont lues qu'une fois. Blocs opaques ignorés.
- `Zone::Table` : cellule d'un tableau du corps, à toute profondeur. Un tableau placé dans un en-tête ou un pied est signalé `Header` / `Footer` (la région l'emporte).
- **Champs** : le texte de résultat fait partie du texte du paragraphe et est donc cherchable ; le code (`Field::instruction`) ne l'est jamais. Objets ancrés, signets, liens : sans effet (le texte n'est pas modifié).
- **Pas de franchissement de paragraphe en V1** : une occurrence tient dans un seul paragraphe ; on ne peut pas chercher « fin de ligne + début de la suivante ». En texte brut, les paragraphes sont les lignes (LF, CRLF, CR isolé) ; les fins de ligne n'appartiennent à aucun paragraphe.
- Notes de bas de page, commentaires, zones de texte : hors périmètre.

## Occurrences qui se chevauchent

Règle de Word : pas de chevauchement. L'occurrence la plus à gauche est retenue, puis la recherche reprend à sa fin : « aa » dans « aaa » donne 1 occurrence, dans « aaaa » deux ; « aba » dans « ababa » donne la plage 0..3 seulement. Un candidat refusé (frontière de graphème, mot entier) ne consomme pas de texte.

## Complexité et limites

Par paragraphe : construction des clés O(n), KMP O(n + m), frontières de mots calculées seulement si `whole_word` et seulement quand un candidat a passé le contrôle des graphèmes. Mémoire O(taille du paragraphe) (clés + une entrée par graphème), jamais du document. `iter()` est paresseux ; `next_from` / `previous_before` s'arrêtent au premier paragraphe qui convient. Pas de comportement quadratique : « a…a » dans « a…a » (200 000), suite de 200 000 marques combinantes, requête presque identique partout sont testés. Limites connues : `Searcher` capture la liste des paragraphes à sa création, il faut en recréer un après chaque modification du document ; `paragraph_index` est linéaire dans le nombre de paragraphes ; pas de règles propres à une langue (turc, lituanien) ni de repli NFKC ; coupures de graphèmes calculées avec la table Unicode 17 d'`atelier-unicode`.

## Pour US-341 (Remplacer)

Une occurrence donne l'identifiant du paragraphe et la plage d'octets exacte, sur frontières de graphèmes. La mise en forme du texte trouvé s'obtient par `RichDocument::paragraph(id)?.attrs_at(range.start)`. Pour « remplacer tout » en une transaction : parcourir `find_all` à l'envers (derniers décalages d'abord) pour que les plages restent valides, ou recréer un `Searcher` après chaque remplacement.

## Preuves

- `crates/text-model/tests/search.rs` (18 tests, résultats calculés à la main) : chaque option et leurs combinaisons, ß/ss dans les deux sens, ligature, NFC/NFD croisés, accent combinant et frontière de graphème, İ, espaces insécables, mot entier (apostrophe, trait d'union), émoji, ZWJ, drapeaux, texte et requête vides, début et fin de paragraphe, non-chevauchement, extraits, plafond, navigation et bouclage, document riche à deux sections avec en-têtes, pied, tableau imbriqué et champ.
- `crates/text-model/tests/search_property.rs` : 30 000 cas à graine fixe, alphabet piégeux, options aléatoires ; vérifie (1) frontières de graphèmes et bornes, (2) clé du texte de la plage = clé de la requête, (3) égalité avec une recherche naïve quadratique écrite dans le test, (4) suivant / précédent parcourent exactement la liste, bouclage compris.
- `crates/text-model/tests/search_perf.rs` : entrées pathologiques et document de 300 pages (1,5 Mo). Mesure indicative en release : voir le rapport de livraison (de l'ordre de 40 à 55 ms pour toutes les occurrences, 0,5 ms pour un « suivant »).

## À brancher par l'intégrateur

1. **Volet Ctrl+F** : créer un `Searcher` à chaque changement de requête, d'option ou de document ; liste = `find_all(DEFAULT_MAX_RESULTS)` avec `excerpt` (afficher « plus de 10 000 » si `truncated`) ; compteur « i sur n » = `index_of` + 1 sur `items.len()`.
2. **Surlignage** : toutes les occurrences visibles via `(paragraph, range)` ; l'occurrence courante dans une autre couleur ; ne pas surligner pendant l'édition sans recalcul.
3. **Raccourcis** : Ctrl+F ouvre le volet avec la sélection comme requête ; Entrée = `next_from(position après la sélection courante)` ; Maj+Entrée = `previous_before(début de la sélection courante)` ; si `wrapped`, afficher l'indication de bouclage ; Échap ferme et rend le focus.
4. Convertir les positions des curseurs du modèle riche en `Position { paragraph: NodeId, offset }`, et, pour le texte brut, utiliser `TextLines(text).position(décalage_global)` et `global_range()`.

## Protocole de l'éditeur riche (R7b-2)

L'éditeur v2 offre recherche, remplacement, statistiques et propriétés par des commandes ATED v2 ([enveloppe et erreurs](us-203-rich-editor-protocol.md)). Tranche R7b-2 : opcodes de commande `0x0130` à `0x0136`, de réponse `0x8120` à `0x8126`, codes d'erreur 40 à 44 (les plages `0x0137-0x013F`, `0x8127-0x812F` et 45-49 restent libres). Code : `crates/core-wasm/src/rich_protocol/find.rs` (codec), `rich_editor/find.rs` (session), `web/rich-editor-protocol.mjs` (codec JS), `web/rich-editor.mjs` (mêmes méthodes publiques que `createEditor`). Vecteurs figés : `tests/rich-editor-find-vectors.txt` (écrits par les tests Rust, `ATELIER_REGEN_VECTORS=1`, relus par `node tests/rich-editor-protocol.mjs` et `node tests/rich-editor-find.mjs`, qui rejoue aussi les scénarios sur le vrai WASM).

**Positions.** Octets UTF-8 du texte plat de v2 : un objet, un saut ou un symbole compte 3 octets (U+FFFC), les paragraphes sont séparés par `\n`, cellules de tableau comprises, en-têtes et pieds exclus (donc **non cherchés** en v2). Une occurrence tient dans un paragraphe ; elle ne franchit jamais un objet, un saut ou un symbole (le motif ne contient pas U+FFFC, sauf requête explicite de la recherche). Côté JS, le codec rend les positions et révisions en `BigInt` ; `rich-editor.mjs` les rend en `Number` comme `state.anchor` (un document tient dans 2^53).

**Options.** Les quatre de v1, mêmes bits : octet `flags` = bit 0 `matchCase`, 1 `wholeWord`, 2 `ignoreAccents`, 3 `matchNbsp` ; autres bits à 0 (`RICH_INVALID_BODY`). Forme JS : `{ matchCase, wholeWord, ignoreAccents, matchNbsp }` (booléens, clé absente = faux ; clé inconnue refusée). **Absents de v1, donc de v2 :** caractères génériques et direction (suivant et précédent se calculent côté client sur la liste, comme en v1).

### Commandes

| Opcode | Nom | Corps (little-endian) | Réponse |
|---:|---|---|---|
| `0x0130` | Search | `revision u64`, `flags u8`, `0 u8[3]`, `len u32` (0 à 1024), requête UTF-8 | SearchResult |
| `0x0131` | Statistics | vide | Statistics |
| `0x0132` | SearchRects | `revision u64`, `first_page u32`, `page_count u32` (1 à 16), `count u32` (1 à 1000), `count` x (`start u64`, `end u64`) | SearchRects |
| `0x0133` | Replace | `revision u64`, `flags u8`, `0 u8[3]`, `start u64`, `end u64`, `qlen u32` (1 à 1024), requête, `rlen u32` (0 à 1 Mio), remplacement | Replaced |
| `0x0134` | ReplaceAll | `revision u64`, `flags u8`, `0 u8[3]`, `qlen u32` (1 à 1024), requête, `rlen u32`, remplacement | Replaced |
| `0x0135` | PropertiesRead | vide | Properties |
| `0x0136` | PropertiesApply | 11 x (`present u8` 0/1, `len u32` jusqu'à 64 Kio, UTF-8 ; `len` nul si absent) dans l'ordre titre, sujet, auteur, responsable, société, catégorie, mots-clés, commentaires, base des liens, état, langue | PropertiesApplied |

Formes JS d'une commande (`rich.command(...)`) : `{ type: "search", revision: BigInt, flags: {…}, query: string }`, `{ type: "statistics" }`, `{ type: "searchRects", revision, firstPage, pageCount, ranges: [{ start: BigInt, end: BigInt }] }`, `{ type: "replace", revision, flags, start: BigInt, end: BigInt, query, replacement }`, `{ type: "replaceAll", revision, flags, query, replacement }`, `{ type: "propertiesRead" }`, `{ type: "propertiesApply", changes: { title?: string, subject?, author?, manager?, company?, category?, keywords?, comments?, hyperlinkBase?, contentStatus?, language? } }` (clé absente = inchangé, texte vide = propriété supprimée).

Règles. `revision` doit être la révision courante (`RICH_STALE_REVISION` sinon, aucune modification). Une requête vide est permise pour `Search` (aucune occurrence) et refusée par `Replace` et `ReplaceAll` (`RICH_EMPTY_QUERY`) ; une requête de plus de 1024 octets est `RICH_QUERY_TOO_LARGE` ; U+FFFC dans la requête d'un remplacement ou dans le texte de remplacement est `RICH_INVALID_TEXT` (le texte de remplacement suit les règles de `InsertText`, `\n` autorisé).

### Réponses

| Opcode | Nom | Corps | Forme JS décodée |
|---:|---|---|---|
| `0x8120` | SearchResult | `revision u64`, `total u32`, `flags u8` (bit 0 `truncated`, bit 1 `total_capped`), `0 u8[3]`, `count u32` (au plus 1000 et au plus `total`), `count` x (`start u64`, `end u64`, `len u32` (au plus 4096), `ex_start u32`, `ex_end u32`, `eflags u8` (bit 0 avant, bit 1 après), `0 u8[3]`, texte de l'extrait) | `{ type: "searchResult", revision: BigInt, total: number, truncated: boolean, totalCapped: boolean, items: [{ start: BigInt, end: BigInt, excerpt: { text: string, start: number, end: number, truncatedBefore: boolean, truncatedAfter: boolean } }] }` |
| `0x8121` | Statistics | `revision u64`, `anchor u64`, `focus u64`, `layout_complete u8`, `0 u8[3]`, document puis sélection : 6 x `u32` (mots, caractères, caractères avec espaces, paragraphes, lignes, pages) | `{ type: "statistics", revision, anchor: BigInt, focus: BigInt, layoutComplete: boolean, document: { words, characters, charactersWithSpaces, paragraphs, lines, pages }, selection: { … } }` (nombres) |
| `0x8122` | SearchRects | `revision u64`, `count u32` (au plus 8192), `count` x (`item u32`, `page u32`, `x`, `y`, `width`, `height` f64) | `{ type: "searchRects", revision, rects: [{ item: number, page: number, x: number, y: number, width: number, height: number }] }` (points de page ; `item` = indice dans la requête, croissant) |
| `0x8123` | Replaced | `count u32`, puis un corps de `State` (`0x8100`) | `{ type: "replaced", count: number, state: { type: "state", … } }` |
| `0x8125` | Properties | `revision u64`, 11 x (`len u32`, UTF-8) valeurs, 11 x `u32` limites en caractères, 3 x date (`kind u8` 0 absente / 1 valide / 2 illisible, `len u32`, texte), 5 x texte facultatif (`present u8`, `len u32`, texte : modifié par, révision, application, version, modèle), durée (`present u8`, `u64`), 6 x nombre facultatif (pages, mots, caractères, avec espaces, lignes, paragraphes), `notices u32` | `{ type: "properties", revision, snapshot: { editable: { title, … }, limits: { title: number, … }, created, modified, lastPrinted: null \| { valid: boolean, text: string }, lastModifiedBy, revision, application, appVersion, template: string \| null, totalTime: number \| null, statistics: { pages, words, characters, charactersWithSpaces, lines, paragraphs: number \| null }, noticeCount: number } }` |
| `0x8126` | PropertiesApplied | `status u8` (0 appliqué, 1 trop long), `0 u8`, `changed u16`, `field u8`, `0 u8[3]`, `max u32`, `len u32`, puis un corps de `State` | `{ type: "propertiesApplied", status: "applied" \| "tooLong", changed: number, field: string \| null, max: number, length: number, state }` |

### Erreurs

| Code | Nom | Quand |
|---:|---|---|
| 40 | `RICH_EMPTY_QUERY` | requête vide pour un remplacement |
| 41 | `RICH_STALE_MATCH` | `Replace` : la plage n'est pas la sélection, ou n'est pas une occurrence de la requête avec ces options |
| 42 | `RICH_QUERY_TOO_LARGE` | requête de plus de 1024 octets |
| 43 | `RICH_PROPERTIES_FAILED` | paquet des propriétés illisible ou impossible à créer |
| 44 | `RICH_TOO_MANY_MATCHES` | `ReplaceAll` aurait plus de 5000 occurrences à remplacer (une étape d'annulation en tient 5000) |

Réutilisés : `RICH_STALE_REVISION` (16), `RICH_READ_ONLY` (21 : remplacement et propriétés d'un document en lecture seule ; recherche, statistiques et lecture des propriétés restent permises), `RICH_POSITION_OUT_OF_RANGE`, `RICH_POSITION_NOT_BOUNDARY`, `RICH_PAGE_RANGE`, `RICH_INVALID_RANGE`, `RICH_LIMIT` (19 : plus de 64 Mio de texte de paragraphes à réécrire), `RICH_UNKNOWN_DOCUMENT` (document fermé). L'interface les renomme comme en v1 : `EDITOR_STALE_QUERY` (révision périmée d'une requête), `EDITOR_STALE_MATCH`, `EDITOR_EMPTY_QUERY`, `MESSAGE_TOO_LARGE`, `EDITOR_READ_ONLY`, `EDITOR_TOO_MANY_MATCHES`.

### Sémantique

- **Search** : au plus 10 000 occurrences comptées (`total` ; `totalCapped` s'il y en a davantage), les 1000 premières renvoyées (`truncated` s'il y en a davantage que renvoyées) avec un extrait de 30 graphèmes de chaque côté borné au paragraphe (4096 octets au plus ; au-delà, l'occurrence seule, rognée). `ex_start..ex_end` situe l'occurrence dans le texte de l'extrait, en octets, sur des frontières de scalaires. L'extrait garde U+FFFC s'il traverse un objet.
- **SearchRects** : rectangles des plages demandées sur les pages `first_page .. first_page + page_count` (les pages visibles). Une plage qui coupe plusieurs lignes ou pages donne plusieurs rectangles, de même `item`. Une page ou une plage que la mise en page n'a pas atteinte ne donne rien. Réponse bornée à 8192 rectangles.
- **Mise en forme du remplacement** : le texte inséré prend les attributs (`CharAttrs`, style de caractère compris) du premier caractère remplacé, pas ceux du caractère qui précède ; une plage vide garde l'ancienne règle.
- **Replace** : remplace l'occurrence `start..end` (qui doit être la sélection) en une étape d'annulation ; le curseur est replié après le texte inséré. Texte identique : aucune modification (révision inchangée), `count` vaut 1.
- **ReplaceAll** : toutes les occurrences, en **une étape d'annulation et une seule révision** (`RichDocument::replace_ranges` : chaque occurrence est remplacée sur place, la mise en forme du texte entre elles est intacte ; une annulation ou un rétablissement prend le groupe entier). Les occurrences déjà égales au remplacement sont comptées et laissées. Aucune occurrence : `count` 0, rien ne change. Curseur replié après la dernière occurrence. Tout ou rien : un échec défait ce qui était fait.
- **Statistics** : mots, caractères, caractères avec espaces, paragraphes du corps et des cellules (en-têtes et pieds exclus, objets et sauts sans caractère) ; lignes et pages de la mise en page. Pour la sélection : les règles de `statistics::selection` (un paragraphe pris entier suit la règle du paragraphe, une partie compte pour un paragraphe si elle contient du texte), lignes et pages touchées. La commande poursuit la mise en page de 1024 paragraphes au plus ; `layout_complete` faux dit que lignes et pages sont provisoires (la pompe du client termine, l'interface les affiche « — » en attendant).
- **PropertiesRead / PropertiesApply** : DOCX importé : le paquet importé (`core.xml` et `app.xml` sont écrits par `Save` avec la date, l'utilisateur, la version et les compteurs) ; document neuf ou TXT : un paquet du profil par défaut, recopié dans le DOCX à chaque `Save` DOCX. Les compteurs en lecture seule sont ceux du document vivant (pages et lignes seulement une fois la mise en page terminée). Un changement de valeur rend le document modifié (`State.modified`) sans changer la révision du texte, jusqu'au `MarkSaved` qui suit un `Save` fait après le changement. Valeur trop longue : `status` 1, `field`, `max`, `len` en caractères, rien n'est appliqué.

### Consommateurs simulés (chemins d'erreur compris)

| Module | Donnée utilisée | Comportement en v2 |
|---|---|---|
| `search-pane.mjs` | `items[].excerpt.{text,start,end}` (octets UTF-8 de l'extrait, relus par `TextEncoder`), `items.length`, `truncated` | identique à v1 ; `total` n'est pas affiché (le compte affiché reste celui de la liste, 1000 au plus, avec la mention de troncature) |
| `navigation-tools.mjs` | `editor.search(q, options)` -> `{ revision (BigInt), items: [{ start, end }] (Number) }` comparé à `state.revision` et à `state.anchor/focus` (Number) ; `setSearchHighlights`, `selectSearchResult`, `replaceCurrent`, `replaceAll`, `statistics` | mêmes appels et mêmes formes qu'en v1. Révision périmée : `EDITOR_STALE_QUERY`, le résultat est ignoré et la recherche repart au prochain état. Document fermé ou remplacé (`documentReplaced`) : `invalidateSearch` annule la requête et efface les surlignages. Resynchronisation (`Resync`) : sans effet sur la recherche (aucune révision n'a changé) ; une réponse de remplacement perdue force la resynchronisation du client, l'état suivant porte la nouvelle révision et relance la recherche. **Lecture seule** : la recherche fonctionne, le volet « Remplacer » n'est pas proposé (Ctrl+H n'ouvre que la recherche), `replaceAll` appelé directement rejette `EDITOR_READ_ONLY`. Sans `state.textBytes` (v2), le texte sélectionné qui amorce la requête est lu par `editor.readText(début, début + 4 Kio)` : le volet s'ouvre une réponse du moteur plus tard |
| `replace-controls.mjs` | `canReplace`, `canReplaceAll`, `count`, `error` donnés par `navigation-tools` | `count` = `count` de `Replaced` ; erreur : le code de l'`EditorError` ; en lecture seule `canReplace` et `canReplaceAll` sont faux |
| `statistics-controls.mjs` | `result.document`, `result.selection` : `{ words, characters, charactersWithSpaces, paragraphs, lines, pages }` (nombres), `revision`, `anchor`, `focus` (Number) comparés à l'état | identique ; `layoutComplete` faux : lignes et pages passent à `null` (« — ») jusqu'à l'état suivant ; document fermé pendant la requête : `EDITOR_CLOSED`, ignoré tant que le module est désactivé |
| `properties-dialog.mjs` | `load()` -> `{ editable, limits, created, modified, lastPrinted, lastModifiedBy, revision, application, statistics }` ; `apply(changes)` -> `{ kind: "ok" }` ou `{ kind: "tooLong", field, max, length }` | `editor.readProperties()` rend `{ kind: "snapshot", … }` (forme de `properties-protocol.mjs`), `editor.applyProperties(changes)` rend `{ kind: "applied", changed, state }` ou `{ kind: "tooLong", field, max, length }` ; `app.mjs` convertit en `{ kind: "ok" }`. Lecture seule : `RICH_READ_ONLY` à l'application, la boîte affiche `properties-applyError` et reste ouverte |
| `editor-shell.mjs`, `app.mjs` | `editor.capabilities.{search, replace, statistics, properties}` | les commandes ne sont plus grisées (le mécanisme `aria-disabled` de R7a reste pour toute capacité à `false`) ; Propriétés exige un éditeur au repos |
| Surlignage (`rich-editor.mjs`) | `setSearchHighlights(result, index)`, révision, pages visibles | la liste de plages est retenue ; pour les pages visibles, une commande `SearchRects` demande les rectangles (une fois par page, refaite quand la mise en page se termine) et le Canvas peint toutes les occurrences visibles, la courante d'une autre couleur ; un résultat d'une autre révision est refusé (`false`), une modification efface tout |

### Limites connues de la tranche

- Recherche et remplacement ignorent les en-têtes et pieds (hors texte plat) ; v1 n'en avait pas non plus.
- Remplacer tout au-delà de 5000 occurrences est refusé (`RICH_TOO_MANY_MATCHES`) ; v1 n'avait pas de plafond. Le travail de réécriture est borné à 64 Mio de texte de paragraphes.
- Le document redevient non modifié quand l'historique revient exactement au point enregistré (position dans l'historique mémorisée au `MarkSaved`, `RichDocument::history_position`, comme en v1) ; la révision, elle, ne cesse de croître.
- Les propriétés d'un document qui n'est pas un DOCX ne sont écrites que si l'utilisateur l'enregistre en DOCX : la boîte le dit (`properties-docxOnly`) et un `Save` TXT ne les marque pas comme enregistrées.
- Le surlignage ne couvre que les pages visibles ; « suivant » fait défiler jusqu'à l'occurrence par le curseur.
