// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RichDocument::apply_format` (F0): formatting as one history step.

mod flat_common;

use atelier_text_model::rich::flat::{FlatEdit, FlatError, FlatIndex, LayoutContext, LayoutScope};
use atelier_text_model::rich::{Alignment, CharAttrs, ParaProps, RichDocument, RichOp};
use flat_common::apply;

fn body(texts: &[&str]) -> RichDocument {
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let first = doc.paragraph_ids()[0];
    apply(
        &mut doc,
        vec![RichOp::InsertText {
            para: first,
            offset: 0,
            text: texts[0].into(),
        }],
    );
    for (i, text) in texts.iter().enumerate().skip(1) {
        let sub = doc.new_paragraph(text).unwrap();
        apply(
            &mut doc,
            vec![RichOp::InsertBlock {
                parent: Some(section),
                position: i,
                subtree: sub,
            }],
        );
    }
    doc
}

fn bold(para: atelier_text_model::rich::NodeId, start: usize, end: usize) -> RichOp {
    RichOp::SetCharAttrs {
        para,
        start,
        end,
        set: CharAttrs {
            bold: Some(true),
            ..CharAttrs::default()
        },
        clear: CharAttrs::default(),
    }
}

fn center(para: atelier_text_model::rich::NodeId) -> RichOp {
    RichOp::SetParaProps {
        para,
        set: ParaProps {
            alignment: Some(Alignment::Center),
            ..ParaProps::default()
        },
        clear: ParaProps::default(),
    }
}

fn is_bold(doc: &RichDocument, index: usize, offset: usize) -> bool {
    LayoutContext::paragraph(doc, index)
        .unwrap()
        .attrs_at(offset)
        .bold
        == Some(true)
}

#[test]
fn one_step_one_revision_and_undo_redo_restore_the_selection() {
    let mut doc = body(&["abcd", "efgh"]);
    doc.replace_range(0, 0, "x").unwrap();
    let ids = doc.paragraph_ids();
    let revision = FlatEdit::revision(&doc);
    let outcome = doc
        .apply_format(
            vec![bold(ids[0], 1, 3), bold(ids[1], 0, 2)],
            (2, 8),
            LayoutScope::Paragraphs,
            false,
        )
        .unwrap();
    assert_eq!(FlatEdit::revision(&doc), revision + 1);
    assert_eq!(outcome.revision, revision + 1);
    assert_eq!(
        (
            outcome.first_paragraph,
            outcome.removed_paragraphs,
            outcome.inserted_paragraphs
        ),
        (0, 2, 2)
    );
    assert_eq!(
        (outcome.start, outcome.removed_bytes, outcome.inserted_bytes),
        (0, 10, 10)
    );
    assert_eq!(outcome.selection, (2, 8));
    assert_eq!(outcome.layout_scope, LayoutScope::Paragraphs);
    assert!(is_bold(&doc, 0, 1) && is_bold(&doc, 1, 0));
    assert!(doc.can_undo());
    let undone = doc.undo().unwrap();
    assert_eq!(undone.selection, (2, 8));
    assert_eq!((undone.first_paragraph, undone.removed_paragraphs), (0, 2));
    assert!(!is_bold(&doc, 0, 1) && !is_bold(&doc, 1, 0));
    assert_eq!(FlatEdit::revision(&doc), revision + 2);
    assert!(doc.can_redo());
    doc.redo().unwrap();
    assert!(is_bold(&doc, 0, 1) && is_bold(&doc, 1, 0));
    // The text edit before it is a step of its own.
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.text(0, doc.len()).unwrap(), "abcd\nefgh");
}

#[test]
fn a_new_format_step_empties_the_redo_stack() {
    let mut doc = body(&["abcd"]);
    doc.replace_range(0, 0, "x").unwrap();
    doc.undo().unwrap();
    assert!(doc.can_redo());
    let id = doc.paragraph_ids()[0];
    doc.apply_format(vec![center(id)], (0, 0), LayoutScope::Paragraphs, false)
        .unwrap();
    assert!(!doc.can_redo());
}

