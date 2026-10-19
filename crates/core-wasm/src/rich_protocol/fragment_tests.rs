// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frozen vectors and refusals of `CopyFragment`, `PasteFragment`, the
//! `Fragment` reply and the errors 70 to 73 (T0). Vectors:
//! `tests/rich-editor-multidoc-vectors.txt`, verified here and by
//! `tests/rich-editor-multidoc.mjs` (`ATELIER_REGEN_VECTORS=1` rewrites them).

use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-multidoc-vectors.txt"
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

fn describe_request(r: &Request) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.command {
        Command::CopyFragment {
            revision,
            start,
            end,
        } => format!("CopyFragment revision={revision} start={start} end={end}"),
        Command::PasteFragment { bytes } => format!("PasteFragment bytes={}", hex(bytes)),
        other => panic!("not a fragment command: {other:?}"),
    };
    format!("{head} {tail}")
}

fn requests() -> Vec<(&'static str, Request)> {
    vec![
        (
            "copy",
            req(
                1,
                Command::CopyFragment {
                    revision: 3,
                    start: 2,
                    end: 9,
                },
            ),
        ),
        (
            "copy-max",
            req(
                2,
                Command::CopyFragment {
                    revision: u64::MAX,
                    start: 0,
                    end: u64::MAX,
                },
            ),
        ),
        (
            "paste",
            req(
                3,
                Command::PasteFragment {
                    bytes: b"ATFR\x01\x00\x00\x00".to_vec(),
                },
            ),
        ),
        (
            "paste-empty",
            req(4, Command::PasteFragment { bytes: Vec::new() }),
        ),
    ]
}

fn describe_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::Fragment {
            revision,
            lossy,
            text,
            bytes,
        } => format!(
            "Fragment revision={revision} lossy={} text={} bytes={}",
            u8::from(*lossy),
            hex(text.as_bytes()),
            hex(bytes)
        ),
        Response::Error(code) => format!("Error code={}", code.name()),
        other => panic!("not a fragment reply: {other:?}"),
    };
    format!("{head} {tail}")
}

fn replies() -> Vec<(&'static str, Reply)> {
    let reply = |request_id, response| Reply {
        doc_id: 7,
        request_id,
        response,
    };
    let fragment = |lossy, text: &str, bytes: &[u8]| Response::Fragment {
        revision: 12,
        lossy,
        text: text.into(),
        bytes: bytes.to_vec(),
    };
    vec![
        (
            "fragment",
            reply(5, fragment(false, "ab\ncd", b"ATFR\x01\x00\x00\x00")),
        ),
        ("fragment-lossy", reply(6, fragment(true, "\u{fffc}", b"x"))),
        (
            "too-many-documents",
            reply(7, Response::Error(ErrorCode::TooManyDocuments)),
        ),
        ("memory", reply(8, Response::Error(ErrorCode::Memory))),
        (
            "fragment-invalid",
            reply(9, Response::Error(ErrorCode::FragmentInvalid)),
        ),
        (
            "empty-selection",
            reply(10, Response::Error(ErrorCode::EmptySelection)),
        ),
    ]
}

fn copy_body(revision: u64, start: u64, end: u64) -> Vec<u8> {
    let mut b = revision.to_le_bytes().to_vec();
    b.extend_from_slice(&start.to_le_bytes());
    b.extend_from_slice(&end.to_le_bytes());
    b
}

fn bad_requests() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let copy = |body: &[u8]| header(opcode::COPY_FRAGMENT, 7, 1, body).unwrap();
    let paste = |body: &[u8]| header(opcode::PASTE_FRAGMENT, 7, 1, body).unwrap();
    let mut too_long = ((MAX_FRAGMENT_BYTES + 1) as u32).to_le_bytes().to_vec();
    too_long.extend_from_slice(&[0; 4]);
    vec![
        (
            "copy-start-after-end",
            copy(&copy_body(1, 5, 4)),
            InvalidRange,
        ),
        ("copy-truncated", copy(&[0; 23]), InvalidBody),
        ("copy-trailing", copy(&[0; 25]), InvalidBody),
        ("paste-too-long", paste(&too_long), Limit),
        (
            "paste-truncated",
            paste(&[4, 0, 0, 0, 1, 2, 3]),
            InvalidBody,
        ),
        ("paste-trailing", paste(&[1, 0, 0, 0, 1, 2]), InvalidBody),
        ("paste-no-length", paste(&[]), InvalidBody),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors of T0 (CopyFragment, PasteFragment, Fragment, errors 70 to 73). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol/fragment_tests.rs (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-multidoc.mjs.\n",
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
fn valid_frames_round_trip_and_invalid_ones_have_stable_codes() {
    for (name, r) in requests() {
        let bytes = encode_request(&r).unwrap();
        assert_eq!(decode_request(&bytes).unwrap(), r, "{name}");
        for cut in 0..bytes.len() {
            assert!(decode_request(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
    for (name, r) in replies() {
        let bytes = encode_reply(&r).unwrap();
        assert_eq!(decode_reply(&bytes).unwrap(), r, "{name}");
        for cut in 0..bytes.len() {
            assert!(decode_reply(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
    for (name, bytes, code) in bad_requests() {
        let error = decode_request(&bytes).unwrap_err();
        assert_eq!(error.code, code, "{name}");
        assert_eq!(error.context, Some((7, 1)), "{name}");
    }
    for (code, name, number) in [
        (ErrorCode::TooManyDocuments, "RICH_TOO_MANY_DOCUMENTS", 70),
        (ErrorCode::Memory, "RICH_MEMORY", 71),
        (ErrorCode::FragmentInvalid, "RICH_FRAGMENT_INVALID", 72),
        (ErrorCode::EmptySelection, "RICH_EMPTY_SELECTION", 73),
    ] {
        assert_eq!(code.name(), name);
        assert_eq!(code as u16, number);
        assert_eq!(ErrorCode::from_u16(number), Some(code));
    }
}

#[test]
fn a_fragment_reply_refuses_reserved_flags_and_oversized_parts() {
    let r = Reply {
        doc_id: 7,
        request_id: 1,
        response: Response::Fragment {
            revision: 1,
            lossy: false,
            text: String::new(),
            bytes: Vec::new(),
        },
    };
    let mut frame = encode_reply(&r).unwrap();
    // Body: revision (8), flags (4), text length (4), bytes length (4).
    frame[HEADER_BYTES + 8] = 2;
    assert_eq!(
        decode_reply(&frame).unwrap_err().code,
        ErrorCode::InvalidBody
    );
    let big = Reply {
        response: Response::Fragment {
            revision: 1,
            lossy: false,
            text: "x".repeat(MAX_FRAGMENT_TEXT_BYTES + 1),
            bytes: Vec::new(),
        },
        ..r
    };
    assert_eq!(encode_reply(&big).unwrap_err(), ErrorCode::Limit);
}
