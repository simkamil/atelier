// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_core_wasm::rich_preview::{import_preview_docx, preview_docx, render_imported_docx};
use atelier_model::Transaction;
use atelier_text_model::rich::RichOp;

const REFERENCE: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");

#[test]
fn wrapper_and_retained_render_seams_produce_identical_preview() {
    let expected = preview_docx(REFERENCE).unwrap();
    let mut imported = import_preview_docx(REFERENCE).unwrap();
    let actual = render_imported_docx(&mut imported).unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn retained_text_edits_change_render_without_changing_package() {
    let mut imported = import_preview_docx(REFERENCE).unwrap();
    let package_before = imported.package.write().unwrap();
    let paragraph = imported.paragraph_ids()[0];
    let edit = Transaction::new(vec![RichOp::InsertText {
        para: paragraph,
        offset: 0,
        text: "Retained edit: ".into(),
    }])
    .unwrap();
    imported.document.apply(&edit).unwrap();

    let rendered = render_imported_docx(&mut imported).unwrap();
    assert!(rendered.text.starts_with("Retained edit: "));
    assert_eq!(imported.package.write().unwrap(), package_before);
}
