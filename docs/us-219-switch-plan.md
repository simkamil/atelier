<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-219 — Préparation de la bascule (R7) et inventaire du retrait (R8)

Préparation du 2026-10-18, lot 2. L'interface n'est **pas** branchée sur l'éditeur v2 (c'est le lot 3) : rien ici ne coche un critère d'US-219.

## 1. Suites navigateur paramétrées

Toutes acceptent `--engine v1|v2` (défaut `v1`, constante `DEFAULT_ENGINE` de `tests/engine-adapter.mjs`).

| Lanceur | Rôle |
|---|---|
| `tests/run-editor-browser.mjs` | produit réel + éditeur (273 vérifications) |
| `tests/run-editor-utilities-browser.mjs` | menu contextuel, À propos, symboles |
| `tests/run-frontend-browser.mjs [all\|canvas\|input]` | Canvas et saisie |
| `tests/run-ui-browser.mjs [--forced-colors] [--screenshots]` | thèmes, composants, ruban |
| `tests/run-image-insert-browser.mjs` | insertion d'images dans le produit |
| `tests/mouse-selection.mjs` | sélection à la souris (test Node, sans navigateur) |
| `tests/run-latency-browser.mjs` | banc de latence (§2) |

Non paramétrée : `tests/run-worker-browser.mjs` (transport du Worker, ne pilote aucun éditeur). Les fichiers `tests/*-browser.mjs` sans lanceur propre sont des modules importés par `editor-browser.mjs` ou servis par `run-ui-browser.mjs` ; ils suivent le lanceur qui les charge.

**Statut v2.** Tant que `ENGINES.v2.ready` vaut `false`, `--engine v2` n'ouvre pas de navigateur : il écrit `{"status":"pending","engine":"v2","reason":"v2 en attente de R7 (lot 3)","exitCode":75}` et sort avec le **code 75**. Codes : 0 succès, 1 échec d'une vérification, 2 usage, **75 « en attente de R7 »**. 75 n'est pas un succès : la CI ne doit jamais l'accepter comme tel (voir §4).

**Adaptateur.** Ce que les suites savent de v1 est isolé dans `tests/editor-test-adapter.mjs` (opcodes ATED v1 : `InsertText`, `SetSelection`), seul fichier à réécrire pour v2 en R7, avec le drapeau `ENGINES.v2.ready` et `DEFAULT_ENGINE`. Aucun test v1 n'a changé de résultat.

