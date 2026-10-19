// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the paragraph-formatting handler and of `FormatMarks` (US-243).

use super::format_tests::{Harness, para_format};
use super::{Session, io};
use crate::rich_protocol::{
    BorderEdit, BorderLine, Command, EditOp, ErrorCode, FormatMark, ParaAttr, ParaEdit, ParaFormat,
    ParaValue, Response, Rgb, Tri,
};
use atelier_model::Transaction;
use atelier_text_model::rich::{BreakKind, Inline, InlineBreak, ParaProps, RichDocument, RichOp};

fn edit(attr: ParaAttr, op: EditOp, value: Option<ParaValue>) -> ParaEdit {
    ParaEdit { attr, op, value }
}

fn format(edits: Vec<ParaEdit>) -> ParaFormat {
    ParaFormat {
        edits,
        ..para_format()
    }
}

fn apply(h: &mut Harness, edits: Vec<ParaEdit>) {
    match h.send(Command::ApplyParaFormat(format(edits))) {
        Response::State(_) => {}
        other => panic!("expected a state, got {other:?}"),
    }
}

/// Three paragraphs "one", "two", "three" (flat 0..3, 4..7, 8..13).
fn three() -> Harness {
    let mut h = Harness::new();
    h.type_text("one\ntwo\nthree");
    h
}

fn set_alignment(number: u8) -> ParaEdit {
    edit(
        ParaAttr::Alignment,
        EditOp::Set,
        Some(ParaValue::Choice(number)),
    )
}

#[test]
fn every_paragraph_the_selection_touches_is_formatted_in_one_undo_step() {
    let mut h = three();
    let revision = h.state().revision;
    h.select(1, 9);
    apply(&mut h, vec![set_alignment(2)]);
    assert_eq!(h.state().revision, revision + 1);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(2));
    // The three paragraphs, each on its own.
    for (from, to) in [(0, 0), (4, 4), (8, 8)] {
        h.select(from, to);
        assert_eq!(h.state().paragraph.alignment, Tri::Value(2), "{from}");
    }
    // One Ctrl+Z restores the three of them and the selection.
    h.select(1, 9);
    assert!(matches!(h.send(Command::Undo), Response::State(_)));
    for (from, to) in [(0, 0), (4, 4), (8, 8)] {
        h.select(from, to);
        assert_eq!(h.state().paragraph.alignment, Tri::Value(0), "{from}");
    }
}

#[test]
fn a_selection_ending_at_the_start_of_a_paragraph_does_not_touch_it() {
    let mut h = three();
    h.select(0, 4);
    apply(&mut h, vec![set_alignment(1)]);
    h.select(4, 4);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(0));
    h.select(0, 0);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(1));
    // A caret touches its own paragraph, a selection starting at the end of one touches it too.
    h.select(3, 5);
    apply(&mut h, vec![set_alignment(3)]);
    h.select(0, 0);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(3));
    h.select(4, 4);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(3));
    h.select(8, 8);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(0));
}

#[test]
fn a_mixed_selection_reports_mixed_and_a_command_unifies_it() {
    let mut h = three();
    h.select(0, 0);
    apply(&mut h, vec![set_alignment(1)]);
    h.select(0, 13);
    assert_eq!(h.state().paragraph.alignment, Tri::Mixed);
    apply(&mut h, vec![set_alignment(0)]);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(0));
}

#[test]
fn the_indent_steps_by_720_twips_from_each_paragraphs_own_value_and_stops_at_zero() {
    let mut h = three();
    h.props(
        1,
        ParaProps {
            indent_left: Some(1000),
            ..ParaProps::default()
        },
    );
    h.select(0, 13);
    assert_eq!(h.state().paragraph.indent_left, Tri::Mixed);
    apply(
        &mut h,
        vec![edit(ParaAttr::IndentLeft, EditOp::StepUp, None)],
    );
    h.select(0, 0);
    assert_eq!(h.state().paragraph.indent_left, Tri::Value(720));
    h.select(4, 4);
    assert_eq!(h.state().paragraph.indent_left, Tri::Value(1440));
    h.select(0, 0);
    apply(
        &mut h,
        vec![edit(ParaAttr::IndentLeft, EditOp::StepDown, None)],
    );
    assert_eq!(h.state().paragraph.indent_left, Tri::Value(0));
    // At zero a decrease changes nothing: same revision, no new step.
    let revision = h.state().revision;
    apply(
        &mut h,
        vec![edit(ParaAttr::IndentLeft, EditOp::StepDown, None)],
    );
    assert_eq!(h.state().revision, revision);
    h.select(4, 4);
    apply(
        &mut h,
        vec![edit(ParaAttr::IndentLeft, EditOp::StepDown, None)],
    );
    assert_eq!(h.state().paragraph.indent_left, Tri::Value(720));
}

