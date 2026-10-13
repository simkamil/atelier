# US-291 — Signets et Atteindre (partie bibliothèque)

Module `atelier_text_model::navigation`, au-dessus du modèle riche (US-200). Il ne fait que **construire des transactions** `RichOp` existantes (`InsertInline`, `ModifyInline`, `RemoveInline`) et **résoudre des positions** : aucune opération nouvelle, aucune dépendance, aucun accès à la mise en page. Les dialogues, l'affichage des crochets et le défilement sont à l'intégrateur. Parcours du document : celui de la recherche (US-340, `ParagraphSource for RichDocument`) : par section, en-têtes, corps (tableaux cellule par cellule), pieds. Tout est linéaire en la taille du document.

## API

Signets (`bookmarks.rs`) :
- `insert_bookmark(&mut doc, nom, début, fin, NameMode) -> Result<BookmarkEdit, BookmarkError>`, `rename_bookmark(&doc, ancien, nouveau, mode)`, `delete_bookmark(&doc, nom)`. `BookmarkEdit { transaction, links }` : la transaction s'applique par `RichDocument::apply` ou `History` ; son inverse (rendu par `apply`) annule l'action et rend le document identique.
- `lookup_bookmark(&doc, nom)` (avec `ambiguous`), `list_bookmarks(&doc, BookmarkOrder::{Name, Position}, inclure_masqués) -> Vec<BookmarkInfo>` (`name`, `paragraph`, `start`, `end`, `zone`, `section`, `paragraph_index`, `hidden`) ; `find_bookmark(&doc, nom)`.
- Noms (`name.rs`) : `validate_bookmark_name`, `is_hidden_name`, `name_key` (clé d'unicité), `MAX_NAME_CHARS = 40`.

Atteindre (`goto.rs`, `pages.rs`) :
- `goto(&doc, &dyn PageMap, position_courante, GotoKind, saisie) -> Result<GotoOutcome, GotoError>` ; `step(…, Direction::{Next, Previous})` pour les boutons ; `parse_amount(kind, saisie)`.
- `GotoKind` : `Page`, `Section`, `Line`, `Bookmark`, `Table`, `Object` (image et tout objet ancré), `Heading` (titre).
- `goto(…, from: impl Into<GotoOrigin>, …)` accepte un `TextPosition` ou un `GotoOrigin` ; variantes `goto_with_styles` / `step_with_styles` avec `&StyleCatalog`.
- `GotoOutcome::Reached(Landing)` ou `NoElements`. `Landing { position: TextPosition, selection_end, zone, section, index, count, clamp }` : `selection_end` est la fin de plage à sélectionner pour un signet non vide ; `index/count` permettent d'afficher « 2 sur 5 » ; `clamp` vaut `Some(ToLast | ToFirst)` si la cible dépassait une extrémité ; `ambiguous` signale un signet trouvé à la casse près parmi plusieurs.
- `trait PageMap` (1-based) : `page_count`, `page_start(n)`, `page_of(pos)`, `line_count`, `line_start(n)`, `line_of(pos)`. `FixedPageMap::new(&doc, débuts_de_pages, débuts_de_lignes)` en est une version factice pour les tests.

## Règles de nom et certitude par rapport à Word

| Règle | Certitude |
|---|---|
| Commence par une lettre ; lettres, chiffres, soulignés ; pas d'espace | Certaine (documentation Microsoft) |
| Unicité insensible à la casse (`name_key` : NFC + repli de casse, **accents conservés** : `e` ≠ `é`) | Certaine pour la casse ; accents : hypothèse |
| 40 caractères au plus | Certaine pour 40. **Hypothèse** : on compte des caractères Unicode après NFC ; Word compte vraisemblablement des unités UTF-16, donc un caractère hors BMP (émoji, idéogramme rare) vaut 2 chez lui : un nom accepté ici peut être refusé par Word |
| Nom saisi en forme décomposée (`e` + accent combinant) | **Normalisé en NFC** avant validation et stockage (`normalize_name`) : `é` et `é` sont le même nom. Choix, pas une règle de Word connue |
| « Lettre » = `char::is_alphabetic`, « chiffre » = `is_alphanumeric` (lettres accentuées acceptées) | Hypothèse |
| Nom commençant par `_` : masqué (`_Toc…`, `_GoBack`) ; conservé, exclu de la liste par défaut, atteignable et supprimable par son nom | Certaine ; seul l'interdit de la saisie utilisateur est une déduction |
| Un utilisateur ne peut pas créer un nom en `_` (`NameMode::User`) ; la bibliothèque le peut (`NameMode::Internal`, nom en `_` obligatoire) | Choix |

Toute règle non certaine ci-dessus est une hypothèse. Chaque règle tient dans `name.rs` (`validate_bookmark_name`, `name_key`, `is_hidden_name`). Le modèle n'interdit que deux noms **strictement** égaux ; la casse est vérifiée ici, sur la liste complète (masqués compris).

## Choix de comportement

- **Plage sur plusieurs paragraphes** : erreur typée `RangeSpansParagraphs` (jamais de signet raccourci en silence). Bornes inversées : échangées. Débordement ou milieu de graphème : `Model(OffsetOutOfBounds | NotGraphemeBoundary)` à la construction, document intact.
- **Résolution d'un nom** (insertion, renommage, suppression, Atteindre) : la correspondance **exacte** l'emporte toujours ; sans exacte, on prend la correspondance à la casse près la première dans l'ordre du document et on le signale (`BookmarkEdit::ambiguous`, `Landing::ambiguous`, `lookup_bookmark(..).ambiguous`). Cela ne concerne que les documents importés où `Titre` et `titre` coexistent (le modèle les accepte) ; les liens suivent la même règle de résolution.
- **Insérer avec un nom existant (exact, sinon casse ignorée)** : le signet est **déplacé** (même identifiant, orthographe d'origine conservée, donc les liens restent valides) ; une ou deux opérations selon que le paragraphe change.
- **Renommer** : même transaction que la mise à jour des liens internes (`LinkTarget::Bookmark`, casse ignorée) ; `links` rend les liens retouchés. Un **autre** signet qui porte le nom, exactement ou à la casse près : `NameTaken`, détecté avant que le modèle ne refuse ; changement de casse seule : permis ; même nom : transaction vide.
- **Supprimer** : les liens internes sont **laissés tels quels** (comme Word, ils ne mènent plus nulle part) et listés dans `links` pour avertir l'utilisateur ; l'annulation les rend de nouveau valides.
- **Tri par nom** : clé = NFD, repli de casse, marques `Mn` retirées ; à clé égale, ordre de lecture. **Tri par position** : ordre de lecture, puis début, fin.

## Syntaxe d'Atteindre

Saisie : `12` (absolu), `+3` / `-2` (relatif), vide = suivant (`+1`). Espaces tolérés autour et après le signe (`+ 3`), pas dans un nombre (`1 2` est invalide). Pour les pages seulement : `p3s2` = page 3 de la section 2 (espaces et casse tolérés ; la page est comptée depuis la première page de la section et bornée à la dernière page de celle-ci). Erreurs : `ParseError::InvalidNumber` (tout le reste, y compris `+`, `3.5`, chiffres non ASCII) et `TooLarge` (> 10⁹) ; jamais de panique. Pour `Bookmark`, la saisie est **d'abord cherchée comme nom** (exact, sinon casse ignorée ; masqués atteignables) : un nom importé comme `1abc` ou `12` est donc atteignable, et prime sur le nombre. Sans signet de ce nom : une saisie commençant par une lettre ou `_` donne `BookmarkNotFound`, le reste est lu comme un nombre dans la liste des signets visibles par position.

Numérotation (corps uniquement, pour les types calculés) : sections = début du premier paragraphe du corps ; tableaux = tous les tableaux du corps, imbriqués compris, au premier paragraphe de leur première cellule ; objets = objets ancrés du corps et de ses tableaux, position exacte ; titres = paragraphes dont le style est un titre (`atelier.heading.1..9`, `Heading1`, `Titre 2`…, `is_heading_style`). **Sans catalogue** (`goto`, `step`) seul le style direct du paragraphe est lu : approximation, un style basé sur « Titre 1 » n'est pas vu. **Avec catalogue** (`goto_with_styles`, `step_with_styles`, `is_heading_style_in(style, &StyleCatalog)`) la chaîne `based_on` est remontée sur 64 niveaux au plus (cycle ou style absent : pas un titre). Le modèle ne porte aucun niveau hiérarchique direct sur le paragraphe (`ParaProps` n'en a pas) : seuls les styles définissent un titre. Signets : toutes zones.

## Bornes, suivant et précédent

Une position `cur` a `c` débuts d'éléments à son niveau ou avant. **Suivant** = élément `c+1`. **Précédent** = dernier début *strictement* avant `cur` : depuis le milieu d'un tableau ou d'une section, « précédent » mène donc au **début de l'élément courant** (hypothèse, calquée sur le précédent de la recherche), depuis son début à l'élément d'avant. `+n` répète suivant n fois, `-n` répète précédent n fois, `+0` = élément courant, `n` absolu. Au-delà de la fin : dernier élément, `clamp = ToLast` ; avant le début ou `0` : premier, `ToFirst`. Aucun élément du type (document sans tableau, page de mise en page vide…) : `NoElements`. Position courante hors document : `UnknownPosition`.

**Débuts confondus.** Plusieurs éléments peuvent commencer au même endroit (tableau imbriqué au début de la première cellule de son parent, objets ancrés au même décalage, signets au même point). Avec la position seule, le curseur compte comme posé sur le **dernier** d'entre eux. Pour que « suivant » et « précédent » les parcourent un à un, passer `Landing::origin()` du résultat précédent (`GotoOrigin { position, rank }`) comme point de départ : le rang n'est pris en compte que si l'élément de ce rang commence bien à la position (sinon ignoré). `ToLast` et `ToFirst` ne sont annoncés qu'aux vraies extrémités ; répété depuis l'élément 1, « suivant » visite exactement les `count` éléments (testé pour chaque type).

**Table de pages incohérente.** Aucune panique quelle que soit la `PageMap` (zéro page, `page_of` hors `1..=page_count`, début absent ou hors document, comptes énormes) : les rangs sont ramenés dans l'intervalle, tous les calculs sont faits en `i128` ou en opérations bornées, et un début de page absent ou étranger au document donne `GotoError::InconsistentPageMap`.

## À brancher par l'intégrateur

- Dialogues **Signet** (nom, liste triée nom/position, case « signets masqués », Ajouter/Supprimer/Atteindre, avertissement avec `links` à la suppression et au renommage) et **Atteindre** (type, saisie, boutons Suivant/Précédent, message « aucun » et indication de borne). Raccourcis proposés : Ctrl+Maj+F5 (Signet) et Ctrl+G / F5 (Atteindre), à confirmer.
- Appliquer `BookmarkEdit::transaction` via `History` (une action annulable).
- Après `Landing` : placer le curseur en `position` (étendre la sélection à `selection_end`), faire défiler selon `zone`/`section`.
- Crochets de signet facultatifs : dessiner `[` et `]` aux `start`/`end` de `list_bookmarks(…, true or false)`.
- Implémenter `PageMap` sur la mise en page (`atelier-text-layout`) : débuts de pages et de lignes, `page_of`/`line_of` ; les positions rendues doivent être des paragraphes du document.
- Clés de catalogue proposées : `goto.title`, `goto.kind.{page,section,line,bookmark,table,object,heading}`, `goto.next`, `goto.previous`, `goto.none`, `goto.clamped.last`, `goto.clamped.first`, `goto.error.invalid`, `goto.error.too-large`, `goto.error.bookmark-not-found`, `bookmark.title`, `bookmark.name`, `bookmark.sort.name`, `bookmark.sort.position`, `bookmark.show-hidden`, `bookmark.error.{empty,first-char,char,too-long,range,taken,not-found}`, `bookmark.links-updated`, `bookmark.links-dangling`.

## Besoins constatés dans le modèle

Aucun blocage. Utile : un accès public direct « liens vers un signet » et un index des signets insensible à la casse (ici recalculés en parcours linéaire) ; une entrée de transaction « déplacer un signet » (ici `RemoveInline` + `InsertInline` avec le même identifiant).
