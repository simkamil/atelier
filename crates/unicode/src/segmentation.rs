// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unicode 17.0.0 default extended grapheme and word segmentation (UAX #29 rev. 47).

use core::ops::Range;

/// Invalid byte offset supplied to a boundary index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OffsetError {
    /// The offset exceeds the byte length of the indexed text.
    OutOfBounds { offset: usize, len: usize },
    /// The offset points into a UTF-8 scalar encoding.
    NotCharBoundary { offset: usize },
}

impl core::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfBounds { offset, len } => {
                write!(f, "byte offset {offset} exceeds text length {len}")
            }
            Self::NotCharBoundary { offset } => {
                write!(f, "byte offset {offset} is not a UTF-8 character boundary")
            }
        }
    }
}
impl std::error::Error for OffsetError {}

/// A borrowed text and its sorted UTF-8 byte boundaries.
#[derive(Clone, Debug)]
pub struct BoundaryIndex<'a> {
    text: &'a str,
    boundaries: Vec<usize>,
}

impl<'a> BoundaryIndex<'a> {
    fn new(text: &'a str, boundaries: Vec<usize>) -> Self {
        Self { text, boundaries }
    }
    /// All boundary byte offsets, including 0 and the text length.
    pub fn boundaries(&self) -> &[usize] {
        &self.boundaries
    }
    /// Iterate over all half-open segments.
    pub fn segments(&self) -> Segments<'_> {
        Segments {
            boundaries: &self.boundaries,
            front: 0,
            back: self.boundaries.len().saturating_sub(1),
        }
    }
    fn validate(&self, offset: usize) -> Result<(), OffsetError> {
        if offset > self.text.len() {
            return Err(OffsetError::OutOfBounds {
                offset,
                len: self.text.len(),
            });
        }
        if !self.text.is_char_boundary(offset) {
            return Err(OffsetError::NotCharBoundary { offset });
        }
        Ok(())
    }
    /// Whether an offset is a segment boundary.
    pub fn is_boundary(&self, offset: usize) -> Result<bool, OffsetError> {
        self.validate(offset)?;
        Ok(self.boundaries.binary_search(&offset).is_ok())
    }
    /// Greatest boundary less than or equal to an offset.
    pub fn floor_boundary(&self, offset: usize) -> Result<usize, OffsetError> {
        self.validate(offset)?;
        let i = self.boundaries.partition_point(|&b| b <= offset);
        Ok(self.boundaries[i - 1])
    }
    /// Least boundary greater than or equal to an offset.
    pub fn ceil_boundary(&self, offset: usize) -> Result<usize, OffsetError> {
        self.validate(offset)?;
        let i = self.boundaries.partition_point(|&b| b < offset);
        Ok(self.boundaries[i])
    }
    /// Strictly previous boundary, if one exists.
    pub fn previous_boundary(&self, offset: usize) -> Result<Option<usize>, OffsetError> {
        self.validate(offset)?;
        let i = self.boundaries.partition_point(|&b| b < offset);
        Ok(i.checked_sub(1).map(|n| self.boundaries[n]))
    }
    /// Strictly next boundary, if one exists.
    pub fn next_boundary(&self, offset: usize) -> Result<Option<usize>, OffsetError> {
        self.validate(offset)?;
        let i = self.boundaries.partition_point(|&b| b <= offset);
        Ok(self.boundaries.get(i).copied())
    }
    /// Segments whose end is greater than `offset`, in forward order.
    pub fn forward_from(
        &self,
        offset: usize,
    ) -> Result<impl Iterator<Item = Range<usize>> + '_, OffsetError> {
        self.validate(offset)?;
        let first = self
            .boundaries
            .partition_point(|&b| b <= offset)
            .saturating_sub(1);
        Ok(Segments {
            boundaries: &self.boundaries,
            front: first,
            back: self.boundaries.len().saturating_sub(1),
        })
    }
    /// Segments whose start is less than `offset`, in reverse order.
    pub fn backward_from(
        &self,
        offset: usize,
    ) -> Result<impl DoubleEndedIterator<Item = Range<usize>> + '_, OffsetError> {
        self.validate(offset)?;
        let end = self.boundaries.partition_point(|&b| b < offset);
        Ok(Segments {
            boundaries: &self.boundaries,
            front: 0,
            back: end,
        }
        .rev())
    }
}

