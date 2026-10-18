// SPDX-License-Identifier: AGPL-3.0-or-later
//! The paragraph node: UTF-8 text, character-attribute runs, paragraph
//! properties, anchored objects and in-text fields, bookmarks and links.
//!
//! All offsets are UTF-8 byte offsets into `text` and must fall on extended
//! grapheme cluster boundaries. The mutators below work on `&mut self` and may
//! leave it inconsistent when they fail; callers always mutate a clone and
//! commit it only after `validate_local` succeeds.

use super::NodeId;
use super::attrs::{CharAttrs, ParaProps};
use super::error::RichError;
use atelier_unicode::segmentation::{BoundaryIndex, grapheme_boundaries};
use std::collections::BTreeSet;

/// A maximal span of characters sharing the same non-empty attributes.
/// Characters outside every run carry no direct attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharRun {
    pub start: usize,
    pub end: usize,
    pub attrs: CharAttrs,
}

/// An object (picture, shape, text box...) anchored at a text offset.
/// `kind` and `data` are not interpreted: unknown objects are preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchoredObject {
    pub id: NodeId,
    pub offset: usize,
    pub kind: String,
    pub data: Vec<u8>,
}

/// Kind of an inline break.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakKind {
    Line,
    Page,
    Column,
}

/// A line, page or column break at a text offset. It is zero-width in `text`
/// and counts as one U+FFFC (3 bytes) in flat text (`rich::flat`).
///
/// US-203 freezes the field only; keeping `offset` in step with text edits,
/// the DOCX import and `validate_local` are R1's (US-204). Items of
/// `anchored`, `breaks` and `symbols` at one offset are ordered by `id`
/// across the three lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineBreak {
    pub id: NodeId,
    pub offset: usize,
    pub kind: BreakKind,
}

/// A symbol (font + code point, Word `w:sym`) at a text offset. Zero-width in
/// `text`, one U+FFFC (3 bytes) in flat text. Same status as [`InlineBreak`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineSymbol {
    pub id: NodeId,
    pub offset: usize,
    pub font: String,
    pub code: u32,
}

/// A field (page number, date, ...) whose displayed result is `start..end`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub id: NodeId,
    pub start: usize,
    pub end: usize,
    pub instruction: String,
}

/// A bookmark: a named range. Start and end form one record, so they are
/// paired by construction; `start <= end` is an invariant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bookmark {
    pub id: NodeId,
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Destination of a hyperlink.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkTarget {
    External(String),
    /// Name of a bookmark.
    Bookmark(String),
}

/// A hyperlink over a non-empty text range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Link {
    pub id: NodeId,
    pub start: usize,
    pub end: usize,
    pub target: LinkTarget,
    pub tooltip: Option<String>,
}

/// Any item attached to the text of a paragraph, addressed by its stable id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Inline {
    Anchored(AnchoredObject),
    Break(InlineBreak),
    Symbol(InlineSymbol),
    Field(Field),
    Bookmark(Bookmark),
    Link(Link),
}

impl Inline {
    pub fn id(&self) -> NodeId {
        match self {
            Self::Anchored(x) => x.id,
            Self::Break(x) => x.id,
            Self::Symbol(x) => x.id,
            Self::Field(x) => x.id,
            Self::Bookmark(x) => x.id,
            Self::Link(x) => x.id,
        }
    }
}

/// A paragraph.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Paragraph {
    pub text: String,
    /// Sorted, disjoint, non-empty, normalized (no touching equal neighbours).
    pub runs: Vec<CharRun>,
    pub props: ParaProps,
    /// Sorted by `(offset, id)`.
    pub anchored: Vec<AnchoredObject>,
    /// Sorted by `(offset, id)`.
    pub breaks: Vec<InlineBreak>,
    /// Sorted by `(offset, id)`.
    pub symbols: Vec<InlineSymbol>,
    /// Sorted by `(start, end, id)`, pairwise disjoint.
    pub fields: Vec<Field>,
    /// Sorted by `(start, end, id)`.
    pub bookmarks: Vec<Bookmark>,
    /// Sorted by `(start, end, id)`, pairwise disjoint.
    pub links: Vec<Link>,
}

