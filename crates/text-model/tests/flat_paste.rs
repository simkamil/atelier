// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RichDocument::replace_with_paragraphs` (T0): a paste with runs, paragraph
//! properties and inline items, in one undo step.

mod flat_common;

use atelier_text_model::rich::flat::{FlatEdit, FlatIndex, OBJECT_LEN};
use atelier_text_model::rich::{
    AnchoredObject, BreakKind, CharAttrs, CharRun, InlineBreak, InlineSymbol, NodeId, ParaProps,
    Paragraph, RichDocument, RichOp,
};
use flat_common::apply;

fn doc_with(texts: &[&str]) -> RichDocument {
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

fn heading() -> ParaProps {
    ParaProps {
        style: Some("Heading1".into()),
        ..ParaProps::default()
    }
}

fn piece(text: &str) -> Paragraph {
    Paragraph::with_text(text)
}

#[test]
fn paste_of_several_paragraphs_is_one_undo_step() {
    let mut doc = doc_with(&["Hello world", "Second"]);
    let before = doc.clone();
    let mut a = piece("big");
    a.runs.push(CharRun {
        start: 0,
        end: 3,
        attrs: bold(),
    });
    let mut b = piece("title");
    b.props = heading();
    let c = piece("end");
    let revision = doc.revision();
    let outcome = doc.replace_with_paragraphs(5, 5, &[a, b, c]).unwrap();
    assert_eq!(flat(&doc), "Hellobig\ntitle\nend world\nSecond");
    assert_eq!(doc.revision(), revision + 1);
    assert_eq!(outcome.revision, doc.revision());
    assert_eq!(outcome.selection, (18, 18));
    let p = doc.paragraph_ids();
    assert_eq!(p.len(), 4);
    assert_eq!(doc.paragraph(p[0]).unwrap().runs[0].attrs, bold());
    assert_eq!(doc.paragraph(p[0]).unwrap().runs[0].start, 5);
    assert!(doc.paragraph(p[0]).unwrap().props.style.is_none());
    assert_eq!(
        doc.paragraph(p[1]).unwrap().props.style.as_deref(),
        Some("Heading1")
    );
    assert!(doc.paragraph(p[2]).unwrap().props.style.is_none());
    doc.validate().unwrap();
    let after = doc.clone();
    doc.undo().unwrap();
    assert_eq!(doc.revision(), revision + 2);
    assert_eq!(flat(&doc), flat(&before));
    assert_eq!(doc.paragraph_ids().len(), 2);
    doc.redo().unwrap();
    assert_eq!(flat(&doc), flat(&after));
    assert_eq!(
        doc.paragraph(doc.paragraph_ids()[1])
            .unwrap()
            .props
            .style
            .as_deref(),
        Some("Heading1")
    );
    doc.undo().unwrap();
    assert!(!doc.can_undo() || flat(&doc) == flat(&before));
}

#[test]
fn items_travel_with_the_text() {
    let mut doc = doc_with(&["ab"]);
    let ids: Vec<NodeId> = (0..3).map(|_| doc.alloc_id().unwrap()).collect();
    let mut p = piece("xy");
    p.anchored.push(AnchoredObject {
        id: ids[0],
        offset: 1,
        kind: "pic".into(),
        data: vec![1, 2, 3],
    });
    p.breaks.push(InlineBreak {
        id: ids[1],
        offset: 2,
        kind: BreakKind::Page,
    });
    p.symbols.push(InlineSymbol {
        id: ids[2],
        offset: 0,
        font: "Symbol".into(),
        code: 0xf0b7,
    });
    let len = doc.len();
    doc.replace_with_paragraphs(1, 1, &[p]).unwrap();
    assert_eq!(doc.len(), len + 2 + 3 * OBJECT_LEN);
    assert_eq!(
        doc.text(0, doc.len()).unwrap().matches('\u{fffc}').count(),
        3
    );
    let para = doc.paragraph(doc.paragraph_ids()[0]).unwrap().clone();
    assert_eq!(para.text, "axyb");
    assert_eq!(para.anchored[0].offset, 2);
    assert_eq!(para.anchored[0].data, vec![1, 2, 3]);
    assert_eq!(para.breaks[0].offset, 3);
    assert_eq!(para.symbols[0].offset, 1);
    doc.validate().unwrap();
    doc.undo().unwrap();
    assert_eq!(flat(&doc), "ab");
    assert_eq!(doc.len(), len);
}

#[test]
fn selection_is_replaced_and_empty_paragraph_adopts_the_first_piece() {
    let mut doc = doc_with(&["abc", ""]);
    let mut p = piece("X");
    p.props = heading();
    // Replace "b" by a piece: the paragraph keeps its own properties.
    doc.replace_with_paragraphs(1, 2, &[p.clone()]).unwrap();
    assert_eq!(flat(&doc), "aXc\n");
    assert!(
        doc.paragraph(doc.paragraph_ids()[0])
            .unwrap()
            .props
            .style
            .is_none()
    );
    // Into the empty paragraph: its properties become those of the piece.
    let at = doc.len();
    doc.replace_with_paragraphs(at, at, &[p]).unwrap();
    assert_eq!(flat(&doc), "aXc\nX");
    assert_eq!(
        doc.paragraph(doc.paragraph_ids()[1])
            .unwrap()
            .props
            .style
            .as_deref(),
        Some("Heading1")
    );
    doc.undo().unwrap();
    assert_eq!(flat(&doc), "aXc\n");
    assert!(
        doc.paragraph(doc.paragraph_ids()[1])
            .unwrap()
            .props
            .style
            .is_none()
    );
}

#[test]
fn a_trailing_empty_piece_leaves_the_tail_in_its_own_paragraph() {
    let mut doc = doc_with(&["abcd"]);
    let mut head = piece("one");
    head.props = heading();
    let mut tail = piece("");
    tail.props = heading();
    doc.replace_with_paragraphs(2, 2, &[head, tail]).unwrap();
    assert_eq!(flat(&doc), "abone\ncd");
    let p = doc.paragraph_ids();
    assert!(doc.paragraph(p[1]).unwrap().props.style.is_none());
    assert_eq!(doc.paragraph(p[1]).unwrap().text, "cd");
}

#[test]
fn refused_text_changes_nothing() {
    let mut doc = doc_with(&["abc"]);
    let before = doc.clone();
    let bad = piece("a\u{1}b");
    assert!(doc.replace_with_paragraphs(0, 0, &[bad]).is_err());
    assert_eq!(doc, before);
    assert!(!doc.can_undo() || doc.revision() == before.revision());
}
