// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the style handlers (US-250): `ListStyles`, `ApplyStyle` and the
//! preview of `PreviewStyle`, with its error paths (typing, undo, lost reply,
//! resynchronisation, reopening, read-only, plain text, save).

use super::format_tests::{Harness, bold};
use super::*;
use crate::rich_protocol::{
    NO_REVISION_GUARD, OpenKind, Profile, SaveFormat, Stamp, StyleEntry, StyleFilter, StyleTarget,
    Tri,
};
use atelier_render::DisplayList;
use atelier_text_model::rich::{Alignment, CharAttrs, ParaProps, RichOp};

const NORMAL: &str = "atelier.normal";
const H1: &str = "atelier.heading.1";
const H2: &str = "atelier.heading.2";
const TITLE: &str = "atelier.title";
const EMPHASIS: &str = "atelier.emphasis";

fn state_of(response: Response) -> State {
    match response {
        Response::State(state) => state,
        other => panic!("expected a state, got {other:?}"),
    }
}

fn styles(h: &mut Harness, filter: StyleFilter, used_only: bool) -> Vec<StyleEntry> {
    match h.send(Command::ListStyles { filter, used_only }) {
        Response::StyleList { styles, .. } => styles,
        other => panic!("expected a style list, got {other:?}"),
    }
}

fn apply(h: &mut Harness, target: StyleTarget, id: &str) -> Response {
    h.send(Command::ApplyStyle {
        base_revision: NO_REVISION_GUARD,
        command_id: 0,
        target,
        id: id.into(),
    })
}

fn paragraph_style(h: &mut Harness, id: &str) -> State {
    state_of(apply(h, StyleTarget::Paragraph, id))
}

fn preview(h: &mut Harness, target: StyleTarget, id: &str) -> State {
    state_of(h.send(Command::PreviewStyle {
        target,
        show: true,
        id: id.into(),
    }))
}

fn cancel(h: &mut Harness) -> State {
    state_of(h.send(Command::PreviewStyle {
        target: StyleTarget::Paragraph,
        show: false,
        id: String::new(),
    }))
}

/// A page as the client would draw it, with the glyph identifiers (indices of
/// the atlas, which depend on the history of the session) set aside: what is
/// compared is the geometry, the colours and the structure of the page.
fn drawn(display: &[u8]) -> DisplayList {
    let mut list = DisplayList::decode(display).expect("a valid display list");
    for command in &mut list.commands {
        if let atelier_render::Command::DrawGlyphRun { glyphs, .. } = command {
            for glyph in glyphs {
                glyph.glyph_id = 0;
            }
        }
    }
    list
}

fn page_at(h: &mut Harness, index: u32) -> DisplayList {
    match h.send(Command::Pages {
        first: index,
        count: 1,
    }) {
        Response::Pages { pages, .. } => drawn(&pages[0].display),
        other => panic!("expected pages, got {other:?}"),
    }
}

/// The painted first page, as the client would draw it now.
fn page0(h: &mut Harness) -> DisplayList {
    page_at(h, 0)
}

/// Runs the pending layout to the end.
fn layout_all(h: &mut Harness) -> State {
    for _ in 0..400 {
        let state = state_of(h.send(Command::Layout { budget: 256 }));
        if state.layout_complete {
            return state;
        }
    }
    panic!("the layout does not complete");
}

fn style_of(h: &mut Harness, index: usize) -> Option<String> {
    LayoutContext::paragraph(h.doc(), index)
        .unwrap()
        .props
        .style
        .clone()
}

fn text_of(h: &mut Harness) -> String {
    let len = h.doc().len();
    h.doc().text(0, len).unwrap()
}

fn open_blank(h: &mut Harness) {
    assert!(matches!(
        h.send(Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Document,
            data: Vec::new()
        }),
        Response::State(_)
    ));
}

fn italic() -> CharAttrs {
    CharAttrs {
        italic: Some(true),
        ..CharAttrs::default()
    }
}

