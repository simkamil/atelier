// SPDX-License-Identifier: AGPL-3.0-or-later
//! Flat-position contract of the rich editor (US-203, implemented by US-204).
//!
//! Declarations only, apart from [`EditOutcome::map_position`]. R1 implements
//! [`FlatIndex`], [`ParagraphAccess`] and [`FlatEdit`] for `RichDocument`; the
//! protocol layer (`atelier-core-wasm::rich_protocol`) and the layout cache
//! (`rich_layout`) depend only on these signatures. Full rules:
//! `docs/us-203-rich-editor-protocol.md`.
//!
//! Flat positions are an interchange representation of the protocol only:
//! the model and the recovery journal keep node id + offset (SPEC 6.3) and
//! never store flat positions; these traits convert on demand.
//!
//! Flat text of a document: the body paragraphs in order, joined by one `\n`
//! byte (no trailing `\n`). Every inline item that is not text counts as
//! U+FFFC, three bytes ([`OBJECT_LEN`]): images and anchored objects, line,
//! page and column breaks (`breaks`) and symbols (`symbols`). `run_styles` is
//! a run attribute (a character style in `CharAttrs`) and counts 0 bytes.
//! Everything else is its UTF-8. A position is a byte offset in `0..=len()`.
//!
//! Clusters. Boundaries follow UAX #29 with these overrides, whatever the
//! UAX rules say: there is always a boundary before and after U+FFFC and
//! before and after every `\n`. A combining mark inserted right after an
//! object therefore forms its own cluster. A **valid position** is a
//! boundary of this segmentation (so also a scalar boundary, never inside
//! U+FFFC). Both [`FlatEdit::replace_range`] ends must be valid positions.

/// Bytes of one non-text inline item (object, break, symbol): U+FFFC.
pub const OBJECT_LEN: usize = 3;
/// Bytes of the separator between two paragraphs.
pub const SEPARATOR_LEN: usize = 1;

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
    /// Text contains U+FFFC, `\r`, or a control character other than `\n`/`\t`.
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

/// Prefix-sum index over body paragraphs: each operation is `O(log n)` plus
/// the length of one paragraph, except [`FlatIndex::text`], linear in the
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
    /// Start of the word at or before `position` (UAX #29 words; objects and
    /// `\n` are words of their own), `None` at 0.
    fn prev_word(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// End of the word at or after `position`, `None` at `len()`.
    fn next_word(&self, position: usize) -> Result<Option<usize>, FlatError>;
    /// Enclosing word `(start, end)`; at the end of the document, the last
    /// word; `None` for an empty document.
    fn word_at(&self, position: usize) -> Result<Option<(usize, usize)>, FlatError>;
}

/// Read access to the paragraphs behind the index, for layout. Object-safe.
pub trait ParagraphAccess: FlatIndex {
    fn paragraph(&self, index: usize) -> Option<&super::Paragraph>;
    /// Flat position of the first byte of paragraph `index`.
    fn paragraph_start(&self, index: usize) -> Result<usize, FlatError>;
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
    /// during one command.
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
}
