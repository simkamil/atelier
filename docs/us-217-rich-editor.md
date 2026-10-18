# US-217 — Édition riche dans le moteur (tranche R3a)

Session de l'éditeur riche : `crates/core-wasm/src/rich_editor.rs` (sous-module `rich_editor/accessible.rs`, tests `rich_editor/tests.rs`). Elle exécute les commandes ATED v2 de [la spécification](us-203-rich-editor-protocol.md) sur un `RichDocument` ([US-204](us-204-flat-model.md)) et un `PageCache` ([US-216](us-216-incremental-layout.md)). L'éditeur v1 (`editor.rs`), `RichSession` et les tables annexes restent en place jusqu'à R8.

## Routage

`lib.rs` envoie à `RichEditor::respond` tout message `Editor` dont la charge commence par `ATED` suivi de la version 2 et compte au moins 8 octets (`rich_editor::is_rich`). Les trames plus courtes, y compris celles de 6 ou 7 octets qui annoncent la version 2, restent à la v1. `editor.rs` n'est pas modifié. La session v2 est un `static` distinct de la session v1 : elle n'est jamais clonée par commande (la v1 clone l'état, le publie si la réponse tient dans le registre ABI). Un seul document actif : `Open` avec un identifiant remplace le document ; une commande avec un autre identifiant répond `RICH_UNKNOWN_DOCUMENT` ; un en-tête illisible ou l'identifiant 0 répondent par l'échec externe `Failed` dont la charge est le nom ASCII de l'erreur.

## Commandes

| Commande | Comportement |
|---|---|
| `Open` | Vierge (profil document : document par défaut et son catalogue de styles ; profil TXT : document vide), TXT par `plain_io::open_text` (BOM retiré, fins de ligne normalisées, un paragraphe par ligne, 2^20 lignes au plus), DOCX par `import_docx_for_editor` (`rich_preview.rs` : 262 144 paragraphes, 16 Mio de texte, 1 M runs ; taille d'entrée et limites du paquet de l'aperçu inchangées ; l'aperçu garde ses 1024 paragraphes et 128 Kio ; erreurs codées, sans formatage). Le catalogue et le thème importés sont donnés au document (`set_styles`). Réponse : `State` de révision 0, avec les 16 premières pages et l'invalidation de tout le cache client. Un `Open` refusé laisse le document actif intact. |
| `InsertText` | Remplace la sélection. Sélection après : celle du résultat d'édition (curseur après le texte). |
| `ReplaceRange` | `check_range`, puis `replace_range` (extrémités sur des positions valides). |
| `Delete` | Sélection non vide : supprimée entière. Sinon graphème (`prev/next_grapheme`, un `\n` ou un objet est un seul groupe) ou mot (`prev/next_word`, comme Ctrl+Retour arrière et Ctrl+Suppr de Word). Rien à supprimer aux bords : `State` inchangé, sans révision. |
| `Move` | Directions 0 à 13 de la v1. Gauche et droite sans extension replient une sélection sur son début ou sa fin. Haut, bas, page haut et page bas gardent l'abscisse préférée (`move_vertical`, `move_page`) ; une cible pas encore mise en page est atteinte en mettant la queue en page par blocs de 256 puis 1024 paragraphes, 4096 au plus par commande (voir plus bas) ; début et fin de ligne utilisent `line_edges`, la fin d'une ligne coupée prend l'affinité amont ; paragraphe haut et bas vont au début du paragraphe suivant, du paragraphe courant ou du précédent. Avec extension, l'ancre reste. |
| `SetSelection` | Les deux extrémités sont ramenées en arrière sur une position valide ; au-delà de la fin : `POSITION_OUT_OF_RANGE`. |
| `HitTest` | `PageCache::hit_test`. Sonde : `HitResult`, aucun changement. Placer ou étendre : `State`, affinité calculée par le moteur. |
| `Undo`, `Redo` | Rien à défaire : `State` inchangé. La sélection est celle de l'`EditOutcome` (la plage touchée, dans l'ordre : la v1 restaurait aussi le sens ancre/foyer). |
| `TextWindow`, `Pages` | Comme la spécification ; une page non prête donne `RICH_NOT_READY`. |
| `MarkSaved` | Révision courante : `modified` retombe ; sinon `RICH_STALE_REVISION`. `modified` vaut « révision différente de la dernière sauvegarde » : un annuler qui revient au texte enregistré garde `modified` (la révision a augmenté). |
| `Layout` | `PageCache::step(budget)` puis `State`. |
| `AccessibleWindow` | Voir la spécification (opcode ajouté par US-217). |

Toute commande d'édition, de déplacement, de sélection ou de test de position qui répond un `State` exécute aussi `step` avec `DEFAULT_LAYOUT_BUDGET` (16 paragraphes). Le `State` porte la révision (de 0 à `Open`, le modèle part de sa propre révision), la sélection et son affinité, les drapeaux, le caret, les pages modifiées (`split_page_delta` sur la fusion des deltas de `apply_edit` et de `step`, au plus 16 pages ; les autres en invalidation) et une fenêtre de texte de 2 Kio de chaque côté du foyer (sauf `Layout` et `MarkSaved`). Une page annoncée modifiée et qui ne se peint pas encore est transformée en invalidation.

Atomicité : toutes les vérifications précèdent la première modification ; un échec du modèle est annulé par le modèle lui-même (`replace_range`). Une commande refusée ne change rien (testé commande par commande).

## Pages denses, bornes d'octets

Une liste d'affichage coûte de 500 à 1600 octets par glyphe (mesuré sur le vrai WASM, `tests/rich-editor-abi.mjs` : 836 octets par glyphe sur des minuscules, 1561 sur des lignes de 100 colonnes de lettres à contours riches, 4 215 269 octets pour 2700 caractères sur la première page). Cause : `PageCache::paint_page` peint avec `PaintSink::new(false, 0)`, donc sans atlas de glyphes ; chaque glyphe de la page est un `FillPath` qui recopie son contour complet converti en éléments de chemin (un octet de type et 2 à 6 flottants de 8 octets par élément, de dix à soixante éléments par lettre), alors que l'aperçu DOCX active l'atlas au-delà de 64 Kio de texte. Le format ATDL n'est pas modifié ici : le cache de contours par police et par glyphe (Path2D) relève d'une story à part, liée à US-101. (Mesures d'avant R2b : depuis, l'atlas de glyphes ramène une page dense à 36 octets par glyphe, voir `rich-editor-migration.md`.)

Bornes : une page jusqu'à 16 Mio (`MAX_PAGE_DISPLAY_BYTES`, sous les 26 Mio de `rich_paint.rs`), la somme des listes d'une réponse au plus 8 Mio sauf pour une réponse à une seule page (`MAX_RESPONSE_PAGE_BYTES`, `page_fits`), vérifiées à l'encodage et au décodage (Rust et JS). `State` et `Pages` s'arrêtent à la borne ; les pages écartées d'un `State` passent dans `invalidated` et le client les demande par `Pages` (la première page tient toujours).

## Mise en page à la demande, resynchronisation

- Un déplacement ou un test de position dont la cible n'est pas mise en page appelle `step` jusqu'à pouvoir répondre (`ready`) ; les changements de pages trouvés ainsi sont gardés (`pending`) et portés par le `State` suivant. Budget épuisé : la sélection ne bouge pas et le `State` n'a pas de caret, `layout_complete = 0`, jamais une erreur ; le client envoie `Layout` et recommence. Ctrl+Fin et Ctrl+Début, ainsi que `SetSelection`, mettent la queue en page jusqu'au caret dans la même limite. `RICH_NOT_READY` ne reste que pour `Pages` et pour une sonde `HitTest` qui ne peut pas être placée.
- Si la mise en page a renoncé (`give_up`), la requête qui échoue reconstruit le `PageCache` du document (toutes les pages sont invalidées) au lieu de rester bloquée jusqu'au prochain `Open`.
- Réponse perdue : un échec d'encodage de réponse, `STATUS_LIMIT` ou `EDITOR_RESPONSE_LIMIT` pose `resync` (`RichEditor::mark_resync`, appelé par `lib.rs`) ; le `State` suivant invalide `(0, page_count)` et porte la fenêtre de texte, une fois.

## Performances : sauts lointains (perf-layout-jump)

Aucune commande interactive ne met en page plus de `REACH_BUDGET` = 32 paragraphes (tranches de `REACH_SLICE` = 16), plus les 16 de `finish` : environ 0,17 ms par paragraphe en release sur cette machine, soit 8 ms par commande. Si la cible (caret, déplacement vertical, test de position) n'est pas atteinte, le `State` porte la sélection posée (positions plates valides, aussi pour Ctrl+Fin et Maj+Ctrl+Fin), `caret` absent et `layout_complete = 0` ; la pompe `Layout` du client continue (64 paragraphes par pas, environ 10 ms, tant qu'une commande de sélection attend son caret ; 16 sinon) et le caret arrive au `State` suivant, où l'interface fait défiler vers lui (une fois sa page reçue). La mise en page est séquentielle : la page du foyer ne peut pas être servie avant ce qui la précède (pas de priorité de page possible dans le protocole). Les frappes mises en file passent entre deux pas de pompe et s'appliquent au foyer par position plate.