#[test]
fn first_line_and_hanging_indents_and_right_indent_are_set_and_cleared() {
    let mut h = three();
    h.select(0, 0);
    apply(
        &mut h,
        vec![
            edit(
                ParaAttr::FirstLine,
                EditOp::Set,
                Some(ParaValue::Twips(-360)),
            ),
            edit(
                ParaAttr::IndentLeft,
                EditOp::Set,
                Some(ParaValue::Twips(720)),
            ),
            edit(
                ParaAttr::IndentRight,
                EditOp::Set,
                Some(ParaValue::Twips(144)),
            ),
        ],
    );
    let s = h.state();
    assert_eq!(s.paragraph.first_line, Tri::Value(-360));
    assert_eq!(s.paragraph.indent_left, Tri::Value(720));
    assert_eq!(s.paragraph.indent_right, Tri::Value(144));
    apply(
        &mut h,
        vec![
            edit(ParaAttr::FirstLine, EditOp::Clear, None),
            edit(ParaAttr::IndentRight, EditOp::Clear, None),
        ],
    );
    let s = h.state();
    assert_eq!(s.paragraph.first_line, Tri::Value(0));
    assert_eq!(s.paragraph.indent_right, Tri::Value(0));
    assert_eq!(s.paragraph.indent_left, Tri::Value(720));
}

#[test]
fn line_spacing_is_set_with_its_rule_and_cleared_back_to_the_style() {
    let mut h = three();
    h.select(0, 13);
    apply(
        &mut h,
        vec![edit(
            ParaAttr::LineSpacing,
            EditOp::Set,
            Some(ParaValue::Line {
                rule: 0,
                value: 360,
            }),
        )],
    );
    assert_eq!(h.state().paragraph.line_spacing, Tri::Value((0, 360)));
    apply(
        &mut h,
        vec![edit(
            ParaAttr::LineSpacing,
            EditOp::Set,
            Some(ParaValue::Line {
                rule: 2,
                value: 400,
            }),
        )],
    );
    assert_eq!(h.state().paragraph.line_spacing, Tri::Value((2, 400)));
    apply(
        &mut h,
        vec![edit(ParaAttr::LineSpacing, EditOp::Clear, None)],
    );
    assert!(matches!(
        h.state().paragraph.line_spacing,
        Tri::Value((0, _))
    ));
}

#[test]
fn the_spacing_toggle_adds_twelve_points_when_every_paragraph_has_none_else_removes() {
    let mut h = three();
    h.select(0, 13);
    let toggle = || vec![edit(ParaAttr::SpaceBefore, EditOp::Toggle, None)];
    // The default style gives no space before: add.
    assert_eq!(h.state().paragraph.space_before, Tri::Value(0));
    apply(&mut h, toggle());
    assert_eq!(h.state().paragraph.space_before, Tri::Value(240));
    apply(&mut h, toggle());
    assert_eq!(h.state().paragraph.space_before, Tri::Value(0));
    // After: the second toggle on a mixed selection removes (the client sends `set 240` to add).
    h.select(0, 0);
    apply(
        &mut h,
        vec![edit(
            ParaAttr::SpaceAfter,
            EditOp::Set,
            Some(ParaValue::Length(0)),
        )],
    );
    h.select(0, 13);
    let after = h.state().paragraph.space_after;
    apply(
        &mut h,
        vec![edit(ParaAttr::SpaceAfter, EditOp::Toggle, None)],
    );
    match after {
        Tri::Mixed => assert_eq!(h.state().paragraph.space_after, Tri::Value(0)),
        Tri::Value(0) => assert_eq!(h.state().paragraph.space_after, Tri::Value(240)),
        other => panic!("unexpected {other:?}"),
    }
}

