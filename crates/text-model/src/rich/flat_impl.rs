// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`FlatIndex`] and [`LayoutContext`] on [`RichDocument`] (US-204).
//!
//! The prefix-sum index ([`FlatTree`]) is built on first use from the block
//! tree (paragraphs of every section body, table cells included, opaque
//! blocks as one U+FFFC paragraph; headers and footers are not part of it)
//! and kept in step by the flat edits. Every query is `O(log n)` plus the
//! length of the paragraphs it reads: cluster and word boundaries are those
//! of `crates/unicode` computed on the flat text of one paragraph, cut at
//! every U+FFFC, with a boundary before and after every `\n` separator.

use std::borrow::Cow;
use std::sync::PoisonError;

use atelier_unicode::segmentation::{grapheme_boundaries, word_boundaries};

use super::attrs::CharAttrs;
use super::document::RichDocument;
use super::flat::{
    BlockKind, FlatError, FlatIndex, LayoutContext, ListCounters, OBJECT_LEN, PageSetup,
    ParagraphPosition, paragraph_flat,
};
use super::flat_tree::{CellContext, Entry, FlatTree, ListState};
use super::node::{BlockList, Node};
use super::{ListRef, NodeId, Paragraph, ResolvedProperties, Section, StyleCatalog, Theme};

/// Flat length of a paragraph: its text plus 3 bytes per non-text item.
pub(crate) fn flat_len(p: &Paragraph) -> usize {
    p.text.len() + OBJECT_LEN * (p.anchored.len() + p.breaks.len() + p.symbols.len())
}

/// Distance between two list-counter checkpoints, in paragraphs.
const CHECKPOINT_EVERY: usize = 256;

impl RichDocument {
    /// Builds the index from the block tree.
    pub(crate) fn build_flat(&self) -> FlatTree {
        let mut out = Vec::new();
        for section in self.sections.to_vec() {
            if let Some(Node::Section(s)) = self.nodes.get(&section) {
                self.collect_flat(&s.body, section, section, None, &mut out);
            }
        }
        FlatTree::from_entries(out)
    }

