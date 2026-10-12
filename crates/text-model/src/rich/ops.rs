// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reversible operations of the rich model, usable in `atelier-model`
//! transactions and history.
//!
//! Every operation first checks all its preconditions and only then mutates,
//! so a rejected operation leaves the document untouched. Inverses are
//! computed from the state just before the operation (`Operation::invert`):
//! text insertion inverts to a range deletion; every other operation inverts
//! to the exact inverse operation or to a snapshot restore of the affected
//! branch (`RestoreSubtree`, `CollapsePair`, `ExpandPair`).

use super::NodeId;
use super::attrs::{CharAttrs, ParaProps};
use super::document::RichDocument;
use super::error::RichError;
use super::node::{HfKind, HfVariant, Node, Subtree};
use super::paragraph::{Inline, Paragraph};
use super::table::{self, NewCell};
use atelier_model::selection::PositionMap;
use atelier_model::{CodecError, Decoder, Encoder, Operation};

/// One reversible edit of a [`RichDocument`].
///
/// `parent` designates a block container: `None` is the document (its
/// children are sections), `Some(id)` a section body, header/footer or cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RichOp {
    /// Inserts text; it inherits the attributes of the preceding character.
    InsertText {
        para: NodeId,
        offset: usize,
        text: String,
    },
    /// Deletes the range `start..end`.
    DeleteText {
        para: NodeId,
        start: usize,
        end: usize,
    },
    /// Un-defines the properties of `clear`, then defines those of `set`, on a
    /// range; adjacent identical runs are merged.
    SetCharAttrs {
        para: NodeId,
        start: usize,
        end: usize,
        set: CharAttrs,
        clear: CharAttrs,
    },
    /// Un-defines the properties of `clear`, then defines those of `set`.
    SetParaProps {
        para: NodeId,
        set: ParaProps,
        clear: ParaProps,
    },
    /// Assigns or removes the style reference of a table.
    SetTableStyle {
        table: NodeId,
        style: Option<String>,
    },
    /// Adds an anchored object, field, bookmark or link (new id required).
    InsertInline {
        para: NodeId,
        item: Inline,
    },
    /// Replaces the inline item having the same id (and the same kind).
    ModifyInline {
        para: NodeId,
        item: Inline,
    },
    RemoveInline {
        para: NodeId,
        id: NodeId,
    },
    /// Splits the paragraph at `position` at byte `offset`; the tail gets `new_id`.
    SplitParagraph {
        parent: Option<NodeId>,
        position: usize,
        offset: usize,
        new_id: NodeId,
    },
    /// Merges the paragraphs at `position` and `position + 1`; the first one
    /// keeps its id and properties.
    MergeParagraphs {
        parent: Option<NodeId>,
        position: usize,
    },
    /// Inserts a branch (a section when `parent` is `None`, else a block).
    InsertBlock {
        parent: Option<NodeId>,
        position: usize,
        subtree: Subtree,
    },
    RemoveBlock {
        parent: Option<NodeId>,
        position: usize,
    },
    AttachHeaderFooter {
        section: NodeId,
        kind: HfKind,
        variant: HfVariant,
        subtree: Subtree,
    },
    DetachHeaderFooter {
        section: NodeId,
        kind: HfKind,
        variant: HfVariant,
    },
    /// Inserts a row before row `index`; `cells` are the new 1x1 cells for the
    /// grid columns not already covered by a cell spanning from above.
    InsertRow {
        table: NodeId,
        index: usize,
        row: NodeId,
        cells: Vec<NewCell>,
    },
    DeleteRow {
        table: NodeId,
        index: usize,
    },
    /// Inserts a column before grid column `index`; `cells` are the new cells,
    /// top to bottom, for the rows not already covered by a spanning cell.
    InsertColumn {
        table: NodeId,
        index: usize,
        cells: Vec<NewCell>,
    },
    DeleteColumn {
        table: NodeId,
        index: usize,
    },
    /// Merges the grid rectangle into its top-left cell. The rectangle must
    /// not cut through a cell.
    MergeCells {
        table: NodeId,
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
    },
    /// Splits a merged cell into 1x1 cells (content stays in the first one).
    SplitCell {
        table: NodeId,
        cell: NodeId,
        cells: Vec<NewCell>,
    },
    /// Replaces an existing branch by a snapshot with the same root id.
    RestoreSubtree {
        subtree: Subtree,
    },
    /// Replaces the paragraph at `position` by `merged` and removes the one
    /// after it (inverse of a split).
    CollapsePair {
        parent: Option<NodeId>,
        position: usize,
        merged: Subtree,
    },
    /// Replaces the paragraph at `position` by `first` and inserts `second`
    /// after it (inverse of a merge).
    ExpandPair {
        parent: Option<NodeId>,
        position: usize,
        first: Subtree,
        second: Subtree,
    },
}

