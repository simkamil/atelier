// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the formatting family of the session (F0): `FormatState` written
//! end to end, the stubs of the three extension modules and their documented
//! error, the revision of the commands that must not change the document.

use super::*;
use crate::rich_protocol::{
    CharAttr, CharEdit, CharFormat, EditOp, FormatScope, FormatStateData, ParaAttr, ParaEdit,
    ParaFormat, ParaValue, Request, Rgb, StyleFilter, StyleTarget, Tri,
};
use atelier_model::Transaction;
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, ParaProps, RichOp, Underline, VerticalAlign,
};

struct Harness {
    editor: RichEditor,
    request: u32,
}

impl Harness {
    fn new() -> Self {
        let mut h = Self {
            editor: RichEditor::default(),
            request: 0,
        };
        h.send(Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Document,
            data: Vec::new(),
        });
        h
    }

    fn send(&mut self, command: Command) -> Response {
        self.request += 1;
        let frame = proto::encode_request(&Request {
            doc_id: 1,
            request_id: self.request,
            command,
        })
        .unwrap();
        let reply = self.editor.respond(&Message {
            kind: MessageType::Editor,
            request_id: 7,
            payload: frame,
        });
        assert_eq!(reply.kind, MessageType::EditorResult);
        proto::decode_reply(&reply.payload).unwrap().response
    }

    fn error(&mut self, command: Command) -> ErrorCode {
        match self.send(command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn type_text(&mut self, text: &str) {
        assert!(matches!(
            self.send(Command::InsertText { text: text.into() }),
            Response::State(_)
        ));
    }

    fn select(&mut self, anchor: u64, focus: u64) {
        assert!(matches!(
            self.send(Command::SetSelection {
                anchor,
                focus,
                upstream: false
            }),
            Response::State(_)
        ));
    }

    fn state(&mut self) -> FormatStateData {
        match self.send(Command::FormatState) {
            Response::FormatState(state) => *state,
            other => panic!("expected a format state, got {other:?}"),
        }
    }

    fn doc(&mut self) -> &mut RichDocument {
        &mut self.editor.active.as_mut().unwrap().doc
    }

    /// Direct character attributes on `start..end` of paragraph `index`.
    fn attrs(&mut self, index: usize, start: usize, end: usize, set: CharAttrs) {
        let para = self.doc().paragraph_ids()[index];
        self.doc()
            .apply(
                &Transaction::new(vec![RichOp::SetCharAttrs {
                    para,
                    start,
                    end,
                    set,
                    clear: CharAttrs::default(),
                }])
                .unwrap(),
            )
            .unwrap();
    }

    fn props(&mut self, index: usize, set: ParaProps) {
        let para = self.doc().paragraph_ids()[index];
        self.doc()
            .apply(
                &Transaction::new(vec![RichOp::SetParaProps {
                    para,
                    set,
                    clear: ParaProps::default(),
                }])
                .unwrap(),
            )
            .unwrap();
    }
}

fn bold() -> CharAttrs {
    CharAttrs {
        bold: Some(true),
        ..CharAttrs::default()
    }
}

fn char_format() -> CharFormat {
    CharFormat {
        base_revision: crate::rich_protocol::NO_REVISION_GUARD,
        scope: FormatScope::WordOrTyping,
        edits: vec![CharEdit {
            attr: CharAttr::Bold,
            op: EditOp::Toggle,
            value: None,
        }],
    }
}

fn para_format() -> ParaFormat {
    ParaFormat {
        base_revision: crate::rich_protocol::NO_REVISION_GUARD,
        edits: vec![ParaEdit {
            attr: ParaAttr::Alignment,
            op: EditOp::Set,
            value: Some(ParaValue::Choice(1)),
        }],
    }
}

#[test]
fn a_blank_document_shows_the_default_formatting() {
    let mut h = Harness::new();
    let s = h.state();
    assert!(s.collapsed && !s.read_only && !s.partial && !s.typing);
    assert_eq!(s.revision, 0);
    let Tri::Value(font) = &s.chars.font else {
        panic!("a font: {:?}", s.chars.font);
    };
    assert!(!font.requested.is_empty() && !font.used.is_empty());
    assert!(matches!(s.chars.half_points, Tri::Value(v) if v >= 2));
    assert_eq!(s.chars.bold, Tri::Value(false));
    assert_eq!(s.chars.underline, Tri::Value(0));
    assert_eq!(s.chars.vertical_align, Tri::Value(0));
    assert_eq!(s.chars.color, Tri::Unset);
    assert_eq!(s.chars.highlight, Tri::Unset);
    assert_eq!(s.chars.style, Tri::Unset);
    assert_eq!(s.paragraph.alignment, Tri::Value(0));
    assert_eq!(s.paragraph.indent_left, Tri::Value(0));
    assert_eq!(s.paragraph.shading, Tri::Unset);
    assert!(s.paragraph.borders.iter().all(|b| *b == Tri::Unset));
}

#[test]
fn a_selection_shows_the_uniform_value_or_mixed() {
    let mut h = Harness::new();
    h.type_text("Hello world");
    h.attrs(0, 0, 5, bold());
    h.attrs(
        0,
        3,
        8,
        CharAttrs {
            italic: Some(true),
            underline: Some(Underline::Wave),
            ..CharAttrs::default()
        },
    );
    h.attrs(
        0,
        6,
        11,
        CharAttrs {
            color: Some(Color { r: 255, g: 0, b: 0 }),
            vertical_align: Some(VerticalAlign::Superscript),
            size_half_points: Some(30),
            ..CharAttrs::default()
        },
    );
    // "Hel": bold, upright, no colour.
    h.select(0, 3);
    let s = h.state();
    assert!(!s.collapsed);
    assert_eq!(s.chars.bold, Tri::Value(true));
    assert_eq!(s.chars.italic, Tri::Value(false));
    assert_eq!(s.chars.underline, Tri::Value(0));
    assert_eq!(s.chars.color, Tri::Unset);
    // "Hello": bold, italic on its tail.
    h.select(0, 5);
    let s = h.state();
    assert_eq!(s.chars.bold, Tri::Value(true));
    assert_eq!(s.chars.italic, Tri::Mixed);
    assert_eq!(s.chars.underline, Tri::Mixed);
    // The whole text: everything but the font is mixed.
    h.select(0, 11);
    let s = h.state();
    assert_eq!(s.chars.bold, Tri::Mixed);
    assert_eq!(s.chars.color, Tri::Mixed);
    assert_eq!(s.chars.half_points, Tri::Mixed);
    assert_eq!(s.chars.vertical_align, Tri::Mixed);
    assert!(matches!(s.chars.font, Tri::Value(_)));
    // A selection made backwards is the same range.
    h.select(11, 0);
    assert_eq!(h.state().chars.bold, Tri::Mixed);
    // "world": red superscript 15 pt.
    h.select(6, 11);
    let s = h.state();
    assert_eq!(s.chars.color, Tri::Value(Rgb { r: 255, g: 0, b: 0 }));
    assert_eq!(s.chars.vertical_align, Tri::Value(1));
    assert_eq!(s.chars.half_points, Tri::Value(30));
    assert_eq!(s.chars.bold, Tri::Value(false));
}

#[test]
fn a_caret_shows_the_character_before_it_and_the_one_after_at_the_start() {
    let mut h = Harness::new();
    h.type_text("ab cd");
    h.attrs(0, 0, 2, bold());
    h.select(2, 2);
    assert_eq!(h.state().chars.bold, Tri::Value(true));
    h.select(3, 3);
    assert_eq!(h.state().chars.bold, Tri::Value(false));
    h.select(0, 0);
    let s = h.state();
    assert!(s.collapsed);
    assert_eq!(s.chars.bold, Tri::Value(true));
    // Multi-byte text: the caret after an accented letter reads that letter.
    h.type_text("é");
    let end = h.state().revision;
    assert!(end > 0);
}

#[test]
fn a_selection_covers_every_paragraph_it_touches() {
    let mut h = Harness::new();
    h.type_text("one\ntwo\nthree");
    h.props(
        1,
        ParaProps {
            alignment: Some(Alignment::Center),
            indent_left: Some(720),
            style: Some("Quote".into()),
            ..ParaProps::default()
        },
    );
    // Paragraph 2 alone.
    h.select(5, 6);
    let s = h.state();
    assert_eq!(s.paragraph.alignment, Tri::Value(1));
    assert_eq!(s.paragraph.indent_left, Tri::Value(720));
    // Paragraphs 1 and 2.
    h.select(1, 6);
    let s = h.state();
    assert_eq!(s.paragraph.alignment, Tri::Mixed);
    assert_eq!(s.paragraph.indent_left, Tri::Mixed);
    assert_eq!(s.paragraph.style, Tri::Mixed);
    // A selection that ends at the very start of paragraph 2 does not touch it.
    h.select(0, 4);
    let s = h.state();
    assert_eq!(s.paragraph.alignment, Tri::Value(0));
    // The default document gives every paragraph an explicit style.
    assert_eq!(s.paragraph.style, Tri::Value("atelier.normal".into()));
    // One that starts at the end of paragraph 1 (its mark is selected) does.
    h.select(3, 5);
    assert_eq!(h.state().paragraph.alignment, Tri::Mixed);
}

#[test]
fn an_absent_font_reports_the_family_that_replaces_it() {
    let mut h = Harness::new();
    h.type_text("text");
    h.attrs(
        0,
        0,
        4,
        CharAttrs {
            font: Some("Zzz Unknown Sans".into()),
            ..CharAttrs::default()
        },
    );
    h.select(0, 4);
    let s = h.state();
    let Tri::Value(font) = &s.chars.font else {
        panic!("a font");
    };
    assert_eq!(font.requested, "Zzz Unknown Sans");
    assert!(font.substituted, "{font:?}");
    assert_ne!(font.used, font.requested);
    // A font the engine has is not a substitution.
    h.attrs(
        0,
        0,
        4,
        CharAttrs {
            font: Some("Carlito".into()),
            ..CharAttrs::default()
        },
    );
    let Tri::Value(font) = h.state().chars.font else {
        panic!("a font");
    };
    assert_eq!(
        (
            font.requested.as_str(),
            font.used.as_str(),
            font.substituted
        ),
        ("Carlito", "Carlito", false)
    );
}

#[test]
fn format_state_changes_nothing_and_is_idempotent() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(0, 3);
    let (before, can_undo) = {
        let s = h.editor.active.as_ref().unwrap();
        (s.doc.revision(), s.doc.can_undo())
    };
    let first = h.state();
    let second = h.state();
    assert_eq!(first, second);
    let s = h.editor.active.as_ref().unwrap();
    assert_eq!((s.doc.revision(), s.doc.can_undo()), (before, can_undo));
    assert_eq!(first.revision, 1);
}