#[test]
fn the_style_list_orders_flags_and_describes_the_tiles() {
    let mut h = Harness::new();
    let all = styles(&mut h, StyleFilter::All, false);
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    // Word's order: the default style, then uiPriority; character styles last.
    assert_eq!(ids[0], NORMAL);
    let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
    assert!(at("atelier.no_spacing") < at(H1) && at(H1) < at(H2) && at(H2) < at(TITLE));
    assert!(at(TITLE) < at("atelier.subtitle") && at("atelier.subtitle") < at("atelier.quote"));
    assert!(at("atelier.quote.intense") < at(EMPHASIS));
    // No table style, no style of another kind in a filtered list.
    assert!(!ids.contains(&"atelier.table.grid"));
    let normal = &all[0];
    // Predefined, used, current, default, quick.
    assert_eq!(normal.flags, 0b11111);
    assert_eq!(normal.name, "Normal");
    let h1 = &all[at(H1)];
    assert_eq!(h1.heading_level, 1);
    assert_eq!(h1.name, "heading 1");
    assert_eq!(h1.based_on, NORMAL);
    assert_eq!(h1.target, StyleTarget::Paragraph);
    assert_eq!(h1.flags, 0b10001, "predefined and quick, not used");
    assert_eq!((h1.half_points, h1.char_flags & 1), (32, 1));
    assert_eq!(h1.font, "Carlito", "the font of the document defaults");
    assert_eq!(all[at("atelier.heading.4")].flags & 0b10000, 0, "not quick");
    assert_eq!(all[at("atelier.heading.4")].heading_level, 4);
    let emphasis = &all[at(EMPHASIS)];
    assert_eq!(emphasis.target, StyleTarget::Character);
    assert_eq!(emphasis.char_flags & 2, 2, "italic");
    // Filters.
    let character = styles(&mut h, StyleFilter::Character, false);
    assert!(character.iter().all(|s| s.target == StyleTarget::Character));
    assert_eq!(character.len(), 3);
    let paragraph = styles(&mut h, StyleFilter::Paragraph, false);
    assert!(paragraph.iter().all(|s| s.target == StyleTarget::Paragraph));
    assert_eq!(paragraph.len() + character.len(), all.len());
}

#[test]
fn the_style_list_follows_use_and_the_current_style() {
    let mut h = Harness::new();
    h.type_text("Hello");
    // Only the style in use, current and default.
    let used = styles(&mut h, StyleFilter::All, true);
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].id, NORMAL);
    paragraph_style(&mut h, H1);
    let used = styles(&mut h, StyleFilter::All, true);
    let ids: Vec<&str> = used.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [NORMAL, H1],
        "Normal stays (default), Heading 1 is used"
    );
    let all = styles(&mut h, StyleFilter::All, false);
    let flags = |id: &str| all.iter().find(|s| s.id == id).unwrap().flags;
    assert_eq!(flags(H1) & 0b110, 0b110, "used and current");
    assert_eq!(flags(NORMAL) & 0b110, 0, "no paragraph uses it any more");
    // A character style: current when the selection has it.
    h.select(0, 3);
    apply(&mut h, StyleTarget::Character, EMPHASIS);
    let all = styles(&mut h, StyleFilter::All, false);
    let emphasis = all.iter().find(|s| s.id == EMPHASIS).unwrap();
    assert_eq!(emphasis.flags & 0b110, 0b110);
    h.select(4, 5);
    let all = styles(&mut h, StyleFilter::All, false);
    assert_eq!(
        all.iter().find(|s| s.id == EMPHASIS).unwrap().flags & 0b100,
        0
    );
    // Two paragraphs in two styles: no paragraph style is current.
    h.select(5, 5);
    h.type_text("\nsecond");
    paragraph_style(&mut h, H2);
    h.select(0, 12);
    let revision = h.state().revision;
    let all = styles(&mut h, StyleFilter::Paragraph, false);
    assert!(all.iter().all(|s| s.flags & 0b100 == 0));
    // The list does not change the document.
    assert_eq!(h.state().revision, revision);
}