Mesures (`cargo run --release --locked --offline -p atelier-core-wasm --example rich_jump_bench`, texte brut, document neuf non mis en page, pompe `Layout` de 16) :

| Paragraphes | Commande | Avant : 1er appel | Après : 1er appel (max) | Après : total jusqu'au caret |
|---|---|---|---|---|
| 3 000 | Ctrl+Fin | 474 ms | 8,8 ms | 494 ms (178 appels) |
| 3 000 | Maj+Ctrl+Fin | 474 ms | 8,1 ms | 486 ms |
| 3 000 | clic lointain (`SetSelection`) | 474 ms | 7,8 ms | 472 ms |
| 12 000 | Ctrl+Fin | 710 ms (puis 470 appels, 2 000 ms au total) | 7,8 ms | 2 011 ms (740 appels) |
| 12 000 | Maj+Ctrl+Fin | 710 ms | 7,7 ms | 2 032 ms |
| 12 000 | clic lointain | 710 ms | 7,7 ms | 1 934 ms |

Le travail total pour atteindre la fin est inchangé (la mise en page est la même) ; il ne bloque plus la commande. Le pas de 64 de la pompe réduit les allers-retours à environ 45 (3 000 paragraphes) ou 190 (12 000). Non traités ici : `Statistics` (`STATISTICS_LAYOUT_BUDGET`) et la recherche, qui complètent la mise en page à la demande.

