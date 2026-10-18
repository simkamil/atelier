# US-203 — Rich editor protocol (ATED v2)

Contract of the single rich editor of the [migration plan](rich-editor-migration.md). It fixes the envelope, the position model, the command and reply catalogue, the errors and the limits so that the flat model (R1, US-204), the incremental layout (R2, US-216), the JS codec and page cache (R4) and the editor (R3a, US-217) can be written independently. Reference code: `crates/core-wasm/src/rich_protocol.rs` (Rust), `web/rich-editor-protocol.mjs` (JS), frozen vectors `tests/rich-editor-protocol-vectors.txt`.

## Compatibility

ATED v1 (`docs/us-120-editor-engine.md`, opcodes 0-30) is unchanged and frozen until the parity switch (US-219). v1 and v2 share the outer US-091 `Editor` / `EditorResult` messages and the 4-byte magic `ATED`; the version field at offset 4 selects the codec (`1` routes to `editor.rs` as before, `2` to the rich editor). v2 opcodes (`0x0101..`) never collide with v1 (`0..30`). A v1 frame is never valid v2 and the reverse: a v2 decoder answers `RICH_UNSUPPORTED_VERSION` to version 1. Routing belongs to the integrator; this story adds no route. Routing rule for the integrator: a payload goes to v2 when it has at least 8 bytes (the v1 minimum), starts with `ATED` and has version 2 at offset 4; anything shorter, including a 6 or 7 byte frame that claims version 2, stays with v1, which answers `EDITOR_INVALID_COMMAND`. Between 8 and 19 bytes a version 2 frame is a v2 `RICH_INVALID_ENVELOPE` (outer `Failed`, no header to echo).

## Envelope

Little-endian, 20-byte header then exactly `body_len` bytes (no trailing data). The whole frame is at most `MAX_FRAME_BYTES = 32 MiB`, in both directions.

| Offset | Width | Field | Rule |
|---:|---:|---|---|
| 0 | 4 | Magic | `ATED` |
| 4 | 2 | Version | `2` |
| 6 | 2 | Opcode | command `0x01xx`, reply `0x8xxx` |
| 8 | 4 | Document id | `u32`, `0` reserved and always invalid |
| 12 | 4 | Request id | `u32`, chosen by the caller, echoed unchanged in the reply |
| 16 | 4 | Body length | `u32` |

Document id: one active document until US-327. `Open` with id `N` replaces the active document (the previous one is dropped without notice). Any other command with an id that is not the active one is answered `RICH_UNKNOWN_DOCUMENT`. The outer US-091 request id (`u64`) still correlates transport messages; the inner `u32` correlates editor requests.

Every request gets exactly one reply: the success reply of its command or an `Error` reply carrying the same document and request ids. When the header cannot be read (no magic, version other than 2, shorter than 20 bytes) there is nothing to echo and the host answers with the outer `Failed` message whose payload is the ASCII error name. The state is never changed by a refused command.

## Scope of the first layout

Batch 1 and R2 do not lay out tables, headers or footers. Paragraphs of table cells are laid out as full-width body paragraphs (documented degraded mode: no cell borders, widths or row heights), and headers and footers are not drawn. The flat text still contains the cell paragraphs, so positions stay stable when table layout arrives. The accessors for tables, cells and headers and footers are added by the story that implements them, not by US-203.

## Positions

Flat positions are an **interchange representation of this protocol only**. The model and the recovery journal keep node id + offset (SPEC §6.3) and never store flat positions; the model converts flat position <-> (node id, offset) on demand (`FlatIndex`), and the model keeps no side table: breaks, symbols and objects are inline items of the paragraph (3 flat bytes each) and run styles a run attribute, so `position_map` remaps nothing outside the model (`ImportedDocx::breaks`, `symbols` and `run_styles` survive only as a derived view for the legacy `RichSession`, removed with R8).

The flat text of a document covers **every paragraph in document order**, table cell paragraphs included (table by table, row by row, cell by cell), joined by one `\n` byte, with no trailing `\n`; headers and footers are not part of it. An opaque block counts as one paragraph made of a single U+FFFC. Inside a paragraph, every inline item that is not text counts as U+FFFC, **3 bytes** (`EF BF BC`) each: anchored objects (images...), line, page and column breaks and symbols. `run_styles` is a run attribute (`CharAttrs::char_style`), not an inline item: it counts **0 bytes**. Items at one text offset appear in `id` order across anchored objects, breaks and symbols. All positions, ranges and lengths are **byte offsets into that text**, `u64` on the wire (BigInt in JS), valid in `0..=doc_len`; the engine converts them with `usize::try_from` (`rich_protocol::to_position`), a value that does not fit is out of range.

