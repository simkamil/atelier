<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-073 — Modèle de sélection

`atelier_model::selection` provides stable, directed text ranges and a pure
position-mapping contract for future text operations. It is independent from
`Operation<S>`, `Transaction`, and the `ATOP` codec. A concrete text adapter
collects the relevant maps while applying an edit and publishes the resulting
selection only when the document edit succeeds.

## Positions and ranges

`TextPosition` identifies a node with `NodeId` and an offset measured in UTF-8
**bytes**. Node IDs are local to their owning tree. The selection module has no text contents, so it can check arithmetic
and structural map validity but cannot check whether an offset is a UTF-8
character boundary. The owning text model must validate boundaries before
accepting edits or exposing positions.

Each endpoint has an `Affinity`. `Before` stays before text inserted exactly at
that endpoint; `After` follows the inserted text. `SelectionRange` stores its
anchor and head separately, preserving direction, including for collapsed
ranges. `Selection::caret` uses `After` for both endpoints so a caret follows
typing by default. A selection preserves the caller's range order and primary
range index, and accepts between 1 and 64 ranges.

```rust
use atelier_model::{IndexedTree, selection::{Affinity, Selection, SelectionRange, TextPosition}};

let mut nodes = IndexedTree::new();
let paragraph = nodes.insert(0, ()).unwrap();
let caret = Selection::caret(paragraph, 0);
let directed = Selection::new(vec![SelectionRange::new(
    TextPosition::new(paragraph, 5, Affinity::Before),
    TextPosition::new(paragraph, 2, Affinity::After),
)], 0).unwrap();
assert_eq!(caret.ranges().len(), 1);
assert_eq!(directed.ranges()[0].anchor.offset, 5);
```

## Position maps

`PositionMap` describes one concrete edit and `Selection::mapped` returns a
mapped copy. `Selection::transform` commits only after every endpoint maps
successfully; overflow or an invalid event leaves the original selection
unchanged.

- `Insert { node, at, byte_len }`: endpoints before `at` stay put; endpoints
  after it shift by `byte_len`; equality follows affinity.
- `Delete { node, start, end }`: deletes the half-open interval `[start, end)`.
  Endpoints in the interval or exactly at `end` collapse to `start`; later
  endpoints move left by the deleted byte count. `start > end` is rejected.
- `Split { source, right, at }`: endpoints after the split move to `right` with
  `at` subtracted. At equality, `Before` remains on the left and `After` moves
  to offset zero on the right.
- `Merge { left, right, left_len }`: endpoints in `right` move to `left` at
  `left_len + offset`, with checked arithmetic.
- `Remove { node, fallback }`: every endpoint in the removed node takes the
  explicit fallback. The fallback must name a surviving node; this generic
  mapper does not infer a neighbor from a tree.

Maps carry node IDs, offsets and lengths so future operations can emit them
without adding a selection bound to generic operations. The caller must ensure
the event describes the actual edit, both source and target identities belong
to the relevant document, and all mapped byte offsets remain valid UTF-8
boundaries. For a sequence of maps, apply them in edit order to a candidate
selection and publish the candidate with the document under the same
transaction/checkpoint boundary.

## Integration limits

US-073 defines the selection value and mapping contract only. It does not
automatically observe edits, update `Operation<S>`, alter `History`, or create a
text model. `History<Op, Selection>` can store exact before/after snapshots
because selection is `Clone`; concrete editing code remains responsible for
calculating each snapshot and coordinating position maps with document changes.

Les preuves de validation et les limites d’intégration sont rassemblées dans le [suivi du lot](MODEL-FONT-PROGRESS.md).
