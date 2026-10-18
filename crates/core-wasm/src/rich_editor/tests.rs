// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the rich editor session: every command through the real v2 codec
//! (`encode_request` / `decode_reply`), no private shortcut except to build a
//! document with inline items.

use super::*;
use crate::rich_protocol::{Request, inline_kind, role};
use atelier_model::Transaction;
use atelier_text_model::rich::{AnchoredObject, BreakKind, Inline, InlineBreak, RichOp};

const OBJ: &str = "\u{fffc}";

struct Harness {
    editor: RichEditor,
    request: u32,
}

impl Harness {
    fn new() -> Self {
        Self {
            editor: RichEditor::default(),
            request: 0,
        }
    }

    fn raw(&mut self, payload: Vec<u8>) -> Message {
        self.editor.respond(&Message {
            kind: MessageType::Editor,
            request_id: 7,
            payload,
        })
    }

    fn send(&mut self, doc: u32, command: Command) -> Response {
        self.request += 1;
        let frame = proto::encode_request(&Request {
            doc_id: doc,
            request_id: self.request,
            command,
        })
        .unwrap();
        let reply = self.raw(frame);
        assert_eq!(reply.kind, MessageType::EditorResult);
        assert_eq!(reply.request_id, 7);
        let reply = proto::decode_reply(&reply.payload).unwrap();
        assert_eq!((reply.doc_id, reply.request_id), (doc, self.request));
        reply.response
    }

    fn state(&mut self, command: Command) -> State {
        match self.send(1, command) {
            Response::State(s) => {
                check_state(&s);
                s
            }
            other => panic!("expected a state, got {other:?}"),
        }
    }