fn block_at(
    doc: &RichDocument,
    parent: Option<NodeId>,
    position: usize,
) -> Result<NodeId, RichError> {
    let list = doc.blocks_of(parent)?;
    list.get(position).ok_or(RichError::PositionOutOfBounds {
        position,
        len: list.len(),
    })
}

fn edit_paragraph(
    doc: &mut RichDocument,
    id: NodeId,
    edit: impl FnOnce(&mut Paragraph) -> Result<(), RichError>,
) -> Result<(), RichError> {
    let mut paragraph = doc.paragraph(id)?.clone();
    edit(&mut paragraph)?;
    doc.replace_paragraph(id, paragraph)
}

fn as_paragraph_branch(branch: &Subtree) -> Result<(), RichError> {
    match branch.nodes.get(&branch.root) {
        Some(Node::Paragraph(_)) if branch.nodes.len() == 1 => Ok(()),
        _ => Err(RichError::WrongNodeKind {
            id: branch.root,
            expected: "single paragraph",
        }),
    }
}

fn expand_pair(
    doc: &mut RichDocument,
    parent: Option<NodeId>,
    position: usize,
    first: &Subtree,
    second: &Subtree,
) -> Result<(), RichError> {
    as_paragraph_branch(first)?;
    as_paragraph_branch(second)?;
    let current = block_at(doc, parent, position)?;
    doc.paragraph(current)?;
    if first.root != current {
        return Err(RichError::Invariant("first paragraph must keep its id"));
    }
    doc.check_replace(&[current], &[first.clone(), second.clone()])?;
    doc.blocks_of_mut(parent)?
        .insert(position + 1, second.root)?;
    doc.replace_branches(&[current], vec![first.clone(), second.clone()])
}

fn collapse_pair(
    doc: &mut RichDocument,
    parent: Option<NodeId>,
    position: usize,
    merged: &Subtree,
) -> Result<(), RichError> {
    as_paragraph_branch(merged)?;
    let first = block_at(doc, parent, position)?;
    let second = block_at(doc, parent, position + 1)?;
    doc.paragraph(first)?;
    doc.paragraph(second)?;
    if merged.root != first {
        return Err(RichError::Invariant(
            "merged paragraph must keep the first id",
        ));
    }
    doc.check_replace(&[first, second], std::slice::from_ref(merged))?;
    doc.blocks_of_mut(parent)?.remove(position + 1)?;
    doc.replace_branches(&[first, second], vec![merged.clone()])
}

fn edit_table(
    doc: &mut RichDocument,
    table: NodeId,
    edit: impl FnOnce(&mut Subtree) -> Result<(), RichError>,
) -> Result<(), RichError> {
    let mut branch = doc.extract(table)?;
    if !matches!(branch.nodes.get(&table), Some(Node::Table(_))) {
        return Err(RichError::WrongNodeKind {
            id: table,
            expected: "table",
        });
    }
    edit(&mut branch)?;
    doc.replace_branches(&[table], vec![branch])
}

fn header_footer_list(
    doc: &mut RichDocument,
    section: NodeId,
    kind: HfKind,
) -> Result<&mut Vec<(HfVariant, NodeId)>, RichError> {
    match doc.nodes.get_mut(&section) {
        Some(Node::Section(s)) => Ok(match kind {
            HfKind::Header => &mut s.headers,
            HfKind::Footer => &mut s.footers,
        }),
        Some(_) => Err(RichError::WrongNodeKind {
            id: section,
            expected: "section",
        }),
        None => Err(RichError::NodeNotFound(section)),
    }
}

fn header_footer_id(
    doc: &mut RichDocument,
    section: NodeId,
    kind: HfKind,
    variant: HfVariant,
) -> Result<NodeId, RichError> {
    header_footer_list(doc, section, kind)?
        .iter()
        .find(|(v, _)| *v == variant)
        .map(|(_, id)| *id)
        .ok_or(RichError::Invariant("no such header or footer"))
}

impl RichOp {
    fn snapshot_paragraph(doc: &RichDocument, para: NodeId) -> Result<RichOp, RichError> {
        doc.paragraph(para)?;
        Ok(RichOp::RestoreSubtree {
            subtree: doc.extract(para)?,
        })
    }

