// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test double of the flat contract (US-203): [`VecParagraphs`] implements
//! [`FlatIndex`] and [`LayoutContext`] over a plain vector of paragraphs so
//! that the layout (R2) can be tested without waiting for the model (R1).
//! It is simple and linear, not an implementation to ship: R1 implements the
//! same traits on `RichDocument` with a prefix-sum index.

use std::borrow::Cow;

use atelier_unicode::segmentation::{grapheme_boundaries, word_boundaries};

use super::flat::{
    BlockKind, EditOutcome, FlatError, FlatIndex, LayoutContext, LayoutScope, ListCounters,
    OBJECT_LEN, ParagraphPosition, SEPARATOR_LEN, paragraph_flat,
};
use super::{NodeId, Paragraph, Section, StyleCatalog, Theme};

/// Paragraphs in document order; `None` is an opaque block.
#[derive(Debug, Clone, Default)]
pub struct VecParagraphs {
    entries: Vec<Option<Paragraph>>,
    kinds: Vec<BlockKind>,
    starts: Vec<usize>,
    total: usize,
    pub catalog: StyleCatalog,
    pub theme: Theme,
    pub section: Section,
    revision: u64,
}

impl VecParagraphs {
    /// Body paragraphs, one section.
    pub fn new(paragraphs: Vec<Paragraph>) -> Self {
        let mut this = Self {
            kinds: vec![BlockKind::Body; paragraphs.len()],
            entries: paragraphs.into_iter().map(Some).collect(),
            ..Self::default()
        };
        this.reindex();
        this
    }

    /// Overrides the kind of one block (`Opaque` also empties the entry).
    pub fn set_kind(&mut self, index: usize, kind: BlockKind) {
        if let Some(slot) = self.kinds.get_mut(index) {
            *slot = kind;
            if kind == BlockKind::Opaque {
                self.entries[index] = None;
            }
            self.reindex();
        }
    }

    /// Replaces paragraphs `first..first + removed` by `new` and returns the
    /// outcome of that edit: the bytes of the replaced paragraphs (joined by
    /// newline) became the bytes of the new ones. Meant for `removed >= 1` and
    /// at least one new paragraph (no pure insertion or deletion of whole
    /// paragraphs); the selection goes to the end of the new text.
    pub fn replace_paragraphs(
        &mut self,
        first: usize,
        removed: usize,
        new: Vec<Paragraph>,
    ) -> EditOutcome {
        let first = first.min(self.entries.len());
        let removed = removed.min(self.entries.len() - first);
        let joined = |this: &Self, a: usize, n: usize| -> usize {
            (a..a + n)
                .map(|i| this.paragraph_flat(i).map_or(0, |t| t.len()))
                .sum::<usize>()
                + n.saturating_sub(1)
        };
        let start = self.starts.get(first).copied().unwrap_or(self.total);
        let removed_bytes = joined(self, first, removed);
        let inserted = new.len();
        self.kinds
            .splice(first..first + removed, vec![BlockKind::Body; inserted]);
        self.entries
            .splice(first..first + removed, new.into_iter().map(Some));
        self.reindex();
        self.revision += 1;
        let inserted_bytes = joined(self, first, inserted);
        EditOutcome {
            revision: self.revision,
            start,
            removed_bytes,
            inserted_bytes,
            doc_len: self.total,
            first_paragraph: first,
            removed_paragraphs: removed,
            inserted_paragraphs: inserted,
            layout_scope: LayoutScope::Paragraphs,
            selection: (start + inserted_bytes, start + inserted_bytes),
            can_undo: true,
            can_redo: false,
        }
    }

    fn reindex(&mut self) {
        self.starts.clear();
        let mut at = 0;
        for entry in &self.entries {
            self.starts.push(at);
            at += entry
                .as_ref()
                .map_or(OBJECT_LEN, |p| paragraph_flat(p).len())
                + SEPARATOR_LEN;
        }
        self.total = at.saturating_sub(SEPARATOR_LEN);
    }

