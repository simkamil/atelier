// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_core_wasm::editor::EditorSession;

fn command(op: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"ATED\x01\x00"[..]);
    bytes.extend_from_slice(&op.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn state(session: &EditorSession) -> Vec<u8> {
    session.transact(&command(0, &[])).unwrap().1
}

fn text(state: &[u8]) -> &str {
    let len = u32_at(state, 24) as usize;
    core::str::from_utf8(&state[64..64 + len]).unwrap()
}

fn set_selection(session: &EditorSession, anchor: u32, focus: u32) -> EditorSession {
    let mut body = anchor.to_le_bytes().to_vec();
    body.extend_from_slice(&focus.to_le_bytes());
    session.transact(&command(7, &body)).unwrap().0
}

fn drop_body(
    revision: u64,
    now: u64,
    anchor: u32,
    focus: u32,
    target: u32,
    kind: u8,
    text: &[u8],
) -> Vec<u8> {
    let mut body = revision.to_le_bytes().to_vec();
    body.extend_from_slice(&now.to_le_bytes());
    body.extend_from_slice(&anchor.to_le_bytes());
    body.extend_from_slice(&focus.to_le_bytes());
    body.extend_from_slice(&target.to_le_bytes());
    body.push(kind);
    body.extend_from_slice(&[0, 0, 0]);
    body.extend_from_slice(text);
    body
}

fn replace_body(revision: u64, now: u64, start: u32, end: u32, text: &[u8]) -> Vec<u8> {
    let mut body = revision.to_le_bytes().to_vec();
    body.extend_from_slice(&now.to_le_bytes());
    body.extend_from_slice(&start.to_le_bytes());
    body.extend_from_slice(&end.to_le_bytes());
    body.extend_from_slice(text);
    body
}

#[test]
fn point_probe_returns_atpt_without_changing_selection_revision_or_undo_history() {
    let mut session = EditorSession::default()
        .transact(&command(1, b"abc"))
        .unwrap()
        .0;
    session = set_selection(&session, 3, 1);
    let mut insert = 1u64.to_le_bytes().to_vec();
    insert.extend_from_slice(b"!");
    session = session.transact(&command(2, &insert)).unwrap().0;
    session = set_selection(&session, 2, 1);
    let before = state(&session);

    let mut probe = 0u32.to_le_bytes().to_vec();
    probe.extend_from_slice(&54.0f64.to_le_bytes());
    probe.extend_from_slice(&65.0f64.to_le_bytes());
    let (probed, response) = session.transact(&command(15, &probe)).unwrap();
    assert_eq!(&response[..4], b"ATPT");
    assert_eq!(response.len(), 48);
    assert_eq!(u64_at(&response, 8), u64_at(&before, 8));
    assert!(u32_at(&response, 16) <= u32_at(&before, 24));
    let after_probe = state(&probed);
    assert_eq!(
        after_probe, before,
        "probe preserves directed selection, revision, and undo flags"
    );

    let undone = probed.transact(&command(5, &[])).unwrap().0;
    let after_undo = state(&undone);
    assert_eq!(text(&after_undo), "abc");
    assert_eq!(u32_at(&after_undo, 16), 3);
    assert_eq!(u32_at(&after_undo, 20), 1);
}

#[test]
fn drop_move_before_after_and_copy_inside_are_single_reversible_transactions() {
    let mut moved = EditorSession::default()
        .transact(&command(1, b"\xef\xbb\xbfA\xc3\xa9\r\nBC"))
        .unwrap()
        .0;
    moved = set_selection(&moved, 6, 5);
    let revision = u64_at(&state(&moved), 8);
    moved = moved
        .transact(&command(16, &drop_body(revision, 10, 6, 5, 0, 0, b"")))
        .unwrap()
        .0;
    let after_move = state(&moved);
    assert_eq!(text(&after_move), "BAé\r\nC");
    assert_eq!(u32_at(&after_move, 16), 1);
    moved = moved.transact(&command(5, &[])).unwrap().0;
    let restored = state(&moved);
    assert_eq!(text(&restored), "Aé\r\nBC");
    assert_eq!(u32_at(&restored, 16), 6);
    assert_eq!(u32_at(&restored, 20), 5);
    moved = moved.transact(&command(6, &[])).unwrap().0;
    assert_eq!(text(&state(&moved)), "BAé\r\nC");
    let exported = moved.transact(&command(10, &[])).unwrap().1;
    assert_eq!(&exported[16..], b"\xef\xbb\xbfBA\xc3\xa9\r\nC");

    let mut after = EditorSession::default()
        .transact(&command(1, "Aé\r\nBC".as_bytes()))
        .unwrap()
        .0;
    after = set_selection(&after, 5, 6);
    let revision = u64_at(&state(&after), 8);
    after = after
        .transact(&command(
            16,
            &drop_body(revision, 20, 5, 6, "Aé\r\nBC".len() as u32, 0, b""),
        ))
        .unwrap()
        .0;
    assert_eq!(text(&state(&after)), "Aé\r\nCB");
    after = after.transact(&command(5, &[])).unwrap().0;
    let restored = state(&after);
    assert_eq!(text(&restored), "Aé\r\nBC");
    assert_eq!((u32_at(&restored, 16), u32_at(&restored, 20)), (5, 6));

    let mut copied = EditorSession::default()
        .transact(&command(1, b"abcdef"))
        .unwrap()
        .0;
    copied = set_selection(&copied, 1, 4);
    let revision = u64_at(&state(&copied), 8);
    copied = copied
        .transact(&command(16, &drop_body(revision, 30, 1, 4, 2, 1, b"")))
        .unwrap()
        .0;
    assert_eq!(text(&state(&copied)), "abbcdcdef");
    copied = copied.transact(&command(5, &[])).unwrap().0;
    let restored = state(&copied);
    assert_eq!(text(&restored), "abcdef");
    assert_eq!((u32_at(&restored, 16), u32_at(&restored, 20)), (1, 4));
    copied = copied.transact(&command(6, &[])).unwrap().0;
    assert_eq!(text(&state(&copied)), "abbcdcdef");
}

#[test]
fn external_drop_uses_source_zero_and_inserts_at_target() {
    let mut session = EditorSession::default()
        .transact(&command(1, b"ab"))
        .unwrap()
        .0;
    let revision = u64_at(&state(&session), 8);
    session = session
        .transact(&command(
            16,
            &drop_body(revision, 1, 0, 0, 1, 2, "é".as_bytes()),
        ))
        .unwrap()
        .0;
    assert_eq!(text(&state(&session)), "aéb");
    session = session.transact(&command(5, &[])).unwrap().0;
    assert_eq!(text(&state(&session)), "ab");
}

#[test]
fn french_spacing_replacement_undo_keeps_raw_punctuation_then_redoes_spacing() {
    let mut session = EditorSession::default()
        .transact(&command(1, b"mot"))
        .unwrap()
        .0;
    session = set_selection(&session, 3, 3);
    let mut insert = 10u64.to_le_bytes().to_vec();
    insert.extend_from_slice(b":");
    session = session.transact(&command(2, &insert)).unwrap().0;
    assert_eq!(text(&state(&session)), "mot:");
    let revision = u64_at(&state(&session), 8);
    session = session
        .transact(&command(
            17,
            &replace_body(revision, 11, 3, 4, "\u{00a0}:".as_bytes()),
        ))
        .unwrap()
        .0;
    assert_eq!(text(&state(&session)), "mot\u{00a0}:");

    session = session.transact(&command(5, &[])).unwrap().0;
    let raw = state(&session);
    assert_eq!(text(&raw), "mot:");
    assert_eq!(u32_at(&raw, 20), 4);
    session = session.transact(&command(6, &[])).unwrap().0;
    assert_eq!(text(&state(&session)), "mot\u{00a0}:");

    let mut next = 12u64.to_le_bytes().to_vec();
    next.extend_from_slice(b"x");
    session = session.transact(&command(2, &next)).unwrap().0;
    session = session.transact(&command(5, &[])).unwrap().0;
    assert_eq!(
        text(&state(&session)),
        "mot\u{00a0}:",
        "later typing has its own undo group"
    );
    session = session.transact(&command(5, &[])).unwrap().0;
    assert_eq!(
        text(&state(&session)),
        "mot:",
        "next undo removes only spacing"
    );
}

#[test]
fn stale_or_malformed_commands_fail_without_publishing_state() {
    let mut session = EditorSession::default()
        .transact(&command(1, b"abc"))
        .unwrap()
        .0;
    session = set_selection(&session, 1, 2);
    let before = state(&session);
    let revision = u64_at(&before, 8);

    assert_eq!(
        session
            .transact(&command(17, &replace_body(revision + 1, 1, 0, 1, b"x")))
            .err()
            .unwrap()
            .0,
        "EDITOR_STALE_EDIT"
    );
    assert_eq!(
        session
            .transact(&command(17, &replace_body(revision, 1, 1, 99, b"x")))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_SELECTION"
    );
    assert_eq!(
        session
            .transact(&command(17, &replace_body(revision, 1, 0, 1, &[0xff])))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_UTF8"
    );

    for (kind, reserved) in [(3, [0, 0, 0]), (0, [1, 0, 0])] {
        let mut body = drop_body(revision, 1, 1, 2, 0, kind, b"");
        body[28] = kind;
        body[29..32].copy_from_slice(&reserved);
        assert_eq!(
            session.transact(&command(16, &body)).err().unwrap().0,
            "EDITOR_INVALID_COMMAND"
        );
    }
    assert_eq!(
        session
            .transact(&command(16, &drop_body(revision + 1, 1, 1, 2, 0, 0, b"")))
            .err()
            .unwrap()
            .0,
        "EDITOR_STALE_EDIT"
    );
    assert_eq!(
        session
            .transact(&command(16, &drop_body(revision, 1, 1, 2, 99, 0, b"")))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_SELECTION"
    );
    assert_eq!(
        session
            .transact(&command(16, &drop_body(revision, 1, 1, 0, 0, 2, b"x")))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_COMMAND"
    );

    let oversized = vec![b'x'; 128 * 1024 + 1];
    assert_eq!(
        session
            .transact(&command(17, &replace_body(revision, 1, 0, 1, &oversized)))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_COMMAND"
    );
    assert_eq!(
        state(&session),
        before,
        "failed transactions leave published state unchanged"
    );
}