Limite connue : `editor-browser.mjs` et ses modules (`zoom-`, `navigation-`, `replacement-`, `document-status-integration-`, `editor-utilities-integration-`, `properties-integration-browser.mjs`) pilotent encore `createEditor` de `web/editor.mjs` ; R7 les fera passer par une fabrique d'éditeur de l'adaptateur (aujourd'hui `import { createEditor } from "../web/editor.mjs"`, `tests/editor-browser.mjs:2`).

## 2. Banc de latence de bout en bout

`node tests/run-latency-browser.mjs [--shape lines|dense] [--pages 300] [--keys 300] [--engine v1|v2]` (page `tests/latency-browser.html`).

- Mesure : événement `keydown` + `beforeinput` synthétiques dans Edge headless, jusqu'à la fin du dessin Canvas. Le dessin est synchrone dans `publish()` (`web/editor.mjs:190-212`, `setPages` puis `updateViewport`) et le rappel `onState` suit immédiatement : son horodatage marque la fin du dessin. Sont aussi rapportés : aller-retour moteur, décodage + dessin, et temps jusqu'à l'image d'animation suivante. Aucune modification du code produit, aucun crochet.
- Séquence : 5 frappes d'échauffement non comptées, N caractères, Entrée, 20 caractères, Entrée ; 20 ms entre frappes ; le caret est posé juste après un marqueur situé au milieu du document.
- Documents déterministes (générateur congruentiel fixe) : `lines` = lignes courtes (3 à 7 lettres), `dense` = phrases pleines.
- Limites v1 : l'éditeur plafonne le texte à 128 Kio (`MAX_TEXT`, `crates/core-wasm/src/editor.rs:11`) **et** renvoie toutes les pages à chaque réponse, plafonnées à 32 Mio (`MAX_RESPONSE`, `editor.rs:13`). 300 pages tiennent en octets de texte (lignes courtes), mais le moteur répond `EDITOR_RESPONSE_LIMIT` : le banc cherche par dichotomie le plus grand document accepté, retire 10 % de marge (la frappe ajoute du texte) et l'écrit dans `document.note`.

### Résultats v1 (Edge 155 headless, 2026-10-18)

Brut dans `target/us-219/latency-*.json` (non versionné). Temps en ms de l'événement clavier à la fin du dessin Canvas ; 222 frappes (200 caractères, Entrée, 20 caractères, Entrée).

| Document réellement mesuré | octets | pages | moyenne | p50 | p95 | max |
|---|---|---|---|---|---|---|
| `lines`, 300 pages demandées : **refusées** (`EDITOR_RESPONSE_LIMIT`) ; mesure sur 98 pages (plus grand accepté env. 109) | 24 893 | 98 | 741,3 | 740,1 | 769,9 | 796,7 |
| `dense`, 124 Kio demandés : **refusés** ; mesure sur le plus grand accepté avec marge | 24 666 | 5 | 763,2 | 760,9 | 793,5 | 824,9 |
| contrôle : une page de 219 octets | 219 | 1 | 6,6 | 6,4 | 9,0 | 10,0 |

Sur le document de 98 pages : aller-retour moteur 580,5 ms en moyenne, décodage + dessin 160,8 ms ; Entrée 746,8 ms. Les 300 pages ne tiennent donc **pas** : en v1 le moteur refait la mise en page de tout le texte et renvoie toutes les pages à chaque frappe, et il refuse la réponse au-delà de 32 Mio, bien avant les 128 Kio de texte. Le coût suit la taille du texte (environ 30 µs par octet), pas le nombre de pages : 25 Ko donnent 0,74 s que ce soit sur 5 ou 98 pages. La machine de mesure n'était pas isolée (autres sessions Edge ouvertes), mais l'écart avec la cible est de deux ordres de grandeur.

Lecture : la cible d'US-219 (moins de 16 ms sur 300 pages) n'est atteignable qu'avec v2 (pages modifiées seules, voir `docs/rich-editor-migration.md`). Ces chiffres sont la référence à comparer à la mesure v2 de R7.

## 3. Inventaire R8 — ce que le retrait devra supprimer

Numéros de ligne relevés sur `main` au 2026-10-18 ; à relire avant R8. « Garder » = hors périmètre de la suppression d'après US-219 (« éditeur v1 retiré : `editor-layout.rs`, branches TXT de `editor.rs`, `PlainTextDocument` dans l'éditeur »).

### Rust (`crates/core-wasm`)

