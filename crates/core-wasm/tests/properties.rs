// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: document-property commands of the engine ABI (opcodes 26/27) and the
//! save stamp of the staged export (kind 3).

use atelier_core_wasm::editor::EditorSession;
use atelier_core_wasm::properties::{FIELDS, OP_APPLY, OP_READ};
use atelier_core_wasm::protocol::{Message, MessageType};
use atelier_core_wasm::rich_preview::import_preview_docx;
use atelier_core_wasm::staged::{KIND_EXPORT_SAVE, OP_BEGIN, OP_STEP, StagedEngine};

const BASIC: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");

fn command(opcode: u16, body: &[u8]) -> Vec<u8> {
    let mut payload = b"ATED".to_vec();
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&opcode.to_le_bytes());
    payload.extend_from_slice(body);
    payload
}

fn run(opcode: u16, body: &[u8]) -> Result<Vec<u8>, &'static str> {
    EditorSession::default()
        .transact(&command(opcode, body))
        .map(|(_, payload)| payload)
        .map_err(|e| e.0)
}

struct Snapshot {
    fields: Vec<String>,
    limits: Vec<u32>,
    dates: Vec<(u8, String)>,
    opts: Vec<Option<String>>,
    counters: Vec<Option<u64>>,
    notices: u32,
}

struct Cur<'a>(&'a [u8]);
impl Cur<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().unwrap())
    }
    fn string(&mut self) -> String {
        let n = self.u32() as usize;
        String::from_utf8(self.take(n).to_vec()).unwrap()
    }
}

fn read(docx: &[u8]) -> Snapshot {
    let payload = run(OP_READ, docx).unwrap();
    assert_eq!(&payload[..4], b"ATPP");
    let mut c = Cur(&payload[8..]);
    let fields = (0..11).map(|_| c.string()).collect();
    let limits = (0..11).map(|_| c.u32()).collect();
    let dates = (0..3)
        .map(|_| {
            let k = c.u8();
            (k, c.string())
        })
        .collect();
    let opts = (0..5)
        .map(|_| {
            let p = c.u8();
            let s = c.string();
            (p == 1).then_some(s)
        })
        .collect();
    let mut counters = Vec::new();
    // total time, then pages, words, characters, characters with spaces, lines, paragraphs
    for _ in 0..7 {
        let p = c.u8();
        let v = c.u64();
        counters.push((p == 1).then_some(v));
    }
    let notices = c.u32();
    assert!(c.0.is_empty());
    Snapshot {
        fields,
        limits,
        dates,
        opts,
        counters,
        notices,
    }
}

fn apply_body(docx: &[u8], changes: &[(&str, &str)]) -> Vec<u8> {
    let mut body = (docx.len() as u32).to_le_bytes().to_vec();
    body.extend_from_slice(docx);
    for name in FIELDS {
        match changes.iter().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                body.push(1);
                body.extend_from_slice(&(v.len() as u32).to_le_bytes());
                body.extend_from_slice(v.as_bytes());
            }
            None => {
                body.push(0);
                body.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }
    body
}

/// Returns the new DOCX, or the `(field, max, len)` refusal.
fn apply(docx: &[u8], changes: &[(&str, &str)]) -> Result<(u32, Vec<u8>), (usize, u32, u32)> {
    let payload = run(OP_APPLY, &apply_body(docx, changes)).unwrap();
    assert_eq!(&payload[..4], b"ATPA");
    let status = u16::from_le_bytes([payload[6], payload[7]]);
    let word = |i: usize| u32::from_le_bytes(payload[i..i + 4].try_into().unwrap());
    if status == 1 {
        assert_eq!(payload.len(), 20);
        return Err((word(8) as usize, word(12), word(16)));
    }
    assert_eq!(status, 0);
    Ok((word(8), payload[12..].to_vec()))
}

#[test]
fn reads_a_document_without_property_parts() {
    let s = read(BASIC);
    assert!(s.fields.iter().all(String::is_empty));
    assert_eq!(s.limits[0], 255);
    assert_eq!(s.limits[7], 4096);
    assert!(s.dates.iter().all(|d| d.0 == 0));
    assert_eq!(s.notices, 0);
}

