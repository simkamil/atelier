---
title: 'Persistent rich DOCX editing and history'
type: feature
created: '2026-10-14'
status: done
route: dispatch
baseline_commit: a5a50ae3f488eb95662181776fdc03ef7d6a2e6b
context: []
---

## Intent

Implement the session component of `spec-docx-edit-session.md` (read fully). Own only new `crates/core-wasm/src/rich_session.rs`, its `pub mod rich_session` declaration in lib.rs, and optional unit tests inside your module. Another agent owns rich_preview.rs; parent owns integration tests/docs/graph. Worktree C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau, shell cmd.exe login:false. Do not stage/commit/graphify or fmt other files.

## Implementation Notes

Required public API (coordinate any change with parent before implementation): `RichSession::open(bytes: &[u8]) -> Result<Self, RichSessionError>`; `imported(&self) -> &ImportedDocx`; `revision(&self) -> u64`; `selection(&self) -> RichSelection`; `can_undo/can_redo(&self) -> bool`; `preview(&mut self) -> Result<RichPreview, RichSessionError>`; `replace(&mut self, expected_revision: u64, para: NodeId, start: usize, end: usize, text: &str) -> Result<RichPreview, RichSessionError>`; `undo/redo(&mut self, expected_revision: u64) -> Result<RichPreview, RichSessionError>`; `select(&mut self, expected_revision: u64, selection: RichSelection) -> Result<(), RichSessionError>`. RichSelection is Copy/Eq with public `paragraph: NodeId, anchor: usize, focus: usize`. Initial selection first paragraph at zero, revision0; successful non-noop edit/undo/redo increments revision; selection moves/noops do not. Error enum categories include StaleRevision, InvalidSelection, InvalidText, Limit, Model, Preview (payloads your choice). Add `open_with_history_budget(bytes, budget_bytes)` for budget rejection tests (default4MiB, hardcap4MiB). State changes only after successful render; use candidate RichDocument and metadata snapshots with swap/rollback or equivalent. Avoid cloning package/font buffers per keystroke.

Rendering agent supplies `rich_preview::import_preview_docx(bytes) -> Result<ImportedDocx, PreviewError>` and `render_imported_docx(&mut ImportedDocx) -> Result<RichPreview, PreviewError>` shortly. Retain original ImportedDocx package/style/theme/fonts/table metadata; no importer source edits. Use rich operations on cloned model and restore exact para subtree for history. Snapshot paragraph including inline objects plus per-paragraph run_styles, breaks and symbols; immutable other metadata stays shared/retained. Bound history conservatively by actual encoded RichOp snapshot cost + heap strings in metadata + entry overhead for both directions; do not count only text. Undo/redo metadata must restore exactly. New edit clears redo after success only; no-op must preserve redo. For run-style insertion use preceding grapheme style, first grapheme style at offset0; split/shift/delete ranges and normalize. Remap surviving break/symbol offsets; delete records whose characters removed. Reject CR/LF and invalid grapheme boundaries before mutation. Reject new invisible source-control markers if necessary, document restriction; tabs allowed. Atomic failure must preserve semantic model, selection, revision, history and metadata even after renderer fails. Existing style-cache mutations may be harmless but must not alter future rendering on rollback. No UI/protocol changes, no save, no split/join. Read graph report first. Format only owned files. Run targeted core tests and return evidence. Update this spec when done.

## Completion

Implemented `RichSession` with retained import, grapheme-checked paragraph replacement, selection, undo/redo, source metadata remapping, and rollback when candidate rendering fails. The history cost bound accounts for rich paragraph and inline data, character and paragraph formatting strings, retained source metadata strings and structure, and entry overhead in both directions. It is a conservative logical bound, not an allocator measurement; `RichOp`'s journal codec is unavailable.

Verification: `cargo test -p atelier-core-wasm --lib --locked --offline` passed (12 tests); `cargo test -p atelier-core-wasm --test rich_session --locked --offline` passed (13 tests); `cargo test -p atelier-core-wasm --test rich_preview --test retained_preview --locked --offline` passed (10 tests); `cargo clippy -p atelier-core-wasm --lib --locked --offline -- -D warnings` passed; `rustfmt --edition 2024 --check crates/core-wasm/src/lib.rs crates/core-wasm/src/rich_session.rs` passed.