Rules, checked by the engine (`RICH_*` code in parentheses):

- A position greater than `doc_len` is refused (`POSITION_OUT_OF_RANGE`). A range with `start > end` is refused (`INVALID_RANGE`), checked in the codec.
- **Clusters.** Boundaries follow UAX #29 with two overrides, whatever the UAX rules say: there is always a boundary before and after U+FFFC, and before and after every `\n`. A combining mark inserted right after an object therefore forms its own cluster. A **valid position** is such a boundary (hence a scalar boundary, never inside U+FFFC); any other position is refused with `POSITION_NOT_BOUNDARY`.
- `ReplaceRange` needs both ends to be valid positions (`POSITION_NOT_BOUNDARY` otherwise). `SetSelection` snaps both ends backwards to the previous valid position instead of failing. A selection reported by the engine always lies on valid positions. The affinity (`upstream`) of the focus says, at a soft line wrap, whether the caret is drawn at the end of the previous line (true) or the start of the next one.
- Deletions by unit (`Delete`) are computed by the engine on cluster or word boundaries (R1 provides `prev/next_grapheme`, `prev/next_word`, `word_at`, `snap_back` on `FlatIndex`), never by the caller. A non-collapsed selection is deleted whole whatever the unit. Deleting at a `\n` removes the whole separator and merges the two paragraphs.
- Text sent by the caller (`InsertText`, `ReplaceRange`): valid UTF-8 (`INVALID_UTF8`); no U+FFFC, no `\r`, no control character except `\n` and `\t` (`INVALID_TEXT`). `\n` splits paragraphs. An object enters the document only through its own command (US-218).
- A selection is `(anchor, focus)`, not ordered; the caret is `anchor == focus`.
- `revision` counts applied edits (undo and redo included) from 0 at `Open`. Selection changes do not change it.

## Commands

Body layouts; reserved bytes must be zero, flags are `0` or `1`, floats must be finite.

| Opcode | Name | Body | Reply |
|---:|---|---|---|
| `0x0101` | Open | `kind u8` (0 blank, 1 TXT, 2 DOCX), `profile u8` (0 document, 1 TXT), `0 u16`, `len u32`, `data` | State (revision 0, first pages) |
| `0x0102` | Close | empty | Closed |
| `0x0103` | InsertText | `len u32`, UTF-8 (non-empty, at most 1 MiB): replaces the selection | State |
| `0x0104` | ReplaceRange | `start u64`, `end u64`, `len u32`, UTF-8 (may be empty = delete) | State |
| `0x0105` | Delete | `forward u8`, `unit u8` (0 grapheme, 1 word), `0 u16` | State |
| `0x0106` | Move | `direction u8` (v1 numbering 0-13), `extend u8`, `0 u16` | State |
| `0x0107` | SetSelection | `anchor u64`, `focus u64`, `upstream u8` (flag), `0 u8[3]` | State |
| `0x0108` | HitTest | `page u32`, `mode u8` (0 probe, 1 set caret, 2 extend), `0 u8[3]`, `x f64`, `y f64` (page points) | probe: HitResult; else State (affinity computed by the engine) |
| `0x0109` | Undo | empty | State (unchanged state when nothing to undo) |
| `0x010A` | Redo | empty | State |
| `0x010B` | TextWindow | `start u64`, `len u32` (1 to 256 KiB) | TextWindow |
| `0x010C` | Pages | `first u32`, `count u32` (1 to 16) | Pages (`NOT_READY` if a requested page is not computed yet) |
| `0x010D` | MarkSaved | `revision u64` | State (`modified` cleared if the revision is current, else `STALE_REVISION`) |
| `0x010E` | Layout | `budget u32` (1 to 4096 paragraphs) | State |
| `0x010F` | AccessibleWindow | `start u64`, `end u64` (valid positions, `end - start` at most 256 KiB) | AccessibleWindow |
| `0x0118` | Resync | empty | State (new atlas epoch, every page invalidated, see "Glyph atlas") |