    fn collect_flat(
        &self,
        list: &BlockList,
        parent: NodeId,
        section: NodeId,
        cell: Option<CellContext>,
        out: &mut Vec<Entry>,
    ) {
        for index in 0..list.len() {
            let Some(id) = list.get(index) else { continue };
            match self.nodes.get(&id) {
                Some(Node::Paragraph(p)) => out.push(Entry {
                    id,
                    opaque: false,
                    cell,
                    parent,
                    section,
                    len: flat_len(p),
                }),
                Some(Node::Opaque(_)) => out.push(Entry {
                    id,
                    opaque: true,
                    cell,
                    parent,
                    section,
                    len: OBJECT_LEN,
                }),
                Some(Node::Table(table)) => {
                    for row_id in &table.rows {
                        let Some(Node::Row(row)) = self.nodes.get(row_id) else {
                            continue;
                        };
                        for cell_id in &row.cells {
                            let Some(Node::Cell(c)) = self.nodes.get(cell_id) else {
                                continue;
                            };
                            let context = CellContext {
                                table: id,
                                row: *row_id,
                                cell: *cell_id,
                            };
                            self.collect_flat(&c.blocks, *cell_id, section, Some(context), out);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Runs `f` on the index, building it first when needed. `f` must not
    /// call back into a method that takes the index.
    pub(crate) fn with_flat<R>(&self, f: impl FnOnce(&mut FlatTree) -> R) -> R {
        let mut guard = self.flat.lock().unwrap_or_else(PoisonError::into_inner);
        let tree = guard.get_or_insert_with(|| self.build_flat());
        f(tree)
    }

    /// Takes the index out (building it when absent) so that a flat edit can
    /// mutate the document without the primitives dropping it.
    pub(crate) fn take_flat(&mut self) -> FlatTree {
        let taken = self
            .flat
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match taken {
            Some(tree) => tree,
            None => self.build_flat(),
        }
    }

    pub(crate) fn put_flat(&mut self, tree: FlatTree) {
        *self.flat.get_mut().unwrap_or_else(PoisonError::into_inner) = Some(tree);
    }

    /// Flat text of the paragraph an entry stands for.
    pub(crate) fn flat_str(&self, entry: &Entry) -> Option<Cow<'_, str>> {
        if entry.opaque {
            return Some(Cow::Borrowed("\u{FFFC}"));
        }
        match self.nodes.get(&entry.id) {
            Some(Node::Paragraph(p)) => Some(paragraph_flat(p)),
            _ => None,
        }
    }

    fn paragraph_of(&self, tree: &FlatTree, index: usize) -> Option<&Paragraph> {
        let entry = tree.entry(index)?;
        if entry.opaque {
            return None;
        }
        match self.nodes.get(&entry.id) {
            Some(Node::Paragraph(p)) => Some(p),
            _ => None,
        }
    }

    /// List the paragraph belongs to: its direct reference, else the one of
    /// its style (or a style it is based on), else the document default.
    fn effective_list(&self, p: &Paragraph) -> Option<ListRef> {
        if p.props.list.is_some() {
            return p.props.list;
        }
        let catalog = self.styles.catalog();
        let mut next = p.props.style.as_deref();
        for _ in 0..64 {
            let style = catalog.style(next?)?;
            if style.paragraph.list.is_some() {
                return style.paragraph.list;
            }
            next = style.based_on.as_deref();
        }
        catalog.defaults().paragraph.list
    }
}

/// Boundaries of a paragraph's flat text, as offsets in it: cluster or word
/// boundaries of each piece between two U+FFFC, plus both sides of every
/// U+FFFC, plus 0 and the length.
fn local_bounds(text: &str, words: bool) -> Vec<usize> {
    fn piece(text: &str, start: usize, end: usize, words: bool, out: &mut Vec<usize>) {
        if end > start {
            let part = &text[start..end];
            let index = if words {
                word_boundaries(part)
            } else {
                grapheme_boundaries(part)
            };
            out.extend(index.boundaries().iter().map(|b| start + b));
        }
    }
    let mut out = vec![0usize];
    let mut piece_start = 0;
    for (at, c) in text.char_indices() {
        if c == '\u{FFFC}' {
            piece(text, piece_start, at, words, &mut out);
            out.push(at);
            out.push(at + OBJECT_LEN);
            piece_start = at + OBJECT_LEN;
        }
    }
    piece(text, piece_start, text.len(), words, &mut out);
    out.push(text.len());
    out.sort_unstable();
    out.dedup();
    out
}

fn is_blank(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c == ' ' || c == '\t')
}

/// Boundary navigation over the flat text, caching the segmentation of the
/// paragraph read last. Boundaries: those of each paragraph, plus the
/// separator after each paragraph (a `\n` is a cluster and a word of its own).
struct Nav<'a> {
    doc: &'a RichDocument,
    tree: &'a FlatTree,
    words: bool,
    index: usize,
    text: Cow<'a, str>,
    bounds: Vec<usize>,
    loaded: bool,
}

impl<'a> Nav<'a> {
    fn new(doc: &'a RichDocument, tree: &'a FlatTree, words: bool) -> Self {
        Self {
            doc,
            tree,
            words,
            index: 0,
            text: Cow::Borrowed(""),
            bounds: Vec::new(),
            loaded: false,
        }
    }

    fn load(&mut self, index: usize) -> Option<()> {
        if !(self.loaded && self.index == index) {
            let entry = self.tree.entry(index)?;
            self.text = self.doc.flat_str(entry)?;
            self.bounds = local_bounds(&self.text, self.words);
            self.index = index;
            self.loaded = true;
        }
        Some(())
    }

    /// Largest boundary strictly before `position`.
    fn prev(&mut self, position: usize) -> Option<usize> {
        let (i, o) = self.tree.find(position)?;
        let start = self.tree.start(i)?;
        if o > 0 {
            self.load(i)?;
            let p = self.bounds.partition_point(|b| *b < o);
            self.bounds.get(p.checked_sub(1)?).map(|b| start + b)
        } else if i == 0 {
            None
        } else {
            Some(start - 1)
        }
    }

    /// Smallest boundary strictly after `position`.
    fn next(&mut self, position: usize) -> Option<usize> {
        let (i, o) = self.tree.find(position)?;
        let start = self.tree.start(i)?;
        let len = self.tree.entry(i)?.len;
        if o < len {
            self.load(i)?;
            let p = self.bounds.partition_point(|b| *b <= o);
            self.bounds.get(p).map(|b| start + b)
        } else if i + 1 < self.tree.count() {
            Some(start + len + 1)
        } else {
            None
        }
    }

    /// Whether the segment `a..b` (between two consecutive boundaries) is
    /// made of spaces and tabs only.
    fn blank(&mut self, a: usize, b: usize) -> bool {
        let Some((i, o)) = self.tree.find(a) else {
            return false;
        };
        let Some(start) = self.tree.start(i) else {
            return false;
        };
        let len = self.tree.entry(i).map_or(0, |e| e.len);
        if o >= len || self.load(i).is_none() {
            return false;
        }
        self.text.get(o..b - start).is_some_and(is_blank)
    }
}

impl FlatIndex for RichDocument {
    fn len(&self) -> usize {
        self.with_flat(|t| t.total())
    }

    fn paragraph_count(&self) -> usize {
        self.with_flat(|t| t.count())
    }

    fn locate(&self, position: usize) -> Result<ParagraphPosition, FlatError> {
        self.with_flat(|t| t.find(position))
            .map(|(paragraph, offset)| ParagraphPosition { paragraph, offset })
            .ok_or(FlatError::OutOfRange)
    }

    fn position(&self, at: ParagraphPosition) -> Result<usize, FlatError> {
        self.with_flat(|t| {
            let start = t.start(at.paragraph)?;
            let entry = t.entry(at.paragraph)?;
            (at.offset <= entry.len).then_some(start + at.offset)
        })
        .ok_or(FlatError::OutOfRange)
    }

    fn check_position(&self, position: usize) -> Result<(), FlatError> {
        self.with_flat(|t| {
            let (i, o) = t.find(position).ok_or(FlatError::OutOfRange)?;
            let entry = t.entry(i).ok_or(FlatError::OutOfRange)?;
            if o == 0 || o == entry.len {
                return Ok(());
            }
            let text = self.flat_str(entry).ok_or(FlatError::OutOfRange)?;
            if local_bounds(&text, false).binary_search(&o).is_ok() {
                Ok(())
            } else {
                Err(FlatError::NotBoundary)
            }
        })
    }

    fn text(&self, start: usize, end: usize) -> Result<String, FlatError> {
        if start > end {
            return Err(FlatError::InvalidRange);
        }
        self.with_flat(|t| {
            let (first, first_offset) = t.find(start).ok_or(FlatError::OutOfRange)?;
            let (last, last_offset) = t.find(end).ok_or(FlatError::OutOfRange)?;
            let mut out = String::with_capacity(end - start);
            for i in first..=last {
                let entry = t.entry(i).ok_or(FlatError::OutOfRange)?;
                let text = self.flat_str(entry).ok_or(FlatError::OutOfRange)?;
                let from = if i == first { first_offset } else { 0 };
                let to = if i == last { last_offset } else { entry.len };
                out.push_str(text.get(from..to).ok_or(FlatError::NotBoundary)?);
                if i < last {
                    out.push('\n');
                }
            }
            Ok(out)
        })
    }

    fn snap_back(&self, position: usize) -> Result<usize, FlatError> {
        self.with_flat(|t| {
            let (i, o) = t.find(position).ok_or(FlatError::OutOfRange)?;
            let entry = t.entry(i).ok_or(FlatError::OutOfRange)?;
            let start = t.start(i).ok_or(FlatError::OutOfRange)?;
            if o == 0 || o == entry.len {
                return Ok(position);
            }
            let text = self.flat_str(entry).ok_or(FlatError::OutOfRange)?;
            let bounds = local_bounds(&text, false);
            let p = bounds.partition_point(|b| *b <= o);
            Ok(start + bounds.get(p.saturating_sub(1)).copied().unwrap_or(0))
        })
    }

    fn prev_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.with_flat(|t| {
            if position > t.total() {
                return Err(FlatError::OutOfRange);
            }
            Ok(Nav::new(self, t, false).prev(position))
        })
    }

    fn next_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.with_flat(|t| {
            if position > t.total() {
                return Err(FlatError::OutOfRange);
            }
            Ok(Nav::new(self, t, false).next(position))
        })
    }

    fn prev_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.with_flat(|t| {
            if position > t.total() {
                return Err(FlatError::OutOfRange);
            }
            let mut nav = Nav::new(self, t, true);
            let Some(mut at) = nav.prev(position) else {
                return Ok(None);
            };
            while at > 0 {
                let end = nav.next(at).unwrap_or(t.total());
                if !nav.blank(at, end) {
                    break;
                }
                at = nav.prev(at).unwrap_or(0);
            }
            Ok(Some(at))
        })
    }