fn border(style: u8, eighths: u16, space: u8) -> BorderEdit {
    BorderEdit::Line(BorderLine {
        style,
        eighths,
        space,
        color: None,
    })
}

/// top, left, bottom, right, between, bar.
fn borders(sides: [BorderEdit; 6]) -> ParaEdit {
    edit(
        ParaAttr::Borders,
        EditOp::Set,
        Some(ParaValue::Borders(sides)),
    )
}

fn box_of(line: BorderEdit) -> ParaEdit {
    borders([line, line, line, line, BorderEdit::Keep, BorderEdit::Keep])
}

fn yellow() -> ParaEdit {
    edit(
        ParaAttr::Shading,
        EditOp::Set,
        Some(ParaValue::Color(Rgb {
            r: 255,
            g: 255,
            b: 0,
        })),
    )
}

#[test]
fn shading_is_set_on_every_touched_paragraph_in_one_step_and_reported_mixed_or_uniform() {
    let mut h = three();
    let revision = h.state().revision;
    h.select(1, 5);
    let mut f = format(vec![yellow()]);
    f.command_id = 41;
    assert!(matches!(
        h.send(Command::ApplyParaFormat(f)),
        Response::State(_)
    ));
    assert_eq!(h.state().revision, revision + 1);
    assert_eq!(h.state().last_command_id, 41);
    let yellow_value = Tri::Value(Rgb {
        r: 255,
        g: 255,
        b: 0,
    });
    assert_eq!(h.state().paragraph.shading, yellow_value);
    h.select(8, 8);
    assert_eq!(h.state().paragraph.shading, Tri::Unset);
    h.select(0, 13);
    assert_eq!(h.state().paragraph.shading, Tri::Mixed);
    // "No colour" on the whole selection, then one Ctrl+Z brings the colours back.
    apply(&mut h, vec![edit(ParaAttr::Shading, EditOp::Clear, None)]);
    assert_eq!(h.state().paragraph.shading, Tri::Unset);
    assert!(matches!(h.send(Command::Undo), Response::State(_)));
    assert_eq!(h.state().paragraph.shading, Tri::Mixed);
    h.select(4, 4);
    assert_eq!(h.state().paragraph.shading, yellow_value);
}

#[test]
fn borders_are_set_per_side_kept_removed_and_reported_per_side() {
    let mut h = three();
    h.select(0, 13);
    apply(&mut h, vec![box_of(border(1, 12, 3))]);
    let state = h.state();
    let line = || {
        Tri::Value(BorderLine {
            style: 1,
            eighths: 12,
            space: 3,
            color: None,
        })
    };
    // top, left, bottom, right set; between and bar untouched.
    assert_eq!(
        state.paragraph.borders[..4],
        [line(), line(), line(), line()]
    );
    assert_eq!(state.paragraph.borders[4], Tri::Unset);
    // Remove the bottom side only; the others are kept.
    apply(
        &mut h,
        vec![borders([
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::None,
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
        ])],
    );
    let state = h.state();
    assert_eq!(state.paragraph.borders[0], line());
    assert_eq!(state.paragraph.borders[2], Tri::Unset);
    // Only the middle paragraph changes its top: mixed on the whole selection.
    h.select(4, 4);
    apply(
        &mut h,
        vec![borders([
            border(0, 4, 0),
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
        ])],
    );
    h.select(0, 13);
    assert_eq!(h.state().paragraph.borders[0], Tri::Mixed);
    // "No border": six `none`.
    apply(&mut h, vec![borders([BorderEdit::None; 6])]);
    assert!(h.state().paragraph.borders.iter().all(|b| *b == Tri::Unset));
    // Undo restores the previous step as a whole.
    assert!(matches!(h.send(Command::Undo), Response::State(_)));
    assert_eq!(h.state().paragraph.borders[0], Tri::Mixed);
}

