---
title: Accessible ribbon and themes for the Windows MVP
type: feature
created: 2026-10-12
status: done
route: dispatch
review_loop_iteration: 0
baseline: 72874af6736729c52b715103108b77cab26ba215
baseline_commit: 72874af6736729c52b715103108b77cab26ba215
---

<frozen-after-approval reason="User authorized a complementary multi-US lot and parallel Luna medium development">

## Intent

Deliver US-104, US-105 and US-106 as a coherent editor shell: reusable accessible controls, six declarative ribbon tabs, and light/dark/system themes. Preserve the working TXT editor and read-only DOCX preview. US-102 belongs to the other developer; use caller-provided labels and the existing catalogue, with no competing translation service.

## Boundaries & Constraints

Always use vanilla ES modules, native HTML where appropriate, local assets and no external dependencies. Preserve Atelier's Carlito typography and green accent. Every widget has a visible focus indicator, an accessible name and documented keyboard behavior. Dispose listeners, observers and popup DOM. Use textContent for labels. Keep document ATDL colors and white paper unchanged across themes.

Never implement rich document editing, PDF, search, metadata or import features assigned elsewhere. Unsupported ribbon commands stay disabled with an explanation. Never edit the other task's checkout. Only the parent touches shared app/HTML/CSS/catalogue/resources/build/docs files, commits and publication. Three Luna medium implementers own disjoint new files; fresh independent reviewers follow integration. The user explicitly requested several complementary US and parallel work, overriding workflow defaults that would serialize this lot or request another scope approval.

## I/O & Edge-Case Matrix

| Given / when | Then |
|---|---|
| Native button or toggle activated | Exactly one callback; disabled controls do nothing; pressed state is exposed. |
| Split button, dropdown or context menu opened | Correct popup roles/relationships; arrows, Home/End, Enter/Space, Escape and outside dismissal work; focus returns appropriately. |
| Font combo with keyboard input or IME | Filtered font previews and valid active descendant; composition does not trigger premature selection. |
| Color picker or number/unit control used | Named theme/standard/custom colors; keyboard operation; finite bounded numeric values and unit callbacks; invalid drafts do not produce valid changes. |
| Modal dialog or tooltip opened | Modal traps focus and restores it on close; tooltip works on focus/hover and Escape, preserving prior descriptions. |
| Tab arrows/Home/End used | One tab stop, synchronized selected state and labeled visible panel; disabled tabs skipped. |
| Theme changed, storage fails or OS preference changes | Immediate palette update; system follows live media events; storage errors are harmless; invalid stored preference falls back to system. |
| Forced colors or narrow viewport active | Controls and focus remain readable, groups reduce without losing commands or overflowing the shell; document colors remain true. |
| Bare Alt/keytips or Ctrl+F1 used | Tabs/commands accessible and ribbon collapses; Escape restores previous focus; AltGr, composition and modified shortcuts are preserved. |
| Busy/loading/preview state or disposal | Existing button states remain authoritative; no unavailable command dispatch and no late handlers after disposal. |

</frozen-after-approval>

## Code Map

- `US-V0-V1.md` US-104/105/106 and `SPEC.md` D11, 6.14/6.15 define acceptance.
- `web/app.mjs`: existing catalogue and node references, state-driven disabling, preview suspension, lifecycle. Move existing command elements; preserve IDs/listeners.
- `web/index.html`, `web/app.css`: shell and inline toolbar; keep document workspaces intact.
- `web/canvas-renderer.mjs`, `web/rich-preview.mjs`: document rendering, unchanged.
- `crates/desktop-win/src/resources.rs`: explicit embedded routes; add every new production resource.
- `tests/run-editor-browser.mjs`, `tests/editor-browser.mjs`: real Edge/Worker/WASM regression harness.
- `graphify-out/GRAPH_REPORT.md`: frontend/native resource dependency clusters; fresh AST extracted into ignored target/ui-research-graph, 7223 nodes / 20135 edges, zero LLM tokens.

## Tasks & Acceptance

