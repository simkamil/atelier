// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: save stamp of the staged export (kind 3). Reading and editing the
//! properties go through the rich editor (`rich_editor/find_tests.rs`); the
//! plain opcodes 26/27 were removed in R8, so the saved file is read here with
//! the OOXML crate directly.

use atelier_core_wasm::protocol::{Message, MessageType};
use atelier_core_wasm::rich_preview::import_preview_docx;
use atelier_core_wasm::staged::{KIND_EXPORT_SAVE, OP_BEGIN, OP_STEP, StagedEngine};
use atelier_fmt_ooxml::{Package, PropertyChanges, PropertyDate};

const BASIC: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");

fn command(opcode: u16, body: &[u8]) -> Vec<u8> {
    let mut payload = b"ATED".to_vec();
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&opcode.to_le_bytes());
    payload.extend_from_slice(body);
    payload
}

struct Snapshot {
    title: String,
    author: String,
    dates: Vec<Option<String>>,
    opts: Vec<Option<String>>,
    counters: Vec<Option<u64>>,
}

fn read(docx: &[u8]) -> Snapshot {
    let snapshot = Package::read(docx).unwrap().properties_snapshot();
    let (e, r) = (&snapshot.editable, &snapshot.read_only);
    let date = |d: &Option<PropertyDate>| match d {
        Some(PropertyDate::Valid(v)) => Some(v.to_w3cdtf()),
        _ => None,
    };
    let st = &r.statistics;
    Snapshot {
        title: e.title.clone(),
        author: e.author.clone(),
        dates: vec![date(&r.created), date(&r.modified)],
        opts: vec![
            r.last_modified_by.clone(),
            r.revision.clone(),
            r.application.clone(),
            r.app_version.clone(),
        ],
        counters: vec![
            r.total_time,
            st.pages,
            st.words,
            st.characters,
            st.characters_with_spaces,
            st.lines,
            st.paragraphs,
        ],
    }
}

fn edited(docx: &[u8]) -> Vec<u8> {
    let mut package = Package::read(docx).unwrap();
    let changes = PropertyChanges {
        title: Some("Titre".into()),
        author: Some("Ada".into()),
        ..Default::default()
    };
    package.apply_property_changes(&changes).unwrap();
    package.write().unwrap()
}

fn stamped_export(docx: &[u8], now: i64, user: &str) -> Vec<u8> {
    let mut input = now.to_le_bytes().to_vec();
    for text in [user, "0.1.0"] {
        input.extend_from_slice(&(text.len() as u16).to_le_bytes());
        input.extend_from_slice(text.as_bytes());
    }
    input.extend_from_slice(docx);
    let mut body = KIND_EXPORT_SAVE.to_le_bytes().to_vec();
    body.extend_from_slice(&[0, 0]);
    body.extend_from_slice(&input);
    let mut engine = StagedEngine::default();
    let msg = |opcode: u16, body: &[u8]| Message {
        kind: MessageType::Editor,
        request_id: 1,
        payload: command(opcode, body),
    };
    let reply = engine.respond(&msg(OP_BEGIN, &body));
    assert_eq!(reply.kind, MessageType::EditorResult);
    let id = reply.payload[16..24].to_vec();
    loop {
        let reply = engine.respond(&msg(OP_STEP, &id));
        assert_eq!(reply.kind, MessageType::EditorResult);
        if &reply.payload[..4] == b"ATSV" {
            return reply.payload[12..].to_vec();
        }
    }
}

#[test]
fn save_stamps_dates_user_revision_application_and_statistics() {
    let saved = stamped_export(&edited(BASIC), 1_700_000_000, "Grace");
    let s = read(&saved);
    assert_eq!((s.title.as_str(), s.author.as_str()), ("Titre", "Ada"));
    assert_eq!(
        s.dates[0].as_deref(),
        Some("2023-11-14T22:13:20Z"),
        "created set"
    );
    assert_eq!(
        s.dates[1].as_deref(),
        Some("2023-11-14T22:13:20Z"),
        "modified"
    );
    assert_eq!(s.opts[0].as_deref(), Some("Grace"));
    assert_eq!(s.opts[1].as_deref(), Some("1"));
    assert_eq!(s.opts[2].as_deref(), Some("Atelier"));
    assert_eq!(s.opts[3].as_deref(), Some("0.1.0"));
    // counters: total time, pages, words, characters, characters with spaces, lines, paragraphs
    assert!(s.counters[2].is_some_and(|w| w > 0), "{:?}", s.counters);
    assert!(s.counters[3].is_some() && s.counters[4] >= s.counters[3]);
    assert!(s.counters[6].is_some_and(|p| p > 0));
    // A later save increments the revision and keeps the creation date.
    let again = read(&stamped_export(&saved, 1_700_086_400, "Grace"));
    assert_eq!(again.opts[1].as_deref(), Some("2"));
    assert_eq!(again.dates[0].as_deref(), Some("2023-11-14T22:13:20Z"));
    assert_eq!(again.dates[1].as_deref(), Some("2023-11-15T22:13:20Z"));
    import_preview_docx(&saved).expect("saved docx reopens");
}

#[test]
fn save_stamp_rejects_an_invalid_prefix() {
    let mut engine = StagedEngine::default();
    let mut body = KIND_EXPORT_SAVE.to_le_bytes().to_vec();
    body.extend_from_slice(&[0, 0, 1, 2, 3]);
    let reply = engine.respond(&Message {
        kind: MessageType::Editor,
        request_id: 1,
        payload: command(OP_BEGIN, &body),
    });
    assert_eq!(reply.kind, MessageType::Failed);
}
