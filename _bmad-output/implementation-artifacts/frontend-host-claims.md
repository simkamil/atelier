---
title: 'US-101 — Canvas et décodage ATDL'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'b72b0377832358f590e313602cbef9f55a4ecc6d'
route: 'dispatch'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="persistent user authorization to develop stories in parallel">

## Intent

Le navigateur reçoit les buffers WASM mais ne sait pas afficher les listes de dessin. Ajouter un décodeur ATDL 1.0 et un moteur Canvas réutilisable pour avancer US-101 et préparer US-120. Les ressources de glyphes/images arrivent par résolveurs injectés, comme pour le backend SVG. Le transport des contours depuis le moteur et l'intégration d'un éditeur restent à réaliser ; cette livraison ne clôt pas automatiquement US-101.

## Boundaries & Constraints

JavaScript ES modules vanilla, zéro dépendance. Respecter exactement docs/us-080-display-list.md et la validation de crates/render/src/lib.rs, notamment entiers u64 BigInt, bornes cumulées, UTF-8 strict, piles indépendantes, composition ATDL self puis next (next*self), identique à DisplayTransform::then, clips déjà en coordonnées de page et rejet des octets finaux. Ne jamais interpréter une chaîne comme HTML. Les erreurs sont des diagnostics développeur, aucune nouvelle interface produit.

Canvas réel avec Path2D, cache borné par (font_id,glyph_id) et invalidation explicite lors d'un changement de ressources. Les résolveurs synchrones fournissent contours en unités de page relatives à l'origine et images CanvasImageSource ; documenter cette responsabilité. Distinguer points document, zoom CSS et DPR. Un Canvas représente le viewport, jamais 300 grandes surfaces allouées. Ne rejouer que les pages visibles. Une région sale efface et repeint tous les objets qu'elle intersecte dans leur ordre, y compris leur ancien emplacement après changement de page/viewport. Gérer resize, DPR 1/1.5/2, zoom, page hors écran, viewport vide et échec de ressource sans laisser un état Canvas déséquilibré. API de composition overlay : liste de rectangles CSS locaux au viewport, soulignés, remplaçables/effaçables, aucune mise en page du texte IME dans ce module.

L'agent possède uniquement web/display-list.mjs, web/canvas-renderer.mjs, tests/display-list.mjs, tests/canvas-browser.html, tests/canvas-browser.mjs, crates/render/examples/display_list_oracle.rs et docs/us-101-canvas-renderer.md. Root possède README/backlog/CI/runner/main/Graphify/Git et assemble les tests. Ne pas lancer Graphify, d'autres agents, commit/push ou contrôles workspace ; lancer les tests ciblés disponibles. UTF-8/LF et SPDX. Documenter sans affirmer un test exécuté par root. Le benchmark 300 pages mesure réellement les durées, sans assimiler headless logiciel à une preuve de 60 fps sur matériel physique.

## I/O & Edge-Case Matrix

| Scénario | Entrée / état | Résultat attendu | Erreur |
|---|---|---|---|
| ATDL Rust complet | tous opcodes, gros IDs u64 | mêmes valeurs et fin exacte | aucun arrondi d'ID |
| Flux hostile | troncature, tag/version/UTF8/pile/limite invalide | rejet borné avant dessin | erreur explicite |
| Pages et glyphes | visibles/hors écran, glyphes répétés | seuls visibles dessinés, Path2D réutilisé | ressource absente signalée |
| Échelles | viewport resize, DPR1.5/2, zoom | dimensions physiques et pixels corrects | nombre invalide rejeté |
| Région sale | ancien dessin/clip/transformation | effacement et replay sans résidu | état restauré même sur erreur |
| Composition | rectangles changés puis effacés | soulignement visible puis supprimé | rect invalide rejeté |

</frozen-after-approval>

## Code Map

- `crates/render/src/lib.rs`, `svg.rs` — schéma et sémantique de référence.
- `crates/font/src/outline.rs` — contours en unités de police, conversion appartenant au fournisseur.
- `tests/run-worker-browser.mjs` — root prépare un runner analogue pour Canvas et saisie.

## Tasks & Acceptance

