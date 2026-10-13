# US-291 — Signets et Atteindre (partie bibliothèque)

Module `atelier_text_model::navigation`, au-dessus du modèle riche (US-200). Il ne fait que **construire des transactions** `RichOp` existantes (`InsertInline`, `ModifyInline`, `RemoveInline`) et **résoudre des positions** : aucune opération nouvelle, aucune dépendance, aucun accès à la mise en page. Les dialogues, l'affichage des crochets et le défilement sont à l'intégrateur. Parcours du document : celui de la recherche (US-340, `ParagraphSource for RichDocument`) : par section, en-têtes, corps (tableaux cellule par cellule), pieds. Tout est linéaire en la taille du document.

## API

Signets (`bookmarks.rs`) :
- `insert_bookmark(&mut doc, nom, début, fin, NameMode) -> Result<BookmarkEdit, BookmarkError>`, `rename_bookmark(&doc, ancien, nouveau, mode)`, `delete_bookmark(&doc, nom)`. `BookmarkEdit { transaction, links }` : la transaction s'applique par `RichDocument::apply` ou `History` ; son inverse (rendu par `apply`) annule l'action et rend le document identique.
- `list_bookmarks(&doc, BookmarkOrder::{Name, Position}, inclure_masqués) -> Vec<BookmarkInfo>` (`name`, `paragraph`, `start`, `end`, `zone`, `section`, `paragraph_index`, `hidden`) ; `find_bookmark(&doc, nom)`.
- Noms (`name.rs`) : `validate_bookmark_name`, `is_hidden_name`, `name_key` (clé d'unicité), `MAX_NAME_CHARS = 40`.

Atteindre (`goto.rs`, `pages.rs`) :
- `goto(&doc, &dyn PageMap, position_courante, GotoKind, saisie) -> Result<GotoOutcome, GotoError>` ; `step(…, Direction::{Next, Previous})` pour les boutons ; `parse_amount(kind, saisie)`.
- `GotoKind` : `Page`, `Section`, `Line`, `Bookmark`, `Table`, `Object` (image et tout objet ancré), `Heading` (titre).
- `GotoOutcome::Reached(Landing)` ou `NoElements`. `Landing { position: TextPosition, selection_end, zone, section, index, count, clamp }` : `selection_end` est la fin de plage à sélectionner pour un signet non vide ; `index/count` permettent d'afficher « 2 sur 5 » ; `clamp` vaut `Some(ToLast | ToFirst)` si la cible dépassait une extrémité.
- `trait PageMap` (1-based) : `page_count`, `page_start(n)`, `page_of(pos)`, `line_count`, `line_start(n)`, `line_of(pos)`. `FixedPageMap::new(&doc, débuts_de_pages, débuts_de_lignes)` en est une version factice pour les tests.

## Règles de nom et certitude par rapport à Word

| Règle | Certitude |
|---|---|
| Commence par une lettre ; lettres, chiffres, soulignés ; pas d'espace | Certaine (documentation Microsoft) |
| Unicité insensible à la casse (`name_key` : NFC + repli de casse, **accents conservés** : `e` ≠ `é`) | Certaine pour la casse ; accents : hypothèse |
| 40 caractères au plus | Certaine pour 40 ; **hypothèse** : on compte des caractères Unicode (Word compte peut-être des unités UTF-16) |
| « Lettre » = `char::is_alphabetic`, « chiffre » = `is_alphanumeric` (lettres accentuées acceptées) | Hypothèse |
| Nom commençant par `_` : masqué (`_Toc…`, `_GoBack`) ; conservé, exclu de la liste par défaut, atteignable et supprimable par son nom | Certaine ; seul l'interdit de la saisie utilisateur est une déduction |
| Un utilisateur ne peut pas créer un nom en `_` (`NameMode::User`) ; la bibliothèque le peut (`NameMode::Internal`, nom en `_` obligatoire) | Choix |

Chaque règle tient dans `name.rs` (`validate_bookmark_name`, `name_key`, `is_hidden_name`). Le modèle n'interdit que deux noms **strictement** égaux ; la casse est vérifiée ici, sur la liste complète (masqués compris).

## Choix de comportement

- **Plage sur plusieurs paragraphes** : erreur typée `RangeSpansParagraphs` (jamais de signet raccourci en silence). Bornes inversées : échangées. Débordement ou milieu de graphème : `Model(OffsetOutOfBounds | NotGraphemeBoundary)` à la construction, document intact.
- **Insérer avec un nom existant (casse ignorée)** : le signet est **déplacé** (même identifiant, orthographe d'origine conservée, donc les liens restent valides) ; une ou deux opérations selon que le paragraphe change.
- **Renommer** : même transaction que la mise à jour des liens internes (`LinkTarget::Bookmark`, casse ignorée) ; `links` rend les liens retouchés. Nom pris par un autre signet : `NameTaken` ; changement de casse seule : permis ; même nom : transaction vide.
- **Supprimer** : les liens internes sont **laissés tels quels** (comme Word, ils ne mènent plus nulle part) et listés dans `links` pour avertir l'utilisateur ; l'annulation les rend de nouveau valides.
- **Tri par nom** : clé = NFD, repli de casse, marques `Mn` retirées ; à clé égale, ordre de lecture. **Tri par position** : ordre de lecture, puis début, fin.

## Syntaxe d'Atteindre

Saisie : `12` (absolu), `+3` / `-2` (relatif), vide = suivant (`+1`). Espaces tolérés autour et après le signe (`+ 3`), pas dans un nombre (`1 2` est invalide). Pour les pages seulement : `p3s2` = page 3 de la section 2 (espaces et casse tolérés ; la page est comptée depuis la première page de la section et bornée à la dernière page de celle-ci). Erreurs : `ParseError::InvalidNumber` (tout le reste, y compris `+`, `3.5`, chiffres non ASCII) et `TooLarge` (> 10⁹) ; jamais de panique. Pour `Bookmark`, une saisie commençant par une lettre ou `_` est un nom (insensible à la casse, masqués atteignables, absent : `BookmarkNotFound`) ; sinon c'est un nombre dans la liste des signets visibles par position.

Numérotation (corps uniquement, pour les types calculés) : sections = début du premier paragraphe du corps ; tableaux = tous les tableaux du corps, imbriqués compris, au premier paragraphe de leur première cellule ; objets = objets ancrés du corps et de ses tableaux, position exacte ; titres = paragraphes dont le style direct est un titre (`atelier.heading.1..9`, `Heading1`, `Titre 2`…, `is_heading_style`, **hypothèse** : le style n'est pas résolu par héritage). Signets : toutes zones.

## Bornes, suivant et précédent

Une position `cur` a `c` débuts d'éléments à son niveau ou avant. **Suivant** = élément `c+1`. **Précédent** = dernier début *strictement* avant `cur` : depuis le milieu d'un tableau ou d'une section, « précédent » mène donc au **début de l'élément courant** (hypothèse, calquée sur le précédent de la recherche), depuis son début à l'élément d'avant. `+n` répète suivant n fois, `-n` répète précédent n fois, `+0` = élément courant, `n` absolu. Au-delà de la fin : dernier élément, `clamp = ToLast` ; avant le début ou `0` : premier, `ToFirst`. Aucun élément du type (document sans tableau, page de mise en page vide…) : `NoElements`. Position courante hors document : `UnknownPosition`.

## À brancher par l'intégrateur

- Dialogues **Signet** (nom, liste triée nom/position, case « signets masqués », Ajouter/Supprimer/Atteindre, avertissement avec `links` à la suppression et au renommage) et **Atteindre** (type, saisie, boutons Suivant/Précédent, message « aucun » et indication de borne). Raccourcis proposés : Ctrl+Maj+F5 (Signet) et Ctrl+G / F5 (Atteindre), à confirmer.
- Appliquer `BookmarkEdit::transaction` via `History` (une action annulable).
- Après `Landing` : placer le curseur en `position` (étendre la sélection à `selection_end`), faire défiler selon `zone`/`section`.
- Crochets de signet facultatifs : dessiner `[` et `]` aux `start`/`end` de `list_bookmarks(…, true or false)`.
- Implémenter `PageMap` sur la mise en page (`atelier-text-layout`) : débuts de pages et de lignes, `page_of`/`line_of` ; les positions rendues doivent être des paragraphes du document.
- Clés de catalogue proposées : `goto.title`, `goto.kind.{page,section,line,bookmark,table,object,heading}`, `goto.next`, `goto.previous`, `goto.none`, `goto.clamped.last`, `goto.clamped.first`, `goto.error.invalid`, `goto.error.too-large`, `goto.error.bookmark-not-found`, `bookmark.title`, `bookmark.name`, `bookmark.sort.name`, `bookmark.sort.position`, `bookmark.show-hidden`, `bookmark.error.{empty,first-char,char,too-long,range,taken,not-found}`, `bookmark.links-updated`, `bookmark.links-dangling`.

## Besoins constatés dans le modèle

Aucun blocage. Utile : un accès public direct « liens vers un signet » et un index des signets insensible à la casse (ici recalculés en parcours linéaire) ; une entrée de transaction « déplacer un signet » (ici `RemoveInline` + `InsertInline` avec le même identifiant).
