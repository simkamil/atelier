# Epic 1.2 Context: Mise en page

<!-- Compiled from US-V0-V1.md and SPEC.md because planning artifacts are absent. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Lay out rich text deterministically into lines and pages, with paragraph geometry and text controls that keep the result close to Word behavior across desktop and web. Planning artifacts (PRD, architecture, UX/design, and product brief) were absent, so this context is limited to the user stories and product specification.

## Stories

- Story 210: Mise en page des lignes
- Story 211: Tabulations
- Story 212: Pagination
- Story 213: Mise en page incrémentale
- Story 214: Géométrie du curseur
- Story 215: Bordures et trame de paragraphe

## Requirements & Constraints

Line layout uses greedy Unicode line breaking, alignment, paragraph indents, and line spacing. It must account for mixed fonts and sizes. Tabs align text at left, center, right, decimal (French comma), and bar stops, with dotted, dashed, or solid leaders; default stops repeat every 1.25 cm. Pagination handles paragraph spacing, widow/orphan control, keep rules, and page breaks. Incremental results must match full layout, with a typing target below 16 ms in a 300-page document. Cursor geometry maps text positions to rectangles and points to nearby positions, including ligatures and multi-line selections. The wider product targets at least 95% agreement with Word line breaks and page counts on reference corpora; Microsoft Word may be unavailable on the development host, so report comparison evidence separately from deterministic local behavior.

## Technical Decisions

Input is the resolved rich-text model and styles; output is a page/line/glyph-fragment tree plus cursor geometry. Layout is deterministic and platform independent, uses compatible metric fonts, and is incremental at paragraph and page boundaries. The renderer consumes positioned glyphs through a per-page display list. Keep calculations checked and resource-bounded; workspace code has no external dependencies and prohibits unsafe code outside FFI crates.

## Cross-Story Dependencies

Line layout depends on shaping and resolved paragraph/character styles. Tabs depend on line layout; pagination depends on line layout and feeds incremental layout; cursor geometry consumes line and page geometry. Paragraph borders and shading build on paragraph layout and rendering.