- [x] `web/display-list.mjs` — décodage strict et borné ATDL 1.0.
- [x] `web/canvas-renderer.mjs` — replay, ressources/cache, culling, dirty/DPR, overlay.
- [x] `crates/render/examples/display_list_oracle.rs`, `tests/display-list.mjs` — oracle Rust indépendant et flux invalides.
- [x] `tests/canvas-browser.html`, `tests/canvas-browser.mjs` — pixels de vrais Canvas, caches, régions sales, culling, DPR et benchmark 300 pages. Exporter `runCanvasChecks()` retournant un tableau de résultats ; le root fournit le runner/report.
- [x] `docs/us-101-canvas-renderer.md` — API, contrat ressources et limites de validation.

Given des flux produits par Rust, when JS décode, then les dix opcodes et limites ont les mêmes valeurs/sémantiques. Given un vrai Chromium, when Canvas rejoue chemins/glyphes/images/clips/matrices et régions sales, then pixels et statistiques de culling/cache confirment le résultat. Given 300 pages, when le viewport défile, then le nombre de pages rejouées reste borné à la visibilité et les durées mesurées sont rapportées sans seuil inventé.

## Implementation Notes

2026-10-11 — Module implémenté ; la validation de livraison ne clôt pas les critères matériels/intégration encore ouverts dans US-V0-V1.md.

## Spec Change Log

2026-10-11 — Correction factuelle du plan : la formulation initiale « current*new » était ambiguë et contraire à DisplayTransform::then. L’oracle Rust et le test SVG translate5 puis scale2 donnent e10 : conserver next*current dans la pile ATDL, puis appliquer la base viewport/DPR après la transformation documentaire. Cette correction ne change pas l’intention utilisateur ; elle aligne le plan sur le contrat existant. KEEP : piles indépendantes et clips en coordonnées de page.

## Review Triage Log

| ID | Claim | Verdict | Route | Evidence |
|---|---|---|---|---|
| B1 | transform stack off-by-one | false | Rejected | transforms starts with identity (length1); before the128th push length128 is accepted, before129th length129 >128 rejects. >= would wrongly reject valid128th transform. Rust MAX_STACK_DEPTH counts pushed transforms, not identity. |
| B2 | rectangle extent exceeds coordinate bound | false | Rejected | Rust validate_rect checks finite extents and endpoint coordinates, not width<=MAX_COORDINATE. Two endpoints at ±1e150 yield2e150, still finite. JS matches this contract; no overflow or invalid accepted geometry demonstrated. |
| B3 | pageGap NaN/negative | low | Patch Canvas | Constructor accepted non-finite/negative pageGap, causing invalid page offsets or overlap. Direct finite/non-negative validation and browser assertions added. |
| B4 | invalid Canvas callbacks | low | Patch Canvas | Non-function resolver/onDiagnostic options failed during replay and could replace diagnostics. Constructor now validates callback types, covered in Chromium. |
| B5 | offscreen overlays replayed | low | Rejected | Offscreen overlay iteration is real, while canvas clip prevents incorrect pixels. Normal composition uses a small rectangle set; filtering adds another branch without an everyday defect or measured material regression. |
| B6 | benchmark always green | low | Patch report label | The culling assertions are functional; timing is explicitly headless/informative in docs and has no physical60fps claim. Add informational:true directly to the measurement record to make this distinction machine-readable. |
| B7 | selection clone fallback aliases owner | medium | Patch input | structuredClone failure returned the original object, breaking the stable composition snapshot. Reject through existing onError path; clone outgoing payloads as well to prevent callbacks mutating internal snapshot. |
| B8 | IME duplicate after zero timer | maybe-false | Defer unverified medium | Synthetic delayed insertText can trigger this path; a real WebView2 event sequence crossing this task boundary has not been established. Indefinite suppression demonstrably drops later identical user text. Capture native IME traces at US-103 manual acceptance to settle ordering. |
| B9 | paste duplicate after zero timer | false | Rejected | onPaste cancels the native paste default before routing. A successful canceled paste does not subsequently generate native insertion; synthetic unrelated later insertFromPaste must not be suppressed indefinitely. Same-task duplicate protection remains tested. |
| B10 | focusable aria-hidden textarea | medium | Patch input | Created textarea was focusable and aria-hidden, hiding the active input from accessibility APIs. Remove aria-hidden, add an accessible label and owner override documentation; browser DOM assertions cover the direct semantics. |
| E1 | unsupported edit delegated twice | medium | Patch input | Successful onUnhandled in beforeinput was called again for matching input. Reuse existing same-task handled-input suppression; retain fallback when first callback failed and native text when no delegate exists. |
| E2 | noncloneable composition selection | medium | Patch input; same cause B7 | Verified separately: copySelection returned an alias after clone failure; subsequent owner mutation affected commit. The B7 correction also covers this outcome. |
| E3 | pageGap invalid | low | Patch Canvas; same cause B3 | Verified separately: adding NaN/Infinity to top made later pages non-visible; negative gap overlapped. Constructor validation covers this outcome. |

