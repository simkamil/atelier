// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(dead_code)]

use super::common::*;
use atelier_model::selection::{Affinity, TextPosition};
use atelier_text_model::rich::*;

pub fn pos(node: NodeId, offset: usize) -> TextPosition {
    TextPosition::new(node, offset, Affinity::After)
}

/// Appends a paragraph of `text` at the end of `parent`'s blocks.
pub fn add_para(doc: &mut RichDocument, parent: NodeId, text: &str) -> NodeId {
    let subtree = doc.new_paragraph(text).unwrap();
    let id = subtree.root;
    let position = doc.block_ids(Some(parent)).unwrap().len();
    run_checked(
        doc,
        RichOp::InsertBlock {
            parent: Some(parent),
            position,
            subtree,
        },
    );
    id
}

pub fn set_text(doc: &mut RichDocument, para: NodeId, text: &str) {
    run_checked(
        doc,
        RichOp::InsertText {
            para,
            offset: 0,
            text: text.into(),
        },
    );
}

pub fn set_style(doc: &mut RichDocument, para: NodeId, style: &str) {
    run_checked(
        doc,
        RichOp::SetParaProps {
            para,
            set: ParaProps {
                style: Some(style.into()),
                ..ParaProps::default()
            },
            clear: ParaProps::default(),
        },
    );
}

pub fn add_object(doc: &mut RichDocument, para: NodeId, offset: usize) {
    let id = doc.alloc_id().unwrap();
    run_checked(
        doc,
        RichOp::InsertInline {
            para,
            item: Inline::Anchored(AnchoredObject {
                id,
                offset,
                kind: "image".into(),
                data: vec![1],
            }),
        },
    );
}

pub fn add_link(
    doc: &mut RichDocument,
    para: NodeId,
    start: usize,
    end: usize,
    target: &str,
) -> NodeId {
    let id = doc.alloc_id().unwrap();
    run_checked(
        doc,
        RichOp::InsertInline {
            para,
            item: Inline::Link(Link {
                id,
                start,
                end,
                target: LinkTarget::Bookmark(target.into()),
                tooltip: None,
            }),
        },
    );
    id
}

pub fn add_header(doc: &mut RichDocument, section: NodeId, text: &str) -> NodeId {
    let subtree = doc.new_header_footer(text).unwrap();
    let hf = subtree.root;
    run_checked(
        doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Header,
            variant: HfVariant::Default,
            subtree,
        },
    );
    doc.block_ids(Some(hf)).unwrap()[0]
}

/// Appends a section; returns its id and its first paragraph.
pub fn add_section(doc: &mut RichDocument) -> (NodeId, NodeId) {
    let subtree = doc.new_section().unwrap();
    let id = subtree.root;
    let position = doc.section_ids().len();
    run_checked(
        doc,
        RichOp::InsertBlock {
            parent: None,
            position,
            subtree,
        },
    );
    (id, doc.block_ids(Some(id)).unwrap()[0])
}

/// Appends a table to any section body.
pub fn add_table_to(doc: &mut RichDocument, section: NodeId, rows: usize, cols: usize) -> NodeId {
    let subtree = doc.new_table(rows, cols).unwrap();
    let table = subtree.root;
    let position = doc.block_ids(Some(section)).unwrap().len();
    run_checked(
        doc,
        RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree,
        },
    );
    table
}

/// The paragraph of a cell.
pub fn cell_para(doc: &RichDocument, cell: NodeId) -> NodeId {
    doc.block_ids(Some(cell)).unwrap()[0]
}

pub fn bookmark_at(doc: &mut RichDocument, name: &str, para: NodeId, start: usize, end: usize) {
    use atelier_text_model::navigation::*;
    let edit = insert_bookmark(
        doc,
        name,
        pos(para, start),
        pos(para, end),
        NameMode::Internal,
    )
    .or_else(|_| insert_bookmark(doc, name, pos(para, start), pos(para, end), NameMode::User))
    .unwrap();
    doc.apply(&edit.transaction).unwrap();
}

/// Inserts a bookmark straight through the model, bypassing the name rules
/// (what an imported document can contain).
pub fn raw_bookmark(doc: &mut RichDocument, name: &str, para: NodeId, start: usize, end: usize) {
    let id = doc.alloc_id().unwrap();
    run_checked(
        doc,
        RichOp::InsertInline {
            para,
            item: Inline::Bookmark(Bookmark {
                id,
                name: name.into(),
                start,
                end,
            }),
        },
    );
}

/// A 1x1 table whose cell starts with a nested 1x1 table. Returns the paragraph
/// of the nested cell and the paragraph of the outer cell (after the nested table).
pub fn add_nested_table(doc: &mut RichDocument, section: NodeId) -> (NodeId, NodeId) {
    let outer = add_table_to(doc, section, 1, 1);
    let cell = cells(doc, outer)[0][0];
    let outer_para = cell_para(doc, cell);
    let nested = doc.new_table(1, 1).unwrap();
    let nested_root = nested.root;
    run_checked(
        doc,
        RichOp::InsertBlock {
            parent: Some(cell),
            position: 0,
            subtree: nested,
        },
    );
    let ncell = cells(doc, nested_root)[0][0];
    (cell_para(doc, ncell), outer_para)
}