    fn next_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.with_flat(|t| {
            let total = t.total();
            if position > total {
                return Err(FlatError::OutOfRange);
            }
            if position == total {
                return Ok(None);
            }
            let mut nav = Nav::new(self, t, true);
            let mut at = nav.next(position).unwrap_or(total);
            while at < total {
                let end = nav.next(at).unwrap_or(total);
                if !nav.blank(at, end) {
                    break;
                }
                at = end;
            }
            Ok(Some(at))
        })
    }

    fn word_at(&self, position: usize) -> Result<Option<(usize, usize)>, FlatError> {
        self.with_flat(|t| {
            let total = t.total();
            if position > total {
                return Err(FlatError::OutOfRange);
            }
            if total == 0 {
                return Ok(None);
            }
            let anchor = position.min(total - 1);
            let (i, o) = t.find(anchor).ok_or(FlatError::OutOfRange)?;
            let start = t.start(i).ok_or(FlatError::OutOfRange)?;
            let entry = t.entry(i).ok_or(FlatError::OutOfRange)?;
            if o >= entry.len {
                return Ok(Some((start + entry.len, start + entry.len + 1)));
            }
            let text = self.flat_str(entry).ok_or(FlatError::OutOfRange)?;
            let bounds = local_bounds(&text, true);
            let p = bounds.partition_point(|b| *b <= o);
            let a = bounds.get(p.saturating_sub(1)).copied().unwrap_or(0);
            let b = bounds.get(p).copied().unwrap_or(entry.len);
            Ok(Some((start + a, start + b)))
        })
    }
}

