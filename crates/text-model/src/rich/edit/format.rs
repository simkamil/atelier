// SPDX-License-Identifier: AGPL-3.0-or-later
//! Formatting as one history step (F0, `docs/us-240-format-protocol.md`).
//!
//! `RichDocument::apply` runs a raw transaction and leaves the flat edit
//! history out of it, so a formatting command could not be undone with the
//! text edits. [`RichDocument::apply_format`] applies `SetCharAttrs` and
//! `SetParaProps` operations as **one** history step (one undo, one revision),
//! all or nothing, and reports the span of paragraphs it touched. US-240,
//! US-243 and US-250 build their operations and call it; none of them needs
//! to touch the history.

use super::{
    Applier, EditOutcome, FlatError, FlatIndex, Item, LayoutScope, Meta, NodeId, RichDocument,
    RichOp,
};

impl RichDocument {
    /// Node ids of the flat paragraphs `first..first + count` (opaque blocks
    /// included), to name the paragraphs of `SetCharAttrs` operations.
    pub fn flat_node_ids(&self, first: usize, count: usize) -> Vec<NodeId> {
        self.with_flat(|t| t.range(first, count).iter().map(|e| e.id).collect())
    }

    /// Applies formatting operations (`SetCharAttrs`, `SetParaProps`) in one
    /// history step.
    ///
    /// * `selection` is the selection to restore with undo and redo (and the
    ///   one the outcome reports).
    /// * `scope` is `LayoutScope::All` when the operations can change more
    ///   than the paragraphs they name (a style that carries numbering).
    /// * `join_previous` merges the step with the one on top of the undo
    ///   stack (typing attributes: the inserted text and its formatting are
    ///   one undo) and does not raise the revision again.
    ///
    /// Any other operation is `FlatError::Unsupported`. When no operation
    /// changes a paragraph nothing is recorded (same revision, no undo step).
    /// A failure (unknown paragraph, invalid attribute, model limit) restores
    /// the document and the history.
    pub fn apply_format(
        &mut self,
        ops: Vec<RichOp>,
        selection: (usize, usize),
        scope: LayoutScope,
        join_previous: bool,
    ) -> Result<EditOutcome, FlatError> {
        let mut paragraphs = Vec::with_capacity(ops.len());
        for op in &ops {
            match op {
                RichOp::SetCharAttrs { para, .. } | RichOp::SetParaProps { para, .. } => {
                    paragraphs.push(*para);
                }
                _ => return Err(FlatError::Unsupported),
            }
        }
        if ops.is_empty() {
            return Ok(self.idle_outcome(selection.0.min(self.len())));
        }
        let tree = self.take_flat();
        let indexes: Option<Vec<usize>> = paragraphs.iter().map(|id| tree.index_of(*id)).collect();
        let Some(indexes) = indexes else {
            self.put_flat(tree);
            return Err(FlatError::Style);
        };
        let first = indexes.iter().copied().min().unwrap_or(0);
        let last = indexes.iter().copied().max().unwrap_or(0);
        let count = last - first + 1;
        let entries = tree.range(first, count);
        let Some(start) = tree.start(first) else {
            self.put_flat(tree);
            return Err(FlatError::Style);
        };
        let mut applier = Applier {
            doc: self,
            inverses: Vec::with_capacity(ops.len()),
        };
        let mut changed = false;
        let mut failure = None;
        for (op, id) in ops.into_iter().zip(paragraphs) {
            let result = applier.paragraph(id).and_then(|before| {
                applier.exec(op)?;
                Ok(applier.paragraph(id)? != before)
            });
            match result {
                Ok(differs) => changed |= differs,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        let inverses = std::mem::take(&mut applier.inverses);
        if failure.is_some() || !changed {
            for inverse in inverses.iter().rev() {
                let _ = inverse.apply_to(self);
            }
            self.put_flat(tree);
            return match failure {
                Some(error) => Err(error),
                None => Ok(self.idle_outcome(selection.0.min(self.len()))),
            };
        }
        let bytes = entries.iter().map(|e| e.len).sum::<usize>() + count - 1;
        let mut tree = tree;
        tree.splice(first, count, &entries);
        let doc_len = tree.total();
        let meta = Meta {
            start,
            removed_bytes: bytes,
            inserted_bytes: bytes,
            first,
            removed_paragraphs: count,
            inserted_paragraphs: count,
            before: entries.clone(),
            after: entries,
            here: selection,
            back: selection,
        };
        self.history.redo.clear();
        if !join_previous {
            self.history.revision += 1;
        }
        let mut ops = inverses;
        ops.reverse();
        let id = self.history.next_id();
        let joined = if join_previous {
            self.history.undo.last().map_or(1, |top| top.group.max(1)) + 1
        } else {
            1
        };
        self.history.push_undo(Item {
            id,
            ops,
            meta: meta.swapped(),
            group: joined,
            all: scope == LayoutScope::All,
        });
        self.put_flat(tree);
        let mut outcome = self.outcome(&meta, doc_len);
        outcome.layout_scope = scope;
        Ok(outcome)
    }
}