    fn error(&mut self, command: Command) -> ErrorCode {
        match self.send(1, command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn open(&mut self, kind: OpenKind, profile: Profile, data: &[u8]) -> Response {
        self.send(
            1,
            Command::Open {
                kind,
                profile,
                data: data.to_vec(),
            },
        )
    }

    fn open_state(&mut self, kind: OpenKind, profile: Profile, data: &[u8]) -> State {
        match self.open(kind, profile, data) {
            Response::State(s) => {
                check_state(&s);
                s
            }
            other => panic!("open failed: {other:?}"),
        }
    }

    fn blank(&mut self) -> State {
        self.open_state(OpenKind::Blank, Profile::Document, &[])
    }

    fn txt(&mut self, text: &str) -> State {
        self.open_state(OpenKind::Txt, Profile::Txt, text.as_bytes())
    }

    fn text(&mut self) -> String {
        match self.send(
            1,
            Command::TextWindow {
                start: 0,
                len: proto::MAX_WINDOW_BYTES as u32,
            },
        ) {
            Response::TextWindow { text, .. } => text,
            other => panic!("{other:?}"),
        }
    }

    fn insert(&mut self, text: &str) -> State {
        self.state(Command::InsertText { text: text.into() })
    }

    fn select(&mut self, anchor: u64, focus: u64) -> State {
        self.state(Command::SetSelection {
            anchor,
            focus,
            upstream: false,
        })
    }

    fn step(&mut self, direction: Direction, extend: bool) -> State {
        self.state(Command::Move { direction, extend })
    }

    fn delete(&mut self, forward: bool, unit: DeleteUnit) -> State {
        self.state(Command::Delete { forward, unit })
    }

    fn undo(&mut self) -> State {
        self.state(Command::Undo)
    }

    fn redo(&mut self) -> State {
        self.state(Command::Redo)
    }

    /// Runs `Layout` until the layout is complete.
    fn finish_layout(&mut self) -> State {
        let mut last = self.state(Command::Layout { budget: 256 });
        for _ in 0..2000 {
            if last.layout_complete {
                return last;
            }
            last = self.state(Command::Layout { budget: 256 });
        }
        panic!("layout never completed");
    }
}

/// Invariants of every `State` (the codec already re-decoded it).
fn check_state(s: &State) {
    assert!(s.anchor <= s.doc_len && s.focus <= s.doc_len);
    assert!(s.pages.len() <= proto::MAX_PAGES_PER_RESPONSE);
    assert!(s.pages.iter().all(|p| p.index < s.page_count));
    assert!(
        s.invalidated
            .iter()
            .all(|(a, b)| a < b && *b <= s.page_count)
    );
    if let Some(c) = s.caret {
        assert!(c.page < s.page_count && c.height > 0.0);
    }
}

/// A document of one paragraph `a<object>b<line break>` (8 flat bytes).
fn object_document() -> RichDocument {
    let mut doc = RichDocument::new();
    let para = doc.paragraph_ids()[0];
    let (object, line) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
    let transaction = Transaction::new(vec![
        RichOp::InsertText {
            para,
            offset: 0,
            text: "ab".into(),
        },
        RichOp::InsertInline {
            para,
            item: Inline::Anchored(AnchoredObject {
                id: object,
                offset: 1,
                kind: "pic".into(),
                data: vec![],
            }),
        },
        RichOp::InsertInline {
            para,
            item: Inline::Break(InlineBreak {
                id: line,
                offset: 2,
                kind: BreakKind::Line,
            }),
        },
    ])
    .unwrap();
    doc.apply(&transaction).unwrap();
    doc
}

fn harness_with(doc: RichDocument) -> Harness {
    let mut h = Harness::new();
    h.editor.active =
        Some(Session::with_document(1, doc, true, io::Origin::new(io::Source::New)).unwrap());
    h
}

#[test]
fn blank_document_opens_with_its_first_page() {
    let mut h = Harness::new();
    let s = h.blank();
    assert_eq!((s.revision, s.doc_len, s.anchor, s.focus), (0, 0, 0, 0));
    assert!(!s.modified && !s.can_undo && !s.can_redo);
    assert!(s.page_count >= 1 && s.layout_complete);
    assert_eq!(s.pages.first().map(|p| p.index), Some(0));
    assert!(s.pages[0].display.starts_with(b"ATDL"));
    assert!(s.caret.is_some());
    assert_eq!(s.window.as_ref().map(|w| w.start), Some(0));
    assert_eq!(h.editor.active_document(), Some(1));
}

#[test]
fn typing_reports_the_changed_page_and_the_flags() {
    let mut h = Harness::new();
    h.blank();
    let s = h.insert("Bonjour");
    assert_eq!((s.revision, s.doc_len, s.anchor, s.focus), (1, 7, 7, 7));
    assert!(s.modified && s.can_undo && !s.can_redo);
    assert_eq!(s.pages.iter().map(|p| p.index).collect::<Vec<_>>(), [0]);
    assert_eq!(s.window.unwrap().text, "Bonjour");
    assert_eq!(h.text(), "Bonjour");
    let s = h.insert(" monde");
    assert_eq!((s.revision, s.doc_len), (2, 13));
}

#[test]
fn multi_paragraph_editing_splits_and_merges() {
    let mut h = Harness::new();
    h.blank();
    let s = h.insert("ab\ncd\nef");
    assert_eq!((s.doc_len, s.focus), (8, 8));
    assert_eq!(h.text(), "ab\ncd\nef");
    // Backspace at the start of "cd" removes the whole separator.
    h.select(3, 3);
    let s = h.delete(false, DeleteUnit::Grapheme);
    assert_eq!((s.doc_len, s.focus, s.revision), (7, 2, 2));
    assert_eq!(h.text(), "abcd\nef");
    // Forward delete at the end of "abcd" merges with the next paragraph.
    h.select(4, 4);
    h.delete(true, DeleteUnit::Grapheme);
    assert_eq!(h.text(), "abcdef");
    // A range across separators merges what it joins.
    h.undo();
    h.undo();
    assert_eq!(h.text(), "ab\ncd\nef");
    let s = h.state(Command::ReplaceRange {
        start: 1,
        end: 7,
        text: "X\nY".into(),
    });
    assert_eq!(h.text(), "aX\nYf");
    assert_eq!((s.anchor, s.focus), (4, 4));
    // Typing over a selection replaces it.
    h.select(0, 2);
    h.insert("Z");
    assert_eq!(h.text(), "Z\nYf");
}

#[test]
fn grapheme_and_word_deletion() {
    let mut h = Harness::new();
    h.blank();
    h.insert("un e\u{301}te\u{301} long");
    // One backspace removes a base letter with its combining mark only at the
    // end of the cluster, never half of it.
    h.select(10, 10);
    let before = h.text();
    h.delete(false, DeleteUnit::Grapheme);
    assert_eq!(
        before.len() - h.text().len(),
        3,
        "e + U+0301 is one cluster"
    );
    h.undo();
    // Ctrl+Backspace deletes back to the start of the previous word.
    let end = h.text().len() as u64;
    h.select(end, end);
    h.delete(false, DeleteUnit::Word);
    assert_eq!(h.text(), "un e\u{301}te\u{301} ");
    // Ctrl+Delete deletes up to the start of the next word.
    h.select(0, 0);
    h.delete(true, DeleteUnit::Word);
    assert_eq!(h.text(), "e\u{301}te\u{301} ");
    // Nothing to delete at the edges: a state, no edit.
    h.select(0, 0);
    let s = h.delete(false, DeleteUnit::Grapheme);
    assert_eq!(s.revision, 5);
}

#[test]
fn inline_objects_count_three_bytes_and_move_as_one_cluster() {
    let mut h = harness_with(object_document());
    assert_eq!(h.text(), format!("a{OBJ}b{OBJ}"));
    h.select(0, 0);
    // a | object | b | break
    let positions: Vec<u64> = (0..4)
        .map(|_| h.step(Direction::Right, false).focus)
        .collect();
    assert_eq!(positions, [1, 4, 5, 8]);
    // A position inside an object is refused, and nothing changes.
    let revision = h.state(Command::Layout { budget: 1 }).revision;
    assert_eq!(
        h.error(Command::ReplaceRange {
            start: 2,
            end: 4,
            text: String::new()
        }),
        ErrorCode::PositionNotBoundary
    );
    // SetSelection snaps back instead of failing.
    let s = h.select(2, 6);
    assert_eq!((s.anchor, s.focus), (1, 5));
    // Backspace removes the whole object (3 bytes); undo brings it back.
    h.select(4, 4);
    let s = h.delete(false, DeleteUnit::Grapheme);
    assert_eq!((s.doc_len, s.focus, s.revision), (5, 1, revision + 1));
    assert_eq!(h.text(), format!("ab{OBJ}"));
    let s = h.undo();
    assert_eq!((s.doc_len, s.revision), (8, revision + 2));
    assert_eq!(h.text(), format!("a{OBJ}b{OBJ}"));
    // A combining mark typed after an object forms its own cluster.
    h.select(4, 4);
    h.insert("\u{301}");
    h.step(Direction::Left, false);
    assert_eq!(h.state(Command::Layout { budget: 1 }).focus, 4);
    // Text typed at the end of the document stays after the break.
    h.select(0, 0);
    let s = h.step(Direction::DocumentEnd, false);
    assert_eq!(s.focus, s.doc_len);
}

#[test]
fn accessible_window_describes_paragraphs_and_objects() {
    let mut h = harness_with(object_document());
    h.select(8, 8);
    h.insert("\nsuite");
    let items = match h.send(1, Command::AccessibleWindow { start: 0, end: 14 }) {
        Response::AccessibleWindow { revision, items } => {
            assert_eq!(revision, 1);
            items
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(items.len(), 2);
    assert_eq!((items[0].para_start, items[0].para_end), (0, 8));
    assert_eq!((items[0].role, items[0].level), (role::PARAGRAPH, 0));
    assert_eq!(items[0].text, format!("a{OBJ}b{OBJ}"));
    assert_eq!(
        items[0]
            .inlines
            .iter()
            .map(|o| (o.offset, o.kind))
            .collect::<Vec<_>>(),
        [(1, inline_kind::OBJECT), (5, inline_kind::LINE_BREAK)]
    );
    assert_eq!((items[1].para_start, items[1].para_end), (9, 14));
    // Only the paragraphs that intersect the range.
    let one =
        |h: &mut Harness, start, end| match h.send(1, Command::AccessibleWindow { start, end }) {
            Response::AccessibleWindow { items, .. } => items,
            other => panic!("{other:?}"),
        };
    assert_eq!(one(&mut h, 9, 9).len(), 1);
    assert_eq!(
        one(&mut h, 8, 8)[0].para_start,
        0,
        "the separator ends paragraph 0"
    );
    assert_eq!(
        one(&mut h, 0, 9).len(),
        1,
        "the separator is not in paragraph 1"
    );
    assert_eq!(one(&mut h, 0, 10).len(), 2);
    assert_eq!(
        h.error(Command::AccessibleWindow { start: 2, end: 4 }),
        ErrorCode::PositionNotBoundary
    );
    assert_eq!(
        h.error(Command::AccessibleWindow { start: 0, end: 99 }),
        ErrorCode::PositionOutOfRange
    );
}

#[test]
fn accessible_window_reads_headings_and_is_capped() {
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/basic.docx"
    ))
    .unwrap();
    let s = h.open_state(OpenKind::Docx, Profile::Document, &docx);
    assert!(
        s.window
            .unwrap()
            .text
            .starts_with("R\u{e9}f\u{e9}rence de titre")
    );
    let items = match h.send(
        1,
        Command::AccessibleWindow {
            start: 0,
            end: s.doc_len,
        },
    ) {
        Response::AccessibleWindow { items, .. } => items,
        other => panic!("{other:?}"),
    };
    assert!(
        items
            .iter()
            .any(|i| i.role == role::HEADING && i.level >= 1)
    );
    // 300 empty paragraphs: at most 256 items, the client continues after.
    h.txt(&"\n".repeat(300));
    let items = match h.send(1, Command::AccessibleWindow { start: 0, end: 300 }) {
        Response::AccessibleWindow { items, .. } => items,
        other => panic!("{other:?}"),
    };
    assert_eq!(items.len(), proto::MAX_ACCESSIBLE_ITEMS);
    assert_eq!(items[255].para_start, 255);
}

#[test]
fn undo_redo_and_mark_saved() {
    let mut h = Harness::new();
    h.txt("abc");
    let s = h.select(3, 3);
    assert!(!s.modified && !s.can_undo);
    // Nothing to undo: an unchanged state, not an error.
    let s = h.undo();
    assert_eq!((s.revision, s.can_undo, s.can_redo), (0, false, false));
    let edited = h.insert("d");
    assert_eq!(edited.revision, 1);
    assert!(edited.modified);
    // MarkSaved with the current revision clears `modified`.
    let s = h.state(Command::MarkSaved { revision: 1 });
    assert!(!s.modified && s.can_undo);
    // A stale revision is refused and changes nothing.
    h.insert("e");
    assert_eq!(
        h.error(Command::MarkSaved { revision: 1 }),
        ErrorCode::StaleRevision
    );
    assert!(h.state(Command::Layout { budget: 1 }).modified);
    // Undo and redo count as edits for the revision; the selection comes back.
    let s = h.undo();
    assert_eq!((s.revision, s.can_redo, s.focus), (3, true, 4));
    assert_eq!(h.text(), "abcd");
    let s = h.undo();
    assert_eq!((s.revision, s.focus, s.can_undo), (4, 3, false));
    assert_eq!(h.text(), "abc");
    let s = h.redo();
    assert_eq!((s.revision, s.doc_len), (5, 4));
    assert_eq!(h.text(), "abcd");
    assert!(s.modified, "back at the saved text, but a later revision");
    // A new edit clears the redo stack.
    h.insert("!");
    assert!(!h.state(Command::Layout { budget: 1 }).can_redo);
}

#[test]
fn moves_follow_the_v1_numbering_and_extend() {
    let mut h = Harness::new();
    h.txt("un deux trois\nquatre cinq\n\nsix");
    h.select(0, 0);
    let end_of = |h: &mut Harness| h.state(Command::Layout { budget: 1 });
    assert_eq!(h.step(Direction::Right, false).focus, 1);
    assert_eq!(h.step(Direction::Left, false).focus, 0);
    assert_eq!(h.step(Direction::WordRight, false).focus, 3);
    assert_eq!(h.step(Direction::WordRight, false).focus, 8);
    assert_eq!(h.step(Direction::WordLeft, false).focus, 3);
    assert_eq!(h.step(Direction::End, false).focus, 13);
    assert_eq!(h.step(Direction::Home, false).focus, 0);
    // Paragraph moves: next start, then start of this one.
    assert_eq!(h.step(Direction::ParagraphDown, false).focus, 14);
    assert_eq!(h.step(Direction::ParagraphDown, false).focus, 26);
    assert_eq!(h.step(Direction::ParagraphDown, false).focus, 27);
    assert_eq!(
        h.step(Direction::ParagraphDown, false).focus,
        27,
        "last paragraph"
    );
    h.select(17, 17);
    assert_eq!(h.step(Direction::ParagraphUp, false).focus, 14);
    assert_eq!(h.step(Direction::ParagraphUp, false).focus, 0);
    // Vertical moves keep the column.
    h.select(4, 4);
    let down = h.step(Direction::Down, false);
    assert!((14..=24).contains(&down.focus));
    assert!(h.step(Direction::Up, false).focus <= 13);
    // Extending keeps the anchor; a plain Left collapses to the start.
    h.select(5, 5);
    let s = h.step(Direction::Right, true);
    assert_eq!((s.anchor, s.focus), (5, 6));
    let s = h.step(Direction::WordRight, true);
    assert_eq!((s.anchor, s.focus), (5, 8));
    let s = h.step(Direction::Left, false);
    assert_eq!((s.anchor, s.focus), (5, 5));
    h.select(9, 4);
    let s = h.step(Direction::Right, false);
    assert_eq!((s.anchor, s.focus), (9, 9), "collapses to the end");
    h.select(9, 4);
    let s = h.step(Direction::Left, false);
    assert_eq!((s.anchor, s.focus), (4, 4), "collapses to the start");
    assert_eq!(h.step(Direction::DocumentEnd, true).focus, 30);
    assert_eq!(h.step(Direction::DocumentStart, false).focus, 0);
    let _ = end_of(&mut h);
}

#[test]
fn hit_test_probes_sets_and_extends() {
    let mut h = Harness::new();
    h.txt("Bonjour tout le monde");
    h.select(0, 0);
    let before = h.state(Command::Layout { budget: 1 });
    let caret = before.caret.unwrap();
    let probe = |h: &mut Harness, mode, x: f64, y: f64| {
        h.send(
            1,
            Command::HitTest {
                page: 0,
                mode,
                x,
                y,
            },
        )
    };
    let (x, y) = (caret.x + 40.0, caret.y + caret.height / 2.0);
    let Response::HitResult { position, page, .. } = probe(&mut h, HitMode::Probe, x, y) else {
        panic!("probe answers a HitResult");
    };
    assert_eq!(page, 0);
    assert!(position > 0 && position < 21);
    // A probe changes nothing.
    let same = h.state(Command::Layout { budget: 1 });
    assert_eq!((same.focus, same.revision), (0, 0));
    let Response::State(s) = probe(&mut h, HitMode::SetCaret, x, y) else {
        panic!("set caret answers a State");
    };
    assert_eq!((s.anchor, s.focus), (position, position));
    let Response::State(s) = probe(&mut h, HitMode::Extend, caret.x + 500.0, y) else {
        panic!("extend answers a State");
    };
    assert_eq!((s.anchor, s.focus), (position, 21));
    assert_eq!(
        match h.send(
            1,
            Command::HitTest {
                page: 40,
                mode: HitMode::Probe,
                x: 0.0,
                y: 0.0
            }
        ) {
            Response::Error(code) => code,
            other => panic!("{other:?}"),
        },
        ErrorCode::PageRange
    );
}

#[test]
fn layout_runs_to_completion_and_pages_follow() {
    let mut h = Harness::new();
    let text: String = (0..900)
        .map(|i| format!("Paragraphe {i} avec assez de mots pour occuper une ligne ou deux."))
        .collect::<Vec<_>>()
        .join("\n");
    let opened = h.txt(&text);
    assert!(
        !opened.layout_complete,
        "the tail paginates in the background"
    );
    let mut counts = vec![opened.page_count];
    let mut last = opened;
    for _ in 0..2000 {
        if last.layout_complete {
            break;
        }
        last = h.state(Command::Layout { budget: 64 });
        counts.push(last.page_count);
    }
    assert!(last.layout_complete, "mise en page terminée");
    assert!(last.page_count > 5);
    // A Pages request for a page that existed all along works.
    match h.send(
        1,
        Command::Pages {
            first: last.page_count - 1,
            count: 16,
        },
    ) {
        Response::Pages {
            page_count, pages, ..
        } => {
            assert_eq!(page_count, last.page_count);
            assert_eq!(pages.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        h.error(Command::Pages {
            first: last.page_count,
            count: 1
        }),
        ErrorCode::PageRange
    );
    // Edit near the end: only a few pages come back.
    h.select(last.doc_len, last.doc_len);
    let s = h.insert("fin");
    assert!(s.pages.len() <= 3, "{} pages sent", s.pages.len());
}

#[test]
fn edits_before_the_laid_out_tail_stay_consistent() {
    let mut h = Harness::new();
    let text: String = (0..600)
        .map(|i| format!("ligne numero {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let opened = h.txt(&text);
    assert!(!opened.layout_complete);
    h.select(0, 0);
    h.insert("debut\n");
    h.select(0, 0);
    h.delete(true, DeleteUnit::Word);
    let done = h.finish_layout();
    assert!(done.page_count >= 1);
    assert!(h.text().starts_with("\nligne numero 0"));
}

#[test]
fn docx_opens_with_the_document_profile() {
    let mut h = Harness::new();
    let docx = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/docx-reference/basic.docx"
    ))
    .unwrap();
    let s = h.open_state(OpenKind::Docx, Profile::Document, &docx);
    assert_eq!(s.revision, 0);
    assert!(s.doc_len > 10 && !s.pages.is_empty());
    let s = h.insert("X");
    assert_eq!((s.revision, s.focus), (1, 1));
    assert!(h.text().starts_with("XR\u{e9}f\u{e9}rence"));
}

#[test]
fn engine_errors_leave_the_state_unchanged() {
    let mut h = Harness::new();
    h.txt("abc\u{e9}");
    h.select(2, 3);
    let stable = h.state(Command::Layout { budget: 1 });
    let unchanged = |h: &mut Harness| {
        let s = h.state(Command::Layout { budget: 1 });
        assert_eq!(
            (s.revision, s.anchor, s.focus, s.doc_len, s.can_undo),
            (
                stable.revision,
                stable.anchor,
                stable.focus,
                stable.doc_len,
                stable.can_undo
            )
        );
    };
    for (command, code) in [
        (
            Command::ReplaceRange {
                start: 0,
                end: 99,
                text: "x".into(),
            },
            ErrorCode::PositionOutOfRange,
        ),
        (
            Command::ReplaceRange {
                start: 4,
                end: 5,
                text: "x".into(),
            },
            ErrorCode::PositionNotBoundary,
        ),
        (
            Command::SetSelection {
                anchor: 0,
                focus: 99,
                upstream: false,
            },
            ErrorCode::PositionOutOfRange,
        ),
        (
            Command::SetSelection {
                anchor: u64::MAX,
                focus: 0,
                upstream: false,
            },
            ErrorCode::PositionOutOfRange,
        ),
        (
            Command::TextWindow { start: 99, len: 4 },
            ErrorCode::PositionOutOfRange,
        ),
        (
            Command::TextWindow { start: 4, len: 4 },
            ErrorCode::PositionNotBoundary,
        ),
        (Command::MarkSaved { revision: 9 }, ErrorCode::StaleRevision),
        (
            Command::Pages {
                first: 99,
                count: 1,
            },
            ErrorCode::PageRange,
        ),
        (
            Command::HitTest {
                page: 99,
                mode: HitMode::SetCaret,
                x: 1.0,
                y: 1.0,
            },
            ErrorCode::PageRange,
        ),
    ] {
        assert_eq!(h.error(command.clone()), code, "{command:?}");
        unchanged(&mut h);
    }
    // Another document id is unknown, and so is any document once closed.
    assert!(matches!(
        h.send(2, Command::Undo),
        Response::Error(ErrorCode::UnknownDocument)
    ));
    assert!(matches!(
        h.send(2, Command::Close),
        Response::Error(ErrorCode::UnknownDocument)
    ));
    unchanged(&mut h);
    assert!(matches!(h.send(1, Command::Close), Response::Closed));
    assert_eq!(h.editor.active_document(), None);
    assert_eq!(h.error(Command::Undo), ErrorCode::UnknownDocument);
}

#[test]
fn a_refused_open_keeps_the_active_document() {
    let mut h = Harness::new();
    h.txt("garde");
    h.insert("!");
    assert!(matches!(
        h.send(
            2,
            Command::Open {
                kind: OpenKind::Docx,
                profile: Profile::Document,
                data: vec![1, 2, 3],
            }
        ),
        Response::Error(ErrorCode::OpenFailed)
    ));
    assert_eq!(h.editor.active_document(), Some(1));
    assert_eq!(h.text(), "!garde");
    // Opening with another id replaces the document.
    let s = match h.send(
        2,
        Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Txt,
            data: vec![],
        },
    ) {
        Response::State(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(s.doc_len, 0);
    assert_eq!(h.editor.active_document(), Some(2));
    assert!(matches!(
        h.send(1, Command::Undo),
        Response::Error(ErrorCode::UnknownDocument)
    ));
}

#[test]
fn txt_with_crlf_and_bom_opens_as_paragraphs() {
    let mut h = Harness::new();
    h.open_state(OpenKind::Txt, Profile::Txt, b"\xef\xbb\xbfseed\r\n\xc3\xa9");
    assert_eq!(h.text(), "seed\n\u{e9}");
}

#[test]
fn invalid_frames_are_answered_without_changing_anything() {
    let mut h = Harness::new();
    h.txt("abc");
    let good = proto::encode_request(&Request {
        doc_id: 1,
        request_id: 5,
        command: Command::Undo,
    })
    .unwrap();
    let failed = |m: Message| {
        assert_eq!(m.kind, MessageType::Failed);
        String::from_utf8(m.payload).unwrap()
    };
    // Unreadable header: the outer Failed carries the error name.
    assert_eq!(failed(h.raw(good[..12].to_vec())), "RICH_INVALID_ENVELOPE");
    let mut zero_doc = good.clone();
    zero_doc[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(failed(h.raw(zero_doc)), "RICH_INVALID_DOCUMENT_ID");
    // Readable header: an Error reply with the same ids.
    let mut text = proto::encode_request(&Request {
        doc_id: 1,
        request_id: 6,
        command: Command::InsertText { text: "a".into() },
    })
    .unwrap();
    *text.last_mut().unwrap() = 0x0d;
    let reply = h.raw(text);
    assert_eq!(reply.kind, MessageType::EditorResult);
    let reply = proto::decode_reply(&reply.payload).unwrap();
    assert_eq!((reply.doc_id, reply.request_id), (1, 6));
    assert_eq!(reply.response, Response::Error(ErrorCode::InvalidText));
    let mut unknown = good;
    unknown[6..8].copy_from_slice(&0x0fffu16.to_le_bytes());
    let reply = proto::decode_reply(&h.raw(unknown).payload).unwrap();
    assert_eq!(reply.response, Response::Error(ErrorCode::UnknownOpcode));
    assert_eq!(h.text(), "abc");
    // Routing rule: version 2 with at least 8 bytes, nothing shorter.
    assert!(is_rich(b"ATED\x02\x00\x01\x01"));
    assert!(!is_rich(b"ATED\x02\x00\x01"));
    assert!(!is_rich(b"ATED\x01\x00\x01\x01"));
    assert!(!is_rich(b"XTED\x02\x00\x01\x01"));
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Random ASCII edits against a shadow string, with undo and redo.
#[test]
fn random_edits_match_a_shadow_text() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut h = Harness::new();
    h.blank();
    let mut shadow = String::new();
    let (mut undo, mut redo): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let pick = |rng: &mut Rng, len: usize| rng.below(len as u64 + 1) as usize;
    for step in 0..400 {
        let len = shadow.len();
        match rng.below(6) {
            0 | 1 => {
                let words = ["a", "bc", "def\n", "\n", " x ", "gh\nij"];
                let text = words[rng.below(6) as usize];
                let (a, b) = (pick(&mut rng, len), pick(&mut rng, len));
                h.select(a as u64, b as u64);
                h.insert(text);
                undo.push(shadow.clone());
                redo.clear();
                shadow.replace_range(a.min(b)..a.max(b), text);
            }
            2 => {
                let (a, b) = (pick(&mut rng, len), pick(&mut rng, len));
                let (a, b) = (a.min(b), a.max(b));
                h.state(Command::ReplaceRange {
                    start: a as u64,
                    end: b as u64,
                    text: "Q\nR".into(),
                });
                {
                    undo.push(shadow.clone());
                    redo.clear();
                }
                shadow.replace_range(a..b, "Q\nR");
            }
            3 => {
                let at = pick(&mut rng, len);
                h.select(at as u64, at as u64);
                let forward = rng.below(2) == 0;
                let before = h.state(Command::Layout { budget: 1 }).revision;
                let s = h.delete(forward, DeleteUnit::Grapheme);
                if s.revision != before {
                    undo.push(shadow.clone());
                    redo.clear();
                    if forward {
                        shadow.remove(at);
                    } else {
                        shadow.remove(at - 1);
                    }
                } else {
                    assert!(if forward { at == len } else { at == 0 });
                }
            }
            4 => {
                let s = h.undo();
                if let Some(previous) = undo.pop() {
                    redo.push(std::mem::replace(&mut shadow, previous));
                } else {
                    assert!(!s.can_undo);
                }
            }
            _ => {
                let s = h.redo();
                if let Some(next) = redo.pop() {
                    undo.push(std::mem::replace(&mut shadow, next));
                } else {
                    assert!(!s.can_redo);
                }
            }
        }
        if step % 25 == 0 {
            assert_eq!(h.text(), shadow, "step {step}");
        }
    }
    assert_eq!(h.text(), shadow);
    h.finish_layout();
}

/// Random commands and random bytes never panic nor break the invariants.
#[test]
fn random_commands_and_frames_never_panic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut h = harness_with(object_document());
    for _ in 0..600 {
        let len = h.state(Command::Layout { budget: 1 }).doc_len;
        let at = rng.below(len + 4);
        let command = match rng.below(14) {
            0 => Command::InsertText {
                text: "é\n".into()
            },
            1 => Command::ReplaceRange {
                start: at,
                end: at + rng.below(6),
                text: "z".into(),
            },
            2 => Command::Delete {
                forward: rng.below(2) == 0,
                unit: if rng.below(2) == 0 {
                    DeleteUnit::Word
                } else {
                    DeleteUnit::Grapheme
                },
            },
            3 => Command::Move {
                direction: [
                    Direction::Left,
                    Direction::Right,
                    Direction::Up,
                    Direction::Down,
                    Direction::Home,
                    Direction::End,
                    Direction::DocumentStart,
                    Direction::DocumentEnd,
                    Direction::PageUp,
                    Direction::PageDown,
                    Direction::WordLeft,
                    Direction::WordRight,
                    Direction::ParagraphUp,
                    Direction::ParagraphDown,
                ][rng.below(14) as usize],
                extend: rng.below(2) == 0,
            },
            4 => Command::SetSelection {
                anchor: at,
                focus: rng.below(len + 4),
                upstream: rng.below(2) == 0,
            },
            5 => Command::HitTest {
                page: rng.below(3) as u32,
                mode: HitMode::Extend,
                x: rng.below(700) as f64,
                y: rng.below(900) as f64,
            },
            6 => Command::Undo,
            7 => Command::Redo,
            8 => Command::TextWindow { start: at, len: 8 },
            9 => Command::Pages { first: 0, count: 2 },
            10 => Command::MarkSaved {
                revision: rng.below(4),
            },
            11 => Command::AccessibleWindow {
                start: at,
                end: at + rng.below(8),
            },
            12 => Command::Layout {
                budget: 1 + rng.below(8) as u32,
            },
            _ => Command::HitTest {
                page: 0,
                mode: HitMode::Probe,
                x: -5.0,
                y: 1e9,
            },
        };
        if let Response::State(s) = h.send(1, command) {
            check_state(&s);
        }
    }
    // Garbage frames, never a panic, never a change of the active document.
    for _ in 0..2000 {
        let n = rng.below(80) as usize;
        let mut payload: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        if n >= 20 && rng.below(2) == 0 {
            payload[..4].copy_from_slice(b"ATED");
            payload[4..6].copy_from_slice(&[2, 0]);
            payload[6] = 3 + rng.below(0x1d) as u8; // not Open nor Close
            payload[7] = 1;
            payload[8..12].copy_from_slice(&1u32.to_le_bytes());
            let body = (n - 20) as u32;
            payload[16..20].copy_from_slice(&body.to_le_bytes());
        }
        let _ = h.raw(payload);
    }
    assert_eq!(h.editor.active_document(), Some(1));
}

#[test]
fn symbols_and_page_breaks_edit_like_other_inline_items() {
    let mut doc = RichDocument::new();
    let para = doc.paragraph_ids()[0];
    let (symbol, page) = (doc.alloc_id().unwrap(), doc.alloc_id().unwrap());
    let transaction = Transaction::new(vec![
        RichOp::InsertText {
            para,
            offset: 0,
            text: "xy".into(),
        },
        RichOp::InsertInline {
            para,
            item: Inline::Symbol(atelier_text_model::rich::InlineSymbol {
                id: symbol,
                offset: 1,
                font: "Symbol".into(),
                code: 0xf0b7,
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
    .unwrap();
    doc.apply(&transaction).unwrap();
    let mut h = harness_with(doc);
    assert_eq!(h.text(), format!("x{OBJ}y{OBJ}"));
    h.select(4, 4);
    h.delete(false, DeleteUnit::Grapheme);
    assert_eq!(h.text(), format!("xy{OBJ}"));
    h.undo();
    assert_eq!(h.text(), format!("x{OBJ}y{OBJ}"));
    h.select(8, 8);
    h.delete(false, DeleteUnit::Word);
    assert_eq!(h.text(), format!("x{OBJ}y"));
    h.undo();
    let items = match h.send(1, Command::AccessibleWindow { start: 0, end: 8 }) {
        Response::AccessibleWindow { items, .. } => items,
        other => panic!("{other:?}"),
    };
    let inlines: Vec<(u64, u8, &str)> = items[0]
        .inlines
        .iter()
        .map(|i| (i.offset, i.kind, i.value.as_str()))
        .collect();
    // F0B7 shows as its low byte, U+00B7.
    assert_eq!(
        inlines,
        [
            (1, inline_kind::SYMBOL, "·"),
            (5, inline_kind::PAGE_BREAK, "")
        ]
    );
}

/// `ab`, a 1 x 2 table, an opaque block, `cd`.
fn table_document() -> RichDocument {
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let table = doc.new_table(1, 2).unwrap();
    let opaque = doc.new_opaque("w:sdt", &[1]).unwrap();
    let last = doc.new_paragraph("cd").unwrap();
    let first = doc.paragraph_ids()[0];
    let mut ops = vec![RichOp::InsertText {
        para: first,
        offset: 0,
        text: "ab".into(),
    }];
    for (position, subtree) in [(1, table), (2, opaque), (3, last)] {
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree,
        });
    }
    doc.apply(&Transaction::new(ops).unwrap()).unwrap();
    doc
}

#[test]
fn separators_between_blocks_of_different_containers_are_not_deletable() {
    let mut h = harness_with(table_document());
    let text = format!("ab\n\n\n{OBJ}\ncd");
    assert_eq!(h.text(), text);
    // Backspace and Delete on the separators around the table and the opaque
    // block: no edit, no revision, and the caret stays where it was.
    for (forward, positions) in [(false, [3u64, 4, 5, 9]), (true, [2, 3, 4, 8])] {
        for at in positions {
            h.select(at, at);
            let s = h.delete(forward, DeleteUnit::Grapheme);
            assert_eq!((s.revision, s.focus, s.doc_len), (0, at, 11), "at {at}");
            assert!(!s.can_undo);
        }
    }
    // Words too.
    h.select(3, 3);
    assert_eq!(h.delete(false, DeleteUnit::Word).revision, 0);
    assert_eq!(h.text(), text);
    // Typing in a cell and in the paragraph after the opaque block works.
    h.select(3, 3);
    h.insert("X");
    h.select(12, 12);
    h.insert("!");
    assert_eq!(h.text(), format!("ab\nX\n\n{OBJ}\ncd!"));
    // A selection across structure clears the text it covers and keeps the
    // cells, the table and the opaque block.
    h.select(1, 5);
    let s = h.delete(false, DeleteUnit::Grapheme);
    assert_eq!(s.revision, 3);
    assert_eq!(h.text().matches('\n').count(), 4, "{:?}", h.text());
    assert!(h.text().contains(OBJ));
    assert!(h.text().ends_with("cd!"));
    h.undo();
    assert_eq!(h.text(), format!("ab\nX\n\n{OBJ}\ncd!"));
}

#[test]
fn every_direction_with_and_without_extension() {
    let text: String = (0..40)
        .map(|i| format!("Ligne {i} avec assez de mots pour passer a la ligne dans la page"))
        .collect::<Vec<_>>()
        .join("\n");
    for direction in [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
        Direction::Home,
        Direction::End,
        Direction::DocumentStart,
        Direction::DocumentEnd,
        Direction::PageUp,
        Direction::PageDown,
        Direction::WordLeft,
        Direction::WordRight,
        Direction::ParagraphUp,
        Direction::ParagraphDown,
    ] {
        for extend in [false, true] {
            for start in [0u64, 7, 300, 1200] {
                let mut h = Harness::new();
                let opened = h.txt(&text);
                let start = start.min(opened.doc_len);
                h.select(start, start);
                let before = h.step(Direction::Right, true);
                let s = h.step(direction, extend);
                let what = format!("{direction:?} extend={extend} from {start}");
                assert!(s.focus <= s.doc_len && s.anchor <= s.doc_len, "{what}");
                assert_eq!(s.revision, 0, "{what}");
                assert!(s.caret.is_some() || !s.layout_complete, "{what}");
                if extend {
                    assert_eq!(s.anchor, before.anchor, "{what}: the anchor stays");
                } else {
                    assert_eq!(s.anchor, s.focus, "{what}: a plain move collapses");
                }
                // Moves never produce a position inside a cluster.
                h.select(s.focus, s.focus);
            }
        }
    }
}

/// A DOCX of `n` one-line paragraphs, written by the engine's own exporter.
fn docx_of_paragraphs(n: usize) -> Vec<u8> {
    let text: String = (0..n)
        .map(|i| format!("Paragraphe numero {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    crate::docx_export::export_docx(&text, &crate::images::ImageStore::default())
        .map_err(|e| e.0)
        .unwrap()
        .bytes
}

#[test]
fn docx_with_more_than_a_thousand_paragraphs_opens_in_the_editor() {
    let docx = docx_of_paragraphs(2500);
    // The preview keeps its limit of 1024 paragraphs; the editor does not.
    assert!(crate::rich_preview::import_preview_docx(&docx).is_err());
    let mut h = Harness::new();
    let s = h.open_state(OpenKind::Docx, Profile::Document, &docx);
    assert!(s.doc_len > 2500 * 15);
    let done = h.finish_layout();
    assert!(done.page_count > 10);
    h.select(0, 0);
    h.step(Direction::DocumentEnd, false);
    let s = h.insert("fin");
    assert!(s.doc_len > done.doc_len);
    assert!(h.text().contains("Paragraphe numero 2499"));
}

#[test]
fn moves_reach_a_tail_that_is_not_laid_out_yet() {
    let text: String = (0..12_000)
        .map(|i| format!("ligne {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut h = Harness::new();
    let opened = h.txt(&text);
    assert!(!opened.layout_complete);
    // Ctrl+End: the selection moves at once; the caret comes with the layout.
    let end = h.step(Direction::DocumentEnd, false);
    assert_eq!(end.focus, end.doc_len);
    assert!(end.caret.is_some() || !end.layout_complete);
    // Up before the layout is finished: never an error. Each call lays the
    // tail out as far as its budget allows and moves once it can.
    let mut up = h.step(Direction::Up, false);
    let mut tries = 0;
    while up.focus == end.focus {
        tries += 1;
        assert!(tries < 20, "the move never completed");
        up = h.step(Direction::Up, false);
    }
    assert!(up.focus < end.focus);
    assert!(up.caret.is_some());
    // Home, PageUp (and PageDown from the start) work from there.
    let home = h.step(Direction::Home, false);
    assert!(home.focus <= up.focus && home.caret.is_some());
    let page_up = h.step(Direction::PageUp, false);
    assert!(page_up.focus < home.focus);
    assert!(page_up.caret.unwrap().page < up.caret.unwrap().page);
    h.select(0, 0);
    assert!(h.step(Direction::PageDown, false).focus > 0);
    let start = h.step(Direction::DocumentEnd, false);
    assert!(
        start.caret.is_some(),
        "laid out: the caret is there at once"
    );
}

#[test]
fn a_lost_reply_makes_the_next_state_invalidate_everything() {
    let mut h = Harness::new();
    h.txt(&"mot ".repeat(2000));
    let page_count = h.finish_layout().page_count;
    h.insert("x");
    let quiet = h.state(Command::Layout { budget: 1 });
    assert!(quiet.invalidated.is_empty() && quiet.window.is_none());
    h.editor.mark_resync();
    let s = h.state(Command::Layout { budget: 1 });
    assert_eq!(s.invalidated, [(0, page_count)]);
    assert!(s.window.is_some());
    // Only once.
    let again = h.state(Command::Layout { budget: 1 });
    assert!(again.invalidated.is_empty());
}

#[test]
fn a_dense_page_is_sent_within_the_response_bounds() {
    let line = "x".repeat(91);
    let text = vec![line; 100].join("\n");
    let mut h = Harness::new();
    let opened = h.txt(&text);
    let sent: usize = opened.pages.iter().map(|p| p.display.len()).sum();
    assert!(
        opened
            .pages
            .iter()
            .all(|p| p.display.len() <= proto::MAX_PAGE_DISPLAY_BYTES)
    );
    assert!(opened.pages.len() <= 1 || sent <= proto::MAX_RESPONSE_PAGE_BYTES);
    assert!(!opened.pages.is_empty(), "the first page is always sent");
    // The pages left out of the reply are invalidated, then asked for.
    let done = h.finish_layout();
    for index in 0..done.page_count {
        match h.send(
            1,
            Command::Pages {
                first: index,
                count: 1,
            },
        ) {
            Response::Pages { pages, .. } => assert_eq!(pages.len(), 1),
            other => panic!("{other:?}"),
        }
    }
}

/// What a client keeps of the atlas: the epoch and the number of entries.
#[derive(Default)]
struct AtlasView {
    epoch: Option<u32>,
    len: u32,
}

impl AtlasView {
    /// Applies a delta by the client rules; `true` when the epoch changed.
    fn apply(&mut self, atlas: &proto::AtlasDelta) -> bool {
        let fresh = self.epoch != Some(atlas.epoch);
        if fresh {
            self.epoch = Some(atlas.epoch);
            self.len = 0;
        }
        assert_eq!(atlas.first, self.len, "atlas_first follows the known index");
        self.len += atlas.entries.len() as u32;
        fresh
    }

    /// Every glyph of the pages refers to a known entry.
    fn covers(&self, pages: &[PageDisplay]) {
        for page in pages {
            let list = atelier_render::DisplayList::decode(&page.display).unwrap();
            for command in list.commands {
                if let atelier_render::Command::DrawGlyphRun {
                    font_id, glyphs, ..
                } = command
                {
                    assert_eq!(font_id, 0);
                    assert!(glyphs.iter().all(|g| g.glyph_id < self.len));
                }
            }
        }
    }
}

// ---- US-218: open, create and save ---------------------------------------

use crate::rich_protocol::{SaveFormat, Stamp, notice};

fn stamp() -> Stamp {
    Stamp {
        now_ms: 1_700_000_000_000,
        user: "Alice".into(),
        version: "1.2".into(),
    }
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/docx-reference/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

impl Harness {
    fn save(&mut self, format: SaveFormat) -> (Vec<u8>, Vec<proto::Notice>) {
        match self.send(
            1,
            Command::Save {
                format,
                stamp: stamp(),
            },
        ) {
            Response::SaveData { bytes, notices, .. } => (bytes, notices),
            other => panic!("expected SaveData, got {other:?}"),
        }
    }

    fn docx(&mut self, bytes: &[u8]) -> State {
        self.open_state(OpenKind::Docx, Profile::Document, bytes)
    }
}

/// `text` (LF breaks) as a file: line break, encoding, BOM.
fn file_bytes(text: &str, eol: &str, encoding: &str, bom: bool) -> Vec<u8> {
    let text = text.replace('\n', eol);
    let mut out = Vec::new();
    match encoding {
        "utf8" => {
            if bom {
                out.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            }
            out.extend_from_slice(text.as_bytes());
        }
        "utf16le" | "utf16be" => {
            let le = encoding == "utf16le";
            if bom {
                out.extend_from_slice(if le { &[0xff, 0xfe] } else { &[0xfe, 0xff] });
            }
            for unit in text.encode_utf16() {
                out.extend_from_slice(&if le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
        }
        _ => {
            for c in text.chars() {
                out.push(match c {
                    '\u{e9}' => 0xe9,
                    '\u{e8}' => 0xe8,
                    '\u{e0}' => 0xe0,
                    '\u{20ac}' => 0x80,
                    c if c.is_ascii() => c as u8,
                    other => panic!("not in the test table: {other}"),
                });
            }
        }
    }
    out
}

const CORPUS_TEXT: &str =
    "Premier \u{e9}t\u{e9}, \u{e0} 5\u{20ac}\n\nTroisi\u{e8}me ligne\tavec tabulation\nfin";

type TextCase = (String, Vec<u8>, &'static str, bool, &'static str, bool);

fn text_corpus() -> Vec<TextCase> {
    let mut out = Vec::new();
    for (eol_name, eol) in [("lf", "\n"), ("crlf", "\r\n"), ("cr", "\r")] {
        for (encoding, bom) in [
            ("utf8", false),
            ("utf8", true),
            ("utf16le", true),
            ("utf16be", true),
            ("cp1252", false),
        ] {
            for final_break in [false, true] {
                let text = format!("{CORPUS_TEXT}{}", if final_break { "\n" } else { "" });
                out.push((
                    format!("{encoding}-bom{bom}-{eol_name}-final{final_break}"),
                    file_bytes(&text, eol, encoding, bom),
                    encoding,
                    bom,
                    eol,
                    final_break,
                ));
            }
        }
    }
    out
}

#[test]
fn text_files_save_back_byte_for_byte_and_after_an_edit() {
    for (name, bytes, encoding, bom, eol, final_break) in text_corpus() {
        let mut h = Harness::new();
        h.open_state(OpenKind::Txt, Profile::Txt, &bytes);
        let (saved, notices) = h.save(SaveFormat::Txt);
        assert_eq!(saved, bytes, "{name}: unmodified file");
        assert!(notices.is_empty(), "{name}: {notices:?}");
        // Edit: insert after "Premier", then delete the last character.
        h.select(7, 7);
        h.insert("!");
        let end = h.text().len() as u64;
        h.select(end, end);
        h.delete(false, DeleteUnit::Grapheme);
        let mut expected = CORPUS_TEXT.to_owned();
        expected.insert(7, '!');
        if final_break {
            expected.push('\n');
        }
        expected.pop();
        let (saved, notices) = h.save(SaveFormat::Txt);
        assert_eq!(
            saved,
            file_bytes(&expected, eol, encoding, bom),
            "{name}: edited file"
        );
        assert!(notices.is_empty(), "{name}: {notices:?}");
    }
}

#[test]
fn atlas_entries_arrive_once_in_order_and_cover_the_pages() {
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let opened = h.txt("Hello world, hello atlas.");
    assert!(view.apply(&opened.atlas), "the first reply starts an epoch");
    assert!(!opened.atlas.entries.is_empty());
    assert!(
        opened
            .atlas
            .entries
            .iter()
            .all(|e| e.starts_with(b"ATDL") && e.len() < proto::MAX_ATLAS_ENTRY_BYTES)
    );
    view.covers(&opened.pages);
    let known = view.len;
    // A page already painted brings nothing new.
    match h.send(1, Command::Pages { first: 0, count: 1 }) {
        Response::Pages { atlas, pages, .. } => {
            assert!(!view.apply(&atlas));
            assert_eq!((atlas.first, atlas.entries.len()), (known, 0));
            view.covers(&pages);
        }
        other => panic!("{other:?}"),
    }
    // Only the glyphs of a new letter are added by an edit.
    let state = h.insert("Q");
    assert!(!view.apply(&state.atlas));
    assert_eq!(state.atlas.first, known);
    assert!(!state.atlas.entries.is_empty() && state.atlas.entries.len() <= 3);
    view.covers(&state.pages);
    let known = view.len;
    let state = h.insert("Q");
    assert!(!view.apply(&state.atlas));
    assert_eq!(
        state.atlas.entries.len(),
        0,
        "a known glyph is not sent again"
    );
    assert_eq!(view.len, known);
    view.covers(&state.pages);
}

#[test]
fn the_atlas_epoch_changes_at_open_and_at_resync() {
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let first = h.txt(&"mot ".repeat(2000));
    assert!(view.apply(&first.atlas));
    h.finish_layout();
    // Open again: a new epoch and numbering from 0.
    let second = h.txt("autre texte");
    assert_ne!(second.atlas.epoch, first.atlas.epoch);
    assert_eq!(second.atlas.first, 0);
    assert!(view.apply(&second.atlas));
    view.covers(&second.pages);
    // A lost reply: the next State starts a new atlas, with every entry the
    // pages it sends use, and the pages painted before are painted again.
    h.editor.mark_resync();
    let state = h.state(Command::Layout { budget: 1 });
    assert_ne!(state.atlas.epoch, second.atlas.epoch);
    assert_eq!(state.atlas.first, 0);
    assert!(view.apply(&state.atlas));
    assert_eq!(state.invalidated.len(), usize::from(state.page_count > 0));
    match h.send(1, Command::Pages { first: 0, count: 1 }) {
        Response::Pages { atlas, pages, .. } => {
            assert_eq!(atlas.epoch, state.atlas.epoch);
            assert!(!view.apply(&atlas));
            view.covers(&pages);
        }
        other => panic!("{other:?}"),
    }
    // Closed and opened again: never the epoch of the closed document.
    let last = view.epoch;
    assert!(matches!(h.send(1, Command::Close), Response::Closed));
    let third = h.txt("encore");
    assert_ne!(Some(third.atlas.epoch), last);
}

#[test]
fn a_lost_pages_reply_is_recovered_by_the_epoch() {
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let opened = h.txt(&"Zéro ".repeat(300));
    view.apply(&opened.atlas);
    // The reply to this edit never reaches the client: the host calls
    // mark_resync, the client's atlas (already complete for the old pages)
    // is dropped with the new epoch.
    h.insert("W");
    h.editor.mark_resync();
    let state = h.state(Command::Layout { budget: 1 });
    assert!(view.apply(&state.atlas), "the epoch changed");
    assert_eq!(view.len, state.atlas.entries.len() as u32);
    for index in 0..state.page_count.min(2) {
        if let Response::Pages { atlas, pages, .. } = h.send(
            1,
            Command::Pages {
                first: index,
                count: 1,
            },
        ) {
            assert!(!view.apply(&atlas));
            view.covers(&pages);
        }
    }
}

#[test]
fn mixed_line_breaks_stay_with_their_paragraph_and_unmappable_characters_are_reported() {
    let mut h = Harness::new();
    let original = b"a\r\nb\r\nc\nd\re";
    h.open_state(OpenKind::Txt, Profile::Txt, original);
    let (saved, notices) = h.save(SaveFormat::Txt);
    assert_eq!(saved, original, "each line keeps its own break");
    assert!(notices.is_empty(), "{notices:?}");
    // An edit inside a line changes nothing else; a new paragraph takes the dominant CRLF.
    h.select(1, 1);
    h.insert("!");
    let (saved, _) = h.save(SaveFormat::Txt);
    assert_eq!(saved, b"a!\r\nb\r\nc\nd\re");
    let end = h.text().len() as u64;
    h.select(end, end);
    h.insert("\nf");
    let (saved, _) = h.save(SaveFormat::Txt);
    assert_eq!(saved, b"a!\r\nb\r\nc\nd\re\r\nf");
    // UTF-16 with a BOM and mixed breaks: the BOM only once.
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "x\r\ny\nz".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    h.open_state(OpenKind::Txt, Profile::Txt, &utf16);
    assert_eq!(h.save(SaveFormat::Txt).0, utf16);
    // A Windows-1252 file that gains a character the encoding cannot hold.
    h.open_state(OpenKind::Txt, Profile::Txt, &[b'a', 0xe9, b'\n']);
    h.select(0, 0);
    h.insert("\u{4e2d}");
    let (saved, notices) = h.save(SaveFormat::Txt);
    assert_eq!(saved, [b'?', b'a', 0xe9, b'\n']);
    assert_eq!(
        notices.iter().map(|n| n.code).collect::<Vec<_>>(),
        [notice::REPLACED_CHARACTERS]
    );
}

fn repo_text(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../compress/tests/corpus/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn repository_and_edge_text_files_round_trip() {
    let utf16 = |text: &str, bom: bool| {
        let mut out = if bom { vec![0xff, 0xfe] } else { Vec::new() };
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("mixed-utf8.txt", repo_text("mixed-utf8.txt")),
        ("repetitive.txt", repo_text("repetitive.txt")),
        ("empty", Vec::new()),
        ("bom only", vec![0xef, 0xbb, 0xbf]),
        ("utf16 bom only", vec![0xff, 0xfe]),
        (
            "utf16 without bom",
            utf16("Bonjour le monde, ligne un\nligne deux\n", false),
        ),
        (
            "outside the BMP, utf8",
            "pile \u{1f600} face \u{1d11e}\n".as_bytes().to_vec(),
        ),
        (
            "outside the BMP, utf16",
            utf16("pile \u{1f600} face \u{1d11e}\n", true),
        ),
        ("only breaks", b"\n\n\r\n\r".to_vec()),
    ];
    for (name, bytes) in cases {
        let mut h = Harness::new();
        h.open_state(OpenKind::Txt, Profile::Txt, &bytes);
        let (saved, notices) = h.save(SaveFormat::Txt);
        assert_eq!(saved, bytes, "{name}");
        assert!(notices.is_empty(), "{name}: {notices:?}");
    }
    // Edited: a character typed first, everything else stays byte for byte.
    for name in ["mixed-utf8.txt", "repetitive.txt"] {
        let bytes = repo_text(name);
        let mut h = Harness::new();
        h.open_state(OpenKind::Txt, Profile::Txt, &bytes);
        h.select(0, 0);
        h.insert("!");
        let expected = [b"!".to_vec(), bytes].concat();
        assert_eq!(h.save(SaveFormat::Txt).0, expected, "{name}");
    }
}

#[test]
fn resync_recovers_replies_the_client_abandoned() {
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let opened = h.txt(&format!("{}b", "a\n".repeat(400)));
    view.apply(&opened.atlas);
    let last = h.finish_layout().page_count - 1;
    assert!(last >= 1);
    // A Pages reply the engine produced and the client dropped (cancelled
    // request, stopPump): its entries are never applied to the view.
    match h.send(
        1,
        Command::Pages {
            first: last,
            count: 1,
        },
    ) {
        Response::Pages { .. } => {}
        other => panic!("{other:?}"),
    }
    // The next reply no longer follows what the client holds.
    let gap = h.state(Command::Layout { budget: 1 });
    assert_ne!(gap.atlas.first, view.len, "the gap exists");
    // Resync: new epoch, atlas from 0, everything invalidated, text window.
    let resynced = h.state(Command::Resync);
    assert_ne!(Some(resynced.atlas.epoch), view.epoch);
    assert_eq!(resynced.atlas.first, 0);
    assert_eq!(resynced.invalidated, vec![(0, resynced.page_count)]);
    assert!(resynced.window.is_some());
    assert!(view.apply(&resynced.atlas));
    view.covers(&resynced.pages);
    // Pages and later States follow the new epoch.
    for index in 0..resynced.page_count.min(3) {
        match h.send(
            1,
            Command::Pages {
                first: index,
                count: 1,
            },
        ) {
            Response::Pages { atlas, pages, .. } => {
                assert_eq!(atlas.epoch, resynced.atlas.epoch);
                assert!(!view.apply(&atlas));
                view.covers(&pages);
            }
            other => panic!("{other:?}"),
        }
    }
    let after = h.state(Command::Layout { budget: 1 });
    assert!(!view.apply(&after.atlas));
    view.covers(&after.pages);
    // A Resync whose reply is lost is replayed: idempotent.
    let _lost = h.state(Command::Resync);
    let replay = h.state(Command::Resync);
    assert_ne!(replay.atlas.epoch, resynced.atlas.epoch);
    assert_eq!(replay.atlas.first, 0);
    assert_eq!(replay.invalidated, vec![(0, replay.page_count)]);
    assert!(view.apply(&replay.atlas));
    view.covers(&replay.pages);
    assert_eq!(replay.doc_len, resynced.doc_len);
    assert_eq!(replay.revision, resynced.revision);
}

#[test]
fn resync_needs_a_document() {
    let mut h = Harness::new();
    assert_eq!(h.error(Command::Resync), ErrorCode::UnknownDocument);
    h.txt("abc");
    assert!(matches!(
        h.send(2, Command::Resync),
        Response::Error(ErrorCode::UnknownDocument)
    ));
    assert!(matches!(h.send(1, Command::Close), Response::Closed));
    assert_eq!(h.error(Command::Resync), ErrorCode::UnknownDocument);
}

#[test]
fn a_new_document_saves_as_utf8_crlf_text_and_as_docx() {
    let mut h = Harness::new();
    h.blank();
    h.insert("Bonjour\n\u{e9}t\u{e9}");
    let (text, _) = h.save(SaveFormat::Txt);
    assert_eq!(text, "Bonjour\r\n\u{e9}t\u{e9}".as_bytes());
    for format in [SaveFormat::Docx, SaveFormat::Docm, SaveFormat::Dotx] {
        let (bytes, notices) = h.save(format);
        assert!(notices.is_empty(), "{format:?}: {notices:?}");
        let mut again = Harness::new();
        let state = again.docx(&bytes);
        assert!(!state.read_only && !state.has_revisions);
        assert_eq!(again.text(), "Bonjour\n\u{e9}t\u{e9}");
    }
}

#[test]
fn docx_files_round_trip_without_a_notice() {
    for name in ["basic.docx", "libreoffice/properties.docx"] {
        let bytes = fixture(name);
        let mut h = Harness::new();
        h.docx(&bytes);
        let text = h.text();
        let (saved, notices) = h.save(SaveFormat::Docx);
        assert!(notices.is_empty(), "{name}: {notices:?}");
        let mut again = Harness::new();
        again.docx(&saved);
        assert_eq!(again.text(), text, "{name}");
        // Modified: still no notice for a plain insertion in a text paragraph.
        h.select(0, 0);
        h.insert("Ajout ");
        let (edited, notices) = h.save(SaveFormat::Docx);
        assert!(notices.is_empty(), "{name} edited: {notices:?}");
        let mut again = Harness::new();
        again.docx(&edited);
        assert_eq!(again.text(), format!("Ajout {text}"), "{name}");
    }
}

fn edit_commands() -> Vec<Command> {
    vec![
        Command::InsertText { text: "x".into() },
        Command::ReplaceRange {
            start: 0,
            end: 0,
            text: "x".into(),
        },
        Command::Delete {
            forward: true,
            unit: DeleteUnit::Grapheme,
        },
        Command::Undo,
        Command::Redo,
        Command::InsertImage {
            alt: String::new(),
            bytes: vec![1, 2, 3],
        },
    ]
}

#[test]
fn revisions_and_comments_open_read_only_in_the_accepted_state() {
    for (name, text) in [
        (
            "revisions.docx",
            "D\u{e9}but ins\u{e9}r\u{e9} fin.\n\nd\u{e9}plac\u{e9}\ngras\ncentr\u{e9}\nsuite",
        ),
        ("comments.docx", "Avant comment\u{e9} apr\u{e8}s.\nFin."),
    ] {
        let bytes = fixture(name);
        let mut h = Harness::new();
        let state = h.docx(&bytes);
        assert!(state.read_only && state.has_revisions, "{name}");
        assert_eq!(h.text(), text, "{name}");
        for command in edit_commands() {
            assert_eq!(h.error(command), ErrorCode::ReadOnly, "{name}");
        }
        assert_eq!(h.text(), text, "{name}: nothing changed");
        // Moving and selecting are not edits.
        let moved = h.step(Direction::Right, false);
        assert!(moved.read_only && moved.has_revisions && !moved.modified);
        // Saving the read-only document gives back what was opened.
        let (saved, notices) = h.save(SaveFormat::Docx);
        assert!(notices.is_empty(), "{name}: {notices:?}");
        let mut again = Harness::new();
        assert!(again.docx(&saved).read_only, "{name}");
        assert_eq!(again.text(), text);
        // The editable copy: a DOCX without revisions or comments, with notices.
        let (copy, notices) = match h.send(1, Command::MakeEditableCopy { stamp: stamp() }) {
            Response::SaveData { bytes, notices, .. } => (bytes, notices),
            other => panic!("{other:?}"),
        };
        assert!(!notices.is_empty(), "{name}");
        let mut editable = Harness::new();
        let state = editable.docx(&copy);
        assert!(!state.read_only && !state.has_revisions, "{name}");
        assert_eq!(editable.text(), text, "{name}: same accepted text");
        editable.select(0, 0);
        editable.insert("Z");
        assert_eq!(editable.text(), format!("Z{text}"));
        let (out, notices) = editable.save(SaveFormat::Docx);
        assert!(notices.is_empty(), "{name}: {notices:?}");
        assert!(!out.is_empty());
    }
    // The flags are those of the session just opened, not of the previous one.
    let mut h = Harness::new();
    let state = h.docx(&fixture("revisions.docx"));
    assert!(state.read_only);
    let state = h.docx(&fixture("basic.docx"));
    assert!(!state.read_only && !state.has_revisions);
    h.insert("ok");
}

#[test]
fn make_editable_copy_of_an_editable_document_is_its_save() {
    let mut h = Harness::new();
    h.blank();
    h.insert("abc");
    let Response::SaveData { bytes, .. } = h.send(1, Command::MakeEditableCopy { stamp: stamp() })
    else {
        panic!("expected SaveData");
    };
    let mut again = Harness::new();
    again.docx(&bytes);
    assert_eq!(again.text(), "abc");
}

fn png() -> Vec<u8> {
    std::fs::read(format!(
        "{}/../image/tests/fixtures/png/browser-200x100-red.png",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn media_parts(docx: &[u8]) -> Vec<String> {
    atelier_fmt_ooxml::Package::read(docx)
        .unwrap()
        .parts()
        .keys()
        .filter(|u| u.as_str().starts_with("word/media/"))
        .map(ToString::to_string)
        .collect()
}

#[test]
fn an_inserted_picture_is_an_object_saved_in_word_media() {
    let mut h = Harness::new();
    h.blank();
    h.insert("ab");
    h.select(1, 1);
    let state = h.state(Command::InsertImage {
        alt: "Un carr\u{e9} rouge".into(),
        bytes: png(),
    });
    assert_eq!(state.doc_len, 5);
    assert_eq!((state.anchor, state.focus), (4, 4));
    assert!(state.modified && state.can_undo);
    assert_eq!(h.text(), format!("a{OBJ}b"));
    let (bytes, notices) = h.save(SaveFormat::Docx);
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(media_parts(&bytes), ["word/media/image1.png"]);
    let package = atelier_fmt_ooxml::Package::read(&bytes).unwrap();
    let uri = atelier_fmt_ooxml::PartUri::parse("word/document.xml").unwrap();
    let xml = String::from_utf8(package.part(&uri).unwrap().bytes.clone()).unwrap();
    assert!(xml.contains("Un carr\u{e9} rouge"), "alt text kept");
    // The text save drops the picture and says so.
    let (text, notices) = h.save(SaveFormat::Txt);
    assert_eq!(text, b"ab");
    assert!(
        notices.iter().any(|n| n.code == notice::LOSS_BASE + 4),
        "{notices:?}"
    );
    // Undo and redo are one step.
    assert_eq!(h.undo().doc_len, 2);
    assert_eq!(h.redo().doc_len, 5);
    // A selection is replaced by the picture.
    h.select(0, 1);
    h.state(Command::InsertImage {
        alt: String::new(),
        bytes: png(),
    });
    assert_eq!(h.text(), format!("{OBJ}{OBJ}b"));
}

#[test]
fn a_picture_added_to_an_imported_docx_gets_a_free_media_name() {
    // rich.docx already holds word/media/image1.png.
    let source = fixture("libreoffice/rich.docx");
    let before = media_parts(&source);
    assert_eq!(before, ["word/media/image1.png"]);
    let mut h = Harness::new();
    let state = h.docx(&source);
    // rich.docx has revisions and comments: the picture goes in the copy.
    assert!(state.read_only);
    let Response::SaveData { bytes: copy, .. } =
        h.send(1, Command::MakeEditableCopy { stamp: stamp() })
    else {
        panic!("copy");
    };
    h.docx(&copy);
    h.select(0, 0);
    h.state(Command::InsertImage {
        alt: "neuf".into(),
        bytes: png(),
    });
    let (saved, notices) = h.save(SaveFormat::Docx);
    // The paragraph that received the picture is written from the model.
    assert_eq!(
        notices.iter().map(|n| n.code).collect::<Vec<_>>(),
        [notice::WRITE_BASE + 1],
        "{notices:?}"
    );
    let after = media_parts(&saved);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after.contains(&"word/media/image1.png".to_owned()));
    // The first picture is unchanged byte for byte.
    let kept = atelier_fmt_ooxml::Package::read(&saved).unwrap();
    let original = atelier_fmt_ooxml::Package::read(&source).unwrap();
    let uri = atelier_fmt_ooxml::PartUri::parse("word/media/image1.png").unwrap();
    assert_eq!(
        kept.part(&uri).unwrap().bytes,
        original.part(&uri).unwrap().bytes
    );
}

#[test]
fn pictures_are_validated_and_bounded() {
    let mut h = Harness::new();
    h.blank();
    let image = |bytes: Vec<u8>| Command::InsertImage {
        alt: String::new(),
        bytes,
    };
    assert_eq!(h.error(image(vec![1, 2, 3])), ErrorCode::ImageInvalid);
    let mut truncated = png();
    truncated.truncate(40);
    assert_eq!(h.error(image(truncated)), ErrorCode::ImageInvalid);
    let mut huge = png();
    huge.resize(proto::MAX_IMAGE_BYTES + 1, 0);
    let frame = proto::encode_request(&Request {
        doc_id: 1,
        request_id: 9,
        command: image(huge),
    });
    assert!(frame.is_err(), "the codec refuses a picture over 8 MiB");
    assert_eq!(h.text(), "");
    assert_eq!(h.state(Command::Layout { budget: 1 }).doc_len, 0);
    // The store of one document holds 512 pictures at most.
    for _ in 0..512 {
        h.state(image(png()));
    }
    assert_eq!(h.error(image(png())), ErrorCode::Limit);
}

#[test]
fn save_with_nothing_open_is_refused() {
    let mut h = Harness::new();
    assert_eq!(
        h.error(Command::Save {
            format: SaveFormat::Txt,
            stamp: stamp(),
        }),
        ErrorCode::UnknownDocument
    );
}

#[test]
fn the_client_time_and_user_land_in_the_document_properties() {
    let package = |bytes: &[u8]| atelier_fmt_ooxml::Package::read(bytes).unwrap();
    // An imported DOCX.
    let mut h = Harness::new();
    h.docx(&fixture("basic.docx"));
    let (saved, _) = h.save(SaveFormat::Docx);
    let p = package(&saved);
    assert_eq!(
        p.core_properties().last_modified_by.as_deref(),
        Some("Alice")
    );
    assert_eq!(
        p.core_properties().modified.as_deref(),
        Some("2023-11-14T22:13:20Z")
    );
    assert_eq!(p.app_properties().application.as_deref(), Some("Atelier"));
    assert_eq!(p.app_properties().app_version.as_deref(), Some("1.2"));
    assert!(p.app_properties().words.is_some());
    // A new document.
    h.blank();
    h.insert("un deux trois");
    let (saved, _) = h.save(SaveFormat::Docx);
    let p = package(&saved);
    assert_eq!(
        p.core_properties().last_modified_by.as_deref(),
        Some("Alice")
    );
    assert_eq!(p.app_properties().words.as_deref(), Some("3"));
    // The editable copy: statistics of the accepted text.
    h.docx(&fixture("revisions.docx"));
    let Response::SaveData { bytes, .. } = h.send(1, Command::MakeEditableCopy { stamp: stamp() })
    else {
        panic!("copy");
    };
    let p = package(&bytes);
    assert_eq!(p.app_properties().application.as_deref(), Some("Atelier"));
    assert!(p.app_properties().words.is_some());
    // An empty user leaves the author alone; a time out of range is refused.
    let bad = Stamp {
        now_ms: u64::MAX,
        ..stamp()
    };
    h.docx(&fixture("basic.docx"));
    assert_eq!(
        h.error(Command::Save {
            format: SaveFormat::Docx,
            stamp: bad
        }),
        ErrorCode::InvalidBody
    );
}

#[test]
fn alternative_text_refuses_what_xml_forbids() {
    for alt in ["a\u{fffe}", "\u{ffff}", "a\u{1}", "a\u{7f}"] {
        let r = proto::encode_request(&Request {
            doc_id: 1,
            request_id: 1,
            command: Command::InsertImage {
                alt: alt.into(),
                bytes: png(),
            },
        });
        assert!(r.is_err(), "{alt:?}");
    }
}

// ---- R2b x R3b: atlas and resync in a read-only or saving session ----------

#[test]
fn a_read_only_document_accepts_resync_with_a_fresh_atlas() {
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let opened = h.docx(&fixture("revisions.docx"));
    assert!(opened.read_only && opened.has_revisions);
    assert!(view.apply(&opened.atlas));
    view.covers(&opened.pages);
    let text = h.text();
    for command in edit_commands() {
        assert_eq!(h.error(command), ErrorCode::ReadOnly);
    }
    // The reply is lost: the read-only guard does not stand in the way.
    let resynced = h.state(Command::Resync);
    assert!(resynced.read_only && resynced.has_revisions && !resynced.modified);
    assert_ne!(Some(resynced.atlas.epoch), view.epoch);
    assert_eq!(resynced.atlas.first, 0);
    assert_eq!(resynced.invalidated, vec![(0, resynced.page_count)]);
    assert!(view.apply(&resynced.atlas));
    view.covers(&resynced.pages);
    assert_eq!(h.text(), text, "nothing changed");
    // Idempotent, and the edits are still refused afterwards.
    let again = h.state(Command::Resync);
    assert_ne!(again.atlas.epoch, resynced.atlas.epoch);
    assert_eq!(
        h.error(Command::InsertText { text: "x".into() }),
        ErrorCode::ReadOnly
    );
}

#[test]
fn an_abandoned_save_reply_is_recovered_by_the_atlas_resync() {
    // A SaveData larger than the frame cannot be encoded (`io_tests` checks
    // MAX_SAVE_BYTES + 1) and a document that large cannot be built here; the
    // path is the same as for a reply the ABI refuses: `transact` and the host
    // both end in `mark_resync`.
    let mut h = Harness::new();
    let mut view = AtlasView::default();
    let opened = h.docx(&fixture("revisions.docx"));
    assert!(view.apply(&opened.atlas));
    let (bytes, _) = h.save(SaveFormat::Docx);
    assert!(!bytes.is_empty());
    h.editor.mark_resync();
    let next = h.state(Command::Layout { budget: 1 });
    assert!(next.read_only && !next.modified);
    assert_ne!(Some(next.atlas.epoch), view.epoch, "a fresh epoch");
    assert_eq!(next.atlas.first, 0);
    assert_eq!(next.invalidated, vec![(0, next.page_count)]);
    assert!(view.apply(&next.atlas));
    view.covers(&next.pages);
    // An explicit Resync after it is accepted all the same.
    let resynced = h.state(Command::Resync);
    assert_ne!(resynced.atlas.epoch, next.atlas.epoch);
    assert!(view.apply(&resynced.atlas));
    view.covers(&resynced.pages);
}
