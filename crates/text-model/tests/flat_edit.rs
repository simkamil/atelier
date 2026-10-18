// SPDX-License-Identifier: AGPL-3.0-or-later
//! Flat-position edits of `RichDocument` (US-204): scenarios, and fixed-seed
//! random sequences that must reproduce a plain string model and come back to
//! the initial state after inversion.

mod flat_common;

use atelier_text_model::rich::flat::{
    BlockKind, EditOutcome, FlatEdit, FlatError, FlatIndex, LayoutContext, LayoutScope,
};
use atelier_text_model::rich::{CharAttrs, CharRun, ParaProps, Paragraph, RichDocument, RichOp};
use flat_common::*;

fn para_at(doc: &RichDocument, index: usize) -> &Paragraph {
    LayoutContext::paragraph(doc, index).expect("a paragraph")
}

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

/// The outcome describes the change exactly: what precedes `start` and what
/// follows the replaced interval are untouched.
fn check_outcome(before: &str, after: &RichDocument, start: usize, end: usize, o: &EditOutcome) {
    assert_eq!(o.start, start);
    assert_eq!(o.removed_bytes, end - start);
    assert_eq!(o.doc_len, after.len());
    assert_eq!(o.doc_len, before.len() - o.removed_bytes + o.inserted_bytes);
    assert_eq!(after.text(0, start).unwrap(), before[..start]);
    assert_eq!(
        after.text(start + o.inserted_bytes, after.len()).unwrap(),
        before[end..]
    );
    assert_eq!(o.layout_scope, LayoutScope::Paragraphs);
    assert_eq!(o.revision, after.revision());
    assert_eq!(o.can_undo, after.can_undo());
    assert_eq!(o.can_redo, after.can_redo());
    assert_eq!(o.first_paragraph, {
        // Paragraph holding `start` before the edit.
        let mut count = 0;
        let mut paragraph = 0;
        for (i, line) in before.split('\n').enumerate() {
            if start <= count + line.len() {
                paragraph = i;
                break;
            }
            count += line.len() + 1;
        }
        paragraph
    });
}

#[test]
fn typing_inserts_and_enter_splits_with_the_same_properties() {
    let mut doc = body(&["hello world"]);
    let first = doc.paragraph_ids()[0];
    apply(
        &mut doc,
        vec![RichOp::SetParaProps {
            para: first,
            set: ParaProps {
                style: Some("Heading1".into()),
                space_after: Some(120),
                ..ParaProps::default()
            },
            clear: ParaProps::default(),
        }],
    );
    let o = doc.replace_range(5, 5, ",").unwrap();
    assert_eq!(flat(&doc), "hello, world");
    assert_eq!(
        (o.removed_bytes, o.inserted_bytes, o.selection),
        (0, 1, (6, 6))
    );
    assert_eq!(
        (
            o.first_paragraph,
            o.removed_paragraphs,
            o.inserted_paragraphs
        ),
        (0, 1, 1)
    );
    let o = doc.replace_range(6, 7, "\n").unwrap();
    assert_eq!(flat(&doc), "hello,\nworld");
    assert_eq!((o.removed_paragraphs, o.inserted_paragraphs), (1, 2));
    assert_eq!(doc.paragraph_count(), 2);
    assert_eq!(para_at(&doc, 0).props, para_at(&doc, 1).props);
    assert_eq!(para_at(&doc, 1).props.style.as_deref(), Some("Heading1"));
    assert_eq!(doc.locate(7).unwrap().paragraph, 1);
    doc.validate().unwrap();
    // Deleting the separator merges, the first paragraph keeping its id.
    let o = doc.replace_range(6, 7, "").unwrap();
    assert_eq!(flat(&doc), "hello,world");
    assert_eq!((o.removed_paragraphs, o.inserted_paragraphs), (2, 1));
    assert_eq!(doc.paragraph_ids()[0], first);
    doc.validate().unwrap();
}

