---
title: 'US-224 — déplacer et copier une sélection dans le démonstrateur TXT'
type: feature
created: '2026-10-12'
status: done
route: dispatch
baseline_commit: 76b1ab0771ea32c8edf72a39e0a2cbb26c3c8997
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

Le texte sélectionné doit pouvoir être déplacé à la souris, copié avec Ctrl et déposé depuis une autre application. Le démonstrateur TXT doit offrir un indicateur d'insertion et une seule opération Annuler pour chaque dépôt. Le lot est autorisé par la demande persistante de poursuivre les US en parallèle, avec documentation et publication après validation.

## Boundaries & Constraints

**Always:** conserver les offsets UTF-8, le texte exact et le BOM; capturer la révision et la sélection source, rejeter les gestes périmés, ne modifier aucun état pour une sonde de position; transactions isolées de la frappe; Ctrl déterminé au relâchement; préserver la sélection à l'annulation. Les raccourcis existants couper/copier/coller restent l'alternative clavier. À l'ouverture d'un dialogue, composition, annulation du pointeur ou destruction, libérer capture et indicateur. Les réponses asynchrones périmées ne peuvent ni déplacer le marqueur ni déclencher un dépôt.

**Never:** enrichir le modèle TXT, accepter HTML/fichiers depuis le dépôt, ajouter dépendances, modifier les worktrees externes, prétendre une certification native interapplication sans test utilisateur.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| move | Sélection UTF-8, cible avant/après | Texte déplacé, cible ajustée après retrait, un Undo/Redo | Échec atomique |
| copy | Ctrl au relâchement | Copie à la cible, source conservée, un Undo | Échec atomique |
| inside | Déplacement vers l'intérieur ou bord source | Aucun changement, aucun historique ajouté | Aucun |
| external | text/plain déposé sur une page | Insertion exacte à la cible, pas remplacement de la sélection | Bornes existantes |
| marker | Drag sur pages/zoom/défilement | Trait d'insertion visible, coordonnées cohérentes | Cacher si sonde échoue |
| selection | Clic dans sélection sans franchir 4px, clic ailleurs, double clic | Clic simple replace caret; sélection et double clic existants conservés | Aucun |
| lifecycle | Escape, cancel, busy, composition, dispose | Aucun dépôt et aucune capture/frame/indicateur résiduel | Erreur remontée sans mutation |
| stale | Révision/sélection changées, réponse de sonde tardive | Geste refusé, aucune modification nouvelle | EDITOR_STALE_EDIT |
| malformed | Offset non frontière, UTF-8 invalide, taille/flags/NaN invalides | Rejet sans texte/historique/révision modifiés | Erreur ATED stricte |

</frozen-after-approval>

## Code Map

- `web/mouse-selection.mjs`: capture de sélection existante, coordonnées page en points.
- `web/editor.mjs`: file de commandes, garde busy/composition, état et overlay.
- `web/editor-protocol.mjs`, `crates/core-wasm/src/editor.rs`: ATED v1, nouveaux opcodes ProbePoint=15 et DropText=16; réponse ATPT v1 pour sonde, aucune publication d'état pour celle-ci.
- `crates/text-model/src/lib.rs`: historique borné et transaction unique.

## Tasks & Acceptance

**Execution:**
- [x] `web/text-drag-drop.mjs`, `tests/text-drag-drop.mjs` — agent drag_text: contrôleur et tests ciblés. Aucun autre fichier détenu par cet agent. API attachTextDragDrop({canvas,scrollport,getGeometry,getSelectionSnapshot,isPointInSelection,probePoint,onDrop,onClick,onDragState,isEnabled,focus,onError}). Snapshot {text,anchor,focus,revision}; probePoint reçoit {page,x,y}, retourne Promise<{offset,revision,caret:{page,x,y,height}}> sans mutation. onDrop reçoit {kind:'move'|'copy'|'external',revision,sourceAnchor,sourceFocus,target,text}; target est offset UTF-8. onClick reçoit le résultat de sonde. onDragState reçoit caret ou null. Interception capture-phase seulement si isPointInSelection(clientX,clientY), sans Ctrl/Shift/Alt restrictions hors Ctrl copie, et detail===1; appeler stopImmediatePropagation pour éviter conflit. Coalescer les sondes, finaliser avec la position exacte du relâchement, autoscroll borné, empêcher le défaut des dépôts texte même lorsqu'ils sont refusés pour busy.
- [x] `web/editor.mjs`, `web/editor-protocol.mjs`, `crates/core-wasm/src/editor.rs`, `crates/text-model/src/lib.rs` — parent: sonde non mutante, transaction atomique, raccordement UI.
- [x] `web/app.css`, `crates/desktop-win/src/resources.rs`, `tests/editor-browser.mjs`, `crates/core-wasm/tests/editor_engine.rs`, `docs/PROTOTYPE-PROGRESS.md`, `US-V0-V1.md` — parent: indicateur embarqué, intégration réelle, suivi exact.