impl LayoutContext for RichDocument {
    fn catalog(&self) -> &StyleCatalog {
        self.styles.catalog()
    }

    fn theme(&self) -> &Theme {
        &self.theme
    }

    fn paragraph(&self, index: usize) -> Option<&Paragraph> {
        self.with_flat(|t| self.paragraph_of(t, index))
    }

    fn block_kind(&self, index: usize) -> Result<BlockKind, FlatError> {
        self.with_flat(|t| t.entry(index).map(Entry::kind))
            .ok_or(FlatError::OutOfRange)
    }

    fn section_of(&self, index: usize) -> Result<(usize, &Section), FlatError> {
        let section = self
            .with_flat(|t| t.entry(index).map(|e| e.section))
            .ok_or(FlatError::OutOfRange)?;
        let position = self
            .sections
            .position(section)
            .ok_or(FlatError::OutOfRange)?;
        match self.nodes.get(&section) {
            Some(Node::Section(s)) => Ok((position, s)),
            _ => Err(FlatError::OutOfRange),
        }
    }

    fn resolved(&self, index: usize) -> Result<ResolvedProperties, FlatError> {
        self.resolve_at(index, None).map(|(resolved, _)| resolved)
    }

    fn resolved_run(&self, index: usize, direct: &CharAttrs) -> Result<CharAttrs, FlatError> {
        self.resolve_at(index, Some(direct))
            .map(|(resolved, _)| resolved.character)
    }

