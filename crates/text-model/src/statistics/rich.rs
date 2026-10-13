// SPDX-License-Identifier: AGPL-3.0-or-later
//! Statistics of a rich document: scope, selection and incremental cache.

use super::rules::count_paragraph;
use super::{Counts, Statistics, StatisticsError, StatisticsOptions};
use crate::rich::{Node, NodeId, Paragraph, RichDocument};
use atelier_model::selection::{Selection, TextPosition};
use std::collections::{BTreeMap, BTreeSet};

/// Pushes the paragraphs reachable from a block container, in document order
/// (tables are read row by row, cell by cell, cells may nest tables). Opaque
/// blocks hold no countable text.
fn walk_blocks(doc: &RichDocument, id: NodeId, out: &mut Vec<NodeId>) {
    match doc.node(id) {
        Some(Node::Paragraph(_)) => out.push(id),
        Some(node @ (Node::HeaderFooter(_) | Node::Table(_) | Node::Row(_) | Node::Cell(_))) => {
            for child in node.children() {
                walk_blocks(doc, child, out);
            }
        }
        _ => {}
    }
}

/// Paragraphs of the counted scope, in document order.
fn scoped_paragraphs(doc: &RichDocument, options: &StatisticsOptions) -> Vec<NodeId> {
    let mut out = Vec::new();
    for section in doc.section_ids() {
        let Some(Node::Section(s)) = doc.node(section) else {
            continue;
        };
        for block in s.body.to_vec() {
            walk_blocks(doc, block, &mut out);
        }
        if options.include_headers_footers {
            for (_, hf) in s.headers.iter().chain(&s.footers) {
                walk_blocks(doc, *hf, &mut out);
            }
        }
        // Notes and text boxes: the model has none yet (see the options).
    }
    out
}

fn paragraph_of(doc: &RichDocument, id: NodeId) -> Result<&Paragraph, StatisticsError> {
    match doc.node(id) {
        Some(Node::Paragraph(p)) => Ok(p),
        Some(_) => Err(StatisticsError::NotAParagraph(id)),
        None => Err(StatisticsError::UnknownNode(id)),
    }
}

/// Statistics of the counted scope of a rich document: the body by default,
/// table cells included, headers and footers when asked.
pub fn document(doc: &RichDocument, options: &StatisticsOptions) -> Statistics {
    let mut total = Counts::default();
    for id in scoped_paragraphs(doc, options) {
        if let Ok(p) = paragraph_of(doc, id) {
            total += count_paragraph(&p.text, &options.rules);
        }
    }
    total.with_layout(options.lines, options.pages)
}

/// Every paragraph with the story it belongs to (the section for its body,
/// the header/footer node otherwise), headers and footers included.
fn stories(doc: &RichDocument) -> Vec<(NodeId, NodeId)> {
    let mut out = Vec::new();
    for section in doc.section_ids() {
        let Some(Node::Section(s)) = doc.node(section) else {
            continue;
        };
        let mut push = |story: NodeId, roots: Vec<NodeId>| {
            let mut ids = Vec::new();
            for root in roots {
                walk_blocks(doc, root, &mut ids);
            }
            out.extend(ids.into_iter().map(|p| (p, story)));
        };
        push(section, s.body.to_vec());
        for (_, hf) in s.headers.iter().chain(&s.footers) {
            push(*hf, vec![*hf]);
        }
    }
    out
}

fn check_offset(p: &Paragraph, position: &TextPosition) -> Result<(), StatisticsError> {
    if position.offset > p.text.len() {
        return Err(StatisticsError::OffsetOutOfBounds {
            offset: position.offset,
            len: p.text.len(),
        });
    }
    if !p.text.is_char_boundary(position.offset) {
        return Err(StatisticsError::NotCharBoundary {
            offset: position.offset,
        });
    }
    Ok(())
}