    fn snapshot_table(doc: &RichDocument, table: NodeId) -> Result<RichOp, RichError> {
        let subtree = doc.extract(table)?;
        if !matches!(subtree.nodes.get(&table), Some(Node::Table(_))) {
            return Err(RichError::WrongNodeKind {
                id: table,
                expected: "table",
            });
        }
        Ok(RichOp::RestoreSubtree { subtree })
    }

    /// How a selection must follow this operation, for the operations that
    /// move text positions (`atelier_model::selection::Selection::mapped`).
    pub fn position_map(&self, doc: &RichDocument) -> Result<Option<PositionMap>, RichError> {
        Ok(match self {
            RichOp::InsertText { para, offset, text } => Some(PositionMap::Insert {
                node: *para,
                at: *offset,
                byte_len: text.len(),
            }),
            RichOp::DeleteText { para, start, end } if start <= end => Some(PositionMap::Delete {
                node: *para,
                start: *start,
                end: *end,
            }),
            RichOp::SplitParagraph {
                parent,
                position,
                offset,
                new_id,
            } => Some(PositionMap::Split {
                source: block_at(doc, *parent, *position)?,
                right: *new_id,
                at: *offset,
            }),
            RichOp::MergeParagraphs { parent, position } => {
                let left = block_at(doc, *parent, *position)?;
                let right = block_at(doc, *parent, *position + 1)?;
                Some(PositionMap::Merge {
                    left,
                    right,
                    left_len: doc.paragraph(left)?.text.len(),
                })
            }
            _ => None,
        })
    }