    fn flat_text(&self) -> String {
        let mut out = String::with_capacity(self.total);
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            match entry {
                Some(p) => out.push_str(&paragraph_flat(p)),
                None => out.push('\u{FFFC}'),
            }
        }
        out
    }

    /// Cluster boundaries and word boundaries of the whole flat text, with a
    /// cut before and after every U+FFFC and `\n`.
    fn boundaries(&self) -> (Vec<usize>, Vec<usize>) {
        let text = self.flat_text();
        let (mut clusters, mut words) = (vec![0], vec![0]);
        let mut piece_start = 0;
        let flush =
            |start: usize, end: usize, clusters: &mut Vec<usize>, words: &mut Vec<usize>| {
                if end > start {
                    let piece = &text[start..end];
                    clusters.extend(
                        grapheme_boundaries(piece)
                            .boundaries()
                            .iter()
                            .map(|b| start + b),
                    );
                    words.extend(
                        word_boundaries(piece)
                            .boundaries()
                            .iter()
                            .map(|b| start + b),
                    );
                }
            };
        for (at, c) in text.char_indices() {
            if c == '\n' || c == '\u{FFFC}' {
                flush(piece_start, at, &mut clusters, &mut words);
                let end = at + c.len_utf8();
                clusters.extend([at, end]);
                words.extend([at, end]);
                piece_start = end;
            }
        }
        flush(piece_start, text.len(), &mut clusters, &mut words);
        for list in [&mut clusters, &mut words] {
            list.push(text.len());
            list.sort_unstable();
            list.dedup();
        }
        (clusters, words)
    }

    fn check_bound(&self, position: usize) -> Result<(), FlatError> {
        if position > self.total {
            Err(FlatError::OutOfRange)
        } else {
            Ok(())
        }
    }
}

fn is_blank(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c == ' ' || c == '\t')
}

impl FlatIndex for VecParagraphs {
    fn len(&self) -> usize {
        self.total
    }

    fn paragraph_count(&self) -> usize {
        self.entries.len()
    }

    fn locate(&self, position: usize) -> Result<ParagraphPosition, FlatError> {
        self.check_bound(position)?;
        let paragraph = self
            .starts
            .partition_point(|s| *s <= position)
            .saturating_sub(1);
        Ok(ParagraphPosition {
            paragraph,
            offset: position - self.starts.get(paragraph).copied().unwrap_or(0),
        })
    }

    fn position(&self, at: ParagraphPosition) -> Result<usize, FlatError> {
        let start = *self.starts.get(at.paragraph).ok_or(FlatError::OutOfRange)?;
        let len = self.paragraph_flat(at.paragraph)?.len();
        if at.offset > len {
            return Err(FlatError::OutOfRange);
        }
        Ok(start + at.offset)
    }

    fn check_position(&self, position: usize) -> Result<(), FlatError> {
        self.check_bound(position)?;
        let (clusters, _) = self.boundaries();
        clusters
            .binary_search(&position)
            .map(|_| ())
            .map_err(|_| FlatError::NotBoundary)
    }

    fn text(&self, start: usize, end: usize) -> Result<String, FlatError> {
        if start > end {
            return Err(FlatError::InvalidRange);
        }
        self.check_bound(end)?;
        self.flat_text()
            .get(start..end)
            .map(str::to_owned)
            .ok_or(FlatError::NotBoundary)
    }

    fn snap_back(&self, position: usize) -> Result<usize, FlatError> {
        self.check_bound(position)?;
        let (clusters, _) = self.boundaries();
        Ok(clusters[clusters.partition_point(|b| *b <= position) - 1])
    }

