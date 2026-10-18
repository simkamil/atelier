// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use std::fmt::Write as _;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/rich-editor-protocol-vectors.txt"
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

fn page(fill: u8) -> Vec<u8> {
    let mut p = vec![0u8; MIN_PAGE_DISPLAY_BYTES];
    p[..4].copy_from_slice(b"ATDL");
    p[4] = 1;
    p[24..32].copy_from_slice(&595.276f64.to_le_bytes());
    p[32..40].copy_from_slice(&841.89f64.to_le_bytes());
    p[43] = fill;
    p
}

fn describe_request(r: &Request) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.command {
        Command::Open {
            kind,
            profile,
            data,
        } => format!(
            "Open kind={} profile={} data={}",
            *kind as u8,
            *profile as u8,
            hex(data)
        ),
        Command::Close => "Close".into(),
        Command::InsertText { text } => format!("InsertText text={}", hex(text.as_bytes())),
        Command::ReplaceRange { start, end, text } => format!(
            "ReplaceRange start={start} end={end} text={}",
            hex(text.as_bytes())
        ),
        Command::Delete { forward, unit } => {
            format!("Delete forward={} unit={}", u8::from(*forward), *unit as u8)
        }
        Command::Move { direction, extend } => format!(
            "Move direction={} extend={}",
            *direction as u8,
            u8::from(*extend)
        ),
        Command::SetSelection { anchor, focus } => {
            format!("SetSelection anchor={anchor} focus={focus}")
        }
        Command::HitTest { page, mode, x, y } => format!(
            "HitTest page={page} mode={} x={} y={}",
            *mode as u8,
            bits(*x),
            bits(*y)
        ),
        Command::Undo => "Undo".into(),
        Command::Redo => "Redo".into(),
        Command::TextWindow { start, len } => format!("TextWindow start={start} len={len}"),
        Command::Pages { first, count } => format!("Pages first={first} count={count}"),
        Command::MarkSaved { revision } => format!("MarkSaved revision={revision}"),
    };
    format!("{head} {tail}")
}

fn describe_pages(pages: &[PageDisplay]) -> String {
    if pages.is_empty() {
        return "none".into();
    }
    pages
        .iter()
        .map(|p| format!("{}:{}", p.index, p.display.len()))
        .collect::<Vec<_>>()
        .join(",")
}

