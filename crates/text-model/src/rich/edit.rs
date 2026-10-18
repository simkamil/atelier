// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`FlatEdit`] on [`RichDocument`] (US-204): multi-paragraph edits by flat
//! position, applied in place with their inverse, with undo and redo.
//!
//! An edit replaces a flat range by text. It is turned into [`RichOp`]s
//! (`RestoreSubtree` of one paragraph, `ExpandPair`, `CollapsePair`,
//! `InsertBlock`, `RemoveBlock`) that are applied directly to the document,
//! never through a transaction (a transaction checkpoints by cloning the
//! document). The inverse of every operation is computed from the state just
//! before it and kept, so undo applies the inverses backwards and redo the
//! inverses of those. The document is only touched paragraph by paragraph:
//! the cost of an edit is that of the paragraphs it touches plus `O(log n)`.
//!
//! Rules of a replacement, with `P1` the paragraph holding `start` and `P2`
//! the one holding `end`:
//!
//! * One paragraph: the flat range (text and the items whose U+FFFC lies in
//!   it) is deleted, then the text is inserted. The new text inherits the
//!   direct attributes of the character before it.
//! * `P1` and `P2` in the same block sequence (same section body or same
//!   cell): every block between them is removed (tables and opaque blocks
//!   included), the tail of `P1` and the head of `P2` are deleted, and the two
//!   paragraphs are merged; the merged paragraph keeps the id and the
//!   properties of `P1` (`MergeParagraphs`). Deleting a `\n` is that case.
//! * Otherwise (a range crossing cells, a table boundary or an opaque block)
//!   structure is kept: the text of every paragraph of the range is cleared
//!   and nothing is merged or removed, so the flat text keeps its separators
//!   and objects of opaque blocks. The reported outcome still describes the
//!   whole replaced interval.
//! * `\n` in the inserted text splits paragraphs. The new paragraphs take the
//!   properties of the split paragraph: the model has no "next style"
//!   (Word's style for the following paragraph), so the style that follows
//!   a split is the style of the paragraph being split.
//! * Text typed at a position of an opaque block (before or after its single
//!   U+FFFC) becomes new paragraphs before or after the block.
//! * A failure of the model while applying (limit reached, unexpected error)
//!   rolls everything back before the error is returned: `Limit` for an
//!   exhausted id space or capacity, `Style` (protocol `RICH_INTERNAL`) for an
//!   unexpected failure.

use super::NodeId;
use super::document::RichDocument;
use super::error::RichError;
use super::flat::{EditOutcome, FlatEdit, FlatError, FlatIndex, LayoutScope};
use super::flat_impl::flat_len;
use super::flat_tree::{Entry, FlatTree};
use super::node::{Node, Subtree};
use super::ops::RichOp;
use super::paragraph::{CharRun, Paragraph};

/// Entries kept in each stack; the oldest are dropped beyond it.
const MAX_HISTORY: usize = 10_000;

/// What applying an item changes, in the direction it is applied.
#[derive(Clone)]
struct Meta {
    start: usize,
    removed_bytes: usize,
    inserted_bytes: usize,
    first: usize,
    removed_paragraphs: usize,
    inserted_paragraphs: usize,
    /// Index entries replaced / entries that replace them.
    before: Vec<Entry>,
    after: Vec<Entry>,
    /// Selection after applying, and after applying the opposite item.
    here: (usize, usize),
    back: (usize, usize),
}

impl Meta {
    fn swapped(&self) -> Meta {
        Meta {
            start: self.start,
            removed_bytes: self.inserted_bytes,
            inserted_bytes: self.removed_bytes,
            first: self.first,
            removed_paragraphs: self.inserted_paragraphs,
            inserted_paragraphs: self.removed_paragraphs,
            before: self.after.clone(),
            after: self.before.clone(),
            here: self.back,
            back: self.here,
        }
    }
}

/// Operations to apply, in order, to perform an item.
struct Item {
    ops: Vec<RichOp>,
    meta: Meta,
}

