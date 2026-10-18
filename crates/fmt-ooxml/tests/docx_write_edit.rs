// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-307: imported documents edited through model operations, then written.
mod support;

use atelier_fmt_ooxml::{
    DocxWriteOptions, ImportedDocx, Package, WriteNotice, import_docx, write_docx,
};
use atelier_model::Transaction;
use atelier_text_model::rich::{CharAttrs, Node, NodeId, ParaProps, RichDocument, RichOp};
use support::order::order_problems;
use support::scratch::*;
use support::*;

fn edit(imp: &mut ImportedDocx, ops: Vec<RichOp>) {
    imp.document
        .apply(&Transaction::new(ops).unwrap())
        .expect("valid edit");
}

fn written_document(imp: &ImportedDocx) -> (Vec<u8>, Vec<WriteNotice>) {
    let w = write_docx(imp, &DocxWriteOptions::default()).unwrap();
    let package = Package::read(&w.bytes).unwrap();
    let main = package
        .parts()
        .values()
        .find(|p| p.uri.as_str().ends_with("document.xml") || p.uri.as_str().contains("main"))
        .unwrap();
    (main.bytes.clone(), w.notices)
}

fn xml(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn doc_with(inner: &str) -> ImportedDocx {
    let mut d = doc(body(inner));
    d.styles = Some(STYLES.into());
    d.settings = Some(SETTINGS.into());
    d.import()
}

#[test]
fn text_inserted_and_deleted_inside_a_formatted_run_keeps_the_run() {
    let mut imp = doc_with(
        r#"<w:p w:rsidR="1"><w:pPr><w:jc w:val="center"/></w:pPr><w:r w:rsidR="2"><w:rPr><w:b/><w14:x xmlns:w14="urn:x"/></w:rPr><w:t>Hello world</w:t></w:r></w:p>"#,
    );
    let p = imp.paragraph_ids()[0];
    edit(
        &mut imp,
        vec![
            RichOp::InsertText {
                para: p,
                offset: 5,
                text: ", dear".into(),
            },
            RichOp::DeleteText {
                para: p,
                start: 0,
                end: 1,
            },
        ],
    );
    let (bytes, notices) = written_document(&imp);
    let text = xml(&bytes);
    assert!(notices.is_empty(), "{notices:?}");
    assert!(
        text.contains(r#"w:rsidR="1""#) && text.contains(r#"w:rsidR="2""#),
        "{text}"
    );
    assert!(text.contains("<w:t>ello, dear world</w:t>"), "{text}");
    assert!(text.contains("<w:b/>") && text.contains("w14:x"), "{text}");
    let again = import_docx(
        &write_docx(&imp, &DocxWriteOptions::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(
        again
            .document
            .paragraph(again.paragraph_ids()[0])
            .unwrap()
            .text,
        "ello, dear world"
    );
}

#[test]
fn formatting_applied_to_part_of_a_run_splits_it_and_orders_properties() {
    let mut imp = doc_with(
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Carlito"/><w:sz w:val="24"/></w:rPr><w:t>abcdef</w:t></w:r></w:p>"#,
    );
    let p = imp.paragraph_ids()[0];
    edit(
        &mut imp,
        vec![RichOp::SetCharAttrs {
            para: p,
            start: 2,
            end: 4,
            set: CharAttrs {
                bold: Some(true),
                color: Some(atelier_text_model::rich::Color { r: 1, g: 2, b: 255 }),
                underline: Some(atelier_text_model::rich::Underline::Double),
                ..CharAttrs::default()
            },
            clear: CharAttrs::default(),
        }],
    );
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    assert!(
        text.contains("<w:t>ab</w:t>")
            && text.contains("<w:t>cd</w:t>")
            && text.contains("<w:t>ef</w:t>"),
        "{text}"
    );
    assert!(order_problems(&parse(&bytes)).is_empty(), "{text}");
    // rFonts, b, color, sz, u in schema order in the middle run.
    let middle = text.split("<w:t>cd</w:t>").next().unwrap();
    let rpr = middle.rsplit("<w:rPr>").next().unwrap();
    let positions: Vec<_> = ["rFonts", "w:b/", "color", "w:sz", "w:u "]
        .iter()
        .map(|n| rpr.find(n).unwrap_or_else(|| panic!("{n} in {rpr}")))
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{rpr}");
}

#[test]
fn split_and_merged_paragraphs_keep_their_text_and_source_properties() {
    let mut imp = doc_with(
        r#"<w:p w14:paraId="AA"><w:pPr><w:keepNext/><w:jc w:val="right"/></w:pPr><w:r><w:t>onetwo</w:t></w:r></w:p><w:p><w:r><w:t>three</w:t></w:r></w:p>"#,
    );
    let section = imp.document.section_ids()[0];
    let new_id = imp.document.alloc_id().unwrap();
    edit(
        &mut imp,
        vec![RichOp::SplitParagraph {
            parent: Some(section),
            position: 0,
            offset: 3,
            new_id,
        }],
    );
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    assert!(text.contains("<w:keepNext/>"), "{text}");
    let back = import_docx(
        &write_docx(&imp, &DocxWriteOptions::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(all_text(&back.document), ["one", "two", "three"]);
    edit(
        &mut imp,
        vec![RichOp::MergeParagraphs {
            parent: Some(section),
            position: 1,
        }],
    );
    let back = import_docx(
        &write_docx(&imp, &DocxWriteOptions::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(all_text(&back.document), ["one", "twothree"]);
}

#[test]
fn inline_objects_follow_the_text_and_deleted_ones_disappear() {
    let mut imp = doc_with(
        r#"<w:p><w:r><w:t>ab</w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:t>cd</w:t></w:r><w:bookmarkStart w:id="1" w:name="mk"/><w:r><w:t>ef</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
    );
    let p = imp.paragraph_ids()[0];
    edit(
        &mut imp,
        vec![RichOp::InsertText {
            para: p,
            offset: 0,
            text: "XY".into(),
        }],
    );
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} in {text}"))
    };
    assert!(
        at("XYab") < at("proofErr") && at("proofErr") < at("<w:t>cd</w:t>"),
        "{text}"
    );
    assert!(
        at("bookmarkStart") < at("<w:t>ef</w:t>") && at("<w:t>ef</w:t>") < at("bookmarkEnd"),
        "{text}"
    );
    // Deleting the text up to and including the proofing mark's position.
    edit(
        &mut imp,
        vec![RichOp::DeleteText {
            para: p,
            start: 0,
            end: 4,
        }],
    );
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    assert!(
        text.contains("bookmarkStart") && text.contains("<w:t>ef</w:t>"),
        "{text}"
    );
    assert!(!text.contains("<w:t>ab</w:t>"), "{text}");
}

#[test]
fn envelopes_stay_when_text_is_edited_and_go_when_emptied() {
    let mut imp = doc_with(
        r#"<w:p><w:r><w:t>a </w:t></w:r><w:hyperlink r:id="rId9" w:history="1"><w:r><w:t>link</w:t></w:r></w:hyperlink><w:ins w:id="1" w:author="x"><w:r><w:t>new</w:t></w:r></w:ins><w:r><w:t> z</w:t></w:r></w:p>"#,
    );
    let p = imp.paragraph_ids()[0];
    edit(
        &mut imp,
        vec![RichOp::InsertText {
            para: p,
            offset: 6,
            text: "ed".into(),
        }],
    );
    let (bytes, notices) = written_document(&imp);
    let text = xml(&bytes);
    assert!(
        text.contains(r#"<w:hyperlink r:id="rId9" w:history="1">"#),
        "{text}"
    );
    assert!(text.contains("<w:t>linked</w:t>"), "{text}");
    assert!(notices.is_empty(), "{notices:?}");
    // Emptying the hyperlink drops it; the tracked insertion stays.
    edit(
        &mut imp,
        vec![RichOp::DeleteText {
            para: p,
            start: 2,
            end: 8,
        }],
    );
    let (bytes, notices) = written_document(&imp);
    let text = xml(&bytes);
    assert!(!text.contains("hyperlink"), "{text}");
    assert!(text.contains("<w:ins "), "{text}");
    assert!(
        notices
            .iter()
            .any(|n| matches!(n, WriteNotice::EnvelopeDropped { .. })),
        "{notices:?}"
    );
}

#[test]
fn text_edited_in_an_alternate_content_branch_replaces_the_element() {
    let mut imp = doc_with(
        r#"<w:p><w:r><w:t>x</w:t></w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:r><w:t>CH</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>FB</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>"#,
    );
    let p = imp.paragraph_ids()[0];
    assert_eq!(imp.document.paragraph(p).unwrap().text, "xFB");
    // Unmodified: both branches stay.
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    assert!(text.contains("CH") && text.contains("FB"), "{text}");
    // Edit outside the branch: still both.
    edit(
        &mut imp,
        vec![RichOp::InsertText {
            para: p,
            offset: 0,
            text: "y".into(),
        }],
    );
    let (bytes, _) = written_document(&imp);
    let text = xml(&bytes);
    assert!(text.contains("<mc:Choice") && text.contains("CH"), "{text}");
    // Edit inside the branch text: the element is replaced by the edited content.
    edit(
        &mut imp,
        vec![RichOp::InsertText {
            para: p,
            offset: 3,
            text: "!".into(),
        }],
    );
    let (bytes, notices) = written_document(&imp);
    let text = xml(&bytes);
    assert!(
        !text.contains("AlternateContent") && !text.contains("CH"),
        "{text}"
    );
    assert!(
        text.contains("yxF!") && text.contains("<w:t>B</w:t>"),
        "{text}"
    );
    assert!(
        notices.contains(&WriteNotice::AlternateContentFlattened),
        "{notices:?}"
    );
}

#[test]
fn table_rows_added_and_removed_are_written_in_place() {
    let cell = |t: &str| {
        format!(
            r#"<w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#
        )
    };
    let mut imp = doc_with(&format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl><w:p><w:r><w:t>end</w:t></w:r></w:p>"#,
        cell("a"),
        cell("b"),
        cell("c"),
        cell("d")
    ));
    let table = imp
        .document
        .block_ids(Some(imp.document.section_ids()[0]))
        .unwrap()[0];
    let op = imp.document.insert_row_op(table, 1).unwrap();
    edit(&mut imp, vec![op]);
    let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
    let back = import_docx(&w.bytes).unwrap();
    let Some(Node::Table(t)) = back.document.node(
        back.document
            .block_ids(Some(back.document.section_ids()[0]))
            .unwrap()[0],
    ) else {
        panic!("table")
    };
    assert_eq!((t.rows.len(), t.columns), (3, 2));
    let op = imp.document.insert_column_op(table, 2).unwrap();
    edit(&mut imp, vec![op]);
    edit(&mut imp, vec![RichOp::DeleteRow { table, index: 0 }]);
    let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
    let back = import_docx(&w.bytes).unwrap();
    let Some(Node::Table(t)) = back.document.node(
        back.document
            .block_ids(Some(back.document.section_ids()[0]))
            .unwrap()[0],
    ) else {
        panic!("table")
    };
    assert_eq!((t.rows.len(), t.columns), (2, 3));
    assert_eq!(all_text(&back.document), all_text(&imp.document));
    let package = Package::read(&w.bytes).unwrap();
    let main = package
        .parts()
        .values()
        .find(|p| p.uri.as_str().ends_with("document.xml"))
        .unwrap();
    let text = xml(&main.bytes);
    assert_eq!(text.matches("<w:gridCol ").count(), 3, "{text}");
}

#[test]
fn a_changed_style_and_default_tab_stop_are_written() {
    let mut imp = doc_with(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#);
    let mut style = imp.styles.catalog().style("Normal").unwrap().clone();
    style.character.values.bold = Some(false);
    style.character.values.size_half_points = Some(30);
    style.paragraph.alignment = Some(atelier_text_model::rich::Alignment::Center);
    imp.styles.set_style(style).unwrap();
    imp.settings.default_tab_stop_twips = Some(500);
    let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
    assert!(
        w.regenerated_parts
            .iter()
            .any(|p| p.ends_with("styles.xml"))
    );
    assert!(
        w.regenerated_parts
            .iter()
            .any(|p| p.ends_with("settings.xml"))
    );
    let back = import_docx(&w.bytes).unwrap();
    let got = back.styles.catalog().style("Normal").unwrap();
    assert_eq!(got.character.values.size_half_points, Some(30));
    assert_eq!(got.character.values.bold, Some(false));
    assert_eq!(
        got.paragraph.alignment,
        Some(atelier_text_model::rich::Alignment::Center)
    );
    assert_eq!(back.settings.default_tab_stop_twips, Some(500));
    let package = Package::read(&w.bytes).unwrap();
    let styles = package
        .parts()
        .values()
        .find(|p| p.uri.as_str().ends_with("styles.xml"))
        .unwrap();
    let text = xml(&styles.bytes);
    // Unknown children and attributes of the style stay; rPr in schema order.
    assert!(
        text.contains(r#"w14:foo="1""#) && text.contains("<w:noProof/>"),
        "{text}"
    );
    assert!(
        text.contains(r#"<w:rPr><w:noProof/><w:b w:val="0"/><w:sz w:val="30"/></w:rPr>"#),
        "{text}"
    );
    // Untouched parts are reused byte for byte.
    let none = write_docx(&doc_with("<w:p/>"), &DocxWriteOptions::default()).unwrap();
    assert!(
        none.regenerated_parts.is_empty(),
        "{:?}",
        none.regenerated_parts
    );
}

fn default_style(imp: &ImportedDocx) -> Option<String> {
    use atelier_fmt_ooxml::PreservedChild;
    let styles = imp.preserved.styles.as_ref()?;
    styles.root.children.iter().find_map(|c| match c {
        PreservedChild::Element(e)
            if e.name.ends_with("style")
                && e.attrs
                    .iter()
                    .any(|a| a.name.ends_with("default") && a.value == "1") =>
        {
            e.attrs
                .iter()
                .find(|a| a.name.ends_with("styleId"))
                .map(|a| a.value.clone())
        }
        _ => None,
    })
}

/// What the model should read back as: one entry per block in document
/// order, with the empty paragraph the writer puts after a table that ends a
/// body or a cell (Word's rule).
fn shape(doc: &RichDocument) -> Vec<String> {
    fn blocks(doc: &RichDocument, container: NodeId, out: &mut Vec<String>) {
        let ids = doc.block_ids(Some(container)).unwrap_or_default();
        for id in &ids {
            match doc.node(*id) {
                Some(Node::Table(t)) => {
                    out.push(format!("table {}x{}", t.rows.len(), t.columns));
                    for r in &t.rows {
                        if let Some(Node::Row(row)) = doc.node(*r) {
                            for c in &row.cells {
                                if let Some(Node::Cell(cell)) = doc.node(*c) {
                                    out.push(format!("cell {}x{}", cell.col_span, cell.row_span));
                                    blocks(doc, *c, out);
                                }
                            }
                        }
                    }
                }
                Some(Node::Opaque(o)) => out.push(format!("opaque {}", o.kind)),
                Some(Node::Paragraph(p)) => {
                    out.push(format!("p {}", atelier_fmt_ooxml::source_text(p)))
                }
                _ => {}
            }
        }
        if !matches!(
            ids.last().and_then(|b| doc.node(*b)),
            Some(Node::Paragraph(_))
        ) {
            out.push("p ".into());
        }
    }
    let mut out = Vec::new();
    for s in doc.section_ids() {
        blocks(doc, s, &mut out);
    }
    out
}

/// The paragraph texts of [`shape`].
fn texts(doc: &RichDocument) -> Vec<String> {
    shape(doc)
        .into_iter()
        .filter_map(|s| s.strip_prefix("p ").map(str::to_owned))
        .collect()
}

/// Random edits, then write and read back: no panic, a readable file, and
/// a model equal to the edited one (text, structure, properties).
#[test]
fn seeded_edit_campaign_writes_files_that_read_back_equal() {
    let mut docs = corpus();
    docs.extend(built_documents());
    let ordered = [
        "basic.docx",
        "libreoffice/rich.docx",
        "libreoffice/properties.docx",
        "text",
    ];
    let mut files = 0;
    let mut problems = Vec::new();
    // Formatting differences on edited paragraphs (documented limits).
    let mut soft: Vec<String> = Vec::new();
    for (name, bytes) in &docs {
        for seed in 0..40u64 {
            let mut imp = import_docx(bytes).unwrap();
            let mut rng = Rng(0xFEED_0000 + seed * 7919 + name.len() as u64);
            for _ in 0..(1 + seed % 12) {
                mutate(&mut imp.document, &mut rng);
            }
            imp.document.validate().unwrap();
            let w = match write_docx(&imp, &DocxWriteOptions::default()) {
                Ok(w) => w,
                Err(e) => {
                    problems.push(format!("{name}/{seed}: write {e:?}"));
                    continue;
                }
            };
            files += 1;
            let again = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
            if again.bytes != w.bytes {
                problems.push(format!("{name}/{seed}: not deterministic"));
            }
            let back = match import_docx(&w.bytes) {
                Ok(b) => b,
                Err(e) => {
                    problems.push(format!("{name}/{seed}: unreadable: {e:?}"));
                    continue;
                }
            };
            let flattened = w.notices.contains(&WriteNotice::AlternateContentFlattened);
            let (want, got) = (texts(&imp.document), all_text(&back.document));
            if got != want {
                let at = want
                    .iter()
                    .zip(&got)
                    .position(|(a, b)| a != b)
                    .unwrap_or(want.len().min(got.len()));
                problems.push(format!(
                    "{name}/{seed}: text differs at paragraph {at} ({} vs {} paragraphs): {:?} vs {:?}",
                    want.len(),
                    got.len(),
                    want.get(at),
                    got.get(at)
                ));
            }
            let mut expected_shape = shape(&imp.document);
            if flattened {
                // The copy of a flattened mc:AlternateContent is gone.
                expected_shape.retain(|s| s != "opaque mc:AlternateContent");
            }
            let mut got_shape = shape(&back.document);
            if flattened {
                got_shape.retain(|s| s != "opaque mc:AlternateContent");
            }
            if got_shape != expected_shape {
                let (a, b) = (expected_shape, got_shape);
                let at = a
                    .iter()
                    .zip(&b)
                    .position(|(x, y)| x != y)
                    .unwrap_or(a.len().min(b.len()));
                problems.push(format!(
                    "{name}/{seed}: structure differs at {at}: {:?} vs {:?}",
                    a.get(at),
                    b.get(at)
                ));
            }
            if ordered.contains(&name.as_str()) {
                let package = Package::read(&w.bytes).unwrap();
                for (uri, part) in package.parts() {
                    if part.content_type.ends_with("main+xml") {
                        for p in order_problems(&parse(&part.bytes)) {
                            problems.push(format!("{name}/{seed} {uri}: {p}"));
                        }
                    }
                }
            }
            // Runs and properties, paragraph by paragraph (identifiers are
            // minted again by the import, so the order is the key).
            let default = default_style(&back);
            for (x, y) in imp
                .document
                .paragraph_ids()
                .iter()
                .zip(back.document.paragraph_ids())
            {
                let (p, q) = (
                    imp.document.paragraph(*x).unwrap(),
                    back.document.paragraph(y).unwrap(),
                );
                let norm = |x: &ParaProps| {
                    let mut x = x.clone();
                    x.list = None;
                    if x.style.is_none() {
                        x.style = default.clone();
                    }
                    x
                };
                // An explicit color or highlight wins over `auto` / `none`, which
                // the model keeps beside it as an opaque attribute.
                let clean = |runs: &[atelier_text_model::rich::CharRun]| {
                    runs.iter()
                        .map(|r| {
                            let mut r = r.clone();
                            if r.attrs.color.is_some() {
                                r.attrs.opaque.retain(|o| o.name != "color");
                            }
                            if r.attrs.highlight.is_some() {
                                r.attrs.opaque.retain(|o| o.name != "highlight");
                            }
                            r
                        })
                        .collect::<Vec<_>>()
                };
                if clean(&p.runs) != clean(&q.runs) && p.anchored.is_empty() {
                    soft.push(format!("{name}/{seed}: runs"));
                }
                if norm(&p.props) != norm(&q.props) {
                    soft.push(format!("{name}/{seed}: props"));
                }
            }
        }
    }
    println!(
        "{files} edited files written, {} soft formatting differences: {soft:?}",
        soft.len()
    );
    assert!(soft.len() <= 5, "{soft:?}");
    assert!(
        problems.is_empty(),
        "{} problems: {:#?}",
        problems.len(),
        &problems[..problems.len().min(25)]
    );
}

const CT_WML: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

fn regenerate_all() -> DocxWriteOptions {
    DocxWriteOptions {
        regenerate_all: true,
        ..DocxWriteOptions::default()
    }
}

#[test]
fn a_list_reference_without_definition_in_the_copied_numbering_is_reported() {
    use atelier_text_model::rich::ListRef;
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"/><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let mut d = doc(body(
        "<w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p>",
    ));
    d.extra.push((
        "word/numbering.xml",
        Box::leak(format!("{CT_WML}.numbering+xml").into_boxed_str()),
        numbering.into_bytes(),
        "numbering",
    ));
    let mut imp = d.import();
    let ids = imp.document.paragraph_ids();
    let list = |list_id| ParaProps {
        list: Some(ListRef { list_id, level: 0 }),
        ..ParaProps::default()
    };
    edit(
        &mut imp,
        vec![
            RichOp::SetParaProps {
                para: ids[0],
                set: list(0),
                clear: ParaProps::default(),
            },
            RichOp::SetParaProps {
                para: ids[1],
                set: list(7),
                clear: ParaProps::default(),
            },
        ],
    );
    let w = write_docx(&imp, &regenerate_all()).unwrap();
    let missing: Vec<_> = w
        .notices
        .iter()
        .filter_map(|n| match n {
            WriteNotice::ListDefinitionMissing { list_id } => Some(*list_id),
            _ => None,
        })
        .collect();
    assert_eq!(missing, [7], "{:?}", w.notices);
}

#[test]
fn notes_and_comments_copied_keep_unreferenced_entries_after_a_deletion() {
    let notes = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:footnotes xmlns:w="{W}"><w:footnote w:id="2"><w:p><w:r><w:t>note</w:t></w:r></w:p></w:footnote></w:footnotes>"#
    );
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:comments xmlns:w="{W}"><w:comment w:id="5" w:author="a"><w:p><w:r><w:t>remark</w:t></w:r></w:p></w:comment></w:comments>"#
    );
    let mut d = doc(body(
        r#"<w:p><w:r><w:t>one</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r><w:commentRangeStart w:id="5"/><w:r><w:t>x</w:t></w:r><w:commentRangeEnd w:id="5"/><w:r><w:commentReference w:id="5"/></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>"#,
    ));
    d.extra.push((
        "word/footnotes.xml",
        Box::leak(format!("{CT_WML}.footnotes+xml").into_boxed_str()),
        notes.into_bytes(),
        "footnotes",
    ));
    d.extra.push((
        "word/comments.xml",
        Box::leak(format!("{CT_WML}.comments+xml").into_boxed_str()),
        comments.into_bytes(),
        "comments",
    ));
    let mut imp = d.import();
    let section = imp.document.section_ids()[0];
    edit(
        &mut imp,
        vec![RichOp::RemoveBlock {
            parent: Some(section),
            position: 0,
        }],
    );
    let w = write_docx(&imp, &regenerate_all()).unwrap();
    let out = Package::read(&w.bytes).expect("the package reads back (XML parts validated)");
    let text = |name: &str| {
        let part = out
            .parts()
            .values()
            .find(|p| p.uri.as_str() == name)
            .unwrap();
        String::from_utf8(part.bytes.clone()).unwrap()
    };
    let document = text("word/document.xml");
    assert!(!document.contains("footnoteReference") && !document.contains("commentReference"));
    // Documented limit: the copied parts keep their entries, now unreferenced.
    assert!(text("word/footnotes.xml").contains(r#"w:id="2""#));
    assert!(text("word/comments.xml").contains("remark"));
    assert!(w.opaque_parts.iter().any(|p| p == "word/footnotes.xml"));
    assert!(import_docx(&w.bytes).is_ok());
}
