// SPDX-License-Identifier: AGPL-3.0-or-later
//! Flat-position contract of the rich editor (US-203, implemented by US-204).
//!
//! Only declarations: nothing here is implemented yet. R1 implements
//! [`FlatIndex`] and [`FlatEdit`] for `RichDocument`; the protocol layer
//! (`atelier-core-wasm::rich_protocol`) and the layout cache only depend on
//! these signatures. Full rules: `docs/us-203-rich-editor-protocol.md`.
//!
//! Flat positions are an interchange representation of the protocol only:
//! the model and the recovery journal keep node id + offset (SPEC §6.3) and
//! never store flat positions; this trait converts on demand.
//!
//! Flat text of a document: the body paragraphs in order, joined by one
//! `\n` byte (no trailing `\n`); an inline object counts as U+FFFC (three
//! bytes, [`OBJECT_LEN`]): images, anchored objects, and the `breaks` and
//! `symbols` side tables once R1 integrates them as inline items; `run_styles`
//! are run attributes and count 0 bytes; everything else is its UTF-8 text. A position is a
//! byte offset in `0..=len()`. A valid position lies on a scalar boundary and
//! never inside U+FFFC. Positions inside a grapheme cluster are valid for
//! [`FlatEdit::replace_range`] but never reported back as a selection.

/// Bytes of one inline object, break or symbol in flat text (U+FFFC).
pub const OBJECT_LEN: usize = 3;
/// Bytes of the separator between two paragraphs.
pub const SEPARATOR_LEN: usize = 1;

/// Why a flat position or edit was refused. State is unchanged on error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatError {
    /// A position is greater than `len()`.
    OutOfRange,
    /// A position is not on a scalar boundary or falls inside an object.
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

/// Prefix-sum index over body paragraphs: every operation is `O(log n)`
/// except [`FlatIndex::text`], which is linear in the returned bytes.
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
    /// Checks the validity rules above for one position.
    fn check_position(&self, position: usize) -> Result<(), FlatError>;
    /// Flat text of `start..end` (objects as U+FFFC, separators as `\n`).
    fn text(&self, start: usize, end: usize) -> Result<String, FlatError>;
}

/// What one applied edit changed, enough for incremental layout and for
/// remapping side tables with `position_map`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    /// Model revision after the edit (increments by one per applied edit).
    pub revision: u64,
    /// First paragraph index touched.
    pub first_paragraph: usize,
    /// Paragraphs replaced, counted before the edit.
    pub removed_paragraphs: usize,
    /// Paragraphs that replace them, counted after the edit.
    pub inserted_paragraphs: usize,
    /// Selection `(anchor, focus)` to apply after the edit, in new positions.
    pub selection: (usize, usize),
}

/// In-place editing with history. No clone of the document, history or side
/// tables per call.
pub trait FlatEdit: FlatIndex {
    /// Replaces `start..end` by `text` (`\n` splits paragraphs, the new
    /// paragraph takes the style that follows the split; a range spanning
    /// separators merges the paragraphs it joins). Deleting is an empty text.
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
