---
title: 'US-220/226 — persistent DOCX editing session prerequisite'
type: feature
created: '2026-10-14'
status: done
route: dispatch
baseline_commit: a5a50ae3f488eb95662181776fdc03ef7d6a2e6b
context: []
---

<frozen-after-approval reason="Existing authorization to continue MVP development in parallel">

## Intent

**Problem:** DOCX preview imports then discards the rich document. Editing the flattened TXT would lose the structure and formatting needed by the forthcoming DOCX writer.

**Approach:** Retain the imported document in an independent Rust rich session. Support replacement within one paragraph, selection, undo and redo, and render from the retained model. This is a tested engine prerequisite for US-220/226; UI editing, save integration and full story completion remain separate work.

## Boundaries & Constraints

**Always:** Preserve the OPC package, style catalog, theme, fonts, other paragraphs and table metadata. Reuse rich operations and existing renderer. Byte offsets must be extended grapheme boundaries. Each rejected edit/history/render operation must leave semantic document, revision, selection and history unchanged. Keep the existing TXT and preview protocols unchanged. Original imported bytes are never written. Run-style references, break offsets and symbol offsets must follow edits and undo exactly. New content inherits preceding character formatting (first character at offset zero); do not flatten effective properties into direct formatting. Work in `C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau`, shell `cmd.exe`, `login:false`. Do not modify the other developer's writer or main checkout.

**Never:** Cross-paragraph replacement, split/join paragraphs, structural mutation, native file writes, new UI/protocol opcodes or DOCX export in this lot. Reject CR/LF insertion; tab insertion remains text. No claim that package preservation implies a validated DOCX round trip. No third-party product dependency, no relaxed limits.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Retained import | Bounded valid DOCX | Initial rendering equals existing preview, original package retained | Invalid or over-limit import fails |
| Replace | Paragraph ID, grapheme range, UTF-8 text, expected revision | One edit, collapsed caret after insertion, incremented revision, new rendering | Unknown paragraph, stale revision, invalid boundaries and newline rejected atomically |
| Styles | Multiple character styles and direct formatting around insertion/deletion | Remaining styles follow their source text, insertion inherits preceding formatting | No stale style spans or dropped unrelated metadata |
| Source controls | Imported break or symbol before/inside/after replacement | Shift surviving records, remove records for deleted source characters | Undo restores exact metadata |
| History | Replace, undo, redo, then new edit | Exact paragraph/metadata/selection restoration; new edit clears redo | Empty navigation is no-op; bounded history rejects oversized edit before mutation |
| Render failure | Edit would exceed text/layout/render ceilings | Error, unchanged session including history and revision | Same deterministic existing renderer limits |

</frozen-after-approval>

## Code Map

- `crates/core-wasm/src/rich_preview.rs`: existing import limits and ATDL rendering; extract `import_preview_docx(bytes)` and `render_imported_docx(&mut ImportedDocx)`; preserve `preview_docx(bytes)` wrapper behavior.
- `crates/fmt-ooxml/src/docx.rs`: ImportedDocx owns package, model, style engine, run_styles, breaks, symbols; paragraph IDs stable in this scope. Do not edit importer source.
- `crates/text-model/src/rich/ops.rs`: RichOp insertion/deletion, inverse and paragraph subtree restore; reuse for paragraph mutation.
- `crates/core-wasm/src/rich_session.rs`: new persistent session, public engine API; no integration into EditorSession in this lot.
- `crates/core-wasm/src/lib.rs`: export new module only.
- `crates/core-wasm/tests/rich_preview.rs`: existing preview regression suite.
- `crates/core-wasm/tests/rich_session.rs`: independent integration acceptance tests with synthetic OPC fixtures and real basic.docx.

## Tasks & Acceptance

**Execution:**
- [x] `crates/core-wasm/src/rich_preview.rs` — extract import/render seams with unchanged preview output.
- [x] `crates/core-wasm/src/rich_session.rs`, `src/lib.rs` — implement retained session and checked replacement/history/render.
- [x] `crates/core-wasm/tests/rich_session.rs` — verify every matrix scenario including style identity and package bytes.
- [x] `docs/us-220-rich-session.md`, `US-V0-V1.md`, `docs/PROTOTYPE-PROGRESS.md` — document evidence and explicit remaining UI/save scope without marking full US done.
- [x] `graphify-out/*` — refresh architectural map preserving documentary concepts and source locations.

