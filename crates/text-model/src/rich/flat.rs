// SPDX-License-Identifier: AGPL-3.0-or-later
//! Flat-position contract of the rich editor (US-203, implemented by US-204).
//!
//! Declarations, plus the pure conversions between a paragraph and its flat
//! text and [`EditOutcome::map_position`]. R1 implements [`FlatIndex`],
//! [`LayoutContext`] and [`FlatEdit`] on `RichDocument` (the type R1
//! implements; [`super::flat_double::VecParagraphs`] is a test double that
//! implements the same traits over a plain vector). The protocol layer
//! (`atelier-core-wasm::rich_protocol`) and the layout cache (`rich_layout`)
//! depend only on these signatures. Full rules:
//! `docs/us-203-rich-editor-protocol.md`.
//!
//! Flat positions are an interchange representation of the protocol only:
//! the model and the recovery journal keep node id + offset (SPEC 6.3) and
//! never store flat positions; these traits convert on demand.
//!
//! Flat text. The flat index covers **every paragraph of the document in
//! document order**, table cell paragraphs included (cell by cell, row by
//! row), each separated from the next by one `\n` byte, no trailing `\n`.
//! An opaque block counts as one paragraph made of a single U+FFFC. Inside a
//! paragraph, every inline item that is not text is one U+FFFC (three bytes,
//! [`OBJECT_LEN`]): anchored objects (images...), [`super::InlineBreak`] and
//! [`super::InlineSymbol`]. `CharAttrs::char_style` is a run attribute and
//! counts 0 bytes. Everything else is its UTF-8. A position is a byte offset
//! in `0..=len()`. Items at one text offset appear in `id` order across the
//! three lists. Headers and footers are not part of the flat text.
//!
//! Clusters. Boundaries follow UAX #29 with these overrides, whatever the
//! UAX rules say: there is always a boundary before and after U+FFFC and
//! before and after every `\n`. A combining mark inserted right after an
//! object therefore forms its own cluster. A **valid position** is a
//! boundary of this segmentation (so also a scalar boundary, never inside
//! U+FFFC). Both [`FlatEdit::replace_range`] ends must be valid positions.

use std::borrow::Cow;

use super::{
    AnchoredObject, InlineBreak, InlineSymbol, NodeId, Paragraph, Section, StyleCatalog, Theme,
};

/// Bytes of one non-text inline item (object, break, symbol): U+FFFC.
pub const OBJECT_LEN: usize = 3;
/// Bytes of the separator between two paragraphs.
pub const SEPARATOR_LEN: usize = 1;
/// Number of list nesting levels (`MAX_LIST_LEVEL + 1`).
pub const LIST_LEVELS: usize = 9;

/// Why a flat position or edit was refused. State is unchanged on error.
/// Protocol mapping (`rich_protocol`): `OutOfRange` to
/// `RICH_POSITION_OUT_OF_RANGE`, `NotBoundary` to `RICH_POSITION_NOT_BOUNDARY`,
/// `InvalidRange` to `RICH_INVALID_RANGE`, `InvalidText` to
/// `RICH_INVALID_TEXT`, `Limit` to `RICH_LIMIT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatError {
    /// A position is greater than `len()`.
    OutOfRange,
    /// A position is not a cluster boundary.
    NotBoundary,
    /// `start > end`.
    InvalidRange,
    /// Text contains U+FFFC, `\r`, or a C0 control character other than
    /// `\n` and `\t` (DEL and C1 controls are accepted).
    InvalidText,
    /// The edit would exceed a model limit.
    Limit,
}

/// A position as paragraph index and byte offset inside that paragraph's flat
/// text (so `offset == paragraph_len` is the end of the paragraph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphPosition {
    pub paragraph: usize,
    pub offset: usize,
}

