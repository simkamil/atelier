# Epic 0.11 Context: Socle du front-end

## Sources and scope

This context is derived from the repository’s product specification and backlog because no separate planning artifacts are present. It does not imply that a PRD, UX specification, or architecture document exists.

- `SPEC.md` §6.7 defines the handwritten WASM-to-JavaScript bridge. Text input uses an invisible textarea positioned at the caret to capture keyboard, dead-key, IME, `beforeinput`, and composition events; Canvas displays the result.
- `US-V0-V1.md` E0.10 records the existing ABI, binary protocol, and Worker foundation (US-090–092). E0.11 defines the front-end Host (US-100), Canvas renderer (US-101), localization (US-102), input capture (US-103), and UI components (US-104 onward).
- `US-V0-V1.md` US-103 calls for a focusable hidden text area, routing `keydown`/`beforeinput`/composition events, French keyboard and AltGr text, and composition display. US-120 depends on US-103 for the plain-text editor slice.
- `docs/us-092-worker.md`, `web/engine-client.mjs`, and `web/wasm-protocol.mjs` establish the current transport boundary. The client returns an echo frame; document editing handlers and selection geometry have not been implemented.

## Architectural constraints

The product uses vanilla ES modules without third-party JavaScript dependencies (SPEC.md D11). The browser resolves text through its native input and composition behavior; `KeyboardEvent.key` is not a text source. The input adapter passes structured commands and composition previews to an owner that supplies the current selection and viewport caret rectangle. It does not own document state, derive selection geometry, or draw text over the document. The owner can use Canvas for composition underlines.

Text input and document-model positions have different representation concerns: DOM selection offsets are UTF-16, while the current Rust model’s positions are UTF-8 byte offsets (`crates/model/src/selection.rs`). The adapter therefore treats selection as owner-provided data and does not convert or interpret its indices.

## Dependencies and open boundaries

- US-103 depends on US-092’s browser Worker/transport foundation, while its callbacks remain independent of the echo ABI.
- US-120 also depends on the Canvas path (US-101), model/history work, and native bridge (US-113). The input adapter alone cannot satisfy end-to-end editing.
- US-100’s desktop and fake Host implementations remain open in the backlog. They are outside US-103.
- The backlog marks native AZERTY, Alt+code, and real IME behavior as acceptance goals. Synthetic browser events can test event routing but cannot prove native keyboard layouts, WebView2 behavior, or IME integration. Keep US-103 open until those manual checks and the later editor integration are complete.
