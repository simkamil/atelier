---
title: 'US-341 — Contrôles Remplacer'
status: in-progress
context:
  - _bmad-output/implementation-artifacts/spec-us341-us344.md
---

Read the shared spec fully; implement only independent view `web/replace-controls.mjs`, `.css`, `tests/replace-controls-browser.mjs`. Follow exact contract/label names. Preserve textarea focus/value when setState updates; busy disables actions but close belongs to parent pane. Use textContent, existing theme tokens and component styling. Tests export async run({assert,fixture}) like existing search-pane/browser suite; parent registers them. No edits to app/editor/navigation-tools/search-pane/catalog/runners/Rust/docs beyond these owned files. No skills/subagents/Graphify/commits. Every exec_command shell:'cmd.exe',login:false,workdir C:/Users/simka/.codex/worktrees/ui-components-ribbon/mon-bureau; read `py target/read-sections.py path:1:200`. Run syntax checks and only focused tests available; parent runs complete browser suite. Include empty/newline/markup replacement, count zero/one/many, disabled/busy/error, focus and disposal assertions. Report changes/results/remaining issues.
