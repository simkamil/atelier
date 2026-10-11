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
