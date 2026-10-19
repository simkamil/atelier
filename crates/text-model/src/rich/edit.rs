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
use super::attrs::CharAttrs;
use super::document::RichDocument;
use super::error::RichError;
use super::flat::{EditOutcome, FlatEdit, FlatError, FlatIndex, LayoutScope};
use super::flat_impl::flat_len;
use super::flat_tree::{Entry, FlatTree};
use super::node::{Node, Subtree};
use super::ops::RichOp;
use super::paragraph::{CharRun, Paragraph};

mod format;

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
    /// Identifies the step in the history; undo and redo keep it, so that the
    /// position of the history can be compared with an earlier one.
    id: u64,
    ops: Vec<RichOp>,
    meta: Meta,
    /// Items undone or redone together with this one: the top item of a group
    /// (`replace_ranges`) carries the size of the group, the others one. A
    /// group is one history step: one undo, one revision.
    group: usize,
    /// Applying the item changes more than paragraphs (a style): the layout
    /// of the whole document must be redone (`LayoutScope::All`).
    all: bool,
}

/// What an edit replaced, in bytes and in paragraphs: `removed` units at
/// `start` became `inserted` units. Several of them in a row compose into the
/// smallest span that covers all.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    removed: usize,
    inserted: usize,
    first: usize,
    removed_paragraphs: usize,
    inserted_paragraphs: usize,
}

impl Span {
    fn of(meta: &Meta) -> Span {
        Span {
            start: meta.start,
            removed: meta.removed_bytes,
            inserted: meta.inserted_bytes,
            first: meta.first,
            removed_paragraphs: meta.removed_paragraphs,
            inserted_paragraphs: meta.inserted_paragraphs,
        }
    }

    /// `then` replaced a part of the document that `self` had already
    /// replaced, or lies next to or apart from it (coordinates after `self`).
    fn then(self, then: Span) -> Span {
        fn hull(
            (a_start, a_removed, a_inserted): (usize, usize, usize),
            (b_start, b_removed, b_inserted): (usize, usize, usize),
        ) -> (usize, usize, usize) {
            let start = a_start.min(b_start);
            let end = (a_start + a_inserted).max(b_start + b_removed);
            let width = end - start;
            (
                start,
                width - a_inserted + a_removed,
                width - b_removed + b_inserted,
            )
        }
        let (start, removed, inserted) = hull(
            (self.start, self.removed, self.inserted),
            (then.start, then.removed, then.inserted),
        );
        let (first, removed_paragraphs, inserted_paragraphs) = hull(
            (
                self.first,
                self.removed_paragraphs,
                self.inserted_paragraphs,
            ),
            (
                then.first,
                then.removed_paragraphs,
                then.inserted_paragraphs,
            ),
        );
        Span {
            start,
            removed,
            inserted,
            first,
            removed_paragraphs,
            inserted_paragraphs,
        }
    }
}

/// Undo and redo stacks and the revision counter of the flat edits.
#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Item>,
    redo: Vec<Item>,
    revision: u64,
    /// Last step identifier given.
    serial: u64,
    /// Times the stacks were emptied: a position of an earlier life is never equal.
    cleared: u64,
}

/// Makes room for `count` more items on a stack without dropping any of them
/// while they are pushed.
fn make_room(stack: &mut Vec<Item>, count: usize) {
    let excess = (stack.len() + count).saturating_sub(MAX_HISTORY);
    stack.drain(..excess.min(stack.len()));
}