#[test]
fn the_stubs_answer_the_documented_error_and_change_nothing() {
    let mut h = Harness::new();
    h.type_text("Hello");
    let revision = h.state().revision;
    assert_eq!(
        h.error(Command::ApplyCharFormat(char_format())),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::ApplyParaFormat(para_format())),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::ListStyles {
            filter: StyleFilter::All,
            used_only: false
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: crate::rich_protocol::NO_REVISION_GUARD,
            target: StyleTarget::Paragraph,
            id: "Heading1".into()
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(
        h.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: true,
            id: "Heading1".into()
        }),
        ErrorCode::FormatNotImplemented
    );
    assert_eq!(h.state().revision, revision);
    // The session is still usable.
    h.type_text("!");
    assert_eq!(h.state().revision, revision + 1);
}

#[test]
fn a_read_only_document_refuses_changes_and_still_answers_the_state() {
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/revisions.docx"
    ))
    .unwrap();
    assert!(matches!(
        h.send(Command::Open {
            kind: OpenKind::Docx,
            profile: Profile::Document,
            data: docx
        }),
        Response::State(s) if s.read_only
    ));
    h.select(0, 3);
    let s = h.state();
    assert!(s.read_only && !s.collapsed);
    assert!(matches!(s.chars.bold, Tri::Value(_) | Tri::Mixed));
    assert_eq!(
        h.error(Command::ApplyCharFormat(char_format())),
        ErrorCode::ReadOnly
    );
    assert_eq!(
        h.error(Command::ApplyParaFormat(para_format())),
        ErrorCode::ReadOnly
    );
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: 0,
            target: StyleTarget::Paragraph,
            id: "Heading1".into()
        }),
        ErrorCode::ReadOnly
    );
    assert_eq!(
        h.error(Command::PreviewStyle {
            target: StyleTarget::Paragraph,
            show: true,
            id: "Heading1".into()
        }),
        ErrorCode::ReadOnly
    );
}