- `src/editor-layout.rs` (576 lignes) : supprimer en entier, tests internes inclus (`#[cfg(test)]` l.566). Déclaré `lib.rs:14-15` (`#[path = "editor-layout.rs"] mod editor_layout;`). Importé par `editor.rs:7`, `editor.rs:675-711` (`Layout::new`, `LayoutError::{TooManyPages,TooLarge}`) et `editor_queries.rs:5`. Hash dans `data/engine/manifest.json:18`, à régénérer.
- `src/editor.rs` (1181 lignes), session texte brut `EditorSession` (l.17-25) :
  - champs `document: Option<PlainTextDocument>` (l.18), `layout` (l.21), `preferred_x` (l.22) ; import `PlainTextDocument, PlainTextLimits` (l.4) ; `limits()` (l.819-824) ; `MAX_TEXT` (l.11) et `MAX_HISTORY` (l.12) ; `ensure_document` (l.661-668), `current_layout` (l.670-692), `state_response` (l.694-723), `map_text_error` (l.944-956).
  - branches d'opcodes de `transact` (l.56-650) : 10 Export (l.71-), 0 State (l.282), 1 Load (l.285-313, ouverture TXT via `PlainTextDocument::from_utf8` l.289), 2 InsertText (l.314), 3-4 suppressions (l.328), 5-6 Undo/Redo (l.343), 7 SetSelection (l.356), 8 Move (l.367-414 dont `layout.move_offset*` l.395-396), 9 HitTest (l.415), 12-13 mots (l.445), 14 SelectPoint (l.469), 16 DropText (l.539), 17 ReplaceRange (l.583), 11 MarkSaved (l.608), `_ => EDITOR_INVALID_COMMAND` (l.647).
  - helpers de navigation texte brut : `word_*`, `paragraph_*`, `extend_unit_selection`, `validate_grapheme` (l.826-942) et les tests `navigation_tests` (l.1167-).
  - Encodage de l'état v1 (`ATED`, `Writer`/`Reader` l.1071-1165) : à retirer s'il n'est plus partagé avec le protocole riche ; à vérifier avec `rich_protocol.rs`.
  - À **garder ou migrer** (hors TXT, encore utilisés par l'interface) : opcode 18 PreviewDocx et `encode_preview` (l.62-66, 777-817, `rich_preview`), 28/29 images (`images`, `ImageStore`), 30 ExportDocx (l.133-, `docx_export`), 19-22 Search/Statistics/Replace (`editor_queries.rs`, qui dépend de `Layout` pour les rectangles : à réécrire sur la géométrie v2, `editor_queries.rs:5`), `properties` (l.60). R8 ne commence que quand v2 sert ces fonctions.
- `src/lib.rs` : `mod editor_layout` (l.14-15) ; `editor_session()` (l.227-229) et `dispatch_editor` (l.330-) à rebrancher sur la session riche (`rich_session`) ; test de panne `abi-test-hooks` (l.271-) à conserver.
- Tests Rust v1 devenus inutiles (à supprimer ou à porter vers `rich_session`) : `crates/core-wasm/tests/editor_engine.rs` (448 l.), `drag_spacing.rs` (308 l., glisser-déposer TXT), `navigation_queries.rs` (292 l.), `replacement.rs` (261 l.), `insert_image.rs` (656 l.) — ces trois derniers couvrent des fonctions à conserver : les porter plutôt que les supprimer.

### Rust (`crates/text-model`) — `PlainTextDocument`

- Définition `src/lib.rs:170` (impl l.177). **Ne pas supprimer le type tant que d'autres lecteurs existent** : `plain_io/mod.rs:19,295-309,771-773` (US-316, `to_plain_document`), `search/source.rs:5,84` (`ParagraphSource`), `statistics/plain.rs:7,90`, `inline_image.rs:7` (commentaire). Dans l'éditeur seulement : retirer `editor.rs:4,18,289,661-664`. La suppression complète du type relève d'une décision séparée (US-218 pour TXT, US-316) ; à trancher avant R8.
- Tests qui l'utilisent et restent tant que le type reste : `tests/plain_text.rs`, `plain_io.rs`, `drag_spacing.rs`, `search.rs:11,268`, `statistics.rs:16,524`.

### JavaScript (`web/`)

- `web/editor.mjs` (662 l.) : éditeur v1 en entier, remplacé par le client riche (`web/rich-editor-client.mjs`, `rich-editor-protocol.mjs`). Points durs : `publish()` l.189-212 (contrôle A4 unique 595,276 × 841,89, `pages.length <= 1024`, texte complet dans `textRepresentation` à chaque état), `dispatch()` l.285-300 (listes d'opcodes v1 l.296-298), `openDocument` l.611, `saveAs` l.620-.
- `web/editor-protocol.mjs` (494 l.) : `EditorOpcode` (l.3-9), `EDITOR_LIMITS`, `encodeEditorCommand`, `decodeEditorResponse`. À garder en partie : `EDITOR_LIMITS` est lu par `web/image-insert.mjs:5`, et les opcodes 18/28-30 servent encore à l'aperçu, aux images et à l'export tant que ces fonctions ne passent pas par le protocole riche.
- Appelants : `web/app.mjs:4` (import `createEditor`), `:310-313` (création), `:433` (`openDocument`), `:441` (`saveAs`), `:353` (`previewDocx`) ; `web/editor-shell.mjs` et `web/editor-context-menu.mjs` si dépendants de l'état v1 (à vérifier au lot 3).
- Ressources embarquées à retirer : `crates/desktop-win/src/resources.rs:234-241` (`/editor-protocol.mjs`, `/editor.mjs`) et la liste de sondes `crates/desktop-win/src/win32.rs:2009`.

### Tests JavaScript

- À retirer ou porter : `tests/editor-protocol.mjs`, `tests/editor-abi.mjs` (ATED v1), `tests/editor-browser.mjs` (+ `editor-browser.html`, `editor-browser-watchdog.mjs`, `run-editor-browser.mjs`) et les modules d'intégration qu'il importe (`zoom-`, `navigation-`, `replacement-`, `document-status-integration-`, `editor-utilities-integration-`, `properties-integration-browser.mjs`) : les scénarios restent vrais pour v2, seul l'adaptateur change ; `tests/editor-test-adapter.mjs` (partie v1), `tests/wasm-worker.mjs:11` (`decodeEditorResponse`), `tests/image-insert.mjs:5`, `tests/replacement-protocol.mjs:3`, `tests/navigation-query-protocol.mjs:3`, `tests/rich-preview-protocol.mjs:5` (opcodes v1 de transport : à conserver tant que ces opcodes vivent).
- CI : `.github/workflows/ci.yml:80-97,163-174` (lignes des suites ci-dessus) ; retirer `--engine v1` et la ligne v1 au moment de R8, garder v2.
- Budget WASM : mesurer avant/après avec `tests/check-wasm-size.mjs` ; référence `tools/bench/references/wasm-size.json` (gain visé −25 à −50 Ko, `docs/rich-editor-migration.md:71`).

### Dépendances et ordre

1. R7 : v2 couvre aperçu DOCX, images, recherche/remplacement, statistiques, propriétés et enregistrement TXT (US-218), suites v2 vertes, latence < 16 ms mesurée avec `tests/run-latency-browser.mjs --engine v2`.
2. Rebrancher `dispatch_editor` (`lib.rs:330`) sur la session riche ; porter les tests Rust listés.
3. Supprimer `editor-layout.rs`, branches TXT de `editor.rs`, puis `web/editor.mjs` et la partie v1 de `editor-protocol.mjs`, ressources embarquées, `manifest.json` régénéré.
4. Retirer `--engine v1`, `ENGINES.v1` et l'adaptateur v1 ; mesurer le budget WASM.

## 4. Commandes à ajouter à la CI (à faire par l'intégrateur)

À côté des étapes existantes (qui restent valides, v1 par défaut) :

```
node tests/run-latency-browser.mjs --keys 100        # informatif en v1 (ne bloque pas)
node tests/run-editor-browser.mjs --engine v2        # attendu : code 75 tant que R7 n'est pas branché
```

Une étape « v2 en attente » doit traiter explicitement le code 75 (par ex. `$LASTEXITCODE -eq 75` est accepté et affiché comme « en attente », tout autre code non nul échoue) et **être retirée au lot 3** : alors v2 devient le défaut et 75 devient un échec. Même schéma pour `run-editor-utilities-browser`, `run-frontend-browser`, `run-ui-browser` (`--forced-colors` inclus), `run-image-insert-browser` et `mouse-selection`.

## 5. R7a — interface branchée (2026-10-18)

L'interface crée l'éditeur v2 avec `?engine=v2` (ou `localStorage["atelier.engine"] = "v2"`, `?engine=v1` forçant v1) ; `tests/engine-adapter.mjs` tient maintenant un statut **par suite** : `run-ui-browser`, `run-image-insert-browser` et `run-editor-browser` passent en v2, les autres gardent le code 75 avec leur raison. `tests/editor-test-adapter.mjs` produit les trames hors bande du moteur de la page de test. Voir [r7-ui-parity.md](r7-ui-parity.md) (parité d'API, fonctions désactivées, écarts) et [tests/README.md](../tests/README.md).

À faire en R7b avant de basculer le défaut : surbrillance de la sélection (protocole), dessin des images, sélection par mot et paragraphe, mesure de latence v2 (`run-latency-browser --engine v2` : le banc pilote encore `createEditor`), recopie de la CI (`run-rich-editor-ui-browser`, `rich-editor-unit`, lignes `--engine v2` des trois suites ; le code 75 des autres reste accepté comme « en attente »).

**R7b-1 (2026-10-18, aucune case d'US-219 cochée)** : surbrillance de la sélection, images dessinées et double et triple clic sont livrés en v2 ([spec](us-203-r7b1-selection-images.md), [écarts fermés](r7-ui-parity.md) §8 points 1 à 3) ; il reste, pour cette liste, la mesure de latence v2 et la recopie de la CI.

## 6. R7c — bascule de l'interface sur v2 (2026-10-18)

**Décision : la bascule est faite.** `web/engine-choice.mjs` : v2 par défaut ; `?engine=v1` dans l'URL de la page, ou l'option persistée `localStorage.setItem("atelier.engine", "v1")`, ramène à v1 (`removeItem` revient au défaut ; l'URL l'emporte sur l'option ; toute autre valeur donne v2). `DEFAULT_ENGINE` de `tests/engine-adapter.mjs` passe à `"v2"` : une suite lancée sans option teste le moteur par défaut, `--engine v1` teste v1. Les conditions posées à la bascule : (1) toutes les suites passent en v2, (2) le banc v2 est meilleur que v1 sur la moyenne et le p95.

### Glisser-déposer de texte (dernier écart fonctionnel)

`MoveText` `0x0140` ([protocole](us-203-rich-editor-protocol.md), « R7c ») : déplacement ou copie d'une plage en une étape d'annulation, mise en forme des caractères conservée, erreur `RICH_MOVE_UNSUPPORTED` (50). `RICH_CAPABILITIES.textDragDrop` vaut `true` ; `web/rich-editor.mjs` branche `web/text-drag-drop.mjs` (instantané sans texte : le moteur contrôle les frontières). Limites connues : pas de repère de dépôt (le module n'en reçoit pas du moteur, un `HitTest` ne rend pas de caret) ; une plage qui contient une image, un symbole, un lien, un champ, un signet, un tableau ou un bloc opaque est refusée (`RICH_MOVE_UNSUPPORTED`, message `status-move-unsupported` dans la région de statut) ; les paragraphes créés prennent les propriétés du paragraphe cible.

### État de parité final des suites

| Suite | v1 | v2 | Détail |
|---|---|---|---|
| `run-editor-browser` | vert | vert | v2 : scénario `editor-browser-v2.mjs` (double et triple clic, glisser-déposer, MoveText, recherche, zoom portés). `skipped` v2 : refus d'un fichier UTF-8 invalide (v2 détecte l'encodage), aperçu DOCX (remplacé par l'ouverture dans l'éditeur), boîte des propriétés sur l'aperçu (elle lit le document ouvert ; son scénario est dans `run-rich-editor-ui-browser`) |
| `run-rich-editor-ui-browser` | sans objet | vert | produit réel en v2 ; ajouts R7c : intégration des utilitaires (symboles, Ctrl+Maj+Espace, menu contextuel, presse-papiers, À propos : `editor-utilities-integration-v2.mjs`), barre d'état (`document-status-integration-v2.mjs`), Ctrl+N, Ctrl+O, documents lancés par le shell (US-115), dépôt de texte |
| `run-image-insert-browser` | vert | vert | v2 ne saute plus rien |
| `run-ui-browser` (et `--forced-colors`) | vert | vert | `skipped` v2 propre à la suite (rapporté dans sa sortie) |
| `run-editor-utilities-browser` | vert | vert | modules isolés (menu contextuel, À propos, symboles) sans éditeur : mêmes vérifications sous les deux moteurs ; l'intégration avec l'éditeur v2 est dans `run-rich-editor-ui-browser` |
| `run-frontend-browser` | vert | vert | `CanvasRenderer` et `text-input` partagés, sans éditeur ; le rendu des pages v2 est dans `run-rich-atlas-render-browser` |
| `mouse-selection.mjs` | vert | vert | module partagé ; la souris v2 est dans `run-editor-browser --engine v2` |
| `run-latency-browser` | mesuré | mesuré | voir ci-dessous |

Plus de code 75 : `V2_PENDING_REASONS` est vide ; un 75 reste un échec pour la CI.

### Latence de bout en bout (banc de R7c)

`tests/latency-browser.mjs` pilote `createEditor` (v1) ou `createRichEditor` (v2, configuration du produit : textes i18n, donc miroir DOM) ; document déterministe de lignes courtes (générateur congruentiel fixe) ; curseur au milieu (juste après un marqueur) ; 5 frappes d'échauffement non comptées, 300 caractères, puis 40 paires Entrée / Retour arrière ; 20 ms entre frappes ; temps de l'événement clavier à la **fin du dessin Canvas**. v1 : `onState` suit le dessin synchrone. v2 : `syncPages` dessine avant `onState` ; le banc enveloppe `CanvasRenderer.setPages` et `EditorPageCache.apply` et retient le premier dessin où la révision du cache a changé et où aucune page visible ne manque. Banc lancé **seul** (aucun autre test ni processus lourd en parallèle ; `machine.ranAlone: true` dans le JSON), Edge 155 headless, rendu logiciel, événements clavier synthétiques, i9-13900KF, Windows 11. Brut : `target/us-219/latency-{v1,v2}-r7c.json` (non versionné).

| Moteur, document | frappes | moyenne | p50 | p95 | max |
|---|---|---|---|---|---|
| v1, 98 pages (300 refusées : `EDITOR_RESPONSE_LIMIT` ; plus grand accepté env. 109) | 380 | 739,6 | 736,8 | 767,1 | 802,4 |
| v2, **300 pages** (79 489 octets) | 380 | **4,0** | 4,1 | **6,5** | **7,7** |

Par type de frappe (v1 / v2), moyenne, p95, max : caractères 738,2 / 765,5 / 802,4 contre 3,7 / 5,5 / 7,6 ; Entrée 741,8 / 765,7 / 768,4 contre 5,2 / 6,6 / 7,3 ; Retour arrière 748,2 / 775,9 / 784,6 contre 5,2 / 7,1 / 7,7. v2 : aller-retour moteur 3,4 ms en moyenne, décodage et dessin 0,6 ms. `untilNextFrameMs` (attente de l'image d'animation suivante, qui dépend de la cadence de l'écran) : v2 moyenne 10,6, p95 17,7, max 20,3.

Lecture : v2 est meilleur que v1 sur la moyenne (4,0 contre 739,6) et sur le p95 (6,5 contre 767,1), sur un document trois fois plus grand que le plus grand accepté en v1. Le critère « moins de 16 ms de bout en bout sur 300 pages » est tenu **au p95 (6,5) et au max (7,7)**, au sens du banc (fin du dessin Canvas) ; il ne l'est pas si l'on compte l'attente de l'image suivante (p95 17,7, max 20,3), qui n'est pas du calcul : c'est le rafraîchissement de l'écran à 60 Hz (16,7 ms entre images). Réserves : événements synthétiques (pas de clavier physique ni d'IME), Edge headless en rendu logiciel, une seule machine.

### Commandes à changer dans la CI (intégrateur ; la CI n'est pas modifiée ici)

- Chaque ligne de suite sans option teste maintenant **v2** : garder les lignes actuelles (`node tests/run-editor-browser.mjs`, `run-image-insert-browser`, `run-ui-browser`, `run-ui-browser --forced-colors`, `run-editor-utilities-browser`, `run-frontend-browser`, `mouse-selection.mjs`) et **ajouter** la même ligne avec `--engine v1` pour les sept ; supprimer les lignes `--engine v2` redondantes ou les garder explicites.
- Supprimer le traitement du code 75 s'il existe (75 = échec).
- Ajouter `node tests/run-rich-editor-ui-browser.mjs` (si absent), `node tests/rich-editor-move.mjs`, `node tests/rich-editor-unit.mjs`, `cargo test --locked --offline -p atelier-core-wasm -p atelier-text-model` (nouveaux : `--test rich_move`, `--test flat_move`, `rich_protocol::move_tests`).
- Banc de latence (hors blocage, machine dédiée) : `ATELIER_BENCH_ALONE=1 node tests/run-latency-browser.mjs --engine v2` ; v1 : `... --engine v1`.

## 7. R8 — lecteurs relevés avant suppression (2026-10-18, main @ ad117fc)

Relevé par `grep` dans `crates/`, `web/`, `tests/`, `tools/`, `.github/`, avant tout retrait. Le moteur a trois chemins dans `dispatch_editor` (`lib.rs`) : riche (`rich_editor::is_rich`), étagé (`staged::is_staged`) et « v1 » (`EditorSession::prepare_message`). Seul le troisième disparaît.

| Élément retiré | Lecteurs relevés | Décision |
|---|---|---|
| `editor-layout.rs` (`Layout`) | `editor.rs:7`, `editor_queries.rs:5`, `lib.rs:14` ; aucun autre | supprimé |
| `editor_queries.rs` (recherche, statistiques, remplacement v1) | `editor.rs` opcodes 19-22 seulement ; la v2 a `rich_editor/find.rs` (qui ne lit de v1 que `properties::{FIELDS, LIMITS}`) | supprimé, aucun appelant |
| `EditorSession` (`editor.rs`), `parse_command`, `Reader`, navigation TXT | `lib.rs:236,343-360` ; tests `editor_engine`, `drag_spacing`, `navigation_queries`, `replacement`, `insert_image`, `properties`, `rich_preview_session`, `rich_session.rs:459` | supprimés ; tests : voir ci-dessous |
| `editor.rs` : `EditorError`, `Writer`, `encode_preview` | `staged.rs:23`, `properties.rs:34`, `docx_export.rs:5`, `editor_queries.rs:4` | **gardés** (petit module de codage commun) |
| `docx_export.rs` (export TXT -> DOCX v1) | `editor.rs` opcode 30 ; `rich_editor/tests.rs:1210` (aide de test) | supprimé, aide de test réécrite |
| `images.rs` : `ImageStore`, `get`, `plain_bytes` | `editor.rs`, `docx_export.rs`, `editor-layout.rs:12` | retirés ; `analyze`, `render_bitmap`, `ImageInsertError` **gardés** (`rich_editor/io.rs:23,508`, `sel.rs:9,89`) |
| `properties.rs` : `is_properties`, `handle`, `body_len_ok` (opcodes 26/27) | `editor.rs:59,999` ; JS : `web/properties-protocol.mjs` (via `app.mjs` v1 seulement), `tests/properties-abi.mjs` | retirés ; `FIELDS`, `LIMITS`, `SaveStamp`, `parse_stamp`, `stamp`, `context` **gardés** (`rich_editor/io.rs:24`, `find.rs:31`, `staged.rs`) |
| opcode 18 `PreviewDocx` v1 | `editor.rs:62`, `web/editor.mjs` ; v2 : `previewDocx()` rejette (`rich-editor.mjs:1140`) | retiré ; `rich_preview.rs` **gardé** (`rich_session.rs`, `rich_editor.rs:21`, `rich_layout`, `staged.rs`) |
| `staged.rs` (US-092) | aucun appelant dans l'interface v2 (`web/rich-editor*.mjs`, `app.mjs`) ; transport `engine-worker.mjs`, `wasm-protocol.mjs`, `tests/wasm-worker.mjs`, `tests/properties.rs` (Rust) | **gardé** (hors périmètre sans certitude : transport annulable testé, peut resservir à l'ouverture de gros fichiers) |
| `web/editor.mjs` | `app.mjs:4,405`, `tests/editor-browser.mjs:2`, `tests/latency-browser.mjs:12`, commentaires | supprimé |
| `web/editor-protocol.mjs` | `web/editor.mjs`, `web/image-insert.mjs:5` (`EDITOR_LIMITS`), `tests/{editor-protocol,editor-abi,image-insert,replacement-protocol,navigation-query-protocol,rich-preview-protocol,wasm-worker,editor-test-adapter}.mjs`, `tests/check-i18n.mjs:33`, `resources.rs:234`, `win32.rs:2039` | voir commit JS |
| `web/engine-choice.mjs` | `app.mjs:21,196`, `tests/rich-editor-unit.mjs:5`, `resources.rs:289` | supprimé (plus qu'un moteur) |
| `tests/engine-adapter.mjs`, `--engine` | tous les `tests/run-*-browser.mjs`, `mouse-selection.mjs` | simplifié |
| CI | `ci.yml:148,158,179,194-204` | lignes v1 retirées |

## 8. R8 — retrait exécuté (2026-10-18, branche `r8-remove-v1`)

**Retiré.** Moteur : `editor-layout.rs` (576 lignes), `editor_queries.rs` (388), `docx_export.rs` (157), `editor.rs` ramené de 1 181 à 140 lignes (reste `EditorError`, `Writer`, `encode_preview`, partagés par `staged.rs` et `properties.rs`), `ImageStore` et constantes de `images.rs`, opcodes 26/27 de `properties.rs`, branche v1 de `dispatch_editor` (un message ni riche ni étagé répond `EDITOR_INVALID_COMMAND`). Tests Rust : `editor_engine`, `drag_spacing`, `navigation_queries`, `replacement`, `insert_image`, `rich_preview_session` (2 311 lignes) et un test de `rich_session.rs` ; `tests/properties.rs` réécrit (le tampon d'enregistrement est lu avec `fmt-ooxml` directement). Front : `web/editor.mjs` (662), `engine-choice.mjs`, `properties-protocol.mjs`, `editor-protocol.mjs` réduit de 494 à 101 lignes (échec et aperçu `ATRP` de l'ouverture étagée), `status-preview-*` du catalogue. Tests : `editor-protocol`, `editor-abi`, `properties-abi`, `replacement-protocol`, `navigation-query-protocol`, `editor-browser` (scénario v1, 721), `navigation-browser`, `properties-integration-browser`, `checkAppZoom`, `--engine` de tous les lanceurs, `ENGINES.v1`. CI : 3 lignes `--engine v1`, `editor-protocol`, `editor-abi`, `properties-abi` et deux protocoles v1 de la ligne `node --test`. Exe : ressources `/editor.mjs`, `/engine-choice.mjs`, `/properties-protocol.mjs` (59 ressources sondées, `check-embedded-resources.ps1` mis à jour) ; la sonde `--resources-self-check` pilotait le texte brut de v1 (`getState().text`, TXT en LF) : elle lit maintenant `readText()` et l'export CRLF de l'éditeur riche.

**Gardé.** `rich_preview.rs` (import, rendu et aperçu : `rich_session`, `rich_editor`, `rich_layout`, `staged`), `staged.rs` (ouverture/export annulables d'US-092 : aucun appelant dans l'interface v2, mais transport Worker testé par `tests/wasm-worker.mjs` ; suppression sans certitude refusée), `images.rs` (`analyze`, `render_bitmap`), `properties.rs` (table de champs, `SaveStamp`), `PlainTextDocument` dans `text-model`. Les fonctions recherche, statistiques, remplacement et propriétés n'ont pas été « migrées » : la v2 les avait déjà (R7b-2) et rien n'appelait plus leur version v1 ; leurs tests v1 sont remplacés par `rich_editor/find_tests.rs`, `tests/rich-editor-find.mjs`, `rich_selection.rs` (images) et `rich_move.rs`.

**Budget WASM** (`node tests/check-wasm-size.mjs --measure`, build `wasm-release`) : code 2 292 691 -> 2 182 764 octets (-109 927, -4,8 %) ; total 5 026 935 -> 4 917 008. L'objectif de revenir à 2 111 059 n'est **pas atteint** : il manque 71 705 octets. Répartition des corps de fonctions du build avec symboles (1 614 635 octets) : `alloc` 447 Ko et `core` 319 Ko (instanciations génériques des types du moteur), `atelier_fmt_ooxml` 283 Ko (lecture/écriture DOCX), `atelier_text_model` 139 Ko (dont `rich` 122 Ko), `atelier_core_wasm` 106 Ko (`rich_editor` 35, `rich_layout` 33, `rich_protocol` 15, `rich_preview` 14), `atelier_font` 78 Ko, `atelier_xml` 62 Ko, `atelier_text_layout` 45 Ko, `atelier_image` 37 Ko. Le moteur lui-même n'est plus la cause : l'écart est dans l'éditeur riche et le format DOCX. `tools/bench/references/wasm-size.json` non modifié.

**Réserves.** (1) Le mode « aperçu DOCX » de `web/app.mjs` est mort (`previewActive` ne devient jamais vrai : `openDocx` ouvre dans l'éditeur) mais n'est pas retiré : `web/rich-preview.mjs`, ses éléments de `index.html`, `closePreview`, `status-preview` restants, ressources embarquées ; à traiter en R8b. (2) `web/editor-protocol.mjs` garde son nom alors qu'il ne décode plus que l'aperçu étagé ; `EDITOR_LIMITS` reste lu par `web/image-insert.mjs`. (3) `data/engine/` n'est pas régénéré (l'intégrateur reconstruira). (4) La CI n'a pas été exécutée ; les suites ont été lancées une à une en local (voir le rapport).