**Acceptance Criteria:**
- Given un éditeur initialisé, when un texte est déplacé/cop ié/déposé, then l'export et l'annulation suivent la matrice sur le WASM réellement embarqué.
- Given une sélection, when la souris sert à sélectionner ou à faire un double clic, then les parcours existants restent disponibles.
- Given le module embarqué, when les tests navigateur couvrent le geste, then l'indicateur et le résultat sont observés sans simuler la mutation du moteur.

## Implementation Notes

Modifier priority: Shift/Alt/Meta pointerdown is delegated to the existing selector; Ctrl alone controls copy at release. Root owns shared Rust/protocol/editor/UI and documentation; french_spacing also implements isolated new Rust test files and ABI tests. All three independent reviews ran before triage; generated graph integrity is checked separately.

## Spec Change Log

## Review Triage Log

| Finding | Verdict | Route | Evidence |
|---|---|---|---|
| blind-1 | medium | patch | HTML/file-only drop returns before preventDefault; browser fallback can navigate. Prevent every canvas drop default, insert only plain text, regression added by original implementer. |
| blind-2 | medium | patch | PUNCTUATION_RUN contains closing » and suppresses required spacing in «mot»:. Remove that exclusion for subsequent French punctuation and test it. |
| blind-3 | low | rejected | validBoundary does allocate a UTF-8 buffer per coalesced probe. Input is bounded at 128 KiB; no user latency regression was demonstrated. Snapshot caching adds lifecycle state for a negligible bounded allocation and is not introduced in this change. |
| blind-4 | medium | patch | The external drop path awaits onDrop but never focus(), unlike internal completion. Restore the existing hidden input focus after successful external insertion and test callback invocation. |
| blind-5 | false | rejected | Espace is feminine in French typography; « Espaces françaises automatiques » agrees with the noun and accurately labels the option. The alleged grammatical defect is absent. |
| blind-6 | medium | patch | The actual app initialization reads localStorage but browser assertions only covered writing it. Seed a saved disabled setting before initialization, then initialize a fresh module without a saved setting and verify enabled default. |
| blind-7 | low | patch | No multiple-ASCII-space case was present. Preserve earlier spaces, replace only the adjacent one, and leave paragraph/tab-leading whitespace runs unchanged; regression documents this existing narrow substitution contract. |
| blind-8 | low | patch | Controller Ctrl-copy already starts from reversed 5/1 selection, but direct model copy Undo/Redo only covered forward 1/4. Add reversed model copy restoration coverage. |
| blind-9 | false | rejected | The drag spec maps its own ProbePoint/DropText commands. The separate spacing spec and docs/us-228-french-spacing.md already map ReplaceRange=17. No protocol consumer loses that command; spec-only expansion is also excluded by review routing. |
| blind-10 | low | rejected | The « cop ié » typo is confined to the build spec acceptance text, not product/docs. The workflow explicitly rejects findings whose fix edits the build spec; runtime behavior and verification are unaffected. |
| edge-1 | false | rejected | ReplaceRange intentionally accepts UTF-8 character boundaries for an explicit range edit. Its helper replaces just-inserted punctuation/adjacent space and preserves every other scalar. Valid Unicode grapheme editing is not displayed-text corruption; no reachable user text-loss case was demonstrated. DropText separately enforces grapheme boundaries. |
| edge-2 | false | rejected | Shift+click must retain existing selection extension; Alt/Meta gestures must reach the existing selector. The guard prevents interception and the targeted regression verifies propagation. User intent does not require Shift/Alt copy-drag; Ctrl alone performs copy. The nonfrozen handoff wording was imprecise, and implementation notes record modifier priority. |
| parent-1 | medium | patch | The frozen marker matrix promised multipage/zoom/scroll/auto-scroll behavior but the new module tests initially used only one page at zoom=1. Add exact mapping and bounded RAF lifecycle tests. |
| parent-2 | medium | patch | The added new-opcode ABI section asserted encoding but did not invoke the new commands directly. Browser exercises valid real WASM paths, and Rust tests reject malformed payloads; add direct actual-WASM malformed/new-command tests to support the stronger ABI claim. |

| parent-3 | medium | patch | Clippy rejects the deliberately reversed literal range. Represent the invalid input as an explicit Range struct, preserving rejection coverage without a lint suppression. |
| parent-4 | medium | patch | Clippy rejects is_none/expect comparisons. Use guarded matches and a short mutable stats scope, retaining exact results and avoiding deep copies. Targeted release regressions and -D warnings pass. |

## Verification

- `node --test tests/text-drag-drop.mjs` — matrice contrôleur.
- `cargo test --locked --offline -p atelier-text-model -p atelier-core-wasm` — transactions, protocole.
- Validation complète du parent incluant WASM et navigateur, puis carte Graphify actualisée.

Final receipt: editor-drag-spacing-perf-validation.json; 23 gates, 749 Windows tests, 78 layout release tests, 82 browser assertions. Frozen matrices are traced by editor-drag-spacing-perf-matrix.json. Native external Windows drag and the 16 ms UI criterion remain open.
