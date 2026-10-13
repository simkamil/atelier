// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_core_wasm::editor::EditorSession;

fn command(opcode: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"ATED\x01\x00".to_vec();
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}
fn u64le(value: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn u32le(value: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn error_code(session: &EditorSession, payload: &[u8]) -> &'static str {
    match session.transact(payload) {
        Err(error) => error.0,
        Ok(_) => panic!("expected query command failure"),
    }
}
fn load(text: &str) -> EditorSession {
    let session = EditorSession::default();
    session.transact(&command(1, text.as_bytes())).unwrap().0
}
fn revision(session: &EditorSession) -> u64 {
    let state = session.transact(&command(0, &[])).unwrap().1;
    u64::from_le_bytes(state[8..16].try_into().unwrap())
}
fn search(session: &EditorSession, query: &str, flags: u8) -> Result<Vec<u8>, &'static str> {
    let mut body = Vec::new();
    u64le(revision(session), &mut body);
    body.extend_from_slice(&[flags, 0, 0, 0]);
    u32le(query.len() as u32, &mut body);
    body.extend_from_slice(query.as_bytes());
    session
        .transact(&command(19, &body))
        .map(|(_, out)| out)
        .map_err(|e| e.0)
}

#[test]
fn unicode_search_uses_exact_ranges_and_cached_geometry_without_changing_state() {
    let session = load("café café");
    let before = session.transact(&command(0, &[])).unwrap().1;
    let result = search(&session, "é", 1).unwrap();
    assert_eq!(&result[..4], b"ATQF");
    assert_eq!(u16::from_le_bytes(result[4..6].try_into().unwrap()), 1);
    assert_eq!(
        u64::from_le_bytes(result[8..16].try_into().unwrap()),
        revision(&session)
    );
    assert_eq!(u32::from_le_bytes(result[16..20].try_into().unwrap()), 2);
    assert_eq!(result[20], 0);
    let first_start = u32::from_le_bytes(result[24..28].try_into().unwrap());
    let first_end = u32::from_le_bytes(result[28..32].try_into().unwrap());
    assert_eq!((first_start, first_end), (3, 5));
    let rect_count_offset =
        24 + 24 + u32::from_le_bytes(result[32..36].try_into().unwrap()) as usize;
    let rect_count = u32::from_le_bytes(
        result[rect_count_offset..rect_count_offset + 4]
            .try_into()
            .unwrap(),
    );
    assert!(rect_count > 0);
    let after = session.transact(&command(0, &[])).unwrap().1;
    assert_eq!(
        after, before,
        "query preserves state and its selection rectangles"
    );
}

#[test]
fn malformed_search_frames_and_stale_revisions_are_rejected() {
    let session = load("query");
    let mut valid = Vec::new();
    u64le(revision(&session), &mut valid);
    valid.extend_from_slice(&[0, 0, 0, 0]);
    u32le(1, &mut valid);
    valid.push(b'q');
    for (name, body, code) in [
        (
            "unknown flags",
            {
                let mut b = valid.clone();
                b[8] = 0x80;
                b
            },
            "EDITOR_INVALID_COMMAND",
        ),
        (
            "reserved byte",
            {
                let mut b = valid.clone();
                b[9] = 1;
                b
            },
            "EDITOR_INVALID_COMMAND",
        ),
        (
            "reserved word",
            {
                let mut b = valid.clone();
                b[10] = 1;
                b
            },
            "EDITOR_INVALID_COMMAND",
        ),
        (
            "trailing bytes",
            {
                let mut b = valid.clone();
                b.push(0);
                b
            },
            "EDITOR_INVALID_COMMAND",
        ),
        (
            "declared length mismatch",
            {
                let mut b = valid.clone();
                b[12..16].copy_from_slice(&2u32.to_le_bytes());
                b
            },
            "EDITOR_INVALID_COMMAND",
        ),
        (
            "malformed UTF-8",
            {
                let mut b = valid.clone();
                b[16] = 0xff;
                b
            },
            "EDITOR_INVALID_UTF8",
        ),
    ] {
        assert_eq!(error_code(&session, &command(19, &body)), code, "{name}");
    }
    let mut stale = valid;
    stale[..8].copy_from_slice(&(revision(&session) + 1).to_le_bytes());
    assert_eq!(
        error_code(&session, &command(19, &stale)),
        "EDITOR_STALE_QUERY"
    );
    let mut stale_stats = Vec::new();
    u64le(revision(&session) + 1, &mut stale_stats);
    assert_eq!(
        error_code(&session, &command(20, &stale_stats)),
        "EDITOR_STALE_QUERY"
    );
    stale_stats.push(0);
    assert_eq!(
        error_code(&session, &command(20, &stale_stats)),
        "EDITOR_INVALID_COMMAND"
    );
    let mut too_long = Vec::new();
    u64le(revision(&session), &mut too_long);
    too_long.extend_from_slice(&[0, 0, 0, 0]);
    u32le(1025, &mut too_long);
    too_long.extend(std::iter::repeat_n(b'x', 1025));
    assert_eq!(
        error_code(&session, &command(19, &too_long)),
        "EDITOR_INVALID_COMMAND"
    );
}