**R7b-1 (surbrillance de la sélection, images, double et triple clic)** : commandes `0x0120` SelectionRects, `0x0121` ImageBitmap, `0x0122` SelectUnit, réponses `0x8110` et `0x8111`, erreur 30 : [us-203-r7b1-selection-images.md](us-203-r7b1-selection-images.md).

**Background layout.** The tail of a long document paginates incrementally. Every edit, move and selection command also runs `DEFAULT_LAYOUT_BUDGET` (16) paragraphs of pending layout, then answers. `Layout { budget }` runs up to `budget` paragraphs more and answers a State, so the client repeats it until the State carries the layout-complete flag; `page_count` grows meanwhile. `Pages` for a page that is not computed yet answers `RICH_NOT_READY` and the client retries after another `Layout`.

`Open` consistency: a blank document carries no data; TXT requires profile TXT; DOCX requires profile document. A source the engine rejects is `OPEN_FAILED`. `data` is at most `MAX_OPEN_BYTES` (frame limit minus header and fields). Direction numbers: 0 left, 1 right, 2 up, 3 down, 4 home, 5 end, 6 document start, 7 document end, 8 page up, 9 page down, 10 word left, 11 word right, 12 paragraph up, 13 paragraph down.

`TextWindow`: `start` must be a valid position; the engine returns at most `len` bytes, shortened backwards to a valid position, and fewer at the end of the document. `Pages`: pages `first .. first+count` clamped to the page count; a `first` at or beyond it is `PAGE_RANGE`.

## Replies

Reply opcodes: `0x8000` Error, `0x8100` State, `0x8101` TextWindow, `0x8102` Pages, `0x8103` HitResult, `0x8104` Closed, `0x8105` AccessibleWindow. The full export is never returned.

**Error** (`0x8000`): `code u16`, `0 u16`.

**State** (`0x8100`), after every command that can change the state:

| Field | Type | Meaning |
|---|---|---|
| revision | u64 | after the command |
| anchor, focus | u64, u64 | selection, `<= doc_len` |
| doc_len | u64 | flat bytes |
| flags | u32 | bit 0 modified, 1 can undo, 2 can redo, 3 layout complete, 4 text window present, 5 focus is upstream; other bits 0 |
| page_count | u32 | pages so far (grows while `layout_complete` is 0) |
| caret | u32 page (`0xFFFFFFFF` none), `x f64`, `y f64`, `height f64` | geometry of the focus; zeros when none |
| invalidated | `n u32` (at most 1024), `n` x (`first u32`, `end u32`) | half-open page ranges the client must drop; sorted and disjoint; ranges that overlap or **touch** (`end == next first`) are merged by the sender (`split_page_delta`) and refused by the decoder (`PAGE_RANGE`) |
| atlas | `atlas_epoch u32`, `atlas_first u32`, `atlas_count u32`, `atlas_count` x (`len u32`, ATDL) | glyph atlas entries new since the previous reply, see "Glyph atlas" below; sent just before the pages that use them |
| pages | `n u32` (at most 16), `n` x (`index u32`, `len u32`, ATDL) | pages that changed, indices strictly increasing and `< page_count`; each is a complete ATDL display list (`docs/us-080-display-list.md`, 44 bytes to 16 MiB); the sum of the lists of one reply is at most 8 MiB unless the reply carries a single page (`rich_protocol::page_fits`, checked by encoders and decoders, `RICH_FRAME_TOO_LARGE`); the engine drops the pages that do not fit from `pages` and reports them in `invalidated`, the client asks for them with `Pages` |
| window | `start u64`, `len u32`, UTF-8 (at most 256 KiB, within `doc_len`) | only if flag 4: the text around the focus that the engine chooses to send so the DOM mirror stays current |

When more than 16 pages change, or their display lists exceed the reply bound, the first ones are sent and the others appear in `invalidated`: the client re-requests them with `Pages` (which also stops at the bound and answers fewer pages, the first one always). Since R2b a page references glyph contours through the atlas below: about 36 bytes per glyph (one 36-byte `DrawGlyphRun` entry, plus 29 bytes per run of one colour), against 836 to 1561 bytes before, when every glyph repeated its contour as a `FillPath`.

