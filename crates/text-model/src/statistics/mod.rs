// SPDX-License-Identifier: AGPL-3.0-or-later
//! Document statistics (US-343): words, characters with and without spaces
//! and paragraphs of a rich document, a plain-text document or a selection,
//! with counting rules modelled on Microsoft Word (see
//! `docs/us-343-statistics.md` for the rules and their level of certainty).
//!
//! Lines and pages come from the layout, which this module does not know: the
//! caller passes them in [`StatisticsOptions`] and gets them back in
//! [`Statistics`].
//!
//! Layers: [`count_paragraph`] counts one paragraph; [`Counts`] values add and
//! subtract, which gives the incremental caches ([`StatisticsCache`] for the
//! rich model, [`PlainStatisticsCache`] for plain text); [`document`],
//! [`selection`], [`plain_text`] and [`plain_selection`] are the one-shot
//! entry points.

mod plain;
mod rich;
mod rules;

use crate::rich::NodeId;
use core::fmt;
use core::iter::Sum;
use core::ops::{Add, AddAssign, Sub, SubAssign};

pub use plain::{
    PlainStatisticsCache, plain_paragraphs, plain_selection, plain_text, plain_text_range,
};
pub use rich::{StatisticsCache, document, selection};
pub use rules::{
    CharClass, CharUnit, CountingRules, classify, count_paragraph, is_cjk_punctuation, is_ideograph,
};

/// The additive counters of a stretch of text. The counters of a document are
/// the sum of those of its paragraphs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    pub words: usize,
    /// Characters that are not spaces (spaces, tabs and line breaks excluded).
    pub characters: usize,
    /// All characters, paragraph marks excluded.
    pub characters_with_spaces: usize,
    /// Paragraphs counted (empty ones are skipped by default).
    pub paragraphs: usize,
}

impl Counts {
    /// Replaces the contribution `old` of a paragraph by `new`: the update
    /// done when a cached paragraph is edited. `old` must have been added.
    pub fn replace(&mut self, old: Counts, new: Counts) {
        *self += new;
        *self -= old;
    }

    /// `self - other`, or `None` when a counter would go below zero.
    pub fn checked_sub(self, other: Counts) -> Option<Counts> {
        Some(Counts {
            words: self.words.checked_sub(other.words)?,
            characters: self.characters.checked_sub(other.characters)?,
            characters_with_spaces: self
                .characters_with_spaces
                .checked_sub(other.characters_with_spaces)?,
            paragraphs: self.paragraphs.checked_sub(other.paragraphs)?,
        })
    }

    /// The counters with the layout figures supplied by the caller.
    pub fn with_layout(self, lines: Option<usize>, pages: Option<usize>) -> Statistics {
        Statistics {
            words: self.words,
            characters: self.characters,
            characters_with_spaces: self.characters_with_spaces,
            paragraphs: self.paragraphs,
            lines,
            pages,
        }
    }
}

impl Add for Counts {
    type Output = Counts;
    fn add(self, rhs: Counts) -> Counts {
        Counts {
            words: self.words + rhs.words,
            characters: self.characters + rhs.characters,
            characters_with_spaces: self.characters_with_spaces + rhs.characters_with_spaces,
            paragraphs: self.paragraphs + rhs.paragraphs,
        }
    }
}

impl AddAssign for Counts {
    fn add_assign(&mut self, rhs: Counts) {
        *self = *self + rhs;
    }
}

/// Saturating subtraction; use [`Counts::checked_sub`] to detect underflow.
impl Sub for Counts {
    type Output = Counts;
    fn sub(self, rhs: Counts) -> Counts {
        Counts {
            words: self.words.saturating_sub(rhs.words),
            characters: self.characters.saturating_sub(rhs.characters),
            characters_with_spaces: self
                .characters_with_spaces
                .saturating_sub(rhs.characters_with_spaces),
            paragraphs: self.paragraphs.saturating_sub(rhs.paragraphs),
        }
    }
}

impl SubAssign for Counts {
    fn sub_assign(&mut self, rhs: Counts) {
        *self = *self - rhs;
    }
}

impl Sum for Counts {
    fn sum<I: Iterator<Item = Counts>>(iter: I) -> Counts {
        iter.fold(Counts::default(), |a, b| a + b)
    }
}

/// The statistics of a document or a selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Statistics {
    pub words: usize,
    /// Characters without spaces.
    pub characters: usize,
    pub characters_with_spaces: usize,
    pub paragraphs: usize,
    /// Echo of [`StatisticsOptions::lines`]: not computed here.
    pub lines: Option<usize>,
    /// Echo of [`StatisticsOptions::pages`]: not computed here.
    pub pages: Option<usize>,
}

impl Statistics {
    /// The additive part.
    pub fn counts(&self) -> Counts {
        Counts {
            words: self.words,
            characters: self.characters,
            characters_with_spaces: self.characters_with_spaces,
            paragraphs: self.paragraphs,
        }
    }
}

/// What is counted and how.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsOptions {
    /// Count the headers and footers of every section (default `false`).
    pub include_headers_footers: bool,
    /// Count footnotes, endnotes and text boxes (default `false`). The rich
    /// model carries none of them yet (anchored objects are opaque), so this
    /// flag changes nothing today; it exists so that callers can already wire
    /// the "include notes and text boxes" checkbox of the dialog.
    pub include_notes_and_text_boxes: bool,
    pub rules: CountingRules,
    /// Number of lines, from the layout, restituted in [`Statistics::lines`].
    pub lines: Option<usize>,
    /// Number of pages, from the layout, restituted in [`Statistics::pages`].
    pub pages: Option<usize>,
}

/// A selection that cannot be counted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsError {
    /// The node does not exist in the document.
    UnknownNode(NodeId),
    /// The node exists but is not a paragraph.
    NotAParagraph(NodeId),
    /// An offset is beyond the end of its text.
    OffsetOutOfBounds { offset: usize, len: usize },
    /// An offset is inside a UTF-8 sequence.
    NotCharBoundary { offset: usize },
    /// The two ends of a range lie in different stories (body, header...).
    RangeAcrossStories,
    /// The paragraph is not in the cache: call `sync`.
    NotCached(NodeId),
}

impl fmt::Display for StatisticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(f, "node {id:?} not found"),
            Self::NotAParagraph(id) => write!(f, "node {id:?} is not a paragraph"),
            Self::OffsetOutOfBounds { offset, len } => {
                write!(f, "offset {offset} is beyond the text length {len}")
            }
            Self::NotCharBoundary { offset } => {
                write!(f, "offset {offset} is not a UTF-8 character boundary")
            }
            Self::RangeAcrossStories => {
                f.write_str("the range starts and ends in different stories")
            }
            Self::NotCached(id) => write!(f, "paragraph {id:?} is not in the statistics cache"),
        }
    }
}

impl std::error::Error for StatisticsError {}