/// Double-ended, constant-time-per-item iterator over segment byte ranges.
#[derive(Clone, Debug)]
pub struct Segments<'a> {
    boundaries: &'a [usize],
    front: usize,
    back: usize,
}
impl Iterator for Segments<'_> {
    type Item = Range<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }
        let i = self.front;
        self.front += 1;
        Some(self.boundaries[i]..self.boundaries[i + 1])
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.back.saturating_sub(self.front);
        (n, Some(n))
    }
}
impl DoubleEndedIterator for Segments<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }
        self.back -= 1;
        Some(self.boundaries[self.back]..self.boundaries[self.back + 1])
    }
}
impl ExactSizeIterator for Segments<'_> {}
impl core::iter::FusedIterator for Segments<'_> {}

/// Compute default extended grapheme cluster boundaries.
pub fn grapheme_boundaries(text: &str) -> BoundaryIndex<'_> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = vec![0];
    let (mut indic_consonant_chain, mut indic_linker_seen, mut ep_extend_chain, mut ri_run) =
        (false, false, false, 0usize);
    let mut zwj_after_ep = vec![false; chars.len()];
    for (i, &(_, c)) in chars.iter().enumerate() {
        let g = crate::properties::grapheme_cluster_break_property(c);
        zwj_after_ep[i] = g == "ZWJ" && ep_extend_chain;
        if i > 0
            && grapheme_break(
                &chars,
                &zwj_after_ep,
                i,
                indic_consonant_chain && indic_linker_seen,
                ri_run,
            )
        {
            out.push(chars[i].0);
        }
        let incb = crate::properties::indic_conjunct_break_property(c);
        match incb {
            "Consonant" => {
                indic_consonant_chain = true;
                indic_linker_seen = false;
            }
            "Linker" if indic_consonant_chain => indic_linker_seen = true,
            "Extend" if indic_consonant_chain => {}
            _ => {
                indic_consonant_chain = false;
                indic_linker_seen = false;
            }
        }
        if crate::properties::is_extended_pictographic(c) {
            ep_extend_chain = true;
        } else if g != "Extend" {
            ep_extend_chain = false;
        }
        if g == "Regional_Indicator" {
            ri_run += 1;
        } else {
            ri_run = 0;
        }
    }
    if !text.is_empty() {
        out.push(text.len());
    }
    BoundaryIndex::new(text, out)
}

fn grapheme_break(
    chars: &[(usize, char)],
    zwj_after_ep: &[bool],
    i: usize,
    gb9c: bool,
    ri_run: usize,
) -> bool {
    use crate::properties::{grapheme_cluster_break_property as p, is_extended_pictographic as ep};
    let a = p(chars[i - 1].1);
    let b = p(chars[i].1);
    if a == "CR" && b == "LF" {
        return false;
    }
    if matches!(a, "Control" | "CR" | "LF") || matches!(b, "Control" | "CR" | "LF") {
        return true;
    }
    if a == "L" && matches!(b, "L" | "V" | "LV" | "LVT") {
        return false;
    }
    if matches!(a, "LV" | "V") && matches!(b, "V" | "T") {
        return false;
    }
    if matches!(a, "LVT" | "T") && b == "T" {
        return false;
    }
    if matches!(b, "Extend" | "ZWJ" | "SpacingMark") || a == "Prepend" {
        return false;
    }
    if gb9c && crate::properties::indic_conjunct_break_property(chars[i].1) == "Consonant" {
        return false;
    }
    if a == "ZWJ" && zwj_after_ep[i - 1] && ep(chars[i].1) {
        return false;
    }
    if a == "Regional_Indicator" && b == "Regional_Indicator" {
        return ri_run % 2 == 0;
    }
    true
}