**Glyph atlas (R2b).** The pages of the rich editor draw a glyph with a `DrawGlyphRun` command of `font_id` 0 whose `glyph_id` is an index into the atlas of the document session; the glyph `position` is the origin (baseline start) of the glyph in page points and `advance` is `(0, 0)`. Consecutive glyphs of one colour share a run; any other command (an underline stroke, an inline contour) ends the run, so paint order is the one of the layout. An atlas entry is the ATDL display list of one contour, in page points relative to the glyph origin and already scaled to the glyph size (y grows downwards): exactly one black, non-zero `FillPath`, the format of the atlas of the DOCX preview (`glyph_atlas` of preview v2). Entries are numbered in creation order from 0; the key is (face, glyph id, size in 1/64 pt), so one entry serves every colour (the colour is the one of the run). Pages and entries are painted lazily: a reply carries the entries created since the previous reply (`atlas_first` is the number of entries already sent in this epoch, `atlas_count` those of this reply, possibly 0), which include every entry the pages of this reply and of the replies before it refer to.

Epoch. `atlas_epoch` changes whenever the numbering restarts at 0: at every `Open` (never equal to the epoch of the document it replaces or of the last closed one), at a resynchronisation (`mark_resync`: the engine empties its atlas and its painted pages) and when the engine rebuilds its layout after a failure. A client that receives another epoch empties its atlas **and its cached pages** (they carry the old indices), then stores the reply; `atlas_first` must then be 0. In the same epoch `atlas_first` must equal the number of entries the client already holds, otherwise the client reports `CLIENT_BAD_REPLY` and starts its resynchronisation (`Resync`, below). A reply that the client ignores for other reasons (stale revision) still carries entries that it must store.

Bounds. At most 4096 entries per session (`MAX_ATLAS_ENTRIES`; indices are below it), at most 64 KiB per entry (`MAX_ATLAS_ENTRY_BYTES`, 44 bytes minimum) and 4 MiB in total (`MAX_ATLAS_RESPONSE_BYTES`, so the whole atlas always fits one reply); encoders and decoders check them (`RICH_INVALID_BODY`, `RICH_FRAME_TOO_LARGE`). Past a bound the engine does not fail: a glyph that finds the atlas full (or whose contour is too large) is drawn inline, as a `FillPath` of its contour at the glyph position, exactly like before R2b, and keeps being drawn that way. A page may therefore mix runs and `FillPath` commands. The atlas lives as long as the session: it is not trimmed when pages are evicted, and `dropPages` does not empty it.

**Recovery of an abandoned reply (`Resync`, `0x0118`).** The engine counts an entry as sent when it builds a State or Pages reply, so a reply that the client abandons after the engine produced it (an aborted request, `stopPump`, a transport or decoding failure) leaves the client behind the engine: the next reply would carry an `atlas_first` above what the client holds. The client therefore requests a resynchronisation, with `Resync` (empty body, answered by a `State`), whenever a State or Pages exchange fails other than by a `RICH_*` refusal, and at the first `atlas_first` that does not follow (`CLIENT_BAD_REPLY`). The engine answers like `mark_resync` followed by a `Layout` of budget 1 with a text window: a new epoch, the atlas and the painted pages emptied, `atlas_first = 0`, `invalidated = [(0, page_count)]`, the text window and the entries of the pages it sends. `Resync` is idempotent: when its own reply is lost the client sends it again until an answer arrives (the resynchronisation stays requested while the exchange fails), and each answer starts a new epoch from 0. Without a document it is `RICH_UNKNOWN_DOCUMENT`; an `Open` needs no resynchronisation before it.

**Resynchronisation.** When the engine knows that a reply never reached the client (encoding failure, ABI `STATUS_LIMIT`, `EDITOR_RESPONSE_LIMIT`), the next `State` carries `invalidated = [(0, page_count)]` and a text window, once; the client then rebuilds its cache from `Pages` requests.

**Moves and the layout.** A move or a hit test whose target is not laid out yet lays the tail out itself (blocks of 256 then 1024 paragraphs, at most 4096 paragraphs per command) and answers a `State` with the pages it changed; when the budget is spent the selection stays and the `State` has no caret and `layout_complete = 0`: the client sends `Layout` and repeats. `RICH_NOT_READY` is answered to `Pages` (and to a `HitTest` probe that cannot be placed). After a layout failure the engine rebuilds its layout instead of staying blocked. A page absent from both lists is unchanged and its cached list stays valid.