/// Statistics of a selection (any number of ranges, possibly over several
/// paragraphs and through table cells). Headers and footers are counted when
/// the selection lies in them whatever the scope options say. A word
/// partially selected counts for one word; overlapping ranges are merged so
/// that no text is counted twice; a caret gives zeros.
pub fn selection(
    doc: &RichDocument,
    selection: &Selection,
    options: &StatisticsOptions,
) -> Result<Statistics, StatisticsError> {
    let order = stories(doc);
    let index: BTreeMap<NodeId, usize> = order
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let locate = |position: &TextPosition| -> Result<usize, StatisticsError> {
        match index.get(&position.node) {
            Some(i) => Ok(*i),
            None => paragraph_of(doc, position.node)
                .and(Err(StatisticsError::UnknownNode(position.node))),
        }
    };
    // (paragraph index, start, end) pieces to count.
    let mut pieces: Vec<(usize, usize, usize)> = Vec::new();
    for range in selection.ranges() {
        let (a, h) = (&range.anchor, &range.head);
        let (ia, ih) = (locate(a)?, locate(h)?);
        check_offset(paragraph_of(doc, a.node)?, a)?;
        check_offset(paragraph_of(doc, h.node)?, h)?;
        let ((i0, o0), (i1, o1)) = if (ia, a.offset) <= (ih, h.offset) {
            ((ia, a.offset), (ih, h.offset))
        } else {
            ((ih, h.offset), (ia, a.offset))
        };
        if order[i0].1 != order[i1].1 {
            return Err(StatisticsError::RangeAcrossStories);
        }
        if i0 == i1 && o0 == o1 {
            continue;
        }
        for (i, (para, _)) in order.iter().enumerate().take(i1 + 1).skip(i0) {
            let len = paragraph_of(doc, *para)?.text.len();
            let s = if i == i0 { o0 } else { 0 };
            let e = if i == i1 { o1 } else { len };
            pieces.push((i, s, e));
        }
    }
    pieces.sort_unstable();
    let mut merged: Vec<(usize, usize, usize)> = Vec::new();
    for piece in pieces {
        match merged.last_mut() {
            Some(last) if last.0 == piece.0 && piece.1 <= last.2 => last.2 = last.2.max(piece.2),
            _ => merged.push(piece),
        }
    }
    let mut total = Counts::default();
    for (i, s, e) in merged {
        let text = &paragraph_of(doc, order[i].0)?.text;
        let mut counts = count_paragraph(&text[s..e], &options.rules);
        // A piece shorter than its paragraph counts as a paragraph when it
        // holds text; a whole paragraph follows the paragraph rule.
        if !(s == 0 && e == text.len()) {
            counts.paragraphs = usize::from(s < e);
        }
        total += counts;
    }
    Ok(total.with_layout(options.lines, options.pages))
}

/// Per-paragraph counts of a rich document, kept up to date from the
/// identifiers of the paragraphs that changed.
///
/// [`Self::update_paragraph`] handles an edit inside one paragraph in the
/// time of counting that paragraph. After a structural change (paragraph
/// split, merge, insertion or deletion, table edits), [`Self::sync`] walks the
/// ids of the document (no text is read) and recounts only the paragraphs
/// that are new or listed as changed.
#[derive(Clone, Debug)]
pub struct StatisticsCache {
    options: StatisticsOptions,
    paragraphs: BTreeMap<NodeId, Counts>,
    total: Counts,
}

impl StatisticsCache {
    /// Counts the scope of `doc` completely.
    pub fn build(doc: &RichDocument, options: StatisticsOptions) -> Self {
        let mut cache = Self {
            options,
            paragraphs: BTreeMap::new(),
            total: Counts::default(),
        };
        cache.sync(doc, &[]);
        cache
    }

    pub fn counts(&self) -> Counts {
        self.total
    }

    pub fn statistics(&self) -> Statistics {
        self.total
            .with_layout(self.options.lines, self.options.pages)
    }

    /// Counts held for one paragraph.
    pub fn paragraph_counts(&self, id: NodeId) -> Option<Counts> {
        self.paragraphs.get(&id).copied()
    }

    pub fn options(&self) -> &StatisticsOptions {
        &self.options
    }

    /// Sets the layout figures restituted by [`Self::statistics`].
    pub fn set_layout(&mut self, lines: Option<usize>, pages: Option<usize>) {
        self.options.lines = lines;
        self.options.pages = pages;
    }

    /// Recounts one paragraph whose text changed. It must be in the cache.
    pub fn update_paragraph(
        &mut self,
        doc: &RichDocument,
        id: NodeId,
    ) -> Result<(), StatisticsError> {
        let old = *self
            .paragraphs
            .get(&id)
            .ok_or(StatisticsError::NotCached(id))?;
        let new = count_paragraph(&paragraph_of(doc, id)?.text, &self.options.rules);
        self.total.replace(old, new);
        self.paragraphs.insert(id, new);
        Ok(())
    }

    /// Re-reads the structure: paragraphs gone from the scope are dropped,
    /// new ones and those in `changed` are recounted, the others are kept.
    pub fn sync(&mut self, doc: &RichDocument, changed: &[NodeId]) {
        let ids = scoped_paragraphs(doc, &self.options);
        let present: BTreeSet<NodeId> = ids.iter().copied().collect();
        let gone: Vec<NodeId> = self
            .paragraphs
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(old) = self.paragraphs.remove(&id) {
                self.total -= old;
            }
        }
        let dirty: BTreeSet<NodeId> = changed.iter().copied().collect();
        for id in ids {
            if self.paragraphs.contains_key(&id) && !dirty.contains(&id) {
                continue;
            }
            if let Ok(p) = paragraph_of(doc, id) {
                let new = count_paragraph(&p.text, &self.options.rules);
                let old = self.paragraphs.insert(id, new).unwrap_or_default();
                self.total.replace(old, new);
            }
        }
    }
}