fn describe_reply(r: &Reply) -> String {
    let head = format!("doc={} req={}", r.doc_id, r.request_id);
    let tail = match &r.response {
        Response::Error(code) => format!("Error code={}", code.name()),
        Response::Closed => "Closed".into(),
        Response::HitResult {
            revision,
            page,
            position,
        } => {
            format!("HitResult revision={revision} page={page} position={position}")
        }
        Response::TextWindow {
            revision,
            start,
            doc_len,
            text,
        } => format!(
            "TextWindow revision={revision} start={start} doc_len={doc_len} text={}",
            hex(text.as_bytes())
        ),
        Response::Pages {
            revision,
            page_count,
            pages,
        } => format!(
            "Pages revision={revision} page_count={page_count} pages={}",
            describe_pages(pages)
        ),
        Response::State(s) => {
            let caret = s.caret.map_or("none".into(), |c| {
                format!("{}:{}:{}:{}", c.page, bits(c.x), bits(c.y), bits(c.height))
            });
            let inval = if s.invalidated.is_empty() {
                "none".into()
            } else {
                s.invalidated
                    .iter()
                    .map(|(a, b)| format!("{a}-{b}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let window = s.window.as_ref().map_or("none".into(), |w| {
                format!("{}:{}", w.start, hex(w.text.as_bytes()))
            });
            format!(
                "State revision={} anchor={} focus={} doc_len={} modified={} can_undo={} can_redo={} layout_complete={} page_count={} caret={caret} invalidated={inval} pages={} window={window}",
                s.revision,
                s.anchor,
                s.focus,
                s.doc_len,
                u8::from(s.modified),
                u8::from(s.can_undo),
                u8::from(s.can_redo),
                u8::from(s.layout_complete),
                s.page_count,
                describe_pages(&s.pages)
            )
        }
    };
    format!("{head} {tail}")
}

fn req(doc_id: u32, request_id: u32, command: Command) -> Request {
    Request {
        doc_id,
        request_id,
        command,
    }
}

fn requests() -> Vec<(&'static str, Request)> {
    vec![
        (
            "open-blank-document",
            req(
                1,
                1,
                Command::Open {
                    kind: OpenKind::Blank,
                    profile: Profile::Document,
                    data: vec![],
                },
            ),
        ),
        (
            "open-blank-txt",
            req(
                1,
                2,
                Command::Open {
                    kind: OpenKind::Blank,
                    profile: Profile::Txt,
                    data: vec![],
                },
            ),
        ),
        (
            "open-txt",
            req(
                7,
                3,
                Command::Open {
                    kind: OpenKind::Txt,
                    profile: Profile::Txt,
                    data: b"a\nb".to_vec(),
                },
            ),
        ),
        (
            "open-docx",
            req(
                7,
                4,
                Command::Open {
                    kind: OpenKind::Docx,
                    profile: Profile::Document,
                    data: vec![0x50, 0x4b, 3, 4],
                },
            ),
        ),
        ("close", req(7, 5, Command::Close)),
        (
            "insert-text",
            req(
                7,
                6,
                Command::InsertText {
                    text: "h\u{e9}llo\nw\u{1f600}".into(),
                },
            ),
        ),
        (
            "replace-range",
            req(
                7,
                7,
                Command::ReplaceRange {
                    start: 1,
                    end: 4,
                    text: "xy".into(),
                },
            ),
        ),
        (
            "replace-range-delete",
            req(
                7,
                8,
                Command::ReplaceRange {
                    start: 0,
                    end: u64::from(u32::MAX) + 5,
                    text: String::new(),
                },
            ),
        ),
        (
            "delete-backward-grapheme",
            req(
                7,
                9,
                Command::Delete {
                    forward: false,
                    unit: DeleteUnit::Grapheme,
                },
            ),
        ),
        (
            "delete-forward-word",
            req(
                7,
                10,
                Command::Delete {
                    forward: true,
                    unit: DeleteUnit::Word,
                },
            ),
        ),
        (
            "move-left",
            req(
                7,
                11,
                Command::Move {
                    direction: Direction::Left,
                    extend: false,
                },
            ),
        ),
        (
            "move-paragraph-down-extend",
            req(
                7,
                12,
                Command::Move {
                    direction: Direction::ParagraphDown,
                    extend: true,
                },
            ),
        ),
        (
            "set-selection",
            req(
                7,
                13,
                Command::SetSelection {
                    anchor: 9,
                    focus: 2,
                },
            ),
        ),
        (
            "hit-probe",
            req(
                7,
                14,
                Command::HitTest {
                    page: 3,
                    mode: HitMode::Probe,
                    x: 100.5,
                    y: -2.25,
                },
            ),
        ),
        (
            "hit-extend",
            req(
                7,
                15,
                Command::HitTest {
                    page: 0,
                    mode: HitMode::Extend,
                    x: 0.0,
                    y: 841.89,
                },
            ),
        ),
        ("undo", req(7, 16, Command::Undo)),
        ("redo", req(7, 17, Command::Redo)),
        (
            "text-window",
            req(
                7,
                18,
                Command::TextWindow {
                    start: 10,
                    len: MAX_WINDOW_BYTES as u32,
                },
            ),
        ),
        (
            "pages",
            req(
                7,
                19,
                Command::Pages {
                    first: 4,
                    count: MAX_PAGES_PER_RESPONSE as u32,
                },
            ),
        ),
        (
            "mark-saved",
            req(7, 20, Command::MarkSaved { revision: u64::MAX }),
        ),
        ("max-ids", req(u32::MAX, u32::MAX, Command::Undo)),
    ]
}

fn state() -> State {
    State {
        revision: 42,
        anchor: 3,
        focus: 5,
        doc_len: 12,
        modified: true,
        can_undo: true,
        can_redo: false,
        layout_complete: true,
        page_count: 3,
        caret: Some(Caret {
            page: 1,
            x: 72.0,
            y: 96.5,
            height: 14.0,
        }),
        invalidated: vec![(2, 3)],
        pages: vec![
            PageDisplay {
                index: 0,
                display: page(0),
            },
            PageDisplay {
                index: 1,
                display: page(1),
            },
        ],
        window: Some(TextWindowData {
            start: 2,
            text: "a\nb".into(),
        }),
    }
}

fn replies() -> Vec<(&'static str, Reply)> {
    let r = |doc_id, request_id, response| Reply {
        doc_id,
        request_id,
        response,
    };
    let mut bare = state();
    bare.caret = None;
    bare.invalidated.clear();
    bare.pages.clear();
    bare.window = None;
    bare.modified = false;
    bare.can_undo = false;
    bare.layout_complete = false;
    bare.page_count = 0;
    vec![
        (
            "error-position",
            r(7, 6, Response::Error(ErrorCode::PositionNotBoundary)),
        ),
        (
            "error-unknown-document",
            r(9, 1, Response::Error(ErrorCode::UnknownDocument)),
        ),
        ("closed", r(7, 5, Response::Closed)),
        (
            "hit-result",
            r(
                7,
                14,
                Response::HitResult {
                    revision: 5,
                    page: 3,
                    position: 17,
                },
            ),
        ),
        (
            "text-window",
            r(
                7,
                18,
                Response::TextWindow {
                    revision: 5,
                    start: 2,
                    doc_len: 12,
                    text: "ab\n".into(),
                },
            ),
        ),
        (
            "text-window-empty",
            r(
                7,
                18,
                Response::TextWindow {
                    revision: 5,
                    start: 12,
                    doc_len: 12,
                    text: String::new(),
                },
            ),
        ),
        (
            "pages",
            r(
                7,
                19,
                Response::Pages {
                    revision: 5,
                    page_count: 6,
                    pages: vec![
                        PageDisplay {
                            index: 4,
                            display: page(4),
                        },
                        PageDisplay {
                            index: 5,
                            display: page(5),
                        },
                    ],
                },
            ),
        ),
        (
            "pages-empty",
            r(
                7,
                19,
                Response::Pages {
                    revision: 5,
                    page_count: 2,
                    pages: vec![],
                },
            ),
        ),
        ("state-full", r(7, 6, Response::State(state()))),
        ("state-bare", r(7, 6, Response::State(bare))),
    ]
}

fn frame(op: u16, doc: u32, req: u32, body: &[u8]) -> Vec<u8> {
    header(op, doc, req, body).unwrap()
}

fn text_body(text: &[u8]) -> Vec<u8> {
    let mut b = (text.len() as u32).to_le_bytes().to_vec();
    b.extend_from_slice(text);
    b
}

fn state_frame(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut body = Vec::new();
    let mut s = state();
    s.window = None;
    encode_state(&s, &mut body).unwrap();
    edit(&mut body);
    frame(opcode::R_STATE, 7, 1, &body)
}

type Bad = (&'static str, Vec<u8>, ErrorCode, bool);

fn bad_requests() -> Vec<Bad> {
    use ErrorCode::*;
    let good = encode_request(&req(7, 1, Command::Undo)).unwrap();
    let mut trailing = good.clone();
    trailing.push(0);
    let mut v1 = good.clone();
    v1[4] = 1;
    let mut magic = good.clone();
    magic[0] = b'X';
    let mut huge = good.clone();
    huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut zero_doc = good.clone();
    zero_doc[8..12].copy_from_slice(&0u32.to_le_bytes());
    let long = vec![b'a'; MAX_TEXT_BYTES + 1];
    let mut hit_nan = vec![0u8; 8];
    hit_nan.extend_from_slice(&f64::NAN.to_bits().to_le_bytes());
    hit_nan.extend_from_slice(&0f64.to_bits().to_le_bytes());
    let mut replace_inverted = 5u64.to_le_bytes().to_vec();
    replace_inverted.extend_from_slice(&2u64.to_le_bytes());
    replace_inverted.extend_from_slice(&text_body(b""));
    let mut hit_mode = vec![0u8; 20];
    hit_mode[4] = 3;
    vec![
        ("empty", vec![], InvalidEnvelope, false),
        ("wrong-magic", magic, InvalidEnvelope, false),
        ("version-1", v1, UnsupportedVersion, false),
        (
            "truncated-header",
            good[..19].to_vec(),
            InvalidEnvelope,
            false,
        ),
        ("trailing-byte", trailing, InvalidEnvelope, true),
        (
            "truncated-body",
            frame(opcode::MARK_SAVED, 7, 1, &[0; 8])[..24].to_vec(),
            InvalidEnvelope,
            true,
        ),
        ("body-length-huge", huge, FrameTooLarge, true),
        ("document-zero", zero_doc, InvalidDocumentId, true),
        (
            "unknown-opcode",
            frame(0x0001, 7, 1, &[]),
            UnknownOpcode,
            true,
        ),
        (
            "reply-opcode-as-request",
            frame(opcode::R_CLOSED, 7, 1, &[]),
            UnknownOpcode,
            true,
        ),
        (
            "undo-with-body",
            frame(opcode::UNDO, 7, 1, &[0]),
            InvalidBody,
            true,
        ),
        (
            "move-direction-14",
            frame(opcode::MOVE, 7, 1, &[14, 0, 0, 0]),
            InvalidBody,
            true,
        ),
        (
            "move-extend-2",
            frame(opcode::MOVE, 7, 1, &[0, 2, 0, 0]),
            InvalidBody,
            true,
        ),
        (
            "move-reserved",
            frame(opcode::MOVE, 7, 1, &[0, 0, 1, 0]),
            InvalidBody,
            true,
        ),
        (
            "delete-unit-2",
            frame(opcode::DELETE, 7, 1, &[0, 2, 0, 0]),
            InvalidBody,
            true,
        ),
        (
            "insert-invalid-utf8",
            frame(opcode::INSERT_TEXT, 7, 1, &text_body(&[0xff, 0xfe])),
            InvalidUtf8,
            true,
        ),
        (
            "insert-empty",
            frame(opcode::INSERT_TEXT, 7, 1, &text_body(b"")),
            InvalidBody,
            true,
        ),
        (
            "insert-object-char",
            frame(
                opcode::INSERT_TEXT,
                7,
                1,
                &text_body("a\u{fffc}".as_bytes()),
            ),
            InvalidText,
            true,
        ),
        (
            "insert-carriage-return",
            frame(opcode::INSERT_TEXT, 7, 1, &text_body(b"a\r\nb")),
            InvalidText,
            true,
        ),
        (
            "insert-nul",
            frame(opcode::INSERT_TEXT, 7, 1, &text_body(b"a\0")),
            InvalidText,
            true,
        ),
        (
            "insert-too-long",
            frame(opcode::INSERT_TEXT, 7, 1, &text_body(&long)),
            FrameTooLarge,
            true,
        ),
        (
            "insert-length-lies",
            frame(opcode::INSERT_TEXT, 7, 1, &[9, 0, 0, 0, b'a']),
            InvalidBody,
            true,
        ),
        (
            "replace-inverted",
            frame(opcode::REPLACE_RANGE, 7, 1, &replace_inverted),
            InvalidRange,
            true,
        ),
        (
            "hit-nan",
            frame(opcode::HIT_TEST, 7, 1, &hit_nan),
            InvalidBody,
            true,
        ),
        (
            "hit-mode-3",
            frame(opcode::HIT_TEST, 7, 1, &hit_mode),
            InvalidBody,
            true,
        ),
        (
            "window-zero",
            frame(opcode::TEXT_WINDOW, 7, 1, &[0; 12]),
            InvalidBody,
            true,
        ),
        (
            "window-too-large",
            frame(
                opcode::TEXT_WINDOW,
                7,
                1,
                &[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 4, 0],
            ),
            WindowTooLarge,
            true,
        ),
        (
            "pages-zero",
            frame(opcode::PAGES, 7, 1, &[0; 8]),
            InvalidBody,
            true,
        ),
        (
            "pages-17",
            frame(opcode::PAGES, 7, 1, &[0, 0, 0, 0, 17, 0, 0, 0]),
            TooManyPages,
            true,
        ),
        (
            "pages-first-out-of-bounds",
            frame(opcode::PAGES, 7, 1, &[0, 0, 16, 0, 1, 0, 0, 0]),
            PageRange,
            true,
        ),
        (
            "open-blank-with-data",
            frame(opcode::OPEN, 7, 1, &[0, 0, 0, 0, 1, 0, 0, 0, 1]),
            InvalidBody,
            true,
        ),
        (
            "open-txt-profile-document",
            frame(opcode::OPEN, 7, 1, &[1, 0, 0, 0, 0, 0, 0, 0]),
            InvalidBody,
            true,
        ),
        (
            "open-kind-3",
            frame(opcode::OPEN, 7, 1, &[3, 0, 0, 0, 0, 0, 0, 0]),
            InvalidBody,
            true,
        ),
    ]
}

fn bad_replies() -> Vec<Bad> {
    use ErrorCode::*;
    let u32_at =
        |body: &mut Vec<u8>, at: usize, v: u32| body[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let u64_at =
        |body: &mut Vec<u8>, at: usize, v: u64| body[at..at + 8].copy_from_slice(&v.to_le_bytes());
    let mut not_atdl = state_frame(|_| {});
    let last_page = not_atdl.len() - MIN_PAGE_DISPLAY_BYTES;
    not_atdl[last_page] = b'X';
    let mut past_end = vec![0u8; 8];
    past_end.extend_from_slice(&5u64.to_le_bytes());
    past_end.extend_from_slice(&6u64.to_le_bytes());
    past_end.extend_from_slice(&text_body(b"ab"));
    let mut unordered = vec![0u8; 8];
    unordered.extend_from_slice(&6u32.to_le_bytes());
    unordered.extend_from_slice(&2u32.to_le_bytes());
    for _ in 0..2 {
        unordered.extend_from_slice(&3u32.to_le_bytes());
        unordered.extend_from_slice(&(MIN_PAGE_DISPLAY_BYTES as u32).to_le_bytes());
        unordered.extend_from_slice(&page(3));
    }
    vec![
        (
            "error-code-zero",
            frame(opcode::R_ERROR, 7, 1, &[0, 0, 0, 0]),
            InvalidBody,
            true,
        ),
        (
            "error-reserved",
            frame(opcode::R_ERROR, 7, 1, &[1, 0, 1, 0]),
            InvalidBody,
            true,
        ),
        (
            "closed-with-body",
            frame(opcode::R_CLOSED, 7, 1, &[0]),
            InvalidBody,
            true,
        ),
        (
            "command-opcode-as-reply",
            frame(opcode::UNDO, 7, 1, &[]),
            UnknownOpcode,
            true,
        ),
        (
            "state-anchor-past-end",
            state_frame(|b| u64_at(b, 8, 99)),
            InvalidBody,
            true,
        ),
        (
            "state-reserved-flags",
            state_frame(|b| u32_at(b, 32, 0x20)),
            InvalidBody,
            true,
        ),
        (
            "state-caret-page-out-of-range",
            state_frame(|b| u32_at(b, 40, 3)),
            InvalidBody,
            true,
        ),
        (
            "state-page-count-too-big",
            state_frame(|b| u32_at(b, 36, MAX_PAGES + 1)),
            TooManyPages,
            true,
        ),
        ("state-page-not-atdl", not_atdl, InvalidBody, true),
        (
            "state-truncated",
            state_frame(|b| b.truncate(30)),
            InvalidBody,
            true,
        ),
        (
            "text-window-past-end",
            frame(opcode::R_TEXT_WINDOW, 7, 1, &past_end),
            PositionOutOfRange,
            true,
        ),
        (
            "pages-not-increasing",
            frame(opcode::R_PAGES, 7, 1, &unordered),
            PageRange,
            true,
        ),
    ]
}

fn render_vectors() -> String {
    let mut out = String::from(
        "# Frozen ATED v2 vectors (US-203). Columns: kind, name, frame hex, expectation.\n\
         # Generated by the Rust tests of rich_protocol (ATELIER_REGEN_VECTORS=1), verified by Rust and tests/rich-editor-protocol.mjs.\n",
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
    for (kind, list) in [
        ("bad-request", bad_requests()),
        ("bad-reply", bad_replies()),
    ] {
        for (name, f, code, ctx) in list {
            let note = if ctx { "" } else { " nocontext" };
            let _ = writeln!(out, "{kind}\t{name}\t{}\t{}{note}", hex(&f), code.name());
        }
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
    assert_eq!(
        frozen, rendered,
        "vectors are frozen; regenerate only for a deliberate v2 change"
    );
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
    for (name, f, code, ctx) in bad_requests() {
        let e = decode_request(&f).expect_err(name);
        assert_eq!((e.code, e.context.is_some()), (code, ctx), "{name}");
    }
    for (name, f, code, ctx) in bad_replies() {
        let e = decode_reply(&f).expect_err(name);
        assert_eq!((e.code, e.context.is_some()), (code, ctx), "{name}");
    }
}

#[test]
fn truncations_and_bit_flips_never_panic() {
    let mut frames: Vec<Vec<u8>> = requests()
        .iter()
        .map(|(_, r)| encode_request(r).unwrap())
        .collect();
    frames.extend(replies().iter().map(|(_, r)| encode_reply(r).unwrap()));
    for f in &frames {
        for cut in 0..f.len() {
            assert!(decode_request(&f[..cut]).is_err());
            assert!(decode_reply(&f[..cut]).is_err());
        }
        for at in 0..f.len().min(120) {
            let mut m = f.clone();
            m[at] ^= 0xff;
            let _ = decode_request(&m);
            let _ = decode_reply(&m);
        }
    }
}

#[test]
fn encoder_refuses_what_decoder_would_reject() {
    let bad = req(0, 1, Command::Undo);
    assert_eq!(encode_request(&bad), Err(ErrorCode::InvalidDocumentId));
    let big = req(
        1,
        1,
        Command::InsertText {
            text: "a".repeat(MAX_TEXT_BYTES + 1),
        },
    );
    assert_eq!(encode_request(&big), Err(ErrorCode::FrameTooLarge));
    let obj = req(
        1,
        1,
        Command::InsertText {
            text: "\u{fffc}".into(),
        },
    );
    assert_eq!(encode_request(&obj), Err(ErrorCode::InvalidText));
    let mut s = state();
    s.pages.push(PageDisplay {
        index: 1,
        display: page(9),
    });
    let reply = Reply {
        doc_id: 1,
        request_id: 1,
        response: Response::State(s),
    };
    assert_eq!(encode_reply(&reply), Err(ErrorCode::PageRange));
    let open = |n| {
        req(
            1,
            1,
            Command::Open {
                kind: OpenKind::Docx,
                profile: Profile::Document,
                data: vec![0; n],
            },
        )
    };
    assert_eq!(
        encode_request(&open(MAX_OPEN_BYTES + 1)),
        Err(ErrorCode::FrameTooLarge)
    );
    assert_eq!(
        encode_request(&open(MAX_OPEN_BYTES)).map(|f| f.len()),
        Ok(MAX_FRAME_BYTES)
    );
}

#[test]
fn error_codes_are_stable_and_named() {
    for n in 1..=18u16 {
        let code = ErrorCode::from_u16(n).unwrap();
        assert_eq!(code as u16, n);
        assert!(code.name().starts_with("RICH_"));
    }
    assert!(ErrorCode::from_u16(0).is_none() && ErrorCode::from_u16(19).is_none());
}

#[test]
fn range_rules() {
    assert_eq!(check_range(10, 2, 10), Ok(()));
    assert_eq!(check_range(10, 3, 2), Err(ErrorCode::InvalidRange));
    assert_eq!(check_range(10, 2, 11), Err(ErrorCode::PositionOutOfRange));
}