#[test]
fn a_styled_docx_reports_resolved_values_and_direct_styles() {
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/basic.docx"
    ))
    .unwrap();
    assert!(matches!(
        h.send(Command::Open {
            kind: OpenKind::Docx,
            profile: Profile::Document,
            data: docx
        }),
        Response::State(_)
    ));
    let len = h.editor.active.as_ref().unwrap().doc.len() as u64;
    h.select(0, len);
    let s = h.state();
    // Whatever the document holds, the answer is complete and consistent.
    assert!(!matches!(s.chars.font, Tri::Unset), "{:?}", s.chars.font);
    assert!(!matches!(s.chars.half_points, Tri::Unset));
    assert!(matches!(s.paragraph.alignment, Tri::Value(_) | Tri::Mixed));
    let frame = proto::encode_reply(&proto::Reply {
        doc_id: 1,
        request_id: 1,
        response: Response::FormatState(Box::new(s.clone())),
    })
    .unwrap();
    let Response::FormatState(back) = proto::decode_reply(&frame).unwrap().response else {
        panic!("a format state");
    };
    assert_eq!(*back, s);
}

#[test]
fn a_huge_selection_is_answered_partially_not_refused() {
    let mut h = Harness::new();
    // More runs than the scan limit: 240 paragraphs of 50 alternating bold
    // and plain letters (100 pieces each).
    let paragraphs = crate::rich_protocol::MAX_STATE_RUNS / 100 + 40;
    let line = "ab".repeat(50);
    h.type_text(&vec![line; paragraphs].join(
        "
",
    ));
    let ids = h.doc().paragraph_ids();
    assert_eq!(ids.len(), paragraphs);
    let ops: Vec<RichOp> = ids
        .iter()
        .flat_map(|para| {
            (0..50).map(|i| RichOp::SetCharAttrs {
                para: *para,
                start: i * 2,
                end: i * 2 + 1,
                set: bold(),
                clear: CharAttrs::default(),
            })
        })
        .collect();
    for chunk in ops.chunks(1000) {
        h.doc()
            .apply(&Transaction::new(chunk.to_vec()).unwrap())
            .unwrap();
    }
    let end = h.doc().len() as u64;
    h.select(0, end);
    let s = h.state();
    assert!(s.partial);
    assert_eq!(s.chars.bold, Tri::Mixed);
    // A selection of the first paragraphs only is complete.
    h.select(0, 100);
    assert!(!h.state().partial);
}