#[test]
fn applying_a_paragraph_style_keeps_direct_character_and_paragraph_formatting() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.attrs(0, 0, 5, italic());
    h.props(
        0,
        ParaProps {
            alignment: Some(Alignment::Center),
            ..ParaProps::default()
        },
    );
    let before = h.state().revision;
    let after = paragraph_style(&mut h, H1);
    assert_eq!(after.revision, before + 1, "one revision");
    assert!(after.can_undo && after.modified);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(H1));
    let paragraph = LayoutContext::paragraph(h.doc(), 0).unwrap().clone();
    assert_eq!(paragraph.runs[0].attrs.italic, Some(true), "direct italic");
    assert_eq!(
        paragraph.props.alignment,
        Some(Alignment::Center),
        "direct alignment"
    );
    let state = h.state();
    assert!(matches!(&state.paragraph.style, Tri::Value(id) if id == H1));
    assert!(matches!(state.chars.italic, Tri::Value(true)));
    assert!(
        matches!(state.chars.bold, Tri::Value(true)),
        "from Heading 1"
    );
    // The same style again changes nothing.
    let again = paragraph_style(&mut h, H1);
    assert_eq!(again.revision, after.revision);
    // One Ctrl+Z restores the style, the direct formatting stays.
    let undone = state_of(h.send(Command::Undo));
    assert_eq!(undone.revision, after.revision + 1);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(NORMAL));
    let paragraph = LayoutContext::paragraph(h.doc(), 0).unwrap().clone();
    assert_eq!(paragraph.runs[0].attrs.italic, Some(true));
    let redone = state_of(h.send(Command::Redo));
    assert!(redone.can_undo);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(H1));
}

#[test]
fn a_style_goes_to_every_paragraph_the_selection_touches_in_one_step() {
    let mut h = Harness::new();
    h.type_text("one\ntwo\nthree");
    let before = h.state().revision;
    // A selection that ends at the start of a paragraph does not touch it.
    h.select(0, 8);
    paragraph_style(&mut h, H2);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(H2));
    assert_eq!(style_of(&mut h, 1).as_deref(), Some(H2));
    assert_eq!(style_of(&mut h, 2).as_deref(), Some(NORMAL));
    assert_eq!(h.state().revision, before + 1);
    h.select(0, 13);
    paragraph_style(&mut h, TITLE);
    for index in 0..3 {
        assert_eq!(style_of(&mut h, index).as_deref(), Some(TITLE));
    }
    state_of(h.send(Command::Undo));
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(H2));
    assert_eq!(style_of(&mut h, 2).as_deref(), Some(NORMAL));
    state_of(h.send(Command::Undo));
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(NORMAL));
}

#[test]
fn a_character_style_covers_the_selection_or_the_word_and_can_be_removed() {
    let mut h = Harness::new();
    h.type_text("Hello world");
    h.select(0, 3);
    let before = h.state().revision;
    apply(&mut h, StyleTarget::Character, EMPHASIS);
    assert_eq!(h.state().revision, before + 1);
    let runs = LayoutContext::paragraph(h.doc(), 0).unwrap().runs.clone();
    assert_eq!((runs[0].start, runs[0].end), (0, 3));
    assert_eq!(runs[0].attrs.char_style.as_deref(), Some(EMPHASIS));
    // A caret in a word: the word.
    h.select(8, 8);
    apply(&mut h, StyleTarget::Character, "atelier.strong");
    let runs = LayoutContext::paragraph(h.doc(), 0).unwrap().runs.clone();
    assert!(
        runs.iter().any(|r| (r.start, r.end) == (6, 11)
            && r.attrs.char_style.as_deref() == Some("atelier.strong"))
    );
    // Removing the style (empty identifier).
    h.select(0, 3);
    apply(&mut h, StyleTarget::Character, "");
    let runs = LayoutContext::paragraph(h.doc(), 0).unwrap().runs.clone();
    assert!(
        runs.iter()
            .all(|r| r.attrs.char_style.as_deref() != Some(EMPHASIS))
    );
    // The paragraph style is not the character style: Normal stays.
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(NORMAL));
    // A caret outside a word arms the style for the next typed text.
    h.select(11, 11);
    h.type_text(" ");
    apply(&mut h, StyleTarget::Character, EMPHASIS);
    assert!(h.state().typing);
    h.type_text("x");
    let runs = LayoutContext::paragraph(h.doc(), 0).unwrap().runs.clone();
    assert!(
        runs.iter()
            .any(|r| r.end == 13 && r.attrs.char_style.as_deref() == Some(EMPHASIS))
    );
}

