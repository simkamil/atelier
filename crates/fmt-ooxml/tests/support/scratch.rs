// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rich documents built and edited through model operations, for the writer
//! tests (US-307): systematic variations, a seeded random editor.
#![allow(dead_code)]

use atelier_model::Transaction;
use atelier_text_model::rich::{
    Alignment, Bookmark, CharAttrs, Color, DefaultRichDocument, Field, Inline, LineRule,
    LineSpacing, Link, LinkTarget, ListRef, Node, NodeId, ParaProps, RichDocument, RichOp,
    Underline, VerticalAlign, default_rich_document,
};

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

pub const SAMPLES: [&str; 8] = [
    "Bonjour le monde",
    "Éléphant à l'été — œuvre « test »",
    "日本語のテキスト",
    "tabs\tand\nbreak",
    "  leading and  double  spaces  ",
    "emoji 😀 and combining e\u{301}",
    "Ligne & <balises> \"quotes\" 'apos'",
    "Ünïcödé ñ ç ß ø",
];

pub fn apply(doc: &mut RichDocument, ops: Vec<RichOp>) -> bool {
    match Transaction::new(ops) {
        Ok(tx) => doc.apply(&tx).is_ok(),
        Err(_) => false,
    }
}

pub fn last_section(doc: &RichDocument) -> NodeId {
    *doc.section_ids().last().unwrap()
}

/// Appends a paragraph with attributed segments `(text, attrs)` to a section.
pub fn push_paragraph(
    doc: &mut RichDocument,
    section: NodeId,
    segments: &[(String, CharAttrs)],
    props: ParaProps,
) -> NodeId {
    let text: String = segments.iter().map(|(t, _)| t.as_str()).collect();
    let sub = doc.new_paragraph(&text).unwrap();
    let id = sub.root;
    let position = doc.block_ids(Some(section)).unwrap().len();
    let mut ops = vec![RichOp::InsertBlock {
        parent: Some(section),
        position,
        subtree: sub,
    }];
    if props != ParaProps::default() {
        ops.push(RichOp::SetParaProps {
            para: id,
            set: props,
            clear: ParaProps::default(),
        });
    }
    let mut at = 0;
    for (t, a) in segments {
        if !a.is_empty() && !t.is_empty() {
            ops.push(RichOp::SetCharAttrs {
                para: id,
                start: at,
                end: at + t.len(),
                set: a.clone(),
                clear: CharAttrs::default(),
            });
        }
        at += t.len();
    }
    assert!(apply(doc, ops), "push_paragraph");
    id
}

pub fn random_attrs(rng: &mut Rng) -> CharAttrs {
    let mut a = CharAttrs::default();
    if rng.chance(40) {
        a.bold = Some(rng.chance(80));
    }
    if rng.chance(40) {
        a.italic = Some(rng.chance(80));
    }
    if rng.chance(30) {
        a.underline = Some(
            [
                Underline::None,
                Underline::Single,
                Underline::Double,
                Underline::Thick,
                Underline::Dotted,
                Underline::Dashed,
                Underline::Wave,
            ][rng.below(7)],
        );
    }
    if rng.chance(20) {
        a.strike = Some(true);
    }
    if rng.chance(15) {
        a.vertical_align = Some([VerticalAlign::Superscript, VerticalAlign::Subscript][rng.below(2)]);
    }
    if rng.chance(30) {
        a.font = Some(["Carlito", "Liberation Serif", "Courier New", "Arial"][rng.below(4)].into());
    }
    if rng.chance(30) {
        a.size_half_points = Some(16 + 2 * rng.below(20) as u32);
    }
    if rng.chance(30) {
        a.color = Some(Color {
            r: rng.below(256) as u8,
            g: rng.below(256) as u8,
            b: rng.below(256) as u8,
        });
    }
    if rng.chance(15) {
        // One of the sixteen Word highlight colors.
        a.highlight = Some(
            [
                Color { r: 255, g: 255, b: 0 },
                Color { r: 0, g: 255, b: 255 },
                Color { r: 0, g: 255, b: 0 },
                Color { r: 255, g: 0, b: 255 },
            ][rng.below(4)],
        );
    }
    a
}