/// Baseline of the paragraph marks (kind 0) of page 0, points.
fn mark_ys(h: &mut Harness) -> Vec<f64> {
    marks(h, 0, 1)
        .iter()
        .filter(|m| m.kind == 0)
        .map(|m| m.y)
        .collect()
}

#[test]
fn borders_take_room_and_consecutive_paragraphs_with_the_same_borders_share_one_box() {
    let mut h = three();
    let plain = mark_ys(&mut h);
    // A 1 pt border at 1 pt from the text takes 2 pt on the sides that have one.
    let room = 2.0;
    h.select(4, 4);
    apply(&mut h, vec![box_of(border(0, 8, 1))]);
    let boxed = mark_ys(&mut h);
    assert!((boxed[0] - plain[0]).abs() < 0.01);
    assert!((boxed[1] - plain[1] - room).abs() < 0.01, "{boxed:?}");
    assert!((boxed[2] - plain[2] - 2.0 * room).abs() < 0.01, "{boxed:?}");
    // The last two paragraphs with the same box: one top and one bottom only.
    h.select(0, 0);
    h.select(4, 13);
    apply(&mut h, vec![box_of(border(0, 8, 1))]);
    let shared = mark_ys(&mut h);
    assert!((shared[1] - plain[1] - room).abs() < 0.01, "{shared:?}");
    assert!((shared[2] - plain[2] - room).abs() < 0.01, "{shared:?}");
    // A "between" line takes room between them.
    apply(
        &mut h,
        vec![borders([
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
            BorderEdit::Keep,
            border(0, 8, 1),
            BorderEdit::Keep,
        ])],
    );
    let between = mark_ys(&mut h);
    assert!(
        (between[2] - plain[2] - 2.0 * room).abs() < 0.01,
        "{between:?}"
    );
    // Ctrl+Z gives back the geometry.
    assert!(matches!(h.send(Command::Undo), Response::State(_)));
    let undone = mark_ys(&mut h);
    assert!((undone[2] - shared[2]).abs() < 0.01);
}

#[test]
fn identifiers_guards_and_commands_without_effect_follow_the_contract() {
    let mut h = three();
    h.select(0, 0);
    let mut f = format(vec![set_alignment(1)]);
    f.command_id = 5;
    assert!(matches!(
        h.send(Command::ApplyParaFormat(f.clone())),
        Response::State(_)
    ));
    assert_eq!(h.state().last_command_id, 5);
    assert_eq!(
        h.error(Command::ApplyParaFormat(f.clone())),
        ErrorCode::DuplicateCommand
    );
    // Already centred: accepted without a revision or a step, and recorded.
    let revision = h.state().revision;
    f.command_id = 6;
    assert!(matches!(
        h.send(Command::ApplyParaFormat(f.clone())),
        Response::State(_)
    ));
    assert_eq!(h.state().revision, revision);
    assert_eq!(h.state().last_command_id, 6);
    // A stale guard changes nothing.
    f.command_id = 7;
    f.base_revision = revision + 9;
    assert_eq!(
        h.error(Command::ApplyParaFormat(f)),
        ErrorCode::StaleRevision
    );
}

#[test]
fn marks_give_spaces_tabs_no_break_spaces_and_paragraph_ends_in_reading_order() {
    let mut h = Harness::new();
    h.type_text("a b\tc\u{a0}d\ne");
    let marks = match h.send(Command::FormatMarks { first: 0, count: 1 }) {
        Response::FormatMarks { revision, marks } => {
            assert_eq!(revision, h.state().revision);
            marks
        }
        other => panic!("expected marks, got {other:?}"),
    };
    let kinds: Vec<u8> = marks.iter().map(|m| m.kind).collect();
    // space, tab, no-break space, paragraph end; paragraph end of the second paragraph.
    assert_eq!(kinds, vec![1, 2, 3, 0, 0]);
    let first_line: Vec<&FormatMark> = marks.iter().take(4).collect();
    assert!(first_line.windows(2).all(|w| w[0].x < w[1].x));
    assert!(
        first_line
            .iter()
            .all(|m| m.y == first_line[0].y && m.width >= 0.0)
    );
    // The space has the advance of a space; the tab reaches the next stop.
    assert!(marks[0].width > 0.5 && marks[1].width > marks[0].width);
    // The second paragraph is lower.
    assert!(marks[4].y > marks[3].y);
    // A selection or a formatting command changes no mark of a paragraph it does not move.
    assert!(marks.iter().all(|m| m.page == 0));
}