    fn apply_to(&self, doc: &mut RichDocument) -> Result<(), RichError> {
        match self {
            RichOp::InsertText { para, offset, text } => {
                edit_paragraph(doc, *para, |p| p.insert_text(*offset, text))
            }
            RichOp::DeleteText { para, start, end } => {
                edit_paragraph(doc, *para, |p| p.delete_text(*start, *end))
            }
            RichOp::SetCharAttrs {
                para,
                start,
                end,
                set,
                clear,
            } => edit_paragraph(doc, *para, |p| p.set_char_attrs(*start, *end, set, clear)),
            RichOp::SetParaProps { para, set, clear } => edit_paragraph(doc, *para, |p| {
                p.props.clear(clear);
                p.props.overlay(set);
                Ok(())
            }),
            RichOp::SetTableStyle { table, style } => {
                if style.as_deref() == Some("") {
                    return Err(RichError::InvalidAttribute("empty table style reference"));
                }
                match doc.node(*table) {
                    Some(Node::Table(_)) => {}
                    Some(_) => {
                        return Err(RichError::WrongNodeKind {
                            id: *table,
                            expected: "table",
                        });
                    }
                    None => return Err(RichError::NodeNotFound(*table)),
                }
                if let Some(Node::Table(value)) = doc.nodes.get_mut(table) {
                    value.style = style.clone();
                }
                Ok(())
            }
            RichOp::InsertInline { para, item } => {
                edit_paragraph(doc, *para, |p| p.insert_inline(item.clone()))
            }
            RichOp::ModifyInline { para, item } => {
                edit_paragraph(doc, *para, |p| p.replace_inline(item.clone()).map(|_| ()))
            }
            RichOp::RemoveInline { para, id } => {
                edit_paragraph(doc, *para, |p| p.remove_inline(*id).map(|_| ()))
            }
            RichOp::SplitParagraph {
                parent,
                position,
                offset,
                new_id,
            } => {
                let first_id = block_at(doc, *parent, *position)?;
                let mut head = doc.paragraph(first_id)?.clone();
                let tail = head.split_off(*offset)?;
                expand_pair(
                    doc,
                    *parent,
                    *position,
                    &Subtree::leaf(first_id, Node::Paragraph(head)),
                    &Subtree::leaf(*new_id, Node::Paragraph(tail)),
                )
            }
            RichOp::MergeParagraphs { parent, position } => {
                let first_id = block_at(doc, *parent, *position)?;
                let second_id = block_at(doc, *parent, *position + 1)?;
                let mut merged = doc.paragraph(first_id)?.clone();
                merged.append(doc.paragraph(second_id)?.clone());
                collapse_pair(
                    doc,
                    *parent,
                    *position,
                    &Subtree::leaf(first_id, Node::Paragraph(merged)),
                )
            }
            RichOp::InsertBlock {
                parent,
                position,
                subtree,
            } => {
                let len = doc.blocks_of(*parent)?.len();
                if *position > len {
                    return Err(RichError::PositionOutOfBounds {
                        position: *position,
                        len,
                    });
                }
                let root = subtree.nodes.get(&subtree.root);
                let allowed = match parent {
                    None => matches!(root, Some(Node::Section(_))),
                    Some(_) => root.is_some_and(Node::is_block),
                };
                if !allowed {
                    return Err(RichError::WrongNodeKind {
                        id: subtree.root,
                        expected: "node allowed in this container",
                    });
                }
                doc.check_replace(&[], std::slice::from_ref(subtree))?;
                doc.blocks_of_mut(*parent)?
                    .insert(*position, subtree.root)?;
                doc.commit_install(subtree.clone());
                Ok(())
            }
            RichOp::RemoveBlock { parent, position } => {
                let list = doc.blocks_of(*parent)?;
                let id = block_at(doc, *parent, *position)?;
                match parent {
                    None if list.len() < 2 => {
                        return Err(RichError::Invariant("a document needs a section"));
                    }
                    Some(p) if matches!(doc.node(*p), Some(Node::Cell(_))) => {
                        let remaining = list
                            .to_vec()
                            .into_iter()
                            .filter(|b| *b != id)
                            .filter(|b| matches!(doc.node(*b), Some(Node::Paragraph(_))))
                            .count();
                        if remaining == 0 {
                            return Err(RichError::Invariant("a cell needs a paragraph"));
                        }
                    }
                    _ => {}
                }
                doc.blocks_of_mut(*parent)?.remove(*position)?;
                doc.remove_branch(id)?;
                Ok(())
            }
            RichOp::AttachHeaderFooter {
                section,
                kind,
                variant,
                subtree,
            } => {
                if !matches!(
                    subtree.nodes.get(&subtree.root),
                    Some(Node::HeaderFooter(_))
                ) {
                    return Err(RichError::WrongNodeKind {
                        id: subtree.root,
                        expected: "header or footer",
                    });
                }
                if header_footer_list(doc, *section, *kind)?
                    .iter()
                    .any(|(v, _)| v == variant)
                {
                    return Err(RichError::Invariant("header/footer variant already set"));
                }
                doc.check_replace(&[], std::slice::from_ref(subtree))?;
                let list = header_footer_list(doc, *section, *kind)?;
                list.push((*variant, subtree.root));
                list.sort_by_key(|(v, _)| *v);
                doc.commit_install(subtree.clone());
                Ok(())
            }
            RichOp::DetachHeaderFooter {
                section,
                kind,
                variant,
            } => {
                let id = header_footer_id(doc, *section, *kind, *variant)?;
                header_footer_list(doc, *section, *kind)?.retain(|(v, _)| v != variant);
                doc.remove_branch(id)?;
                Ok(())
            }
            RichOp::InsertRow {
                table: t,
                index,
                row,
                cells,
            } => edit_table(doc, *t, |s| table::insert_row(s, *index, *row, cells)),
            RichOp::DeleteRow { table: t, index } => {
                edit_table(doc, *t, |s| table::delete_row(s, *index))
            }
            RichOp::InsertColumn {
                table: t,
                index,
                cells,
            } => edit_table(doc, *t, |s| table::insert_column(s, *index, cells)),
            RichOp::DeleteColumn { table: t, index } => {
                edit_table(doc, *t, |s| table::delete_column(s, *index))
            }
            RichOp::MergeCells {
                table: t,
                row,
                col,
                rows,
                cols,
            } => edit_table(doc, *t, |s| table::merge_cells(s, *row, *col, *rows, *cols)),
            RichOp::SplitCell {
                table: t,
                cell,
                cells,
            } => edit_table(doc, *t, |s| table::split_cell(s, *cell, cells)),
            RichOp::RestoreSubtree { subtree } => {
                doc.replace_branches(&[subtree.root], vec![subtree.clone()])
            }
            RichOp::CollapsePair {
                parent,
                position,
                merged,
            } => collapse_pair(doc, *parent, *position, merged),
            RichOp::ExpandPair {
                parent,
                position,
                first,
                second,
            } => expand_pair(doc, *parent, *position, first, second),
        }
    }