**TextWindow** (`0x8101`): `revision u64`, `start u64`, `doc_len u64`, `len u32`, UTF-8, with `start + len <= doc_len`.
**Pages** (`0x8102`): `revision u64`, `page_count u32`, then the atlas section and the page list as above (`atlas_epoch u32`, `atlas_first u32`, `atlas_count u32`, entries, then `n u32` and the pages).
**HitResult** (`0x8103`): `revision u64`, `page u32`, `position u64` (a valid position), `upstream u8` (flag), `0 u8[3]`.
**Closed** (`0x8104`): empty.
**AccessibleWindow** (`0x8105`), added by US-217 for the accessible DOM mirror (US-209): `revision u64`, `count u32` (at most 256), then `count` items in document order:

| Field | Type | Meaning |
|---|---|---|
| para_start, para_end | u64, u64 | flat positions of the paragraph (`para_end - para_start` is the byte length of its text; the separator is not part of it) |
| role | u8 | 0 paragraph, 1 heading, 2 list item, 3 table cell, 4 opaque block |
| level | u8 | heading 1-9, list level 0-8 (list items only), else 0 |
| reserved | u16 | 0 |
| label | `len u16`, UTF-8 | computed list label (`1.`), possibly empty |
| text | `len u32`, UTF-8 | flat text of the paragraph, objects, breaks and symbols as U+FFFC |
| inlines | `n u16`, then `n` x (`offset u64`, `kind u8`, `0 u8`, `len u16` + UTF-8) | exactly one per U+FFFC of the text, in text order; `offset` is flat, relative to `para_start`, and always on a U+FFFC |

Inline kinds: 0 anchored object (value: alternative text, possibly empty), 1 line break or text-wrapping break (empty value), 2 page or column break (empty value), 3 symbol (value: the displayable character; a code point F000-F0FF is reduced to its low Latin-1 byte, as the R2 drawing does). The command answers the paragraphs that intersect `[start, end)`: the one that holds `start` (a position on a separator belongs to the paragraph that ends there) up to the one that holds the last byte before `end` (only that of `start` when `start == end`), at most 256 items; a client that needs more continues from `last para_end + 1`. Both positions must be valid positions (`POSITION_NOT_BOUNDARY`, `POSITION_OUT_OF_RANGE`); `start > end` is `INVALID_RANGE`; `end - start` over 256 KiB is `WINDOW_TOO_LARGE`. The decoders check the whole structure (roles and levels, ordering, lengths, one inline per U+FFFC at its place). JS: `decodeRichReply` returns `items[].inlines: [{ offset: BigInt, kind, value }]` with `kind` one of `object`, `lineBreak`, `pageBreak`, `symbol`; the command is `{ type: "accessibleWindow", start, end }`.
Limits of the model (US-217): it holds no numbering definitions, so a list label is the decimal counter of the paragraph followed by a dot (bullets are not distinguished), and an object has no alternative text yet (empty value). Heading levels come from the style id (`atelier.heading.N`, `HeadingN`, `Heading N`, `TitreN`, case-insensitive) or from the styles it is based on.

**Client rules (R4).** The client first applies the atlas section (epoch change empties atlas and pages, then `atlas_first` is checked, then the entries are stored), even for a reply it then ignores. It applies `invalidated`, then stores `pages`. It drops its cached pages with index `>= page_count` whenever the page count decreases (the sender clamps its own ranges to `page_count`). It ignores a `Pages` reply whose `revision` is not the current revision (an edit happened since the request) and requests again. Cache entries are keyed by page index and are valid until invalidated, replaced or dropped.

## Client (R4)

`web/editor-page-cache.mjs` and `web/rich-editor-client.mjs` implement the client rules above; tests: `tests/editor-page-cache.mjs`, `tests/rich-editor-client.mjs` (simulated engine and transport, built with the R0 codec). They are not wired into `web/editor.mjs` or `web/app.mjs` (R7).

**`EditorPageCache`** holds the state of one document: `revision`, `selection {anchor, focus, upstream}` (BigInt), `caret`, `docLen`, `modified`, `canUndo`, `canRedo`, `layoutComplete`, `pageCount`, `window` (the text window, only while its revision is current). `apply(response)` takes a decoded reply and returns `{ignored, updated, dropped, invalidated}`: State (invalidated ranges, then pages, then the page count, then selection, flags and window), Pages and TextWindow (ignored unless their revision is the current one; a Pages reply before any State is ignored), hit results, errors and Closed (no effect). Application is atomic: a reply that fails the checks throws `RichClientError` `CLIENT_BAD_REPLY` and changes nothing. Display lists are decoded as views into the frame and copied once into buffers owned by the cache, so no reply frame is retained. `page(i)`, `has(i)`, `displayLists(placeholder)` (dense array for `CanvasRenderer.setPages`), `missing(first, end)` (runs of at most 16 unknown pages, clamped to the page count to request with `Pages`), `dropPages()`, `reset()`, `version` and an optional `onChange`.

