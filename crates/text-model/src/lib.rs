// SPDX-License-Identifier: AGPL-3.0-or-later
//! A bounded, undoable owner for UTF-8 plain text.
//!
//! Positions are global UTF-8 byte offsets. They are intentionally not the
//! stable `NodeId` positions used by the future rich document model.

#![forbid(unsafe_code)]

pub mod rich;

use atelier_model::{EditKind, History, HistoryError, Operation, Transaction, TransactionalState};
use atelier_unicode::segmentation::{OffsetError, grapheme_boundaries};
use core::fmt;

const UTF8_BOM: &[u8; 3] = b"\xef\xbb\xbf";
// Covers the private History entry plus its deque slot, the Transaction Vec
// allocation containing ReplaceText, allocator headers for the operation and
// owned strings, and a generous fixed implementation margin. Variable string
// bytes are accounted for separately in `apply_range`.
const fn history_entry_overhead() -> usize {
    core::mem::size_of::<ReplaceText>()
        + core::mem::size_of::<Selection>() * 2
        + core::mem::size_of::<usize>() * 16
        + 256
}

/// Resource limits for a plain-text document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlainTextLimits {
    /// Maximum UTF-8 bytes in the text, excluding an optional BOM.
    pub max_text_bytes: usize,
    /// Conservative logical budget for retained undo and redo entries.
    pub max_history_bytes: usize,
}

impl Default for PlainTextLimits {
    fn default() -> Self {
        Self {
            max_text_bytes: 16 * 1024 * 1024,
            max_history_bytes: 32 * 1024 * 1024,
        }
    }
}

/// A directed selection in UTF-8 byte offsets. `anchor` and `focus` may be
/// reversed; equal offsets represent a caret.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub const fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            focus: offset,
        }
    }
    fn range(self) -> core::ops::Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }
}

/// Failure while importing or editing plain text.
#[derive(Debug, Eq, PartialEq)]
pub enum PlainTextError {
    InvalidUtf8,
    TextTooLarge { limit: usize, actual: usize },
    InvalidOffset { offset: usize, len: usize },
    OffsetNotCharBoundary { offset: usize },
    AllocationFailed,
    HistoryBudgetExceeded { limit: usize, required: usize },
    HistoryFailure,
}

impl fmt::Display for PlainTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("input is not valid UTF-8"),
            Self::TextTooLarge { limit, actual } => {
                write!(f, "text has {actual} bytes; limit is {limit}")
            }
            Self::InvalidOffset { offset, len } => {
                write!(f, "byte offset {offset} exceeds text length {len}")
            }
            Self::OffsetNotCharBoundary { offset } => {
                write!(f, "byte offset {offset} is not a UTF-8 character boundary")
            }
            Self::AllocationFailed => f.write_str("could not reserve text storage"),
            Self::HistoryBudgetExceeded { limit, required } => write!(
                f,
                "history entry requires {required} bytes; limit is {limit}"
            ),
            Self::HistoryFailure => f.write_str("history operation failed"),
        }
    }
}
impl std::error::Error for PlainTextError {}

#[derive(Clone)]
struct TextState(String);
impl TransactionalState for TextState {
    type Checkpoint = String;
    fn checkpoint(&self) -> String {
        self.0.clone()
    }
    fn restore(&mut self, checkpoint: String) {
        self.0 = checkpoint;
    }
}

#[derive(Clone)]
struct ReplaceText {
    start: usize,
    removed: String,
    inserted: String,
}
impl Operation<TextState> for ReplaceText {
    type Error = ();
    fn apply(&self, state: &mut TextState) -> Result<(), ()> {
        let end = self.start.checked_add(self.removed.len()).ok_or(())?;
        if end > state.0.len()
            || !state.0.is_char_boundary(self.start)
            || !state.0.is_char_boundary(end)
            || state.0.get(self.start..end) != Some(self.removed.as_str())
        {
            return Err(());
        }
        state.0.replace_range(self.start..end, &self.inserted);
        Ok(())
    }
    fn invert(&self, state: &TextState) -> Result<Self, ()> {
        let end = self.start.checked_add(self.removed.len()).ok_or(())?;
        let inserted = state.0.get(self.start..end).ok_or(())?.to_owned();
        Ok(Self {
            start: self.start,
            removed: self.inserted.clone(),
            inserted,
        })
    }
    fn encode(
        &self,
        encoder: &mut atelier_model::Encoder,
    ) -> Result<(), atelier_model::CodecError> {
        encoder.write_u64(
            u64::try_from(self.start).map_err(|_| atelier_model::CodecError::TooLarge)?,
        )?;
        encoder.write_u32(
            u32::try_from(self.removed.len()).map_err(|_| atelier_model::CodecError::TooLarge)?,
        )?;
        encoder.write_bytes(self.removed.as_bytes())?;
        encoder.write_u32(
            u32::try_from(self.inserted.len()).map_err(|_| atelier_model::CodecError::TooLarge)?,
        )?;
        encoder.write_bytes(self.inserted.as_bytes())
    }
    fn decode(_: &mut atelier_model::Decoder<'_>) -> Result<Self, atelier_model::CodecError> {
        Err(atelier_model::CodecError::InvalidData)
    }
}