impl History {
    /// Empty stacks, same revision.
    pub(crate) fn fresh(&self) -> History {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            revision: self.revision,
            serial: self.serial,
            cleared: self.cleared + 1,
        }
    }

    pub(crate) fn next_id(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    /// Where the history stands: the step on top of the undo stack. Two equal
    /// positions mean the same sequence of edits, whatever the revision.
    pub(crate) fn position(&self) -> (u64, u64) {
        (self.cleared, self.undo.last().map_or(0, |item| item.id))
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
        self.cleared += 1;
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
///
/// `lead` are the attributes of the first character that the edit replaced:
/// the new text takes them instead of those of the character before it.
fn insert_pieces(
    mut cur: Paragraph,
    at: usize,
    text: &str,
    lead: Option<CharAttrs>,
) -> Result<Vec<Paragraph>, FlatError> {
    // Gives the text just inserted at flat offset `from` the attributes of `lead`.
    let restyle = |p: &mut Paragraph, from: usize, len: usize, inherited: &CharAttrs| {
        let Some(lead) = lead.as_ref().filter(|l| *l != inherited && len > 0) else {
            return;
        };
        if let Ok((offset, _)) = p.cut(from) {
            // A cluster merged with its neighbour cannot be split: keep what was inherited.
            let _ = p.set_char_attrs(offset, offset + len, lead, inherited);
        }
    };
    if !text.contains('\n') {
        let inherited = cur.attrs_before_flat(at);
        cur.insert_flat(at, text)?;
        restyle(&mut cur, at, text.len(), &inherited);
        return Ok(vec![cur]);
    }
    let segments: Vec<&str> = text.split('\n').collect();
    let inherited = cur.attrs_before_flat(at);
    let inherit = lead.clone().unwrap_or_else(|| inherited.clone());
    let tail = cur.split_flat(at)?;
    let mut head = cur;
    let head_at = flat_len(&head);
    let first = segments.first().copied().unwrap_or("");
    head.insert_flat(head_at, first)?;
    restyle(&mut head, head_at, first.len(), &inherited);
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
        // The text that replaces a range looks like the start of that range.
        let lead = if end > start {
            cur.as_ref().and_then(|p| p.attrs_starting_flat(o1))
        } else {
            None
        };
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
                let mut pieces = insert_pieces(cur, o1, text, lead)?.into_iter();
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

    /// Applies the operations of one history item; `None` (nothing applied)
    /// when one of them fails. Returns the inverse operations and the flat
    /// length after the item.
    fn apply_item(&mut self, item: &Item) -> Option<(Vec<RichOp>, usize)> {
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
                    return None;
                }
            }
        }
        inverses.reverse();
        tree.splice(item.meta.first, item.meta.before.len(), &item.meta.after);
        let doc_len = tree.total();
        self.put_flat(tree);
        Some((inverses, doc_len))
    }

    /// Performs the top item of the undo (or redo) stack, with the rest of its
    /// group (`replace_ranges`): one step, one revision.
    fn step(&mut self, undo: bool) -> Option<EditOutcome> {
        let stack = |h: &mut History| if undo { h.undo.pop() } else { h.redo.pop() };
        let leader = stack(&mut self.history)?;
        let size = leader.group.max(1);
        let mut items = vec![leader];
        while items.len() < size {
            match stack(&mut self.history) {
                Some(item) => items.push(item),
                None => break,
            }
        }
        let mut span: Option<Span> = None;
        let all = items.iter().any(|item| item.all);
        let mut opposites = Vec::with_capacity(items.len());
        let (mut last, mut doc_len, mut failed) = (None, 0, false);
        for item in items {
            let Some((inverses, len)) = self.apply_item(&item) else {
                failed = true;
                break;
            };
            let here = Span::of(&item.meta);
            span = Some(span.map_or(here, |s| s.then(here)));
            doc_len = len;
            opposites.push(Item {
                id: item.id,
                ops: inverses,
                meta: item.meta.swapped(),
                group: 1,
                all: item.all,
            });
            last = Some(item.meta);
        }
        if failed {
            // Nothing can be trusted to come back: the history ends here.
            self.history.clear();
        }
        let meta = last?;
        self.history.revision += 1;
        if !failed {
            let count = opposites.len();
            if let Some(top) = opposites.last_mut() {
                top.group = count;
            }
            let target = if undo {
                &mut self.history.redo
            } else {
                &mut self.history.undo
            };
            make_room(target, count);
            for item in opposites {
                if undo {
                    self.history.push_redo(item);
                } else {
                    self.history.push_undo(item);
                }
            }
        }
        let mut outcome = self.outcome(&meta, doc_len);
        if all {
            outcome.layout_scope = LayoutScope::All;
        }
        if let Some(span) = span {
            outcome.start = span.start;
            outcome.removed_bytes = span.removed;
            outcome.inserted_bytes = span.inserted;
            outcome.first_paragraph = span.first;
            outcome.removed_paragraphs = span.removed_paragraphs;
            outcome.inserted_paragraphs = span.inserted_paragraphs;
        }
        Some(outcome)
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
        let id = self.history.next_id();
        self.history.push_undo(Item {
            id,
            ops,
            meta: meta.swapped(),
            group: 1,
            all: false,
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

impl RichDocument {
    /// Where the edit history stands (see `History::position`): equal to an
    /// earlier value when undo and redo brought the document back to the state
    /// it had then.
    pub fn history_position(&self) -> (u64, u64) {
        self.history.position()
    }

    /// Most ranges one [`RichDocument::replace_ranges`] call takes (half of
    /// the history, so that the group is never cut by the oldest entries).
    pub const MAX_REPLACE_RANGES: usize = MAX_HISTORY / 2;

    /// Replaces several ranges in one history step (find and replace all).
    /// `edits` are `(start, end, text)` in position order, disjoint, each
    /// valid for [`FlatEdit::replace_range`] on the document as it is now.
    /// Each range is replaced on its own, so the formatting of what lies
    /// between them is untouched; one undo (or redo) takes all back (or
    /// forth) and the revision rises by one. All or nothing: a failure undoes
    /// the ranges already applied. The outcome covers the smallest span of
    /// bytes and paragraphs that holds every change; its selection is a caret
    /// after the last replacement.
    pub fn replace_ranges(
        &mut self,
        edits: &[(usize, usize, &str)],
    ) -> Result<EditOutcome, FlatError> {
        if edits.is_empty() {
            return Ok(self.idle_outcome(0));
        }
        if edits.len() > Self::MAX_REPLACE_RANGES {
            return Err(FlatError::Limit);
        }
        let total = self.len();
        let mut floor = 0;
        for &(start, end, text) in edits {
            if start > end || start < floor {
                return Err(FlatError::InvalidRange);
            }
            if end > total {
                return Err(FlatError::OutOfRange);
            }
            check_text(text)?;
            self.check_position(start)?;
            self.check_position(end)?;
            floor = end;
        }
        let revision = self.history.revision;
        let saved_redo = std::mem::take(&mut self.history.redo);
        make_room(&mut self.history.undo, edits.len());
        let mut span: Option<Span> = None;
        // (start, bytes removed, bytes inserted) of the ranges that changed
        // something, last range first.
        let mut applied: Vec<(usize, usize, usize)> = Vec::new();
        for &(start, end, text) in edits.iter().rev() {
            let before = self.history.revision;
            match self.replace_range(start, end, text) {
                Ok(outcome) if self.history.revision != before => {
                    let here = Span {
                        start: outcome.start,
                        removed: outcome.removed_bytes,
                        inserted: outcome.inserted_bytes,
                        first: outcome.first_paragraph,
                        removed_paragraphs: outcome.removed_paragraphs,
                        inserted_paragraphs: outcome.inserted_paragraphs,
                    };
                    span = Some(span.map_or(here, |s| s.then(here)));
                    applied.push((start, outcome.removed_bytes, outcome.inserted_bytes));
                }
                Ok(_) => {}
                Err(error) => {
                    for _ in 0..applied.len() {
                        let _ = self.step(true);
                    }
                    self.history.redo = saved_redo;
                    self.history.revision = revision;
                    return Err(error);
                }
            }
        }
        let Some(span) = span else {
            self.history.redo = saved_redo;
            return Ok(self.idle_outcome(edits[0].0));
        };
        if let Some(top) = self.history.undo.last_mut() {
            top.group = applied.len();
        }
        self.history.revision = revision + 1;
        // Caret after the last replacement (the first one applied), shifted
        // by every change before it.
        let (start, _, inserted) = applied[0];
        let shift: isize = applied[1..]
            .iter()
            .map(|&(_, removed, inserted)| inserted as isize - removed as isize)
            .sum();
        let caret = (start + inserted).saturating_add_signed(shift);
        Ok(EditOutcome {
            revision: self.history.revision,
            start: span.start,
            removed_bytes: span.removed,
            inserted_bytes: span.inserted,
            doc_len: self.len(),
            first_paragraph: span.first,
            removed_paragraphs: span.removed_paragraphs,
            inserted_paragraphs: span.inserted_paragraphs,
            layout_scope: LayoutScope::Paragraphs,
            selection: (caret, caret),
            can_undo: !self.history.undo.is_empty(),
            can_redo: false,
        })
    }
}

impl RichDocument {
    /// Inserts an anchored object (a picture...) at the flat position `at`, as
    /// one undoable edit that adds [`super::flat::OBJECT_LEN`] bytes. `at`
    /// must be a valid position inside a paragraph; an object anchored there
    /// goes after the items already anchored at the same text offset (items at
    /// one offset appear in id order), so the reported `start` can be later
    /// than `at`, never earlier. The selection after the edit is the caret
    /// after the object.
    pub fn insert_object(
        &mut self,
        at: usize,
        kind: &str,
        data: Vec<u8>,
    ) -> Result<EditOutcome, FlatError> {
        self.check_position(at)?;
        let total = self.len();
        let mut tree = self.take_flat();
        let mut applier = Applier {
            doc: self,
            inverses: Vec::new(),
        };
        let result = Self::apply_object(&mut applier, &tree, at, kind, data);
        let inverses = std::mem::take(&mut applier.inverses);
        let (done, start) = match result {
            Ok(done) => done,
            Err(error) => {
                for inverse in inverses.iter().rev() {
                    let _ = inverse.apply_to(self);
                }
                self.put_flat(tree);
                return Err(error);
            }
        };
        tree.splice(done.first, done.before.len(), &done.after);
        let doc_len = tree.total();
        let meta = Meta {
            start,
            removed_bytes: 0,
            inserted_bytes: doc_len.saturating_sub(total),
            first: done.first,
            removed_paragraphs: done.before.len(),
            inserted_paragraphs: done.after.len(),
            before: done.before,
            after: done.after,
            here: (done.caret, done.caret),
            back: (start, start),
        };
        self.history.redo.clear();
        self.history.revision += 1;
        let mut ops = inverses;
        ops.reverse();
        let outcome_meta = meta.clone();
        let id = self.history.next_id();
        self.history.push_undo(Item {
            id,
            ops,
            meta: meta.swapped(),
            group: 1,
            all: false,
        });
        self.put_flat(tree);
        Ok(self.outcome(&outcome_meta, doc_len))
    }

    fn apply_object(
        ap: &mut Applier<'_>,
        tree: &FlatTree,
        at: usize,
        kind: &str,
        data: Vec<u8>,
    ) -> Result<(Applied, usize), FlatError> {
        use super::flat::{flat_to_text_offset, text_to_flat_offset};
        use super::paragraph::{AnchoredObject, Inline};
        let (index, offset) = tree.find(at).ok_or(FlatError::OutOfRange)?;
        let entry = *tree.entry(index).ok_or(FlatError::OutOfRange)?;
        if entry.opaque {
            return Err(FlatError::InvalidRange);
        }
        let mut paragraph = ap.paragraph(entry.id)?;
        let text_offset = flat_to_text_offset(&paragraph, offset)?;
        let flat_offset = text_to_flat_offset(&paragraph, text_offset, true)?;
        let start = at - offset + flat_offset;
        let id = ap.new_id()?;
        paragraph
            .insert_inline(Inline::Anchored(AnchoredObject {
                id,
                offset: text_offset,
                kind: kind.to_owned(),
                data,
            }))
            .map_err(model_error)?;
        let after = Self::entry_for(&entry, entry.id, &paragraph);
        ap.exec(restore(entry.id, paragraph))?;
        Ok((
            Applied {
                first: index,
                before: tree.range(index, 1),
                after: vec![after],
                caret: start + super::flat::OBJECT_LEN,
            },
            start,
        ))
    }
}

/// Source of a move or copy: its text split by paragraph, with the character
/// runs of each segment relative to the start of the segment's text.
struct Fragment {
    segments: Vec<(String, Vec<CharRun>)>,
}

impl RichDocument {
    /// Reads the formatted text of `start..end` (valid positions, `start <
    /// end`). The range must lie in plain paragraphs of one container and
    /// hold no object, link, field or bookmark (`Unsupported` otherwise).
    fn fragment(&self, start: usize, end: usize) -> Result<Fragment, FlatError> {
        let found = self.with_flat(|t| {
            let (p1, o1) = t.find(start)?;
            let (p2, o2) = t.find(end)?;
            Some((o1, o2, t.range(p1, p2 - p1 + 1)))
        });
        let (o1, o2, entries) = found.ok_or(FlatError::OutOfRange)?;
        let Some(first) = entries.first().copied() else {
            return Err(FlatError::OutOfRange);
        };
        if entries
            .iter()
            .any(|e| e.opaque || e.cell.is_some() || e.parent != first.parent)
        {
            return Err(FlatError::Unsupported);
        }
        let mut segments = Vec::with_capacity(entries.len());
        let count = entries.len();
        for (i, entry) in entries.iter().enumerate() {
            let p = self.paragraph(entry.id).map_err(model_error)?;
            let from = if i == 0 { o1 } else { 0 };
            let to = if i + 1 == count { o2 } else { entry.len };
            let (ts, ks) = p.cut(from)?;
            let (te, ke) = p.cut(to)?;
            // Links, fields and bookmarks do not follow a move: refused, not lost.
            if p.links.iter().any(|l| l.start < te && l.end > ts)
                || p.fields.iter().any(|f| f.start < te && f.end > ts)
                || p.bookmarks.iter().any(|b| {
                    (b.start < te && b.end > ts)
                        || (b.start == b.end && b.start >= ts && b.start <= te)
                })
            {
                return Err(FlatError::Unsupported);
            }
            if ks != ke {
                return Err(FlatError::Unsupported);
            }
            let runs = p
                .runs
                .iter()
                .filter(|r| r.end > ts && r.start < te)
                .map(|r| CharRun {
                    start: r.start.max(ts) - ts,
                    end: r.end.min(te) - ts,
                    attrs: r.attrs.clone(),
                })
                .collect();
            segments.push((p.text[ts..te].to_owned(), runs));
        }
        Ok(Fragment { segments })
    }

    /// Gives the text just inserted at flat `at` (segments of `fragment`, a
    /// `\n` between them) the character formatting it had at its source, as
    /// one history item. Returns its span, `None` when nothing differs.
    fn restyle_inserted(
        &mut self,
        at: usize,
        fragment: &Fragment,
    ) -> Result<Option<Span>, FlatError> {
        let (index, offset) = self
            .with_flat(|t| t.find(at))
            .ok_or(FlatError::OutOfRange)?;
        let count = fragment.segments.len();
        let before = self.with_flat(|t| t.range(index, count));
        if before.len() != count || before.iter().any(|e| e.opaque) {
            return Err(FlatError::Unsupported);
        }
        let mut tree = self.take_flat();
        let mut applier = Applier {
            doc: self,
            inverses: Vec::new(),
        };
        let mut result = Ok(());
        for (i, ((text, runs), entry)) in fragment.segments.iter().zip(&before).enumerate() {
            result = Self::restyle_one(
                &mut applier,
                entry.id,
                if i == 0 { offset } else { 0 },
                text.len(),
                runs,
            );
            if result.is_err() {
                break;
            }
        }
        let inverses = std::mem::take(&mut applier.inverses);
        if let Err(error) = result {
            for inverse in inverses.iter().rev() {
                let _ = inverse.apply_to(self);
            }
            self.put_flat(tree);
            return Err(error);
        }
        if inverses.is_empty() {
            self.put_flat(tree);
            return Ok(None);
        }
        // Flat lengths are unchanged: the index needs no update.
        tree.splice(index, count, &before);
        self.put_flat(tree);
        let mut ops = inverses;
        ops.reverse();
        let meta = Meta {
            start: at,
            removed_bytes: 0,
            inserted_bytes: 0,
            first: index,
            removed_paragraphs: count,
            inserted_paragraphs: count,
            before: before.clone(),
            after: before,
            here: (at, at),
            back: (at, at),
        };
        let span = Span::of(&meta);
        let id = self.history.next_id();
        self.history.push_undo(Item {
            id,
            ops,
            meta,
            group: 1,
            all: false,
        });
        Ok(Some(span))
    }

    fn restyle_one(
        applier: &mut Applier<'_>,
        id: NodeId,
        flat_at: usize,
        len: usize,
        runs: &[CharRun],
    ) -> Result<(), FlatError> {
        let mut p = applier.paragraph(id)?;
        let (base, _) = p.cut(flat_at)?;
        let shifted: Vec<CharRun> = runs
            .iter()
            .map(|r| CharRun {
                start: r.start + base,
                end: r.end + base,
                attrs: r.attrs.clone(),
            })
            .collect();
        let before = p.runs.clone();
        p.overwrite_runs(base, base + len, &shifted);
        if p.runs != before {
            applier.exec(restore(id, p))?;
        }
        Ok(())
    }

    /// Moves (or, with `copy`, duplicates) the flat range `start..end` to the
    /// position `target`, keeping the character formatting of the text, as
    /// one history step (one undo, one revision). A target strictly inside
    /// the range is `InvalidRange`; moving onto either end of the range
    /// changes nothing; a range with objects, tables or opaque blocks, or
    /// ends in different containers, is `Unsupported`. Paragraphs created by
    /// the move take the properties of the paragraph of the target. All or
    /// nothing. The selection after the call is the moved text.
    pub fn move_range(
        &mut self,
        start: usize,
        end: usize,
        target: usize,
        copy: bool,
    ) -> Result<EditOutcome, FlatError> {
        if start > end {
            return Err(FlatError::InvalidRange);
        }
        if end > self.len() || target > self.len() {
            return Err(FlatError::OutOfRange);
        }
        self.check_position(start)?;
        self.check_position(end)?;
        self.check_position(target)?;
        if start == end {
            return Ok(self.idle_outcome(target));
        }
        if target > start && target < end {
            return Err(FlatError::InvalidRange);
        }
        if !copy && (target == start || target == end) {
            return Ok(self.idle_outcome(start));
        }
        let fragment = self.fragment(start, end)?;
        let text = fragment
            .segments
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let width = end - start;
        if text.len() != width {
            return Err(FlatError::Unsupported);
        }
        // The target paragraph must be a plain one of the same container.
        let target_ok = self.with_flat(|t| {
            let source = t.find(start).and_then(|(i, _)| t.entry(i).copied());
            let here = t.find(target).and_then(|(i, _)| t.entry(i).copied());
            matches!((source, here), (Some(s), Some(h)) if !h.opaque && h.cell.is_none() && h.parent == s.parent)
        });
        if !target_ok {
            return Err(FlatError::Unsupported);
        }
        check_text(&text)?;
        let revision = self.history.revision;
        let saved_redo = std::mem::take(&mut self.history.redo);
        make_room(&mut self.history.undo, 3);
        let mut steps = 0usize;
        let result = self.move_steps(start, end, target, copy, &fragment, &text, &mut steps);
        let (span, landed) = match result {
            Ok(done) => done,
            Err(error) => {
                for _ in 0..steps {
                    let _ = self.step(true);
                }
                self.history.redo = saved_redo;
                self.history.revision = revision;
                return Err(error);
            }
        };
        let first = self.history.undo.len().saturating_sub(steps);
        // Undo ends on the first item of the group (it selects the source
        // range again), redo on the last one (it selects the moved text).
        if let Some(item) = self.history.undo.get_mut(first) {
            item.meta.here = (start, end);
        }
        if let Some(top) = self.history.undo.last_mut() {
            top.group = steps;
            top.meta.back = (landed, landed + width);
        }
        self.history.revision = revision + 1;
        Ok(EditOutcome {
            revision: self.history.revision,
            start: span.start,
            removed_bytes: span.removed,
            inserted_bytes: span.inserted,
            doc_len: self.len(),
            first_paragraph: span.first,
            removed_paragraphs: span.removed_paragraphs,
            inserted_paragraphs: span.inserted_paragraphs,
            layout_scope: LayoutScope::Paragraphs,
            selection: (landed, landed + width),
            can_undo: !self.history.undo.is_empty(),
            can_redo: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn move_steps(
        &mut self,
        start: usize,
        end: usize,
        target: usize,
        copy: bool,
        fragment: &Fragment,
        text: &str,
        steps: &mut usize,
    ) -> Result<(Span, usize), FlatError> {
        let span_of = |o: &EditOutcome| Span {
            start: o.start,
            removed: o.removed_bytes,
            inserted: o.inserted_bytes,
            first: o.first_paragraph,
            removed_paragraphs: o.removed_paragraphs,
            inserted_paragraphs: o.inserted_paragraphs,
        };
        let width = end - start;
        let before = self.history.revision;
        let inserted = self.replace_range(target, target, text)?;
        if self.history.revision == before {
            return Err(FlatError::Style);
        }
        *steps += 1;
        let mut span = span_of(&inserted);
        if let Some(styled) = self.restyle_inserted(target, fragment)? {
            *steps += 1;
            span = span.then(styled);
        }
        if copy {
            return Ok((span, target));
        }
        // The source, in coordinates after the insertion.
        let (s, e) = if target >= end {
            (start, end)
        } else {
            (start + width, end + width)
        };
        let deleted = self.replace_range(s, e, "")?;
        *steps += 1;
        span = span.then(span_of(&deleted));
        Ok((
            span,
            if target >= end {
                target - width
            } else {
                target
            },
        ))
    }
}