/// Prefix-sum index over paragraphs: each operation is `O(log n)` plus the
/// length of one paragraph, except [`FlatIndex::text`], linear in the
/// returned bytes. Object-safe.
pub trait FlatIndex {
    /// Total bytes of flat text.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn paragraph_count(&self) -> usize;
    /// Maps a flat position to its paragraph. A position on a `\n` separator
    /// belongs to the paragraph that ends there.
    fn locate(&self, position: usize) -> Result<ParagraphPosition, FlatError>;
    /// Inverse of [`FlatIndex::locate`].
    fn position(&self, at: ParagraphPosition) -> Result<usize, FlatError>;
    /// Checks that `position` is a valid position (cluster boundary).
    fn check_position(&self, position: usize) -> Result<(), FlatError>;
    /// Flat text of `start..end` (items as U+FFFC, separators as `\n`).
    fn text(&self, start: usize, end: usize) -> Result<String, FlatError>;

    /// Largest valid position `<= position` (snaps back into a cluster).
    fn snap_back(&self, position: usize) -> Result<usize, FlatError>;
    /// Previous cluster boundary, `None` at 0. A `\n` is one cluster.
    fn prev_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// Next cluster boundary, `None` at `len()`.
    fn next_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// Start of the word **strictly before** `position`, skipping the spaces
    /// and tabs between that word and `position` (Ctrl+Left in Word); `None`
    /// at 0. UAX #29 words; objects and `\n` are words of their own.
    fn prev_word(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// Start of the word **strictly after** `position`: the end of the word
    /// containing `position`, then the spaces and tabs that follow it are
    /// skipped (Ctrl+Right in Word, US-221). `len()` when none follows,
    /// `None` at `len()`.
    fn next_word(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// Enclosing word `(start, end)`; at the end of the document, the last
    /// word; `None` for an empty document.
    fn word_at(&self, position: usize) -> Result<Option<(usize, usize)>, FlatError>;
}

/// Kind of the block a paragraph of the flat index comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// A paragraph directly in a section body.
    Body,
    /// A paragraph of a table cell (ids of the table, row and cell nodes).
    TableCell {
        table: NodeId,
        row: NodeId,
        cell: NodeId,
    },
    /// An opaque block, one paragraph made of a single U+FFFC.
    Opaque,
}

/// Counters of one list before a paragraph, per level, from which a layout
/// computes the label of that paragraph (the paragraph itself not counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListCounters {
    pub list_id: u32,
    pub level: u8,
    pub before: [u32; LIST_LEVELS],
}

/// Everything the layout needs from the document, read-only and consistent
/// with the flat index. Methods taking a paragraph `index` use the order of
/// the flat index. Object-safe; R2 receives it as `&dyn LayoutContext` after
/// the edit it lays out has been applied.
pub trait LayoutContext: FlatIndex {
    /// Style definitions of the document. R2 owns its `StyleEngine` (it has a
    /// mutable cache) built from [`LayoutContext::catalog`] and
    /// [`LayoutContext::theme`], and rebuilds it on [`LayoutScope::All`].
    fn catalog(&self) -> &StyleCatalog;
    fn theme(&self) -> &Theme;
    /// The paragraph, `None` for an opaque block.
    fn paragraph(&self, index: usize) -> Option<&Paragraph>;
    fn block_kind(&self, index: usize) -> Result<BlockKind, FlatError>;
    /// Index and layout of the section containing the paragraph: page
    /// properties are the section's opaque attributes, headers and footers
    /// are opaque nodes of their own.
    fn section_of(&self, index: usize) -> Result<(usize, &Section), FlatError>;
    /// Resource bytes (image data...) of an anchored object.
    fn object_resource(&self, object: NodeId) -> Option<&[u8]>;
    /// List state before the paragraph; `None` when it is not in a list.
    fn list_counters(&self, index: usize) -> Result<Option<ListCounters>, FlatError>;
    /// Flat position of the first byte of paragraph `index`.
    fn paragraph_start(&self, index: usize) -> Result<usize, FlatError>;
    /// Flat text of one paragraph (items as U+FFFC; an opaque block is a
    /// single U+FFFC).
    fn paragraph_flat(&self, index: usize) -> Result<Cow<'_, str>, FlatError> {
        match self.paragraph(index) {
            Some(p) => Ok(paragraph_flat(p)),
            None => match self.block_kind(index)? {
                BlockKind::Opaque => Ok(Cow::Borrowed("\u{FFFC}")),
                _ => Err(FlatError::OutOfRange),
            },
        }
    }
    /// Offset in `Paragraph::text` for an offset in the paragraph's flat text
    /// (see [`flat_to_text_offset`]); for an opaque block `0`.
    fn flat_to_text(&self, index: usize, flat: usize) -> Result<usize, FlatError> {
        match self.paragraph(index) {
            Some(p) => flat_to_text_offset(p, flat),
            None if flat <= OBJECT_LEN && flat % OBJECT_LEN == 0 => Ok(0),
            None => Err(FlatError::NotBoundary),
        }
    }
    /// Offset in the paragraph's flat text for an offset in `Paragraph::text`
    /// (see [`text_to_flat_offset`]).
    fn text_to_flat(
        &self,
        index: usize,
        offset: usize,
        after_items: bool,
    ) -> Result<usize, FlatError> {
        match self.paragraph(index) {
            Some(p) => text_to_flat_offset(p, offset, after_items),
            None if offset == 0 => Ok(if after_items { OBJECT_LEN } else { 0 }),
            None => Err(FlatError::OutOfRange),
        }
    }
}

