// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frozen vectors and refusals of the messages of US-218 (`Save`,
//! `InsertImage`, `MakeEditableCopy`, `SaveData`, the flag bits of `State`).
//! Vectors: `tests/rich-editor-save-vectors.txt`, verified here and by
//! `tests/rich-editor-save.mjs` (`ATELIER_REGEN_VECTORS=1` rewrites them).

use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-save-vectors.txt"
);

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn req(request_id: u32, command: Command) -> Request {
    Request {
        doc_id: 7,
        request_id,
        command,
    }
}

fn reply(request_id: u32, response: Response) -> Reply {
    Reply {
        doc_id: 7,
        request_id,
        response,
    }
}

fn describe_request(r: &Request) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.command {
        Command::Save { format, stamp } => {
            format!("Save format={} {}", *format as u8, describe_stamp(stamp))
        }
        Command::InsertImage { alt, bytes } => {
            format!(
                "InsertImage alt={} bytes={}",
                hex(alt.as_bytes()),
                hex(bytes)
            )
        }
        Command::MakeEditableCopy { stamp } => {
            format!("MakeEditableCopy {}", describe_stamp(stamp))
        }
        other => panic!("not a US-218 command: {other:?}"),
    };
    format!("{head} {tail}")
}

fn describe_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::SaveData {
            revision,
            bytes,
            notices,
        } => {
            let notices = if notices.is_empty() {
                "none".to_owned()
            } else {
                notices
                    .iter()
                    .map(|n| format!("{:04x}:{}", n.code, hex(n.text.as_bytes())))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!(
                "SaveData revision={revision} bytes={} notices={notices}",
                hex(bytes)
            )
        }
        Response::Error(code) => format!("Error code={}", code.name()),
        other => panic!("not a US-218 reply: {other:?}"),
    };
    format!("{head} {tail}")
}

fn stamp() -> Stamp {
    Stamp {
        now_ms: 1_700_000_000_123,
        user: "J\u{e9}r\u{f4}me".into(),
        version: "0.1.0".into(),
    }
}

fn describe_stamp(s: &Stamp) -> String {
    format!(
        "now_ms={} user={} version={}",
        s.now_ms,
        hex(s.user.as_bytes()),
        hex(s.version.as_bytes())
    )
}

/// The wire form of a stamp.
fn stamp_bytes(user: &[u8], version: &[u8]) -> Vec<u8> {
    let mut b = 5u64.to_le_bytes().to_vec();
    for t in [user, version] {
        b.extend_from_slice(&(t.len() as u16).to_le_bytes());
        b.extend_from_slice(t);
    }
    b
}

fn with_stamp(head: &[u8], user: &[u8], version: &[u8]) -> Vec<u8> {
    let mut b = head.to_vec();
    b.extend(stamp_bytes(user, version));
    b
}

fn requests() -> Vec<(&'static str, Request)> {
    vec![
        (
            "save-txt",
            req(
                1,
                Command::Save {
                    format: SaveFormat::Txt,
                    stamp: stamp(),
                },
            ),
        ),
        (
            "save-docx",
            req(
                2,
                Command::Save {
                    format: SaveFormat::Docx,
                    stamp: stamp(),
                },
            ),
        ),
        (
            "save-docm",
            req(
                3,
                Command::Save {
                    format: SaveFormat::Docm,
                    stamp: stamp(),
                },
            ),
        ),
        (
            "save-dotx",
            req(
                4,
                Command::Save {
                    format: SaveFormat::Dotx,
                    stamp: stamp(),
                },
            ),
        ),
        (
            "insert-image",
            req(
                5,
                Command::InsertImage {
                    alt: "\u{e9}t\u{e9}".into(),
                    bytes: vec![0x89, b'P', b'N', b'G'],
                },
            ),
        ),
        (
            "insert-image-no-alt",
            req(
                6,
                Command::InsertImage {
                    alt: String::new(),
                    bytes: vec![0xff, 0xd8],
                },
            ),
        ),
        (
            "make-editable-copy",
            req(7, Command::MakeEditableCopy { stamp: stamp() }),
        ),
    ]
}

fn replies() -> Vec<(&'static str, Reply)> {
    vec![
        (
            "save-data-empty",
            reply(
                1,
                Response::SaveData {
                    revision: 0,
                    bytes: Vec::new(),
                    notices: Vec::new(),
                },
            ),
        ),
        (
            "save-data-notices",
            reply(
                2,
                Response::SaveData {
                    revision: 3,
                    bytes: b"PK\x03\x04".to_vec(),
                    notices: vec![
                        Notice {
                            code: notice::WRITE_BASE + 1,
                            text: "ParagraphRebuilt".into(),
                        },
                        Notice {
                            code: notice::LOSS_BASE + 3,
                            text: "text-loss-tables=2".into(),
                        },
                    ],
                },
            ),
        ),
        (
            "read-only-error",
            reply(3, Response::Error(ErrorCode::ReadOnly)),
        ),
        (
            "save-failed-error",
            reply(4, Response::Error(ErrorCode::SaveFailed)),
        ),
        (
            "image-invalid-error",
            reply(5, Response::Error(ErrorCode::ImageInvalid)),
        ),
        (
            "revisions-unsupported-error",
            reply(6, Response::Error(ErrorCode::RevisionsUnsupported)),
        ),
    ]
}

