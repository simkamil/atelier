// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_core_wasm::editor::EditorSession;

fn command(opcode: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"ATED\x01\x00"[..]);
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn state(session: &EditorSession) -> Vec<u8> {
    session.transact(&command(0, &[])).unwrap().1
}

fn replace_command(
    opcode: u16,
    revision: u64,
    query: &str,
    replacement: &str,
    flags: u8,
    selected: Option<(u32, u32)>,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&revision.to_le_bytes());
    body.extend_from_slice(&7u64.to_le_bytes());
    body.push(flags);
    body.extend_from_slice(&[0, 0, 0]);
    body.extend_from_slice(&(query.len() as u32).to_le_bytes());
    body.extend_from_slice(&(replacement.len() as u32).to_le_bytes());
    if opcode == 21 {
        let (start, end) = selected.unwrap();
        body.extend_from_slice(&start.to_le_bytes());
        body.extend_from_slice(&end.to_le_bytes());
    }
    body.extend_from_slice(query.as_bytes());
    body.extend_from_slice(replacement.as_bytes());
    command(opcode, &body)
}

fn replacement_count(response: &[u8]) -> u32 {
    assert_eq!(&response[..4], b"ATQR");
    u32::from_le_bytes(response[8..12].try_into().unwrap())
}

fn nested_state(response: &[u8]) -> &[u8] {
    assert_eq!(
        u32::from_le_bytes(response[12..16].try_into().unwrap()) as usize,
        response.len() - 16
    );
    &response[16..]
}

fn state_text(state: &[u8]) -> &str {
    let len = u32::from_le_bytes(state[24..28].try_into().unwrap()) as usize;
    std::str::from_utf8(&state[64..64 + len]).unwrap()
}

