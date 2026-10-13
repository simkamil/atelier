---
title: 'US-341 — Moteur et codec du remplacement'
status: in-progress
context:
  - _bmad-output/implementation-artifacts/spec-us341-us344.md
---

Read the context spec fully; implement only its engine/codec task. Working directory C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau. Own `crates/core-wasm/src/editor.rs`, `editor_queries.rs`, optional new private `editor_replacement.rs` and its lib.rs registration, new `crates/core-wasm/tests/replacement.rs`, `web/editor-protocol.mjs`, new `tests/replacement-protocol.mjs`, and `_bmad-output/implementation-artifacts/replacement-wire-contract.md`. Preserve other APIs and do not edit text-model/lib.rs or external navigation/plain_io modules. No app/editor frontend/labels/native changes, no skills/subagents/Graphify/commits. Every exec_command shell:'cmd.exe',login:false,explicit workdir (PowerShell hangs); read files with `py target/read-sections.py path:1:200`. Implement the exact opcodes/frame contract from shared spec; report bounded performance and transaction behavior. Run core-wasm focused/full package tests, new Node codec tests, fmt and core Clippy as appropriate, not browser/workspace-wide. Parent integrates and validates whole lot.
