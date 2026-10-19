// SPDX-License-Identifier: AGPL-3.0-or-later
//! Targeted tests of the rich document operations (US-200).

mod common;

use atelier_model::selection::{Affinity, Selection, TextPosition};
use atelier_model::{EditKind, History};
use atelier_text_model::rich::*;
use common::*;

const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";

#[test]
fn new_document_is_valid_with_one_section_and_one_paragraph() {
    let doc = RichDocument::new();
    doc.validate().unwrap();
    assert_eq!(doc.section_ids().len(), 1);
    assert_eq!(doc.paragraph_ids().len(), 1);
    assert_eq!(doc.node_count(), 2);
}

#[test]
fn ids_are_unique_and_never_reused() {
    let mut doc = RichDocument::new();
    let a = doc.alloc_id().unwrap();
    let b = doc.alloc_id().unwrap();
    assert_ne!(a, b);
    assert!(doc.node(a).is_none());
    assert!(!doc.paragraph_ids().contains(&a));
}

// ----- text ----------------------------------------------------------------

#[test]
fn insert_text_at_start_middle_end_and_undo() {
    let (mut doc, p) = with_text("ac");
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 1,
            text: "b".into(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 0,
            text: ">".into(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 4,
            text: "<".into(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().text, ">abc<");
}

#[test]
fn empty_insert_and_empty_delete_are_no_ops() {
    let (mut doc, p) = with_text("abc");
    let before = doc.clone();
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 1,
            text: String::new(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::DeleteText {
            para: p,
            start: 2,
            end: 2,
        },
    );
    assert_eq!(doc, before);
}

#[test]
fn multibyte_graphemes_are_atomic_for_offsets() {
    let text = format!("aé{FAMILY}e\u{301}z");
    let (mut doc, p) = with_text(&text);
    let e_acute = 1 + 'é'.len_utf8();
    // Inside the two bytes of é, inside the family emoji, inside e + combining mark.
    for offset in [2, 1 + 2 + 4, e_acute + FAMILY.len() + 1] {
        let err = assert_rejected(
            &mut doc,
            RichOp::InsertText {
                para: p,
                offset,
                text: "x".into(),
            },
        );
        assert!(
            matches!(err, RichError::NotGraphemeBoundary { .. }),
            "{offset}: {err:?}"
        );
        assert_rejected(
            &mut doc,
            RichOp::DeleteText {
                para: p,
                start: offset,
                end: text.len(),
            },
        );
    }
    assert_eq!(
        assert_rejected(
            &mut doc,
            RichOp::DeleteText {
                para: p,
                start: 0,
                end: 999
            }
        ),
        RichError::OffsetOutOfBounds {
            offset: 999,
            len: text.len()
        }
    );
    assert_eq!(
        assert_rejected(
            &mut doc,
            RichOp::DeleteText {
                para: p,
                start: 3,
                end: 1
            }
        ),
        RichError::InvalidRange { start: 3, end: 1 }
    );
    // Whole clusters can be deleted and restored.
    run_checked(
        &mut doc,
        RichOp::DeleteText {
            para: p,
            start: e_acute,
            end: e_acute + FAMILY.len(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().text, "aée\u{301}z");
}

#[test]
fn insert_that_would_glue_clusters_is_rejected() {
    // A combining mark inserted before "b" would merge with "a".
    let (mut doc, p) = with_text("ab");
    let err = assert_rejected(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 1,
            text: "\u{301}".into(),
        },
    );
    assert!(matches!(err, RichError::NotGraphemeBoundary { .. }));
}

#[test]
fn inserted_text_inherits_previous_attributes_but_not_at_start() {
    let (mut doc, p) = with_text("ab");
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 2,
            set: bold(),
            clear: CharAttrs::default(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 2,
            text: "c".into(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().runs.len(), 1);
    assert_eq!(doc.paragraph(p).unwrap().runs[0].end, 3);
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 0,
            text: "_".into(),
        },
    );
    let runs = &doc.paragraph(p).unwrap().runs;
    assert_eq!((runs[0].start, runs[0].end), (1, 4));
}

#[test]
fn delete_whole_text_and_across_runs_normalizes() {
    let (mut doc, p) = with_text("aabbcc");
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 2,
            set: bold(),
            clear: CharAttrs::default(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 4,
            end: 6,
            set: bold(),
            clear: CharAttrs::default(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().runs.len(), 2);
    run_checked(
        &mut doc,
        RichOp::DeleteText {
            para: p,
            start: 2,
            end: 4,
        },
    );
    // The two bold runs now touch and merge.
    let runs = doc.paragraph(p).unwrap().runs.clone();
    assert_eq!(runs.len(), 1);
    assert_eq!((runs[0].start, runs[0].end), (0, 4));
    run_checked(
        &mut doc,
        RichOp::DeleteText {
            para: p,
            start: 0,
            end: 4,
        },
    );
    assert!(doc.paragraph(p).unwrap().runs.is_empty());
}

// ----- attributes ----------------------------------------------------------

#[test]
fn apply_and_remove_attributes_with_normalization() {
    let (mut doc, p) = with_text("abcdef");
    let none = CharAttrs::default();
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 3,
            set: bold(),
            clear: none.clone(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 3,
            end: 6,
            set: bold(),
            clear: none.clone(),
        },
    );
    let runs = &doc.paragraph(p).unwrap().runs;
    assert_eq!(runs.len(), 1, "adjacent identical ranges merge");
    assert_eq!((runs[0].start, runs[0].end), (0, 6));
    // Italic on the middle splits the bold run in three.
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 2,
            end: 4,
            set: italic(),
            clear: none.clone(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().runs.len(), 3);
    // Removing italic merges them again.
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 6,
            set: none.clone(),
            clear: italic(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().runs.len(), 1);
    // Removing the last property drops the run.
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 6,
            set: none.clone(),
            clear: bold(),
        },
    );
    assert!(doc.paragraph(p).unwrap().runs.is_empty());
    // Empty range: no-op.
    let before = doc.clone();
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 3,
            end: 3,
            set: bold(),
            clear: none,
        },
    );
    assert_eq!(doc, before);
}

