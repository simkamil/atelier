# US-340 — Rechercher du texte (partie bibliothèque)

Module `atelier_text_model::search` : un seul moteur, utilisable sur le document riche (`RichDocument`, US-200) et sur le texte brut (`PlainTextDocument`, US-120). L'interface (volet Ctrl+F, surlignage, raccourcis) n'est pas incluse : elle se branche sur l'API ci-dessous. Aucune dépendance externe ; réutilise `atelier-unicode` (graphèmes et mots UAX #29, `nfc`, `nfd`, `to_case_folded`).

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