    fn prev_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.check_bound(position)?;
        let (clusters, _) = self.boundaries();
        Ok(clusters.iter().rev().find(|b| **b < position).copied())
    }

    fn next_grapheme(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.check_bound(position)?;
        let (clusters, _) = self.boundaries();
        Ok(clusters.iter().find(|b| **b > position).copied())
    }

    fn prev_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.check_bound(position)?;
        let (_, words) = self.boundaries();
        let text = self.flat_text();
        let mut at = match words.iter().rev().find(|b| **b < position) {
            Some(b) => *b,
            None => return Ok(None),
        };
        while at > 0 {
            let end = words
                .iter()
                .find(|b| **b > at)
                .copied()
                .unwrap_or(text.len());
            if !is_blank(&text[at..end]) {
                break;
            }
            at = words.iter().rev().find(|b| **b < at).copied().unwrap_or(0);
        }
        Ok(Some(at))
    }

    fn next_word(&self, position: usize) -> Result<Option<usize>, FlatError> {
        self.check_bound(position)?;
        if position == self.total {
            return Ok(None);
        }
        let (_, words) = self.boundaries();
        let text = self.flat_text();
        let mut at = words
            .iter()
            .find(|b| **b > position)
            .copied()
            .unwrap_or(text.len());
        while at < text.len() {
            let end = words
                .iter()
                .find(|b| **b > at)
                .copied()
                .unwrap_or(text.len());
            if !is_blank(&text[at..end]) {
                break;
            }
            at = end;
        }
        Ok(Some(at))
    }

    fn word_at(&self, position: usize) -> Result<Option<(usize, usize)>, FlatError> {
        self.check_bound(position)?;
        if self.total == 0 {
            return Ok(None);
        }
        let (_, words) = self.boundaries();
        let anchor = position.min(self.total - 1);
        let i = words.partition_point(|b| *b <= anchor) - 1;
        Ok(Some((words[i], words[i + 1])))
    }
}

impl LayoutContext for VecParagraphs {
    fn catalog(&self) -> &StyleCatalog {
        &self.catalog
    }

    fn theme(&self) -> &Theme {
        &self.theme
    }

    fn paragraph(&self, index: usize) -> Option<&Paragraph> {
        self.entries.get(index)?.as_ref()
    }

    fn block_kind(&self, index: usize) -> Result<BlockKind, FlatError> {
        self.kinds.get(index).copied().ok_or(FlatError::OutOfRange)
    }

    fn section_of(&self, index: usize) -> Result<(usize, &Section), FlatError> {
        self.check_index(index)?;
        Ok((0, &self.section))
    }

    fn object_resource(&self, object: NodeId) -> Option<&[u8]> {
        self.entries
            .iter()
            .flatten()
            .flat_map(|p| p.anchored.iter())
            .find(|a| a.id == object)
            .map(|a| a.data.as_slice())
    }

    fn list_counters(&self, index: usize) -> Result<Option<ListCounters>, FlatError> {
        self.check_index(index)?;
        Ok(None)
    }

    fn paragraph_start(&self, index: usize) -> Result<usize, FlatError> {
        self.starts.get(index).copied().ok_or(FlatError::OutOfRange)
    }

    fn paragraph_flat(&self, index: usize) -> Result<Cow<'_, str>, FlatError> {
        match self.entries.get(index).ok_or(FlatError::OutOfRange)? {
            Some(p) => Ok(paragraph_flat(p)),
            None => Ok(Cow::Borrowed("\u{FFFC}")),
        }
    }
}