## Verification

- `cargo run -p atelier-render --example display_list_oracle --locked --offline` et `node tests/display-list.mjs` — conformité indépendante.
- `node tests/run-frontend-browser.mjs` — root assemble le runner Edge réel.

Module livré et revu : 34 contrôles fonctionnels/qualité, 318 tests workspace, 27 tests shell release. US-114 validée ; US-101/103/111 restent ouvertes pour les critères explicités au backlog. Voir frontend-host-validation.json et frontend-host-review.md.


---
title: 'US-103 — Capture clavier et composition'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'b72b0377832358f590e313602cbef9f55a4ecc6d'
route: 'dispatch'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="persistent user authorization to develop stories in parallel">

## Intent

Préparer la saisie d'US-120 avec un adaptateur DOM qui transmet le texte résolu par le navigateur et les commandes d'édition à des callbacks injectés. Le WASM actuel renvoie un écho, sans handler d'édition ni géométrie de sélection : l'adaptateur ne prétend pas constituer un éditeur et US-103 reste ouverte tant que cette intégration et les essais clavier natifs manquent.

## Boundaries & Constraints

ES modules vanilla, zéro dépendance. Créer/attacher un textarea réellement focusable, invisible, positionné au caret ; pas de display:none, visibility:hidden ou contenteditable. Le propriétaire fournit sélection courante et caret en coordonnées CSS du viewport et reçoit des commandes structurées. API documentée `attachTextInput` avec focus, updateCaret et dispose. Ne pas dériver les caractères depuis KeyboardEvent.key/keyCode : beforeinput/input sont l'autorité pour caractères résolus, y compris accents, AltGr et Alt+code. Keydown traite uniquement les raccourcis/navigation effectivement reconnus ; ne pas intercepter Ctrl+Alt/AltGraph comme Ctrl ni isComposing/229. Éviter les doublons keydown/beforeinput et beforeinput/input.

La composition est une transaction distincte : début, aperçu non engagé, mise à jour, commit unique ou annulation. Préserver la sélection de début, ne pas commettre insertCompositionText en boucle ; couvrir l'ordre Chromium où le dernier insertText arrive après compositionend, ainsi que compositionend sans input final. Les callbacks d'aperçu reçoivent l'état texte/selection/caret ; le propriétaire fournit les rectangles à Canvas pour le soulignement, sans inventer une mise en page ou afficher un texte DOM superposé au document. Nettoyer la valeur/selection du textarea après traitement sans casser une composition active. Paste texte/plain si disponible ; types non supportés laissent le navigateur agir ou délèguent explicitement, sans perte silencieuse.

Conserver le focus lors d'interactions explicitement demandées par le propriétaire, sans voler celui d'une boîte de dialogue ou au retour de window blur. Aucun listener global permanent, aucune modification de sélection d'autres champs. dispose enlève listeners, textarea créé et styles modifiés/restaure l'élément fourni ; un élément fourni garde ses attributs initiaux. Rectangles finis et non négatifs ; erreurs callback ne laissent pas une composition/focus incohérents. Aucun nouveau libellé produit, uniquement diagnostics développeur.