- [x] Components agent: new `web/ui-components.mjs`, `web/ui-components.css`, `tests/ui-components-browser.mjs`; implement all eleven controls and real DOM tests. Factories return controllers with `element` and `dispose`; labels/options supplied by caller. Export `attachTabs({tablist,tabs:[{tab,panel}],orientation,onSelect})`, returning `select(index)`, `dispose`; reused by ribbon.
- [x] Themes agent: new `web/theme.mjs`, `web/theme.css`, `tests/theme.mjs`, `tests/theme-browser.mjs`. Export `createThemeController({root,storage,matchMedia,onChange})` with `getTheme()`, `getResolvedTheme()`, `setTheme(light|dark|system)`, `dispose()`. Set `data-theme` to resolved palette and `data-theme-preference` to preference. Persistence key `atelier.theme`; default system. CSS defines semantic chrome tokens and forced-colors rules; never filters/recolors Canvas.
- [x] Ribbon agent: new `web/ribbon.mjs`, `web/ribbon.css`, `tests/ribbon-browser.mjs`. Export `createRibbon({root,tabs,onCommand,labels,initialTab})`; tabs contain `{id,label,keyTip,groups:[{id,label,keyTip,commands:[{id,label,keyTip,element?,disabled?}]}]}`. Move supplied elements, do not clone them. Return `selectTab(id)`, `setCollapsed(bool)`, `refresh()`, `dispose()`. `onCommand(id)` only for internally created commands; supplied elements retain their existing listeners. `refresh` reconciles visibility/disabled state. Reuse attachTabs, responsive groups and collision-free two-level keytips. Labels include collapse/expand/navigation from catalogue.
- [x] Parent: integrate existing command elements, six tabs, theme selector and component demonstration/settings dialog in `web/app.mjs`, HTML/CSS/catalogue; register native routes and browser routes; test TXT/DOCX preservation.
- [x] Parent: add browser runner/page and independent review, update docs and checkboxes only with evidence, regenerate Graphify after structure changes, validate/build/package and publish the isolated branch without overwriting concurrent work.

Acceptance: Given the shipped shell, when a user types, opens/saves TXT, views/closes DOCX, changes theme or navigates the ribbon, then current document/selection/history remain intact and implemented commands retain their behavior. Given unsupported commands, when navigating, then they are visibly disabled. Given the eleven component examples, when tested using keyboard and real DOM, then the matrix passes. Given manual assistive-technology checks are unavailable, then docs distinguish automated evidence from NVDA/Narrator verification.

## Implementation Notes

