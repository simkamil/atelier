// SPDX-License-Identifier: AGPL-3.0-or-later
//! R7c: `MoveText` (text drag and drop) of the rich editor session, through
//! the real ATED v2 codec.
use atelier_core_wasm::protocol::{Message, MessageType};
use atelier_core_wasm::rich_editor::RichEditor;
use atelier_core_wasm::rich_protocol::{
    self as proto, Command, ErrorCode, OpenKind, Profile, Request, Response, State,
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
        h.state(Command::Open {
            kind: OpenKind::Blank,
            profile: Profile::Txt,
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

    fn state(&mut self, command: Command) -> State {
        match self.send(command) {
            Response::State(s) => s,
            other => panic!("expected a state, got {other:?}"),
        }
    }

    fn error(&mut self, command: Command) -> ErrorCode {
        match self.send(command) {
            Response::Error(code) => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn text(&mut self) -> String {
        match self.send(Command::TextWindow {
            start: 0,
            len: 10_000,
        }) {
            Response::TextWindow { text, .. } => text,
            other => panic!("expected a window, got {other:?}"),
        }
    }

    fn mv(&mut self, start: u64, end: u64, target: u64, copy: bool) -> State {
        self.state(Command::MoveText {
            start,
            end,
            target,
            copy,
        })
    }
}

fn typed(text: &str) -> Harness {
    let mut h = Harness::new();
    h.state(Command::InsertText { text: text.into() });
    h
}

#[test]
fn moves_selects_the_text_and_undoes_in_one_step() {
    let mut h = typed("abcdef ghi");
    let s = h.mv(0, 3, 6, false);
    assert_eq!(h.text(), "defabc ghi");
    assert_eq!((s.anchor, s.focus), (3, 6));
    assert!(s.can_undo);
    h.state(Command::Undo);
    assert_eq!(
        h.text(),
        "abcdef ghi",
        "one undo step takes the whole move back"
    );
    h.state(Command::Redo);
    assert_eq!(h.text(), "defabc ghi");
}

#[test]
fn copy_keeps_the_source_and_spans_paragraphs() {
    let mut h = typed("ab\ncd\nef");
    let s = h.mv(1, 4, 8, true);
    assert_eq!(h.text(), "ab\ncd\nefb\nc");
    assert_eq!((s.anchor, s.focus), (8, 11));
    h.state(Command::Undo);
    assert_eq!(h.text(), "ab\ncd\nef");
}

#[test]
fn refusals_keep_the_document_and_the_revision() {
    let mut h = typed("abcdef");
    let revision = h.state(Command::Layout { budget: 1 }).revision;
    assert_eq!(
        h.error(Command::MoveText {
            start: 1,
            end: 4,
            target: 2,
            copy: false
        }),
        ErrorCode::InvalidRange
    );
    assert_eq!(
        h.error(Command::MoveText {
            start: 1,
            end: 4,
            target: 3,
            copy: true
        }),
        ErrorCode::InvalidRange
    );
    assert_eq!(
        h.error(Command::MoveText {
            start: 1,
            end: 9,
            target: 0,
            copy: false
        }),
        ErrorCode::PositionOutOfRange
    );
    assert_eq!(
        h.error(Command::MoveText {
            start: 1,
            end: 2,
            target: 9,
            copy: false
        }),
        ErrorCode::PositionOutOfRange
    );
    assert_eq!(h.text(), "abcdef");
    assert_eq!(h.state(Command::Layout { budget: 1 }).revision, revision);
    // Onto its own end: nothing happens.
    h.mv(1, 4, 4, false);
    assert_eq!(h.text(), "abcdef");
    assert_eq!(h.state(Command::Layout { budget: 1 }).revision, revision);
}

#[test]
fn move_text_frames_round_trip_and_refuse_bad_bodies() {
    let command = Command::MoveText {
        start: 1,
        end: 5,
        target: 9,
        copy: true,
    };
    let frame = proto::encode_request(&Request {
        doc_id: 1,
        request_id: 1,
        command: command.clone(),
    })
    .unwrap();
    let decoded = proto::decode_request(&frame).unwrap();
    assert_eq!(decoded.command, command);
    // start > end, a flag other than 0 or 1, non-zero padding.
    let bad = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut f = frame.clone();
        edit(&mut f);
        proto::decode_request(&f).err()
    };
    let n = frame.len();
    assert!(bad(&|f| f[n - 4] = 2).is_some());
    assert!(bad(&|f| f[n - 1] = 1).is_some());
    assert!(bad(&|f| f.truncate(n - 1)).is_some());
}