/// Owned plain text with directed selection and bounded undo/redo history.
#[derive(Clone)]
pub struct PlainTextDocument {
    state: TextState,
    history: History<ReplaceText, Selection>,
    limits: PlainTextLimits,
    had_bom: bool,
}

impl PlainTextDocument {
    /// Imports UTF-8 without replacement or transcoding. A leading UTF-8 BOM is
    /// retained as metadata and excluded from text offsets.
    pub fn from_utf8(bytes: &[u8], limits: PlainTextLimits) -> Result<Self, PlainTextError> {
        let had_bom = bytes.starts_with(UTF8_BOM);
        let text_bytes = if had_bom { &bytes[3..] } else { bytes };
        if text_bytes.len() > limits.max_text_bytes {
            return Err(PlainTextError::TextTooLarge {
                limit: limits.max_text_bytes,
                actual: text_bytes.len(),
            });
        }
        let text = core::str::from_utf8(text_bytes).map_err(|_| PlainTextError::InvalidUtf8)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|_| PlainTextError::AllocationFailed)?;
        owned.push_str(text);
        let selection = Selection::default();
        Ok(Self {
            state: TextState(owned),
            history: History::new(selection, limits.max_history_bytes),
            limits,
            had_bom,
        })
    }
    pub fn text(&self) -> &str {
        &self.state.0
    }
    pub fn selection(&self) -> Selection {
        *self.history.selection()
    }
    pub fn limits(&self) -> PlainTextLimits {
        self.limits
    }
    pub fn export_utf8(&self) -> Result<Vec<u8>, PlainTextError> {
        let total = self
            .state
            .0
            .len()
            .checked_add(if self.had_bom { 3 } else { 0 })
            .ok_or(PlainTextError::AllocationFailed)?;
        let mut out = Vec::new();
        out.try_reserve_exact(total)
            .map_err(|_| PlainTextError::AllocationFailed)?;
        if self.had_bom {
            out.extend_from_slice(UTF8_BOM);
        }
        out.extend_from_slice(self.state.0.as_bytes());
        Ok(out)
    }
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), PlainTextError> {
        validate_offset(&self.state.0, selection.anchor)?;
        validate_offset(&self.state.0, selection.focus)?;
        self.history.move_cursor(selection);
        Ok(())
    }
    /// Inserts exactly the supplied UTF-8 text, replacing any selected range.
    pub fn insert_text(&mut self, text: &str, now_ms: u64) -> Result<(), PlainTextError> {
        self.replace_selection(
            text,
            now_ms,
            EditKind::Typing {
                segment: 0,
                closes_after: text.chars().last().is_some_and(char::is_whitespace),
            },
        )
    }
    /// Removes the selected range or the previous extended grapheme cluster.
    /// CRLF is one grapheme and is deleted atomically.
    pub fn delete_backward(&mut self, now_ms: u64) -> Result<bool, PlainTextError> {
        let selection = self.selection();
        let range = selection.range();
        let range = if range.is_empty() {
            let boundaries = grapheme_boundaries(&self.state.0);
            if !boundaries
                .is_boundary(selection.focus)
                .map_err(map_offset)?
            {
                boundaries
                    .floor_boundary(selection.focus)
                    .map_err(map_offset)?
                    ..boundaries
                        .ceil_boundary(selection.focus)
                        .map_err(map_offset)?
            } else {
                let Some(start) = boundaries
                    .previous_boundary(selection.focus)
                    .map_err(map_offset)?
                else {
                    return Ok(false);
                };
                start..selection.focus
            }
        } else {
            range
        };
        self.apply_range(range, "", now_ms, EditKind::Other)?;
        Ok(true)
    }
    /// Removes the selected range or the next extended grapheme cluster.
    pub fn delete_forward(&mut self, now_ms: u64) -> Result<bool, PlainTextError> {
        let selection = self.selection();
        let range = selection.range();
        let range = if range.is_empty() {
            let boundaries = grapheme_boundaries(&self.state.0);
            if !boundaries
                .is_boundary(selection.focus)
                .map_err(map_offset)?
            {
                boundaries
                    .floor_boundary(selection.focus)
                    .map_err(map_offset)?
                    ..boundaries
                        .ceil_boundary(selection.focus)
                        .map_err(map_offset)?
            } else {
                let Some(end) = boundaries
                    .next_boundary(selection.focus)
                    .map_err(map_offset)?
                else {
                    return Ok(false);
                };
                selection.focus..end
            }
        } else {
            range
        };
        self.apply_range(range, "", now_ms, EditKind::Other)?;
        Ok(true)
    }
    pub fn undo(&mut self) -> Result<bool, PlainTextError> {
        self.history.undo(&mut self.state).map_err(map_history)
    }
    pub fn redo(&mut self) -> Result<bool, PlainTextError> {
        self.history.redo(&mut self.state).map_err(map_history)
    }
    pub fn can_undo(&self) -> bool {
        self.history.undo_len() != 0
    }
    pub fn can_redo(&self) -> bool {
        self.history.redo_len() != 0
    }

    fn replace_selection(
        &mut self,
        inserted: &str,
        now_ms: u64,
        kind: EditKind,
    ) -> Result<(), PlainTextError> {
        let range = self.selection().range();
        self.apply_range(range, inserted, now_ms, kind)
    }
    fn apply_range(
        &mut self,
        range: core::ops::Range<usize>,
        inserted: &str,
        now_ms: u64,
        kind: EditKind,
    ) -> Result<(), PlainTextError> {
        if range.is_empty() && inserted.is_empty() {
            return Ok(());
        }
        let new_len = self
            .state
            .0
            .len()
            .checked_sub(range.len())
            .and_then(|n| n.checked_add(inserted.len()))
            .ok_or(PlainTextError::TextTooLarge {
                limit: self.limits.max_text_bytes,
                actual: usize::MAX,
            })?;
        if new_len > self.limits.max_text_bytes {
            return Err(PlainTextError::TextTooLarge {
                limit: self.limits.max_text_bytes,
                actual: new_len,
            });
        }
        let removed_len = range.len();
        let bound = removed_len
            .checked_add(inserted.len())
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(history_entry_overhead()))
            .ok_or(PlainTextError::AllocationFailed)?;
        if bound > self.limits.max_history_bytes {
            return Err(PlainTextError::HistoryBudgetExceeded {
                limit: self.limits.max_history_bytes,
                required: bound,
            });
        }
        let removed_slice =
            self.state
                .0
                .get(range.clone())
                .ok_or(PlainTextError::InvalidOffset {
                    offset: range.end,
                    len: self.state.0.len(),
                })?;
        let mut removed = String::new();
        removed
            .try_reserve_exact(removed_len)
            .map_err(|_| PlainTextError::AllocationFailed)?;
        removed.push_str(removed_slice);
        let next = range
            .start
            .checked_add(inserted.len())
            .ok_or(PlainTextError::AllocationFailed)?;
        let after = Selection::caret(next);
        let mut inserted_owned = String::new();
        inserted_owned
            .try_reserve_exact(inserted.len())
            .map_err(|_| PlainTextError::AllocationFailed)?;
        inserted_owned.push_str(inserted);
        self.state
            .0
            .try_reserve(inserted.len().saturating_sub(removed.len()))
            .map_err(|_| PlainTextError::AllocationFailed)?;
        let operation = ReplaceText {
            start: range.start,
            removed,
            inserted: inserted_owned,
        };
        let transaction =
            Transaction::new(vec![operation]).map_err(|_| PlainTextError::HistoryFailure)?;
        self.history
            .apply(&mut self.state, transaction, after, now_ms, kind, bound)
            .map_err(map_history)
    }
}

fn validate_offset(text: &str, offset: usize) -> Result<(), PlainTextError> {
    if offset > text.len() {
        return Err(PlainTextError::InvalidOffset {
            offset,
            len: text.len(),
        });
    }
    if !text.is_char_boundary(offset) {
        return Err(PlainTextError::OffsetNotCharBoundary { offset });
    }
    Ok(())
}
fn map_offset(error: OffsetError) -> PlainTextError {
    match error {
        OffsetError::OutOfBounds { offset, len } => PlainTextError::InvalidOffset { offset, len },
        OffsetError::NotCharBoundary { offset } => PlainTextError::OffsetNotCharBoundary { offset },
    }
}
fn map_history<E>(error: HistoryError<E>) -> PlainTextError {
    match error {
        HistoryError::BudgetExceeded { budget, required } => {
            PlainTextError::HistoryBudgetExceeded {
                limit: budget,
                required,
            }
        }
        _ => PlainTextError::HistoryFailure,
    }
}