**Glyph atlas on the client.** `EditorPageCache` keeps the atlas (`atlasEpoch`, `atlasSize`): each entry is decoded and checked (one black non-zero `FillPath`) when stored, bounded by 4096, kept between pages and emptied only by an epoch change or `reset()`. `cache.glyphResolver(fontId, glyphId)` is the `resolveGlyph` of `CanvasRenderer`: it answers `{elements, path}` for font 0 (`0n` or `0`) and a known index, builds the `Path2D` once and returns the same object afterwards, and answers `null` otherwise. `apply` reports `atlasReset: true` (and counts the pages it dropped) when a known epoch was replaced. The client (`createRichEditorClient`) exposes `atlasEpoch` and `glyphResolver`; an `atlas_first` that does not follow raises `CLIENT_BAD_REPLY` and sets the resync (`dropPages` and a `Resync` before the next command). Tests: `tests/editor-page-cache.mjs`, `tests/rich-editor-client.mjs`, `tests/rich-editor-abi.mjs` (real WASM: bytes per glyph, atlas continuity) and `tests/run-rich-atlas-render-browser.mjs` (Edge: atlas drawing against the former `FillPath` drawing, pixel for pixel) on captures from the real WASM: dense TXT, edited TXT, styled DOCX, one family at nine close sizes, and pages with very different glyph counts (one glyph, a full page, a short last line). **Limit of that test:** its `FillPath` reference is rebuilt in JS from the atlas entries of the code under test, so it proves that an atlas glyph is drawn exactly like an inline contour, not that the contours themselves are right. The independent check is the Rust test `the_atlas_pages_expand_to_the_preview_painter_bytes` (`rich_layout/cache.rs`), which expands the atlas pages and compares them byte for byte with the pages of the DOCX preview painter, a separate code path. The client also bounds the bytes of its atlas (`MAX_ATLAS_CLIENT_BYTES`, 16 MiB, over the 4 MiB the engine sends at most per reply), and applies a reply atomically (the atlas section, the epoch and the pages are validated before any of them is stored).

**`createRichEditorClient({transport, docId, ...})`**: `transport.request(frame, {signal, onProgress})` resolves the raw v2 reply frame (the outer US-091 wrapping stays with the transport, as in `engine-client.mjs`). One request is in flight at a time, in submission order; each carries a fresh u32 request id and the reply is checked against the document and request ids (`CLIENT_MISMATCH`) and against the expected reply type. API: `open`, `command` (resolves `{response, change}`), `ensurePages(first, end)` (retries after `Layout` on `RICH_NOT_READY`, bounded, and stops with the error once layout is complete), `setVisibleRange(first, end)` (unknown visible pages are fetched in the background), `closeDocument`, `stopPump`/`startPump`, `whenLayoutComplete`, `idle`, `dispose`. The pump sends `Layout { budget }` while the layout-complete flag is absent, yields between two sends (`yieldTurn`) and runs only when no user command is queued; it stops on `stopPump`, on disposal or abort of the client signal, and on a failing reply (reported through `onError`). A cancelled command whose reply carries a State may have lost invalidations: the cache drops its pages and a one-paragraph `Layout` resynchronises before the next command.

Errors are `RichClientError` with `code` equal to the `RICH_*` name for an engine refusal, or `CLIENT_BAD_REPLY` (truncated or corrupt frame, unexpected type), `CLIENT_MISMATCH`, `CLIENT_TRANSPORT` (the transport failed; `cause` holds the original), `CLIENT_CLOSED`, `CLIENT_RETRY_LIMIT`; cancellation stays an `AbortError`, and a command refused by the codec before sending throws `RichProtocolError`.

## Errors

Stable numeric codes and names, never renumbered:

| Code | Name | When |
|---:|---|---|
| 1 | `RICH_INVALID_ENVELOPE` | bad magic, header shorter than 20, length mismatch or trailing bytes |
| 2 | `RICH_UNSUPPORTED_VERSION` | version other than 2 |
| 3 | `RICH_UNKNOWN_OPCODE` | opcode not in the table (including a reply opcode sent as request) |
| 4 | `RICH_INVALID_BODY` | wrong body size, non-zero reserved byte, bad enum or flag, non-finite float, inconsistent `Open`, zero-length window or page count |
| 5 | `RICH_INVALID_DOCUMENT_ID` | document id `0` |
| 6 | `RICH_UNKNOWN_DOCUMENT` | id is not the active document (engine) |
| 7 | `RICH_POSITION_OUT_OF_RANGE` | position or range end beyond `doc_len` (engine) |
| 8 | `RICH_POSITION_NOT_BOUNDARY` | not a valid position (cluster boundary, never inside an object) (engine) |
| 9 | `RICH_INVALID_RANGE` | `start > end` |
| 10 | `RICH_INVALID_UTF8` | text not valid UTF-8 |
| 11 | `RICH_INVALID_TEXT` | U+FFFC, `\r` or a forbidden control character |
| 12 | `RICH_FRAME_TOO_LARGE` | frame, text or `Open` data over its limit |
| 13 | `RICH_WINDOW_TOO_LARGE` | window over 256 KiB |
| 14 | `RICH_PAGE_RANGE` | page index at or beyond the page count / `MAX_PAGES` |
| 15 | `RICH_TOO_MANY_PAGES` | more than 16 pages requested or returned |
| 16 | `RICH_STALE_REVISION` | `MarkSaved` with a revision that is not current (engine) |
| 17 | `RICH_OPEN_FAILED` | `Open` source rejected (engine) |
| 18 | `RICH_INTERNAL` | engine invariant failure; state unchanged |
| 19 | `RICH_LIMIT` | an edit would exceed a model limit (engine, `FlatError::Limit`) |
| 20 | `RICH_NOT_READY` | layout of the requested area is unfinished (engine, `LayoutError::NotReady`) |

Checks run in this order: envelope, document id, opcode, body (in field order), then engine rules.

Engine error mapping (`impl From` in `rich_protocol`): `FlatError::OutOfRange` to 7, `NotBoundary` to 8, `InvalidRange` to 9, `InvalidText` to 11, `Limit` to 19; `LayoutError::PageOutOfRange` to 14, `PositionOutOfRange` to 7, `NotReady` to 20.

## Limits

| Limit | Value |
|---|---|
| Frame, either direction | 32 MiB |
| `InsertText` / `ReplaceRange` text | 1 MiB |
| Text window (request and reply) | 256 KiB |
| Pages per reply | 16 |
| Glyph atlas entries per session | 4096 (indices below it) |
| One atlas entry | 44 bytes to 64 KiB |
| Atlas entries in one reply | 4 MiB |
| One page display list | 16 MiB |
| Display lists in one reply | 8 MiB (a single page may exceed it, up to 16 MiB) |
| Invalidation ranges per State | 1024 |
| Pages per document (`MAX_PAGES`) | 2^20 |

Encoders check the size limits themselves; in tests every encoded reply is also re-decoded (the product does not pay for it). Edit text accepts DEL (U+007F) and the C1 controls (U+0080 to U+009F); only C0 controls other than `\n` and `\t` are refused. JS decoders return views into the received frame (no copy).

## Interfaces left to the other slices