#[test]
fn the_refusals_of_apply_style() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let before = h.state().revision;
    let refuse = |h: &mut Harness, target, id: &str| {
        h.error(Command::ApplyStyle {
            base_revision: NO_REVISION_GUARD,
            command_id: 0,
            target,
            id: id.into(),
        })
    };
    assert_eq!(
        refuse(&mut h, StyleTarget::Paragraph, "nope"),
        ErrorCode::UnknownStyle
    );
    assert_eq!(
        refuse(&mut h, StyleTarget::Paragraph, EMPHASIS),
        ErrorCode::UnknownStyle,
        "a character style is not a paragraph style"
    );
    assert_eq!(
        refuse(&mut h, StyleTarget::Character, H1),
        ErrorCode::UnknownStyle
    );
    assert_eq!(
        refuse(&mut h, StyleTarget::Character, "nope"),
        ErrorCode::UnknownStyle
    );
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: before + 7,
            command_id: 0,
            target: StyleTarget::Paragraph,
            id: H1.into()
        }),
        ErrorCode::StaleRevision
    );
    assert!(matches!(
        h.send(Command::ApplyStyle {
            base_revision: before,
            command_id: 9,
            target: StyleTarget::Paragraph,
            id: H1.into()
        }),
        Response::State(_)
    ));
    assert_eq!(h.state().last_command_id, 9);
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: NO_REVISION_GUARD,
            command_id: 9,
            target: StyleTarget::Paragraph,
            id: H2.into()
        }),
        ErrorCode::DuplicateCommand
    );
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(H1));
    assert_eq!(h.state().revision, before + 1);
}

#[test]
fn a_preview_changes_the_pages_and_nothing_else() {
    let mut h = Harness::new();
    h.type_text("Titre");
    let base = h.state();
    let base_page = page0(&mut h);
    let base_caret = state_of(h.send(Command::SetSelection {
        anchor: 5,
        focus: 5,
        upstream: false,
    }))
    .caret
    .unwrap();
    let shown = preview(&mut h, StyleTarget::Paragraph, TITLE);
    // Revision, history, `modified`: untouched.
    assert_eq!(shown.revision, base.revision);
    assert!(shown.can_undo && shown.modified);
    assert!(!shown.can_redo);
    assert_eq!(shown.pages.len(), 1, "the touched page is sent");
    assert!(page0(&mut h) != base_page, "the page shows the style");
    assert!(shown.caret.unwrap().height > base_caret.height);
    assert_eq!(
        style_of(&mut h, 0).as_deref(),
        Some(NORMAL),
        "the model is unchanged"
    );
    let state = h.state();
    assert!(matches!(&state.paragraph.style, Tri::Value(id) if id == NORMAL));
    // Cancel: the very pages of before, and again (idempotent).
    let cancelled = cancel(&mut h);
    assert_eq!(cancelled.revision, base.revision);
    assert_eq!(cancelled.pages.len(), 1);
    assert!(page0(&mut h) == base_page, "the pages differ");
    let twice = cancel(&mut h);
    assert!(twice.pages.is_empty());
    assert_eq!(twice.revision, base.revision);
    // The preview of a style the paragraph already has changes nothing.
    let same = preview(&mut h, StyleTarget::Paragraph, NORMAL);
    assert!(same.pages.is_empty());
    assert!(page0(&mut h) == base_page, "the pages differ");
}