fn check_offset(index: &BoundaryIndex<'_>, offset: usize, len: usize) -> Result<(), RichError> {
    if offset > len {
        return Err(RichError::OffsetOutOfBounds { offset, len });
    }
    match index.is_boundary(offset) {
        Ok(true) => Ok(()),
        _ => Err(RichError::NotGraphemeBoundary { offset }),
    }
}

fn normalize(runs: Vec<CharRun>) -> Vec<CharRun> {
    let mut out: Vec<CharRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.start >= run.end || run.attrs.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end == run.start && last.attrs == run.attrs => last.end = run.end,
            _ => out.push(run),
        }
    }
    out
}

impl Paragraph {
    /// A paragraph holding `text` with no attribute.
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            ..Self::default()
        }
    }

    /// Every stable id carried by the paragraph's inline items.
    pub fn inline_ids(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        ids.extend(self.anchored.iter().map(|x| x.id));
        ids.extend(self.breaks.iter().map(|x| x.id));
        ids.extend(self.symbols.iter().map(|x| x.id));
        ids.extend(self.fields.iter().map(|x| x.id));
        ids.extend(self.bookmarks.iter().map(|x| x.id));
        ids.extend(self.links.iter().map(|x| x.id));
        ids
    }

    /// Looks an inline item up by its id.
    pub fn inline(&self, id: NodeId) -> Option<Inline> {
        self.anchored
            .iter()
            .find(|x| x.id == id)
            .map(|x| Inline::Anchored(x.clone()))
            .or_else(|| {
                self.breaks
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| Inline::Break(x.clone()))
            })
            .or_else(|| {
                self.symbols
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| Inline::Symbol(x.clone()))
            })
            .or_else(|| {
                self.fields
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| Inline::Field(x.clone()))
            })
            .or_else(|| {
                self.bookmarks
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| Inline::Bookmark(x.clone()))
            })
            .or_else(|| {
                self.links
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| Inline::Link(x.clone()))
            })
    }

    /// Attributes defined directly on the character starting at `offset`.
    pub fn attrs_at(&self, offset: usize) -> CharAttrs {
        self.runs
            .iter()
            .find(|r| r.start <= offset && offset < r.end)
            .map(|r| r.attrs.clone())
            .unwrap_or_default()
    }

    fn sort_inline(&mut self) {
        self.anchored.sort_by_key(|x| (x.offset, x.id));
        self.breaks.sort_by_key(|x| (x.offset, x.id));
        self.symbols.sort_by_key(|x| (x.offset, x.id));
        self.fields.sort_by_key(|x| (x.start, x.end, x.id));
        self.bookmarks.sort_by_key(|x| (x.start, x.end, x.id));
        self.links.sort_by_key(|x| (x.start, x.end, x.id));
    }

    /// Checks every invariant of this paragraph taken alone.
    pub fn validate_local(&self) -> Result<(), RichError> {
        let len = self.text.len();
        let index = grapheme_boundaries(&self.text);
        self.props.validate()?;
        let mut prev: Option<&CharRun> = None;
        for run in &self.runs {
            if run.start >= run.end {
                return Err(RichError::Invariant("empty attribute run"));
            }
            check_offset(&index, run.start, len)?;
            check_offset(&index, run.end, len)?;
            if run.attrs.is_empty() {
                return Err(RichError::Invariant("run without attributes"));
            }
            run.attrs.validate()?;
            if let Some(p) = prev {
                if run.start < p.end {
                    return Err(RichError::Invariant("attribute runs overlap or unsorted"));
                }
                if run.start == p.end && run.attrs == p.attrs {
                    return Err(RichError::Invariant("adjacent equal attribute runs"));
                }
            }
            prev = Some(run);
        }
        let mut ids = BTreeSet::new();
        for pair in self.anchored.windows(2) {
            if (pair[0].offset, pair[0].id) >= (pair[1].offset, pair[1].id) {
                return Err(RichError::Invariant("anchored objects unsorted"));
            }
        }
        for x in &self.anchored {
            check_offset(&index, x.offset, len)?;
        }
        for x in &self.fields {
            if x.start > x.end {
                return Err(RichError::InvalidRange {
                    start: x.start,
                    end: x.end,
                });
            }
            check_offset(&index, x.start, len)?;
            check_offset(&index, x.end, len)?;
        }
        for pair in self.fields.windows(2) {
            if pair[1].start < pair[0].end
                || (pair[0].start, pair[0].end, pair[0].id)
                    >= (pair[1].start, pair[1].end, pair[1].id)
            {
                return Err(RichError::Invariant("fields overlap or unsorted"));
            }
        }
        for x in &self.links {
            if x.start >= x.end {
                return Err(RichError::InvalidRange {
                    start: x.start,
                    end: x.end,
                });
            }
            check_offset(&index, x.start, len)?;
            check_offset(&index, x.end, len)?;
        }
        for pair in self.links.windows(2) {
            if pair[1].start < pair[0].end
                || (pair[0].start, pair[0].end, pair[0].id)
                    >= (pair[1].start, pair[1].end, pair[1].id)
            {
                return Err(RichError::Invariant("links overlap or unsorted"));
            }
        }
        let mut names = BTreeSet::new();
        for x in &self.bookmarks {
            if x.name.is_empty() {
                return Err(RichError::Invariant("bookmark without a name"));
            }
            if !names.insert(x.name.as_str()) {
                return Err(RichError::DuplicateBookmarkName(x.name.clone()));
            }
            if x.start > x.end {
                return Err(RichError::InvalidRange {
                    start: x.start,
                    end: x.end,
                });
            }
            check_offset(&index, x.start, len)?;
            check_offset(&index, x.end, len)?;
        }
        for pair in self.bookmarks.windows(2) {
            if (pair[0].start, pair[0].end, pair[0].id) >= (pair[1].start, pair[1].end, pair[1].id)
            {
                return Err(RichError::Invariant("bookmarks unsorted"));
            }
        }
        for id in self.inline_ids() {
            if !ids.insert(id) {
                return Err(RichError::DuplicateId(id));
            }
        }
        Ok(())
    }

    /// Inserts `text` at `offset`. The new text inherits the attributes of the
    /// character before it; markers located at or after `offset` move right.
    pub fn insert_text(&mut self, offset: usize, text: &str) -> Result<(), RichError> {
        let len = self.text.len();
        check_offset(&grapheme_boundaries(&self.text), offset, len)?;
        if text.is_empty() {
            return Ok(());
        }
        let n = text.len();
        for run in &mut self.runs {
            if run.start >= offset {
                run.start += n;
                run.end += n;
            } else if run.end >= offset {
                run.end += n;
            }
        }
        for x in &mut self.anchored {
            if x.offset >= offset {
                x.offset += n;
            }
        }
        fn shift(start: &mut usize, end: &mut usize, offset: usize, n: usize) {
            let moved = *start >= offset;
            if moved {
                *start += n;
            }
            if *end > offset || (*end == offset && moved) {
                *end += n;
            }
        }
        for x in &mut self.fields {
            shift(&mut x.start, &mut x.end, offset, n);
        }
        for x in &mut self.links {
            shift(&mut x.start, &mut x.end, offset, n);
        }
        for x in &mut self.bookmarks {
            shift(&mut x.start, &mut x.end, offset, n);
        }
        self.text.insert_str(offset, text);
        let index = grapheme_boundaries(&self.text);
        check_offset(&index, offset, self.text.len())?;
        check_offset(&index, offset + n, self.text.len())
    }

    /// Deletes `start..end`. Markers inside collapse to `start`; anchored
    /// objects inside are removed; links and fields left empty are removed.
    pub fn delete_text(&mut self, start: usize, end: usize) -> Result<(), RichError> {
        let len = self.text.len();
        if start > end {
            return Err(RichError::InvalidRange { start, end });
        }
        let index = grapheme_boundaries(&self.text);
        check_offset(&index, start, len)?;
        check_offset(&index, end, len)?;
        if start == end {
            return Ok(());
        }
        let n = end - start;
        let map = |p: usize| {
            if p <= start {
                p
            } else if p >= end {
                p - n
            } else {
                start
            }
        };
        for run in &mut self.runs {
            run.start = map(run.start);
            run.end = map(run.end);
        }
        self.anchored
            .retain(|x| !(start <= x.offset && x.offset < end));
        for x in &mut self.anchored {
            x.offset = map(x.offset);
        }
        self.fields.retain_mut(|x| {
            let was_empty = x.start == x.end;
            x.start = map(x.start);
            x.end = map(x.end);
            was_empty || x.start != x.end
        });
        self.links.retain_mut(|x| {
            x.start = map(x.start);
            x.end = map(x.end);
            x.start != x.end
        });
        for x in &mut self.bookmarks {
            x.start = map(x.start);
            x.end = map(x.end);
        }
        self.text.replace_range(start..end, "");
        self.runs = normalize(std::mem::take(&mut self.runs));
        self.sort_inline();
        Ok(())
    }

    /// Defines (`set`) then un-defines (`clear`) character properties on a
    /// range, then normalizes the runs. An empty range is a no-op.
    pub fn set_char_attrs(
        &mut self,
        start: usize,
        end: usize,
        set: &CharAttrs,
        clear: &CharAttrs,
    ) -> Result<(), RichError> {
        let len = self.text.len();
        if start > end {
            return Err(RichError::InvalidRange { start, end });
        }
        let index = grapheme_boundaries(&self.text);
        check_offset(&index, start, len)?;
        check_offset(&index, end, len)?;
        set.validate()?;
        if start == end {
            return Ok(());
        }
        let mut segments: Vec<CharRun> = Vec::new();
        let mut pos = 0;
        for run in &self.runs {
            if run.start > pos {
                segments.push(CharRun {
                    start: pos,
                    end: run.start,
                    attrs: CharAttrs::default(),
                });
            }
            segments.push(run.clone());
            pos = run.end;
        }
        if pos < len {
            segments.push(CharRun {
                start: pos,
                end: len,
                attrs: CharAttrs::default(),
            });
        }
        let mut out = Vec::with_capacity(segments.len() + 2);
        for seg in segments {
            let piece = |s: usize, e: usize, attrs: CharAttrs| CharRun {
                start: s,
                end: e,
                attrs,
            };
            if seg.start < start {
                out.push(piece(seg.start, seg.end.min(start), seg.attrs.clone()));
            }
            let (ms, me) = (seg.start.max(start), seg.end.min(end));
            if ms < me {
                let mut attrs = seg.attrs.clone();
                attrs.clear(clear);
                attrs.overlay(set);
                out.push(piece(ms, me, attrs));
            }
            if seg.end > end {
                out.push(piece(seg.start.max(end), seg.end, seg.attrs.clone()));
            }
        }
        self.runs = normalize(out);
        Ok(())
    }

    /// Cuts the paragraph at `offset`; `self` keeps the head and the tail is
    /// returned with the same paragraph properties. Items entirely after the
    /// cut move to the tail; links and bookmarks straddling it are truncated to
    /// the head. A split inside a field is refused.
    pub fn split_off(&mut self, offset: usize) -> Result<Paragraph, RichError> {
        let len = self.text.len();
        check_offset(&grapheme_boundaries(&self.text), offset, len)?;
        if self
            .fields
            .iter()
            .any(|f| f.start < offset && offset < f.end)
        {
            return Err(RichError::SplitInsideField);
        }
        let mut tail = Paragraph {
            text: self.text.split_off(offset),
            props: self.props.clone(),
            ..Paragraph::default()
        };
        let mut head_runs = Vec::new();
        for run in std::mem::take(&mut self.runs) {
            if run.end <= offset {
                head_runs.push(run);
            } else if run.start >= offset {
                tail.runs.push(CharRun {
                    start: run.start - offset,
                    end: run.end - offset,
                    attrs: run.attrs,
                });
            } else {
                tail.runs.push(CharRun {
                    start: 0,
                    end: run.end - offset,
                    attrs: run.attrs.clone(),
                });
                head_runs.push(CharRun { end: offset, ..run });
            }
        }
        self.runs = head_runs;
        for x in std::mem::take(&mut self.anchored) {
            if x.offset < offset {
                self.anchored.push(x);
            } else {
                tail.anchored.push(AnchoredObject {
                    offset: x.offset - offset,
                    ..x
                });
            }
        }
        for x in std::mem::take(&mut self.fields) {
            if x.start >= offset {
                tail.fields.push(Field {
                    start: x.start - offset,
                    end: x.end - offset,
                    ..x
                });
            } else {
                self.fields.push(x);
            }
        }
        for x in std::mem::take(&mut self.links) {
            if x.start >= offset {
                tail.links.push(Link {
                    start: x.start - offset,
                    end: x.end - offset,
                    ..x
                });
            } else {
                self.links.push(Link {
                    end: x.end.min(offset),
                    ..x
                });
            }
        }
        for x in std::mem::take(&mut self.bookmarks) {
            if x.start >= offset {
                tail.bookmarks.push(Bookmark {
                    start: x.start - offset,
                    end: x.end - offset,
                    ..x
                });
            } else {
                self.bookmarks.push(Bookmark {
                    end: x.end.min(offset),
                    ..x
                });
            }
        }
        Ok(tail)
    }

    /// Appends `other` after the text of `self`; `self` keeps its properties.
    pub fn append(&mut self, other: Paragraph) {
        let base = self.text.len();
        self.text.push_str(&other.text);
        self.runs.extend(other.runs.into_iter().map(|r| CharRun {
            start: r.start + base,
            end: r.end + base,
            attrs: r.attrs,
        }));
        self.runs = normalize(std::mem::take(&mut self.runs));
        self.anchored
            .extend(other.anchored.into_iter().map(|x| AnchoredObject {
                offset: x.offset + base,
                ..x
            }));
        self.fields.extend(other.fields.into_iter().map(|x| Field {
            start: x.start + base,
            end: x.end + base,
            ..x
        }));
        self.links.extend(other.links.into_iter().map(|x| Link {
            start: x.start + base,
            end: x.end + base,
            ..x
        }));
        self.bookmarks
            .extend(other.bookmarks.into_iter().map(|x| Bookmark {
                start: x.start + base,
                end: x.end + base,
                ..x
            }));
        self.sort_inline();
    }

    /// Adds an inline item (its id must be unused in the paragraph).
    pub fn insert_inline(&mut self, item: Inline) -> Result<(), RichError> {
        if self.inline(item.id()).is_some() {
            return Err(RichError::DuplicateId(item.id()));
        }
        match item {
            Inline::Anchored(x) => self.anchored.push(x),
            Inline::Break(x) => self.breaks.push(x),
            Inline::Symbol(x) => self.symbols.push(x),
            Inline::Field(x) => self.fields.push(x),
            Inline::Bookmark(x) => self.bookmarks.push(x),
            Inline::Link(x) => self.links.push(x),
        }
        self.sort_inline();
        Ok(())
    }

    /// Removes an inline item and returns it.
    pub fn remove_inline(&mut self, id: NodeId) -> Result<Inline, RichError> {
        let old = self.inline(id).ok_or(RichError::NodeNotFound(id))?;
        self.anchored.retain(|x| x.id != id);
        self.breaks.retain(|x| x.id != id);
        self.symbols.retain(|x| x.id != id);
        self.fields.retain(|x| x.id != id);
        self.bookmarks.retain(|x| x.id != id);
        self.links.retain(|x| x.id != id);
        Ok(old)
    }

    /// Replaces the inline item having the same id and the same kind.
    pub fn replace_inline(&mut self, item: Inline) -> Result<Inline, RichError> {
        let old = self
            .inline(item.id())
            .ok_or(RichError::NodeNotFound(item.id()))?;
        if core::mem::discriminant(&old) != core::mem::discriminant(&item) {
            return Err(RichError::WrongNodeKind {
                id: item.id(),
                expected: "inline item of the same kind",
            });
        }
        self.remove_inline(item.id())?;
        self.insert_inline(item)?;
        Ok(old)
    }
}
