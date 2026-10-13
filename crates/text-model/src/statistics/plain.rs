// SPDX-License-Identifier: AGPL-3.0-or-later
//! Statistics of plain text: paragraphs are the lines separated by LF, CRLF,
//! CR or U+2029; the counting itself is the shared [`count_paragraph`].

use super::rules::count_paragraph;
use super::{Counts, Statistics, StatisticsError, StatisticsOptions};
use crate::PlainTextDocument;
use core::ops::Range;

/// Paragraph spans of `text`: `(start, content_end, end)` where
/// `content_end..end` is the terminator. A text ending with a terminator has
/// a last, empty paragraph.
fn split(text: &str) -> Vec<(usize, usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        let term = match bytes[i] {
            b'\n' => 1,
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => 2,
            b'\r' => 1,
            0xe2 if bytes[i..].starts_with("\u{2029}".as_bytes()) => 3,
            _ => 0,
        };
        if term == 0 {
            i += 1;
        } else {
            out.push((start, i, i + term));
            i += term;
            start = i;
        }
    }
    out.push((start, bytes.len(), bytes.len()));
    out
}

/// Byte ranges of the paragraphs of `text`, terminators excluded.
pub fn plain_paragraphs(text: &str) -> Vec<Range<usize>> {
    split(text).into_iter().map(|(s, e, _)| s..e).collect()
}

fn sum(text: &str, options: &StatisticsOptions, spans: &[(usize, usize, usize)]) -> Counts {
    spans
        .iter()
        .map(|&(s, e, _)| count_paragraph(&text[s..e], &options.rules))
        .sum()
}

/// Statistics of a whole plain text.
pub fn plain_text(text: &str, options: &StatisticsOptions) -> Statistics {
    sum(text, options, &split(text)).with_layout(options.lines, options.pages)
}

/// Statistics of the part `range` of a plain text (byte offsets). A word cut
/// by a bound counts for one word; an empty range gives zeros.
pub fn plain_text_range(
    text: &str,
    range: Range<usize>,
    options: &StatisticsOptions,
) -> Result<Statistics, StatisticsError> {
    let (start, end) = (range.start.min(range.end), range.start.max(range.end));
    for offset in [start, end] {
        if offset > text.len() {
            return Err(StatisticsError::OffsetOutOfBounds {
                offset,
                len: text.len(),
            });
        }
        if !text.is_char_boundary(offset) {
            return Err(StatisticsError::NotCharBoundary { offset });
        }
    }
    if start == end {
        return Ok(Statistics {
            lines: options.lines,
            pages: options.pages,
            ..Statistics::default()
        });
    }
    let slice = &text[start..end];
    let mut spans = split(slice);
    // The empty piece after a final terminator is not part of the selection.
    if spans.len() > 1 && spans.last().is_some_and(|s| s.0 == slice.len()) {
        spans.pop();
    }
    Ok(sum(slice, options, &spans).with_layout(options.lines, options.pages))
}

/// Statistics of the current selection of a plain-text document.
pub fn plain_selection(doc: &PlainTextDocument, options: &StatisticsOptions) -> Statistics {
    let sel = doc.selection();
    // The document guarantees valid offsets.
    plain_text_range(doc.text(), sel.anchor..sel.focus, options).unwrap_or_default()
}

/// Per-paragraph counts of a plain text, kept up to date from edits.
///
/// After replacing the bytes `start..old_end` of the text by `new_len` bytes,
/// [`PlainStatisticsCache::update`] recounts only the paragraphs around the
/// edit and shifts the others: the cost is linear in the number of
/// paragraphs for the lookup and in the size of the touched paragraphs for the
/// counting, never a recount of the whole text.
#[derive(Clone, Debug)]
pub struct PlainStatisticsCache {
    options: StatisticsOptions,
    /// `(bytes including terminator, counts)` per paragraph.
    paragraphs: Vec<(usize, Counts)>,
    total: Counts,
    text_len: usize,
}

impl PlainStatisticsCache {
    /// Counts `text` completely.
    pub fn build(text: &str, options: StatisticsOptions) -> Self {
        let paragraphs: Vec<(usize, Counts)> = split(text)
            .into_iter()
            .map(|(s, e, end)| (end - s, count_paragraph(&text[s..e], &options.rules)))
            .collect();
        let total = paragraphs.iter().map(|p| p.1).sum();
        Self {
            options,
            paragraphs,
            total,
            text_len: text.len(),
        }
    }

    pub fn counts(&self) -> Counts {
        self.total
    }

    pub fn statistics(&self) -> Statistics {
        self.total
            .with_layout(self.options.lines, self.options.pages)
    }

    /// Sets the layout figures restituted by [`Self::statistics`].
    pub fn set_layout(&mut self, lines: Option<usize>, pages: Option<usize>) {
        self.options.lines = lines;
        self.options.pages = pages;
    }

    /// Number of paragraphs held (empty ones included).
    pub fn paragraph_len(&self) -> usize {
        self.paragraphs.len()
    }

    /// Takes into account that the bytes `start..old_end` of the previous text
    /// were replaced by `new_len` bytes; `text` is the new complete text.
    /// Falls back to a full recount if the arguments do not match the cache.
    pub fn update(&mut self, text: &str, start: usize, old_end: usize, new_len: usize) {
        let consistent = start <= old_end
            && old_end <= self.text_len
            && self.text_len - (old_end - start) + new_len == text.len();
        if !consistent {
            *self = Self::build(text, self.options);
            return;
        }
        // Paragraph indexes (and their start offsets) touching the edit, widened
        // by one on each side so that a CR/LF pair formed or broken by the edit
        // is recounted whole.
        let n = self.paragraphs.len();
        let (mut first, mut last) = (n - 1, n - 1);
        let (mut first_start, mut last_end) = (0, self.text_len);
        let (mut at, mut found_first) = (0, false);
        for (i, &(len, _)) in self.paragraphs.iter().enumerate() {
            let p_end = at + len;
            if !found_first && start < p_end {
                (first, first_start, found_first) = (i, at, true);
            }
            if old_end < p_end {
                (last, last_end) = (i, p_end);
                break;
            }
            at = p_end;
        }
        if !found_first {
            first_start = at_of_last(&self.paragraphs);
        }
        if first > 0 {
            first -= 1;
            first_start -= self.paragraphs[first].0;
        }
        if last + 1 < n {
            last += 1;
            last_end += self.paragraphs[last].0;
        }
        let new_end = last_end - (old_end - start) + new_len;
        let region = &text[first_start..new_end];
        let mut spans = split(region);
        if last + 1 < n {
            // The region ends with a terminator: no paragraph follows it.
            spans.pop();
        }
        let fresh: Vec<(usize, Counts)> = spans
            .into_iter()
            .map(|(s, e, end)| (end - s, count_paragraph(&region[s..e], &self.options.rules)))
            .collect();
        let removed: Counts = self.paragraphs[first..=last].iter().map(|p| p.1).sum();
        let added: Counts = fresh.iter().map(|p| p.1).sum();
        self.total.replace(removed, added);
        self.paragraphs.splice(first..=last, fresh);
        self.text_len = text.len();
    }
}

/// Start offset of the last paragraph.
fn at_of_last(paragraphs: &[(usize, Counts)]) -> usize {
    paragraphs[..paragraphs.len() - 1].iter().map(|p| p.0).sum()
}