#[test]
fn a_format_that_changes_nothing_records_nothing() {
    let mut doc = body(&["abcd"]);
    doc.replace_range(0, 0, "x").unwrap();
    let id = doc.paragraph_ids()[0];
    doc.apply_format(vec![bold(id, 0, 2)], (0, 0), LayoutScope::Paragraphs, false)
        .unwrap();
    let (revision, position) = (FlatEdit::revision(&doc), doc.history_position());
    let outcome = doc
        .apply_format(vec![bold(id, 0, 2)], (0, 0), LayoutScope::Paragraphs, false)
        .unwrap();
    assert_eq!((outcome.removed_bytes, outcome.inserted_bytes), (0, 0));
    assert_eq!(FlatEdit::revision(&doc), revision);
    assert_eq!(doc.history_position(), position);
    assert!(
        doc.apply_format(Vec::new(), (0, 0), LayoutScope::Paragraphs, false)
            .is_ok()
    );
    assert_eq!(FlatEdit::revision(&doc), revision);
}

#[test]
fn a_failing_operation_restores_the_document_and_the_history() {
    let mut doc = body(&["abcd", "efgh"]);
    doc.replace_range(0, 0, "x").unwrap();
    let ids = doc.paragraph_ids();
    let (revision, position, text) = (
        FlatEdit::revision(&doc),
        doc.history_position(),
        doc.text(0, doc.len()).unwrap(),
    );
    let invalid = RichOp::SetCharAttrs {
        para: ids[1],
        start: 0,
        end: 2,
        set: CharAttrs {
            size_half_points: Some(0),
            ..CharAttrs::default()
        },
        clear: CharAttrs::default(),
    };
    assert!(
        doc.apply_format(
            vec![bold(ids[0], 0, 2), invalid],
            (0, 0),
            LayoutScope::Paragraphs,
            false
        )
        .is_err()
    );
    assert!(!is_bold(&doc, 0, 0));
    assert_eq!(
        (FlatEdit::revision(&doc), doc.history_position()),
        (revision, position)
    );
    assert_eq!(doc.text(0, doc.len()).unwrap(), text);
    assert_eq!(
        doc.apply_format(
            vec![RichOp::DeleteText {
                para: ids[0],
                start: 0,
                end: 1
            }],
            (0, 0),
            LayoutScope::Paragraphs,
            false
        )
        .unwrap_err(),
        FlatError::Unsupported
    );
}

#[test]
fn joined_with_the_previous_step_it_is_one_undo_and_one_revision() {
    let mut doc = body(&["abcd"]);
    let before = FlatEdit::revision(&doc);
    doc.replace_range(2, 2, "ZZ").unwrap();
    let id = doc.paragraph_ids()[0];
    let outcome = doc
        .apply_format(vec![bold(id, 2, 4)], (4, 4), LayoutScope::Paragraphs, true)
        .unwrap();
    assert_eq!(FlatEdit::revision(&doc), before + 1);
    assert_eq!(outcome.revision, before + 1);
    assert!(is_bold(&doc, 0, 2));
    doc.undo().unwrap();
    assert_eq!(doc.text(0, doc.len()).unwrap(), "abcd");
    assert!(!is_bold(&doc, 0, 2));
    assert_eq!(FlatEdit::revision(&doc), before + 2);
    doc.redo().unwrap();
    assert_eq!(doc.text(0, doc.len()).unwrap(), "abZZcd");
    assert!(is_bold(&doc, 0, 2) && !is_bold(&doc, 0, 4));
}

#[test]
fn a_whole_document_scope_survives_undo_and_redo() {
    let mut doc = body(&["abcd"]);
    let id = doc.paragraph_ids()[0];
    let o = doc
        .apply_format(vec![center(id)], (0, 0), LayoutScope::All, false)
        .unwrap();
    assert_eq!(o.layout_scope, LayoutScope::All);
    assert_eq!(doc.undo().unwrap().layout_scope, LayoutScope::All);
    assert_eq!(doc.redo().unwrap().layout_scope, LayoutScope::All);
    // A plain text edit stays at paragraph scope.
    assert_eq!(
        doc.replace_range(0, 0, "q").unwrap().layout_scope,
        LayoutScope::Paragraphs
    );
}
