<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-101 — Canvas renderer

`web/display-list.mjs` exports `decodeDisplayList(bytes)`. It accepts an
`ArrayBuffer` or `Uint8Array`, decodes the complete ATDL 1.0 stream and returns
`{ page, commands }`. Unsigned 64-bit resource and structure identifiers remain
JavaScript `BigInt` values. `DisplayListError.kind` distinguishes `format`,
`value`, `geometry`, `state`, and `limit` diagnostics. Invalid UTF-8, unknown
values, unbalanced stacks, truncation, excessive budgets, and trailing bytes
are rejected before a list is returned.

`CanvasRenderer(canvas, options)` replays decoded lists on a real 2D Canvas.
Call `setPages(lists)` to replace the document and `setViewport({width, height,
zoom, scrollX, scrollY, dpr})` when viewport state changes. Pages are stacked
vertically with the configurable `pageGap` in CSS pixels. Page coordinates are
scaled by `zoom`; viewport scroll values and underline rectangles are CSS
pixels. The backing store dimensions are CSS dimensions multiplied by DPR, so
1, 1.5, and 2 are supported. Zero-sized viewports draw no pages. `render(rect)`
clears and repaints a CSS-local dirty rectangle, replaying intersecting
commands in original order. Passing no rectangle redraws the full viewport.
`setOverlay(rectangles)` draws CSS-local underlines; `clearOverlay()` erases
them. Text shaping and IME composition remain outside this module.

Glyph resolvers have the synchronous signature
`resolveGlyph(fontId: bigint, glyphId: number) -> { path: Path2D, bounds? }`.
The returned outline must use page units relative to the glyph origin; font
unit conversion, shaping, and origin normalization belong to the provider.
Images use `resolveImage(imageId: bigint) -> CanvasImageSource`. A missing,
throwing, or invalid resource produces an `onDiagnostic` developer diagnostic
and the renderer restores Canvas state before continuing. Glyph Path2D values
are held in a bounded LRU cache keyed by `(font_id, glyph_id)`; call
`invalidateResources()` when provider contents change. Glyph runs are kept in
dirty-region replay until providers can consistently supply conservative
outline bounds.

`tests/display-list.mjs` compares JS decoding against bytes emitted by the
independent Rust `display_list_oracle` example and checks malformed streams.
`tests/canvas-browser.html` imports `runCanvasChecks()` from
`tests/canvas-browser.mjs`; that function returns successful scenario records,
including the measured 300-page benchmark duration. A headless software Canvas
result verifies browser behavior but does not establish 60 fps on physical
hardware. The root browser runner is responsible for collecting and reporting
these results.

Validation finale du lot : [34 contrôles fonctionnels et qualité](../_bmad-output/implementation-artifacts/frontend-host-validation.json),
**318 tests workspace et 27 tests shell release**, sur sources et lock inchangés.
Les tests Chromium couvrent Canvas, saisie et leur composition intégrée.
Les deux profils WebView2 accomplissent 100 cycles fonctionnels avec callbacks équilibrés ;
le rapport conserve séparément l'échec strict des handles et US-111 reste ouverte.
La [revue indépendante](../_bmad-output/implementation-artifacts/frontend-host-review.md)
conserve un point IME natif de gravité moyenne non vérifiée à trancher sur matériel.
Les tests musl sont compilés localement ; leur exécution appartient à la CI Linux.
