// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-306: cost of the capture of unknown elements.
//!
//! `capture_cost_*_in_bounded_time` are non-regression tests with wide bounds.
//! `bench_report` (ignored; run it with `--release -- --ignored --nocapture`)
//! prints the timings and opaque volumes of the review scenarios.
use atelier_fmt_ooxml::{
    DocxError, DocxLimits, ImportedDocx, Package, PartUri, TargetMode, import_docx,
    import_docx_with_limits,
};
use std::time::Instant;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAIN_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const DOC_CT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// A `w:document` root with 26 namespace declarations, like a Word file.
fn root(inner: &str) -> String {
    let mut decls = format!("xmlns:w=\"{W}\"");
    for i in 0..25 {
        decls.push_str(&format!(
            " xmlns:n{i}=\"http://schemas.example.com/office/2010/ns{i}\""
        ));
    }
    format!("<w:document {decls}><w:body>{inner}</w:body></w:document>")
}

fn package(document: &str) -> Vec<u8> {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        DOC_CT,
        document.as_bytes().to_vec(),
    )
    .unwrap();
    p.add_relationship(
        None,
        "rDoc",
        MAIN_REL,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.write().unwrap()
}

/// `count` paragraphs of text with four unknown inline elements each.
fn rich_paragraphs(count: usize) -> String {
    let mut s = String::new();
    for i in 0..count {
        s.push_str(&format!(
            "<w:p><w:bookmarkStart w:id=\"{i}\" w:name=\"b{i}\"/><w:proofErr w:type=\"spellStart\"/>\
             <w:r><w:t>Paragraph {i}</w:t></w:r><w:proofErr w:type=\"spellEnd\"/>\
             <w:bookmarkEnd w:id=\"{i}\"/></w:p>"
        ));
    }
    s
}

/// A document of 2 000 paragraphs, each with one bookmark and one `proofErr`.
fn word_like() -> String {
    let mut s = String::new();
    for i in 0..2000 {
        s.push_str(&format!(
            "<w:p><w:bookmarkStart w:id=\"{i}\" w:name=\"b{i}\"/><w:r><w:t>Paragraph {i}</w:t></w:r>\
             <w:proofErr w:type=\"spellEnd\"/></w:p>"
        ));
    }
    root(&s)
}

fn foreign_body_elements(count: usize) -> String {
    "<x:a xmlns:x=\"urn:x\"/>".repeat(count)
}

fn foreign_in_paragraphs(count: usize) -> String {
    "<w:p><x:a xmlns:x=\"urn:x\"/></w:p>".repeat(count)
}

fn timed(document: &str) -> (f64, Result<ImportedDocx, DocxError>) {
    let bytes = package(document);
    let start = Instant::now();
    let result = import_docx(&bytes);
    (start.elapsed().as_secs_f64(), result)
}

#[test]
#[ignore]
fn bench_report() {
    let scenarios: Vec<(&str, String)> = vec![
        ("8000 paras x4 unknown", root(&rich_paragraphs(8000))),
        ("16000 paras x4 unknown", root(&compact_paragraphs(16000))),
        (
            "20000 hostile body elements",
            root(&foreign_body_elements(20000)),
        ),
        (
            "20000 paras with hostile element",
            root(&foreign_in_paragraphs(20000)),
        ),
        ("word-like 2000 paras", word_like()),
    ];
    for (name, document) in scenarios {
        let (seconds, result) = timed(&document);
        match result {
            Ok(imp) => println!(
                "{name}: {seconds:.2} s, source {} B, opaque {} B in {} items",
                document.len(),
                imp.preserved.opaque_bytes,
                imp.preserved.opaque_items
            ),
            Err(e) => println!("{name}: {seconds:.2} s, error {e}"),
        }
    }
}

/// Same markup as `rich_paragraphs`, shorter values (the test package writer
/// refuses parts above about 2.8 MB).
fn compact_paragraphs(count: usize) -> String {
    "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:proofErr w:type=\"spellStart\"/>\
     <w:r><w:t>P</w:t></w:r><w:proofErr w:type=\"spellEnd\"/><w:bookmarkEnd w:id=\"1\"/></w:p>"
        .repeat(count)
}

#[test]
#[ignore]
fn bench_200000_captures_under_default_limits() {
    let document = root(&compact_paragraphs(50000));
    let (seconds, result) = timed(&document);
    let imp = result.unwrap();
    println!(
        "{} captures: {seconds:.2} s, source {} B, opaque {} B",
        imp.preserved.opaque_items,
        document.len(),
        imp.preserved.opaque_bytes
    );
    assert_eq!(imp.preserved.opaque_items, 200_000);
    assert!(imp.preserved.opaque_bytes < DocxLimits::default().max_opaque_bytes / 4);
}

