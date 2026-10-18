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