L'agent possède web/text-input.mjs, tests/input-browser.html, tests/input-browser.mjs, docs/us-103-text-input.md et _bmad-output/implementation-artifacts/epic-0.11-context.md. Le contexte est explicitement dérivé de SPEC et du backlog faisant autorité (planning-artifacts absent), sans prétendre à un document source inexistant. Root possède runner commun, README, backlog, CI, main, Graphify/Git. Aucun agent supplémentaire, Graphify, commit/push ni tests workspace. UTF-8/LF, SPDX ; ne pas affirmer l'exécution de tests natifs AZERTY/IME à partir d'événements synthétiques.

## I/O & Edge-Case Matrix

| État | Entrée | Résultat attendu | Limite/erreur |
|---|---|---|---|
| Focus | caret bouge/resize puis dispose | textarea au caret, focus explicite, nettoyage | rect hostile rejeté |
| Texte résolu | é, €, @, #, {, [, Unicode | texte intact, commit une fois | aucune traduction de touche |
| Raccourcis | Ctrl+Z/Y, navigation, AltGr, key229 | seules commandes reconnues empêchées | AltGr/composition préservés |
| Édition | retour, delete, backward/forward, paste | commande exacte/texte, aucune duplication | type inconnu non perdu |
| IME | start/update/beforeinput/end/final input | aperçu séparé, sélection stable, commit unique | annulation vide sans insertion |
| Blur/destruction | autre champ focus, dispose en composition | pas de vol de focus, transaction annulée | callbacks arrêtés |

</frozen-after-approval>

## Code Map

- `web/engine-client.mjs`, `wasm-protocol.mjs` — transport indépendant ; ne pas modifier l'écho ABI.
- `crates/model/src/selection.rs` — futures positions moteur en octets UTF-8, ne pas confondre indices DOM UTF-16.
- `SPEC.md` §6.7 et `US-V0-V1.md` E0.11 — source de cette infrastructure.

## Tasks & Acceptance

- [x] `web/text-input.mjs` — adaptateur, contrat commandes/selection/overlay, cycle de vie sûr.
- [x] `tests/input-browser.html`, `tests/input-browser.mjs` — vrais textarea/focus et routage d'événements Chromium, assertions sur tous cas matrice ; exporter `runInputChecks()` retournant un tableau de résultats, root fournit runner/report.
- [x] `docs/us-103-text-input.md` — API, ordre composition, limites et checklist de vrais essais clavier/IME.
- [x] `_bmad-output/implementation-artifacts/epic-0.11-context.md` — contexte source-backed, dépendances manquantes explicites.

Given un vrai DOM Chromium, when les séquences d'événements couvrant la matrice arrivent, then chaque commande/commit/aperçu est reçu exactement une fois avec le bon texte et la bonne sélection. Given un autre champ, when il prend le focus ou l'adaptateur est détruit, then aucun focus n'est volé et aucun callback ultérieur n'est produit. Les événements synthétiques ne valent pas acceptation des scénarios clavier/IME natifs d'US-103.

## Implementation Notes

2026-10-11 — Module implémenté ; la validation de livraison ne clôt pas les critères matériels/intégration encore ouverts dans US-V0-V1.md.

## Spec Change Log

## Review Triage Log

