// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pasting formatted paragraphs as one history step (T0, `docs/us-327-multidoc-protocol.md`).
//!
//! `RichDocument::apply` ends the flat history and `FlatEdit::replace_range`
//! only takes plain text, so a paste that carries runs, paragraph
//! properties and inline items (pictures, breaks, symbols, fields, links) could
//! not be undone with one Ctrl+Z. [`RichDocument::replace_with_paragraphs`]
//! replaces a flat range by the text of the pieces (one `replace_range`), then
//! gives the new paragraphs their runs, properties and items in a second
//! history item grouped with the first: one undo, one revision.

use super::{
    Applier, EditOutcome, FlatError, FlatIndex, Item, LayoutScope, Meta, RichDocument, Span,
    check_text, flat_len, make_room, model_error, restore,
};
use crate::rich::flat::{FlatEdit, text_to_flat_offset};
use crate::rich::paragraph::{
    AnchoredObject, CharRun, Inline, InlineBreak, InlineSymbol, Link, Paragraph,
};

/// True for a piece with no text and no item: the source selection ended right
/// after a paragraph mark.
fn is_empty_piece(p: &Paragraph) -> bool {
    p.text.is_empty()
        && p.anchored.is_empty()
        && p.breaks.is_empty()
        && p.symbols.is_empty()
        && p.fields.is_empty()
        && p.links.is_empty()
}