#[test]
fn text_with_several_newlines_and_inherited_attributes() {
    let mut doc = body(&["ab"]);
    let first = doc.paragraph_ids()[0];
    apply(
        &mut doc,
        vec![RichOp::SetCharAttrs {
            para: first,
            start: 0,
            end: 2,
            set: bold(),
            clear: CharAttrs::default(),
        }],
    );
    let o = doc.replace_range(1, 1, "x\ny\nz").unwrap();
    assert_eq!(flat(&doc), "ax\ny\nzb");
    assert_eq!((o.removed_paragraphs, o.inserted_paragraphs), (1, 3));
    for i in 0..3 {
        let p = para_at(&doc, i);
        assert!(!p.runs.is_empty(), "paragraph {i} keeps the bold attribute");
        assert_eq!(p.runs[0].attrs.bold, Some(true));
        assert_eq!(p.runs[0].start, 0);
        assert_eq!(p.runs[0].end, p.text.len());
    }
    doc.undo().unwrap();
    assert_eq!(flat(&doc), "ab");
    assert_eq!(doc.paragraph_count(), 1);
    doc.validate().unwrap();
}

fn bold() -> CharAttrs {
    CharAttrs {
        bold: Some(true),
        ..CharAttrs::default()
    }
}

#[test]
fn a_range_over_paragraphs_merges_and_removes_the_blocks_between() {
    let mut doc = body(&["one", "two", "three", "four"]);
    let before = flat(&doc);
    let o = doc.replace_range(2, 15, "X").unwrap();
    assert_eq!(flat(&doc), "onXour");
    check_outcome(&before, &doc, 2, 15, &o);
    assert_eq!((o.removed_paragraphs, o.inserted_paragraphs), (4, 1));
    doc.validate().unwrap();
    doc.undo().unwrap();
    assert_eq!(flat(&doc), before);
    assert_eq!(doc.paragraph_count(), 4);
    doc.redo().unwrap();
    assert_eq!(flat(&doc), "onXour");
}

#[test]
fn objects_are_deleted_with_their_range_and_survive_outside_it() {
    use atelier_text_model::rich::{AnchoredObject, Inline, InlineBreak, InlineSymbol};
    let mut doc = body(&["abcd"]);
    let para = doc.paragraph_ids()[0];
    let (a, b, c) = (
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
    );
    apply(
        &mut doc,
        vec![
            RichOp::InsertInline {
                para,
                item: Inline::Anchored(AnchoredObject {
                    id: a,
                    offset: 1,
                    kind: "pic".into(),
                    data: vec![1],
                }),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Break(InlineBreak {
                    id: b,
                    offset: 2,
                    kind: atelier_text_model::rich::BreakKind::Page,
                }),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Symbol(InlineSymbol {
                    id: c,
                    offset: 4,
                    font: "Symbol".into(),
                    code: 0xf0b7,
                }),
            },
        ],
    );
    // a U+FFFC b U+FFFC c d U+FFFC  (flat: 4 + 9 bytes)
    assert_eq!(doc.len(), 13);
    assert_eq!(flat(&doc), "a\u{fffc}b\u{fffc}cd\u{fffc}");
    // Deleting just after the first object keeps it; before it deletes it.
    let before = flat(&doc);
    doc.replace_range(4, 5, "").unwrap();
    assert_eq!(flat(&doc), "a\u{fffc}\u{fffc}cd\u{fffc}");
    assert_eq!(para_at(&doc, 0).anchored.len(), 1);
    doc.undo().unwrap();
    assert_eq!(flat(&doc), before);
    doc.replace_range(1, 4, "").unwrap();
    assert_eq!(flat(&doc), "ab\u{fffc}cd\u{fffc}");
    assert!(para_at(&doc, 0).anchored.is_empty());
    assert_eq!(para_at(&doc, 0).breaks.len(), 1);
    doc.undo().unwrap();
    // Replacing across both breaks and objects.
    doc.replace_range(1, 10, "-").unwrap();
    assert_eq!(flat(&doc), "a-\u{fffc}");
    assert_eq!(para_at(&doc, 0).symbols.len(), 1);
    assert!(para_at(&doc, 0).breaks.is_empty());
    doc.validate().unwrap();
    // Typing between an object and the text after it.
    doc.undo().unwrap();
    let before = flat(&doc);
    doc.replace_range(4, 4, "Q").unwrap();
    assert_eq!(flat(&doc), "a\u{fffc}Qb\u{fffc}cd\u{fffc}");
    doc.undo().unwrap();
    assert_eq!(flat(&doc), before);
    doc.validate().unwrap();
}