| ID | Claim | Verdict | Route | Evidence |
|---|---|---|---|---|
| B1 | transform stack off-by-one | false | Rejected | transforms starts with identity (length1); before the128th push length128 is accepted, before129th length129 >128 rejects. >= would wrongly reject valid128th transform. Rust MAX_STACK_DEPTH counts pushed transforms, not identity. |
| B2 | rectangle extent exceeds coordinate bound | false | Rejected | Rust validate_rect checks finite extents and endpoint coordinates, not width<=MAX_COORDINATE. Two endpoints at ±1e150 yield2e150, still finite. JS matches this contract; no overflow or invalid accepted geometry demonstrated. |
| B3 | pageGap NaN/negative | low | Patch Canvas | Constructor accepted non-finite/negative pageGap, causing invalid page offsets or overlap. Direct finite/non-negative validation and browser assertions added. |
| B4 | invalid Canvas callbacks | low | Patch Canvas | Non-function resolver/onDiagnostic options failed during replay and could replace diagnostics. Constructor now validates callback types, covered in Chromium. |
| B5 | offscreen overlays replayed | low | Rejected | Offscreen overlay iteration is real, while canvas clip prevents incorrect pixels. Normal composition uses a small rectangle set; filtering adds another branch without an everyday defect or measured material regression. |
| B6 | benchmark always green | low | Patch report label | The culling assertions are functional; timing is explicitly headless/informative in docs and has no physical60fps claim. Add informational:true directly to the measurement record to make this distinction machine-readable. |
| B7 | selection clone fallback aliases owner | medium | Patch input | structuredClone failure returned the original object, breaking the stable composition snapshot. Reject through existing onError path; clone outgoing payloads as well to prevent callbacks mutating internal snapshot. |
| B8 | IME duplicate after zero timer | maybe-false | Defer unverified medium | Synthetic delayed insertText can trigger this path; a real WebView2 event sequence crossing this task boundary has not been established. Indefinite suppression demonstrably drops later identical user text. Capture native IME traces at US-103 manual acceptance to settle ordering. |
| B9 | paste duplicate after zero timer | false | Rejected | onPaste cancels the native paste default before routing. A successful canceled paste does not subsequently generate native insertion; synthetic unrelated later insertFromPaste must not be suppressed indefinitely. Same-task duplicate protection remains tested. |
| B10 | focusable aria-hidden textarea | medium | Patch input | Created textarea was focusable and aria-hidden, hiding the active input from accessibility APIs. Remove aria-hidden, add an accessible label and owner override documentation; browser DOM assertions cover the direct semantics. |
| E1 | unsupported edit delegated twice | medium | Patch input | Successful onUnhandled in beforeinput was called again for matching input. Reuse existing same-task handled-input suppression; retain fallback when first callback failed and native text when no delegate exists. |
| E2 | noncloneable composition selection | medium | Patch input; same cause B7 | Verified separately: copySelection returned an alias after clone failure; subsequent owner mutation affected commit. The B7 correction also covers this outcome. |
| E3 | pageGap invalid | low | Patch Canvas; same cause B3 | Verified separately: adding NaN/Infinity to top made later pages non-visible; negative gap overlapped. Constructor validation covers this outcome. |

## Verification

- `node tests/run-frontend-browser.mjs` — root assemble les suites réelles Canvas/DOM, aucune validation d'IME matériel inférée.

Module livré et revu : 34 contrôles fonctionnels/qualité, 318 tests workspace, 27 tests shell release. US-114 validée ; US-101/103/111 restent ouvertes pour les critères explicités au backlog. Voir frontend-host-validation.json et frontend-host-review.md.


---
title: 'US-111/114 — Hébergement WebView2 et runtime absent'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'b72b0377832358f590e313602cbef9f55a4ecc6d'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="persistent user authorization to develop stories in parallel">

## Intent

Héberger un vrai contrôle WebView2 dans la fenêtre Win32 existante et traiter proprement l'absence de runtime. Le prochain lot US-112/113 apportera les ressources de l'application et le pont natif. Cette livraison peut rester sur about:blank : elle n'affirme pas livrer un éditeur.

## Boundaries & Constraints

Rust std uniquement, FFI/COM extern system manuelles repr(C), unsafe justifié. SDK officiel Microsoft.Web.WebView2 1.0.4258.31 : data/webview2/include/WebView2.h est l'oracle ABI exact. Root importe chargeur statique x64 MSVC, licence et manifest, et possède build.rs. Aucun crate externe, DLL cherchée dans cwd/PATH ni export interne du runtime. Root possède main/lib/CI/Git/Graphify ; agent possède window.rs, win32.rs, nouveau webview.rs et docs/us-111-webview2.md. Aucun sous-agent, Graphify, commit/push ni tests workspace.

API window::run(state_file:&Path,title:&str,webview_data:&Path)->io::Result<()> ; root passe DataPaths.webview_dir seulement pour Ready. Ne jamais créer/résoudre une autre racine/cache avant Ready. Conserver --self-check natif existant indépendant du runtime. Fournir window::webview_self_check(state_file:&Path,webview_data:&Path,cycles:usize)->io::Result<String> sur Windows ; contrôler que chemins de test restent sous target avant toute création. Root l'expose via --webview-self-check (paths dérivés d'un seul dossier target fourni), avec 100 cycles bornés. Non-Windows : stubs cohérents.