#[test]
fn marks_follow_the_formatting_of_the_paragraph() {
    let mut h = Harness::new();
    h.type_text("a b");
    let x_of_space = |h: &mut Harness| match h.send(Command::FormatMarks { first: 0, count: 1 }) {
        Response::FormatMarks { marks, .. } => marks[0].x,
        other => panic!("{other:?}"),
    };
    let left = x_of_space(&mut h);
    h.select(0, 0);
    apply(
        &mut h,
        vec![edit(
            ParaAttr::IndentLeft,
            EditOp::Set,
            Some(ParaValue::Twips(1440)),
        )],
    );
    assert!((x_of_space(&mut h) - left - 72.0).abs() < 0.01);
}

#[test]
fn line_and_page_breaks_are_marked_apart() {
    let mut doc = RichDocument::new();
    let para = doc.paragraph_ids()[0];
    let (line, page) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
    doc.apply(
        &Transaction::new(vec![
            RichOp::InsertText {
                para,
                offset: 0,
                text: "ab".into(),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Break(InlineBreak {
                    id: line,
                    offset: 1,
                    kind: BreakKind::Line,
                }),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Break(InlineBreak {
                    id: page,
                    offset: 2,
                    kind: BreakKind::Page,
                }),
            },
        ])
        .unwrap(),
    )
    .unwrap();
    let mut h = Harness::new();
    h.editor.active =
        Some(Session::with_document(1, doc, true, io::Origin::new(io::Source::New)).unwrap());
    let marks = match h.send(Command::FormatMarks { first: 0, count: 2 }) {
        Response::FormatMarks { marks, .. } => marks,
        other => panic!("{other:?}"),
    };
    let kinds: Vec<(u32, u8)> = marks.iter().map(|m| (m.page, m.kind)).collect();
    assert!(kinds.contains(&(0, 4)), "{kinds:?}");
    assert!(kinds.contains(&(0, 5)), "{kinds:?}");
    assert!(kinds.contains(&(1, 0)), "{kinds:?}");
}

#[test]
fn marks_refuse_a_missing_page_and_change_no_state() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let revision = h.state().revision;
    assert_eq!(
        h.error(Command::FormatMarks { first: 5, count: 1 }),
        ErrorCode::PageRange
    );
    // A count past the last page is bounded to it.
    match h.send(Command::FormatMarks {
        first: 0,
        count: 16,
    }) {
        Response::FormatMarks { marks, .. } => assert!(marks.iter().all(|m| m.page == 0)),
        other => panic!("{other:?}"),
    }
    assert_eq!(h.state().revision, revision);
}

fn marks(h: &mut Harness, first: u32, count: u32) -> Vec<FormatMark> {
    match h.send(Command::FormatMarks { first, count }) {
        Response::FormatMarks { marks, .. } => marks,
        other => panic!("expected marks, got {other:?}"),
    }
}

fn alignment_at(h: &mut Harness, position: u64) -> Tri<u8> {
    h.select(position, position);
    h.state().paragraph.alignment
}