fn frame(op: u16, body: &[u8]) -> Vec<u8> {
    header(op, 7, 1, body).unwrap()
}

fn bad_requests() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let image = |alt: &[u8], bytes: &[u8]| {
        let mut b = (alt.len() as u16).to_le_bytes().to_vec();
        b.extend_from_slice(alt);
        b.extend_from_slice(bytes);
        frame(opcode::INSERT_IMAGE, &b)
    };
    let mut too_big_alt = vec![b'a'; MAX_ALT_BYTES + 1];
    too_big_alt.shrink_to_fit();
    let mut big = vec![0u8; MAX_IMAGE_BYTES + 1];
    big[0] = 1;
    vec![
        (
            "save-format-0",
            frame(opcode::SAVE, &with_stamp(&[0, 0, 0, 0], b"", b"")),
            InvalidBody,
        ),
        (
            "save-format-5",
            frame(opcode::SAVE, &with_stamp(&[5, 0, 0, 0], b"", b"")),
            InvalidBody,
        ),
        (
            "save-reserved",
            frame(opcode::SAVE, &with_stamp(&[1, 0, 1, 0], b"", b"")),
            InvalidBody,
        ),
        ("save-short", frame(opcode::SAVE, &[1, 0, 0]), InvalidBody),
        (
            "save-long",
            frame(
                opcode::SAVE,
                &[with_stamp(&[1, 0, 0, 0], b"", b""), vec![0]].concat(),
            ),
            InvalidBody,
        ),
        (
            "copy-with-body",
            frame(
                opcode::MAKE_EDITABLE_COPY,
                &[stamp_bytes(b"", b""), vec![0]].concat(),
            ),
            InvalidBody,
        ),
        (
            "save-no-stamp",
            frame(opcode::SAVE, &[1, 0, 0, 0]),
            InvalidBody,
        ),
        (
            "save-user-too-long",
            frame(
                opcode::SAVE,
                &with_stamp(&[1, 0, 0, 0], "a".repeat(256).as_bytes(), b""),
            ),
            InvalidText,
        ),
        (
            "copy-version-invalid-utf8",
            frame(opcode::MAKE_EDITABLE_COPY, &stamp_bytes(b"", &[0xff])),
            InvalidUtf8,
        ),
        (
            "image-alt-fffe",
            image("a\u{fffe}".as_bytes(), b"x"),
            InvalidText,
        ),
        (
            "image-alt-ffff",
            image("\u{ffff}".as_bytes(), b"x"),
            InvalidText,
        ),
        ("image-empty", image(b"", b""), InvalidBody),
        ("image-alt-too-long", image(&too_big_alt, b"x"), InvalidText),
        ("image-alt-control", image(b"a\nb", b"x"), InvalidText),
        ("image-alt-invalid-utf8", image(&[0xff], b"x"), InvalidUtf8),
        ("image-over-8-mib", image(b"", &big), Limit),
        (
            "image-alt-past-end",
            frame(opcode::INSERT_IMAGE, &[9, 0, b'a']),
            InvalidBody,
        ),
        (
            "image-no-alt-length",
            frame(opcode::INSERT_IMAGE, &[1]),
            InvalidBody,
        ),
    ]
}

