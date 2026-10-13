// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the layout must tell the Go To command (US-291).
//!
//! Pages and lines come from the layout crate, which depends on this one: the
//! integrator implements [`PageMap`] over its layout; [`FixedPageMap`] is a
//! plain implementation for tests. Numbers are 1-based.

use super::Reading;
use crate::rich::RichDocument;
use atelier_model::selection::TextPosition;

/// Page and line correspondence of the laid-out document.
pub trait PageMap {
    fn page_count(&self) -> usize;
    /// Start of page `page` (1-based); `None` out of range.
    fn page_start(&self, page: usize) -> Option<TextPosition>;
    /// Page holding `position` (1-based, `0` if unknown).
    fn page_of(&self, position: TextPosition) -> usize;
    fn line_count(&self) -> usize;
    fn line_start(&self, line: usize) -> Option<TextPosition>;
    fn line_of(&self, position: TextPosition) -> usize;
}

/// A page map made of explicit start positions, in reading order.
pub struct FixedPageMap {
    pages: Vec<TextPosition>,
    lines: Vec<TextPosition>,
    keys_pages: Vec<(usize, usize)>,
    keys_lines: Vec<(usize, usize)>,
    reading: Reading,
}

impl FixedPageMap {
    /// `pages` and `lines` are the start of each page and each line, sorted.
    /// A position outside `doc` is ranked first.
    pub fn new(doc: &RichDocument, pages: Vec<TextPosition>, lines: Vec<TextPosition>) -> Self {
        let reading = Reading::new(doc);
        let keys = |v: &[TextPosition]| {
            v.iter()
                .map(|p| reading.key(*p).unwrap_or((0, 0)))
                .collect()
        };
        Self {
            keys_pages: keys(&pages),
            keys_lines: keys(&lines),
            pages,
            lines,
            reading,
        }
    }

    fn rank(&self, keys: &[(usize, usize)], position: TextPosition) -> usize {
        let key = self.reading.key(position).unwrap_or((0, 0));
        keys.partition_point(|k| *k <= key)
    }
}

impl PageMap for FixedPageMap {
    fn page_count(&self) -> usize {
        self.pages.len()
    }
    fn page_start(&self, page: usize) -> Option<TextPosition> {
        page.checked_sub(1).and_then(|i| self.pages.get(i)).copied()
    }
    fn page_of(&self, position: TextPosition) -> usize {
        self.rank(&self.keys_pages, position)
    }
    fn line_count(&self) -> usize {
        self.lines.len()
    }
    fn line_start(&self, line: usize) -> Option<TextPosition> {
        line.checked_sub(1).and_then(|i| self.lines.get(i)).copied()
    }
    fn line_of(&self, position: TextPosition) -> usize {
        self.rank(&self.keys_lines, position)
    }
}
