// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-307: documents created from scratch.
mod support;

use atelier_fmt_ooxml::{DocxWriteOptions, NewDocument, Package, import_docx, write_new_docx};
use atelier_text_model::rich::{Node, NodeId, RichDocument};
use support::order::order_problems;
use support::scratch::*;
use support::*;

pub fn part_order_problems(bytes: &[u8]) -> Vec<String> {
    let package = Package::read(bytes).unwrap();
    let mut out = Vec::new();
    for (uri, part) in package.parts() {
        if part.content_type.ends_with("+xml") && uri.as_str().contains("word/") {
            for p in order_problems(&parse(&part.bytes)) {
                out.push(format!("{uri}: {p}"));
            }
        }
    }
    out
}

/// Structure of the tables: rows and columns, then per cell spans and text.
fn table_shape(doc: &RichDocument) -> Vec<String> {
    fn walk(doc: &RichDocument, container: NodeId, out: &mut Vec<String>) {
        for id in doc.block_ids(Some(container)).unwrap_or_default() {
            if let Some(Node::Table(t)) = doc.node(id) {
                out.push(format!("table {}x{}", t.rows.len(), t.columns));
                for r in &t.rows {
                    if let Some(Node::Row(row)) = doc.node(*r) {
                        for c in &row.cells {
                            if let Some(Node::Cell(cell)) = doc.node(*c) {
                                let text: Vec<_> = cell
                                    .blocks
                                    .to_vec()
                                    .iter()
                                    .filter_map(|b| {
                                        doc.paragraph(*b).ok().map(atelier_fmt_ooxml::source_text)
                                    })
                                    .collect();
                                out.push(format!(
                                    "cell {}x{} {:?}",
                                    cell.col_span, cell.row_span, text
                                ));
                                walk(doc, *c, out);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for s in doc.section_ids() {
        walk(doc, s, &mut out);
    }
    out
}

#[test]
fn systematic_new_documents_write_and_read_back_equal() {
    let mut problems = Vec::new();
    let count = 120;
    for i in 0..count {
        let built = scratch_document(i);
        let input = NewDocument::from_default(&built.doc);
        let a = write_new_docx(&input, &DocxWriteOptions::default()).unwrap();
        let b = write_new_docx(&input, &DocxWriteOptions::default()).unwrap();
        if a.bytes != b.bytes {
            problems.push(format!("{}: not deterministic", built.name));
        }
        for p in part_order_problems(&a.bytes) {
            problems.push(format!("{}: {p}", built.name));
        }
        let back = match import_docx(&a.bytes) {
            Ok(b) => b,
            Err(e) => {
                problems.push(format!("{}: reimport {e:?}", built.name));
                continue;
            }
        };
        let (want, got) = (all_text(&built.doc.document), all_text(&back.document));
        if want != got {
            problems.push(format!("{}: text {want:?} vs {got:?}", built.name));
        }
        if table_shape(&built.doc.document) != table_shape(&back.document) {
            problems.push(format!(
                "{}: tables {:?} vs {:?}",
                built.name,
                table_shape(&built.doc.document),
                table_shape(&back.document)
            ));
        }
        for (x, y) in built
            .doc
            .document
            .paragraph_ids()
            .iter()
            .zip(back.document.paragraph_ids())
        {
            let (p, q) = (
                built.doc.document.paragraph(*x).unwrap(),
                back.document.paragraph(y).unwrap(),
            );
            if p.runs != q.runs {
                problems.push(format!("{}: runs {:?} vs {:?}", built.name, p.runs, q.runs));
            }
            let (mut pp, mut qp) = (p.props.clone(), q.props.clone());
            for props in [&mut pp, &mut qp] {
                if props.style.as_deref() == Some("atelier.normal") {
                    props.style = None;
                }
                // The import does not read numbering (US-302).
                props.list = None;
            }
            if pp != qp {
                problems.push(format!("{}: props {pp:?} vs {qp:?}", built.name));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{:#?}",
        &problems[..problems.len().min(20)]
    );
}

fn regenerate_all() -> DocxWriteOptions {
    DocxWriteOptions {
        regenerate_all: true,
        ..DocxWriteOptions::default()
    }
}

/// US-307 criterion 1: documents built from scratch, each twice and independently,
/// give the same bytes; once opened and saved again (`regenerate_all`) the output
/// reaches a fixed point after one more cycle.
#[test]
fn new_documents_are_deterministic_and_reach_a_fixed_point() {
    use atelier_fmt_ooxml::write_docx;
    let mut problems = Vec::new();
    for i in 0..120 {
        let (one, two) = (scratch_document(i), scratch_document(i));
        let a = write_new_docx(
            &NewDocument::from_default(&one.doc),
            &DocxWriteOptions::default(),
        )
        .unwrap();
        let b = write_new_docx(
            &NewDocument::from_default(&two.doc),
            &DocxWriteOptions::default(),
        )
        .unwrap();
        if a.bytes != b.bytes {
            problems.push(format!("{}: two builds differ", one.name));
        }
        let cycle = |bytes: &[u8]| {
            let imp = import_docx(bytes).ok()?;
            let w = write_docx(&imp, &regenerate_all()).ok()?;
            let again = write_docx(&imp, &regenerate_all()).ok()?;
            (w.bytes == again.bytes && w.reused_parts.is_empty()).then_some(w.bytes)
        };
        match cycle(&a.bytes).and_then(|c| cycle(&c).map(|d| (c, d))) {
            Some((c, d)) => match cycle(&d) {
                Some(e) if e == d && c == d => {}
                _ => problems.push(format!("{}: no fixed point", one.name)),
            },
            None => problems.push(format!("{}: cycle fails", one.name)),
        }
    }
    assert!(
        problems.is_empty(),
        "{:#?}",
        &problems[..problems.len().min(20)]
    );
}