impl VecParagraphs {
    fn check_index(&self, index: usize) -> Result<(), FlatError> {
        if index < self.entries.len() {
            Ok(())
        } else {
            Err(FlatError::OutOfRange)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Paragraph {
        Paragraph {
            text: text.into(),
            ..Paragraph::default()
        }
    }

    #[test]
    fn flat_index_joins_paragraphs_with_newlines() {
        let doc = VecParagraphs::new(vec![para("ab"), para(""), para("c\u{e9}")]);
        assert_eq!(doc.len(), 2 + 1 + 1 + 3);
        assert_eq!(doc.text(0, doc.len()).unwrap(), "ab\n\nc\u{e9}");
        assert_eq!(
            doc.locate(2).unwrap(),
            ParagraphPosition {
                paragraph: 0,
                offset: 2
            }
        );
        assert_eq!(
            doc.locate(3).unwrap(),
            ParagraphPosition {
                paragraph: 1,
                offset: 0
            }
        );
        assert_eq!(
            doc.locate(4).unwrap(),
            ParagraphPosition {
                paragraph: 2,
                offset: 0
            }
        );
        for p in 0..=doc.len() {
            assert_eq!(doc.position(doc.locate(p).unwrap()).unwrap(), p);
        }
        assert_eq!(doc.locate(doc.len() + 1), Err(FlatError::OutOfRange));
        assert_eq!(doc.text(5, 4), Err(FlatError::InvalidRange));
        assert_eq!(doc.text(0, 6), Err(FlatError::NotBoundary));
    }

    #[test]
    fn clusters_cut_around_objects_and_newlines() {
        // "e" + combining acute is one cluster; after an object it is its own.
        let doc = VecParagraphs::new(vec![para("e\u{301}x")]);
        assert_eq!(doc.check_position(1), Err(FlatError::NotBoundary));
        assert_eq!(doc.snap_back(1), Ok(0));
        assert_eq!(doc.next_grapheme(0), Ok(Some(3)));
        assert_eq!(doc.prev_grapheme(3), Ok(Some(0)));
        assert_eq!(doc.prev_grapheme(0), Ok(None));
        assert_eq!(doc.next_grapheme(4), Ok(None));
        let mut object = para("\u{301}");
        let mut owner = super::super::RichDocument::new();
        object.anchored.push(super::super::AnchoredObject {
            id: owner.alloc_id().unwrap(),
            offset: 0,
            kind: "pic".into(),
            data: vec![9, 9],
        });
        let doc = VecParagraphs::new(vec![object]);
        assert_eq!(doc.text(0, doc.len()).unwrap(), "\u{fffc}\u{301}");
        assert_eq!(doc.check_position(3), Ok(()));
        assert_eq!(doc.check_position(1), Err(FlatError::NotBoundary));
        let id = doc.paragraph(0).unwrap().anchored[0].id;
        assert_eq!(doc.object_resource(id), Some(&[9u8, 9][..]));
    }

    #[test]
    fn words_skip_spaces_like_ctrl_arrows() {
        let doc = VecParagraphs::new(vec![para("hello  world"), para("x")]);
        assert_eq!(doc.next_word(0), Ok(Some(7)));
        assert_eq!(doc.next_word(2), Ok(Some(7)));
        assert_eq!(doc.next_word(7), Ok(Some(12)));
        assert_eq!(doc.prev_word(12), Ok(Some(7)));
        assert_eq!(doc.prev_word(7), Ok(Some(0)));
        assert_eq!(doc.prev_word(5), Ok(Some(0)));
        assert_eq!(doc.prev_word(0), Ok(None));
        assert_eq!(doc.next_word(doc.len()), Ok(None));
        assert_eq!(doc.word_at(8), Ok(Some((7, 12))));
        assert_eq!(doc.word_at(doc.len()), Ok(Some((13, 14))));
        assert_eq!(VecParagraphs::new(vec![]).word_at(0), Ok(None));
    }

    #[test]
    fn opaque_blocks_are_one_object_paragraph() {
        let mut doc = VecParagraphs::new(vec![para("a"), para("b")]);
        doc.set_kind(1, BlockKind::Opaque);
        assert_eq!(doc.len(), 1 + 1 + 3);
        assert_eq!(doc.paragraph_flat(1).unwrap(), "\u{fffc}");
        assert_eq!(doc.block_kind(1), Ok(BlockKind::Opaque));
        assert_eq!(doc.flat_to_text(1, 3), Ok(0));
        assert!(doc.paragraph(1).is_none());
    }

    #[test]
    fn replace_paragraphs_reports_a_consistent_outcome() {
        let mut doc = VecParagraphs::new(vec![para("ab"), para("cd"), para("ef")]);
        let before = doc.len();
        let outcome = doc.replace_paragraphs(1, 1, vec![para("c"), para("d"), para("D")]);
        assert_eq!(
            (
                outcome.first_paragraph,
                outcome.removed_paragraphs,
                outcome.inserted_paragraphs
            ),
            (1, 1, 3)
        );
        assert_eq!(outcome.start, 3);
        assert_eq!(outcome.doc_len, doc.len());
        assert_eq!(
            doc.len(),
            before - outcome.removed_bytes + outcome.inserted_bytes,
        );
        assert_eq!(doc.text(0, doc.len()).unwrap(), "ab\nc\nd\nD\nef");
    }
}
