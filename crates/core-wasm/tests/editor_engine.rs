// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_core_wasm::editor::EditorSession;
use atelier_core_wasm::protocol::{Direction, Message, MessageType, decode, encode};
use atelier_render::{Command, DisplayList};

fn command(op: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(&b"ATED\x01\x00"[..]);
    bytes.extend_from_slice(&op.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}
fn state(session: &EditorSession, bytes: &[u8]) -> Vec<u8> {
    session.transact(bytes).unwrap().1
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

#[test]
fn empty_and_loaded_document_emit_real_carlito_paths_and_preserve_bom_crlf() {
    let session = EditorSession::default();
    let empty = state(&session, &command(0, &[]));
    assert_eq!(&empty[..4], b"ATES");
    assert_eq!(u32_at(&empty, 24), 0);
    assert_eq!(u32_at(&empty, 28), 1);
    let pages_at = 64 + u32_at(&empty, 24) as usize;
    let page_len = u32_at(&empty, pages_at) as usize;
    let page = DisplayList::decode(&empty[pages_at + 4..pages_at + 4 + page_len]).unwrap();
    assert!((page.page.bounds.width() - 595.276).abs() < 1e-6);

    let original = b"\xef\xbb\xbfHello\r\nworld";
    let loaded = command(1, original);
    let (session, response) = session.transact(&loaded).unwrap();
    assert_eq!(&response[..4], b"ATES");
    assert_ne!(
        u16::from_le_bytes(response[6..8].try_into().unwrap()) & 8,
        0
    );
    assert_eq!(u32_at(&response, 24) as usize, original.len() - 3);
    assert_eq!(&response[64..64 + original.len() - 3], b"Hello\r\nworld");
    let at = 64 + u32_at(&response, 24) as usize;
    let len = u32_at(&response, at) as usize;
    let page = DisplayList::decode(&response[at + 4..at + 4 + len]).unwrap();
    assert!(
        page.commands
            .iter()
            .any(|cmd| matches!(cmd, Command::FillPath { path, .. } if !path.elements.is_empty()))
    );

    let export = state(&session, &command(10, &[]));
    assert_eq!(&export[..4], b"ATEX");
    assert_eq!(&export[16..], original);
}

#[test]
fn edit_undo_and_stale_save_preserve_revision_dirty_and_document_state() {
    let mut session = EditorSession::default();
    let (next, _) = session.transact(&command(1, b"abc")).unwrap();
    session = next;
    let baseline_revision = session.transact(&command(0, &[])).unwrap().1;
    assert_eq!(
        u16::from_le_bytes(baseline_revision[6..8].try_into().unwrap()) & 4,
        0
    );
    let mut selection = Vec::new();
    selection.extend_from_slice(&3u32.to_le_bytes());
    selection.extend_from_slice(&3u32.to_le_bytes());
    session = session.transact(&command(7, &selection)).unwrap().0;
    let mut insert = 10u64.to_le_bytes().to_vec();
    insert.extend_from_slice("é".as_bytes());
    session = session.transact(&command(2, &insert)).unwrap().0;
    let edited = state(&session, &command(0, &[]));
    assert_ne!(u16::from_le_bytes(edited[6..8].try_into().unwrap()) & 4, 0);
    let edited_revision = u64_at(&edited, 8);
    let mut stale_body = Vec::new();
    stale_body.extend_from_slice(&edited_revision.saturating_sub(1).to_le_bytes());
    assert_eq!(
        session.transact(&command(11, &stale_body)).err().unwrap().0,
        "EDITOR_STALE_SAVE"
    );
    session = session.transact(&command(5, &[])).unwrap().0;
    let undone = state(&session, &command(0, &[]));
    assert_eq!(&undone[64..67], b"abc");
    assert_eq!(u16::from_le_bytes(undone[6..8].try_into().unwrap()) & 4, 0);
    let mut insert = 20u64.to_le_bytes().to_vec();
    insert.extend_from_slice(b"!");
    session = session.transact(&command(2, &insert)).unwrap().0;
    let changed = state(&session, &command(0, &[]));
    let current_revision = u64_at(&changed, 8);
    session = session
        .transact(&command(11, &current_revision.to_le_bytes()))
        .unwrap()
        .0;
    let saved = state(&session, &command(0, &[]));
    assert_eq!(u16::from_le_bytes(saved[6..8].try_into().unwrap()) & 4, 0);
}

#[test]
fn malformed_commands_and_invalid_utf8_are_rejected_without_publishing_candidate() {
    let session = EditorSession::default();
    for malformed in [
        b"ATED".as_slice(),
        b"ATED\x02\x00\x00\x00",
        b"ATED\x01\x00\x00\x00x",
        b"ATED\x01\x00\x08\x00\x00\x00\x02\x00",
    ] {
        assert!(session.transact(malformed).is_err());
    }
    assert_eq!(
        session.transact(&command(1, &[0xff])).err().unwrap().0,
        "EDITOR_INVALID_UTF8"
    );
    let state = state(&session, &command(0, &[]));
    assert_eq!(u32_at(&state, 24), 0);
}

#[test]
fn atlm_editor_response_keeps_the_request_id_and_editor_types_are_registered() {
    let request = Message {
        kind: MessageType::Editor,
        request_id: 77,
        payload: command(0, &[]),
    };
    let decoded = decode(&encode(&request).unwrap(), Direction::Command).unwrap();
    assert_eq!(decoded, request);
    let (_, response_message) = EditorSession::default().prepare_message(&decoded);
    let response = encode(&response_message).unwrap();
    let decoded = decode(&response, Direction::Event).unwrap();
    assert_eq!(decoded.kind, MessageType::EditorResult);
    assert_eq!(decoded.request_id, 77);

    let malformed = Message {
        kind: MessageType::Editor,
        request_id: 78,
        payload: b"ATED\x01\x00\xff\xff".to_vec(),
    };
    let (candidate, failed) = EditorSession::default().prepare_message(&malformed);
    assert!(candidate.is_none());
    assert_eq!(failed.kind, MessageType::Failed);
    assert_eq!(failed.request_id, 78);
    assert_eq!(failed.payload, b"EDITOR_INVALID_COMMAND");
}

#[test]
fn crlf_is_removed_as_one_grapheme_and_selection_hit_testing_uses_page_slots() {
    let mut session = EditorSession::default();
    session = session.transact(&command(1, b"A\r\nB")).unwrap().0;
    let mut at_end = Vec::new();
    at_end.extend_from_slice(&4u32.to_le_bytes());
    at_end.extend_from_slice(&4u32.to_le_bytes());
    session = session.transact(&command(7, &at_end)).unwrap().0;
    let backspace = 10u64.to_le_bytes().to_vec();
    session = session.transact(&command(3, &backspace)).unwrap().0;
    let response = state(&session, &command(0, &[]));
    assert_eq!(&response[64..67], b"A\r\n");
    // Hit-test near the initial caret; the result is a valid byte offset and
    // the response exposes a selection rectangle for a nonempty selection.
    let mut hit = Vec::new();
    hit.extend_from_slice(&0u32.to_le_bytes());
    hit.extend_from_slice(&54.0f64.to_le_bytes());
    hit.extend_from_slice(&65.0f64.to_le_bytes());
    hit.push(0);
    let hit_state = session.transact(&command(9, &hit)).unwrap().1;
    assert_eq!(u32_at(&hit_state, 20), 0);
    let mut range = Vec::new();
    range.extend_from_slice(&0u32.to_le_bytes());
    range.extend_from_slice(&1u32.to_le_bytes());
    let selected = session.transact(&command(7, &range)).unwrap().1;
    assert!(u32_at(&selected, 60) > 0);
}

#[test]
fn navigation_is_safe_on_fresh_session_and_collapses_selections_by_direction() {
    let fresh = EditorSession::default();
    let left = 0u8.to_le_bytes().to_vec();
    let mut move_left = left;
    move_left.extend_from_slice(&[0, 0, 0]);
    assert!(&fresh.transact(&command(8, &move_left)).unwrap().1[..4] == b"ATES");

    let mut hit = Vec::new();
    hit.extend_from_slice(&0u32.to_le_bytes());
    hit.extend_from_slice(&54.0f64.to_le_bytes());
    hit.extend_from_slice(&65.0f64.to_le_bytes());
    hit.push(0);
    assert!(&fresh.transact(&command(9, &hit)).unwrap().1[..4] == b"ATES");

    let make_selection = |anchor: u32, focus: u32| {
        let mut body = anchor.to_le_bytes().to_vec();
        body.extend_from_slice(&focus.to_le_bytes());
        body
    };
    let session = fresh.transact(&command(1, b"abcd")).unwrap().0;
    for (anchor, focus) in [(1, 3), (3, 1)] {
        let session = session
            .transact(&command(7, &make_selection(anchor, focus)))
            .unwrap()
            .0;
        for (direction, expected) in [(0, 1), (1, 3)] {
            let mut body = vec![direction, 0, 0, 0];
            let response = session.transact(&command(8, &body)).unwrap().1;
            assert_eq!(u32_at(&response, 16), expected);
            assert_eq!(u32_at(&response, 20), expected);
            body.clear();
        }
    }
}

#[test]
fn loading_identical_content_does_not_advance_content_revision() {
    let session = EditorSession::default()
        .transact(&command(1, b"same"))
        .unwrap()
        .0;
    let before = state(&session, &command(0, &[]));
    let revision = u64_at(&before, 8);
    let (reloaded, _) = session.transact(&command(1, b"same")).unwrap();
    let after = state(&reloaded, &command(0, &[]));
    assert_eq!(u64_at(&after, 8), revision);
}

#[test]
fn loading_short_text_rebuilds_pages_and_glyph_paths_after_a_long_document() {
    let mut session = EditorSession::default()
        .transact(&command(1, b"short"))
        .unwrap()
        .0;
    let _ = state(&session, &command(0, &[]));

    let long_text = "line\n".repeat(90);
    session = session
        .transact(&command(1, long_text.as_bytes()))
        .unwrap()
        .0;
    let long_state = state(&session, &command(0, &[]));
    assert_eq!(u32_at(&long_state, 28), 2);

    session = session.transact(&command(1, b"short")).unwrap().0;
    let short_state = state(&session, &command(0, &[]));
    assert_eq!(u32_at(&short_state, 28), 1);
    let page_at = 64 + u32_at(&short_state, 24) as usize;
    let page_len = u32_at(&short_state, page_at) as usize;
    let page = DisplayList::decode(&short_state[page_at + 4..page_at + 4 + page_len]).unwrap();
    assert!(page.commands.iter().any(|command| matches!(
        command,
        Command::FillPath { path, .. } if !path.elements.is_empty()
    )));
}

#[test]
fn selecting_only_an_explicit_newline_emits_a_thin_selection_rectangle() {
    let session = EditorSession::default()
        .transact(&command(1, b"a\nb"))
        .unwrap()
        .0;
    let mut newline = Vec::new();
    newline.extend_from_slice(&1u32.to_le_bytes());
    newline.extend_from_slice(&2u32.to_le_bytes());
    let response = session.transact(&command(7, &newline)).unwrap().1;
    assert_eq!(u32_at(&response, 60), 1);
    let selection_rect_at = response.len() - 36;
    let width = f64::from_le_bytes(
        response[selection_rect_at + 20..selection_rect_at + 28]
            .try_into()
            .unwrap(),
    );
    assert_eq!(width, 1.5);
}
