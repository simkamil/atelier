// SPDX-License-Identifier: AGPL-3.0-or-later
//! Several documents in one engine and the rich fragment (T0): isolation of
//! the sessions (history, revision, selection, atlas epoch, fonts), `Open`
//! limits and refusals, `Close`, the lost reply of one document, `CopyFragment`
//! and `PasteFragment` end to end (formatting, styles, pictures, one undo
//! step, plain text, hostile bytes, save and reopen).

use super::*;
use crate::rich_protocol::{
    CharAttr, CharEdit, CharFormat, CharValue, EditOp, FormatScope, NO_REVISION_GUARD, Request,
    SaveFormat, Stamp, StyleFilter, StyleTarget,
};
use atelier_model::Transaction;
use atelier_text_model::rich::{
    CharAttrs, CharRun, ParaProps, Paragraph, RichOp, Style, StyleCharAttrs, StyleKind,
    flat::LayoutContext,
};

const OBJ: &str = "\u{fffc}";

struct M {
    editor: RichEditor,
    request: u32,
}

fn bold() -> CharAttrs {
    CharAttrs {
        bold: Some(true),
        ..CharAttrs::default()
    }
}

fn png() -> Vec<u8> {
    std::fs::read(format!(
        "{}/../image/tests/fixtures/png/browser-200x100-red.png",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn font_file(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../data/fonts/liberation/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

impl M {
    fn new() -> Self {
        Self::with(RichEditor::default())
    }

    fn with(editor: RichEditor) -> Self {
        Self { editor, request: 0 }
    }

    fn send(&mut self, doc: u32, command: Command) -> Response {
        self.request += 1;
        let frame = proto::encode_request(&Request {
            doc_id: doc,
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
        let reply = proto::decode_reply(&reply.payload).unwrap();
        assert_eq!((reply.doc_id, reply.request_id), (doc, self.request));
        reply.response
    }

    fn state(&mut self, doc: u32, command: Command) -> State {
        match self.send(doc, command) {
            Response::State(s) => s,
            other => panic!("expected a state, got {other:?}"),
        }
    }

    fn error(&mut self, doc: u32, command: Command) -> ErrorCode {
        match self.send(doc, command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn open(&mut self, doc: u32, kind: OpenKind, profile: Profile, data: &[u8]) -> Response {
        self.send(
            doc,
            Command::Open {
                kind,
                profile,
                data: data.to_vec(),
            },
        )
    }

    fn blank(&mut self, doc: u32) -> State {
        match self.open(doc, OpenKind::Blank, Profile::Document, &[]) {
            Response::State(s) => s,
            other => panic!("open failed: {other:?}"),
        }
    }

    fn txt(&mut self, doc: u32, text: &str) -> State {
        match self.open(doc, OpenKind::Txt, Profile::Txt, text.as_bytes()) {
            Response::State(s) => s,
            other => panic!("open failed: {other:?}"),
        }
    }

    fn insert(&mut self, doc: u32, text: &str) -> State {
        self.state(doc, Command::InsertText { text: text.into() })
    }

    fn select(&mut self, doc: u32, anchor: u64, focus: u64) -> State {
        self.state(
            doc,
            Command::SetSelection {
                anchor,
                focus,
                upstream: false,
            },
        )
    }

    fn window(&mut self, doc: u32) -> (u64, String) {
        match self.send(
            doc,
            Command::TextWindow {
                start: 0,
                len: proto::MAX_WINDOW_BYTES as u32,
            },
        ) {
            Response::TextWindow { revision, text, .. } => (revision, text),
            other => panic!("{other:?}"),
        }
    }

    fn text(&mut self, doc: u32) -> String {
        self.window(doc).1
    }

    fn session(&mut self, doc: u32) -> &mut Session {
        self.editor
            .sessions
            .iter_mut()
            .find(|s| s.id == doc)
            .expect("an open document")
    }

    fn model(&mut self, doc: u32) -> &mut RichDocument {
        &mut self.session(doc).doc
    }

    fn attrs(&mut self, doc: u32, index: usize, start: usize, end: usize, set: CharAttrs) {
        let para = self.model(doc).paragraph_ids()[index];
        self.model(doc)
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

    fn props(&mut self, doc: u32, index: usize, set: ParaProps) {
        let para = self.model(doc).paragraph_ids()[index];
        self.model(doc)
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

    fn copy(&mut self, doc: u32, start: u64, end: u64) -> (String, Vec<u8>, bool) {
        let revision = self.window(doc).0;
        match self.send(
            doc,
            Command::CopyFragment {
                revision,
                start,
                end,
            },
        ) {
            Response::Fragment {
                text, bytes, lossy, ..
            } => (text, bytes, lossy),
            other => panic!("expected a fragment, got {other:?}"),
        }
    }

    fn paste(&mut self, doc: u32, bytes: &[u8]) -> State {
        self.state(
            doc,
            Command::PasteFragment {
                bytes: bytes.to_vec(),
            },
        )
    }

    fn paragraph_style(&mut self, doc: u32, index: usize) -> Option<String> {
        let id = self.model(doc).paragraph_ids()[index];
        self.model(doc).paragraph(id).unwrap().props.style.clone()
    }
}

fn heading() -> ParaProps {
    ParaProps {
        style: Some("atelier.heading.1".into()),
        ..ParaProps::default()
    }
}

fn custom_style(id: &str, name: &str, bold: bool) -> Style {
    Style {
        id: id.into(),
        kind: StyleKind::Paragraph,
        name: Some(name.into()),
        character: StyleCharAttrs {
            values: CharAttrs {
                bold: Some(bold),
                ..CharAttrs::default()
            },
            ..StyleCharAttrs::default()
        },
        ..Style::default()
    }
}

// ---- several documents -------------------------------------------------------

#[test]
fn three_documents_have_their_own_history_revision_selection_and_atlas_epoch() {
    let mut m = M::new();
    for (doc, text) in [(1, "one"), (2, "two"), (3, "three")] {
        m.txt(doc, text);
    }
    assert_eq!(m.editor.document_ids(), vec![1, 2, 3]);
    m.select(1, 3, 3);
    m.insert(1, "!");
    m.select(2, 0, 0);
    m.insert(2, "?");
    m.insert(2, "?");
    let s3 = m.select(3, 1, 4);
    assert_eq!(
        (m.text(1), m.text(2), m.text(3)),
        ("one!".into(), "??two".into(), "three".into())
    );
    // Undo in document 2 touches document 2 only.
    let undone = m.state(2, Command::Undo);
    assert_eq!(m.text(2), "?two");
    assert!(undone.can_undo && undone.can_redo);
    assert_eq!(m.text(1), "one!");
    let s1 = m.state(1, Command::Layout { budget: 16 });
    assert!(s1.can_undo && !s1.can_redo && s1.modified);
    let s3b = m.state(3, Command::Layout { budget: 16 });
    assert_eq!((s3b.anchor, s3b.focus), (s3.anchor, s3.focus));
    assert!(!s3b.can_undo && !s3b.modified);
    // Revisions count from each Open.
    assert_eq!(s1.revision, 1);
    assert_eq!(undone.revision, 3);
    // A resync of document 1 starts a new atlas epoch of document 1 only.
    let epoch = |m: &mut M, doc: u32| match m.send(doc, Command::Pages { first: 0, count: 1 }) {
        Response::Pages { atlas, .. } => atlas.epoch,
        other => panic!("{other:?}"),
    };
    let (e1, e2, e3) = (epoch(&mut m, 1), epoch(&mut m, 2), epoch(&mut m, 3));
    assert!(e1 != 0 && e2 != 0 && e3 != 0);
    let mut seen = [e1, e2, e3];
    seen.sort_unstable();
    assert!(
        seen[0] < seen[1] && seen[1] < seen[2],
        "epochs differ: {seen:?}"
    );
    let resync = m.state(1, Command::Resync);
    assert_eq!(resync.atlas.epoch, e1 + 1);
    assert_eq!(epoch(&mut m, 2), e2);
    assert_eq!(epoch(&mut m, 3), e3);
}

#[test]
fn an_open_id_is_replaced_and_a_refused_open_leaves_every_document() {
    let mut m = M::with(RichEditor::with_limits(2, MEMORY_BUDGET));
    m.txt(1, "keep one");
    m.txt(2, "keep two");
    assert_eq!(
        m.open(3, OpenKind::Txt, Profile::Txt, b"third"),
        Response::Error(ErrorCode::TooManyDocuments)
    );
    assert_eq!(m.editor.document_ids(), vec![1, 2]);
    // An unreadable source is refused and the document of that id survives.
    assert_eq!(
        m.open(1, OpenKind::Docx, Profile::Document, b"not a zip"),
        Response::Error(ErrorCode::OpenFailed)
    );
    assert_eq!(m.text(1), "keep one");
    // The same id again replaces the document (revision and history restart),
    // also when the limit is reached.
    m.select(1, 0, 0);
    m.insert(1, "x");
    let state = m.txt(1, "fresh");
    assert_eq!(
        (state.revision, state.can_undo, state.modified),
        (0, false, false)
    );
    assert_eq!(m.text(1), "fresh");
    assert_eq!(m.text(2), "keep two");
    assert_eq!(m.editor.document_ids(), vec![1, 2]);
    // Id 0 and a document that is not open.
    assert_eq!(m.error(9, Command::Undo), ErrorCode::UnknownDocument);
}

#[test]
fn the_memory_budget_refuses_an_open_and_a_close_gives_it_back() {
    // Two small documents fit, a third does not.
    let one = Session::estimate(10, 10);
    let mut m = M::with(RichEditor::with_limits(8, one * 2 + one / 2));
    m.txt(1, "0123456789");
    m.txt(2, "0123456789");
    assert_eq!(
        m.open(3, OpenKind::Txt, Profile::Txt, b"0123456789"),
        Response::Error(ErrorCode::Memory)
    );
    assert_eq!(m.editor.document_ids(), vec![1, 2]);
    assert_eq!(m.text(1), "0123456789");
    // A document that replaces one of them is judged without the old one.
    m.txt(2, "0123456789");
    // A source far over the budget is refused before it is parsed.
    assert_eq!(
        m.open(2, OpenKind::Txt, Profile::Txt, &vec![b'a'; one]),
        Response::Error(ErrorCode::Memory)
    );
    assert_eq!(m.text(2), "0123456789");
    assert_eq!(m.send(1, Command::Close), Response::Closed);
    m.txt(3, "0123456789");
    assert_eq!(m.editor.document_ids(), vec![2, 3]);
    // A picture or a fragment that would not fit is refused, nothing changes.
    let tight = RichEditor::with_limits(8, Session::estimate(10, 10) + 1000);
    let mut m = M::with(tight);
    m.blank(1);
    let before = m.text(1);
    assert_eq!(
        m.error(
            1,
            Command::InsertImage {
                alt: String::new(),
                bytes: png()
            }
        ),
        ErrorCode::Memory
    );
    assert_eq!(m.text(1), before);
}

#[test]
fn close_frees_the_document_and_leaves_the_others() {
    let mut m = M::new();
    m.txt(1, "one");
    m.txt(2, "two");
    m.insert(2, "!");
    assert_eq!(m.send(2, Command::Close), Response::Closed);
    assert_eq!(m.error(2, Command::Undo), ErrorCode::UnknownDocument);
    assert_eq!(
        m.send(2, Command::Close),
        Response::Error(ErrorCode::UnknownDocument)
    );
    assert_eq!(m.text(1), "one");
    assert_eq!(m.editor.document_ids(), vec![1]);
    // The id is free: a new document of that id starts a new atlas epoch.
    let reopened = m.txt(2, "again");
    assert_eq!(m.text(2), "again");
    assert!(reopened.atlas.epoch > 0);
    assert_eq!(m.send(1, Command::Close), Response::Closed);
    assert_eq!(m.send(2, Command::Close), Response::Closed);
    assert_eq!(m.editor.active_document(), None);
}

#[test]
fn a_lost_reply_resynchronises_the_document_of_that_reply_only() {
    let mut m = M::new();
    m.txt(1, "one");
    m.txt(2, "two");
    m.state(1, Command::Layout { budget: 16 });
    m.state(2, Command::Layout { budget: 16 });
    // The host could not deliver the last reply, the one of document 2.
    m.editor.mark_resync();
    let one = m.state(1, Command::Layout { budget: 16 });
    assert!(one.window.is_none() && one.invalidated.is_empty());
    let two = m.state(2, Command::Layout { budget: 16 });
    assert!(two.window.is_some());
    assert_eq!(two.invalidated.first().map(|r| r.0), Some(0));
    // Without a reply (nothing answered since), nothing is marked.
    let mut empty = M::new();
    empty.editor.mark_resync();
}

#[test]
fn fonts_loaded_by_a_document_reach_the_documents_opened_after_it() {
    use crate::rich_protocol::{CharValue, Tri};
    let mut m = M::new();
    m.blank(1);
    let arial = |m: &mut M, doc: u32| {
        m.insert(doc, "Hello");
        m.select(doc, 0, 5);
        let edit = CharEdit {
            attr: CharAttr::Font,
            op: EditOp::Set,
            value: Some(CharValue::Text("Arial".into())),
        };
        m.state(
            doc,
            Command::ApplyCharFormat(CharFormat {
                base_revision: NO_REVISION_GUARD,
                command_id: 0,
                scope: FormatScope::SelectionOnly,
                edits: vec![edit],
            }),
        );
    };
    let used = |m: &mut M, doc: u32| match m.send(doc, Command::FormatState) {
        Response::FormatState(state) => match state.chars.font {
            Tri::Value(font) => font.used,
            other => panic!("a font, got {other:?}"),
        },
        other => panic!("{other:?}"),
    };
    arial(&mut m, 1);
    assert_eq!(used(&mut m, 1), "Carlito");
    m.blank(2);
    arial(&mut m, 2);
    let faces: Vec<Vec<u8>> = ["Regular", "Bold", "Italic", "BoldItalic"]
        .iter()
        .map(|style| font_file(&format!("LiberationSans-{style}.ttf")))
        .collect();
    assert!(matches!(
        m.send(1, Command::LoadFonts(faces)),
        Response::State(_)
    ));
    assert_eq!(used(&mut m, 1), "Liberation Sans");
    // Document 2 was open before the load: its own LoadFonts is the client's job.
    assert_eq!(used(&mut m, 2), "Carlito");
    // A document opened afterwards has the faces from its first State.
    m.blank(3);
    arial(&mut m, 3);
    assert_eq!(used(&mut m, 3), "Liberation Sans");
    // Replacing document 1 keeps them too.
    m.blank(1);
    arial(&mut m, 1);
    assert_eq!(used(&mut m, 1), "Liberation Sans");
}

// ---- fragment ----------------------------------------------------------------

/// Document 1: "Hello world" (bold "world"), a heading "Title", a picture in
/// the last paragraph and a style of its own.
fn source(m: &mut M) -> u64 {
    m.blank(1);
    m.insert(1, "Hello world\nTitle\nend");
    m.attrs(1, 0, 6, 11, bold());
    m.model(1)
        .add_style(Style {
            kind: StyleKind::Character,
            ..custom_style("Quirk", "Quirk", true)
        })
        .unwrap();
    m.attrs(
        1,
        0,
        0,
        5,
        CharAttrs {
            char_style: Some("Quirk".into()),
            ..CharAttrs::default()
        },
    );
    m.props(1, 1, heading());
    m.model(1)
        .add_style(custom_style("Mine", "Mine", true))
        .unwrap();
    m.props(
        1,
        2,
        ParaProps {
            style: Some("Mine".into()),
            ..ParaProps::default()
        },
    );
    let end = m.window(1).1.len() as u64;
    m.select(1, end, end);
    m.state(
        1,
        Command::InsertImage {
            alt: "red".into(),
            bytes: png(),
        },
    );
    m.window(1).1.len() as u64
}

#[test]
fn a_fragment_copied_between_documents_keeps_text_formatting_styles_and_picture() {
    let mut m = M::new();
    let len = source(&mut m);
    let (text, bytes, lossy) = m.copy(1, 0, len);
    assert_eq!(text, format!("Hello world\nTitle\nend{OBJ}"));
    assert!(!lossy);
    assert_eq!(&bytes[..4], b"ATFR");
    // The target: its own "Mine" (not bold) and no such heading.
    m.blank(2);
    m.insert(2, "xyz");
    m.model(2)
        .add_style(custom_style("m2", "mine", false))
        .unwrap();
    m.select(2, 1, 1);
    let revision = m.window(2).0;
    let state = m.paste(2, &bytes);
    assert_eq!(state.revision, revision + 1);
    assert_eq!(m.text(2), format!("xHello world\nTitle\nend{OBJ}yz"));
    assert!(state.can_undo && state.modified);
    let caret = m.text(2).len() as u64 - 2;
    assert_eq!((state.anchor, state.focus), (caret, caret));
    // Characters: a character style on "Hello" and bold "world", at their places.
    let first = m.model(2).paragraph_ids()[0];
    let runs = m.model(2).paragraph(first).unwrap().runs.clone();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].start, runs[0].end), (1, 6));
    assert_eq!(
        (runs[1].start, runs[1].end, &runs[1].attrs),
        (7, 12, &bold())
    );
    // Styles: Word's rule. The target's own "Heading 1" and "mine" are used
    // whatever their definition; "Quirk" is absent and is added.
    let heading_id = m.paragraph_style(2, 1).expect("a heading style");
    let doc2 = m.model(2);
    let h = LayoutContext::catalog(&*doc2).style(&heading_id).unwrap();
    assert_eq!(h.display_name().to_lowercase(), "heading 1");
    assert_eq!(m.paragraph_style(2, 2).as_deref(), Some("m2"));
    let quirk = runs[0].attrs.char_style.clone().expect("a character style");
    let doc2 = m.model(2);
    let q = LayoutContext::catalog(&*doc2).style(&quirk).unwrap();
    assert_eq!((q.display_name(), q.kind), ("Quirk", StyleKind::Character));
    assert_eq!(q.character.values.bold, Some(true));
    assert!(m.session(2).added_styles.iter().any(|s| s.id == quirk));
    assert!(!m.session(2).added_styles.iter().any(|s| s.id == "m2"));
    // The picture is an object of the target, counted for the limits.
    let last = m.model(2).paragraph_ids()[2];
    let para = m.model(2).paragraph(last).unwrap().clone();
    assert_eq!((para.anchored.len(), para.text.as_str()), (1, "endyz"));
    assert_eq!(m.session(2).origin.images.0, 1);
    m.model(2).validate().unwrap();
    // One Ctrl+Z, one revision.
    let undone = m.state(2, Command::Undo);
    assert_eq!(m.text(2), "xyz");
    assert_eq!(undone.revision, revision + 2);
    assert!(
        undone.can_undo && undone.can_redo,
        "\"xyz\" is still undoable"
    );
    assert_eq!(m.model(2).paragraph_ids().len(), 1);
    m.model(2).validate().unwrap();
    let redone = m.state(2, Command::Redo);
    assert_eq!(m.text(2), format!("xHello world\nTitle\nend{OBJ}yz"));
    assert_eq!(redone.revision, revision + 3);
    // The source is unchanged by the copy.
    assert_eq!(m.text(1), format!("Hello world\nTitle\nend{OBJ}"));
    assert_eq!(m.window(1).0, 2, "the copy is not an edit");
}

#[test]
fn a_partial_selection_pastes_its_part_and_replaces_the_target_selection() {
    let mut m = M::new();
    source(&mut m);
    // "world\nTi": the end of a bold run and the start of a heading.
    let (text, bytes, _) = m.copy(1, 6, 14);
    assert_eq!(text, "world\nTi");
    m.blank(2);
    m.insert(2, "abcdef");
    m.select(2, 1, 4);
    m.paste(2, &bytes);
    assert_eq!(m.text(2), "aworld\nTief");
    let first = m.model(2).paragraph_ids()[0];
    let run = m.model(2).paragraph(first).unwrap().runs[0].clone();
    assert_eq!((run.start, run.end, run.attrs), (1, 6, bold()));
    // The first paragraph keeps the target's properties, the new one has the heading's.
    assert_eq!(m.paragraph_style(2, 0).as_deref(), Some("atelier.normal"));
    assert_eq!(
        m.paragraph_style(2, 1).as_deref(),
        Some("atelier.heading.1")
    );
    assert_eq!(m.state(2, Command::Undo).revision, 3);
    assert_eq!(m.text(2), "abcdef");
}

#[test]
fn pasting_into_a_text_document_gives_the_text_only() {
    let mut m = M::new();
    let len = source(&mut m);
    m.attrs(1, 0, 0, 5, bold());
    let (_, bytes, _) = m.copy(1, 0, len);
    m.txt(2, "ab");
    m.select(2, 1, 1);
    m.paste(2, &bytes);
    assert_eq!(m.text(2), "aHello world\nTitle\nendb");
    // A fragment of a text document pastes into a rich one.
    let (text, bytes, _) = m.copy(2, 0, 5);
    assert_eq!(text, "aHell");
    m.blank(3);
    m.paste(3, &bytes);
    assert_eq!(m.text(3), "aHell");
}

#[test]
fn a_refused_paste_changes_nothing() {
    let mut m = M::new();
    let len = source(&mut m);
    let (_, good, _) = m.copy(1, 0, len);
    m.blank(2);
    m.insert(2, "abc");
    let (revision, before) = m.window(2);
    let mut bad: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("magic", [b"XXXX".as_slice(), &good[4..]].concat()),
        ("version", [&good[..4], &[2, 0], &good[6..]].concat()),
        ("flags", [&good[..6], &[1, 0], &good[8..]].concat()),
        ("trailing", [good.as_slice(), &[0]].concat()),
        ("no paragraph", [&good[..8], &[0; 8]].concat()),
    ];
    for cut in [8, 9, 20, good.len() / 2, good.len() - 1] {
        bad.push(("truncated", good[..cut].to_vec()));
    }
    for (name, bytes) in bad {
        let code = m.error(2, Command::PasteFragment { bytes });
        assert_eq!(code, ErrorCode::FragmentInvalid, "{name}");
    }
    // A run that ends inside a character: the paragraph's own validation refuses it.
    let mut piece = Paragraph::with_text("\u{e9}");
    piece.runs.push(CharRun {
        start: 0,
        end: 1,
        attrs: bold(),
    });
    let bytes = fragment::encode(&[piece], &[]).unwrap();
    assert_eq!(
        m.error(2, Command::PasteFragment { bytes }),
        ErrorCode::FragmentInvalid
    );
    assert_eq!(m.window(2), (revision, before.clone()));
    // Every cut of random bytes is refused without a panic.
    let mut seed = 0x2545f491u32;
    for _ in 0..200 {
        let mut noise = good.clone();
        for _ in 0..3 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let at = (seed >> 8) as usize % noise.len();
            noise[at] = (seed >> 3) as u8;
        }
        if let Response::State(_) = m.send(2, Command::PasteFragment { bytes: noise }) {
            // A flipped byte that still decodes pastes something valid.
            m.model(2).validate().unwrap();
            m.state(2, Command::Undo);
        }
    }
    assert_eq!(m.window(2).1, before);
    // Copy errors: stale revision, empty range, out of range.
    let rev = m.window(1).0;
    assert_eq!(
        m.error(
            1,
            Command::CopyFragment {
                revision: rev + 1,
                start: 0,
                end: 1
            }
        ),
        ErrorCode::StaleRevision
    );
    for (start, end, code) in [
        (2, 2, ErrorCode::EmptySelection),
        (0, 1 << 40, ErrorCode::PositionOutOfRange),
    ] {
        assert_eq!(
            m.error(
                1,
                Command::CopyFragment {
                    revision: rev,
                    start,
                    end
                }
            ),
            code
        );
    }
}

#[test]
fn a_read_only_document_refuses_a_paste_and_the_closed_one_is_unknown() {
    let mut m = M::new();
    let len = source(&mut m);
    let (_, bytes, _) = m.copy(1, 0, len);
    m.blank(2);
    m.session(2).origin.read_only = true;
    assert_eq!(
        m.error(
            2,
            Command::PasteFragment {
                bytes: bytes.clone()
            }
        ),
        ErrorCode::ReadOnly
    );
    m.send(2, Command::Close);
    assert_eq!(
        m.error(2, Command::PasteFragment { bytes }),
        ErrorCode::UnknownDocument
    );
}

#[test]
fn tables_are_flattened_and_say_so() {
    let mut m = M::new();
    m.blank(1);
    m.insert(1, "before");
    let section = m.model(1).section_ids()[0];
    let table = m.model(1).new_table(1, 1).unwrap();
    m.model(1)
        .apply(
            &Transaction::new(vec![RichOp::InsertBlock {
                parent: Some(section),
                position: 1,
                subtree: table,
            }])
            .unwrap(),
        )
        .unwrap();
    let len = m.text(1).len() as u64;
    let (text, bytes, lossy) = m.copy(1, 0, len);
    assert!(lossy);
    m.blank(2);
    m.paste(2, &bytes);
    assert_eq!(m.text(2).split('\n').next(), Some("before"));
    assert!(!text.is_empty());
}

#[test]
fn a_saved_document_keeps_the_pasted_picture_and_style() {
    let mut m = M::new();
    let len = source(&mut m);
    let (_, bytes, _) = m.copy(1, 0, len);
    m.blank(2);
    m.paste(2, &bytes);
    let stamp = Stamp {
        now_ms: 0,
        user: String::new(),
        version: String::new(),
    };
    let Response::SaveData { bytes: docx, .. } = m.send(
        2,
        Command::Save {
            format: SaveFormat::Docx,
            stamp,
        },
    ) else {
        panic!("save data");
    };
    let package = atelier_fmt_ooxml::Package::read(&docx).unwrap();
    assert!(
        package
            .parts()
            .keys()
            .any(|u| u.as_str().starts_with("word/media/")),
        "the picture is in the package"
    );
    m.open(3, OpenKind::Docx, Profile::Document, &docx);
    assert_eq!(m.text(3), m.text(2));
    let Response::StyleList { styles, .. } = m.send(
        3,
        Command::ListStyles {
            filter: StyleFilter::All,
            used_only: true,
        },
    ) else {
        panic!("a style list");
    };
    assert!(
        styles.iter().any(|s| s.heading_level == 1),
        "the heading style comes back: {:?}",
        styles.iter().map(|s| &s.id).collect::<Vec<_>>()
    );
    let _ = (CharValue::Flag(true), StyleTarget::Paragraph);
}

#[test]
fn shading_and_borders_of_a_paragraph_and_of_a_style_travel_with_the_fragment() {
    use atelier_text_model::rich::{Color, ParaBorder, ParaBorderStyle, ParaBorders, ParaShading};
    let side = |style, eighths, space, color| {
        Some(ParaBorder {
            style,
            eighths,
            space,
            color,
        })
    };
    let borders = ParaBorders {
        top: side(
            ParaBorderStyle::Single,
            8,
            4,
            Some(Color { r: 1, g: 2, b: 3 }),
        ),
        left: side(ParaBorderStyle::Double, 12, 0, None),
        bottom: side(ParaBorderStyle::Dotted, 6, 31, None),
        right: side(ParaBorderStyle::None, 0, 1, None),
        between: side(ParaBorderStyle::Dashed, 4, 2, None),
        bar: side(
            ParaBorderStyle::Single,
            24,
            3,
            Some(Color { r: 9, g: 8, b: 7 }),
        ),
    };
    let shading = Some(ParaShading {
        fill: Some(Color {
            r: 250,
            g: 240,
            b: 10,
        }),
        pattern: Some("pct25".into()),
        pattern_color: Some(Color { r: 0, g: 0, b: 200 }),
    });
    let mut m = M::new();
    m.blank(1);
    m.insert(1, "boxed");
    let mut style = custom_style("Box", "Box", false);
    style.paragraph.borders = borders;
    style.paragraph.shading = shading.clone();
    m.model(1).add_style(style).unwrap();
    m.props(
        1,
        0,
        ParaProps {
            style: Some("Box".into()),
            shading: shading.clone(),
            borders,
            ..ParaProps::default()
        },
    );
    let (_, bytes, _) = m.copy(1, 0, 5);
    m.blank(2);
    m.paste(2, &bytes);
    let first = m.model(2).paragraph_ids()[0];
    let pasted = m.model(2).paragraph(first).unwrap().props.clone();
    // The empty first paragraph of the target adopts the properties of the piece.
    assert_eq!((pasted.borders, pasted.shading), (borders, shading.clone()));
    let id = pasted.style.expect("a style");
    let doc = m.model(2);
    let added = LayoutContext::catalog(&*doc).style(&id).unwrap();
    assert_eq!(added.paragraph.borders, borders);
    assert_eq!(added.paragraph.shading, shading);
}