    fn inverse_of(&self, doc: &RichDocument) -> Result<RichOp, RichError> {
        Ok(match self {
            RichOp::InsertText { para, offset, text } => {
                doc.paragraph(*para)?;
                RichOp::DeleteText {
                    para: *para,
                    start: *offset,
                    end: offset
                        .checked_add(text.len())
                        .ok_or(RichError::OffsetOutOfBounds {
                            offset: *offset,
                            len: usize::MAX,
                        })?,
                }
            }
            RichOp::DeleteText { para, .. }
            | RichOp::SetCharAttrs { para, .. }
            | RichOp::SetParaProps { para, .. } => Self::snapshot_paragraph(doc, *para)?,
            RichOp::SetTableStyle { table, .. } => match doc.node(*table) {
                Some(Node::Table(value)) => RichOp::SetTableStyle {
                    table: *table,
                    style: value.style.clone(),
                },
                Some(_) => {
                    return Err(RichError::WrongNodeKind {
                        id: *table,
                        expected: "table",
                    });
                }
                None => return Err(RichError::NodeNotFound(*table)),
            },
            RichOp::InsertInline { para, item } => {
                doc.paragraph(*para)?;
                RichOp::RemoveInline {
                    para: *para,
                    id: item.id(),
                }
            }
            RichOp::ModifyInline { para, item } => RichOp::ModifyInline {
                para: *para,
                item: doc
                    .paragraph(*para)?
                    .inline(item.id())
                    .ok_or(RichError::NodeNotFound(item.id()))?,
            },
            RichOp::RemoveInline { para, id } => RichOp::InsertInline {
                para: *para,
                item: doc
                    .paragraph(*para)?
                    .inline(*id)
                    .ok_or(RichError::NodeNotFound(*id))?,
            },
            RichOp::SplitParagraph {
                parent, position, ..
            } => {
                let first = block_at(doc, *parent, *position)?;
                doc.paragraph(first)?;
                RichOp::CollapsePair {
                    parent: *parent,
                    position: *position,
                    merged: doc.extract(first)?,
                }
            }
            RichOp::MergeParagraphs { parent, position }
            | RichOp::CollapsePair {
                parent, position, ..
            } => {
                let first = block_at(doc, *parent, *position)?;
                let second = block_at(doc, *parent, *position + 1)?;
                doc.paragraph(first)?;
                doc.paragraph(second)?;
                RichOp::ExpandPair {
                    parent: *parent,
                    position: *position,
                    first: doc.extract(first)?,
                    second: doc.extract(second)?,
                }
            }
            RichOp::ExpandPair {
                parent, position, ..
            } => {
                let merged = block_at(doc, *parent, *position)?;
                doc.paragraph(merged)?;
                RichOp::CollapsePair {
                    parent: *parent,
                    position: *position,
                    merged: doc.extract(merged)?,
                }
            }
            RichOp::InsertBlock {
                parent, position, ..
            } => RichOp::RemoveBlock {
                parent: *parent,
                position: *position,
            },
            RichOp::RemoveBlock { parent, position } => RichOp::InsertBlock {
                parent: *parent,
                position: *position,
                subtree: doc.extract(block_at(doc, *parent, *position)?)?,
            },
            RichOp::AttachHeaderFooter {
                section,
                kind,
                variant,
                ..
            } => RichOp::DetachHeaderFooter {
                section: *section,
                kind: *kind,
                variant: *variant,
            },
            RichOp::DetachHeaderFooter {
                section,
                kind,
                variant,
            } => {
                let list = match doc.node(*section) {
                    Some(Node::Section(s)) => match kind {
                        HfKind::Header => &s.headers,
                        HfKind::Footer => &s.footers,
                    },
                    Some(_) => {
                        return Err(RichError::WrongNodeKind {
                            id: *section,
                            expected: "section",
                        });
                    }
                    None => return Err(RichError::NodeNotFound(*section)),
                };
                let id = list
                    .iter()
                    .find(|(v, _)| v == variant)
                    .map(|(_, id)| *id)
                    .ok_or(RichError::Invariant("no such header or footer"))?;
                RichOp::AttachHeaderFooter {
                    section: *section,
                    kind: *kind,
                    variant: *variant,
                    subtree: doc.extract(id)?,
                }
            }
            RichOp::InsertRow { table: t, .. }
            | RichOp::DeleteRow { table: t, .. }
            | RichOp::InsertColumn { table: t, .. }
            | RichOp::DeleteColumn { table: t, .. }
            | RichOp::MergeCells { table: t, .. }
            | RichOp::SplitCell { table: t, .. } => Self::snapshot_table(doc, *t)?,
            RichOp::RestoreSubtree { subtree } => RichOp::RestoreSubtree {
                subtree: doc.extract(subtree.root)?,
            },
        })
    }
}

impl Operation<RichDocument> for RichOp {
    type Error = RichError;

    fn apply(&self, state: &mut RichDocument) -> Result<(), RichError> {
        self.apply_to(state)
    }

    fn invert(&self, state: &RichDocument) -> Result<Self, RichError> {
        self.inverse_of(state)
    }

    /// The binary recovery-journal codec is out of scope for US-200.
    fn encode(&self, _: &mut Encoder) -> Result<(), CodecError> {
        Err(CodecError::InvalidData)
    }

    fn decode(_: &mut Decoder<'_>) -> Result<Self, CodecError> {
        Err(CodecError::InvalidData)
    }
}