/// Offsets and ids of the non-text items of a paragraph, ordered by
/// `(offset, id)` across anchored objects, breaks and symbols.
fn item_offsets(p: &Paragraph) -> Vec<usize> {
    let mut items: Vec<(usize, NodeId)> = Vec::new();
    items.extend(p.anchored.iter().map(|x: &AnchoredObject| (x.offset, x.id)));
    items.extend(p.breaks.iter().map(|x: &InlineBreak| (x.offset, x.id)));
    items.extend(p.symbols.iter().map(|x: &InlineSymbol| (x.offset, x.id)));
    items.sort();
    items.into_iter().map(|(offset, _)| offset).collect()
}

/// Flat text of a paragraph: its text with one U+FFFC inserted at the offset
/// of each non-text item. Borrowed when the paragraph has no such item.
pub fn paragraph_flat(p: &Paragraph) -> Cow<'_, str> {
    let offsets = item_offsets(p);
    if offsets.is_empty() {
        return Cow::Borrowed(&p.text);
    }
    let mut out = String::with_capacity(p.text.len() + OBJECT_LEN * offsets.len());
    let mut at = 0;
    for offset in offsets {
        let offset = offset.clamp(at, p.text.len());
        out.push_str(p.text.get(at..offset).unwrap_or(""));
        out.push('\u{FFFC}');
        at = offset;
    }
    out.push_str(p.text.get(at..).unwrap_or(""));
    Cow::Owned(out)
}

/// Offset in `Paragraph::text` for an offset `flat` in the paragraph's flat
/// text. The positions just before and just after an item both map to the
/// item's text offset; a position inside an item is `NotBoundary`; one past
/// the end is `OutOfRange`; a result inside a UTF-8 scalar is `NotBoundary`.
pub fn flat_to_text_offset(p: &Paragraph, flat: usize) -> Result<usize, FlatError> {
    let offsets = item_offsets(p);
    let mut shift = 0;
    for offset in &offsets {
        let start = offset + shift;
        if flat < start {
            break;
        }
        if flat < start + OBJECT_LEN {
            return if flat == start {
                Ok(*offset)
            } else {
                Err(FlatError::NotBoundary)
            };
        }
        if flat == start + OBJECT_LEN {
            return Ok(*offset);
        }
        shift += OBJECT_LEN;
    }
    if flat > p.text.len() + OBJECT_LEN * offsets.len() {
        return Err(FlatError::OutOfRange);
    }
    let text_offset = flat - shift;
    if p.text.is_char_boundary(text_offset) {
        Ok(text_offset)
    } else {
        Err(FlatError::NotBoundary)
    }
}

