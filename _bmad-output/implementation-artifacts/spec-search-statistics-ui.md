---
title: 'US-340 / US-343 — Recherche et statistiques dans l’éditeur texte'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: 662b4f658b0c760e0df383bc05687918320d1865
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="Lot complémentaire de plusieurs US demandé par l’utilisateur, développement parallèle autorisé">

## Intent

Raccorder les bibliothèques déjà livrées à l’éditeur texte : volet Ctrl+F avec extraits, compteur, options, suivant/précédent et surlignage ; mots dans la barre inférieure et dialogue de statistiques. Le produit conserve son modèle Rust comme source de vérité, y compris les règles Unicode. Ce lot comporte deux livrables indépendants à la demande de l’utilisateur.

## Boundaries & Constraints

Scope: the editable TXT buffer only. DOCX remains a read-only simplified preview; disable these tools there and explain that limitation in the documentation. Do not claim search of omitted headers/tables, notes/text boxes or Word parity. US-340/343 stay open where those criteria remain. Preserve zoom, IME, drag, clipboard, history, native file operations and modal keyboard guards. Reuse Atelier theme tokens and accessible dialogs. Inject labels from the existing catalog; do not implement US-102 or touch the other developer’s branches 102/306/315.

Search/statistics queries never mutate text, selection, dirty flag, undo history or revision. Results are bounded and carry the document revision. Discard stale query generations before changing UI/highlights; edits, replacement loads and preview transitions invalidate old results. Escape closes the pane and returns focus. Ctrl+F seeds the query from the current UTF-8 selection, capped at 1024 bytes. Enter/Shift+Enter cycles results with wrap notice. Typing in the query field or changing options does not steal focus into the document; selecting a result scrolls to and selects that occurrence without editing.

## I/O & Edge-Case Matrix

| Situation | Expected result |
|---|---|
| Empty query, zero hits, >1000 hits | Clear highlights; explicit empty/no-result state; bounded list/count with truncation notice |
| Unicode case/accent/NBSP/whole-word options | Exact library ranges; original text shown safely with text nodes, including literal markup |
| Edit, undo, load, query race, pane close | Old results cleared synchronously; latest query only; no late reopen or stale overlays |
| Zoom/scroll/container resize | Highlight geometry uses current page coordinates and visible viewport |
| Selection changes | Status selected/document words update without changing selection; dialogue reads current matching snapshot |
| Invalid wire frames | Strict rejection of lengths, flags, UTF-8, geometry, ranges and trailing bytes; prior document remains intact |
| Busy, preview, modal, composition | Tools disabled or scoped appropriately; no shortcut behind a modal or during IME/AltGr |
| Disposal | Timers/listeners/modal/pane removed; late replies ignored |

</frozen-after-approval>

## Code Map

- `crates/text-model/src/search/`, `statistics/` — reuse existing Unicode and counting algorithms; do not duplicate in JS.
- `crates/core-wasm/src/editor.rs`, `editor-layout.rs` — session, strict opcodes, cached page/line geometry. Derive search rectangles from line slots without rebuilding glyph display lists per hit.
- `web/editor-protocol.mjs` — owned binary command/response codec; extend opcodes 19 Search and 20 Statistics with bounded dedicated response envelopes.
- `web/editor.mjs`, `canvas-renderer.mjs` — parent query queue, selection commands and rendering overlays; never publish query responses as editor states.
- `web/app.mjs`, `editor-shell.mjs`, `index.html` — parent wiring, pane layout, toolbar commands, status strip and lifecycle.
- `web/ui-components.mjs`, `theme.css` — reusable focus-safe dialogs and colors.
- Native resources and browser runners use explicit routes; parent registers every new module/CSS.

## Tasks & Acceptance

- [x] Engine agent: Rust query implementation + JS codec + negative/unit tests + wire contract in `navigation-query-contract.md`.
- [x] Search agent: independent `web/search-pane.mjs`, `.css`, `tests/search-pane-browser.mjs` implementing view contract below.
- [x] Statistics agent: independent `web/statistics-controls.mjs`, `.css`, `tests/statistics-controls-browser.mjs` implementing view contract below.
- [x] Parent: editor query methods/overlays and integration; race-safe controllers, catalog, native routes, real Worker/WASM browser tests.
- [x] Parent: independent review, package/budget checks, docs/backlog, Graphify and publication.

Acceptance: Given a loaded TXT document, when Ctrl+F is used, then the pane finds the same exact UTF-8 ranges as Rust and all displayed hits are highlighted; navigation wraps and preserves editor history. Given a text selection, when statistics refresh, then status and dialogue show Rust-derived document/selection counts and actual layout pages/lines. Given a modal or preview, when shortcuts fire, then inactive tools do not act. Given malformed or stale results, when a reply arrives, then the current document and its overlays remain safe.

## Implementation Notes

Shared contract (freeze before dispatch):