#[test]
fn all_mvp_character_properties_and_opaque_attributes_round_trip() {
    let (mut doc, p) = with_text("abc");
    let opaque = OpaqueAttr {
        namespace: "w14".into(),
        name: "glow".into(),
        value: vec![1, 2, 3],
    };
    let attrs = CharAttrs {
        bold: Some(true),
        italic: Some(false),
        underline: Some(Underline::Wave),
        strike: Some(true),
        vertical_align: Some(VerticalAlign::Superscript),
        font: Some("Carlito".into()),
        size_half_points: Some(22),
        color: Some(Color { r: 1, g: 2, b: 3 }),
        highlight: Some(Color {
            r: 255,
            g: 255,
            b: 0,
        }),
        char_style: Some("Emphasis".into()),
        opaque: vec![opaque.clone()],
    };
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 3,
            set: attrs.clone(),
            clear: CharAttrs::default(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().attrs_at(1), attrs);
    // Clearing only the opaque key keeps the typed properties.
    let mask = CharAttrs {
        opaque: vec![opaque],
        ..CharAttrs::default()
    };
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: p,
            start: 0,
            end: 3,
            set: CharAttrs::default(),
            clear: mask,
        },
    );
    let left = doc.paragraph(p).unwrap().attrs_at(0);
    assert!(left.opaque.is_empty());
    assert_eq!(left.font.as_deref(), Some("Carlito"));
    // Invalid values are refused.
    let bad = CharAttrs {
        size_half_points: Some(0),
        ..CharAttrs::default()
    };
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::SetCharAttrs {
                para: p,
                start: 0,
                end: 1,
                set: bad,
                clear: CharAttrs::default()
            }
        ),
        RichError::InvalidAttribute(_)
    ));
}

#[test]
fn paragraph_properties_set_clear_and_validate() {
    let (mut doc, p) = with_text("x");
    let props = ParaProps {
        alignment: Some(Alignment::Justify),
        indent_left: Some(720),
        indent_right: Some(0),
        indent_first_line: Some(-360),
        space_before: Some(120),
        space_after: Some(240),
        line_spacing: Some(LineSpacing {
            rule: LineRule::Auto,
            value: 276,
        }),
        style: Some("Heading1".into()),
        list: Some(ListRef {
            list_id: 3,
            level: 2,
        }),
        opaque: vec![OpaqueAttr {
            namespace: "w".into(),
            name: "keepNext".into(),
            value: vec![],
        }],
        shading: None,
        borders: Default::default(),
    };
    run_checked(
        &mut doc,
        RichOp::SetParaProps {
            para: p,
            set: props.clone(),
            clear: ParaProps::default(),
        },
    );
    assert_eq!(doc.paragraph(p).unwrap().props, props);
    let mask = ParaProps {
        style: Some(String::new()),
        list: Some(ListRef {
            list_id: 0,
            level: 0,
        }),
        ..ParaProps::default()
    };
    run_checked(
        &mut doc,
        RichOp::SetParaProps {
            para: p,
            set: ParaProps::default(),
            clear: mask,
        },
    );
    let now = &doc.paragraph(p).unwrap().props;
    assert!(now.style.is_none() && now.list.is_none() && now.alignment.is_some());
    let bad = ParaProps {
        list: Some(ListRef {
            list_id: 1,
            level: 9,
        }),
        ..ParaProps::default()
    };
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::SetParaProps {
                para: p,
                set: bad,
                clear: ParaProps::default()
            }
        ),
        RichError::InvalidAttribute(_)
    ));
}

