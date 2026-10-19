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

fn font_file(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../data/fonts/liberation/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("a font of data/fonts")
}

#[test]
fn a_font_loaded_on_demand_replaces_the_substitute_and_is_reported() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(0, 5);
    word(
        &mut h,
        vec![edit(
            CharAttr::Font,
            EditOp::Set,
            Some(CharValue::Text("Arial".into())),
        )],
    );
    let used = |h: &mut Harness| match h.state().chars.font {
        Tri::Value(font) => (font.used, font.substituted),
        other => panic!("a font, got {other:?}"),
    };
    // The editor registers Carlito only: Arial is drawn with it.
    assert_eq!(used(&mut h), ("Carlito".to_string(), true));
    let revision = h.state().revision;
    let faces: Vec<Vec<u8>> = ["Regular", "Bold", "Italic", "BoldItalic"]
        .iter()
        .map(|style| font_file(&format!("LiberationSans-{style}.ttf")))
        .collect();
    assert!(matches!(
        h.send(Command::LoadFonts(faces.clone())),
        Response::State(_)
    ));
    // The metric substitute is now the face of the family; no revision moves.
    assert_eq!(used(&mut h), ("Liberation Sans".to_string(), true));
    assert_eq!(h.state().revision, revision);
    // Loading the same files again changes nothing.
    assert!(matches!(
        h.send(Command::LoadFonts(faces)),
        Response::State(_)
    ));
    assert_eq!(used(&mut h), ("Liberation Sans".to_string(), true));
    let Response::FontList(list) = h.send(Command::ListFonts) else {
        panic!("a font list");
    };
    let arial = list.families.iter().find(|f| f.name == "Arial").unwrap();
    assert_eq!(arial.substitute, "Liberation Sans");
    // Bytes that are no font are refused.
    assert_eq!(
        h.error(Command::LoadFonts(vec![vec![1, 2, 3, 4]])),
        ErrorCode::InvalidBody
    );
}

fn all_font_files(names: &[&str]) -> Vec<Vec<u8>> {
    names.iter().map(|name| font_file(name)).collect()
}

#[test]
fn the_font_budget_is_checked_before_any_font_is_kept() {
    use super::format_char::intern_in;
    use std::sync::Mutex;
    assert_eq!(crate::rich_preview::LOADED_FONT_FACES, 16);
    assert_eq!(crate::rich_preview::LOADED_FONT_BYTES, 6 * 1024 * 1024);
    let sans = all_font_files(&[
        "LiberationSans-Regular.ttf",
        "LiberationSans-Bold.ttf",
        "LiberationSans-Italic.ttf",
        "LiberationSans-BoldItalic.ttf",
    ]);
    // Faces: three at most; a batch of four keeps nothing, even its first fonts.
    let registry = Mutex::new(Vec::new());
    assert!(intern_in(&registry, sans.clone(), 3, usize::MAX).is_none());
    assert!(registry.lock().unwrap().is_empty());
    assert!(intern_in(&registry, sans[..2].to_vec(), 3, usize::MAX).is_some());
    // A font already kept costs nothing; one more fresh font fits; a fourth does not.
    assert!(
        intern_in(
            &registry,
            vec![sans[0].clone(), sans[2].clone()],
            3,
            usize::MAX
        )
        .is_some()
    );
    assert!(intern_in(&registry, vec![sans[3].clone()], 3, usize::MAX).is_none());
    assert_eq!(registry.lock().unwrap().len(), 3);
    // Bytes: room for one face only.
    let registry = Mutex::new(Vec::new());
    assert!(intern_in(&registry, sans[..2].to_vec(), 16, sans[0].len() + 1).is_none());
    assert!(registry.lock().unwrap().is_empty());
    assert!(intern_in(&registry, sans[..1].to_vec(), 16, sans[0].len() + 1).is_some());
    // Something that is no font refuses the whole batch.
    assert!(intern_in(&registry, vec![sans[1].clone(), vec![9; 8]], 16, usize::MAX).is_none());
    assert_eq!(registry.lock().unwrap().len(), 1);
    // The real budget: sixteen faces fit, a seventeenth does not.
    let registry = Mutex::new(Vec::new());
    let mut sixteen = sans.clone();
    sixteen.extend(all_font_files(&[
        "LiberationSerif-Regular.ttf",
        "LiberationSerif-Bold.ttf",
        "LiberationSerif-Italic.ttf",
        "LiberationSerif-BoldItalic.ttf",
        "LiberationMono-Regular.ttf",
        "LiberationMono-Bold.ttf",
        "LiberationMono-Italic.ttf",
        "LiberationMono-BoldItalic.ttf",
    ]));
    let bytes: usize = sixteen.iter().map(Vec::len).sum();
    let limit = (
        crate::rich_preview::LOADED_FONT_FACES,
        crate::rich_preview::LOADED_FONT_BYTES,
    );
    assert!(bytes < limit.1);
    assert!(intern_in(&registry, sixteen.clone(), 12, limit.1).is_some());
    assert!(
        intern_in(
            &registry,
            vec![font_file("../carlito/Carlito-Regular.ttf")],
            12,
            limit.1
        )
        .is_none()
    );
}

#[test]
fn a_load_fonts_body_out_of_bounds_is_refused_by_the_decoder() {
    use crate::rich_protocol::{Request, decode_request, encode_request};
    let frame = encode_request(&Request {
        doc_id: 1,
        request_id: 1,
        command: Command::LoadFonts(vec![vec![0, 1, 0, 0, 7, 7]]),
    })
    .unwrap();
    let body = 20;
    assert!(decode_request(&frame).is_ok());
    let tamper = |at: usize, bytes: &[u8]| {
        let mut copy = frame.clone();
        copy[at..at + bytes.len()].copy_from_slice(bytes);
        decode_request(&copy).is_err()
    };
    assert!(tamper(body, &[0]), "count 0");
    assert!(tamper(body, &[9]), "count 9");
    assert!(tamper(body + 1, &0u32.to_le_bytes()), "length 0");
    assert!(
        tamper(body + 1, &(1024u32 * 1024 + 1).to_le_bytes()),
        "length over 1 MiB"
    );
    // The encoder refuses the same.
    for fonts in [
        vec![],
        vec![vec![]],
        vec![vec![1u8; 4]; 9],
        vec![vec![0u8; 1024 * 1024 + 1]],
    ] {
        assert!(
            encode_request(&Request {
                doc_id: 1,
                request_id: 1,
                command: Command::LoadFonts(fonts)
            })
            .is_err()
        );
    }
}

#[test]
fn fonts_load_in_a_read_only_document_and_a_resync_agrees_with_the_state() {
    use crate::rich_protocol::{OpenKind, Profile};
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/revisions.docx"
    ))
    .unwrap();
    assert!(matches!(
        h.send(Command::Open { kind: OpenKind::Docx, profile: Profile::Document, data: docx }),
        Response::State(s) if s.read_only
    ));
    let faces = all_font_files(&["LiberationSans-Regular.ttf", "LiberationSans-Bold.ttf"]);
    let Response::State(loaded) = h.send(Command::LoadFonts(faces)) else {
        panic!("a State");
    };
    assert!(loaded.read_only, "the document stays read-only");
    let Response::State(again) = h.send(Command::Resync) else {
        panic!("a State");
    };
    assert_eq!(again.page_count, loaded.page_count);
    assert_eq!(again.revision, loaded.revision);
    assert_eq!(again.doc_len, loaded.doc_len);
    // Resync starts the atlas over (a new epoch).
    assert_ne!(again.atlas.epoch, loaded.atlas.epoch);
    assert_eq!(again.atlas.first, 0);
}