#[test]
fn table_cells_are_formatted_and_opaque_blocks_are_skipped() {
    use atelier_text_model::rich::flat::{FlatIndex, ParagraphPosition};
    let mut h = Harness::new();
    h.type_text("first\nsecond");
    let section = h.doc().section_ids()[0];
    let opaque = h.doc().new_opaque("unknown", b"<x/>").unwrap();
    let table = h.doc().new_table(1, 2).unwrap();
    for (position, subtree) in [(1, opaque), (2, table)] {
        h.doc()
            .apply(
                &Transaction::new(vec![RichOp::InsertBlock {
                    parent: Some(section),
                    position,
                    subtree,
                }])
                .unwrap(),
            )
            .unwrap();
    }
    h.send(Command::Resync);
    let start = |h: &mut Harness, paragraph: usize| {
        h.doc()
            .position(ParagraphPosition {
                paragraph,
                offset: 0,
            })
            .unwrap() as u64
    };
    let end = h.doc().len() as u64;
    h.select(0, end);
    apply(&mut h, vec![set_alignment(2)]);
    // first, [opaque], two cells, second.
    for paragraph in [0, 2, 3, 4] {
        let at = start(&mut h, paragraph);
        assert_eq!(
            alignment_at(&mut h, at),
            Tri::Value(2),
            "paragraph {paragraph}"
        );
    }
    // The opaque block alone: accepted, nothing to change.
    let opaque_at = start(&mut h, 1);
    h.select(opaque_at, opaque_at);
    let revision = h.state().revision;
    apply(&mut h, vec![set_alignment(1)]);
    assert_eq!(h.state().revision, revision);
}

#[test]
fn formatting_is_refused_read_only_and_plain_text_and_works_after_a_resync() {
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/revisions.docx"
    ))
    .unwrap();
    h.send(Command::Open {
        kind: crate::rich_protocol::OpenKind::Docx,
        profile: crate::rich_protocol::Profile::Document,
        data: docx,
    });
    assert_eq!(
        h.error(Command::ApplyParaFormat(format(vec![set_alignment(1)]))),
        ErrorCode::ReadOnly
    );
    // Marks only read the layout: allowed.
    assert!(matches!(
        h.send(Command::FormatMarks { first: 0, count: 1 }),
        Response::FormatMarks { .. }
    ));
    h.send(Command::Open {
        kind: crate::rich_protocol::OpenKind::Txt,
        profile: crate::rich_protocol::Profile::Txt,
        data: b"plain text".to_vec(),
    });
    assert_eq!(
        h.error(Command::ApplyParaFormat(format(vec![set_alignment(1)]))),
        ErrorCode::FormatPlainText
    );
    assert!(matches!(
        h.send(Command::FormatMarks { first: 0, count: 1 }),
        Response::FormatMarks { .. }
    ));
    let mut h = three();
    h.select(0, 13);
    h.send(Command::Resync);
    apply(&mut h, vec![set_alignment(3)]);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(3));
}

#[test]
fn marks_of_a_page_that_is_not_laid_out_are_not_ready() {
    use crate::rich_layout::RichPageCache;
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let mut ops = Vec::new();
    for position in 1..4000 {
        let subtree = doc.new_paragraph("mot mot mot mot").unwrap();
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree,
        });
    }
    doc.apply(&Transaction::new(ops).unwrap()).unwrap();
    let mut h = Harness::new();
    h.editor.active =
        Some(Session::with_document(1, doc, true, io::Origin::new(io::Source::New)).unwrap());
    let (page_count, complete) = {
        let cache = &h.editor.active.as_ref().unwrap().cache;
        (cache.page_count(), cache.layout_complete())
    };
    assert!(
        !complete && page_count > 2,
        "the layout of a large document is not finished yet"
    );
    // The first page is laid out; a page the layout has not reached yet is not ready, never a partial list.
    assert!(!marks(&mut h, 0, 1).is_empty());
    let session = h.editor.active.as_ref().unwrap();
    assert_eq!(
        session
            .cache
            .format_marks(&session.doc, page_count + 40, page_count + 41, 10),
        Err(crate::rich_layout::LayoutError::NotReady)
    );
    assert_eq!(
        h.error(Command::FormatMarks {
            first: page_count,
            count: 1
        }),
        ErrorCode::PageRange
    );
}