// ----- split / merge ---------------------------------------------------------

#[test]
fn split_at_start_middle_end_then_merge_back() {
    for offset in [0, 3, 6] {
        let (mut doc, p) = with_text("abcdef");
        run_checked(
            &mut doc,
            RichOp::SetCharAttrs {
                para: p,
                start: 1,
                end: 5,
                set: bold(),
                clear: CharAttrs::default(),
            },
        );
        let section = first_section(&doc);
        let new_id = doc.alloc_id().unwrap();
        run_checked(
            &mut doc,
            RichOp::SplitParagraph {
                parent: Some(section),
                position: 0,
                offset,
                new_id,
            },
        );
        let ids = doc.block_ids(Some(section)).unwrap();
        assert_eq!(ids, vec![p, new_id]);
        assert_eq!(doc.paragraph(p).unwrap().text, "abcdef"[..offset]);
        assert_eq!(doc.paragraph(new_id).unwrap().text, "abcdef"[offset..]);
        // Merge restores the text and the runs (same attributes both sides).
        let mut merged = doc.clone();
        merged
            .apply(&tx(vec![RichOp::MergeParagraphs {
                parent: Some(section),
                position: 0,
            }]))
            .unwrap();
        merged.validate().unwrap();
        assert_eq!(merged.paragraph(p).unwrap().text, "abcdef");
        assert_eq!(merged.paragraph(p).unwrap().runs.len(), 1);
    }
}

#[test]
fn merge_paragraphs_with_different_attributes_keeps_runs_and_first_properties() {
    let (mut doc, a) = with_text("ab");
    let section = first_section(&doc);
    let b_sub = doc.new_paragraph("cd").unwrap();
    let b = b_sub.root;
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: b_sub,
        },
    );
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: a,
            start: 0,
            end: 2,
            set: bold(),
            clear: CharAttrs::default(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::SetCharAttrs {
            para: b,
            start: 0,
            end: 2,
            set: italic(),
            clear: CharAttrs::default(),
        },
    );
    let left = ParaProps {
        alignment: Some(Alignment::Center),
        ..ParaProps::default()
    };
    let right = ParaProps {
        alignment: Some(Alignment::Right),
        ..ParaProps::default()
    };
    run_checked(
        &mut doc,
        RichOp::SetParaProps {
            para: a,
            set: left,
            clear: ParaProps::default(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::SetParaProps {
            para: b,
            set: right,
            clear: ParaProps::default(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::MergeParagraphs {
            parent: Some(section),
            position: 0,
        },
    );
    let merged = doc.paragraph(a).unwrap();
    assert_eq!(merged.text, "abcd");
    assert_eq!(merged.runs.len(), 2);
    assert_eq!(merged.attrs_at(0), bold());
    assert_eq!(merged.attrs_at(3), italic());
    assert_eq!(merged.props.alignment, Some(Alignment::Center));
    assert!(doc.node(b).is_none(), "second paragraph is gone");
}

#[test]
fn split_inside_a_field_is_refused_and_straddling_links_are_truncated() {
    let (mut doc, p) = with_text("abcdef");
    let section = first_section(&doc);
    let field = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Field(Field {
                id: field,
                start: 1,
                end: 4,
                instruction: "PAGE".into(),
            }),
        },
    );
    let new_id = doc.alloc_id().unwrap();
    assert_eq!(
        assert_rejected(
            &mut doc,
            RichOp::SplitParagraph {
                parent: Some(section),
                position: 0,
                offset: 2,
                new_id
            }
        ),
        RichError::SplitInsideField
    );
    let mut doc = with_text("abcdef").0;
    let p = first_para(&doc);
    let section = first_section(&doc);
    let link = doc.alloc_id().unwrap();
    let mark = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Link(Link {
                id: link,
                start: 1,
                end: 5,
                target: LinkTarget::External("https://example.org".into()),
                tooltip: None,
            }),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Bookmark(Bookmark {
                id: mark,
                name: "b".into(),
                start: 4,
                end: 6,
            }),
        },
    );
    let new_id = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::SplitParagraph {
            parent: Some(section),
            position: 0,
            offset: 3,
            new_id,
        },
    );
    let head = doc.paragraph(p).unwrap();
    assert_eq!((head.links[0].start, head.links[0].end), (1, 3));
    let tail = doc.paragraph(new_id).unwrap();
    assert_eq!((tail.bookmarks[0].start, tail.bookmarks[0].end), (1, 3));
    assert_eq!(doc.owner_of_inline(mark), Some(new_id));
}

// ----- blocks ----------------------------------------------------------------