#[test]
fn applies_and_reads_back_all_editable_fields() {
    let edits = [
        ("title", "Rapport « Œuvre » — 2026"),
        ("subject", "Sujet & <test>"),
        ("author", "Élodie Müller"),
        ("keywords", "un; deux; trois"),
        ("comments", "Ligne un\nLigne deux"),
        ("category", "Finance"),
    ];
    let (changed, docx) = apply(BASIC, &edits).unwrap();
    assert_eq!(changed, 6);
    let s = read(&docx);
    assert_eq!(s.fields[0], "Rapport « Œuvre » — 2026");
    assert_eq!(s.fields[1], "Sujet & <test>");
    assert_eq!(s.fields[2], "Élodie Müller");
    assert_eq!(s.fields[5], "Finance");
    assert_eq!(s.fields[6], "un; deux; trois");
    assert!(s.fields[7].starts_with("Ligne un") && s.fields[7].ends_with("Ligne deux"));
    // The document itself is untouched and still opens.
    import_preview_docx(&docx).expect("modified docx reopens");
    // A second round changes one field and removes another.
    let (changed, docx2) = apply(&docx, &[("title", "Autre"), ("category", "")]).unwrap();
    assert_eq!(changed, 2);
    let s = read(&docx2);
    assert_eq!((s.fields[0].as_str(), s.fields[5].as_str()), ("Autre", ""));
    assert_eq!(s.fields[2], "Élodie Müller");
}

#[test]
fn unchanged_values_return_the_input_byte_for_byte() {
    let (changed, docx) = apply(BASIC, &[]).unwrap();
    assert_eq!((changed, docx.as_slice()), (0, BASIC));
    let (_, edited) = apply(BASIC, &[("title", "T")]).unwrap();
    let (changed, same) = apply(&edited, &[("title", "T")]).unwrap();
    assert_eq!((changed, same), (0, edited));
}

#[test]
fn too_long_is_refused_with_field_max_and_length_and_applies_nothing() {
    let long = "é".repeat(256);
    assert_eq!(
        apply(BASIC, &[("author", "ok"), ("title", &long)]).unwrap_err(),
        (0, 255, 256)
    );
    let at_limit = "é".repeat(255);
    assert!(apply(BASIC, &[("title", &at_limit)]).is_ok());
    let comments = "x".repeat(4097);
    assert_eq!(
        apply(BASIC, &[("comments", &comments)]).unwrap_err(),
        (7, 4096, 4097)
    );
}

#[test]
fn malformed_commands_fail_without_panicking() {
    assert!(run(OP_READ, b"not a zip").is_err());
    assert!(run(OP_APPLY, &[]).is_err());
    assert!(run(OP_APPLY, &[1, 0, 0, 0]).is_err());
    let body = apply_body(BASIC, &[("title", "x")]);
    for cut in [1, 5, 40, body.len() - 1] {
        assert!(run(OP_APPLY, &body[..body.len() - cut]).is_err(), "{cut}");
    }
    let mut trailing = body.clone();
    trailing.push(0);
    assert!(run(OP_APPLY, &trailing).is_err());
    let mut bad_flag = body.clone();
    bad_flag[4 + BASIC.len()] = 2;
    assert!(run(OP_APPLY, &bad_flag).is_err());
    // Invalid UTF-8 in the first field's text.
    let mut bad_utf8 = apply_body(BASIC, &[("title", "ab")]);
    bad_utf8[4 + BASIC.len() + 5] = 0xff;
    assert!(run(OP_APPLY, &bad_utf8).is_err());
    // Neighbouring opcodes stay unsupported.
    assert!(run(28, &[]).is_err());
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
    let (_, edited) = apply(BASIC, &[("title", "Titre"), ("author", "Ada")]).unwrap();
    let saved = stamped_export(&edited, 1_700_000_000, "Grace");
    let s = read(&saved);
    assert_eq!(
        (s.fields[0].as_str(), s.fields[2].as_str()),
        ("Titre", "Ada")
    );
    assert_eq!(
        s.dates[0],
        (1, "2023-11-14T22:13:20Z".into()),
        "created set"
    );
    assert_eq!(s.dates[1], (1, "2023-11-14T22:13:20Z".into()), "modified");
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
    assert_eq!(again.dates[0].1, "2023-11-14T22:13:20Z");
    assert_eq!(again.dates[1].1, "2023-11-15T22:13:20Z");
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
