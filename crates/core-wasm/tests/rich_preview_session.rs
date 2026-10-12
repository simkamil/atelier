// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_core_wasm::editor::EditorSession;
use atelier_fmt_ooxml::{Package, PartUri, TargetMode};

#[test]
fn common_empty_paragraphs_can_be_previewed() {
    let mut package = Package::new();
    package.insert_part(PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/><w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p/></w:body></w:document>"#.to_vec()).unwrap();
    package
        .add_relationship(
            None,
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "word/document.xml",
            TargetMode::Internal,
        )
        .unwrap();
    let preview = atelier_core_wasm::rich_preview::preview_docx(&package.write().unwrap()).unwrap();
    assert_eq!(preview.text, "\nHello\n");
    assert_eq!(preview.pages.len(), 1);
}

fn command(op: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"ATED\x01\x00".to_vec();
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(body);
    out
}
#[test]
fn preview_success_and_failure_preserve_txt_selection_history_dirty_and_revision() {
    let (session, _) = EditorSession::default()
        .transact(&command(1, b"seed\r\n"))
        .unwrap();
    let mut insert = 10u64.to_le_bytes().to_vec();
    insert.extend_from_slice("é".as_bytes());
    let (session, _) = session.transact(&command(2, &insert)).unwrap();
    let before = session.transact(&command(0, &[])).unwrap().1;
    let exported = session.transact(&command(10, &[])).unwrap().1;
    let undone = session.transact(&command(5, &[])).unwrap().1;
    let (after, preview) = session
        .transact(&command(
            18,
            include_bytes!("../../../tests/docx-reference/basic.docx"),
        ))
        .unwrap();
    assert_eq!(&preview[..4], b"ATRP");
    assert_eq!(after.transact(&command(0, &[])).unwrap().1, before);
    assert_eq!(after.transact(&command(10, &[])).unwrap().1, exported);
    assert_eq!(after.transact(&command(5, &[])).unwrap().1, undone);
    assert_eq!(
        after.transact(&command(18, b"broken")).err().unwrap().0,
        "EDITOR_PREVIEW_FAILED"
    );
    assert_eq!(after.transact(&command(0, &[])).unwrap().1, before);
    assert_eq!(
        after
            .transact(&command(18, &vec![0; 8 * 1024 * 1024 + 1]))
            .err()
            .unwrap()
            .0,
        "EDITOR_INVALID_COMMAND"
    );
    assert_eq!(after.transact(&command(0, &[])).unwrap().1, before);
}