Initialiser COM STA et équilibrer CoUninitialize après destruction du contrôleur et callbacks. QueryInterface reconnaît IUnknown et IID réel, AddRef/Release symétriques, erreurs HRESULT et pointeurs nuls contrôlés, aucune panique ne traverse FFI. Environnement puis contrôleur créés async ; callbacks ne gardent aucun pointeur temporaire vers un HWND/context détruit. Fermer avant fin d'initialisation, échec immédiat/asynchrone, timeout et destruction sont sûrs et bornés. Close avant Release, aucune référence cyclique ni use-after-free. Ne pas quitter avec callbacks auto-détenus encore vivants sans stratégie sûre de drainage/annulation ; documenter la stratégie réelle.

Adapter Bounds à client rect physique sur WM_SIZE/WM_DPICHANGED ; restaurer visibilité après minimize et MoveFocus(PROGRAMMATIC) sur WM_SETFOCUS. PMv2 reste activé avant HWND. Le DPI de WebView suit le moniteur du parent ; distinguer test de message synthétique et vrais écrans. Settings::put_AreDevToolsEnabled vaut cfg!(debug_assertions), donc false en release, et lire la valeur pour le self-check.

Détecter GetAvailableCoreWebView2BrowserVersionString avant CreateEnvironment, libérer version avec CoTaskMemFree. Runtime absent → dialogue natif français avec URL https://developer.microsoft.com/microsoft-edge/webview2/ et texte expliquant Ctrl+C pour copier le message et l'adresse ; aucun navigateur/installeur lancé automatiquement. Erreur runtime disparue pendant création également contrôlée. Tests injectés prouvent absence → aucun environnement/cache créé, Ready seulement → création, erreurs explicites. Le contrôle réel répète 100 créations/fermetures de contrôleur et confirme les références/callbacks applicatifs revenus au niveau initial ; compte handles avec échauffement et mesure réelle, sans seuil arbitraire masquant une fuite.

## I/O & Edge-Case Matrix

