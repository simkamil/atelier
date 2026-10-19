// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the character-formatting handler (US-240): targets (selection,
//! word, typing attributes), toggles and steps, mixed states, one undo step,
//! typing attributes joined with the insertion. Only US-240 edits this file.

use super::format_tests::{Harness, char_format};
use crate::rich_protocol::{
    CharAttr, CharEdit, CharFormat, CharValue, Command, EditOp, ErrorCode, FormatScope, Response,
    Rgb, Tri,
};
use atelier_text_model::rich::flat::FlatIndex;

fn edit(attr: CharAttr, op: EditOp, value: Option<CharValue>) -> CharEdit {
    CharEdit { attr, op, value }
}

fn apply(h: &mut Harness, scope: FormatScope, edits: Vec<CharEdit>) {
    let command = Command::ApplyCharFormat(CharFormat {
        scope,
        edits,
        ..char_format()
    });
    assert!(
        matches!(h.send(command), Response::State(_)),
        "the command is applied"
    );
}

fn word(h: &mut Harness, edits: Vec<CharEdit>) {
    apply(h, FormatScope::WordOrTyping, edits);
}

fn toggle(attr: CharAttr) -> CharEdit {
    edit(attr, EditOp::Toggle, None)
}

fn undo(h: &mut Harness) {
    assert!(matches!(h.send(Command::Undo), Response::State(_)));
}

#[test]
fn a_toggle_formats_the_selection_and_is_one_undo_step() {
    let mut h = Harness::new();
    h.type_text("Hello world");
    let revision = h.state().revision;
    h.select(0, 5);
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    assert_eq!(h.state().revision, revision + 1);
    h.select(0, 11);
    assert_eq!(h.state().chars.bold, Tri::Mixed);
    // A mixed selection becomes bold, then plain (Word's rule).
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    assert_eq!(h.state().chars.bold, Tri::Value(false));
    undo(&mut h);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    undo(&mut h);
    h.select(0, 11);
    assert_eq!(h.state().chars.bold, Tri::Mixed);
    undo(&mut h);
    h.select(0, 11);
    assert_eq!(h.state().chars.bold, Tri::Value(false));
}

#[test]
fn a_caret_in_a_word_formats_that_word() {
    let mut h = Harness::new();
    h.type_text("foo bar");
    h.select(1, 1);
    word(&mut h, vec![toggle(CharAttr::Italic)]);
    h.select(0, 3);
    assert_eq!(h.state().chars.italic, Tri::Value(true));
    h.select(4, 7);
    assert_eq!(h.state().chars.italic, Tri::Value(false));
    // The selection is untouched.
    h.select(2, 2);
    assert_eq!(h.state().anchor, 2);
    // `selectionOnly` never targets the word.
    apply(
        &mut h,
        FormatScope::SelectionOnly,
        vec![toggle(CharAttr::Strike)],
    );
    h.select(0, 3);
    assert_eq!(h.state().chars.strike, Tri::Value(false));
}

#[test]
fn every_character_attribute_can_be_set_and_cleared() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(0, 5);
    let red = Rgb { r: 255, g: 0, b: 0 };
    let yellow = Rgb {
        r: 255,
        g: 255,
        b: 0,
    };
    word(
        &mut h,
        vec![
            edit(
                CharAttr::Font,
                EditOp::Set,
                Some(CharValue::Text("Arial".into())),
            ),
            edit(CharAttr::Size, EditOp::Set, Some(CharValue::Half(21))),
            edit(CharAttr::Color, EditOp::Set, Some(CharValue::Color(red))),
            edit(
                CharAttr::Highlight,
                EditOp::Set,
                Some(CharValue::Color(yellow)),
            ),
            edit(CharAttr::Underline, EditOp::Set, Some(CharValue::Choice(6))),
            toggle(CharAttr::Strike),
            edit(
                CharAttr::VerticalAlign,
                EditOp::Toggle,
                Some(CharValue::Choice(1)),
            ),
        ],
    );
    let s = h.state();
    let Tri::Value(font) = &s.chars.font else {
        panic!("a font");
    };
    assert_eq!(font.requested, "Arial");
    assert_eq!(s.chars.half_points, Tri::Value(21));
    assert_eq!(s.chars.color, Tri::Value(red));
    assert_eq!(s.chars.highlight, Tri::Value(yellow));
    assert_eq!(s.chars.underline, Tri::Value(6));
    assert_eq!(s.chars.strike, Tri::Value(true));
    assert_eq!(s.chars.vertical_align, Tri::Value(1));
    // Superscript toggled again returns to the baseline; subscript replaces it.
    word(
        &mut h,
        vec![edit(
            CharAttr::VerticalAlign,
            EditOp::Toggle,
            Some(CharValue::Choice(2)),
        )],
    );
    assert_eq!(h.state().chars.vertical_align, Tri::Value(2));
    word(
        &mut h,
        vec![edit(
            CharAttr::VerticalAlign,
            EditOp::Toggle,
            Some(CharValue::Choice(2)),
        )],
    );
    assert_eq!(h.state().chars.vertical_align, Tri::Value(0));
    word(
        &mut h,
        vec![
            edit(CharAttr::Color, EditOp::Clear, None),
            edit(CharAttr::Highlight, EditOp::Clear, None),
            edit(CharAttr::Underline, EditOp::Set, Some(CharValue::Choice(0))),
        ],
    );
    let s = h.state();
    assert_eq!(s.chars.color, Tri::Unset);
    assert_eq!(s.chars.highlight, Tri::Unset);
    assert_eq!(s.chars.underline, Tri::Value(0));
}