/// Offset in the paragraph's flat text for an offset in `Paragraph::text`.
/// Items at exactly `offset` are counted before the result when `after_items`
/// is true, after it otherwise (the caret sits before or after the objects
/// anchored there).
pub fn text_to_flat_offset(
    p: &Paragraph,
    offset: usize,
    after_items: bool,
) -> Result<usize, FlatError> {
    if offset > p.text.len() {
        return Err(FlatError::OutOfRange);
    }
    if !p.text.is_char_boundary(offset) {
        return Err(FlatError::NotBoundary);
    }
    let before = item_offsets(p)
        .into_iter()
        .filter(|o| *o < offset || (after_items && *o == offset))
        .count();
    Ok(offset + OBJECT_LEN * before)
}

/// How far the layout must be redone for one edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutScope {
    /// Only the paragraphs named by the outcome changed.
    Paragraphs,
    /// A change outside paragraphs (styles, theme, section properties,
    /// numbering): rebuild everything.
    All,
}

/// What one applied edit, undo or redo changed. Always expressed from the
/// state *before* the call to the state *after*: the old bytes
/// `start..start + removed_bytes` became `inserted_bytes` new bytes at
/// `start`; paragraphs `first_paragraph..first_paragraph + removed_paragraphs`
/// (indices before) became `inserted_paragraphs` paragraphs at
/// `first_paragraph` (indices after). For undo and redo the same rule holds in
/// the direction of the call. `first_paragraph` is the paragraph containing
/// `start` before the edit; a split or merge counts the paragraphs it splits
/// or joins (a split of one paragraph is `removed 1, inserted 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    /// Model revision after the call (one more per applied edit, undo, redo).
    pub revision: u64,
    pub start: usize,
    pub removed_bytes: usize,
    pub inserted_bytes: usize,
    /// Flat length after the call.
    pub doc_len: usize,
    pub first_paragraph: usize,
    pub removed_paragraphs: usize,
    pub inserted_paragraphs: usize,
    /// `All` for changes outside paragraphs (the paragraph span is then only
    /// informative).
    pub layout_scope: LayoutScope,
    /// Selection `(anchor, focus)` to apply after the call, in new positions.
    pub selection: (usize, usize),
    pub can_undo: bool,
    pub can_redo: bool,
}

impl EditOutcome {
    /// Remaps a position of the state before the call to the state after:
    /// unchanged up to `start`, shifted after the removed range, and collapsed
    /// to `start` when it was strictly inside it. This is the flat view of
    /// `RichOp::position_map` (`ops.rs`), for side tables kept in flat form
    /// during one command. The result can fall inside a cluster that the edit
    /// merged (a mark inserted after it, a deletion joining two clusters):
    /// callers pass it through [`FlatIndex::snap_back`].
    pub fn map_position(&self, position: usize) -> usize {
        let removed_end = self.start.saturating_add(self.removed_bytes);
        if position <= self.start {
            position
        } else if position >= removed_end {
            position - self.removed_bytes + self.inserted_bytes
        } else {
            self.start
        }
    }
}