#[test]
fn a_preview_replaces_the_previous_one_and_a_character_style_can_be_previewed() {
    let mut h = Harness::new();
    h.type_text("Hello world");
    h.select(0, 5);
    let base_page = page0(&mut h);
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    preview(&mut h, StyleTarget::Paragraph, H2);
    let both = page0(&mut h);
    // The same as a preview of Heading 2 alone.
    let mut other = Harness::new();
    other.type_text("Hello world");
    other.select(0, 5);
    preview(&mut other, StyleTarget::Paragraph, H2);
    assert!(both == page0(&mut other), "the pages differ");
    cancel(&mut h);
    assert!(page0(&mut h) == base_page, "the pages differ");
    // A character style over the selection only.
    preview(&mut h, StyleTarget::Character, "atelier.strong");
    assert!(page0(&mut h) != base_page);
    assert_eq!(
        LayoutContext::paragraph(h.doc(), 0).unwrap().runs.len(),
        0,
        "no run in the model"
    );
    cancel(&mut h);
    assert!(page0(&mut h) == base_page, "the pages differ");
}

#[test]
fn typing_ends_the_preview_and_the_state_reports_the_restored_pages() {
    let mut h = Harness::new();
    h.type_text("Titre");
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    let typed = state_of(h.send(Command::InsertText { text: "!".into() }));
    assert_eq!(typed.pages.len(), 1);
    // A document typed the same way, without any preview: the same page.
    let mut reference = Harness::new();
    reference.type_text("Titre");
    reference.type_text("!");
    assert!(page0(&mut h) == page0(&mut reference), "the pages differ");
    assert!(h.editor.session().preview.overlay.is_none());
    // The hover ended: another cancel does nothing.
    assert!(cancel(&mut h).pages.is_empty());
}

#[test]
fn undo_after_a_preview_undoes_the_edit_and_keeps_redo() {
    let mut h = Harness::new();
    h.type_text("a");
    h.type_text("b");
    state_of(h.send(Command::Undo));
    let base = h.state();
    preview(&mut h, StyleTarget::Paragraph, H1);
    // No phantom step: Ctrl+Z goes to the edit before.
    let undone = state_of(h.send(Command::Undo));
    assert_eq!(undone.revision, base.revision + 1);
    assert_eq!(text_of(&mut h), "");
    // Redo still works: the preview did not empty it.
    preview(&mut h, StyleTarget::Paragraph, H1);
    let redone = state_of(h.send(Command::Redo));
    assert_eq!(text_of(&mut h), "a");
    assert!(redone.can_redo);
    // A preview alone never empties the redo stack.
    state_of(h.send(Command::Undo));
    let shown = preview(&mut h, StyleTarget::Paragraph, H1);
    assert!(shown.can_redo);
}

#[test]
fn applying_during_a_preview_ends_it_and_is_one_undo_step() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let base_page = page0(&mut h);
    let base = h.state().revision;
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    let applied = paragraph_style(&mut h, H1);
    assert_eq!(applied.revision, base + 1);
    let mut reference = Harness::new();
    reference.type_text("Hello");
    paragraph_style(&mut reference, H1);
    assert!(page0(&mut h) == page0(&mut reference), "the pages differ");
    state_of(h.send(Command::Undo));
    assert!(page0(&mut h) == base_page, "one Ctrl+Z restores the page");
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(NORMAL));
    // The same style as the preview.
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    paragraph_style(&mut h, TITLE);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(TITLE));
    assert!(h.editor.session().preview.overlay.is_none());
}

#[test]
fn a_selection_change_ends_the_preview() {
    let mut h = Harness::new();
    h.type_text("one\ntwo");
    let base_page = page0(&mut h);
    h.select(0, 0);
    preview(&mut h, StyleTarget::Paragraph, H1);
    assert!(page0(&mut h) != base_page);
    h.select(5, 5);
    assert!(page0(&mut h) == base_page, "the pages differ");
    preview(&mut h, StyleTarget::Paragraph, H1);
    state_of(h.send(Command::Move {
        direction: crate::rich_protocol::Direction::Left,
        extend: false,
    }));
    assert!(page0(&mut h) == base_page, "the pages differ");
}