#[test]
fn insert_and_remove_blocks_including_opaque_and_sections() {
    let (mut doc, _) = with_text("x");
    let section = first_section(&doc);
    let opaque = doc.new_opaque("w:sdt", &[9, 9, 9]).unwrap();
    let oid = opaque.root;
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: opaque,
        },
    );
    let table = add_table(&mut doc, 2, 2);
    assert_eq!(doc.block_ids(Some(section)).unwrap().len(), 3);
    run_checked(
        &mut doc,
        RichOp::RemoveBlock {
            parent: Some(section),
            position: 1,
        },
    );
    assert!(doc.node(oid).is_none());
    assert!(doc.node(table).is_some());
    // A second section, then removing it; the last section cannot go.
    let s2 = doc.new_section().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: None,
            position: 1,
            subtree: s2,
        },
    );
    run_checked(
        &mut doc,
        RichOp::RemoveBlock {
            parent: None,
            position: 1,
        },
    );
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::RemoveBlock {
                parent: None,
                position: 0
            }
        ),
        RichError::Invariant(_)
    ));
}

#[test]
fn block_operations_reject_misplaced_nodes_and_bad_positions() {
    let mut doc = RichDocument::new();
    let section = first_section(&doc);
    let par = doc.new_paragraph("y").unwrap();
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertBlock {
                parent: None,
                position: 0,
                subtree: par.clone()
            }
        ),
        RichError::WrongNodeKind { .. }
    ));
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertBlock {
                parent: Some(section),
                position: 9,
                subtree: par
            }
        ),
        RichError::PositionOutOfBounds { .. }
    ));
    let sec = doc.new_section().unwrap();
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertBlock {
                parent: Some(section),
                position: 0,
                subtree: sec
            }
        ),
        RichError::WrongNodeKind { .. }
    ));
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::RemoveBlock {
                parent: Some(section),
                position: 5
            }
        ),
        RichError::PositionOutOfBounds { .. }
    ));
    // Re-inserting a branch whose ids already live in the document fails.
    let existing = doc.extract(first_para(&doc)).unwrap();
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertBlock {
                parent: Some(section),
                position: 0,
                subtree: existing
            }
        ),
        RichError::DuplicateId(_)
    ));
}

#[test]
fn a_cell_keeps_at_least_one_paragraph() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 1, 1);
    let cell = cells(&doc, table)[0][0];
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::RemoveBlock {
                parent: Some(cell),
                position: 0
            }
        ),
        RichError::Invariant(_)
    ));
    // With a second paragraph the first may go.
    let extra = doc.new_paragraph("more").unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(cell),
            position: 1,
            subtree: extra,
        },
    );
    run_checked(
        &mut doc,
        RichOp::RemoveBlock {
            parent: Some(cell),
            position: 0,
        },
    );
    // A table inside a cell is allowed; an opaque block does not count as a paragraph.
    let nested = doc.new_table(1, 1).unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(cell),
            position: 0,
            subtree: nested,
        },
    );
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::RemoveBlock {
                parent: Some(cell),
                position: 1
            }
        ),
        RichError::Invariant(_)
    ));
}

#[test]
fn headers_and_footers_attach_per_section_and_detach() {
    let mut doc = RichDocument::new();
    let section = first_section(&doc);
    let header = doc.new_header_footer("Entete").unwrap();
    let hid = header.root;
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Header,
            variant: HfVariant::Default,
            subtree: header,
        },
    );
    let footer = doc.new_header_footer("Pied").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Footer,
            variant: HfVariant::First,
            subtree: footer,
        },
    );
    assert_eq!(doc.paragraph_ids().len(), 3);
    let again = doc.new_header_footer("Autre").unwrap();
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::AttachHeaderFooter {
                section,
                kind: HfKind::Header,
                variant: HfVariant::Default,
                subtree: again
            }
        ),
        RichError::Invariant(_)
    ));
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::DetachHeaderFooter {
                section,
                kind: HfKind::Header,
                variant: HfVariant::Even
            }
        ),
        RichError::Invariant(_)
    ));
    // Text edits work inside a header.
    let hp = doc.block_ids(Some(hid)).unwrap()[0];
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: hp,
            offset: 0,
            text: "> ".into(),
        },
    );
    run_checked(
        &mut doc,
        RichOp::DetachHeaderFooter {
            section,
            kind: HfKind::Header,
            variant: HfVariant::Default,
        },
    );
    assert!(doc.node(hid).is_none());
}

// ----- tables ----------------------------------------------------------------