#[test]
fn size_steps_follow_the_word_list_per_character() {
    let mut h = Harness::new();
    h.type_text("ab cd");
    // 11 pt by default (22 half-points).
    h.select(0, 2);
    word(&mut h, vec![edit(CharAttr::Size, EditOp::StepUp, None)]);
    assert_eq!(h.state().chars.half_points, Tri::Value(24));
    h.select(0, 5);
    assert_eq!(h.state().chars.half_points, Tri::Mixed);
    word(&mut h, vec![edit(CharAttr::Size, EditOp::StepUp, None)]);
    // Each character moves to its own next step: 12 -> 14 pt, 11 -> 12 pt.
    h.select(0, 2);
    assert_eq!(h.state().chars.half_points, Tri::Value(28));
    h.select(3, 5);
    assert_eq!(h.state().chars.half_points, Tri::Value(24));
    word(
        &mut h,
        vec![edit(CharAttr::Size, EditOp::StepDownPoint, None)],
    );
    assert_eq!(h.state().chars.half_points, Tri::Value(22));
    word(&mut h, vec![edit(CharAttr::Size, EditOp::StepDown, None)]);
    // 10.5 pt is a step of the list.
    assert_eq!(h.state().chars.half_points, Tri::Value(21));
}

#[test]
fn typing_attributes_are_applied_to_the_next_text_in_one_undo_step() {
    let mut h = Harness::new();
    h.type_text("Hello ");
    h.select(6, 6);
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    let s = h.state();
    assert!(s.typing, "attributes are pending");
    assert_eq!(s.chars.bold, Tri::Value(true));
    let revision = s.revision;
    h.type_text("wo");
    assert_eq!(
        h.state().revision,
        revision + 1,
        "one revision for the typed text"
    );
    h.select(6, 8);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    h.select(0, 6);
    assert_eq!(h.state().chars.bold, Tri::Value(false));
    // Typing goes on in bold from the character before the caret.
    h.select(8, 8);
    h.type_text("rld");
    h.select(6, 11);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    // Ctrl+Z removes the text and its formatting.
    undo(&mut h);
    assert_eq!(h.doc().len(), 8);
    undo(&mut h);
    assert_eq!(h.doc().len(), 6);
    h.select(0, 6);
    assert_eq!(h.state().chars.bold, Tri::Value(false));
}

#[test]
fn pending_typing_attributes_are_dropped_by_a_selection_change() {
    let mut h = Harness::new();
    h.type_text("Hello ");
    h.select(6, 6);
    word(&mut h, vec![toggle(CharAttr::Italic)]);
    assert!(h.state().typing);
    h.select(0, 0);
    h.select(6, 6);
    assert!(!h.state().typing);
    // Toggling twice at a caret outside a word leaves nothing pending.
    word(&mut h, vec![toggle(CharAttr::Italic)]);
    word(&mut h, vec![toggle(CharAttr::Italic)]);
    assert!(!h.state().typing);
    h.type_text("x");
    h.select(6, 7);
    assert_eq!(h.state().chars.italic, Tri::Value(false));
}

#[test]
fn a_style_value_is_overridden_explicitly_and_the_document_stays_clean() {
    let mut h = Harness::new();
    h.type_text("abc");
    h.select(0, 3);
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    // Back to no direct attribute at all: formatting twice leaves no trace.
    let doc = h.doc();
    let p = atelier_text_model::rich::flat::LayoutContext::paragraph(&*doc, 0).unwrap();
    assert!(p.runs.iter().all(|r| r.attrs.bold.is_none()));
}

#[test]
fn refusals_and_identifiers_follow_the_contract() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(0, 5);
    let revision = h.state().revision;
    let with = |command_id, base_revision| {
        Command::ApplyCharFormat(CharFormat {
            command_id,
            base_revision,
            ..char_format()
        })
    };
    assert_eq!(h.error(with(3, revision + 5)), ErrorCode::StaleRevision);
    assert_eq!(h.state().last_command_id, 0);
    assert!(matches!(h.send(with(3, revision)), Response::State(_)));
    assert_eq!(h.state().last_command_id, 3);
    assert_eq!(h.error(with(3, revision + 1)), ErrorCode::DuplicateCommand);
    // Typing attributes alone are accepted without a revision.
    h.select(5, 5);
    let revision = h.state().revision;
    assert!(matches!(h.send(with(4, revision)), Response::State(_)));
    assert_eq!(h.state().revision, revision);
    assert_eq!(h.state().last_command_id, 4);
}

#[test]
fn a_command_that_changes_nothing_records_no_revision() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(0, 5);
    word(&mut h, vec![toggle(CharAttr::Bold)]);
    let revision = h.state().revision;
    // Bold is already set: setting it again changes nothing.
    word(
        &mut h,
        vec![edit(
            CharAttr::Bold,
            EditOp::Set,
            Some(CharValue::Flag(true)),
        )],
    );
    assert_eq!(h.state().revision, revision);
    let _ = CharValue::Flag(true);
}