#[test]
fn the_preview_survives_resync_and_reading_commands_and_ends_with_the_document() {
    let mut h = Harness::new();
    h.type_text("Hello");
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    let shown = page0(&mut h);
    // A lost reply: the client resynchronises, the preview is session state.
    let resynced = state_of(h.send(Command::Resync));
    assert!(!resynced.invalidated.is_empty());
    assert!(page0(&mut h) == shown, "the pages differ");
    // Reading commands do not end it.
    h.state();
    styles(&mut h, StyleFilter::All, false);
    state_of(h.send(Command::Layout { budget: 16 }));
    assert!(page0(&mut h) == shown, "the pages differ");
    assert!(h.editor.session().preview.overlay.is_some());
    // A new document starts without preview.
    open_blank(&mut h);
    assert!(h.editor.session().preview.overlay.is_none());
    let cancelled = cancel(&mut h);
    assert!(cancelled.pages.is_empty());
}

#[test]
fn the_save_never_sees_the_preview() {
    let mut h = Harness::new();
    h.type_text("Hello");
    preview(&mut h, StyleTarget::Paragraph, TITLE);
    let shown = page0(&mut h);
    let bytes = match h.send(Command::Save {
        format: SaveFormat::Docx,
        stamp: Stamp {
            now_ms: 1_700_000_000_000,
            user: "Alice".into(),
            version: "1.2".into(),
        },
    }) {
        Response::SaveData { bytes, .. } => bytes,
        other => panic!("expected SaveData, got {other:?}"),
    };
    // The preview is still on screen after the save...
    assert!(page0(&mut h) == shown, "the pages differ");
    // ...and the file holds the real style.
    let mut reopened = Harness::new();
    assert!(matches!(
        reopened.send(Command::Open {
            kind: OpenKind::Docx,
            profile: Profile::Document,
            data: bytes
        }),
        Response::State(_)
    ));
    reopened.select(0, 0);
    let state = reopened.state();
    assert!(
        matches!(&state.paragraph.style, Tri::Value(id) if id == NORMAL),
        "{:?}",
        state.paragraph.style
    );
    let list = styles(&mut reopened, StyleFilter::All, false);
    let title = list.iter().find(|s| s.id == TITLE).unwrap();
    assert_eq!(title.name, "Title");
    assert!(title.flags & 1 == 1, "predefined");
    assert!(list.iter().any(|s| s.id == H1 && s.heading_level == 1));
}

#[test]
fn the_preview_over_a_long_document_follows_the_layout_pump_and_cancels_cleanly() {
    let mut h = Harness::new();
    let line = "A line of text that is long enough to wrap onto a second line at least.";
    let text = vec![line; 150].join("\n");
    h.type_text(&text);
    let base = layout_all(&mut h);
    let len = h.doc().len() as u64;
    h.select(0, len);
    let base_page = page0(&mut h);
    let shown = preview(&mut h, StyleTarget::Paragraph, H1);
    let during = layout_all(&mut h);
    assert!(
        during.page_count > base.page_count,
        "bigger text, more pages"
    );
    assert_eq!(shown.revision, base.revision);
    let cancelled = cancel(&mut h);
    let after = layout_all(&mut h);
    assert_eq!(after.page_count, base.page_count);
    assert!(cancelled.page_count >= base.page_count);
    assert!(page0(&mut h) == base_page, "the pages differ");
    // The last page too: laid out under the preview, then again without.
    let last = base.page_count - 1;
    let mut reference = Harness::new();
    reference.type_text(&text);
    layout_all(&mut reference);
    assert!(
        page_at(&mut h, last) == page_at(&mut reference, last),
        "the last page differs"
    );
}

#[test]
fn the_style_commands_in_read_only_and_plain_text_documents() {
    let mut plain = Harness::new();
    assert!(matches!(
        plain.send(Command::Open {
            kind: OpenKind::Txt,
            profile: Profile::Txt,
            data: b"plain".to_vec()
        }),
        Response::State(_)
    ));
    plain.select(0, 5);
    assert_eq!(
        plain.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: true,
            id: H1.into()
        }),
        ErrorCode::FormatPlainText
    );
    assert_eq!(
        plain.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: false,
            id: String::new()
        }),
        ErrorCode::FormatPlainText
    );
    // The list is allowed everywhere (a plain document has no style).
    assert!(styles(&mut plain, StyleFilter::All, false).is_empty());
    // An unknown style is refused and leaves no preview.
    let mut h = Harness::new();
    h.type_text("Hello");
    assert_eq!(
        h.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: true,
            id: "nope".into()
        }),
        ErrorCode::UnknownStyle
    );
    assert_eq!(
        h.error(Command::PreviewStyle {
            target: StyleTarget::Character,
            show: true,
            id: H1.into()
        }),
        ErrorCode::UnknownStyle
    );
    assert!(h.editor.session().preview.overlay.is_none());
}

