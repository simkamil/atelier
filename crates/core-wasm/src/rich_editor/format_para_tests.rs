// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the paragraph-formatting handler and of `FormatMarks` (US-243).

use super::format_tests::{Harness, para_format};
use super::{Session, io};
use crate::rich_protocol::{
    Command, EditOp, ErrorCode, FormatMark, ParaAttr, ParaEdit, ParaFormat, ParaValue, Response,
    Tri,
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

#[test]
fn shading_and_borders_are_refused_whole_and_nothing_is_recorded() {
    let mut h = three();
    let revision = h.state().revision;
    for attr in [ParaAttr::Shading, ParaAttr::Borders] {
        let mut f = format(vec![set_alignment(1), edit(attr, EditOp::Clear, None)]);
        f.command_id = 77;
        assert_eq!(
            h.error(Command::ApplyParaFormat(f)),
            ErrorCode::FormatNotImplemented
        );
    }
    assert_eq!(h.state().revision, revision);
    assert_eq!(h.state().paragraph.alignment, Tri::Value(0));
    assert_eq!(h.state().last_command_id, 0);
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