/// Undo and redo stacks and the revision counter of the flat edits.
#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Item>,
    redo: Vec<Item>,
    revision: u64,
}

impl History {
    /// Empty stacks, same revision.
    pub(crate) fn fresh(&self) -> History {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            revision: self.revision,
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// The revision never goes back.
    pub(crate) fn raise_revision(&mut self, revision: u64) {
        self.revision = self.revision.max(revision);
    }

    /// Forgets both stacks (the revision keeps counting).
    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn push_undo(&mut self, item: Item) {
        if self.undo.len() >= MAX_HISTORY {
            self.undo.drain(..MAX_HISTORY / 4);
        }
        self.undo.push(item);
    }

    fn push_redo(&mut self, item: Item) {
        if self.redo.len() >= MAX_HISTORY {
            self.redo.drain(..MAX_HISTORY / 4);
        }
        self.redo.push(item);
    }
}

fn model_error(error: RichError) -> FlatError {
    match error {
        RichError::IdentifiersExhausted | RichError::CapacityExhausted => FlatError::Limit,
        _ => FlatError::Style,
    }
}

fn check_text(text: &str) -> Result<(), FlatError> {
    if text
        .chars()
        .any(|c| c == '\u{FFFC}' || ((c as u32) < 0x20 && c != '\n' && c != '\t'))
    {
        Err(FlatError::InvalidText)
    } else {
        Ok(())
    }
}

fn restore(id: NodeId, paragraph: Paragraph) -> RichOp {
    RichOp::RestoreSubtree {
        subtree: Subtree::leaf(id, Node::Paragraph(paragraph)),
    }
}

/// Applies operations and records their inverses.
struct Applier<'a> {
    doc: &'a mut RichDocument,
    /// Inverses, in application order.
    inverses: Vec<RichOp>,
}

impl Applier<'_> {
    fn exec(&mut self, op: RichOp) -> Result<(), FlatError> {
        let inverse = op.inverse_of(self.doc).map_err(model_error)?;
        op.apply_to(self.doc).map_err(model_error)?;
        self.inverses.push(inverse);
        Ok(())
    }

    fn paragraph(&self, id: NodeId) -> Result<Paragraph, FlatError> {
        self.doc.paragraph(id).cloned().map_err(model_error)
    }

    fn new_id(&mut self) -> Result<NodeId, FlatError> {
        self.doc.alloc_id().map_err(model_error)
    }

    /// Position of a block in its sequence.
    fn position(&self, parent: NodeId, id: NodeId) -> Result<usize, FlatError> {
        self.doc
            .blocks_of(Some(parent))
            .map_err(model_error)?
            .position(id)
            .ok_or(FlatError::Style)
    }
}

/// Result of planning and applying one replacement.
struct Applied {
    first: usize,
    before: Vec<Entry>,
    after: Vec<Entry>,
    /// Flat position of the caret after the edit.
    caret: usize,
}

/// `cur` with `text` inserted at the flat offset `at`: one paragraph, or
/// several when `text` holds `\n`.
fn insert_pieces(mut cur: Paragraph, at: usize, text: &str) -> Result<Vec<Paragraph>, FlatError> {
    if !text.contains('\n') {
        cur.insert_flat(at, text)?;
        return Ok(vec![cur]);
    }
    let segments: Vec<&str> = text.split('\n').collect();
    let inherit = cur.attrs_before_flat(at);
    let tail = cur.split_flat(at)?;
    let mut head = cur;
    head.insert_flat(flat_len(&head), segments.first().copied().unwrap_or(""))?;
    let make = |text: &str| Paragraph {
        text: text.to_owned(),
        runs: if inherit.is_empty() || text.is_empty() {
            Vec::new()
        } else {
            vec![CharRun {
                start: 0,
                end: text.len(),
                attrs: inherit.clone(),
            }]
        },
        props: head.props.clone(),
        ..Paragraph::default()
    };
    let last_index = segments.len() - 1;
    let mut pieces = Vec::with_capacity(segments.len());
    let middle: Vec<Paragraph> = segments
        .iter()
        .take(last_index)
        .skip(1)
        .map(|s| make(s))
        .collect();
    let mut last = make(segments.get(last_index).copied().unwrap_or(""));
    last.append(tail);
    pieces.push(head);
    pieces.extend(middle);
    pieces.push(last);
    Ok(pieces)
}