    fn page_setup(&self, section: usize) -> Result<PageSetup, FlatError> {
        let id = self.sections.get(section).ok_or(FlatError::OutOfRange)?;
        match self.nodes.get(&id) {
            Some(Node::Section(s)) => Ok(PageSetup::from_section(s)),
            _ => Err(FlatError::OutOfRange),
        }
    }

    fn object_resource(&self, object: NodeId) -> Option<&[u8]> {
        let owner = self.inline_index.get(&object)?;
        match self.nodes.get(owner) {
            Some(Node::Paragraph(p)) => p
                .anchored
                .iter()
                .find(|a| a.id == object)
                .map(|a| a.data.as_slice()),
            _ => None,
        }
    }

    fn list_counters(&self, index: usize) -> Result<Option<ListCounters>, FlatError> {
        self.with_flat(|t| {
            if index >= t.count() {
                return Err(FlatError::OutOfRange);
            }
            let slot = t.lists.partition_point(|(k, _)| *k <= index);
            let (mut at, mut state) = match slot.checked_sub(1).and_then(|s| t.lists.get(s)) {
                Some((k, state)) => (*k, state.clone()),
                None => (0, ListState::new()),
            };
            while at < index {
                if at % CHECKPOINT_EVERY == 0 && t.lists.last().is_none_or(|(k, _)| at > *k) {
                    t.lists.push((at, state.clone()));
                }
                if let Some(list) = self
                    .paragraph_of(t, at)
                    .and_then(|p| self.effective_list(p))
                {
                    let counters = state
                        .entry(list.list_id)
                        .or_insert([0; super::flat::LIST_LEVELS]);
                    let level = usize::from(list.level);
                    if let Some(c) = counters.get_mut(level) {
                        *c = c.saturating_add(1);
                    }
                    for deeper in counters.iter_mut().skip(level + 1) {
                        *deeper = 0;
                    }
                }
                at += 1;
            }
            let Some(list) = self
                .paragraph_of(t, index)
                .and_then(|p| self.effective_list(p))
            else {
                return Ok(None);
            };
            Ok(Some(ListCounters {
                list_id: list.list_id,
                level: list.level,
                before: state
                    .get(&list.list_id)
                    .copied()
                    .unwrap_or([0; super::flat::LIST_LEVELS]),
            }))
        })
    }

    fn paragraph_start(&self, index: usize) -> Result<usize, FlatError> {
        self.with_flat(|t| t.start(index))
            .ok_or(FlatError::OutOfRange)
    }
}

impl RichDocument {
    /// Cascade for the paragraph `index`, with `direct` run attributes when
    /// given. The table style is the one of the innermost table holding the
    /// paragraph, else the catalog default for cell paragraphs.
    fn resolve_at(
        &self,
        index: usize,
        direct: Option<&CharAttrs>,
    ) -> Result<(ResolvedProperties, bool), FlatError> {
        let entry = self
            .with_flat(|t| t.entry(index).copied())
            .ok_or(FlatError::OutOfRange)?;
        if entry.opaque && direct.is_none() {
            return Ok((ResolvedProperties::default(), false));
        }
        let paragraph = match self.nodes.get(&entry.id) {
            Some(Node::Paragraph(p)) if !entry.opaque => Some(p),
            _ => None,
        };
        let table_style = entry.cell.and_then(|c| {
            let own = match self.nodes.get(&c.table) {
                Some(Node::Table(table)) => table.style.as_deref(),
                _ => None,
            };
            own.or(self.styles.catalog().defaults().table_style.as_deref())
        });
        let none = CharAttrs::default();
        let direct_attrs = direct.unwrap_or(&none);
        let empty = Default::default();
        let props = paragraph.map_or(&empty, |p| &p.props);
        self.styles
            .resolve_detached(
                props.style.as_deref(),
                direct_attrs.char_style.as_deref(),
                table_style,
                props,
                direct_attrs,
            )
            .map(|r| (r, paragraph.is_some()))
            .map_err(|_| FlatError::Style)
    }
}