**Acceptance Criteria:**
- Given a DOCX with styled runs, when text is edited and undone, then the rendered pages and paragraph metadata return exactly to their initial values.
- Given an existing TXT editor session, when the independent rich session is used, then no TXT protocol or state changes occur.
- Given documented validation commands, when run on the completed source, then host/WASM checks and affected regressions pass.

## Implementation Notes

User authorized continued development and parallel Luna medium agents. No additional spec approval requested. Parallel file ownership is recorded in companion specs. API signature is agreed before tests. History budget is a conservative logical retained-data bound, not an allocator measurement. Capture exact baseline with Git before implementation.

Completed validation: 9 checks, 70 Rust tests including 15 session and 2 retained-render tests, 1,174 frozen product inputs unchanged throughout. Rebuilt embedded WASM: 4,226,606 bytes, SHA-256 `70e2bba4cb59d5c4b0f10877442ca302610ec462c4fc47a495c3d608e7e9b4d9`; source inventory, artifact equality, existing size ceiling and editor protocol pass. Canonical graph: 9,687 nodes / 24,753 edges, 34 Cargo packages, no dependency cycle/dangling endpoint/missing located source; 70 documentary baseline nodes and 56 edges retained. No full workspace/UI/Word validation is claimed. The engine prerequisite is done; US-220 and US-226 remain open.

## Spec Change Log

## Review Triage Log

| Finding | Verdict | Evidence / disposition |
|---|---|---|
| Blind 1: oversized replacement allocated before text ceiling | medium | Corrected: reuse renderer text cap, check incoming length and projected document bytes including separators before cloning; large/projected-limit regressions pass. |
| Blind 2: valid empty DOCX has no paragraph | false | Import starts from RichDocument with mandatory section paragraph, retained for empty body (docx.rs:520-535); empty import/tab regression verifies successful session. |
| Blind 3: no-op failure discards caret movement | false | Rejected operations must preserve selection; render-before-cursor is required atomicity, not a defect. |
| Blind 4: cache mutation can alter rollback rendering | false | StyleEngine caches only immutable style bases keyed by paragraph/style IDs; direct formatting overlays each resolve (styles.rs:368-409). This lot changes no style definition, table style or paragraph property; rollback preview equality and redo are tested. Cache metrics are not semantic state. |
| Blind 5: transient history clone uncounted | false | Explicit documented bound is logical retained history, not transient or allocator memory. No claim that candidate copies fit inside it. |
| Blind 6: allocator capacities uncounted | false | Same explicit logical bound; structure and owned payload lengths are checked, no actual allocator usage guarantee. |
| Blind 7: breaks may be boundary markers | false | Import records offset immediately before pushing newline or symbol scalar (docx.rs:959-992); interval deletion removes exactly the deleted source character. Shift/delete/exact undo tested. |
| Blind 8: inherited defaults not copied | false | Deliberate preservation of style inheritance: renderer resolves document/paragraph defaults and overlays direct attrs; copying defaults into direct attrs would flatten formatting. Visible inherited color regression added. |
| Blind 9: Unicode format controls accepted | false | Ordinary Unicode remains source text; unsupported line/control insertions are explicitly rejected. No requirement to filter all Unicode format controls; rendering limitations documented. |
| Gap 1: no visible assertion for inserted style | medium | Added decoded ATDL glyph color order: inserted x is red and surviving CD blue; keeps model and undo/redo checks. |
| Gap 2: validation receipt not yet present | low | Parent validation runner now emits receipt/log with frozen inputs and actual results before completion. |

Edge-case reviewer returned no findings. All three reviewers were context-free GPT-6 Luna medium per the user's continuing model preference. The blind review returned nine candidates; every one is triaged above. No confirmed defect is deferred.

## Verification

- `cargo test -p atelier-core-wasm --locked --offline`
- `cargo clippy -p atelier-core-wasm --all-targets --locked --offline -- -D warnings`
- `cargo check -p atelier-core-wasm --target wasm32-unknown-unknown --locked --offline`
- `cargo fmt --all -- --check`
- Graphify code extraction and canonical graph receipt after final source changes.
