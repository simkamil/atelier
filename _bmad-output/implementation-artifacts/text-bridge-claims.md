# Claims US-011 / US-053 / US-091

The three implementation specs are the account of this batch:

- [US-011 segmentation](spec-us011-segmentation.md)
- [US-053 kerning and ligatures](spec-us053-kerning-ligatures.md)
- [US-091 Rust/JS binary protocol](spec-us091-binary-protocol.md)

The claimed result is three reusable engine primitives. A graphical MVP,
business handlers and a Worker are still future work. The backlog now contains 29 validated stories out of 158. All 25 local
checks passed after the corrections, with no deferred review findings.

Root verification adds all-position navigation tests across 2 710 official
Unicode cases, 54 716 independent HarfBuzz golden cases on all twenty fonts,
and exact protocol capacity checks on Rust and JavaScript. Existing ABI tests
continue to use the real production WASM and its isolated panic hooks.

Before the formal review, the coordinator found and requested corrections for
quadratic word RI lookback, JS Buffer aliasing, UTF-8 slicing in the oracle CLI,
and missing actual Rust-output decoding in the cross-codec tests.
The font parser also retains lookup subtable alternatives and charges work
to explicit validation/application budgets. The complete validation snapshot confirms the final
implementation on unchanged sources, beyond the targeted development tests.