#[test]
fn capture_cost_of_rich_document_is_bounded() {
    let document = root(&rich_paragraphs(2000));
    let (seconds, result) = timed(&document);
    let imp = result.unwrap();
    assert_eq!(imp.preserved.opaque_items, 8000);
    // The copies stay of the order of the source, not a multiple of it by the
    // number of namespace declarations in scope (it was 33 times before).
    assert!(
        imp.preserved.opaque_bytes < document.len(),
        "opaque {} B for source {} B",
        imp.preserved.opaque_bytes,
        document.len()
    );
    assert!(seconds < 30.0, "2000 paragraphs took {seconds} s");
}

#[test]
fn capture_cost_of_hostile_document_is_bounded() {
    let document = root(&foreign_body_elements(4000));
    let (seconds, result) = timed(&document);
    result.unwrap();
    assert!(seconds < 30.0, "4000 foreign elements took {seconds} s");
    let document = root(&foreign_in_paragraphs(4000));
    let (seconds, result) = timed(&document);
    result.unwrap();
    assert!(
        seconds < 30.0,
        "4000 paragraphs with foreign element took {seconds} s"
    );
}

#[test]
fn a_copy_costs_bytes_of_its_own_not_of_its_scope() {
    // At 200 000 captures the default 128 MiB limit leaves 671 bytes per copy.
    let bytes = package(&root(&rich_paragraphs(500)));
    let imp = import_docx(&bytes).unwrap();
    assert_eq!(imp.preserved.opaque_items, 2000);
    assert!(
        imp.preserved.opaque_bytes / imp.preserved.opaque_items < 100,
        "{} B per copy",
        imp.preserved.opaque_bytes / imp.preserved.opaque_items
    );
}

#[test]
fn the_byte_and_count_limits_cut_a_hostile_document_early() {
    let bytes = package(&root(&foreign_body_elements(20000)));
    for limit in [
        |l: &mut DocxLimits| l.max_opaque_items = 1000,
        |l: &mut DocxLimits| l.max_opaque_bytes = 20_000,
    ] {
        let mut limits = DocxLimits::default();
        limit(&mut limits);
        let start = Instant::now();
        let result = import_docx_with_limits(&bytes, limits);
        assert!(matches!(result, Err(DocxError::Limit(_))));
        assert!(start.elapsed().as_secs_f64() < 10.0);
    }
}

#[test]
fn copies_are_stored_without_the_part_declarations_and_standalone_restores_them() {
    let inner = "<w:p><w:r><w:t>x</w:t></w:r><w:proofErr w:type=\"spellStart\"/>\
                 <z:own xmlns:z=\"urn:z\" xmlns:n3=\"urn:other\"><n3:deep/></z:own></w:p>";
    let imp = import_docx(&package(&root(inner))).unwrap();
    let part = imp.preserved.document.as_ref().unwrap();
    assert_eq!(part.declarations.len(), 26);
    let paragraph = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
    let proof = paragraph
        .anchored
        .iter()
        .find(|a| a.kind == "w:proofErr")
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&proof.data),
        "<w:proofErr w:type=\"spellStart\"/>"
    );
    let own = paragraph
        .anchored
        .iter()
        .find(|a| a.kind == "z:own")
        .unwrap();
    let text = String::from_utf8_lossy(&own.data).into_owned();
    // What the copy introduces or changes stays; the rest is the part's.
    assert!(text.contains("xmlns:z=\"urn:z\"") && text.contains("xmlns:n3=\"urn:other\""));
    assert!(!text.contains("xmlns:w="), "{text}");
    // `standalone` makes a copy parseable on its own with the same meaning.
    let whole = String::from_utf8(part.standalone(&proof.data)).unwrap();
    assert!(whole.starts_with("<w:proofErr xmlns:w=\"") && whole.contains("w:type=\"spellStart\""));
    let whole = String::from_utf8(part.standalone(&own.data)).unwrap();
    assert!(
        whole.contains("xmlns:n3=\"urn:other\"")
            && !whole.contains("urn:example.com/office/2010/ns3\"")
    );
    assert!(whole.contains("xmlns:n4=\""));
    for copy in [&proof.data, &own.data] {
        let whole = part.standalone(copy);
        let mut reader = atelier_xml::Reader::new(&whole[..]);
        let mut root = None;
        while let Some(event) = reader.next_event().unwrap() {
            if let atelier_xml::Event::StartElement { name, .. } = event {
                root.get_or_insert(name);
            }
        }
        assert!(root.is_some());
    }
}
