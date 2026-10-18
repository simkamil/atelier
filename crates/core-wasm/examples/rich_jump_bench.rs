// SPDX-License-Identifier: AGPL-3.0-or-later
//! Host bench of the interactive jumps of the rich editor session: Ctrl+End,
//! a far click and Shift+Ctrl+End on a document whose tail is not laid out,
//! then the `Layout` pump until the caret shows. Run in release:
//! `cargo run --release --locked --offline -p atelier-core-wasm --example rich_jump_bench`

#[cfg(not(target_arch = "wasm32"))]
mod host {
    use atelier_core_wasm::protocol::{Message, MessageType};
    use atelier_core_wasm::rich_editor::RichEditor;
    use atelier_core_wasm::rich_protocol::{
        self as proto, Command, Direction, OpenKind, Profile, Request, Response, State,
    };
    use std::time::Instant;

    fn send(editor: &mut RichEditor, n: &mut u32, command: Command) -> (Response, f64) {
        *n += 1;
        let frame = proto::encode_request(&Request {
            doc_id: 1,
            request_id: *n,
            command,
        })
        .unwrap();
        let t = Instant::now();
        let reply = editor.respond(&Message {
            kind: MessageType::Editor,
            request_id: 7,
            payload: frame,
        });
        let ms = t.elapsed().as_secs_f64() * 1e3;
        assert_eq!(reply.kind, MessageType::EditorResult);
        (proto::decode_reply(&reply.payload).unwrap().response, ms)
    }

    fn state(r: Response) -> State {
        match r {
            Response::State(s) => s,
            other => panic!("{other:?}"),
        }
    }

    /// Calls `command`, then pumps `Layout` (16 paragraphs, like the client)
    /// until the caret shows. Returns (first call ms, max call ms, calls, total ms).
    fn jump(editor: &mut RichEditor, n: &mut u32, command: Command) -> (f64, f64, u32, f64) {
        let (r, first) = send(editor, n, command);
        let mut s = state(r);
        let (mut max, mut calls, mut total) = (first, 1, first);
        while s.caret.is_none() && calls < 100_000 {
            let (r, ms) = send(editor, n, Command::Layout { budget: 16 });
            s = state(r);
            max = max.max(ms);
            total += ms;
            calls += 1;
        }
        (first, max, calls, total)
    }

    /// Statistics (twice: the second is memoised), a keystroke and an undo
    /// while the caret is absent, on a document of `paragraphs` paragraphs.
    fn others(paragraphs: usize, text: &str) {
        let (mut editor, mut n) = (RichEditor::default(), 0u32);
        send(
            &mut editor,
            &mut n,
            Command::Open {
                kind: OpenKind::Txt,
                profile: Profile::Txt,
                data: text.as_bytes().to_vec(),
            },
        );
        let (_, first) = send(&mut editor, &mut n, Command::Statistics);
        let (_, second) = send(&mut editor, &mut n, Command::Statistics);
        let (_, third) = send(&mut editor, &mut n, Command::Statistics);
        println!(
            "{paragraphs} paragraphs Statistics: 1st {first:.1} ms, 2nd {second:.1} ms, 3rd {third:.1} ms"
        );
        let (r, jump) = send(
            &mut editor,
            &mut n,
            Command::Move {
                direction: Direction::DocumentEnd,
                extend: false,
            },
        );
        let s = state(r);
        let (r, typed) = send(
            &mut editor,
            &mut n,
            Command::InsertText { text: "x".into() },
        );
        let t = state(r);
        let (r, undone) = send(&mut editor, &mut n, Command::Undo);
        let u = state(r);
        println!(
            "{paragraphs} paragraphs without caret (caret after jump: {}): jump {jump:.1} ms, keystroke {typed:.1} ms (caret {}), undo {undone:.1} ms (caret {})",
            s.caret.is_some(),
            t.caret.is_some(),
            u.caret.is_some()
        );
    }

    pub fn run() {
        for paragraphs in [3_000usize, 12_000] {
            let text: String = (0..paragraphs)
                .map(|i| format!("Paragraphe numero {i} avec quelques mots"))
                .collect::<Vec<_>>()
                .join("\n");
            others(paragraphs, &text);
            let far = (paragraphs as u64) * 40;
            let scenarios: [(&str, bool, Command); 3] = [
                (
                    "Ctrl+End",
                    false,
                    Command::Move {
                        direction: Direction::DocumentEnd,
                        extend: false,
                    },
                ),
                (
                    "Shift+Ctrl+End",
                    false,
                    Command::Move {
                        direction: Direction::DocumentEnd,
                        extend: true,
                    },
                ),
                (
                    "far click (SetSelection)",
                    false,
                    Command::SetSelection {
                        anchor: far,
                        focus: far,
                        upstream: false,
                    },
                ),
            ];
            for (name, _, command) in scenarios {
                // A fresh, unlaid-out document per scenario.
                let (mut editor, mut n) = (RichEditor::default(), 0u32);
                let (r, ms) = send(
                    &mut editor,
                    &mut n,
                    Command::Open {
                        kind: OpenKind::Txt,
                        profile: Profile::Txt,
                        data: text.clone().into_bytes(),
                    },
                );
                let opened = state(r);
                assert!(!opened.layout_complete);
                let (first, max, calls, total) = jump(&mut editor, &mut n, command);
                println!(
                    "{paragraphs} paragraphs (open {ms:.1} ms) {name}: first call {first:.1} ms, max call {max:.1} ms, {calls} calls, total to caret {total:.1} ms"
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    host::run();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