/// Compute default word boundaries, preserving ignored characters and their byte offsets.
pub fn word_boundaries(text: &str) -> BoundaryIndex<'_> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut out = vec![0];
    let props: Vec<&str> = chars
        .iter()
        .map(|&(_, c)| crate::properties::word_break_property(c))
        .collect();
    let ignored: Vec<bool> = props
        .iter()
        .map(|p| matches!(*p, "Extend" | "Format" | "ZWJ"))
        .collect();
    let mut prev_sig = vec![None; n];
    let mut last = None;
    for i in 0..n {
        prev_sig[i] = last;
        if !ignored[i] {
            last = Some(i);
        }
    }
    let mut next_sig = vec![None; n];
    last = None;
    for i in (0..n).rev() {
        next_sig[i] = last;
        if !ignored[i] {
            last = Some(i);
        }
    }
    let mut ri_run_before = vec![0usize; n];
    let mut ri_run = 0usize;
    for i in 0..n {
        ri_run_before[i] = ri_run;
        if !ignored[i] {
            ri_run = if props[i] == "Regional_Indicator" {
                ri_run + 1
            } else {
                0
            };
        }
    }
    for i in 1..n {
        if word_break(
            &chars,
            &props,
            &ignored,
            &prev_sig,
            &next_sig,
            &ri_run_before,
            i,
        ) {
            out.push(chars[i].0);
        }
    }
    if !text.is_empty() {
        out.push(text.len());
    }
    BoundaryIndex::new(text, out)
}

fn word_break(
    chars: &[(usize, char)],
    p: &[&str],
    ign: &[bool],
    prev: &[Option<usize>],
    next: &[Option<usize>],
    ri_run_before: &[usize],
    i: usize,
) -> bool {
    use crate::properties::is_extended_pictographic as ep;
    if p[i - 1] == "CR" && p[i] == "LF" {
        return false;
    }
    if p[i - 1] == "WSegSpace" && p[i] == "WSegSpace" {
        return false;
    }
    if matches!(p[i - 1], "CR" | "LF" | "Newline") || matches!(p[i], "CR" | "LF" | "Newline") {
        return true;
    }
    if i > 0 && p[i - 1] == "ZWJ" && ep(chars[i].1) {
        return false;
    }
    if ign[i] {
        return i > 0 && matches!(p[i - 1], "CR" | "LF" | "Newline");
    } // WB4
    let li = match prev[i] {
        Some(x) => x,
        None => return true,
    };
    let a = p[li];
    let b = p[i];
    let prev2 = prev[li].map(|j| p[j]);
    let next_i = next[i];
    let next_p = next_i.map(|j| p[j]);
    let ah = |x: &str| matches!(x, "ALetter" | "Hebrew_Letter");
    let mid_letter = |x: &str| matches!(x, "MidLetter" | "MidNumLet" | "Single_Quote");
    let mid_num = |x: &str| matches!(x, "MidNum" | "MidNumLet" | "Single_Quote");
    // WB6/7 and WB11/12 pair a middle punctuation with matching significant neighbors.
    if ah(a) && mid_letter(b) && next_p.is_some_and(ah) {
        return false;
    }
    if mid_letter(a) && ah(b) && prev2.is_some_and(ah) {
        return false;
    }
    if a == "Hebrew_Letter" && b == "Single_Quote" {
        return false;
    }
    if a == "Hebrew_Letter" && b == "Double_Quote" && next_p == Some("Hebrew_Letter") {
        return false;
    }
    if a == "Double_Quote" && b == "Hebrew_Letter" && prev2 == Some("Hebrew_Letter") {
        return false;
    }
    if a == "Numeric" && mid_num(b) && next_p == Some("Numeric") {
        return false;
    }
    if mid_num(a) && b == "Numeric" && prev2 == Some("Numeric") {
        return false;
    }
    if ah(a) && ah(b)
        || a == "Numeric" && b == "Numeric"
        || ah(a) && b == "Numeric"
        || a == "Numeric" && ah(b)
    {
        return false;
    }
    if a == "Katakana" && b == "Katakana" {
        return false;
    }
    let extend_left = |x: &str| ah(x) || matches!(x, "Numeric" | "Katakana" | "ExtendNumLet");
    if extend_left(a) && b == "ExtendNumLet"
        || a == "ExtendNumLet" && matches!(b, "ALetter" | "Hebrew_Letter" | "Numeric" | "Katakana")
    {
        return false;
    }
    if a == "Regional_Indicator" && b == "Regional_Indicator" {
        return ri_run_before[i] % 2 == 0;
    }
    true
}
