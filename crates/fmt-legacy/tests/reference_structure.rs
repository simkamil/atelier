// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-313 reference files of `tests/doc-reference` (`struct-*.doc`, produced by
//! LibreOffice through `tools/generate-doc-reference.py`). The expected values
//! (`structure.*.expect.model` in `expected.json`) come from the description in
//! the generator and were checked there against its independent Python reader;
//! nothing here is derived from the Rust reader.

#[path = "common/json.rs"]
mod json;

use atelier_fmt_legacy::doc::{
    CellVerticalAlign, HeightRule, HfState, ImportedDoc, ListLevelKind, TableAlign, TableColor,
    import_doc,
};
use atelier_text_model::rich::{HfKind, HfVariant, LinkTarget, Node, NodeId};
use json::{Json, Parser};

fn manifest() -> Json {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/doc-reference/expected.json"
    ))
    .unwrap();
    Parser {
        s: text.as_bytes(),
        i: 0,
    }
    .value()
}

fn load(name: &str) -> (ImportedDoc, Json) {
    let m = manifest();
    let entry = m
        .get("structure")
        .and_then(|s| s.get(&format!("{name}.doc")))
        .expect("manifest entry")
        .clone();
    let bytes = std::fs::read(format!(
        "{}/../../tests/doc-reference/{name}.doc",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let doc = import_doc(&bytes).unwrap();
    doc.document.validate().unwrap();
    let model = entry
        .get("expect")
        .and_then(|e| e.get("model"))
        .expect("model expectations")
        .clone();
    (doc, model)
}

fn text(doc: &ImportedDoc, id: NodeId) -> String {
    doc.document.paragraph(id).unwrap().text.clone()
}

fn first_paragraph(doc: &ImportedDoc, container: NodeId) -> NodeId {
    doc.document
        .block_ids(Some(container))
        .unwrap()
        .into_iter()
        .find(|id| matches!(doc.document.node(*id), Some(Node::Paragraph(_))))
        .expect("a paragraph")
}

fn close(a: i64, b: i64) -> bool {
    (a - b).abs() <= 3
}

fn hex(c: &TableColor) -> String {
    match c {
        TableColor::Rgb(c) => format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b),
        TableColor::Auto => "auto".into(),
    }
}

#[test]
fn tables() {
    let (doc, model) = load("struct-tables");
    let body: Vec<String> = doc.paragraph_ids().iter().map(|i| text(&doc, *i)).collect();
    let want: Vec<String> = model
        .get("body_texts")
        .unwrap()
        .arr()
        .iter()
        .map(|t| t.str().to_owned())
        .collect();
    assert_eq!(body, want);
    assert!(
        !doc.notices.iter().any(|n| matches!(
            n,
            atelier_fmt_legacy::doc::ImportNotice::TableCellsFlattened { .. }
        )),
        "{:?}",
        doc.notices
    );
    let expected = model.get("tables").unwrap().arr();
    assert_eq!(doc.tables.tables.len(), expected.len());
    for exp in expected {
        let cells = exp.get("cells").unwrap().arr();
        let first = cells[0].arr()[4].str();
        // The table whose first cell (by grid position) holds that text.
        let (tid, table) = doc
            .tables
            .tables
            .iter()
            .find(|(tid, _)| {
                doc.tables.cells.iter().any(|(cid, c)| {
                    c.table == Some(**tid)
                        && c.row_index == 0
                        && c.column_index == 0
                        && text(&doc, first_paragraph(&doc, *cid)) == first
                })
            })
            .unwrap_or_else(|| panic!("no table starting with {first}"));
        assert_eq!(table.depth as i64, exp.get("depth").unwrap().num());
        assert_eq!(table.columns as i64, exp.get("columns").unwrap().num());
        let grid: Vec<i64> = exp
            .get("grid")
            .unwrap()
            .arr()
            .iter()
            .map(Json::num)
            .collect();
        assert_eq!(table.grid.len(), grid.len());
        for (g, w) in table.grid.iter().zip(&grid) {
            assert!(
                close(i64::from(g.unwrap()), *w),
                "{:?} vs {grid:?}",
                table.grid
            );
        }
        let mut got: Vec<(i64, i64, i64, i64, String)> = doc
            .tables
            .cells
            .iter()
            .filter(|(_, c)| c.table == Some(*tid))
            .map(|(cid, c)| {
                (
                    c.row_index as i64,
                    c.column_index as i64,
                    c.row_span as i64,
                    c.col_span as i64,
                    text(&doc, first_paragraph(&doc, *cid)),
                )
            })
            .collect();
        got.sort();
        let mut want: Vec<(i64, i64, i64, i64, String)> = cells
            .iter()
            .map(|c| {
                let a = c.arr();
                (
                    a[0].num(),
                    a[1].num(),
                    a[2].num(),
                    a[3].num(),
                    a[4].str().to_owned(),
                )
            })
            .collect();
        want.sort();
        assert_eq!(got, want);
        // The model cells agree with the companion structure.
        for (cid, c) in doc
            .tables
            .cells
            .iter()
            .filter(|(_, c)| c.table == Some(*tid))
        {
            let Some(Node::Cell(n)) = doc.document.node(*cid) else {
                panic!("cell node")
            };
            assert_eq!((n.row_span, n.col_span), (c.row_span, c.col_span));
        }
        if exp.get("align").is_some_and(|a| a.str() == "center") {
            assert_eq!(table.props.align, Some(TableAlign::Center));
        }
        if let Some(rows) = exp.get("rows") {
            for (i, r) in rows.arr().iter().enumerate() {
                let (_, row) = doc
                    .tables
                    .rows
                    .iter()
                    .find(|(_, r)| r.table == Some(*tid) && r.index == i)
                    .unwrap();
                match r.get("header").unwrap() {
                    Json::Bool(b) => assert_eq!(row.props.header, Some(*b)),
                    _ => assert_ne!(row.props.header, Some(true)),
                }
                let h = r.get("height").unwrap().arr();
                let rule = match h[1].str() {
                    "exact" => HeightRule::Exact,
                    _ => HeightRule::AtLeast,
                };
                let got = row.props.height.unwrap();
                assert!(close(i64::from(got.twips), h[0].num()) && got.rule == rule);
                assert_eq!(
                    row.props.cant_split,
                    Some(matches!(r.get("cant_split").unwrap(), Json::Bool(true)))
                );
            }
        }
        if let Some(Json::Obj(props)) = exp.get("cell_props") {
            for (label, p) in props {
                let (_, cell) = doc
                    .tables
                    .cells
                    .iter()
                    .find(|(cid, c)| {
                        c.table == Some(*tid) && text(&doc, first_paragraph(&doc, **cid)) == *label
                    })
                    .unwrap();
                if let Some(v) = p.get("valign") {
                    let want = match v.str() {
                        "center" => CellVerticalAlign::Center,
                        _ => CellVerticalAlign::Bottom,
                    };
                    assert_eq!(cell.props.vertical_align, Some(want), "{label}");
                }
                if let Some(f) = p.get("fill") {
                    let s = cell.props.shading.as_ref().unwrap();
                    assert_eq!(s.pattern, "clear");
                    assert_eq!(hex(s.fill.as_ref().unwrap()), f.str());
                }
                for (side, b) in [
                    ("top", &cell.props.borders.top),
                    ("left", &cell.props.borders.left),
                    ("bottom", &cell.props.borders.bottom),
                ] {
                    if let Some(e) = p.get(side) {
                        let b = b.as_ref().unwrap();
                        assert_eq!(b.style, e.arr()[0].str(), "{label} {side}");
                        assert_eq!(hex(b.color.as_ref().unwrap()), e.arr()[1].str());
                    }
                }
            }
        }
    }
}

fn level_kind(k: &ListLevelKind) -> Vec<String> {
    match k {
        ListLevelKind::Bullet(s) => vec!["bullet".into(), s.clone()],
        ListLevelKind::Number {
            format,
            text,
            start,
        } => vec![format!("{format:?}"), text.clone(), start.to_string()],
        ListLevelKind::None => vec!["none".into()],
    }
}

#[test]
fn lists() {
    let (doc, model) = load("struct-lists");
    let mut order: Vec<u32> = Vec::new();
    let mut got = Vec::new();
    for item in &doc.list_items {
        if !order.contains(&item.list_id) {
            order.push(item.list_id);
        }
        let g = order.iter().position(|x| *x == item.list_id).unwrap();
        got.push((text(&doc, item.paragraph), g as i64, i64::from(item.level)));
        let r = doc
            .document
            .paragraph(item.paragraph)
            .unwrap()
            .props
            .list
            .unwrap();
        assert_eq!((r.list_id, r.level), (item.list_id, item.level));
    }
    let want: Vec<(String, i64, i64)> = model
        .get("paragraphs")
        .unwrap()
        .arr()
        .iter()
        .map(|p| {
            let a = p.arr();
            (a[0].str().to_owned(), a[1].num(), a[2].num())
        })
        .collect();
    assert_eq!(got, want);
    for (g, levels) in model.get("groups").unwrap().arr().iter().enumerate() {
        let list = &doc.lists[order[g] as usize - 1];
        for (i, l) in levels.arr().iter().enumerate() {
            let want: Vec<String> = l
                .arr()
                .iter()
                .map(|v| match v {
                    Json::Str(s) => s.clone(),
                    Json::Num(n) => (*n as i64).to_string(),
                    _ => unreachable!(),
                })
                .collect();
            assert_eq!(
                level_kind(&list.levels[i].kind),
                want,
                "group {g} level {i}"
            );
        }
    }
    // Restarted numbering is a different list instance with its own start.
    assert_ne!(order[1], order[3]);
}

#[test]
fn sections() {
    let (doc, model) = load("struct-sections");
    let want = model.get("sections").unwrap().arr();
    assert_eq!(doc.sections.len(), want.len());
    assert_eq!(doc.document.section_ids().len(), want.len());
    for (s, w) in doc.sections.iter().zip(want) {
        assert_eq!(s.section, doc.document.section_ids()[s.index]);
        let label = w.get("text").unwrap().str();
        assert_eq!(text(&doc, first_paragraph(&doc, s.section)), label);
        assert_eq!(
            format!("{:?}", s.break_type),
            w.get("break").unwrap().str(),
            "{label}"
        );
        assert_eq!(
            i64::from(s.columns.count),
            w.get("columns").unwrap().num(),
            "{label}"
        );
        if let Some(v) = w.get("width") {
            assert!(close(i64::from(s.page.width.unwrap()), v.num()), "{label}");
            assert!(close(
                i64::from(s.page.height.unwrap()),
                w.get("height").unwrap().num()
            ));
            assert_eq!(
                s.page.landscape,
                matches!(w.get("landscape").unwrap(), Json::Bool(true))
            );
            let m = w.get("margins").unwrap().arr();
            let got = [
                s.page.margin_left,
                s.page.margin_right,
                s.page.margin_top,
                s.page.margin_bottom,
            ];
            for (g, e) in got.iter().zip(m) {
                assert!(close(i64::from(g.unwrap()), e.num()), "{label} {got:?}");
            }
        }
        if let Some(v) = w.get("column_spacing") {
            assert!(close(i64::from(s.columns.spacing.unwrap()), v.num()));
        }
        if let Some(f) = w.get("format") {
            assert_eq!(
                format!("{:?}", s.page_numbers.format.unwrap()),
                f.str(),
                "{label}"
            );
        }
        assert_eq!(
            s.page_numbers.restart,
            matches!(w.get("restart"), Some(Json::Bool(true))),
            "{label}"
        );
        if let Some(v) = w.get("start") {
            assert_eq!(s.page_numbers.start, Some(v.num() as u32));
        }
    }
}

fn render(doc: &ImportedDoc, hf: NodeId) -> String {
    let ids = doc
        .header_paragraphs
        .iter()
        .find(|(h, _)| *h == hf)
        .unwrap();
    ids.1
        .iter()
        .map(|id| {
            let p = doc.document.paragraph(*id).unwrap();
            let mut out = String::new();
            let mut at = 0;
            for f in &p.fields {
                out.push_str(&p.text[at..f.start]);
                out.push_str(&format!("{{{}}}", f.instruction));
                at = f.end;
            }
            out.push_str(&p.text[at..]);
            out
        })
        .collect::<Vec<_>>()
        .join("\r")
}

#[test]
fn headers_and_footers() {
    let (doc, model) = load("struct-headers");
    let want = model.get("sections").unwrap().arr();
    assert_eq!(doc.sections.len(), want.len());
    for (s, w) in doc.sections.iter().zip(want) {
        assert_eq!(
            s.title_page,
            matches!(w.get("title_page").unwrap(), Json::Bool(true))
        );
        let mut got = Vec::new();
        for h in &s.headers_footers {
            if let HfState::Present(id) = h.state {
                got.push(vec![
                    format!("{:?}", h.kind),
                    format!("{:?}", h.variant),
                    render(&doc, id),
                ]);
            }
        }
        let want: Vec<Vec<String>> = w
            .get("slots")
            .unwrap()
            .arr()
            .iter()
            .map(|a| a.arr().iter().map(|x| x.str().to_owned()).collect())
            .collect();
        let mut got_sorted = got.clone();
        got_sorted.sort();
        let mut want_sorted = want.clone();
        want_sorted.sort();
        assert_eq!(got_sorted, want_sorted);
        // The model holds the same headers and footers.
        let Some(Node::Section(n)) = doc.document.node(s.section) else {
            panic!("section node")
        };
        assert_eq!(n.headers.len() + n.footers.len(), want.len());
        assert!(
            doc.effective_header_footer(s.index, HfKind::Header, HfVariant::Default)
                .is_some()
        );
    }
}

#[test]
fn fields_links_and_bookmarks() {
    let (doc, model) = load("struct-fields");
    let mut bookmarks = Vec::new();
    let mut links = Vec::new();
    let mut fields = Vec::new();
    for id in doc.paragraph_ids() {
        let p = doc.document.paragraph(*id).unwrap();
        for b in &p.bookmarks {
            let imported = doc.bookmarks.iter().find(|x| x.bookmark == b.id).unwrap();
            bookmarks.push((
                b.name.clone(),
                p.text.clone(),
                b.start,
                b.end,
                imported.truncated,
            ));
        }
        for l in &p.links {
            let (kind, value) = match &l.target {
                LinkTarget::External(u) => ("External", u.clone()),
                LinkTarget::Bookmark(n) => ("Bookmark", n.clone()),
            };
            links.push((
                p.text[l.start..l.end].to_owned(),
                kind,
                value,
                l.tooltip.clone(),
            ));
        }
        for f in &p.fields {
            fields.push(f.instruction.clone());
        }
    }
    let want_b: Vec<_> = model
        .get("bookmarks")
        .unwrap()
        .arr()
        .iter()
        .map(|b| {
            let a = b.arr();
            (
                a[0].str().to_owned(),
                a[1].str().to_owned(),
                a[2].num() as usize,
                a[3].num() as usize,
                matches!(a[4], Json::Bool(true)),
            )
        })
        .collect();
    assert_eq!(bookmarks, want_b);
    let want_l: Vec<_> = model
        .get("links")
        .unwrap()
        .arr()
        .iter()
        .map(|l| {
            let a = l.arr();
            (
                a[0].str().to_owned(),
                match a[1].str() {
                    "External" => "External",
                    _ => "Bookmark",
                },
                a[2].str().to_owned(),
                match &a[3] {
                    Json::Str(s) => Some(s.clone()),
                    _ => None,
                },
            )
        })
        .collect();
    assert_eq!(links, want_l);
    let want_f: Vec<String> = model
        .get("fields")
        .unwrap()
        .arr()
        .iter()
        .map(|f| f.str().to_owned())
        .collect();
    assert_eq!(fields, want_f);
}