#[test]
fn marks_of_justified_text_stretch_and_marks_of_a_later_page_use_its_own_coordinates() {
    let mut h = Harness::new();
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
    h.type_text(words);
    let left = marks(&mut h, 0, 1);
    let first_line_spaces = |m: &[FormatMark]| {
        let first = m.iter().find(|m| m.kind == 1).unwrap().y;
        m.iter()
            .filter(|m| m.kind == 1 && m.y == first)
            .cloned()
            .collect::<Vec<_>>()
    };
    let plain = first_line_spaces(&left);
    h.select(0, 0);
    apply(&mut h, vec![set_alignment(3)]);
    let justified = marks(&mut h, 0, 1);
    let stretched = first_line_spaces(&justified);
    assert!(stretched.len() >= 2);
    // The gaps between words are widened: the marks move right, evenly, and the last one nears the right margin.
    assert!(stretched.len() <= plain.len());
    let n = stretched.len();
    assert!(
        stretched[n - 1].x > plain[n - 1].x + 1.0,
        "the spaces of a justified line move right {:?} {:?}",
        plain.iter().map(|m| m.x).collect::<Vec<_>>(),
        stretched.iter().map(|m| m.x).collect::<Vec<_>>()
    );
    let steps: Vec<f64> = stretched.windows(2).map(|w| w[1].x - w[0].x).collect();
    let plain_steps: Vec<f64> = plain.windows(2).map(|w| w[1].x - w[0].x).collect();
    assert!(
        steps.iter().zip(&plain_steps).all(|(a, b)| a > b),
        "every gap is wider than unjustified"
    );
    let last = &stretched[n - 1];
    assert!(
        last.x < 595.276 - 70.866 && last.x > 595.276 - 70.866 - 80.0,
        "the last space stands near the right margin"
    );
    // A later page: its own coordinates, below the top margin and inside the page.
    let mut h = Harness::new();
    h.type_text(&vec!["ab cd"; 120].join("\n"));
    let second = marks(&mut h, 1, 1);
    assert!(!second.is_empty());
    assert!(
        second
            .iter()
            .all(|m| m.page == 1 && m.y > 70.0 && m.y < 842.0 && m.x >= 70.0)
    );
    let ends = second.iter().filter(|m| m.kind == 0).count();
    let first_page_ends = marks(&mut h, 0, 1).iter().filter(|m| m.kind == 0).count();
    assert!(first_page_ends > 20 && ends > 20);
}

/// A harness with `text` whose paragraphs from flat position `from` carry the shared box.
fn boxed_from(text: &str, from: u64) -> Harness {
    let mut h = Harness::new();
    h.type_text(text);
    h.select(from, text.len() as u64);
    apply(&mut h, vec![box_of(border(0, 8, 1))]);
    h
}

#[test]
fn the_neighbours_of_a_shared_box_are_laid_out_again_after_typing_enter_and_a_deleted_break() {
    // "one" plain, "two" and "three" share a box.
    let mut h = boxed_from(
        "one
two
three",
        4,
    );
    // Typing in the first paragraph of the box.
    h.select(7, 7);
    h.type_text("XY");
    let fresh = mark_ys(&mut boxed_from(
        "one
twoXY
three",
        4,
    ));
    assert_eq!(mark_ys(&mut h), fresh, "after typing");
    // Enter in the middle of the box: the new paragraph joins it.
    h.select(9, 9);
    h.type_text(
        "
",
    );
    let fresh = mark_ys(&mut boxed_from(
        "one
twoXY

three",
        4,
    ));
    assert_eq!(mark_ys(&mut h), fresh, "after Enter");
    // Deleting the break again gives the first geometry back.
    h.select(10, 10);
    assert!(matches!(
        h.send(Command::Delete {
            forward: false,
            unit: crate::rich_protocol::DeleteUnit::Grapheme,
        }),
        Response::State(_)
    ));
    let fresh = mark_ys(&mut boxed_from(
        "one
twoXY
three",
        4,
    ));
    assert_eq!(mark_ys(&mut h), fresh, "after the deleted break");
    // Deleting the break between the plain paragraph and the box.
    h.select(4, 4);
    h.send(Command::Delete {
        forward: false,
        unit: crate::rich_protocol::DeleteUnit::Grapheme,
    });
    // The merged paragraph keeps the (plain) properties of the first one; "three" is a box of its own.
    let after = mark_ys(&mut h);
    assert_eq!(
        after,
        mark_ys(&mut boxed_from(
            "onetwoXY
three",
            9
        )),
        "after merging the plain paragraph and the box"
    );
}
