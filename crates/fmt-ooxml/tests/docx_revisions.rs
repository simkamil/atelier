// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-218: detection of tracked changes and comments, and the accepted copy.
mod support;

use atelier_fmt_ooxml::{
    AcceptError, DocxWriteOptions, Package, PartUri, accept_all_revisions, import_docx,
    scan_revisions, write_docx,
};
use support::{body, doc};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/docx-reference/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn texts(bytes: &[u8]) -> Vec<String> {
    let imported = import_docx(bytes).unwrap();
    imported
        .document
        .paragraph_ids()
        .into_iter()
        .map(|id| imported.document.paragraph(id).unwrap().text.clone())
        .collect()
}

#[test]
fn revisions_fixture_is_detected_and_accepted() {
    let bytes = fixture("revisions.docx");
    let package = Package::read(&bytes).unwrap();
    let report = scan_revisions(&package);
    assert_eq!(report.insertions, 1);
    assert_eq!(report.deletions, 1);
    assert_eq!(report.moves, 2);
    assert_eq!(report.property_changes, 2);
    assert_eq!(report.comments, 0);
    assert!(report.has_revisions() && !report.has_comments());
    let accepted = accept_all_revisions(&package).unwrap();
    assert_eq!(accepted.report, report);
    let clean = accepted.package.write().unwrap();
    let after = scan_revisions(&Package::read(&clean).unwrap());
    assert!(!after.has_revisions() && !after.has_comments());
    // The accepted text is what the source already showed.
    assert_eq!(texts(&clean), texts(&bytes));
    assert_eq!(
        texts(&clean),
        [
            "Début inséré fin.",
            "",
            "déplacé",
            "gras",
            "centré",
            "suite"
        ]
    );
    // The copy writes back without a notice and without any revision.
    let written = write_docx(&import_docx(&clean).unwrap(), &DocxWriteOptions::default()).unwrap();
    assert!(written.notices.is_empty(), "{:?}", written.notices);
    assert!(!scan_revisions(&Package::read(&written.bytes).unwrap()).has_revisions());
}

#[test]
fn comments_fixture_is_detected_and_removed() {
    let bytes = fixture("comments.docx");
    let package = Package::read(&bytes).unwrap();
    let report = scan_revisions(&package);
    assert!(report.has_comments() && !report.has_revisions());
    assert_eq!(report.comments, 4, "start, end, reference and the part");
    let accepted = accept_all_revisions(&package).unwrap();
    let clean = accepted.package.write().unwrap();
    let reread = Package::read(&clean).unwrap();
    assert!(
        reread
            .part(&PartUri::parse("word/comments.xml").unwrap())
            .is_none()
    );
    assert!(
        reread
            .relationships(Some(&PartUri::parse("word/document.xml").unwrap()))
            .iter()
            .all(|r| !r.rel_type.ends_with("/comments"))
    );
    assert_eq!(texts(&clean), ["Avant commenté après.", "Fin."]);
    let written = write_docx(&import_docx(&clean).unwrap(), &DocxWriteOptions::default()).unwrap();
    assert!(written.notices.is_empty(), "{:?}", written.notices);
}

#[test]
fn corpus_documents_are_classified() {
    let basic = Package::read(&fixture("basic.docx")).unwrap();
    let report = scan_revisions(&basic);
    assert!(!report.has_revisions() && !report.has_comments());
    // The LibreOffice document carries a tracked insertion, a deletion and a comment.
    let rich = Package::read(&fixture("libreoffice/rich.docx")).unwrap();
    let report = scan_revisions(&rich);
    assert!(report.insertions > 0 && report.deletions > 0 && report.has_comments());
    let clean = accept_all_revisions(&rich)
        .unwrap()
        .package
        .write()
        .unwrap();
    let after = scan_revisions(&Package::read(&clean).unwrap());
    assert!(!after.has_revisions() && !after.has_comments());
    let all = texts(&clean).join(
        "
",
    );
    let source = texts(&fixture("libreoffice/rich.docx")).join(
        "
",
    );
    assert_eq!(all, source, "the source already showed the accepted text");
}

fn blocked(inner: &str) -> AcceptError {
    let bytes = doc(body(inner)).bytes();
    let package = Package::read(&bytes).unwrap();
    accept_all_revisions(&package).unwrap_err()
}

#[test]
fn unsupported_changes_are_refused_not_approximated() {
    let mark = r#"<w:p><w:pPr><w:rPr><w:del w:id="1" w:author="a"/></w:rPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#;
    assert!(matches!(blocked(mark), AcceptError::Unsupported(r) if r.blocked == 1));
}

#[test]
fn deleted_rows_are_removed_and_other_prefixes_work() {
    let table = r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1000"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>keep</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:trPr><w:del w:id="1" w:author="a"/></w:trPr><w:tc><w:p><w:r><w:t>gone</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>"#;
    let bytes = doc(body(table)).bytes();
    let package = Package::read(&bytes).unwrap();
    let accepted = accept_all_revisions(&package).unwrap();
    assert_eq!(accepted.report.rows_removed, 1);
    let clean = accepted.package.write().unwrap();
    let all = texts(&clean).join("|");
    assert!(all.contains("keep") && !all.contains("gone"), "{all}");
    // Another prefix for the same namespace, and a text node that only looks like a change.
    let inner = r#"<x:p xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><x:ins x:id="1" x:author="a"><x:r><x:t>&lt;w:del/&gt; kept</x:t></x:r></x:ins></x:p>"#;
    let package = Package::read(&doc(body(inner)).bytes()).unwrap();
    assert_eq!(scan_revisions(&package).insertions, 1);
    let accepted = accept_all_revisions(&package).unwrap();
    let clean = accepted.package.write().unwrap();
    assert_eq!(texts(&clean)[0], "<w:del/> kept");
}
