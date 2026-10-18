// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RichDocument::move_range` (R7c): move and copy of a flat range with its
//! character formatting, in one undo step.

mod flat_common;

use atelier_text_model::rich::flat::{FlatEdit, FlatError, FlatIndex, LayoutContext};
use atelier_text_model::rich::{CharAttrs, CharRun, Paragraph, RichDocument, RichOp};
use flat_common::*;

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

fn flat(doc: &RichDocument) -> String {
    doc.text(0, doc.len()).unwrap()
}

fn bold() -> CharAttrs {
    CharAttrs {
        bold: Some(true),
        ..CharAttrs::default()
    }
}

fn make_bold(doc: &mut RichDocument, paragraph: usize, start: usize, end: usize) {
    let para = doc.paragraph_ids()[paragraph];
    apply(
        doc,
        vec![RichOp::SetCharAttrs {
            para,
            start,
            end,
            set: bold(),
            clear: CharAttrs::default(),
        }],
    );
}

fn runs(doc: &RichDocument, index: usize) -> Vec<CharRun> {
    let p: &Paragraph = LayoutContext::paragraph(doc, index).expect("a paragraph");
    p.runs.clone()
}

fn run(start: usize, end: usize) -> CharRun {
    CharRun {
        start,
        end,
        attrs: bold(),
    }
}

#[test]
fn move_forward_keeps_formatting_and_is_one_undo_step() {
    let mut doc = body(&["abcdef ghi"]);
    make_bold(&mut doc, 0, 0, 3);
    let original = doc.clone();
    let revision = doc.revision();
    // Move "abc" (bold) after "def".
    let o = doc.move_range(0, 3, 6, false).unwrap();
    assert_eq!(flat(&doc), "defabc ghi");
    assert_eq!(runs(&doc, 0), vec![run(3, 6)]);
    assert_eq!(o.selection, (3, 6));
    assert_eq!(doc.revision(), revision + 1);
    assert!(o.can_undo && !o.can_redo);
    let undone = doc.undo().unwrap();
    assert_eq!(
        undone.selection,
        (0, 3),
        "undo selects the source range again"
    );
    assert_eq!(flat(&doc), "abcdef ghi");
    assert_eq!(runs(&doc, 0), vec![run(0, 3)]);
    assert_eq!(doc, original);
    assert!(!doc.can_undo(), "a single undo step");
    let redone = doc.redo().unwrap();
    assert_eq!(redone.selection, (3, 6), "redo selects the moved text");
    assert_eq!(flat(&doc), "defabc ghi");
    assert_eq!(runs(&doc, 0), vec![run(3, 6)]);
}

#[test]
fn move_backward_and_copy() {
    let mut doc = body(&["one two three"]);
    make_bold(&mut doc, 0, 8, 13);
    doc.move_range(8, 13, 0, false).unwrap();
    assert_eq!(flat(&doc), "threeone two ");
    assert_eq!(runs(&doc, 0), vec![run(0, 5)]);
    // Copy keeps the source.
    let o = doc.move_range(0, 5, 13, true).unwrap();
    assert_eq!(flat(&doc), "threeone two three");
    assert_eq!(runs(&doc, 0), vec![run(0, 5), run(13, 18)]);
    assert_eq!(o.selection, (13, 18));
    doc.undo().unwrap();
    assert_eq!(flat(&doc), "threeone two ");
}

#[test]
fn move_across_paragraphs_splits_and_merges() {
    let mut doc = body(&["alpha", "beta", "gamma"]);
    make_bold(&mut doc, 1, 0, 2);
    let original = doc.clone();
    // "alpha\nbe" (alpha plain, "be" bold) goes to the end of "gamma".
    let end = doc.len();
    doc.move_range(0, 8, end, false).unwrap();
    assert_eq!(flat(&doc), "ta\ngammaalpha\nbe");
    assert_eq!(runs(&doc, 2), vec![run(0, 2)]);
    doc.undo().unwrap();
    assert_eq!(doc, original);
    assert!(!doc.can_undo());
}

#[test]
fn refusals_leave_the_document_untouched() {
    let mut doc = body(&["abcdef"]);
    let before = doc.clone();
    assert_eq!(doc.move_range(1, 4, 2, false), Err(FlatError::InvalidRange));
    assert_eq!(doc.move_range(1, 4, 3, true), Err(FlatError::InvalidRange));
    assert_eq!(doc.move_range(4, 1, 0, false), Err(FlatError::InvalidRange));
    assert_eq!(doc.move_range(1, 9, 0, false), Err(FlatError::OutOfRange));
    assert_eq!(doc.move_range(1, 2, 9, false), Err(FlatError::OutOfRange));
    assert_eq!(doc, before);
    // Onto its own ends: nothing changes, no history step.
    let revision = doc.revision();
    doc.move_range(1, 4, 1, false).unwrap();
    doc.move_range(1, 4, 4, false).unwrap();
    assert_eq!(doc.revision(), revision);
    assert!(!doc.can_undo());
}
