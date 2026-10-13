---
title: 'US-344 — Barre d’état configurable'
status: in-progress
context:
  - _bmad-output/implementation-artifacts/spec-us341-us344.md
---

Read shared spec fully; implement only independent `web/document-status.mjs`, `.css`, `tests/document-status-browser.mjs`, exact contract and labels. Own page/language/view/saved fields; words/zoom stay external and visibility callback tells parent to hide/show their slots. Scope page dialog to visual navigation, no business/selection mutations. Reuse createDialog/createContextMenu from ui-components.mjs, inspect public API; do not edit shared component. Make menu items checkbox semantics and keyboard accessible, named target allows Shift+F10; menuTarget defaults to element. No preferences beyond session. Responsive tokens/forced colors; no hardcoded user-facing strings. Tests export async run({assert,fixture}); parent registers. No edits to app/editor/preview/catalog/runners/native/Rust. No skills/subagents/Graphify/commits. Every exec_command shell:'cmd.exe',login:false,workdir C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau, read `py target/read-sections.py path:1:200`. Run syntax/focused checks only. Cover page boundaries, errors, modal focus, menu visibility states/callbacks, missing language, preview read-only/busy, duplicate renders and disposal. Report files/results/issues.
