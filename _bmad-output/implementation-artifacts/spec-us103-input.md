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