fn bad_replies() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let data = |bytes: &[u8], notices: &[u8]| {
        let mut b = 0u64.to_le_bytes().to_vec();
        b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(bytes);
        b.extend_from_slice(notices);
        frame(opcode::R_SAVE_DATA, &b)
    };
    let mut count_too_big = (MAX_NOTICES as u16 + 1).to_le_bytes().to_vec();
    for _ in 0..=MAX_NOTICES {
        count_too_big.extend_from_slice(&[0, 0, 0, 0]);
    }
    let mut long_text = 1u16.to_le_bytes().to_vec();
    long_text.extend_from_slice(&[1, 0]);
    long_text.extend_from_slice(&((MAX_NOTICE_BYTES + 1) as u16).to_le_bytes());
    long_text.extend_from_slice(&vec![b'a'; MAX_NOTICE_BYTES + 1]);
    let mut over = 0u64.to_le_bytes().to_vec();
    over.extend_from_slice(&((MAX_SAVE_BYTES + 1) as u32).to_le_bytes());
    vec![
        ("save-data-missing-count", data(b"x", &[]), InvalidBody),
        (
            "save-data-count-over-64",
            data(b"", &count_too_big),
            InvalidBody,
        ),
        (
            "save-data-notice-over-255",
            data(b"", &long_text),
            InvalidBody,
        ),
        (
            "save-data-notice-truncated",
            data(b"", &[1, 0, 1, 0, 9, 0, b'a']),
            InvalidBody,
        ),
        ("save-data-trailing", data(b"", &[0, 0, 0]), InvalidBody),
        (
            "save-data-invalid-utf8",
            data(b"", &[1, 0, 1, 0, 1, 0, 0xff]),
            InvalidUtf8,
        ),
        (
            "save-data-over-limit",
            frame(opcode::R_SAVE_DATA, &over),
            Limit,
        ),
        (
            "save-data-length-past-end",
            frame(
                opcode::R_SAVE_DATA,
                &[0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 1],
            ),
            InvalidBody,
        ),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors of US-218 (Save, InsertImage, MakeEditableCopy, SaveData). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol/io_tests.rs (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-save.mjs.\n",
    );
    for (name, r) in requests() {
        let _ = writeln!(
            out,
            "request\t{name}\t{}\t{}",
            hex(&encode_request(&r).unwrap()),
            describe_request(&r)
        );
    }
    for (name, r) in replies() {
        let _ = writeln!(
            out,
            "reply\t{name}\t{}\t{}",
            hex(&encode_reply(&r).unwrap()),
            describe_reply(&r)
        );
    }
    for (name, f, code) in bad_requests() {
        let _ = writeln!(out, "bad-request\t{name}\t{}\t{}", hex(&f), code.name());
    }
    for (name, f, code) in bad_replies() {
        let _ = writeln!(out, "bad-reply\t{name}\t{}\t{}", hex(&f), code.name());
    }
    out
}

#[test]
fn frozen_vectors_match_encoder() {
    let rendered = render_vectors();
    if std::env::var_os("ATELIER_REGEN_VECTORS").is_some() {
        std::fs::write(VECTORS, &rendered).unwrap();
    }
    let frozen = std::fs::read_to_string(VECTORS)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(frozen, rendered, "vectors are frozen");
}

#[test]
fn valid_frames_round_trip() {
    for (name, r) in requests() {
        let bytes = encode_request(&r).unwrap();
        assert_eq!(decode_request(&bytes).unwrap(), r, "{name}");
    }
    for (name, r) in replies() {
        let bytes = encode_reply(&r).unwrap();
        assert_eq!(decode_reply(&bytes).unwrap(), r, "{name}");
    }
}

#[test]
fn invalid_frames_have_stable_codes() {
    for (name, bytes, code) in bad_requests() {
        let error = decode_request(&bytes).unwrap_err();
        assert_eq!(error.code, code, "{name}");
        assert_eq!(error.context, Some((7, 1)), "{name}");
    }
    for (name, bytes, code) in bad_replies() {
        let error = decode_reply(&bytes).unwrap_err();
        assert_eq!(error.code, code, "{name}");
    }
}

#[test]
fn save_data_is_bounded_by_the_frame() {
    let ok = Response::SaveData {
        revision: 1,
        bytes: vec![7; MAX_SAVE_BYTES],
        notices: (0..MAX_NOTICES)
            .map(|_| Notice {
                code: 1,
                text: "a".repeat(MAX_NOTICE_BYTES),
            })
            .collect(),
    };
    let frame = encode_reply(&reply(1, ok)).unwrap();
    assert!(frame.len() <= MAX_FRAME_BYTES, "{}", frame.len());
    let over = Response::SaveData {
        revision: 1,
        bytes: vec![7; MAX_SAVE_BYTES + 1],
        notices: Vec::new(),
    };
    assert_eq!(encode_reply(&reply(1, over)), Err(ErrorCode::Limit));
    let too_many = Response::SaveData {
        revision: 1,
        bytes: Vec::new(),
        notices: vec![
            Notice {
                code: 1,
                text: String::new()
            };
            MAX_NOTICES + 1
        ],
    };
    assert_eq!(
        encode_reply(&reply(1, too_many)),
        Err(ErrorCode::InvalidBody)
    );
}

#[test]
fn state_flag_bits_round_trip() {
    let mut s = State {
        revision: 1,
        anchor: 0,
        focus: 0,
        focus_upstream: false,
        doc_len: 0,
        modified: false,
        can_undo: false,
        can_redo: false,
        layout_complete: true,
        page_count: 0,
        caret: None,
        invalidated: Vec::new(),
        pages: Vec::new(),
        window: None,
        read_only: false,
        has_revisions: false,
    };
    for (read_only, has_revisions) in [(false, false), (true, false), (false, true), (true, true)] {
        s.read_only = read_only;
        s.has_revisions = has_revisions;
        let bytes = encode_reply(&reply(1, Response::State(s.clone()))).unwrap();
        let Response::State(back) = decode_reply(&bytes).unwrap().response else {
            panic!("state");
        };
        assert_eq!(
            (back.read_only, back.has_revisions),
            (read_only, has_revisions)
        );
        assert_eq!(back, s);
    }
}