#[test]
fn insert_and_delete_rows_and_columns() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 2, 2);
    for index in [0, 1, 3] {
        let op = doc.insert_row_op(table, index).unwrap();
        run_checked(&mut doc, op);
    }
    assert_eq!(cells(&doc, table).len(), 5);
    for index in [0, 2, 3] {
        let op = doc.insert_column_op(table, index).unwrap();
        run_checked(&mut doc, op);
    }
    assert_eq!(cells(&doc, table)[0].len(), 5);
    run_checked(&mut doc, RichOp::DeleteRow { table, index: 4 });
    run_checked(&mut doc, RichOp::DeleteColumn { table, index: 0 });
    let oob = doc.insert_row_op(table, 99);
    assert!(matches!(oob, Ok(_) | Err(_)));
    assert!(matches!(
        assert_rejected(&mut doc, RichOp::DeleteRow { table, index: 99 }),
        RichError::PositionOutOfBounds { .. }
    ));
    // The last row and the last column cannot be deleted.
    let mut tiny = RichDocument::new();
    let t = add_table(&mut tiny, 1, 1);
    assert!(matches!(
        assert_rejected(&mut tiny, RichOp::DeleteRow { table: t, index: 0 }),
        RichError::InvalidTable(_)
    ));
    assert!(matches!(
        assert_rejected(&mut tiny, RichOp::DeleteColumn { table: t, index: 0 }),
        RichError::InvalidTable(_)
    ));
}

#[test]
fn wrong_number_of_new_cells_is_rejected() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 2, 2);
    let (row, a, b) = (
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
    );
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertRow {
                table,
                index: 0,
                row,
                cells: vec![(a, b)]
            }
        ),
        RichError::InvalidTable(_)
    ));
}

#[test]
fn merge_and_split_cells() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 3);
    let grid = cells(&doc, table);
    // Put text in two cells that will be merged.
    for (r, text) in [(0usize, "A"), (1, "B")] {
        let p = doc.block_ids(Some(grid[r][1])).unwrap()[0];
        run_checked(
            &mut doc,
            RichOp::InsertText {
                para: p,
                offset: 0,
                text: text.into(),
            },
        );
    }
    // 2x2 merge at (0,0): cells (0,0) (0,1) (1,0) (1,1).
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 0,
            col: 0,
            rows: 2,
            cols: 2,
        },
    );
    let now = cells(&doc, table);
    assert_eq!(now[0].len(), 2);
    assert_eq!(now[1].len(), 1);
    assert_eq!(span(&doc, now[0][0]), (2, 2));
    // Non-blank content of the absorbed cells is kept (A, B), blanks are dropped.
    let blocks = doc.block_ids(Some(now[0][0])).unwrap();
    let texts: Vec<_> = blocks
        .iter()
        .map(|b| doc.paragraph(*b).unwrap().text.clone())
        .collect();
    assert_eq!(texts, ["", "A", "B"]);
    // A region cutting through the merged cell is refused; so is a 1x1 merge.
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::MergeCells {
                table,
                row: 1,
                col: 1,
                rows: 2,
                cols: 1
            }
        ),
        RichError::InvalidTable(_)
    ));
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::MergeCells {
                table,
                row: 2,
                col: 2,
                rows: 1,
                cols: 1
            }
        ),
        RichError::InvalidTable(_)
    ));
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::MergeCells {
                table,
                row: 2,
                col: 2,
                rows: 2,
                cols: 1
            }
        ),
        RichError::InvalidTable(_)
    ));
    // Split gives back a regular 3x3 grid of 1x1 cells.
    let op = doc.split_cell_op(table, now[0][0]).unwrap();
    run_checked(&mut doc, op);
    let after = cells(&doc, table);
    assert!(after.iter().all(|r| r.len() == 3));
    assert!(after.iter().flatten().all(|c| span(&doc, *c) == (1, 1)));
    let not_merged = doc.split_cell_op(table, after[2][2]).unwrap();
    assert!(matches!(
        assert_rejected(&mut doc, not_merged),
        RichError::InvalidTable(_)
    ));
}

#[test]
fn deleting_a_column_through_a_merged_cell_shrinks_it() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 4);
    // Horizontal merge of columns 1..3 on row 0, vertical merge of column 3 on rows 1..3.
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 0,
            col: 1,
            rows: 1,
            cols: 3,
        },
    );
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 1,
            col: 3,
            rows: 2,
            cols: 1,
        },
    );
    let wide = cells(&doc, table)[0][1];
    assert_eq!(span(&doc, wide), (1, 3));
    run_checked(&mut doc, RichOp::DeleteColumn { table, index: 2 });
    assert_eq!(span(&doc, wide), (1, 2));
    // Deleting the column holding the vertical merge removes that cell entirely.
    run_checked(&mut doc, RichOp::DeleteColumn { table, index: 2 });
    assert_eq!(span(&doc, wide), (1, 1));
    let grid = cells(&doc, table);
    assert_eq!(grid[1].len(), 2);
    // Deleting the first column keeps a rectangular grid too.
    run_checked(&mut doc, RichOp::DeleteColumn { table, index: 0 });
    doc.validate().unwrap();
}