#[test]
fn typing_between_two_items_at_one_offset_splits_them() {
    use atelier_text_model::rich::{AnchoredObject, Inline, InlineSymbol};
    let mut doc = body(&["xy"]);
    let para = doc.paragraph_ids()[0];
    let (a, b) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
    apply(
        &mut doc,
        vec![
            RichOp::InsertInline {
                para,
                item: Inline::Anchored(AnchoredObject {
                    id: a,
                    offset: 1,
                    kind: "pic".into(),
                    data: vec![],
                }),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Symbol(InlineSymbol {
                    id: b,
                    offset: 1,
                    font: "Symbol".into(),
                    code: 0xf0b7,
                }),
            },
        ],
    );
    assert_eq!(flat(&doc), "x\u{fffc}\u{fffc}y");
    doc.replace_range(4, 4, "Q").unwrap();
    assert_eq!(flat(&doc), "x\u{fffc}Q\u{fffc}y");
    let p = para_at(&doc, 0);
    assert_eq!((p.anchored[0].offset, p.symbols[0].offset), (1, 2));
    doc.validate().unwrap();
    doc.undo().unwrap();
    let p = para_at(&doc, 0);
    assert_eq!((p.anchored[0].offset, p.symbols[0].offset), (1, 1));
    // Splitting between them puts each in its own paragraph.
    doc.replace_range(4, 4, "\n").unwrap();
    assert_eq!(flat(&doc), "x\u{fffc}\n\u{fffc}y");
    assert_eq!(para_at(&doc, 0).anchored.len(), 1);
    assert_eq!(para_at(&doc, 1).symbols.len(), 1);
    doc.validate().unwrap();
}

#[test]
fn a_mark_after_an_object_joins_nothing_and_keeps_the_model_valid() {
    use atelier_text_model::rich::{AnchoredObject, Inline};
    let mut doc = body(&["e"]);
    let para = doc.paragraph_ids()[0];
    let id = doc.alloc_id().unwrap();
    apply(
        &mut doc,
        vec![RichOp::InsertInline {
            para,
            item: Inline::Anchored(AnchoredObject {
                id,
                offset: 1,
                kind: "pic".into(),
                data: vec![],
            }),
        }],
    );
    // "e" U+FFFC: type a combining acute right after the object.
    let o = doc.replace_range(4, 4, "\u{301}").unwrap();
    assert_eq!(flat(&doc), "e\u{fffc}\u{301}");
    assert_eq!(o.selection, (6, 6));
    doc.validate().unwrap();
    assert_eq!(doc.check_position(4), Ok(()));
    assert_eq!(doc.check_position(5), Err(FlatError::NotBoundary));
    assert_eq!(doc.check_position(6), Ok(()));
    // Deleting the object joins "e" and the mark into one cluster; the
    // outcome position may land inside it and `snap_back` repairs it.
    let o = doc.replace_range(1, 4, "").unwrap();
    assert_eq!(flat(&doc), "e\u{301}");
    let mapped = o.map_position(4);
    assert_eq!(doc.snap_back(mapped), Ok(0));
    doc.validate().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(flat(&doc), "e\u{fffc}");
}

#[test]
fn rejected_edits_leave_the_document_and_history_untouched() {
    let mut doc = body(&["ab\u{301}c", "d"]);
    doc.replace_range(0, 0, "x").unwrap();
    let snapshot = doc.clone();
    let revision = doc.revision();
    let len = doc.len();
    assert_eq!(doc.replace_range(3, 2, ""), Err(FlatError::InvalidRange));
    assert_eq!(
        doc.replace_range(0, len + 1, ""),
        Err(FlatError::OutOfRange)
    );
    assert_eq!(
        doc.replace_range(len + 2, len + 3, ""),
        Err(FlatError::OutOfRange)
    );
    // Inside "b" + combining acute.
    assert_eq!(doc.replace_range(3, 3, "z"), Err(FlatError::NotBoundary));
    assert_eq!(doc.replace_range(0, 3, "z"), Err(FlatError::NotBoundary));
    for bad in ["\u{fffc}", "a\rb", "a\u{0}b", "\u{1b}", "x\u{7}"] {
        assert_eq!(
            doc.replace_range(0, 0, bad),
            Err(FlatError::InvalidText),
            "{bad:?}"
        );
    }
    // DEL and C1 controls are accepted.
    doc.replace_range(0, 0, "\u{7f}\u{85}\t").unwrap();
    doc.undo().unwrap();
    assert_eq!(doc, snapshot);
    assert_eq!(doc.revision(), revision + 2);
    assert!(doc.can_undo());
    // An empty replacement of an empty range is not an edit.
    let o = doc.replace_range(1, 1, "").unwrap();
    assert_eq!(
        (o.removed_bytes, o.inserted_bytes, o.revision),
        (0, 0, doc.revision())
    );
    assert!(doc.can_redo());
}