Planning: clean managed worktree; baseline above. No irreversible operation or unresolved product choice. Existing catalogue is an injected boundary with US-102; its completion is not claimed here. Official WAI APG toolbar/tabs/modal patterns consulted; focus visibility guidance corroborated using ui-ux-pro-max. Preserve existing light palette (#172a2a ink, #5f716d muted, #236c57 green, #dce5df border, #fff paper, #f8faf8 chrome); theme tokens extend it. Dense editor chrome, clear labeled groups, no decorative animation. Component gallery demonstrates controls without pretending to format the read-only DOCX.

## Spec Change Log

## Review Triage Log

Three fresh GPT-6 Luna medium reviewers were launched together against the same 156,619-byte unified diff (including untracked files), before triage. No reviewer modified code.

| Finding | Verdict / route | Evidence and decision |
|---|---|---|
| Blind 1: light toolbar/workspace in dark theme | false / rejected | The cited app.css uses chrome tokens on toolbar, workspace and borders; theme.css overrides them. Actual dark screenshot shows dark surfaces. Reviewer appears to have read removed lines. |
| Blind 2: supplied command cannot re-enable | medium / patch | refresh applies only a truthy disabled spec at line 224. An explicit true→false leaves the DOM disabled; apply defined booleans both ways. |
| Blind 3: font choices cannot open without typing | medium / patch | ArrowDown handler only runs when popup is already open. Open suggestions on ArrowDown from closed. |
| Blind 4: unknown unit setter accepted | medium / patch | setValue records nextUnit even when select has no matching option. Reject unknown configured units. |
| Blind 5: numeric step uses zero base | medium / patch | increments=number/step rejects .5 with min .5 and step 1, although the native input accepts it. Align step base. |
| Blind 6: invalid color setter | low / patch | choose stores any string while input coerces unsupported strings to black. Reject unsupported values without callback. |
| Blind 7: tooltip description detached at focus | medium / patch | aria-describedby is assigned before timer appends the description. Keep the node connected and toggle hidden visibility. |
| Blind 8: context menu beyond viewport | medium / patch | open uses raw x/y with no clamp. Edge coordinates hide selectable commands. Clamp after layout. |
| Blind 9: dialog skips contenteditable | medium / patch | focusables omits editable nodes; boundary cycling can skip an editable last control. Include rendered editable controls. |
| Blind 10: disabled state not restored on dispose | low / patch | Setup/refresh can override caller disabled, but dispose never restores it. Preserve state for ribbon-owned overrides only. |
| Edge 1: supplied disabled true→false | medium / patch | Independently confirmed same defect as Blind 2; retain this individual verdict, grouped for one fix. The reviewer's guard_snippet is the proposed correction, not existing code. |
| Edge 2: numeric defaults have no finite bounds | false / rejected | Bounds are optional native HTML constraints; the API advertises min=-Infinity/max=Infinity, rejects nonfinite values, and honors caller finite bounds. No caller or acceptance example requires mandatory finite limits. |
| Gap 1: standalone UI suite absent from CI | medium / patch | CI ran editor tests but skipped the only dropdown Escape regression. Add Node theme and real Edge UI suites, including forced colors, to normal Windows CI. |
| Parent: stale font active descendant and blur popup | medium / patch | Filtering removes option IDs without clearing aria-activedescendant; popup stays after focus leaves. Clear relationship and dismiss on leaving. |
| Parent: font selection contrast | medium / patch | ui-listbox-option selection uses #eaf2ed with inherited dark-mode light ink. Replace with semantic foreground/background. |
| Parent: tooltip hover persistence and viewport | medium / patch | Trigger mouseleave removes tooltip before pointer can inspect it; raw left can overflow. Retain during related hover/focus and clamp. |
| Parent: dialog includes hidden popup descendants | medium / patch | Filtering only node.hidden includes buttons in hidden ancestor popups; cycling can focus invisible choices. Filter rendered/inert nodes. |
| Parent: supplied class/data restoration | low / patch | original snapshots are stored after class/data mutation; dispose leaks ribbon metadata into restored controls. Capture before mutation. |
| Parent: collapsed keytip targets hidden command | medium / patch | showCommandTips never expands a collapsed body; activation calls focus/click on hidden commands. Reveal selected tab commands before navigation. |
| Parent: editor shortcuts behind modal | medium / patch | Global keydown handlers can call hidden/inert background buttons through JS click while Appearance is modal. Ignore editor/ribbon shortcuts when an external native modal is active; integration checks preserve state and native call count. |
| Parent: CDP screenshot commands unbounded | low / patch | The initial report timeout cannot reject later CDP promises after it has resolved. Bound connection, fetch and command waits and reject pending requests on disconnect. |
| Parent: trigger pointerdown reopens menu | medium / patch | makeMenu outside dismissal treated its own trigger as outside; a physical pointerdown closed it before click reopened it. Exclude the private owner and test pointerdown+click. |
| Parent: dropdown popup association | medium / patch | Expanded state had no aria-controls and reopened at first rather than selected option. Link the popup ID and selected enabled option; update Space test to select Home explicitly. |
| Parent: forced-colors palette assertion | false / corrected test | Exact dark RGB is intentionally replaced by system colors. Assert Highlight/HighlightText under forced colors; both browser modes pass. |
| Parent: native diagnostic log rotation | medium / patch | Startup and 28 explicit resource probes exceed the intentional 64-entry ring. The smoke falsely fails even though every required security/engine probe is retained. Keep all mandatory-entry assertions, report rotation count, and retain bounded-log unit coverage. |
| Parent: forced-colors primary label backplate | medium / patch | Real forced-colors screenshot showed automatic text backplates obscuring the primary button and mark labels. Use explicit Highlight/HighlightText pairs with forced-color-adjust:none for these two surfaces, then inspect new screenshots. |

All surviving defects are direct corrections to existing branches/state capture and introduce no public API. Re-engaged the original components and ribbon implementers; parent owns CI and final verification.

## Verification

Completed 2026-10-13: `ui-shell-validation.json` passes 17 gates with 1,042 immutable inputs. 930 Windows Rust tests passed (2 optional local-corpus tests ignored); after the final CSS-only fix these results were retained against 964 unchanged Rust/data/fixture inputs, and native resource tests were rerun against final embedded bytes. Edge reports 112 UI assertions, 114 forced-colors assertions, 118 editor regressions and 36 Canvas/input assertions. Release WebView2 exercises 28 routes, Worker/WASM editing and CSP refusals; callback objects return 0→0. All confirmed review defects were corrected; nothing deferred. Physical keyboard and screen-reader evidence remains distinct, as declared in the frozen acceptance.

Light/dark/420px and forced-colors screenshots were inspected by the parent; the latter found and verified the primary-label backplate correction. `ui-shell-graph.json` verifies 7,763 nodes / 20,220 simple edges, all 158 story statuses (51 done), 70 preserved baseline semantic documentation nodes, no missing source file or dangling endpoint, and 34 actual Cargo packages with no dependency cycle. 887 endpoint groups aggregate relations in the simple view; the local raw extraction retains those relations. Graph export includes all nodes, with an 8,000-node cap. This AST/API refresh uses no LLM tokens; historical usage is explicitly sourced to the baseline report because ignored cost.json is absent in this worktree.

Publication: `codex/ui-components-ribbon` from the recorded baseline, separately from the other task's checkout. User's persistent push authorization applies; remote CI results are reported separately from local validation.

Node theme and existing frontend helpers; real Edge component/ribbon/theme tests at wide/narrow viewports and forced-colors; existing Worker/WASM editor scenarios; Windows native resource tests/build and release executable; fmt/clippy/license/local-dependency guard; immutable input inventory and reproducible engine package checks. Read source deltas against baseline; report CI availability separately. Never claim synthetic keyboard events validate physical AZERTY, NVDA or Narrator.
