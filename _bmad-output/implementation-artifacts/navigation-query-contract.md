# Editor search and statistics query wire contract

These frames extend ATED command version 1. Existing ATES state and ATRP preview layouts remain unchanged. Integers and IEEE-754 doubles are little-endian. Offsets and ranges count UTF-8 bytes in the editable TXT buffer.

## Commands

The common command envelope remains `ATED | u16 version=1 | u16 opcode | body`.

### Opcode 19: Search

Body layout (16-byte fixed prefix followed by query bytes):

| Offset | Type | Meaning |
|---:|---|---|
| 0 | u64 | Current document revision; mismatch returns `EDITOR_STALE_QUERY` |
| 8 | u8 | Flags: bit 0 matchCase, bit 1 wholeWord, bit 2 ignoreAccents, bit 3 matchNbsp |
| 9 | u8 | Reserved, zero |
| 10 | u16 | Reserved, zero |
| 12 | u32 | Query UTF-8 byte length, at most 1024 |
| 16 | bytes | Exactly that many valid UTF-8 query bytes |

Unknown flag bits, nonzero reserved bytes, invalid UTF-8, a length mismatch, trailing data, and oversized queries fail before search. Search delegates to `atelier_text_model::search::Searcher` over `TextLines`; it does not search omitted DOCX content.

## Search response: ATQF v1

The response has a 24-byte header, then `item_count` entries. It is a dedicated query response and is never an ATES state frame.

| Offset | Type | Meaning |
|---:|---|---|
| 0 | 4 bytes | ASCII `ATQF` |
| 4 | u16 | Version 1 |
| 6 | u16 | Reserved, zero |
| 8 | u64 | Document revision |
| 16 | u32 | Number of returned occurrences, at most 1000 |
| 20 | u8 | `truncated` flag, 0 or 1 |
| 21 | u8 | Reserved, zero |
| 22 | u16 | Reserved, zero |

Each occurrence starts with a 24-byte record, followed by excerpt bytes, a u32 rectangle count, and that many 36-byte rectangles:

| Relative offset | Type | Meaning |
|---:|---|---|
| 0 | u32 | Match start byte in the whole document |
| 4 | u32 | Exclusive match end byte in the whole document |
| 8 | u32 | Excerpt UTF-8 byte length, at most the 128 KiB document limit |
| 12 | u32 | Match start byte inside the excerpt |
| 16 | u32 | Exclusive match end byte inside the excerpt |
| 20 | u8 | Excerpt flags: bit 0 content was omitted before, bit 1 after |
| 21 | u8 | Reserved, zero |
| 22 | u16 | Reserved, zero |
| 24 | bytes | Exactly `excerpt_length` valid UTF-8 bytes |
| after excerpt | u32 | Rectangle count; total across response at most 8192 |
| per rectangle | u32 + 4×f64 | Page index, x, y, width, height in page points |

The rectangle page is zero-based. Geometry must be finite, strictly positive in width and height, and lie on the 595.276 × 841.89 point A4 page. Each match’s grapheme-slot rectangles are merged when adjacent on the same cached layout line. No `Layout::new` call occurs per hit. If result, rectangle, or 32 MiB response bounds prevent returning further complete hits, `truncated` is set; an occurrence is never emitted with partial geometry. Ordinary excerpts use a 512-byte budget with context distributed around the match. If one grapheme-boundary match itself is longer than 512 bytes, its full range is retained; the excerpt may then be as large as the 128 KiB document bound. Context always stops on grapheme/UTF-8 boundaries. Empty queries return zero entries and `truncated=false`.

## Opcode 20: Statistics

Body is exactly one u64 document revision. A mismatch returns `EDITOR_STALE_QUERY`. Counts come from the existing plain-text statistics library; document lines/pages come from the current cached layout. Selection counts use the session’s current selection. Empty selection counters, lines and pages are all zero. For nonempty selections, lines/pages count cached layout lines/pages touched by the selected byte range.

## Statistics response: ATQT v1

The response is exactly 72 bytes:

| Offset | Type | Meaning |
|---:|---|---|
| 0 | 4 bytes | ASCII `ATQT` |
| 4 | u16 | Version 1 |
| 6 | u16 | Reserved, zero |
| 8 | u64 | Document revision |
| 16 | u32 | Current selection anchor byte |
| 20 | u32 | Current selection focus byte |
| 24 | 6×u32 | Document: words, characters, charactersWithSpaces, paragraphs, lines, pages |
| 48 | 6×u32 | Selection: same counter order |

Every counter is nonnegative and finite in JS (u32 wire range, safely representable as a JS number). Query operations return only these dedicated envelopes; they do not publish editor state or change text, selection, dirty state, history, or revision.
