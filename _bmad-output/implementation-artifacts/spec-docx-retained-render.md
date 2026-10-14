---
title: 'Retained DOCX renderer seam'
type: refactor
created: '2026-10-14'
status: done
route: dispatch
baseline_commit: a5a50ae3f488eb95662181776fdc03ef7d6a2e6b
context: []
---

## Intent

Implement the rendering component of `spec-docx-edit-session.md` (read it fully for constraints). Own only `crates/core-wasm/src/rich_preview.rs` and new regression tests `crates/core-wasm/tests/retained_preview.rs`. Other agents own session and acceptance tests; do not edit their files, lib.rs or the original rich_preview test file. Repo C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau; commands shell cmd.exe login:false. Do not commit/stage/graphify or run workspace-wide fmt. Parent handles coordination and docs.

## Implementation Notes

Extract public `import_preview_docx(bytes: &[u8]) -> Result<ImportedDocx, PreviewError>` with exactly existing preview import limits. Extract public `render_imported_docx(imported: &mut ImportedDocx) -> Result<RichPreview, PreviewError>` rendering retained model with same limits. `preview_docx(bytes)` remains import-then-render and outputs exactly same streams. Renderer must see text edits to retained model; no re-import of package bytes. Paragraph structure does not change in this lot. All text limits including separators must still hold even for manually edited retained models. Preserve existing renderer behavior and budgets, no new dependency. Add regressions proving identical wrapper/seam results on reference DOCX and retained text mutation reflected while package unchanged. Format only owned Rust files with rustfmt edition2024. Run targeted tests if possible, coordinate cargo lock naturally. Read graphify-out/GRAPH_REPORT.md before analysis; parent already refreshed extraction. Report tests and limitations, update this spec status when done.

## Completion

- [x] Extract import and retained-render seams, preserving `preview_docx` output.
- [x] Add seam parity, retained text mutation, and package-preservation regressions.
- [x] Format owned Rust files and run targeted integration suites.
