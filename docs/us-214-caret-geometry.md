<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-214 — Rich caret geometry

The `atelier_text_layout::caret` module maps rich paragraph byte offsets to visual line geometry without depending on a paginator. The caller adapts positioned pagination lines to `PositionedLine`: each value identifies its page and paragraph, carries the original paragraph text and its `LayoutLine`, and supplies the page-local top and explicit horizontal translation.

All coordinates, heights, and advances use 1/64 typographic point. Text positions are UTF-8 byte offsets within a paragraph. `CaretLayout::new` validates the complete ordered line set before answering queries: source ranges must cover each supplied paragraph contiguously, terminal line breaks must be followed by their empty line, line boxes on a page cannot overlap, visible text must have glyph geometry, glyph spans must fit on their line and end on grapheme boundaries, line heights must be positive, and arithmetic and work budgets are checked. No partial layout is returned after an error.

```rust,ignore
use atelier_text_layout::caret::{
    CaretAffinity, CaretLayout, CaretOptions, PositionedLine, TextPosition,
};

let positioned = [PositionedLine {
    page_index: 0,
    paragraph_index: 0,
    text: "Hello",
    layout: &paragraph_layout.lines[0],
    top: 0,
    x_translation: 0,
}];
let carets = CaretLayout::new(&positioned, CaretOptions::default())?;
let rect = carets.caret_rect(
    TextPosition { paragraph_index: 0, byte_offset: 5 },
    CaretAffinity::Downstream,
)?;
```

`caret_rect` requires an explicit affinity where a byte position belongs to two visual lines. Upstream chooses the preceding visual line; downstream chooses the following one at soft wraps. Before LF or CRLF, the byte position remains at the end of the preceding line; after the complete line ending it belongs to the following line regardless of affinity. CRLF remains one grapheme, so its interior byte offset is rejected. `hit_test` takes an explicit page index and page-local coordinates; pages can therefore overlap in local space without becoming ambiguous.

`selection_rects` normalizes reversed endpoints and returns one page- and line-indexed rectangle per intersected visual line. `move_vertical` is the compatibility API: it resolves a shared soft-wrap offset downstream, so it does not preserve the selected visual line when its returned position is reused. For persistent vertical navigation, call `move_vertical_with_affinity` with the current affinity and pass its returned destination affinity and preferred x into the next move. The first move captures the current caret x; keeping that x through short intermediate lines preserves the desired column.

Glyphs sharing a source cluster are aggregated before positions are generated, so a cluster's advance is counted once per glyph and never assigned twice to source positions. If a shaping cluster spans multiple graphemes, all its internal caret positions divide the aggregate advance uniformly across those graphemes. This is the geometry contract for ligatures in this API, and is an approximation because exact GDEF caret data is not available. Every generated stop still lies on an extended grapheme boundary. Leading and interior visible whitespace requires glyph geometry. Only trailing whitespace trimmed from the line may remain as distinct byte positions at the final visible x with zero advance.

`CaretOptions` bounds positioned line count, generated stop count, and validation work. Invalid positions/ranges, malformed geometry, page misses, and exhausted budgets return typed `CaretError` values. A caret rectangle has a fixed positive width of one engine unit (1/64 pt), with checked right-edge arithmetic so renderers can draw the insertion mark visibly.

`flow::caret_from_pagination(texts, layouts, pagination, x_translation, options)` builds these views directly from the pagination result. It checks page ranges and complete source-order coverage before constructing the caret geometry. The integrated tests use real rich paragraph layout, default tab advances, paragraph spacing and multiple pages.

This module does not perform pagination, bidi reordering, complex-script visual ordering, UI integration, Word measurement, or GDEF caret positioning.
