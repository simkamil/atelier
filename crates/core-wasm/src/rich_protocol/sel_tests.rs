// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frozen vectors and refusals of the messages of R7b-1 (`SelectionRects`,
//! `ImageBitmap`, `SelectUnit`, the replies and error 30). Vectors:
//! `tests/rich-editor-selection-vectors.txt`, verified here and by
//! `tests/rich-editor-selection.mjs` (`ATELIER_REGEN_VECTORS=1` rewrites them).

use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-selection-vectors.txt"
);

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn bits(v: f64) -> String {
    format!("{:016x}", v.to_bits())
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
        Command::SelectionRects { first, count } => {
            format!("SelectionRects first={first} count={count}")
        }
        Command::ImageBitmap { object_id } => format!("ImageBitmap object={object_id}"),
        Command::SelectUnit {
            position,
            unit,
            extend,
        } => format!(
            "SelectUnit position={position} unit={} extend={}",
            *unit as u8,
            u8::from(*extend)
        ),
        other => panic!("not an R7b-1 command: {other:?}"),
    };
    format!("{head} {tail}")
}

fn describe_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::SelectionRects { revision, rects } => {
            let rects = if rects.is_empty() {
                "none".to_owned()
            } else {
                rects
                    .iter()
                    .map(|q| {
                        format!(
                            "{}:{}:{}:{}:{}",
                            q.page,
                            bits(q.x),
                            bits(q.y),
                            bits(q.width),
                            bits(q.height)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!("SelectionRects revision={revision} rects={rects}")
        }
        Response::ImageBitmap {
            object_id,
            width,
            height,
            rgba,
        } => format!(
            "ImageBitmap object={object_id} width={width} height={height} rgba={}",
            hex(rgba)
        ),
        Response::Error(code) => format!("Error code={}", code.name()),
        other => panic!("not an R7b-1 reply: {other:?}"),
    };
    format!("{head} {tail}")
}

fn rect(page: u32, x: f64, y: f64, width: f64, height: f64) -> SelectionRect {
    SelectionRect {
        page,
        x,
        y,
        width,
        height,
    }
}

fn requests() -> Vec<(&'static str, Request)> {
    let unit = |position, unit, extend| Command::SelectUnit {
        position,
        unit,
        extend,
    };
    vec![
        (
            "selection-rects",
            req(1, Command::SelectionRects { first: 2, count: 3 }),
        ),
        (
            "selection-rects-max",
            req(
                2,
                Command::SelectionRects {
                    first: MAX_PAGES - 1,
                    count: 16,
                },
            ),
        ),
        (
            "image-bitmap",
            req(
                3,
                Command::ImageBitmap {
                    object_id: 0x0102_0304_0506_0708,
                },
            ),
        ),
        ("select-word", req(4, unit(12, SelectionUnit::Word, false))),
        (
            "select-paragraph-extend",
            req(5, unit(u64::MAX, SelectionUnit::Paragraph, true)),
        ),
    ]
}

fn replies() -> Vec<(&'static str, Reply)> {
    vec![
        (
            "selection-rects-empty",
            reply(
                1,
                Response::SelectionRects {
                    revision: 4,
                    rects: Vec::new(),
                },
            ),
        ),
        (
            "selection-rects",
            reply(
                2,
                Response::SelectionRects {
                    revision: 1 << 40,
                    rects: vec![
                        rect(0, 72.0, 90.5, 120.25, 13.0),
                        rect(1, 72.0, 72.0, 0.0, 0.0),
                    ],
                },
            ),
        ),
        (
            "image-bitmap",
            reply(
                3,
                Response::ImageBitmap {
                    object_id: 9,
                    width: 2,
                    height: 1,
                    rgba: vec![255, 0, 0, 255, 0, 0, 255, 128],
                },
            ),
        ),
        (
            "unknown-object-error",
            reply(4, Response::Error(ErrorCode::UnknownObject)),
        ),
    ]
}

fn frame(op: u16, body: &[u8]) -> Vec<u8> {
    header(op, 7, 1, body).unwrap()
}

fn bad_requests() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let rects = |first: u32, count: u32| {
        let mut b = first.to_le_bytes().to_vec();
        b.extend_from_slice(&count.to_le_bytes());
        frame(opcode::SELECTION_RECTS, &b)
    };
    let unit = |unit: u8, extend: u8, tail: [u8; 2]| {
        let mut b = 5u64.to_le_bytes().to_vec();
        b.extend_from_slice(&[unit, extend, tail[0], tail[1]]);
        frame(opcode::SELECT_UNIT, &b)
    };
    vec![
        ("rects-zero-count", rects(0, 0), InvalidBody),
        ("rects-count-17", rects(0, 17), TooManyPages),
        ("rects-first-past-limit", rects(MAX_PAGES, 1), PageRange),
        (
            "rects-truncated",
            frame(opcode::SELECTION_RECTS, &[0; 7]),
            InvalidBody,
        ),
        (
            "rects-trailing",
            frame(opcode::SELECTION_RECTS, &[0, 0, 0, 0, 1, 0, 0, 0, 0]),
            InvalidBody,
        ),
        (
            "bitmap-truncated",
            frame(opcode::IMAGE_BITMAP, &[0; 7]),
            InvalidBody,
        ),
        (
            "bitmap-trailing",
            frame(opcode::IMAGE_BITMAP, &[0; 9]),
            InvalidBody,
        ),
        ("unit-unknown", unit(2, 0, [0, 0]), InvalidBody),
        ("unit-extend-2", unit(0, 2, [0, 0]), InvalidBody),
        ("unit-reserved", unit(0, 0, [1, 0]), InvalidBody),
        (
            "unit-truncated",
            frame(opcode::SELECT_UNIT, &[0; 11]),
            InvalidBody,
        ),
    ]
}

fn bad_replies() -> Vec<(&'static str, Vec<u8>, ErrorCode)> {
    use ErrorCode::*;
    let rects = |count: u32, body: &[u8]| {
        let mut b = 0u64.to_le_bytes().to_vec();
        b.extend_from_slice(&count.to_le_bytes());
        b.extend_from_slice(body);
        frame(opcode::R_SELECTION_RECTS, &b)
    };
    let one = |page: u32, w: f64, h: f64| {
        let mut b = page.to_le_bytes().to_vec();
        for v in [0.0f64, 0.0, w, h] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    };
    let bitmap = |width: u32, height: u32, len: u32, pixels: usize| {
        let mut b = 1u64.to_le_bytes().to_vec();
        for v in [width, height, len] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&vec![0u8; pixels]);
        frame(opcode::R_IMAGE_BITMAP, &b)
    };
    let mut nan = one(0, 1.0, 1.0);
    nan[4..12].copy_from_slice(&f64::NAN.to_le_bytes());
    vec![
        (
            "rects-count-over-4096",
            rects(MAX_SELECTION_RECTS as u32 + 1, &[]),
            InvalidBody,
        ),
        ("rects-truncated", rects(1, &[0; 35]), InvalidBody),
        (
            "rects-negative-width",
            rects(1, &one(0, -1.0, 1.0)),
            InvalidBody,
        ),
        (
            "rects-negative-height",
            rects(1, &one(0, 1.0, -1.0)),
            InvalidBody,
        ),
        (
            "rects-page-past-limit",
            rects(1, &one(MAX_PAGES, 1.0, 1.0)),
            InvalidBody,
        ),
        ("rects-not-finite", rects(1, &nan), InvalidBody),
        ("rects-trailing", rects(0, &[0]), InvalidBody),
        ("bitmap-zero-width", bitmap(0, 1, 0, 0), InvalidBody),
        ("bitmap-zero-height", bitmap(1, 0, 0, 0), InvalidBody),
        (
            "bitmap-width-1601",
            bitmap(1601, 1, 6404, 6404),
            InvalidBody,
        ),
        ("bitmap-length-mismatch", bitmap(2, 2, 15, 15), InvalidBody),
        ("bitmap-length-past-end", bitmap(2, 2, 16, 15), InvalidBody),
        ("bitmap-trailing", bitmap(1, 1, 4, 5), InvalidBody),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors of R7b-1 (SelectionRects, ImageBitmap, SelectUnit). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol/sel_tests.rs (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-selection.mjs.\n",
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
fn encoders_refuse_what_decoders_refuse() {
    let too_many = Response::SelectionRects {
        revision: 0,
        rects: vec![rect(0, 0.0, 0.0, 1.0, 1.0); MAX_SELECTION_RECTS + 1],
    };
    assert_eq!(
        encode_reply(&reply(1, too_many)),
        Err(ErrorCode::InvalidBody)
    );
    let wrong_length = Response::ImageBitmap {
        object_id: 1,
        width: 2,
        height: 2,
        rgba: vec![0; 15],
    };
    assert_eq!(
        encode_reply(&reply(1, wrong_length)),
        Err(ErrorCode::InvalidBody)
    );
    assert_eq!(ErrorCode::from_u16(30), Some(ErrorCode::UnknownObject));
    assert_eq!(ErrorCode::UnknownObject.name(), "RICH_UNKNOWN_OBJECT");
}

#[test]
fn no_prefix_of_a_valid_frame_decodes() {
    for (name, r) in requests() {
        let bytes = encode_request(&r).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode_request(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
    for (name, r) in replies() {
        let bytes = encode_reply(&r).unwrap();
        for cut in 0..bytes.len() {
            assert!(decode_reply(&bytes[..cut]).is_err(), "{name} cut {cut}");
        }
    }
}