pub fn random_props(rng: &mut Rng) -> ParaProps {
    let mut p = ParaProps::default();
    if rng.chance(50) {
        p.alignment = Some([Alignment::Left, Alignment::Center, Alignment::Right, Alignment::Justify][rng.below(4)]);
    }
    if rng.chance(30) {
        p.indent_left = Some(rng.below(1500) as i32);
    }
    if rng.chance(20) {
        p.indent_right = Some(rng.below(800) as i32);
    }
    if rng.chance(20) {
        p.indent_first_line = Some(rng.below(1200) as i32 - 600);
    }
    if rng.chance(30) {
        p.space_before = Some(rng.below(400) as u32);
    }
    if rng.chance(30) {
        p.space_after = Some(rng.below(400) as u32);
    }
    if rng.chance(25) {
        p.line_spacing = Some(LineSpacing {
            rule: [LineRule::Auto, LineRule::Exact, LineRule::AtLeast][rng.below(3)],
            value: 240 + rng.below(240) as u32,
        });
    }
    p
}

/// Text of every paragraph in document order.
pub fn all_text(doc: &RichDocument) -> Vec<String> {
    doc.paragraph_ids()
        .iter()
        .map(|id| doc.paragraph(*id).unwrap().text.clone())
        .collect()
}

/// Words of the text, as the checks of the LibreOffice script compare them.
pub fn words(texts: &[String]) -> Vec<String> {
    texts
        .iter()
        .flat_map(|t| {
            t.replace('\u{ad}', "")
                .replace('\u{2011}', "-")
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

pub struct Built {
    pub name: String,
    pub doc: DefaultRichDocument,
}

fn cell_paragraph(doc: &RichDocument, table: NodeId, row: usize, col: usize) -> Option<NodeId> {
    let Some(Node::Table(t)) = doc.node(table) else {
        return None;
    };
    let Some(Node::Row(r)) = doc.node(*t.rows.get(row)?) else {
        return None;
    };
    let Some(Node::Cell(c)) = doc.node(*r.cells.get(col)?) else {
        return None;
    };
    c.blocks.get(0)
}

/// Document number `i` of the systematic set: every feature depends on a
/// different residue of `i`, so the set covers their combinations.
pub fn scratch_document(i: usize) -> Built {
    let mut d = default_rich_document();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((i as u64 + 1) * 0x1000_0000_01B3));
    let section = last_section(&d.document);
    // Replace the empty first paragraph by a title.
    let first = d.document.paragraph_ids()[0];
    let title = SAMPLES[i % SAMPLES.len()];
    assert!(apply(
        &mut d.document,
        vec![
            RichOp::InsertText {
                para: first,
                offset: 0,
                text: title.into(),
            },
            RichOp::SetParaProps {
                para: first,
                set: ParaProps {
                    style: Some("atelier.title".into()),
                    ..ParaProps::default()
                },
                clear: ParaProps::default(),
            },
        ]
    ));
    let heading = format!("atelier.heading.{}", 1 + i % 3);
    push_paragraph(
        &mut d.document,
        section,
        &[(SAMPLES[(i / 2) % SAMPLES.len()].to_owned(), CharAttrs::default())],
        ParaProps {
            style: Some(heading),
            ..ParaProps::default()
        },
    );
    // A body paragraph with formatted segments.
    let count = 1 + i % 4;
    let segments: Vec<(String, CharAttrs)> = (0..count)
        .map(|k| {
            (
                format!("{} ", SAMPLES[(i + k * 3) % SAMPLES.len()]),
                random_attrs(&mut rng),
            )
        })
        .collect();
    let mut props = random_props(&mut rng);
    if i % 9 == 4 {
        props.list = Some(ListRef {
            list_id: (i % 3) as u32,
            level: (i % 3) as u8,
        });
    }
    let body = push_paragraph(&mut d.document, section, &segments, props);
    let body_len = d.document.paragraph(body).unwrap().text.len();
    // Inline items over the first word of the body paragraph.
    let word_end = d
        .document
        .paragraph(body)
        .unwrap()
        .text
        .find(' ')
        .unwrap_or(body_len)
        .min(body_len);
    let mut inline = Vec::new();
    if word_end > 0 {
        let text = d.document.paragraph(body).unwrap().text.clone();
        let boundary_ok = text.is_char_boundary(word_end)
            && !text[word_end..].starts_with(|c: char| ('\u{300}'..='\u{36f}').contains(&c));
        if boundary_ok && i % 5 == 0 {
            let id = d.document.alloc_id().unwrap();
            inline.push(Inline::Link(Link {
                id,
                start: 0,
                end: word_end,
                target: LinkTarget::External(format!("https://example.org/page?i={i}&x=1")),
                tooltip: (i % 2 == 0).then(|| "Info".to_owned()),
            }));
        }
        if boundary_ok && i % 5 == 1 {
            let id = d.document.alloc_id().unwrap();
            inline.push(Inline::Bookmark(Bookmark {
                id,
                name: format!("signet_{i}"),
                start: 0,
                end: word_end,
            }));
        }
    }
    for item in inline {
        assert!(apply(
            &mut d.document,
            vec![RichOp::InsertInline { para: body, item }]
        ));
    }
    if i % 6 == 2 {
        let fp = push_paragraph(
            &mut d.document,
            section,
            &[("Page 1 sur 1".to_owned(), CharAttrs::default())],
            ParaProps::default(),
        );
        let id = d.document.alloc_id().unwrap();
        assert!(apply(
            &mut d.document,
            vec![RichOp::InsertInline {
                para: fp,
                item: Inline::Field(Field {
                    id,
                    start: 5,
                    end: 6,
                    instruction: "SEQ Figure".into(),
                }),
            }]
        ));
    }
    if i % 3 != 0 {
        let rows = 2 + i % 3;
        let cols = 2 + (i / 3) % 3;
        let sub = d.document.new_table(rows, cols).unwrap();
        let table = sub.root;
        let position = d.document.block_ids(Some(section)).unwrap().len();
        let mut ops = vec![RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree: sub,
        }];
        assert!(apply(&mut d.document, ops.drain(..).collect()));
        let mut texts = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if let Some(p) = cell_paragraph(&d.document, table, r, c) {
                    texts.push(RichOp::InsertText {
                        para: p,
                        offset: 0,
                        text: format!("{}{}-{}", ["a", "É", "z", "日"][(r + c) % 4], r, c),
                    });
                }
            }
        }
        assert!(apply(&mut d.document, texts));
        let merge = match i % 4 {
            1 => Some((0, 0, 1, 2)),
            2 => Some((0, 0, 2, 1)),
            3 if rows > 2 && cols > 2 => Some((1, 1, 2, 2)),
            _ => None,
        };
        if let Some((row, col, rs, cs)) = merge {
            apply(
                &mut d.document,
                vec![RichOp::MergeCells {
                    table,
                    row,
                    col,
                    rows: rs,
                    cols: cs,
                }],
            );
        }
        if i % 2 == 0 {
            apply(
                &mut d.document,
                vec![RichOp::SetTableStyle {
                    table,
                    style: Some("atelier.table.grid".into()),
                }],
            );
        }
        // A paragraph after the table.
        push_paragraph(
            &mut d.document,
            section,
            &[("Après le tableau".to_owned(), CharAttrs::default())],
            ParaProps::default(),
        );
    }
    if i % 8 == 3 {
        let sub = d.document.new_section().unwrap();
        let position = d.document.section_ids().len();
        assert!(apply(
            &mut d.document,
            vec![RichOp::InsertBlock {
                parent: None,
                position,
                subtree: sub,
            }]
        ));
        let s2 = last_section(&d.document);
        push_paragraph(
            &mut d.document,
            s2,
            &[("Deuxième section".to_owned(), CharAttrs::default())],
            ParaProps::default(),
        );
    }
    Built {
        name: format!("scratch-{i:03}"),
        doc: d,
    }
}