- `encodeEditorCommand(EditorOpcode.Search,{revision,query,options})`, `Statistics,{revision}`. Search options keys `matchCase`, `wholeWord`, `ignoreAccents`, `matchNbsp`. Max query 1024 UTF-8 bytes, max results 1000, max total rects 8192; bound excerpts by UTF-8 bytes without splitting scalars. Statistics uses the session’s current selection. Wrong revision fails `EDITOR_STALE_QUERY`.
- Search decoded result `{kind:'search',revision,items:[{start,end,excerpt:{text,start,end,truncatedBefore,truncatedAfter},rects:[{page,x,y,width,height}]}],truncated}`; ranges are UTF-8 bytes. Statistics result `{kind:'statistics',revision,anchor,focus,document,selection}`, counters `{words,characters,charactersWithSpaces,paragraphs,lines,pages}` as finite safe nonnegative numbers. Empty selection counters zero. Document pages/lines from actual layout; selected pages/lines from touched layout ranges.
- `createSearchPane({document,labels,onQuery,onNavigate,onSelect,onClose})` → `{element,open({query,returnFocusTo}),close(),setState({items,index,truncated,busy,error,disabled,wrapped}),dispose()}`. `onQuery({query,options})`; `onNavigate(direction)` with +1/-1; `onSelect(index)`. Excerpt ranges UTF-8: decode byte slices safely for markup nodes, never innerHTML. Opening seeds input and requests query; disabled prevents actions. Result buttons must preserve current input focus when merely refreshing.
- `createStatisticsControls({document,labels,onOpen})` → `{element,setState({document,selection,hasSelection,busy,disabled,error}),open({returnFocusTo}),dispose()}`. Opening asks parent to refresh via onOpen; state updates dialog/status. Disabled unsupported notes/text-box option has clear localized explanatory label.

Agents own disjoint files, do not spawn or run BMAD/Graphify/commits. Parent runs structural refresh once, integrates shared files and publishes only after verification. Dedicated delegate files identify ownership; no competing edits to app/editor renderer/catalog.

## Spec Change Log

## Review Triage Log

Trois revues indépendantes GPT-6 Luna, effort moyen, sans historique partagé : blind hunter, edge-case hunter, verification-gap. Diff complet de 577 313 octets, fichiers nouveaux inclus ; les deux dernières revues ne rapportent aucun défaut ni manque de vérification. Chaque remarque du blind hunter est traitée ci-dessous.

| # | Remarque | Verdict | Preuve et traitement |
|---|---|---|---|
| B1 | 1 000 arrêts Tab dans les résultats | medium | Tous les boutons étaient tabulables. Correction directe : un arrêt mobile et flèches/Home/End, sans sélection automatique ; tests navigateur. |
| B2 | Clic sans focus ni retour accessible | false | Le bouton natif reçoit le focus au clic réel ; les nœuds restent stables et `aria-current` ainsi que le compteur vivant exposent la nouvelle occurrence. Conserver le focus dans le volet est voulu. |
| B3 | Match composé seulement d’une fin de ligne | false | `TextLines` exclut LF/CR/CRLF des paragraphes recherchables. Aucune occurrence composée d’un saut de ligne ne peut atteindre le contrôleur. |
| B4 | Match sans rectangle issu des slots | false | Toute occurrence non vide appartient à un paragraphe et intersecte les slots de graphèmes du layout TXT ; même une largeur nulle est ramenée à 0,5 pt. Les plafonds tronquent la liste explicitement. Le cas de saut de ligne est impossible (B3). |
| B5 | Démarrage depuis le curseur insuffisamment expliqué | low | Comportement correct, boutons de résultats donnant déjà accès au début. Ajout d’une phrase à la documentation d’utilisation ; pas de commande supplémentaire. |
| B6 | Option notes désactivée sans name/value | false | Aucun formulaire n’est soumis et la fonctionnalité n’est pas disponible. L’étiquette et `aria-describedby` expliquent cette limite ; name/value ne rendrait pas le calcul possible. Critère laissé ouvert dans l’US. |
| B7 | Valeurs indisponibles sans lien vers l’explication | low | L’explication était adjacente au tableau mais sans association ARIA. Correction directe : description du tableau pointant vers le statut du dialogue. |
| B8 | Réécriture identique des régions vivantes | low | `textContent` était réaffecté à chaque rendu. Correction directe : ne modifier les régions vivantes que lorsque leur texte change ; test des mutations DOM. Les changements de sélection restent annoncés. |
| B9 | Erreurs génériques et absence de reprise | false | Les labels localisés expliquent l’échec, dont le plafond de requête ; modifier la requête/une option ou rouvrir le dialogue relance la demande. Le code interne n’est pas une action utilisateur. |
| B10 | Limite UTF-8 et réduction de la sélection non documentées | low | Limite appliquée mais absente du guide. Ajout de 1 024 octets, réduction de sélection et erreur en cas de saisie/collage trop long. |

Contrôle supplémentaire de l’intégrateur : la fin d’une composition annulée utilisait un ancien appel d’overlay qui omettait temporairement les surlignages. Remplacement par le rafraîchissement commun et test Canvas immédiat.

La sonde WebView2 a également révélé une rotation du journal de 64 entrées pendant les imports de démarrage. Elle attend désormais `editorReady` avant ses requêtes explicites : aucune réduction des assertions ni augmentation du journal. La sonde réelle confirme les 36 ressources, CSP, Worker, saisie/undo/redo et zéro fuite de callbacks.

## Verification

Native/wasm Rust tests and strict JS codec negatives; real Edge UI suites in normal/forced colors; integrated query races/Unicode/navigation/statistics tests on fresh optimized WASM; existing zoom/edit/preview regressions. Repackage engine, enforce existing WASM size budget, compile/probe native resources, source/index hash audit. No physical keyboard or screen-reader certification implied.

La validation immuable passe **18 contrôles**, **1064 tests Rust**, avec 1119 entrées vérifiées. Les suites navigateur réelles couvrent le Worker/WASM, les réponses tardives, la navigation rapide, la composition annulée, les statistiques de sélection, les thèmes et le contraste élevé. Clippy hôte/WASM, paquet moteur, budget WASM et sonde WebView2 passent.