#[test]
fn undo_redo_report_the_inverse_outcome_and_clear_redo_on_a_new_edit() {
    let mut doc = body(&["alpha", "beta"]);
    let initial = doc.clone();
    let before = flat(&doc);
    let o1 = doc.replace_range(3, 8, "\n--\n").unwrap();
    let after = flat(&doc);
    assert_ne!(after, before);
    let u = doc.undo().unwrap();
    assert_eq!(flat(&doc), before);
    assert_eq!(doc, initial);
    // Same interval, roles swapped.
    assert_eq!(
        (u.start, u.removed_bytes, u.inserted_bytes),
        (o1.start, o1.inserted_bytes, o1.removed_bytes)
    );
    assert_eq!(
        (u.removed_paragraphs, u.inserted_paragraphs),
        (o1.inserted_paragraphs, o1.removed_paragraphs)
    );
    assert_eq!(u.revision, o1.revision + 1);
    assert!(!u.can_undo && u.can_redo);
    assert_eq!(u.selection, (3, 8));
    check_outcome(&after, &doc, u.start, u.start + u.removed_bytes, &u);
    let r = doc.redo().unwrap();
    assert_eq!(flat(&doc), after);
    assert_eq!(r.selection, o1.selection);
    assert_eq!(r.revision, o1.revision + 2);
    doc.undo().unwrap();
    doc.replace_range(0, 0, "!").unwrap();
    assert!(!doc.can_redo());
    assert!(doc.redo().is_none());
    doc.undo().unwrap();
    assert!(doc.undo().is_none());
    assert_eq!(doc, initial);
}

#[test]
fn edits_in_table_cells_and_across_structure() {
    let mut rng = Rng::new(5);
    let mut doc = random_structured(&mut rng, 3);
    let initial = doc.clone();
    let count = doc.paragraph_count();
    let cell_paragraphs: Vec<usize> = (0..count)
        .filter(|i| matches!(doc.block_kind(*i), Ok(BlockKind::TableCell { .. })))
        .collect();
    assert!(!cell_paragraphs.is_empty());
    // Type into the first cell paragraph.
    let first_cell = cell_paragraphs[0];
    let at = doc.paragraph_start(first_cell).unwrap();
    let before = flat(&doc);
    let o = doc.replace_range(at, at, "A\nB").unwrap();
    check_outcome(&before, &doc, at, at, &o);
    assert_eq!(doc.paragraph_count(), count + 1);
    assert!(matches!(
        doc.block_kind(first_cell + 1),
        Ok(BlockKind::TableCell { .. })
    ));
    doc.validate().unwrap();
    // A range over everything clears the text and keeps the structure when
    // it crosses containers (or merges what shares one).
    let len = doc.len();
    let all = flat(&doc);
    let valid_start = 0;
    let valid_end = len;
    if doc.check_position(valid_end).is_ok() {
        let o = doc.replace_range(valid_start, valid_end, "").unwrap();
        check_outcome(&all, &doc, valid_start, valid_end, &o);
        doc.validate().unwrap();
    }
    while doc.undo().is_some() {}
    assert_eq!(doc, initial);
}