| État | Action | Attendu | Erreur |
|---|---|---|---|
| Ready/runtime présent | ouvrir | contrôleur réel dans client rect, cache sous racine fournie | HRESULT explicite |
| Move/Quit | démarrage | aucun environnement/cache | aucun accès caché |
| Runtime absent | probe échoue | message natif copiable, arrêt propre avant création | pas de crash |
| Resize/focus/DPI/minimize | messages parent | bounds/focus/visibilité adaptés | pas de pointeur mort |
| Initialisation async | fermeture/erreur/callback tardif | nettoyage une fois, fin bornée | erreur propagée |
| Debug/release | lire settings | DevTools true/false respectivement | lecture vérifiée |
| Répétition100 | ouvrir/fermer | contrôleurs/callbacks équilibrés | fuite fait échouer contrôle |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/win32.rs` — Context boxed, HWND propriétaire, WM_SIZE/DPICHANGED/CLOSE et pump existants.
- `crates/desktop-win/src/window.rs` — API publique et stubs non-Windows, géométrie persistante.
- `crates/desktop-win/src/data.rs` — DataPaths.webview_dir est pur ; démarrage root contrôle Ready.
- `data/webview2/include/WebView2.h` — signatures, IIDs, ordre des vtables ; ne pas se fier à des indices mémorisés.

## Tasks & Acceptance

- [x] `webview.rs` — COM RAII/callbacks, probe, paramètres, diagnostics et tests ciblés.
- [x] `win32.rs`, `window.rs` — cycle de vie hébergé, bounds/focus/DPI, self-check100 et API.
- [x] `docs/us-111-webview2.md` — ABI/chargeur, démarrage et preuves/limites.
- [ ] Root main/lib/build/CI — Ready gate, chargeur statique et self-check automatisé.

Given Ready avec runtime, when la fenêtre change de taille/DPI/focus, then le vrai contrôle suit le parent et ferme proprement. Given runtime absent, when démarrage normal, then dialogue français copiable et aucune création d'environnement. Given100 cycles et release, when self-check tourne, then références applicatives équilibrées et DevTools désactivés. Les réglages/cache respectent la racine US-116 et le self-check natif d'US-110 reste autonome.

## Implementation Notes

Le coordinateur a finalisé COM/cleanup après la limite d’usage de l’agent. Le contrôle strict100 échoue sur les handles ; US-111 reste ouverte. Le wrapper CI publie le résultat strict et ne tolère que le diagnostic final de croissance après100 cycles fonctionnels sans callback restant. Voir frontend-host-exploration.json et docs/us-111-webview2.md. US-114 peut être validée indépendamment.

2026-10-11 — Module implémenté ; la validation de livraison ne clôt pas les critères matériels/intégration encore ouverts dans US-V0-V1.md.

## Spec Change Log

## Review Triage Log

| ID | Claim | Verdict | Route | Evidence |
|---|---|---|---|---|
| B1 | transform stack off-by-one | false | Rejected | transforms starts with identity (length1); before the128th push length128 is accepted, before129th length129 >128 rejects. >= would wrongly reject valid128th transform. Rust MAX_STACK_DEPTH counts pushed transforms, not identity. |
| B2 | rectangle extent exceeds coordinate bound | false | Rejected | Rust validate_rect checks finite extents and endpoint coordinates, not width<=MAX_COORDINATE. Two endpoints at ±1e150 yield2e150, still finite. JS matches this contract; no overflow or invalid accepted geometry demonstrated. |
| B3 | pageGap NaN/negative | low | Patch Canvas | Constructor accepted non-finite/negative pageGap, causing invalid page offsets or overlap. Direct finite/non-negative validation and browser assertions added. |
| B4 | invalid Canvas callbacks | low | Patch Canvas | Non-function resolver/onDiagnostic options failed during replay and could replace diagnostics. Constructor now validates callback types, covered in Chromium. |
| B5 | offscreen overlays replayed | low | Rejected | Offscreen overlay iteration is real, while canvas clip prevents incorrect pixels. Normal composition uses a small rectangle set; filtering adds another branch without an everyday defect or measured material regression. |
| B6 | benchmark always green | low | Patch report label | The culling assertions are functional; timing is explicitly headless/informative in docs and has no physical60fps claim. Add informational:true directly to the measurement record to make this distinction machine-readable. |
| B7 | selection clone fallback aliases owner | medium | Patch input | structuredClone failure returned the original object, breaking the stable composition snapshot. Reject through existing onError path; clone outgoing payloads as well to prevent callbacks mutating internal snapshot. |
| B8 | IME duplicate after zero timer | maybe-false | Defer unverified medium | Synthetic delayed insertText can trigger this path; a real WebView2 event sequence crossing this task boundary has not been established. Indefinite suppression demonstrably drops later identical user text. Capture native IME traces at US-103 manual acceptance to settle ordering. |
| B9 | paste duplicate after zero timer | false | Rejected | onPaste cancels the native paste default before routing. A successful canceled paste does not subsequently generate native insertion; synthetic unrelated later insertFromPaste must not be suppressed indefinitely. Same-task duplicate protection remains tested. |
| B10 | focusable aria-hidden textarea | medium | Patch input | Created textarea was focusable and aria-hidden, hiding the active input from accessibility APIs. Remove aria-hidden, add an accessible label and owner override documentation; browser DOM assertions cover the direct semantics. |
| E1 | unsupported edit delegated twice | medium | Patch input | Successful onUnhandled in beforeinput was called again for matching input. Reuse existing same-task handled-input suppression; retain fallback when first callback failed and native text when no delegate exists. |
| E2 | noncloneable composition selection | medium | Patch input; same cause B7 | Verified separately: copySelection returned an alias after clone failure; subsequent owner mutation affected commit. The B7 correction also covers this outcome. |
| E3 | pageGap invalid | low | Patch Canvas; same cause B3 | Verified separately: adding NaN/Infinity to top made later pages non-visible; negative gap overlapped. Constructor validation covers this outcome. |

## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — tests ciblés natifs et doubles COM.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — aucun avertissement.
- `atelier.exe --webview-self-check target/webview-check` — root lance100 cycles réels en debug/release.

Module livré et revu : 34 contrôles fonctionnels/qualité, 318 tests workspace, 27 tests shell release. US-114 validée ; US-101/103/111 restent ouvertes pour les critères explicités au backlog. Voir frontend-host-validation.json et frontend-host-review.md.