#[test]
fn deleting_and_inserting_rows_through_vertical_merges() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 4, 2);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 1,
            col: 0,
            rows: 3,
            cols: 1,
        },
    );
    let tall = cells(&doc, table)[1][0];
    // Deleting a covered row shrinks the span.
    run_checked(&mut doc, RichOp::DeleteRow { table, index: 2 });
    assert_eq!(span(&doc, tall), (2, 1));
    // Deleting the anchor row moves the cell down.
    run_checked(&mut doc, RichOp::DeleteRow { table, index: 1 });
    assert_eq!(span(&doc, tall), (1, 1));
    assert_eq!(cells(&doc, table)[1][0], tall);
    // Inserting inside a span extends it and adds a single cell.
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 2);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 0,
            col: 0,
            rows: 2,
            cols: 1,
        },
    );
    let tall = cells(&doc, table)[0][0];
    let op = doc.insert_row_op(table, 1).unwrap();
    let RichOp::InsertRow {
        cells: new_cells, ..
    } = &op
    else {
        unreachable!()
    };
    assert_eq!(new_cells.len(), 1);
    run_checked(&mut doc, op);
    assert_eq!(span(&doc, tall), (3, 1));
}

// ----- inline items ----------------------------------------------------------

#[test]
fn anchored_objects_fields_bookmarks_and_links_insert_modify_remove() {
    let (mut doc, p) = with_text("hello world");
    let (obj, field, mark, link) = (
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
    );
    let items = [
        Inline::Anchored(AnchoredObject {
            id: obj,
            offset: 5,
            kind: "wp:inline".into(),
            data: vec![1, 2],
        }),
        Inline::Field(Field {
            id: field,
            start: 6,
            end: 11,
            instruction: "DATE".into(),
        }),
        Inline::Bookmark(Bookmark {
            id: mark,
            name: "intro".into(),
            start: 0,
            end: 5,
        }),
        Inline::Link(Link {
            id: link,
            start: 0,
            end: 5,
            target: LinkTarget::Bookmark("intro".into()),
            tooltip: Some("t".into()),
        }),
    ];
    for item in &items {
        run_checked(
            &mut doc,
            RichOp::InsertInline {
                para: p,
                item: item.clone(),
            },
        );
        assert_eq!(doc.owner_of_inline(item.id()), Some(p));
    }
    assert_eq!(doc.bookmark_named("intro"), Some(mark));
    // Modify each.
    let edited = [
        Inline::Anchored(AnchoredObject {
            id: obj,
            offset: 0,
            kind: "wp:anchor".into(),
            data: vec![],
        }),
        Inline::Field(Field {
            id: field,
            start: 7,
            end: 9,
            instruction: "PAGE".into(),
        }),
        Inline::Bookmark(Bookmark {
            id: mark,
            name: "renamed".into(),
            start: 1,
            end: 2,
        }),
        Inline::Link(Link {
            id: link,
            start: 1,
            end: 4,
            target: LinkTarget::External("https://a.b".into()),
            tooltip: None,
        }),
    ];
    for item in &edited {
        run_checked(
            &mut doc,
            RichOp::ModifyInline {
                para: p,
                item: item.clone(),
            },
        );
    }
    assert_eq!(doc.bookmark_named("intro"), None);
    assert_eq!(doc.bookmark_named("renamed"), Some(mark));
    for item in &edited {
        run_checked(
            &mut doc,
            RichOp::RemoveInline {
                para: p,
                id: item.id(),
            },
        );
        assert_eq!(doc.owner_of_inline(item.id()), None);
    }
}

#[test]
fn inline_items_are_validated() {
    let (mut doc, p) = with_text("abcdef");
    let id = doc.alloc_id().unwrap();
    let ok = Bookmark {
        id,
        name: "n".into(),
        start: 1,
        end: 2,
    };
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Bookmark(ok),
        },
    );
    let other = doc.alloc_id().unwrap();
    // Same name, same id, empty name, inverted range, out of bounds, overlapping links/fields.
    let attempts = [
        Inline::Bookmark(Bookmark {
            id: other,
            name: "n".into(),
            start: 0,
            end: 1,
        }),
        Inline::Bookmark(Bookmark {
            id,
            name: "m".into(),
            start: 0,
            end: 1,
        }),
        Inline::Bookmark(Bookmark {
            id: other,
            name: String::new(),
            start: 0,
            end: 1,
        }),
        Inline::Bookmark(Bookmark {
            id: other,
            name: "m".into(),
            start: 3,
            end: 2,
        }),
        Inline::Bookmark(Bookmark {
            id: other,
            name: "m".into(),
            start: 0,
            end: 99,
        }),
        Inline::Link(Link {
            id: other,
            start: 2,
            end: 2,
            target: LinkTarget::External("x".into()),
            tooltip: None,
        }),
    ];
    for item in attempts {
        assert_rejected(&mut doc, RichOp::InsertInline { para: p, item });
    }
    let f1 = doc.alloc_id().unwrap();
    let f2 = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Field(Field {
                id: f1,
                start: 0,
                end: 3,
                instruction: "A".into(),
            }),
        },
    );
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::InsertInline {
                para: p,
                item: Inline::Field(Field {
                    id: f2,
                    start: 2,
                    end: 5,
                    instruction: "B".into()
                })
            }
        ),
        RichError::Invariant(_)
    ));
    // Modify must keep the kind; removing an unknown id fails.
    assert!(matches!(
        assert_rejected(
            &mut doc,
            RichOp::ModifyInline {
                para: p,
                item: Inline::Link(Link {
                    id,
                    start: 0,
                    end: 1,
                    target: LinkTarget::External("x".into()),
                    tooltip: None
                })
            }
        ),
        RichError::WrongNodeKind { .. }
    ));
    assert!(matches!(
        assert_rejected(&mut doc, RichOp::RemoveInline { para: p, id: other }),
        RichError::NodeNotFound(_)
    ));
}