#[test]
fn text_at_an_opaque_block_becomes_paragraphs_around_it() {
    let mut rng = Rng::new(9);
    let mut doc = random_structured(&mut rng, 2);
    let initial = doc.clone();
    let opaque = (0..doc.paragraph_count())
        .find(|i| doc.block_kind(*i) == Ok(BlockKind::Opaque))
        .unwrap();
    let start = doc.paragraph_start(opaque).unwrap();
    let before = flat(&doc);
    let o = doc.replace_range(start, start, "pre").unwrap();
    assert_eq!(doc.len(), before.len() + 4);
    assert_eq!(doc.text(start, start + 3).unwrap(), "pre");
    assert_eq!(o.selection, (start + 3, start + 3));
    assert_eq!(doc.block_kind(opaque + 1), Ok(BlockKind::Opaque));
    doc.validate().unwrap();
    let after_opaque = doc.paragraph_start(opaque + 1).unwrap() + 3;
    let o = doc
        .replace_range(after_opaque, after_opaque, "x\ny")
        .unwrap();
    assert_eq!(o.selection, (after_opaque + 4, after_opaque + 4));
    doc.validate().unwrap();
    // Deleting the object itself changes nothing.
    let opaque_start = doc.paragraph_start(opaque + 1).unwrap();
    let revision = doc.revision();
    let o = doc
        .replace_range(opaque_start, opaque_start + 3, "")
        .unwrap();
    assert_eq!(
        (o.removed_bytes, o.inserted_bytes, doc.revision()),
        (0, 0, revision)
    );
    while doc.undo().is_some() {}
    assert_eq!(doc, initial);
}

/// A random flat edit on the document: a valid range and a text.
fn random_edit(doc: &RichDocument, rng: &mut Rng) -> (usize, usize, String) {
    let len = doc.len();
    let valid: Vec<usize> = (0..=len)
        .filter(|p| doc.check_position(*p).is_ok())
        .collect();
    let a = valid[rng.below(valid.len())];
    let b = if rng.chance(3) {
        a
    } else {
        // A nearby end more often than a far one.
        let far = rng.chance(4);
        let candidates: Vec<usize> = valid
            .iter()
            .copied()
            .filter(|p| *p >= a && (far || *p <= a + 12))
            .collect();
        candidates[rng.below(candidates.len())]
    };
    let text = if rng.chance(5) {
        String::new()
    } else {
        random_text(rng, EDIT_PIECES, 5)
    };
    (a, b, text)
}

fn assert_same_index(doc: &RichDocument) {
    // The incremental index equals one rebuilt from the tree.
    let fresh = doc.clone();
    assert_eq!(doc.len(), fresh.len());
    assert_eq!(doc.paragraph_count(), fresh.paragraph_count());
    for i in 0..doc.paragraph_count() {
        assert_eq!(doc.paragraph_start(i), fresh.paragraph_start(i));
        assert_eq!(doc.block_kind(i), fresh.block_kind(i));
        assert_eq!(
            LayoutContext::paragraph(doc, i),
            LayoutContext::paragraph(&fresh, i)
        );
        assert_eq!(
            doc.section_of(i).map(|s| s.0),
            fresh.section_of(i).map(|s| s.0)
        );
    }
    assert_eq!(doc.text(0, doc.len()), fresh.text(0, fresh.len()));
}

/// Plays `steps` edits and checks the string model, the outcomes and the
/// index after each one, then undoes everything (comparing every
/// intermediate state) and redoes everything.
fn play(mut doc: RichDocument, rng: &mut Rng, steps: usize, string_model: bool) {
    let initial = doc.clone();
    let mut states = vec![doc.clone()];
    let mut model = flat(&doc);
    for _ in 0..steps {
        let (start, end, text) = random_edit(&doc, rng);
        let before = flat(&doc);
        let revision = doc.revision();
        let count = doc.paragraph_count();
        let outcome = doc.replace_range(start, end, &text).unwrap();
        doc.validate().unwrap();
        if doc.revision() == revision {
            // An edit that leaves the document identical is not applied.
            assert_eq!((outcome.removed_bytes, outcome.inserted_bytes), (0, 0));
            assert_eq!(flat(&doc), before);
            continue;
        }
        check_outcome(&before, &doc, start, end, &outcome);
        assert_eq!(
            doc.paragraph_count() + outcome.removed_paragraphs,
            count + outcome.inserted_paragraphs
        );
        assert_eq!(doc.revision(), revision + 1);
        if string_model {
            model.replace_range(start..end, &text);
            assert_eq!(
                flat(&doc),
                model,
                "string model after {start}..{end} {text:?}"
            );
            assert_eq!(outcome.inserted_bytes, text.len());
        }
        assert_same_index(&doc);
        states.push(doc.clone());
        if rng.chance(4) && states.len() > 1 {
            // Undo then redo in the middle of the sequence.
            doc.undo().unwrap();
            assert_eq!(&doc, &states[states.len() - 2]);
            doc.redo().unwrap();
            assert_eq!(&doc, states.last().unwrap());
            doc.validate().unwrap();
            assert_same_index(&doc);
        }
    }
    let last = doc.clone();
    for expected in states.iter().rev().skip(1) {
        let outcome = doc.undo().unwrap();
        assert_eq!(outcome.doc_len, doc.len());
        doc.validate().unwrap();
        assert_eq!(&doc, expected);
        assert_same_index(&doc);
    }
    assert!(doc.undo().is_none());
    assert_eq!(doc, initial);
    for expected in states.iter().skip(1) {
        doc.redo().unwrap();
        assert_eq!(&doc, expected);
    }
    assert_eq!(doc, last);
    assert!(doc.redo().is_none());
    while doc.undo().is_some() {}
    assert_eq!(doc, initial);
}