// ---- Random editing ---------------------------------------------------------

/// Container (a section body or a cell) and position of a block.
pub fn locate(doc: &RichDocument, block: NodeId) -> Option<(NodeId, usize)> {
    fn search(doc: &RichDocument, container: NodeId, block: NodeId) -> Option<(NodeId, usize)> {
        let ids = doc.block_ids(Some(container)).ok()?;
        if let Some(p) = ids.iter().position(|b| *b == block) {
            return Some((container, p));
        }
        for id in ids {
            if let Some(Node::Table(t)) = doc.node(id) {
                for r in &t.rows {
                    if let Some(Node::Row(row)) = doc.node(*r) {
                        for c in &row.cells {
                            if let Some(found) = search(doc, *c, block) {
                                return Some(found);
                            }
                        }
                    }
                }
            }
        }
        None
    }
    doc.section_ids().iter().find_map(|s| search(doc, *s, block))
}

fn tables(doc: &RichDocument) -> Vec<NodeId> {
    fn walk(doc: &RichDocument, container: NodeId, out: &mut Vec<NodeId>) {
        for id in doc.block_ids(Some(container)).unwrap_or_default() {
            if let Some(Node::Table(t)) = doc.node(id) {
                out.push(id);
                for r in &t.rows {
                    if let Some(Node::Row(row)) = doc.node(*r) {
                        for c in &row.cells {
                            walk(doc, *c, out);
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

fn offsets(text: &str) -> Vec<usize> {
    let mut v: Vec<usize> = text
        .char_indices()
        .filter(|(_, c)| !('\u{300}'..='\u{36f}').contains(c))
        .map(|(i, _)| i)
        .collect();
    v.push(text.len());
    v
}

/// One random valid edit; false when the model rejected the operation.
pub fn mutate(doc: &mut RichDocument, rng: &mut Rng) -> bool {
    let paragraphs = doc.paragraph_ids();
    if paragraphs.is_empty() {
        return false;
    }
    let pid = paragraphs[rng.below(paragraphs.len())];
    let text = doc.paragraph(pid).unwrap().text.clone();
    let cuts = offsets(&text);
    let at = cuts[rng.below(cuts.len())];
    match rng.below(14) {
        0 | 1 => {
            let s = SAMPLES[rng.below(SAMPLES.len())];
            apply(
                doc,
                vec![RichOp::InsertText {
                    para: pid,
                    offset: at,
                    text: s.into(),
                }],
            )
        }
        2 => {
            let end = cuts[rng.below(cuts.len())];
            let (a, b) = (at.min(end), at.max(end));
            a < b && apply(doc, vec![RichOp::DeleteText { para: pid, start: a, end: b }])
        }
        3 | 4 => {
            let end = cuts[rng.below(cuts.len())];
            let (a, b) = (at.min(end), at.max(end));
            let set = random_attrs(rng);
            let clear = if rng.chance(30) { random_attrs(rng) } else { CharAttrs::default() };
            a < b
                && apply(
                    doc,
                    vec![RichOp::SetCharAttrs {
                        para: pid,
                        start: a,
                        end: b,
                        set,
                        clear,
                    }],
                )
        }
        5 => {
            let set = random_props(rng);
            apply(
                doc,
                vec![RichOp::SetParaProps {
                    para: pid,
                    set,
                    clear: random_props(rng),
                }],
            )
        }
        6 => {
            let Some((parent, position)) = locate(doc, pid) else {
                return false;
            };
            let new_id = doc.alloc_id().unwrap();
            apply(
                doc,
                vec![RichOp::SplitParagraph {
                    parent: Some(parent),
                    position,
                    offset: at,
                    new_id,
                }],
            )
        }
        7 => {
            let Some((parent, position)) = locate(doc, pid) else {
                return false;
            };
            apply(
                doc,
                vec![RichOp::MergeParagraphs {
                    parent: Some(parent),
                    position,
                }],
            )
        }
        8 => {
            let Some((parent, position)) = locate(doc, pid) else {
                return false;
            };
            let sub = if rng.chance(25) {
                doc.new_table(1 + rng.below(3), 1 + rng.below(3)).unwrap()
            } else {
                doc.new_paragraph(SAMPLES[rng.below(SAMPLES.len())]).unwrap()
            };
            apply(
                doc,
                vec![RichOp::InsertBlock {
                    parent: Some(parent),
                    position: position + rng.below(2),
                    subtree: sub,
                }],
            )
        }
        9 => {
            let Some((parent, position)) = locate(doc, pid) else {
                return false;
            };
            apply(
                doc,
                vec![RichOp::RemoveBlock {
                    parent: Some(parent),
                    position,
                }],
            )
        }
        _ => {
            let ts = tables(doc);
            if ts.is_empty() {
                return false;
            }
            let table = ts[rng.below(ts.len())];
            let Some(Node::Table(t)) = doc.node(table).cloned() else {
                return false;
            };
            match rng.below(6) {
                0 => match doc.insert_row_op(table, rng.below(t.rows.len() + 1)) {
                    Ok(op) => apply(doc, vec![op]),
                    Err(_) => false,
                },
                1 => apply(
                    doc,
                    vec![RichOp::DeleteRow {
                        table,
                        index: rng.below(t.rows.len()),
                    }],
                ),
                2 => match doc.insert_column_op(table, rng.below(t.columns + 1)) {
                    Ok(op) => apply(doc, vec![op]),
                    Err(_) => false,
                },
                3 => apply(
                    doc,
                    vec![RichOp::DeleteColumn {
                        table,
                        index: rng.below(t.columns),
                    }],
                ),
                4 => apply(
                    doc,
                    vec![RichOp::MergeCells {
                        table,
                        row: rng.below(t.rows.len()),
                        col: rng.below(t.columns),
                        rows: 1 + rng.below(2),
                        cols: 1 + rng.below(2),
                    }],
                ),
                _ => {
                    let cell = t
                        .rows
                        .get(rng.below(t.rows.len()))
                        .and_then(|r| match doc.node(*r) {
                            Some(Node::Row(row)) => row.cells.get(rng.below(row.cells.len().max(1))).copied(),
                            _ => None,
                        });
                    match cell.map(|c| doc.split_cell_op(table, c)) {
                        Some(Ok(op)) => apply(doc, vec![op]),
                        _ => false,
                    }
                }
            }
        }
    }
}