#[test]
fn text_edits_move_and_drop_inline_items() {
    let (mut doc, p) = with_text("abcdef");
    let (obj, link, mark) = (
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
    );
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Anchored(AnchoredObject {
                id: obj,
                offset: 3,
                kind: "k".into(),
                data: vec![],
            }),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Link(Link {
                id: link,
                start: 2,
                end: 4,
                target: LinkTarget::External("u".into()),
                tooltip: None,
            }),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertInline {
            para: p,
            item: Inline::Bookmark(Bookmark {
                id: mark,
                name: "m".into(),
                start: 2,
                end: 4,
            }),
        },
    );
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p,
            offset: 0,
            text: "XX".into(),
        },
    );
    let para = doc.paragraph(p).unwrap();
    assert_eq!(para.anchored[0].offset, 5);
    assert_eq!((para.links[0].start, para.links[0].end), (4, 6));
    // Deleting the link range removes the link and the object, collapses the bookmark.
    run_checked(
        &mut doc,
        RichOp::DeleteText {
            para: p,
            start: 4,
            end: 6,
        },
    );
    let para = doc.paragraph(p).unwrap();
    assert!(para.links.is_empty() && para.anchored.is_empty());
    assert_eq!((para.bookmarks[0].start, para.bookmarks[0].end), (4, 4));
    assert_eq!(doc.owner_of_inline(link), None);
}

// ----- transactions, history, selections, invariants ---------------------------

#[test]
fn failing_transaction_restores_everything() {
    let (mut doc, p) = with_text("abc");
    let before = doc.clone();
    let result = run(
        &mut doc,
        vec![
            RichOp::InsertText {
                para: p,
                offset: 3,
                text: "def".into(),
            },
            RichOp::SetCharAttrs {
                para: p,
                start: 0,
                end: 6,
                set: bold(),
                clear: CharAttrs::default(),
            },
            RichOp::DeleteText {
                para: p,
                start: 0,
                end: 100,
            },
        ],
    );
    assert!(result.is_err());
    assert_eq!(doc, before);
    doc.validate().unwrap();
}

#[test]
fn history_undo_redo_and_selection_mapping() {
    let (mut doc, p) = with_text("hello");
    let mut history: History<RichOp, Selection> = History::new(Selection::caret(p, 5), 1 << 20);
    let op = RichOp::InsertText {
        para: p,
        offset: 5,
        text: " world".into(),
    };
    let map = op.position_map(&doc).unwrap().unwrap();
    history
        .apply(
            &mut doc,
            tx(vec![op]),
            Selection::caret(p, 11),
            0,
            EditKind::Other,
            1 << 16,
        )
        .unwrap();
    assert_eq!(doc.paragraph(p).unwrap().text, "hello world");
    let tail = Selection::caret(p, 5).mapped(map).unwrap();
    assert_eq!(
        tail.primary_range().head,
        TextPosition::new(p, 11, Affinity::After)
    );
    assert!(history.undo(&mut doc).unwrap());
    assert_eq!(doc.paragraph(p).unwrap().text, "hello");
    assert!(history.redo(&mut doc).unwrap());
    assert_eq!(doc.paragraph(p).unwrap().text, "hello world");
    doc.validate().unwrap();
}

#[test]
fn split_and_merge_position_maps_follow_selections() {
    let (mut doc, p) = with_text("abcdef");
    let section = first_section(&doc);
    let new_id = doc.alloc_id().unwrap();
    let split = RichOp::SplitParagraph {
        parent: Some(section),
        position: 0,
        offset: 2,
        new_id,
    };
    let map = split.position_map(&doc).unwrap().unwrap();
    let moved = Selection::caret(p, 5).mapped(map).unwrap();
    assert_eq!(moved.primary_range().head.node, new_id);
    assert_eq!(moved.primary_range().head.offset, 3);
    run_checked(&mut doc, split);
    let merge = RichOp::MergeParagraphs {
        parent: Some(section),
        position: 0,
    };
    let map = merge.position_map(&doc).unwrap().unwrap();
    let back = Selection::caret(new_id, 3).mapped(map).unwrap();
    assert_eq!(
        (
            back.primary_range().head.node,
            back.primary_range().head.offset
        ),
        (p, 5)
    );
}