/// In-place editing with history. No clone of the document, history or side
/// tables per call.
pub trait FlatEdit: FlatIndex {
    /// Replaces `start..end` by `text`; both ends must be valid positions
    /// (`NotBoundary` otherwise). `\n` in `text` splits paragraphs (the new
    /// paragraph takes the style that follows the split); a range spanning
    /// separators merges the paragraphs it joins. Deleting is an empty text.
    /// Deleting a `\n` removes the whole separator and merges the paragraphs.
    fn replace_range(
        &mut self,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<EditOutcome, FlatError>;
    /// Reverts the last applied edit, `None` when there is nothing to undo.
    fn undo(&mut self) -> Option<EditOutcome>;
    fn redo(&mut self) -> Option<EditOutcome>;
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
    fn revision(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(start: usize, removed: usize, inserted: usize) -> EditOutcome {
        EditOutcome {
            revision: 1,
            start,
            removed_bytes: removed,
            inserted_bytes: inserted,
            doc_len: 0,
            first_paragraph: 0,
            removed_paragraphs: 1,
            inserted_paragraphs: 1,
            layout_scope: LayoutScope::Paragraphs,
            selection: (0, 0),
            can_undo: true,
            can_redo: false,
        }
    }

    #[test]
    fn map_position_follows_the_replaced_range() {
        let e = outcome(4, 3, 5);
        assert_eq!(e.map_position(2), 2);
        assert_eq!(e.map_position(4), 4);
        assert_eq!(e.map_position(5), 4);
        assert_eq!(e.map_position(7), 9);
        assert_eq!(e.map_position(10), 12);
        assert_eq!(outcome(0, 0, 2).map_position(0), 0);
        assert_eq!(outcome(usize::MAX, 9, 0).map_position(3), 3);
    }

    fn with_items() -> Paragraph {
        // An object and a break at offset 1, a symbol at the end.
        let mut p = Paragraph {
            text: "a\u{e9}c".into(),
            ..Paragraph::default()
        };
        let mut doc = super::super::RichDocument::new();
        let mut id = |_: u64| doc.alloc_id().unwrap();
        p.anchored.push(AnchoredObject {
            id: id(1),
            offset: 1,
            kind: "pic".into(),
            data: vec![],
        });
        p.breaks.push(InlineBreak {
            id: id(2),
            offset: 1,
            kind: super::super::BreakKind::Line,
        });
        p.symbols.push(InlineSymbol {
            id: id(3),
            offset: 4,
            font: "Symbol".into(),
            code: 0xf0b7,
        });
        p
    }

    #[test]
    fn paragraph_conversions_account_for_items() {
        let p = with_items();
        let flat = paragraph_flat(&p);
        assert_eq!(&*flat, "a\u{fffc}\u{fffc}\u{e9}c\u{fffc}");
        assert_eq!(flat.len(), 1 + 3 + 3 + 2 + 1 + 3);
        // before the first item and after the second both map to text offset 1
        assert_eq!(flat_to_text_offset(&p, 1), Ok(1));
        assert_eq!(flat_to_text_offset(&p, 4), Ok(1));
        assert_eq!(flat_to_text_offset(&p, 7), Ok(1));
        assert_eq!(flat_to_text_offset(&p, 2), Err(FlatError::NotBoundary));
        assert_eq!(flat_to_text_offset(&p, 8), Err(FlatError::NotBoundary));
        assert_eq!(flat_to_text_offset(&p, 10), Ok(4));
        assert_eq!(flat_to_text_offset(&p, 13), Ok(4));
        assert_eq!(flat_to_text_offset(&p, 14), Err(FlatError::OutOfRange));
        assert_eq!(text_to_flat_offset(&p, 0, false), Ok(0));
        assert_eq!(text_to_flat_offset(&p, 1, false), Ok(1));
        assert_eq!(text_to_flat_offset(&p, 1, true), Ok(7));
        assert_eq!(text_to_flat_offset(&p, 3, false), Ok(9));
        assert_eq!(text_to_flat_offset(&p, 4, true), Ok(13));
        assert_eq!(
            text_to_flat_offset(&p, 2, false),
            Err(FlatError::NotBoundary)
        );
        assert_eq!(
            text_to_flat_offset(&p, 5, false),
            Err(FlatError::OutOfRange)
        );
        let plain = Paragraph {
            text: "xyz".into(),
            ..Paragraph::default()
        };
        assert!(matches!(paragraph_flat(&plain), Cow::Borrowed("xyz")));
        assert_eq!(flat_to_text_offset(&plain, 3), Ok(3));
    }
}