## Limites connues

- Retour arrière et Suppr sur un séparateur entre un paragraphe et un tableau, entre deux cellules ou autour d'un bloc opaque ne font rien : ni révision, ni déplacement du caret (comportement d'`replace_range` d'US-204, rendu explicite). Une sélection qui traverse la structure efface le texte qu'elle couvre sans fusionner ni retirer les cellules, le tableau ou le bloc (testé).
- L'étiquette de liste est le compteur décimal suivi d'un point : le modèle n'a pas de définitions de numérotation. Le texte alternatif d'un objet est vide. Niveau de titre déduit de l'identifiant de style.
- Les modifications sont appliquées sur place : si le registre ABI refuse une réponse (état `2`), l'édition reste appliquée, contrairement à la v1 qui publiait un candidat ; le moteur pose alors `resync` (voir plus haut) et le `State` suivant invalide tout le cache client.
- Pas de déposer-glisser (`DropText`), pas d'export (R3b, US-218), pas de révision attendue sur `ReplaceRange`.
- DOCX : au plus 262 144 paragraphes, 16 Mio de texte et 1 M runs à l'ouverture ; R3b traitera la sauvegarde.

## Tests

- `cargo test -p atelier-core-wasm --lib rich_editor` : 27 tests (dont 12 000 lignes pour les déplacements vers la queue, un DOCX de 2500 paragraphes, les 14 directions avec et sans extension depuis quatre positions, les séparateurs de tableau et de bloc opaque, la réponse perdue et une page dense) de la session par le vrai codec v2 (saisie, multi-paragraphes, graphème et mot, objets, sauts et symboles en ligne, annuler et rétablir, `MarkSaved`, déplacements et extension, test de position, mise en page jusqu'à « terminée » sur 900 paragraphes, DOCX, erreurs sans changement d'état, `Open` refusé, trames invalides, 400 éditions ASCII contre un texte miroir, 600 commandes et 2000 trames aléatoires sans panique).
- `node tests/rich-editor-abi.mjs [moteur.wasm]` : les scénarios de `tests/editor-abi.mjs` rejoués contre le vrai WASM en trames v2, avec le tableau scénario v1 → v2. Six scénarios v1 sans équivalent (`DropText` quatre fois, `Export` octet pour octet, retour arrière d'une réponse refusée par l'ABI, révision attendue d'un remplacement) : le critère « les scénarios de `tests/editor-abi.mjs` passent en v2 » reste décoché.
- Vecteurs gelés `tests/rich-editor-protocol-vectors.txt` régénérés pour `AccessibleWindow` (`node tests/rich-editor-protocol.mjs`).