#[test]
fn random_body_edits_follow_a_string_model_and_invert() {
    for seed in 0..400 {
        let mut rng = Rng::new(seed);
        let count = 1 + rng.below(4);
        let doc = random_body(&mut rng, count, seed % 2 == 0);
        // With objects, U+FFFC bytes are in the model string too.
        play(doc, &mut rng, 6, true);
    }
}

#[test]
fn random_structured_edits_keep_the_model_valid_and_invert() {
    for seed in 0..300 {
        let mut rng = Rng::new(50_000 + seed);
        let count = 1 + rng.below(3);
        let doc = random_structured(&mut rng, count);
        play(doc, &mut rng, 6, false);
    }
}

/// Acceptance criterion of US-204: 10 000 random edit sequences (fixed seeds)
/// return to the initial state after inversion.
#[test]
fn ten_thousand_sequences_return_to_the_initial_state() {
    let mut node_total = 0;
    for seed in 0..10_000u64 {
        let mut rng = Rng::new(1_000_000 + seed);
        let count = 1 + rng.below(4);
        let mut doc = if seed % 4 == 0 {
            random_structured(&mut rng, count.min(3))
        } else {
            random_body(&mut rng, count, seed % 2 == 1)
        };
        let initial = doc.clone();
        let steps = 2 + rng.below(6);
        let mut applied = 0;
        for _ in 0..steps {
            let (start, end, text) = random_edit(&doc, &mut rng);
            let before = doc.revision();
            doc.replace_range(start, end, &text).unwrap();
            if doc.revision() != before {
                applied += 1;
            }
        }
        doc.validate().unwrap();
        let after = doc.clone();
        for _ in 0..applied {
            doc.undo().unwrap();
        }
        assert!(doc.undo().is_none(), "seed {seed}");
        assert_eq!(doc, initial, "seed {seed}");
        for _ in 0..applied {
            doc.redo().unwrap();
        }
        assert_eq!(doc, after, "redo, seed {seed}");
        for _ in 0..applied {
            doc.undo().unwrap();
        }
        assert_eq!(doc, initial, "seed {seed}");
        node_total += node_kinds(&doc).0;
    }
    assert!(node_total > 10_000);
}

#[test]
fn history_is_dropped_by_other_operations_and_clones() {
    let mut doc = body(&["abc"]);
    doc.replace_range(1, 1, "x").unwrap();
    assert!(doc.can_undo());
    assert!(!doc.clone().can_undo());
    let para = doc.paragraph_ids()[0];
    apply(
        &mut doc,
        vec![RichOp::SetCharAttrs {
            para,
            start: 0,
            end: 1,
            set: bold(),
            clear: CharAttrs::default(),
        }],
    );
    assert!(!doc.can_undo());
    // The index follows: an edit still works.
    doc.replace_range(0, 0, "y").unwrap();
    assert_eq!(flat(&doc), "yaxbc");
}