#[test]
fn empty_and_capped_searches_have_bounded_envelopes() {
    let empty = load("nothing");
    let no_hits = search(&empty, "", 0).unwrap();
    assert_eq!(u32::from_le_bytes(no_hits[16..20].try_into().unwrap()), 0);
    assert_eq!(no_hits[20], 0);

    let full = load(&"x ".repeat(50_000));
    let capped = search(&full, "x", 0).unwrap();
    assert_eq!(u32::from_le_bytes(capped[16..20].try_into().unwrap()), 1000);
    assert_eq!(capped[20], 1);
    let mut offset = 24;
    let mut excerpts = 0;
    while excerpts < 1000 {
        let excerpt_len =
            u32::from_le_bytes(capped[offset + 8..offset + 12].try_into().unwrap()) as usize;
        assert!(
            excerpt_len <= 512,
            "ordinary excerpts stay within their byte budget"
        );
        offset += 24 + excerpt_len;
        let rects = u32::from_le_bytes(capped[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4 + rects * 36;
        excerpts += 1;
    }
    assert_eq!(offset, capped.len());
    assert!(capped.len() < 600_000);
}

#[test]
fn statistics_reports_selection_counts_and_layout_totals_without_mutation() {
    let session = load("one two\nthree");
    let mut select = Vec::new();
    u32le(0, &mut select);
    u32le(3, &mut select);
    let session = session.transact(&command(7, &select)).unwrap().0;
    let before = session.transact(&command(0, &[])).unwrap().1;
    let mut body = Vec::new();
    u64le(revision(&session), &mut body);
    let (candidate, result) = session.transact(&command(20, &body)).unwrap();
    assert_eq!(&result[..4], b"ATQT");
    assert_eq!(result.len(), 72);
    assert_eq!(u32::from_le_bytes(result[16..20].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(result[20..24].try_into().unwrap()), 3);
    assert_eq!(
        u32::from_le_bytes(result[24..28].try_into().unwrap()),
        3,
        "document words"
    );
    assert_eq!(
        u32::from_le_bytes(result[40..44].try_into().unwrap()),
        2,
        "document layout lines"
    );
    assert_eq!(
        u32::from_le_bytes(result[44..48].try_into().unwrap()),
        1,
        "document layout pages"
    );
    assert_eq!(
        u32::from_le_bytes(result[48..52].try_into().unwrap()),
        1,
        "selected words"
    );
    assert_eq!(
        u32::from_le_bytes(result[64..68].try_into().unwrap()),
        1,
        "selected touched lines"
    );
    assert_eq!(
        u32::from_le_bytes(result[68..72].try_into().unwrap()),
        1,
        "selected touched pages"
    );
    assert_eq!(candidate.transact(&command(0, &[])).unwrap().1, before);

    let mut select_last_line = Vec::new();
    u32le(8, &mut select_last_line);
    u32le(13, &mut select_last_line);
    let last_line = candidate
        .transact(&command(7, &select_last_line))
        .unwrap()
        .0;
    let mut last_line_request = Vec::new();
    u64le(revision(&last_line), &mut last_line_request);
    let (_, last_line_stats) = last_line
        .transact(&command(20, &last_line_request))
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(last_line_stats[64..68].try_into().unwrap()),
        1,
        "selecting the final line excludes the preceding newline and line"
    );
    assert_eq!(
        u32::from_le_bytes(last_line_stats[68..72].try_into().unwrap()),
        1,
        "the final line touches one page"
    );

    let crlf = load("a\r\nb");
    let mut select_crlf = Vec::new();
    u32le(1, &mut select_crlf);
    u32le(3, &mut select_crlf);
    let crlf = crlf.transact(&command(7, &select_crlf)).unwrap().0;
    let mut crlf_request = Vec::new();
    u64le(revision(&crlf), &mut crlf_request);
    let (_, crlf_stats) = crlf.transact(&command(20, &crlf_request)).unwrap();
    assert_eq!(
        u32::from_le_bytes(crlf_stats[64..68].try_into().unwrap()),
        1,
        "selecting CRLF counts its preceding logical line once"
    );
    assert_eq!(
        u32::from_le_bytes(crlf_stats[68..72].try_into().unwrap()),
        1
    );

    let mut collapse = Vec::new();
    u32le(3, &mut collapse);
    u32le(3, &mut collapse);
    let collapsed = candidate.transact(&command(7, &collapse)).unwrap().0;
    let mut stats_request = Vec::new();
    u64le(revision(&collapsed), &mut stats_request);
    let (_, empty_selection) = collapsed.transact(&command(20, &stats_request)).unwrap();
    assert!(empty_selection[48..72].iter().all(|byte| *byte == 0));
}