impl RichDocument {
    fn entry_for(base: &Entry, id: NodeId, paragraph: &Paragraph) -> Entry {
        Entry {
            id,
            opaque: false,
            cell: base.cell,
            parent: base.parent,
            section: base.section,
            len: flat_len(paragraph),
        }
    }

    /// Plans and applies the replacement; nothing is recorded here. On error
    /// the caller rolls back with the inverses collected so far.
    fn apply_replacement(
        ap: &mut Applier<'_>,
        tree: &FlatTree,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<Applied, FlatError> {
        let (p1, o1) = tree.find(start).ok_or(FlatError::OutOfRange)?;
        let (p2, o2) = tree.find(end).ok_or(FlatError::OutOfRange)?;
        let e1 = *tree.entry(p1).ok_or(FlatError::OutOfRange)?;
        let e2 = *tree.entry(p2).ok_or(FlatError::OutOfRange)?;
        let before = tree.range(p1, p2 - p1 + 1);
        let merge = p1 != p2 && e1.parent == e2.parent && !e1.opaque && !e2.opaque;

        // Deletion: P1 in memory, the other paragraphs applied.
        let mut cur: Option<Paragraph> = if e1.opaque {
            None
        } else {
            Some(ap.paragraph(e1.id)?)
        };
        let mut rest: Vec<Entry> = Vec::new();
        if end > start {
            if p1 == p2 {
                if let Some(p) = cur.as_mut() {
                    p.delete_flat(o1, o2)?;
                }
            } else if merge {
                let Some(first) = cur.as_mut() else {
                    return Err(FlatError::Style);
                };
                let mut second = ap.paragraph(e2.id)?;
                let first_len = flat_len(first);
                first.delete_flat(o1, first_len)?;
                second.delete_flat(0, o2)?;
                let pos1 = ap.position(e1.parent, e1.id)?;
                let pos2 = ap.position(e2.parent, e2.id)?;
                let between = pos2.checked_sub(pos1 + 1).ok_or(FlatError::Style)?;
                for _ in 0..between {
                    ap.exec(RichOp::RemoveBlock {
                        parent: Some(e1.parent),
                        position: pos1 + 1,
                    })?;
                }
                first.append(second);
            } else {
                for i in p1..=p2 {
                    let entry = *tree.entry(i).ok_or(FlatError::OutOfRange)?;
                    let from = if i == p1 { o1 } else { 0 };
                    let to = if i == p2 { o2 } else { entry.len };
                    if i == p1 {
                        if let Some(p) = cur.as_mut() {
                            p.delete_flat(from, to)?;
                        }
                        continue;
                    }
                    let mut kept = entry;
                    if !entry.opaque && to > from {
                        let mut p = ap.paragraph(entry.id)?;
                        p.delete_flat(from, to)?;
                        kept.len = flat_len(&p);
                        ap.exec(restore(entry.id, p))?;
                    }
                    rest.push(kept);
                }
            }
        }

        // Insertion at `start`, in P1.
        let mut caret = start + text.len();
        let mut after: Vec<Entry> = match cur {
            Some(cur) => {
                let pos1 = ap.position(e1.parent, e1.id)?;
                let splits = text.contains('\n');
                let pre = (merge && splits).then(|| cur.clone());
                let mut pieces = insert_pieces(cur, o1, text)?.into_iter();
                let first = pieces.next().ok_or(FlatError::Style)?;
                let others: Vec<Paragraph> = pieces.collect();
                let mut ids = Vec::with_capacity(others.len());
                for _ in &others {
                    ids.push(ap.new_id()?);
                }
                let mut entries = vec![Self::entry_for(&e1, e1.id, &first)];
                entries.extend(
                    others
                        .iter()
                        .zip(&ids)
                        .map(|(p, id)| Self::entry_for(&e1, *id, p)),
                );
                if merge {
                    let merged = pre.unwrap_or_else(|| first.clone());
                    ap.exec(RichOp::CollapsePair {
                        parent: Some(e1.parent),
                        position: pos1,
                        merged: Subtree::leaf(e1.id, Node::Paragraph(merged)),
                    })?;
                    if !others.is_empty() {
                        Self::expand(ap, &e1, pos1, first, others, &ids)?;
                    }
                } else if others.is_empty() {
                    if ap.doc.paragraph(e1.id).map_err(model_error)? != &first {
                        ap.exec(restore(e1.id, first))?;
                    }
                } else {
                    Self::expand(ap, &e1, pos1, first, others, &ids)?;
                }
                entries
            }
            None if text.is_empty() => vec![e1],
            None => {
                // An opaque block: text goes to new paragraphs around it.
                let pos1 = ap.position(e1.parent, e1.id)?;
                let after_block = o1 > 0;
                let base = pos1 + usize::from(after_block);
                let mut entries = Vec::new();
                for (i, segment) in text.split('\n').enumerate() {
                    let paragraph = Paragraph::with_text(segment);
                    let id = ap.new_id()?;
                    entries.push(Self::entry_for(&e1, id, &paragraph));
                    ap.exec(RichOp::InsertBlock {
                        parent: Some(e1.parent),
                        position: base + i,
                        subtree: Subtree::leaf(id, Node::Paragraph(paragraph)),
                    })?;
                }
                if after_block {
                    entries.insert(0, e1);
                    caret += 1;
                } else {
                    entries.push(e1);
                }
                entries
            }
        };
        after.extend(rest);
        Ok(Applied {
            first: p1,
            before,
            after,
            caret,
        })
    }

    /// Replaces the paragraph at `pos` by `first` and inserts `others` after
    /// it, with the ids `ids`.
    fn expand(
        ap: &mut Applier<'_>,
        base: &Entry,
        pos: usize,
        first: Paragraph,
        others: Vec<Paragraph>,
        ids: &[NodeId],
    ) -> Result<(), FlatError> {
        let mut items: Vec<(Paragraph, NodeId)> =
            others.into_iter().zip(ids.iter().copied()).collect();
        let (last, last_id) = items.pop().ok_or(FlatError::Style)?;
        ap.exec(RichOp::ExpandPair {
            parent: Some(base.parent),
            position: pos,
            first: Subtree::leaf(base.id, Node::Paragraph(first)),
            second: Subtree::leaf(last_id, Node::Paragraph(last)),
        })?;
        for (i, (paragraph, id)) in items.into_iter().enumerate() {
            ap.exec(RichOp::InsertBlock {
                parent: Some(base.parent),
                position: pos + 1 + i,
                subtree: Subtree::leaf(id, Node::Paragraph(paragraph)),
            })?;
        }
        Ok(())
    }

    fn outcome(&self, meta: &Meta, doc_len: usize) -> EditOutcome {
        EditOutcome {
            revision: self.history.revision,
            start: meta.start,
            removed_bytes: meta.removed_bytes,
            inserted_bytes: meta.inserted_bytes,
            doc_len,
            first_paragraph: meta.first,
            removed_paragraphs: meta.removed_paragraphs,
            inserted_paragraphs: meta.inserted_paragraphs,
            layout_scope: LayoutScope::Paragraphs,
            selection: meta.here,
            can_undo: !self.history.undo.is_empty(),
            can_redo: !self.history.redo.is_empty(),
        }
    }

    /// Outcome of a call that changed nothing.
    fn idle_outcome(&self, start: usize) -> EditOutcome {
        let (first, doc_len) = self.with_flat(|t| (t.find(start).map_or(0, |(i, _)| i), t.total()));
        EditOutcome {
            revision: self.history.revision,
            start,
            removed_bytes: 0,
            inserted_bytes: 0,
            doc_len,
            first_paragraph: first,
            removed_paragraphs: 0,
            inserted_paragraphs: 0,
            layout_scope: LayoutScope::Paragraphs,
            selection: (start, start),
            can_undo: !self.history.undo.is_empty(),
            can_redo: !self.history.redo.is_empty(),
        }
    }

    /// Performs the top item of the undo (or redo) stack.
    fn step(&mut self, undo: bool) -> Option<EditOutcome> {
        let item = if undo {
            self.history.undo.pop()?
        } else {
            self.history.redo.pop()?
        };
        let mut tree = self.take_flat();
        let mut inverses = Vec::with_capacity(item.ops.len());
        for op in &item.ops {
            let done = op
                .inverse_of(self)
                .and_then(|inverse| op.apply_to(self).map(|()| inverse));
            match done {
                Ok(inverse) => inverses.push(inverse),
                Err(_) => {
                    for inverse in inverses.iter().rev() {
                        let _ = inverse.apply_to(self);
                    }
                    self.put_flat(tree);
                    self.history.clear();
                    return None;
                }
            }
        }
        inverses.reverse();
        tree.splice(item.meta.first, item.meta.before.len(), &item.meta.after);
        let doc_len = tree.total();
        self.history.revision += 1;
        let opposite = Item {
            ops: inverses,
            meta: item.meta.swapped(),
        };
        if undo {
            self.history.push_redo(opposite);
        } else {
            self.history.push_undo(opposite);
        }
        self.put_flat(tree);
        Some(self.outcome(&item.meta, doc_len))
    }
}

impl FlatEdit for RichDocument {
    fn replace_range(
        &mut self,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<EditOutcome, FlatError> {
        if start > end {
            return Err(FlatError::InvalidRange);
        }
        let total = self.len();
        if end > total {
            return Err(FlatError::OutOfRange);
        }
        check_text(text)?;
        self.check_position(start)?;
        self.check_position(end)?;
        if start == end && text.is_empty() {
            return Ok(self.idle_outcome(start));
        }
        let mut tree = self.take_flat();
        let mut applier = Applier {
            doc: self,
            inverses: Vec::new(),
        };
        let result = Self::apply_replacement(&mut applier, &tree, start, end, text);
        let inverses = std::mem::take(&mut applier.inverses);
        let done = match result {
            Ok(done) => done,
            Err(error) => {
                for inverse in inverses.iter().rev() {
                    let _ = inverse.apply_to(self);
                }
                self.put_flat(tree);
                return Err(error);
            }
        };
        if inverses.is_empty() {
            self.put_flat(tree);
            return Ok(self.idle_outcome(start));
        }
        tree.splice(done.first, done.before.len(), &done.after);
        let doc_len = tree.total();
        let removed_bytes = end - start;
        let meta = Meta {
            start,
            removed_bytes,
            inserted_bytes: (doc_len + removed_bytes).saturating_sub(total),
            first: done.first,
            removed_paragraphs: done.before.len(),
            inserted_paragraphs: done.after.len(),
            before: done.before,
            after: done.after,
            here: (done.caret, done.caret),
            back: (start, end),
        };
        self.history.redo.clear();
        self.history.revision += 1;
        let mut ops = inverses;
        ops.reverse();
        let outcome_meta = meta.clone();
        self.history.push_undo(Item {
            ops,
            meta: meta.swapped(),
        });
        self.put_flat(tree);
        Ok(self.outcome(&outcome_meta, doc_len))
    }

    fn undo(&mut self) -> Option<EditOutcome> {
        self.step(true)
    }

    fn redo(&mut self) -> Option<EditOutcome> {
        self.step(false)
    }

    fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    fn revision(&self) -> u64 {
        self.history.revision
    }
}