#[test]
fn char_style_runs_survive_edits_and_clearing() {
    let mut doc = body(&["abcd"]);
    let para = doc.paragraph_ids()[0];
    let styled = CharAttrs {
        char_style: Some("Emphasis".into()),
        ..CharAttrs::default()
    };
    apply(
        &mut doc,
        vec![RichOp::SetCharAttrs {
            para,
            start: 1,
            end: 3,
            set: styled.clone(),
            clear: CharAttrs::default(),
        }],
    );
    let before = doc.clone();
    doc.replace_range(2, 2, "ZZ").unwrap();
    let p = para_at(&doc, 0);
    assert_eq!(
        p.runs,
        vec![CharRun {
            start: 1,
            end: 5,
            attrs: styled.clone()
        }]
    );
    // The style costs no byte in the flat text.
    assert_eq!(doc.len(), 6);
    doc.undo().unwrap();
    assert_eq!(doc, before);
    // Clearing it through a mask works.
    apply(
        &mut doc,
        vec![RichOp::SetCharAttrs {
            para,
            start: 1,
            end: 3,
            set: CharAttrs::default(),
            clear: styled,
        }],
    );
    assert!(para_at(&doc, 0).runs.is_empty());
}

#[test]
fn the_revision_is_never_given_twice() {
    use atelier_model::Transaction;
    let mut doc = body(&["abc"]);
    doc.replace_range(0, 0, "x").unwrap();
    doc.replace_range(0, 0, "y").unwrap();
    assert_eq!(doc.revision(), 2);
    // A clone continues the count, with no history.
    let mut copy = doc.clone();
    assert_eq!(copy.revision(), 2);
    assert!(!copy.can_undo());
    assert_eq!(copy.replace_range(0, 0, "z").unwrap().revision, 3);
    // A failed transaction restores the document without lowering it.
    doc.undo().unwrap();
    assert_eq!(doc.revision(), 3);
    let first = doc.paragraph_ids()[0];
    let bad = Transaction::new(vec![RichOp::DeleteText {
        para: first,
        start: 5,
        end: 99,
    }])
    .unwrap();
    assert!(doc.apply(&bad).is_err());
    assert_eq!(doc.revision(), 3);
    assert_eq!(doc.replace_range(0, 0, "w").unwrap().revision, 4);
}

/// Properties of paragraphs are those of the split paragraph on a split, those
/// of the first on a merge, and come back exactly on inversion.
#[test]
fn properties_and_runs_follow_splits_and_merges_exactly() {
    for seed in 0..300 {
        let mut rng = Rng::new(90_000 + seed);
        let count = 1 + rng.below(3);
        let mut doc = random_body(&mut rng, count, seed % 2 == 0);
        let initial = doc.clone();
        let index = rng.below(doc.paragraph_count());
        let source = para_at(&doc, index).clone();
        let start = doc.paragraph_start(index).unwrap();
        let valid: Vec<usize> = (start..=start + doc.paragraph_flat(index).unwrap().len())
            .filter(|p| doc.check_position(*p).is_ok())
            .collect();
        let at = valid[rng.below(valid.len())];
        // Split.
        doc.replace_range(at, at, "\n").unwrap();
        let head = para_at(&doc, index).clone();
        let tail = para_at(&doc, index + 1).clone();
        assert_eq!(head.props, source.props, "seed {seed}");
        assert_eq!(tail.props, source.props, "seed {seed}");
        doc.validate().unwrap();
        // The text of both halves keeps the attributes of the source.
        for (half, shift) in [(&head, 0), (&tail, head.text.len())] {
            for (position, _) in half.text.char_indices() {
                assert_eq!(
                    half.attrs_at(position),
                    source.attrs_at(position + shift),
                    "seed {seed}"
                );
            }
        }
        // Merge back: the first paragraph keeps its properties.
        let separator = doc.paragraph_start(index + 1).unwrap() - 1;
        doc.replace_range(separator, separator + 1, "").unwrap();
        assert_eq!(para_at(&doc, index).props, source.props, "seed {seed}");
        assert_eq!(para_at(&doc, index).text, source.text);
        doc.undo().unwrap();
        assert_eq!(doc.paragraph_count(), initial.paragraph_count() + 1);
        assert_eq!(para_at(&doc, index + 1).props, source.props);
        doc.undo().unwrap();
        assert_eq!(doc, initial, "seed {seed}");
        // Merging two different paragraphs keeps the first one's properties.
        if doc.paragraph_count() > 1 {
            let first = para_at(&doc, 0).clone();
            let separator = doc.paragraph_start(1).unwrap() - 1;
            doc.replace_range(separator, separator + 1, "").unwrap();
            assert_eq!(para_at(&doc, 0).props, first.props, "seed {seed}");
            doc.undo().unwrap();
            assert_eq!(doc, initial, "seed {seed}");
        }
    }
}