#[test]
fn restoring_an_invalid_branch_is_refused() {
    let (mut doc, p) = with_text("ab");
    let mut branch = doc.extract(p).unwrap();
    if let Some(Node::Paragraph(par)) = branch.nodes.get_mut(&p) {
        par.runs = vec![
            CharRun {
                start: 1,
                end: 2,
                attrs: bold(),
            },
            CharRun {
                start: 0,
                end: 1,
                attrs: bold(),
            },
        ];
        assert!(par.validate_local().is_err());
    }
    assert!(matches!(
        assert_rejected(&mut doc, RichOp::RestoreSubtree { subtree: branch }),
        RichError::Invariant(_)
    ));
}

#[test]
fn validate_local_detects_each_paragraph_invariant() {
    let id = RichDocument::new().alloc_id().unwrap();
    let base = Paragraph::with_text("aé");
    let run = |start, end| CharRun {
        start,
        end,
        attrs: bold(),
    };
    let cases: Vec<(&str, Paragraph)> = vec![
        (
            "empty run",
            Paragraph {
                runs: vec![run(1, 1)],
                ..base.clone()
            },
        ),
        (
            "out of bounds",
            Paragraph {
                runs: vec![run(0, 9)],
                ..base.clone()
            },
        ),
        (
            "inside a char",
            Paragraph {
                runs: vec![run(0, 2)],
                ..base.clone()
            },
        ),
        (
            "overlap",
            Paragraph {
                runs: vec![run(0, 2), run(1, 3)],
                ..base.clone()
            },
        ),
        (
            "not normalized",
            Paragraph {
                runs: vec![run(0, 1), run(1, 3)],
                ..base.clone()
            },
        ),
        (
            "anchor in char",
            Paragraph {
                anchored: vec![AnchoredObject {
                    id,
                    offset: 2,
                    kind: String::new(),
                    data: vec![],
                }],
                ..base.clone()
            },
        ),
        (
            "bookmark inverted",
            Paragraph {
                bookmarks: vec![Bookmark {
                    id,
                    name: "a".into(),
                    start: 3,
                    end: 1,
                }],
                ..base.clone()
            },
        ),
    ];
    for (name, paragraph) in cases {
        assert!(paragraph.validate_local().is_err(), "{name}");
    }
    base.validate_local().unwrap();
}

#[test]
fn rich_operations_have_no_recovery_codec_yet() {
    let (doc, p) = with_text("a");
    let op = RichOp::DeleteText {
        para: p,
        start: 0,
        end: 1,
    };
    assert!(tx(vec![op]).encode::<RichDocument>().is_err());
    drop(doc);
}

#[test]
fn extreme_arguments_are_rejected_without_panicking() {
    let (mut doc, p) = with_text("ab");
    let table = add_table(&mut doc, 2, 2);
    let section = first_section(&doc);
    let m = usize::MAX;
    let id = doc.alloc_id().unwrap();
    let ops = vec![
        RichOp::InsertText {
            para: p,
            offset: m,
            text: "x".into(),
        },
        RichOp::DeleteText {
            para: p,
            start: 0,
            end: m,
        },
        RichOp::SetCharAttrs {
            para: p,
            start: m - 1,
            end: m,
            set: bold(),
            clear: CharAttrs::default(),
        },
        RichOp::SplitParagraph {
            parent: Some(section),
            position: m,
            offset: m,
            new_id: id,
        },
        RichOp::MergeParagraphs {
            parent: Some(section),
            position: m,
        },
        RichOp::RemoveBlock {
            parent: Some(section),
            position: m,
        },
        RichOp::DeleteRow { table, index: m },
        RichOp::DeleteColumn { table, index: m },
        RichOp::MergeCells {
            table,
            row: m,
            col: m,
            rows: m,
            cols: m,
        },
        RichOp::MergeCells {
            table,
            row: 0,
            col: 0,
            rows: m,
            cols: 2,
        },
        RichOp::InsertRow {
            table,
            index: m,
            row: id,
            cells: vec![],
        },
        RichOp::InsertColumn {
            table,
            index: m,
            cells: vec![],
        },
        RichOp::RemoveInline { para: id, id },
        RichOp::DetachHeaderFooter {
            section: table,
            kind: HfKind::Header,
            variant: HfVariant::Default,
        },
    ];
    for op in ops {
        assert_rejected(&mut doc, op);
    }
}