#[test]
fn a_plain_text_document_reports_it_and_refuses_formatting() {
    let mut h = Harness::new();
    assert!(matches!(
        h.send(Command::Open {
            kind: OpenKind::Txt,
            profile: Profile::Txt,
            data: b"plain text".to_vec()
        }),
        Response::State(_)
    ));
    h.select(0, 5);
    let s = h.state();
    assert!(s.plain && !s.read_only);
    assert_eq!(
        h.error(Command::ApplyCharFormat(char_format())),
        ErrorCode::FormatPlainText
    );
    assert_eq!(
        h.error(Command::ApplyParaFormat(para_format())),
        ErrorCode::FormatPlainText
    );
    assert_eq!(
        h.error(Command::ApplyStyle {
            base_revision: 0,
            target: StyleTarget::Paragraph,
            id: "Heading1".into()
        }),
        ErrorCode::FormatPlainText
    );
    // A rich document is not plain.
    let mut rich = Harness::new();
    assert!(!rich.state().plain);
}

#[test]
fn the_font_list_holds_the_engine_faces_the_word_aliases_and_the_document_fonts() {
    let mut h = Harness::new();
    h.type_text("text");
    h.attrs(
        0,
        0,
        4,
        CharAttrs {
            font: Some("Verdana".into()),
            ..CharAttrs::default()
        },
    );
    let Response::FontList(list) = h.send(Command::ListFonts) else {
        panic!("a font list");
    };
    assert_eq!(list.revision, 1);
    let find = |name: &str| list.families.iter().find(|f| f.name == name).cloned();
    // What the editor registers is offered as it is, with its faces.
    let carlito = find("Carlito").expect("Carlito");
    assert_eq!(carlito.flags & 8, 0);
    assert_eq!(carlito.flags & 3, 3, "bold and italic faces");
    assert!(carlito.substitute.is_empty());
    // A metric alias is drawn with its family.
    let calibri = find("Calibri").expect("Calibri");
    assert_eq!(
        (calibri.flags & 8, calibri.substitute.as_str()),
        (8, "Carlito")
    );
    // Another Word font is drawn with the generic fallback until its faces are registered.
    let arial = find("Arial").expect("Arial");
    assert_eq!(arial.flags & 8, 8);
    assert!(!arial.substitute.is_empty());
    // A font the document names and the engine lacks.
    let verdana = find("Verdana").expect("Verdana");
    assert_eq!(verdana.flags & 12, 12, "used and substituted: {verdana:?}");
    assert!(!verdana.substitute.is_empty());
    // Sorted by name, no duplicate.
    let names: Vec<String> = list
        .families
        .iter()
        .map(|f| f.name.to_ascii_lowercase())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted);
    assert_eq!(h.state().revision, 1);
}

#[test]
fn format_state_names_the_selection_it_describes_and_marks_are_a_stub() {
    let mut h = Harness::new();
    h.type_text("Hello");
    h.select(4, 1);
    let s = h.state();
    assert_eq!((s.anchor, s.focus), (4, 1));
    assert_eq!(
        h.error(Command::FormatMarks { first: 0, count: 1 }),
        ErrorCode::FormatNotImplemented
    );
}
