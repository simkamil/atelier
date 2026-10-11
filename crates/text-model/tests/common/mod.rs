// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(dead_code)]

use atelier_model::{Transaction, TransactionError};
use atelier_text_model::rich::{CharAttrs, NodeId, RichDocument, RichError, RichOp};

pub type Undo = Transaction<RichOp>;
pub type Failure = TransactionError<RichError>;

pub fn tx(ops: Vec<RichOp>) -> Transaction<RichOp> {
    Transaction::new(ops).unwrap()
}

/// Applies the operations as one transaction and returns the inverse.
pub fn run(doc: &mut RichDocument, ops: Vec<RichOp>) -> Result<Undo, Failure> {
    doc.apply(&tx(ops))
}

/// Applies one operation, validates the document, and checks that applying
/// the inverse restores the previous document exactly.
pub fn run_checked(doc: &mut RichDocument, op: RichOp) {
    let before = doc.clone();
    let undo = run(doc, vec![op.clone()]).unwrap_or_else(|e| panic!("{op:?} failed: {e}"));
    doc.validate().unwrap();
    let after = doc.clone();
    doc.apply(&undo).unwrap();
    doc.validate().unwrap();
    assert_eq!(*doc, before, "undo of {op:?}");
    doc.apply(&tx(vec![op])).unwrap();
    assert_eq!(*doc, after, "redo");
}

/// An operation that must be rejected, leaving the document unchanged.
pub fn assert_rejected(doc: &mut RichDocument, op: RichOp) -> RichError {
    let before = doc.clone();
    let Err(err) = run(doc, vec![op.clone()]) else {
        panic!("{op:?} should fail");
    };
    assert_eq!(*doc, before, "rejected op mutated the document");
    doc.validate().unwrap();
    match err {
        TransactionError::Inversion { source, .. }
        | TransactionError::Application { source, .. } => source,
    }
}

pub fn first_para(doc: &RichDocument) -> NodeId {
    doc.paragraph_ids()[0]
}

pub fn first_section(doc: &RichDocument) -> NodeId {
    doc.section_ids()[0]
}

/// Puts `text` in the first paragraph.
pub fn with_text(text: &str) -> (RichDocument, NodeId) {
    let mut doc = RichDocument::new();
    let para = first_para(&doc);
    run(
        &mut doc,
        vec![RichOp::InsertText {
            para,
            offset: 0,
            text: text.to_owned(),
        }],
    )
    .unwrap();
    (doc, para)
}

pub fn bold() -> CharAttrs {
    CharAttrs {
        bold: Some(true),
        ..CharAttrs::default()
    }
}

pub fn italic() -> CharAttrs {
    CharAttrs {
        italic: Some(true),
        ..CharAttrs::default()
    }
}

/// Appends a table of the given size to the first section.
pub fn add_table(doc: &mut RichDocument, rows: usize, cols: usize) -> NodeId {
    let section = first_section(doc);
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

/// Cell ids of a table: `[row][cell in row order]`.
pub fn cells(doc: &RichDocument, table: NodeId) -> Vec<Vec<NodeId>> {
    use atelier_text_model::rich::Node;
    let Some(Node::Table(t)) = doc.node(table) else {
        panic!("not a table");
    };
    t.rows
        .iter()
        .map(|r| match doc.node(*r) {
            Some(Node::Row(row)) => row.cells.clone(),
            _ => panic!("not a row"),
        })
        .collect()
}

pub fn span(doc: &RichDocument, cell: NodeId) -> (usize, usize) {
    use atelier_text_model::rich::Node;
    match doc.node(cell) {
        Some(Node::Cell(c)) => (c.row_span, c.col_span),
        _ => panic!("not a cell"),
    }
}