- R1 (US-204): implements on `RichDocument`, in `atelier_text_model::rich::flat`, `FlatIndex` (prefix sums, locate/position, `snap_back`, `prev/next_grapheme`, `prev/next_word`, `word_at`, implemented with `crates/unicode` plus the cluster overrides), `LayoutContext` and `FlatEdit`; `EditOutcome` reports `start`, `removed_bytes`, `inserted_bytes`, `doc_len`, the paragraph span (before/after indices, same meaning for undo and redo), `layout_scope` (`Paragraphs` or `All` for style or section changes) and `can_undo`/`can_redo`. `prev_word` is the start of the word strictly before the position (skipping the blanks in between), `next_word` the start of the word strictly after (skipping the blanks that follow the current word), as Ctrl+Left and Ctrl+Right in Word (US-221). `EditOutcome::map_position` can land inside a cluster merged by the edit: pass its result through `snap_back`. The conversions between a flat offset in a paragraph and an offset in `Paragraph::text`, both ways and aware of objects, breaks and symbols, are implemented (`paragraph_flat`, `flat_to_text_offset`, `text_to_flat_offset` and the matching `LayoutContext` methods). **Model fields frozen by US-203, behaviour left to R1** (`paragraph.rs`, `attrs.rs`): `Inline::Break(InlineBreak)` and `Inline::Symbol(InlineSymbol)` with `Paragraph::breaks` and `Paragraph::symbols` (3 bytes each in flat text), and `CharAttrs::char_style` (0 bytes). R1 feeds them from the DOCX import (`crates/fmt-ooxml/src/docx.rs:234-236` tables), shifts them with text edits and validates them (`paragraph.rs`, `ops.rs`, `validate.rs`). `VecParagraphs` (`rich/flat_double.rs`, a public module compiled out of wasm32 builds, so it adds no byte to the product WASM; tests that use it are host-only (`#[cfg(not(target_arch = "wasm32"))]`)) is a test double implementing `FlatIndex` and `LayoutContext` so that R2 does not wait for R1.
- R2 (US-216): `crate::rich_layout::{RichPageCache, PageDelta, CaretGeometry, LayoutError}` in `crates/core-wasm/src/rich_layout.rs`, declarations only, with the empty submodule `rich_layout::rich_paint` (file `crates/core-wasm/src/rich_layout/rich_paint.rs`) declared so that R2 never touches `lib.rs`. The cache owns no document: `new`, `apply_edit`, `step` and `page_display` receive a `&dyn LayoutContext`, which gives the style catalog and theme, the resolved properties (`resolved(index)` for the paragraph after document defaults, table style, paragraph style and direct formatting; `resolved_run(index, direct)` for one run, `direct.char_style` selecting the character style; `StyleEngine::resolve_detached` is the document-free cascade behind them), the paragraphs (none for an opaque block), the block kind (body, table cell with table, row and cell ids, or opaque), the section and its `page_setup(section)` (`PageSetup`, twips, normalized: read from `Section::opaque` attributes of the WordprocessingML namespace named `pgSz.w`, `pgSz.h`, `pgSz.orient`, `pgMar.top`, `pgMar.right`, `pgMar.bottom`, `pgMar.left`, `pgMar.header`, `pgMar.footer`, `pgMar.gutter`, whose values are the ASCII text of the `w:sectPr` attributes; defaults are those of the US-202 default document, A4 11906 x 16838 with 1417 margins, plus 708 for header and footer distances and 0 gutter, which the code does not define; R1 must fill these attributes from the DOCX import, and `example_section()` in the test double shows the form), object resources and the list counters before each paragraph. `step` takes a budget in paragraphs, never a clock (`Instant::now()` panics on wasm32-unknown-unknown). Queries: `caret(position, upstream)`, `line_edges(position, upstream)`, `hit_test` (position and affinity), `move_vertical`, `move_page(position, pages, preferred_x)`. Target under 4 ms per keystroke on 300 pages, no global pagination pre-validation. `rich_protocol::split_page_delta` turns a `PageDelta` into the pages to send (at most 16, all `< page_count`) and the merged invalidation ranges (at most 1024).
- R3a (US-217): maps `Command` to `FlatEdit` and `RichPageCache` and builds `State` (`rich_protocol::State`); delivered in `crates/core-wasm/src/rich_editor.rs`, see [us-217-rich-editor.md](us-217-rich-editor.md). The route (`rich_editor::is_rich` in `lib.rs`) sends version 2 frames to it.
- R4: `web/rich-editor-protocol.mjs` is frozen by R0; R4 created `web/editor-page-cache.mjs` and `web/rich-editor-client.mjs` (section Client). Editor wiring is R7.
- Integrator: routing by version byte in `editor.rs`, and `node tests/rich-editor-protocol.mjs` in CI next to `tests/editor-protocol.mjs`.

## Frozen vectors

`tests/rich-editor-protocol-vectors.txt`: tab-separated `kind`, `name`, frame hex, expectation. `request` and `reply` rows carry a canonical field dump; `bad-request` and `bad-reply` rows carry the expected error name (`nocontext` when the header is unreadable). The Rust test regenerates the file and fails on any difference (`ATELIER_REGEN_VECTORS=1` rewrites it, only for a deliberate v2 change); the JS test decodes every row, compares the dump, re-encodes to the same bytes, and checks that every strict prefix of a valid frame is rejected.
