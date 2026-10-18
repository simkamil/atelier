# US-217 — Édition riche dans le moteur (tranche R3a)

Session de l'éditeur riche : `crates/core-wasm/src/rich_editor.rs` (sous-module `rich_editor/accessible.rs`, tests `rich_editor/tests.rs`). Elle exécute les commandes ATED v2 de [la spécification](us-203-rich-editor-protocol.md) sur un `RichDocument` ([US-204](us-204-flat-model.md)) et un `PageCache` ([US-216](us-216-incremental-layout.md)). L'éditeur v1 (`editor.rs`), `RichSession` et les tables annexes restent en place jusqu'à R8.

## Routage

`lib.rs` envoie à `RichEditor::respond` tout message `Editor` dont la charge commence par `ATED` suivi de la version 2 et compte au moins 8 octets (`rich_editor::is_rich`). Les trames plus courtes, y compris celles de 6 ou 7 octets qui annoncent la version 2, restent à la v1. `editor.rs` n'est pas modifié. La session v2 est un `static` distinct de la session v1 : elle n'est jamais clonée par commande (la v1 clone l'état, le publie si la réponse tient dans le registre ABI). Un seul document actif : `Open` avec un identifiant remplace le document ; une commande avec un autre identifiant répond `RICH_UNKNOWN_DOCUMENT` ; un en-tête illisible ou l'identifiant 0 répondent par l'échec externe `Failed` dont la charge est le nom ASCII de l'erreur.

## Commandes

| Commande | Comportement |
|---|---|
| `Open` | Vierge (profil document : document par défaut et son catalogue de styles ; profil TXT : document vide), TXT par `plain_io::open_text` (BOM retiré, fins de ligne normalisées, un paragraphe par ligne, 2^20 lignes au plus), DOCX par `import_preview_docx` (limites de l'aperçu : 1024 paragraphes, 128 Kio de texte ; R3b les lève). Le catalogue et le thème importés sont donnés au document (`set_styles`). Réponse : `State` de révision 0, avec les 16 premières pages et l'invalidation de tout le cache client. Un `Open` refusé laisse le document actif intact. |
| `InsertText` | Remplace la sélection. Sélection après : celle du résultat d'édition (curseur après le texte). |
| `ReplaceRange` | `check_range`, puis `replace_range` (extrémités sur des positions valides). |
| `Delete` | Sélection non vide : supprimée entière. Sinon graphème (`prev/next_grapheme`, un `\n` ou un objet est un seul groupe) ou mot (`prev/next_word`, comme Ctrl+Retour arrière et Ctrl+Suppr de Word). Rien à supprimer aux bords : `State` inchangé, sans révision. |
| `Move` | Directions 0 à 13 de la v1. Gauche et droite sans extension replient une sélection sur son début ou sa fin. Haut, bas, page haut et page bas gardent l'abscisse préférée (`move_vertical`, `move_page`) ; début et fin de ligne utilisent `line_edges`, la fin d'une ligne coupée prend l'affinité amont ; paragraphe haut et bas vont au début du paragraphe suivant, du paragraphe courant ou du précédent. Avec extension, l'ancre reste. |
| `SetSelection` | Les deux extrémités sont ramenées en arrière sur une position valide ; au-delà de la fin : `POSITION_OUT_OF_RANGE`. |
| `HitTest` | `PageCache::hit_test`. Sonde : `HitResult`, aucun changement. Placer ou étendre : `State`, affinité calculée par le moteur. |
| `Undo`, `Redo` | Rien à défaire : `State` inchangé. La sélection est celle de l'`EditOutcome` (la plage touchée, dans l'ordre : la v1 restaurait aussi le sens ancre/foyer). |
| `TextWindow`, `Pages` | Comme la spécification ; une page non prête donne `RICH_NOT_READY`. |
| `MarkSaved` | Révision courante : `modified` retombe ; sinon `RICH_STALE_REVISION`. `modified` vaut « révision différente de la dernière sauvegarde » : un annuler qui revient au texte enregistré garde `modified` (la révision a augmenté). |
| `Layout` | `PageCache::step(budget)` puis `State`. |
| `AccessibleWindow` | Voir la spécification (opcode ajouté par US-217). |

Toute commande d'édition, de déplacement, de sélection ou de test de position qui répond un `State` exécute aussi `step` avec `DEFAULT_LAYOUT_BUDGET` (16 paragraphes). Le `State` porte la révision (de 0 à `Open`, le modèle part de sa propre révision), la sélection et son affinité, les drapeaux, le caret, les pages modifiées (`split_page_delta` sur la fusion des deltas de `apply_edit` et de `step`, au plus 16 pages ; les autres en invalidation) et une fenêtre de texte de 2 Kio de chaque côté du foyer (sauf `Layout` et `MarkSaved`). Une page annoncée modifiée et qui ne se peint pas encore est transformée en invalidation.

Atomicité : toutes les vérifications précèdent la première modification ; un échec du modèle est annulé par le modèle lui-même (`replace_range`). Une commande refusée ne change rien (testé commande par commande).

## Limites connues

- Une position dans la queue non encore mise en page donne `RICH_NOT_READY` pour les déplacements et le test de position ; le client relance `Layout`.
- L'étiquette de liste est le compteur décimal suivi d'un point : le modèle n'a pas de définitions de numérotation. Le texte alternatif d'un objet est vide. Niveau de titre déduit de l'identifiant de style.
- Les modifications sont appliquées sur place : si le registre ABI refuse une réponse (état `2`), l'édition reste appliquée, contrairement à la v1 qui publiait un candidat. Le client se resynchronise par `Layout` (la révision est dans chaque réponse).
- Pas de déposer-glisser (`DropText`), pas d'export (R3b, US-218), pas de révision attendue sur `ReplaceRange`.
- DOCX : limites de l'aperçu (1024 paragraphes, 128 Kio) tant que R3b ne les change pas.

## Tests

- `cargo test -p atelier-core-wasm --lib rich_editor` : 20 tests de la session par le vrai codec v2 (saisie, multi-paragraphes, graphème et mot, objets, sauts et symboles en ligne, annuler et rétablir, `MarkSaved`, déplacements et extension, test de position, mise en page jusqu'à « terminée » sur 900 paragraphes, DOCX, erreurs sans changement d'état, `Open` refusé, trames invalides, 400 éditions ASCII contre un texte miroir, 600 commandes et 2000 trames aléatoires sans panique).
- `node tests/rich-editor-abi.mjs [moteur.wasm]` : les scénarios de `tests/editor-abi.mjs` rejoués contre le vrai WASM en trames v2, avec le tableau scénario v1 → v2. Six scénarios v1 sans équivalent (`DropText` quatre fois, `Export` octet pour octet, retour arrière d'une réponse refusée par l'ABI, révision attendue d'un remplacement) : le critère « les scénarios de `tests/editor-abi.mjs` passent en v2 » reste décoché.
- Vecteurs gelés `tests/rich-editor-protocol-vectors.txt` régénérés pour `AccessibleWindow` (`node tests/rich-editor-protocol.mjs`).