#[test]
fn replace_all_processes_the_original_matches_once_and_undoes_as_one_edit() {
    let original = "a ".repeat(1501);
    let session = EditorSession::default()
        .transact(&command(1, original.as_bytes()))
        .unwrap()
        .0;
    let selection = [1u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    let session = session.transact(&command(7, &selection)).unwrap().0;
    let before = state(&session);
    assert_eq!(
        (
            u32::from_le_bytes(before[16..20].try_into().unwrap()),
            u32::from_le_bytes(before[20..24].try_into().unwrap())
        ),
        (1, 0)
    );
    assert_eq!(u16::from_le_bytes(before[6..8].try_into().unwrap()) & 3, 0);
    let response = session
        .transact(&replace_command(
            22,
            u64::from_le_bytes(before[8..16].try_into().unwrap()),
            "a",
            "aa",
            0,
            None,
        ))
        .unwrap();
    let changed_session = response.0;
    let response = response.1;
    assert_eq!(replacement_count(&response), 1501);
    let edited = nested_state(&response);
    assert_eq!(state_text(edited), "aa ".repeat(1501));
    assert_ne!(u16::from_le_bytes(edited[6..8].try_into().unwrap()) & 1, 0);
    let (undone_session, undone) = changed_session.transact(&command(5, &[])).unwrap();
    assert_eq!(state_text(&undone), original);
    assert_eq!(
        (
            u32::from_le_bytes(undone[16..20].try_into().unwrap()),
            u32::from_le_bytes(undone[20..24].try_into().unwrap())
        ),
        (1, 0)
    );
    assert_eq!(
        u16::from_le_bytes(undone[6..8].try_into().unwrap()) & 3,
        2,
        "undo restores the selection and enables only redo"
    );
    let redone = undone_session.transact(&command(6, &[])).unwrap().1;
    assert_eq!(state_text(&redone), "aa ".repeat(1501));
    assert_eq!(
        &redone[16..24],
        &edited[16..24],
        "redo restores the post-replacement selection"
    );
    assert_eq!(
        u16::from_le_bytes(redone[6..8].try_into().unwrap()) & 3,
        1,
        "redo restores the single undo entry"
    );
}

#[test]
fn selected_unicode_match_and_stale_selection_are_checked() {
    let session = EditorSession::default()
        .transact(&command(1, "École e\u{301}cole".as_bytes()))
        .unwrap()
        .0;
    let selected = [0u32.to_le_bytes(), 6u32.to_le_bytes()].concat();
    let session = session.transact(&command(7, &selected)).unwrap().0;
    let before = state(&session);
    let revision = u64::from_le_bytes(before[8..16].try_into().unwrap());
    let response = session
        .transact(&replace_command(
            21,
            revision,
            "ecole",
            "campus",
            4,
            Some((0, 6)),
        ))
        .unwrap()
        .1;
    assert_eq!(replacement_count(&response), 1);
    assert_eq!(state_text(nested_state(&response)), "campus e\u{301}cole");
    let stale = replace_command(21, revision, "ecole", "x", 4, Some((7, 12)));
    assert_eq!(
        session.transact(&stale).err().unwrap().0,
        "EDITOR_STALE_MATCH"
    );
    assert_eq!(state_text(&state(&session)), "École e\u{301}cole");
}

#[test]
fn no_match_and_identical_replacement_keep_revision_and_history() {
    let session = EditorSession::default()
        .transact(&command(1, b"same same"))
        .unwrap()
        .0;
    let before = state(&session);
    let revision = u64::from_le_bytes(before[8..16].try_into().unwrap());
    for (query, replacement, expected_count) in [("missing", "x", 0), ("same", "same", 2)] {
        let response = session
            .transact(&replace_command(22, revision, query, replacement, 0, None))
            .unwrap()
            .1;
        assert_eq!(replacement_count(&response), expected_count);
        assert_eq!(state_text(nested_state(&response)), "same same");
        assert_eq!(
            u64::from_le_bytes(nested_state(&response)[8..16].try_into().unwrap()),
            revision
        );
        assert_eq!(
            u16::from_le_bytes(nested_state(&response)[6..8].try_into().unwrap()) & 1,
            0
        );
        assert_eq!(
            u16::from_le_bytes(nested_state(&response)[6..8].try_into().unwrap()) & 3,
            0,
            "a no-op leaves both history directions unavailable"
        );
    }
}

#[test]
fn stale_revision_invalid_frames_and_output_overflow_are_atomic() {
    let session = EditorSession::default()
        .transact(&command(1, b"old"))
        .unwrap()
        .0;
    let before = state(&session);
    assert_eq!(
        session
            .transact(&replace_command(22, 99, "o", "x", 0, None))
            .err()
            .unwrap()
            .0,
        "EDITOR_STALE_QUERY"
    );
    assert_eq!(
        session
            .transact(&replace_command(
                22,
                u64::from_le_bytes(before[8..16].try_into().unwrap()),
                "",
                "x",
                0,
                None
            ))
            .err()
            .unwrap()
            .0,
        "EDITOR_EMPTY_QUERY"
    );
    let mut malformed = replace_command(22, 0, "o", "x", 0, None);
    *malformed.last_mut().unwrap() = 0xff;
    assert_eq!(
        session.transact(&malformed).err().unwrap().0,
        "EDITOR_INVALID_UTF8"
    );
    let mut reserved = replace_command(22, 0, "o", "x", 0, None);
    reserved[8 + 17] = 1;
    assert_eq!(
        EditorSession::default()
            .transact(&reserved)
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_COMMAND"
    );
    let mut trailing = replace_command(22, 0, "o", "x", 0, None);
    trailing.push(0);
    assert_eq!(
        EditorSession::default()
            .transact(&trailing)
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_COMMAND"
    );

    let full = EditorSession::default()
        .transact(&command(1, "x".repeat(10 * 1024).as_bytes()))
        .unwrap()
        .0;
    let full_state = state(&full);
    let revision = u64::from_le_bytes(full_state[8..16].try_into().unwrap());
    assert_eq!(
        full.transact(&replace_command(
            22,
            revision,
            "x",
            &"x".repeat(20),
            0,
            None
        ))
        .err()
        .unwrap()
        .0,
        "EDITOR_TEXT_LIMIT"
    );
    assert_eq!(state_text(&state(&full)), "x".repeat(10 * 1024));
}