impl RichDocument {
    /// Replaces the flat range `start..end` by `pieces` (`pieces.len() - 1`
    /// paragraph marks between them), keeping what each piece carries, as one
    /// history step. A piece holds `text`, `runs` (relative to its text),
    /// `props` and the items `anchored`, `breaks`, `symbols`, `fields`, `links`
    /// (offsets relative to its text); the ids of the items must come from
    /// [`RichDocument::alloc_id`] of this document (fresh, never reused);
    /// bookmarks are ignored.
    ///
    /// * The first piece joins the paragraph at `start`, which keeps its
    ///   properties, unless it is empty and nothing is replaced: it then takes
    ///   the properties of the first piece.
    /// * The other pieces are new paragraphs with their own properties; the
    ///   last one, when it is empty, is the tail of the split paragraph and
    ///   keeps its properties; otherwise the tail follows its text.
    /// * A position in an opaque block is `Unsupported`.
    ///
    /// All or nothing. The selection after the call is a caret after the pasted
    /// content.
    pub fn replace_with_paragraphs(
        &mut self,
        start: usize,
        end: usize,
        pieces: &[Paragraph],
    ) -> Result<EditOutcome, FlatError> {
        if pieces.is_empty() {
            return self.replace_range(start, end, "");
        }
        let text = pieces
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        check_text(&text)?;
        let first = self
            .with_flat(|t| {
                t.find(start)
                    .and_then(|(i, o)| t.entry(i).map(|e| (i, *e, o)))
            })
            .ok_or(FlatError::OutOfRange)?;
        let (index, entry, o1) = first;
        if entry.opaque {
            return Err(FlatError::Unsupported);
        }
        let before_p = self.paragraph(entry.id).map_err(model_error)?.clone();
        let (a0, _) = before_p.cut(o1)?;
        let adopt = start == end && flat_len(&before_p) == 0;
        let revision = self.history.revision;
        let saved_redo = std::mem::take(&mut self.history.redo);
        make_room(&mut self.history.undo, 2);
        let out1 = match self.replace_range(start, end, &text) {
            Ok(out) => out,
            Err(error) => {
                self.history.redo = saved_redo;
                return Err(error);
            }
        };
        if self.history.revision == revision {
            self.history.redo = saved_redo;
            return Ok(out1);
        }
        match self.restyle_pieces(index, a0, adopt, pieces, (start, end)) {
            Ok((span, caret)) => {
                let span = Span {
                    start: out1.start,
                    removed: out1.removed_bytes,
                    inserted: out1.inserted_bytes,
                    first: out1.first_paragraph,
                    removed_paragraphs: out1.removed_paragraphs,
                    inserted_paragraphs: out1.inserted_paragraphs,
                }
                .then(span);
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
            Err(error) => {
                let _ = self.step(true);
                self.history.redo = saved_redo;
                self.history.revision = revision;
                Err(error)
            }
        }
    }

    /// Second item of the paste: runs, properties and items of the `pieces`
    /// in the paragraphs `index..index + pieces.len()` that `replace_range`
    /// made. Returns the span of the item and the caret after the content.
    fn restyle_pieces(
        &mut self,
        index: usize,
        a0: usize,
        adopt: bool,
        pieces: &[Paragraph],
        selection: (usize, usize),
    ) -> Result<(Span, usize), FlatError> {
        let n = pieces.len();
        let before = self.with_flat(|t| t.range(index, n));
        if before.len() != n || before.iter().any(|e| e.opaque) {
            return Err(FlatError::Unsupported);
        }
        let start = self
            .with_flat(|t| t.start(index))
            .ok_or(FlatError::OutOfRange)?;
        let mut done: Vec<(usize, Paragraph, usize, bool)> = Vec::with_capacity(n);
        for (i, (piece, entry)) in pieces.iter().zip(&before).enumerate() {
            let mut p = self.paragraph(entry.id).map_err(model_error)?.clone();
            let base = if i == 0 { a0 } else { 0 };
            let end = base + piece.text.len();
            if p.text.get(base..end) != Some(piece.text.as_str()) {
                return Err(FlatError::Style);
            }
            let runs: Vec<CharRun> = piece
                .runs
                .iter()
                .map(|r| CharRun {
                    start: r.start + base,
                    end: r.end + base,
                    attrs: r.attrs.clone(),
                })
                .collect();
            p.overwrite_runs(base, end, &runs);
            let mut at_end = false;
            let mut items: Vec<Inline> = Vec::new();
            for x in &piece.anchored {
                at_end |= x.offset == piece.text.len();
                items.push(Inline::Anchored(AnchoredObject {
                    id: x.id,
                    offset: x.offset + base,
                    kind: x.kind.clone(),
                    data: x.data.clone(),
                }));
            }
            for x in &piece.breaks {
                at_end |= x.offset == piece.text.len();
                items.push(Inline::Break(InlineBreak {
                    id: x.id,
                    offset: x.offset + base,
                    kind: x.kind,
                }));
            }
            for x in &piece.symbols {
                at_end |= x.offset == piece.text.len();
                items.push(Inline::Symbol(InlineSymbol {
                    id: x.id,
                    offset: x.offset + base,
                    font: x.font.clone(),
                    code: x.code,
                }));
            }
            for x in &piece.fields {
                let mut field = x.clone();
                field.start += base;
                field.end += base;
                items.push(Inline::Field(field));
            }
            for x in &piece.links {
                items.push(Inline::Link(Link {
                    id: x.id,
                    start: x.start + base,
                    end: x.end + base,
                    target: x.target.clone(),
                    tooltip: x.tooltip.clone(),
                }));
            }
            for item in items {
                p.insert_inline(item).map_err(model_error)?;
            }
            let own_props = if i == 0 {
                adopt
            } else {
                !(i + 1 == n && is_empty_piece(piece))
            };
            if own_props {
                p.props = piece.props.clone();
            }
            done.push((i, p, end, at_end));
        }
        let mut tree = self.take_flat();
        let mut applier = Applier {
            doc: self,
            inverses: Vec::new(),
        };
        let mut result = Ok(());
        let mut after = Vec::with_capacity(n);
        let mut changed = false;
        for ((_, p, _, _), entry) in done.iter().zip(&before) {
            after.push(RichDocument::entry_for(entry, entry.id, p));
            let differs = applier.paragraph(entry.id).map(|old| old != *p);
            match differs {
                Ok(true) => {
                    changed = true;
                    result = applier.exec(restore(entry.id, p.clone()));
                }
                Ok(false) => {}
                Err(error) => result = Err(error),
            }
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
        let removed: usize = before.iter().map(|e| e.len).sum::<usize>() + n - 1;
        let inserted: usize = after.iter().map(|e| e.len).sum::<usize>() + n - 1;
        tree.splice(index, n, &after);
        let last_start = tree.start(index + n - 1).unwrap_or(start);
        self.put_flat(tree);
        let (_, last, last_end, at_end) = done.pop().ok_or(FlatError::Style)?;
        let caret = last_start + text_to_flat_offset(&last, last_end, at_end).unwrap_or(last_end);
        if !changed {
            return Ok((
                Span {
                    start,
                    removed: 0,
                    inserted: 0,
                    first: index,
                    removed_paragraphs: 0,
                    inserted_paragraphs: 0,
                },
                caret,
            ));
        }
        let meta = Meta {
            start,
            removed_bytes: removed,
            inserted_bytes: inserted,
            first: index,
            removed_paragraphs: n,
            inserted_paragraphs: n,
            before,
            after,
            here: (caret, caret),
            back: selection,
        };
        let mut ops = inverses;
        ops.reverse();
        let id = self.history.next_id();
        self.history.push_undo(Item {
            id,
            ops,
            meta: meta.swapped(),
            group: 2,
            all: false,
        });
        Ok((Span::of(&meta), caret))
    }
}