#[test]
fn bold_direct_formatting_is_kept_by_a_style_that_is_not_bold() {
    // Normal is not bold: a bold word keeps its bold when Heading 2 and back to Normal apply.
    let mut h = Harness::new();
    h.type_text("Hello");
    h.attrs(0, 0, 5, bold());
    paragraph_style(&mut h, H2);
    paragraph_style(&mut h, NORMAL);
    let runs = LayoutContext::paragraph(h.doc(), 0).unwrap().runs.clone();
    assert_eq!(runs[0].attrs.bold, Some(true));
}

#[test]
fn statistics_and_properties_do_not_keep_the_preview() {
    // Their page and line counts come from the layout: they must not count the preview.
    for command in [Command::Statistics, Command::PropertiesRead] {
        let mut h = Harness::new();
        h.type_text("Hello");
        let base_page = page0(&mut h);
        preview(&mut h, StyleTarget::Paragraph, TITLE);
        assert!(page0(&mut h) != base_page);
        h.send(command);
        assert!(h.editor.session().preview.overlay.is_none());
        assert!(page0(&mut h) == base_page, "the preview ended");
    }
}

#[test]
fn the_default_style_on_a_paragraph_without_reference_changes_nothing() {
    let mut h = Harness::new();
    h.type_text("Hello");
    // No style reference: the paragraph is in the default style (None is Normal).
    let para = h.doc().paragraph_ids()[0];
    h.doc()
        .apply(
            &atelier_model::Transaction::new(vec![RichOp::SetParaProps {
                para,
                set: ParaProps::default(),
                clear: ParaProps {
                    style: Some("x".into()),
                    ..ParaProps::default()
                },
            }])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(style_of(&mut h, 0), None);
    let before = state_of(h.send(Command::Layout { budget: 1 }));
    let after = paragraph_style(&mut h, NORMAL);
    assert_eq!(after.revision, before.revision, "no revision");
    assert_eq!(after.can_undo, before.can_undo);
    assert_eq!(style_of(&mut h, 0), None, "no reference written");
    // A preview of the default style shows nothing either.
    let shown = preview(&mut h, StyleTarget::Paragraph, NORMAL);
    assert!(shown.pages.is_empty());
    assert!(h.editor.session().preview.overlay.is_none());
}

/// A DOCX whose catalog lacks Heading 2, Heading 3 and Quote (Word writes them once used).
fn docx_without_headings() -> Vec<u8> {
    use atelier_fmt_ooxml::{DocxWriteOptions, NewDocument, write_new_docx};
    let mut profile = atelier_text_model::rich::default_rich_document();
    for id in ["atelier.heading.2", "atelier.heading.3", "atelier.quote"] {
        profile.styles.remove_style(id);
        profile.style_names.remove(id);
    }
    write_new_docx(
        &NewDocument::from_default(&profile),
        &DocxWriteOptions::default(),
    )
    .unwrap()
    .bytes
}

fn save_docx(h: &mut Harness) -> Vec<u8> {
    match h.send(Command::Save {
        format: SaveFormat::Docx,
        stamp: Stamp {
            now_ms: 1_700_000_000_000,
            user: "Alice".into(),
            version: "1.2".into(),
        },
    }) {
        Response::SaveData { bytes, .. } => bytes,
        other => panic!("expected SaveData, got {other:?}"),
    }
}

#[test]
fn a_built_in_style_the_file_lacks_is_offered_instantiated_when_applied_and_saved() {
    let mut h = Harness::new();
    assert!(matches!(
        h.send(Command::Open {
            kind: OpenKind::Docx,
            profile: Profile::Document,
            data: docx_without_headings()
        }),
        Response::State(_)
    ));
    h.type_text("Hello");
    let listed = styles(&mut h, StyleFilter::All, false);
    let heading = listed.iter().find(|s| s.id == "Heading2").expect("offered");
    assert_eq!(
        (heading.name.as_str(), heading.heading_level, heading.flags),
        ("heading 2", 2, 0b10001)
    );
    assert_eq!(heading.half_points, 30, "sampled from the profile");
    assert!(
        listed
            .iter()
            .any(|s| s.id == "Heading3" && s.heading_level == 3)
    );
    assert!(!listed.iter().any(|s| s.id == "atelier.heading.2"));
    // Not used, not current: absent from the used list.
    assert!(
        !styles(&mut h, StyleFilter::All, true)
            .iter()
            .any(|s| s.id == "Heading2")
    );
    // Applying it defines it (based on the default style) in the same undo step.
    let before = h.state().revision;
    let applied = paragraph_style(&mut h, "Heading2");
    assert_eq!(applied.revision, before + 1);
    assert_eq!(style_of(&mut h, 0).as_deref(), Some("Heading2"));
    let listed = styles(&mut h, StyleFilter::All, false);
    let heading = listed.iter().find(|s| s.id == "Heading2").unwrap();
    assert_eq!(heading.flags & 0b110, 0b110, "used and current");
    assert_eq!(heading.based_on, NORMAL);
    assert_eq!(
        listed.iter().filter(|s| s.id == "Heading2").count(),
        1,
        "once"
    );
    let state = h.state();
    assert!(
        matches!(&state.chars.half_points, Tri::Value(30)),
        "{:?}",
        state.chars.half_points
    );
    // One Ctrl+Z restores the paragraph.
    state_of(h.send(Command::Undo));
    assert_eq!(style_of(&mut h, 0).as_deref(), Some(NORMAL));
    state_of(h.send(Command::Redo));
    // Round trip: the saved file defines it, with its outline level, and the style is applied.
    let bytes = save_docx(&mut h);
    let mut again = Harness::new();
    assert!(matches!(
        again.send(Command::Open {
            kind: OpenKind::Docx,
            profile: Profile::Document,
            data: bytes
        }),
        Response::State(_)
    ));
    assert_eq!(style_of(&mut again, 0).as_deref(), Some("Heading2"));
    let listed = styles(&mut again, StyleFilter::All, false);
    let heading = listed.iter().find(|s| s.id == "Heading2").unwrap();
    assert_eq!((heading.heading_level, heading.flags & 0b11), (2, 0b11));
    assert!(heading.name == "heading 2" && heading.half_points == 30);
    // Heading 3 was never used: still offered, still not in the file.
    assert!(
        listed
            .iter()
            .any(|s| s.id == "Heading3" && s.flags & 0b10 == 0)
    );
}

#[test]
fn a_previewed_built_in_style_is_saved_only_when_applied() {
    let mut h = Harness::new();
    h.send(Command::Open {
        kind: OpenKind::Docx,
        profile: Profile::Document,
        data: docx_without_headings(),
    });
    h.type_text("Hello");
    preview(&mut h, StyleTarget::Paragraph, "Heading3");
    cancel(&mut h);
    state_of(h.send(Command::Layout { budget: 8 }));
    let mut again = Harness::new();
    again.send(Command::Open {
        kind: OpenKind::Docx,
        profile: Profile::Document,
        data: save_docx(&mut h),
    });
    // Hovered only: not defined in the saved file, still offered.
    let listed = styles(&mut again, StyleFilter::All, false);
    assert!(
        listed
            .iter()
            .any(|s| s.id == "Heading3" && s.flags & 0b10 == 0)
    );
    assert_eq!(
        LayoutContext::catalog(again.doc())
            .style("Heading3")
            .map(|s| s.id.clone()),
        None
    );
    // An unknown identifier is still refused.
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: NO_REVISION_GUARD,
            command_id: 0,
            target: StyleTarget::Paragraph,
            id: "Heading7x".into()
        }),
        ErrorCode::UnknownStyle
    );
}
