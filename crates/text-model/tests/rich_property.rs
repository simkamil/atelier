// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests of the rich document model (US-200 / US-071).
//!
//! A fixed-seed generator plays long random sequences of operations. After each
//! applied operation the invariants must hold; a failing operation must leave
//! the document untouched; and a whole transaction followed by its inverse
//! must give back a structurally identical document, and replaying it the
//! same result again.

mod common;

use atelier_text_model::rich::*;
use atelier_unicode::segmentation::grapheme_boundaries;
use common::*;
use std::collections::BTreeMap;

const SEEDS: u64 = 64;
const STEPS: usize = 100;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5851_f42d_4c95_7f2d)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
    fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        if items.is_empty() {
            None
        } else {
            Some(items[self.below(items.len())].clone())
        }
    }
}

const TEXTS: &[&str] = &[
    "a",
    "bc",
    "é",
    "e\u{301}",
    "日本",
    "👨\u{200d}👩\u{200d}👧",
    "🇫🇷",
    "x y",
    "\n",
    "Hello",
    "\u{301}",
    "\u{200d}",
];
const NAMES: &[&str] = &["alpha", "beta", "gamma"];
const FONTS: &[&str] = &["Carlito", "Caladea", "Liberation Serif"];

#[derive(Default)]
struct Scan {
    containers: Vec<Option<NodeId>>,
    sections: Vec<NodeId>,
    tables: Vec<NodeId>,
}

fn scan(doc: &RichDocument, id: NodeId, out: &mut Scan) {
    match doc.node(id) {
        Some(Node::Section(s)) => {
            out.sections.push(id);
            out.containers.push(Some(id));
            for c in s.body.to_vec() {
                scan(doc, c, out);
            }
            for (_, h) in s.headers.iter().chain(s.footers.iter()) {
                scan(doc, *h, out);
            }
        }
        Some(Node::HeaderFooter(h)) => {
            out.containers.push(Some(id));
            for c in h.blocks.to_vec() {
                scan(doc, c, out);
            }
        }
        Some(Node::Table(t)) => {
            out.tables.push(id);
            for r in &t.rows {
                if let Some(Node::Row(row)) = doc.node(*r) {
                    for c in &row.cells {
                        scan(doc, *c, out);
                    }
                }
            }
        }
        Some(Node::Cell(c)) => {
            out.containers.push(Some(id));
            for b in c.blocks.to_vec() {
                scan(doc, b, out);
            }
        }
        _ => {}
    }
}

fn scan_doc(doc: &RichDocument) -> Scan {
    let mut out = Scan {
        containers: vec![None],
        ..Scan::default()
    };
    for s in doc.section_ids() {
        scan(doc, s, &mut out);
    }
    out
}

fn offset(rng: &mut Rng, doc: &RichDocument, para: NodeId) -> usize {
    let text = &doc.paragraph(para).unwrap().text;
    if rng.chance(14) {
        return rng.below(text.len() + 3);
    }
    let b = grapheme_boundaries(text).boundaries().to_vec();
    b[rng.below(b.len())]
}

fn range(rng: &mut Rng, doc: &RichDocument, para: NodeId) -> (usize, usize) {
    let (a, b) = (offset(rng, doc, para), offset(rng, doc, para));
    if rng.chance(30) {
        (a, b)
    } else {
        (a.min(b), a.max(b))
    }
}

fn random_char_attrs(rng: &mut Rng) -> CharAttrs {
    let mut a = CharAttrs::default();
    // Mostly one of a few simple sets, so that equal neighbours (and thus run
    // normalization) really occur.
    if !rng.chance(4) {
        match rng.below(3) {
            0 => a.bold = Some(true),
            1 => a.italic = Some(true),
            _ => a.font = Some(FONTS[0].to_owned()),
        }
        return a;
    }
    if rng.chance(2) {
        a.bold = Some(rng.chance(2));
    }
    if rng.chance(3) {
        a.italic = Some(rng.chance(2));
    }
    if rng.chance(5) {
        a.underline = rng.pick(&[Underline::None, Underline::Single, Underline::Wave]);
    }
    if rng.chance(6) {
        a.strike = Some(true);
    }
    if rng.chance(8) {
        a.vertical_align = rng.pick(&[VerticalAlign::Superscript, VerticalAlign::Subscript]);
    }
    if rng.chance(5) {
        a.font = rng.pick(FONTS).map(str::to_owned);
    }
    if rng.chance(5) {
        a.size_half_points = Some(16 + 2 * rng.below(8) as u32);
    }
    if rng.chance(6) {
        a.color = Some(Color {
            r: rng.below(256) as u8,
            g: 0,
            b: 7,
        });
    }
    if rng.chance(8) {
        a.highlight = Some(Color {
            r: 255,
            g: 255,
            b: 0,
        });
    }
    if rng.chance(6) {
        a.opaque.push(OpaqueAttr {
            namespace: "x".into(),
            name: format!("k{}", rng.below(3)),
            value: vec![rng.below(256) as u8],
        });
    }
    a
}

fn random_para_props(rng: &mut Rng) -> ParaProps {
    let mut p = ParaProps::default();
    if rng.chance(2) {
        p.alignment = rng.pick(&[
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
        ]);
    }
    if rng.chance(3) {
        p.indent_left = Some(rng.below(2000) as i32);
    }
    if rng.chance(4) {
        p.indent_right = Some(rng.below(500) as i32);
    }
    if rng.chance(4) {
        p.indent_first_line = Some(rng.below(600) as i32 - 300);
    }
    if rng.chance(4) {
        p.space_before = Some(rng.below(400) as u32);
    }
    if rng.chance(4) {
        p.space_after = Some(rng.below(400) as u32);
    }
    if rng.chance(4) {
        p.line_spacing = Some(LineSpacing {
            rule: LineRule::Auto,
            value: 240 + rng.below(100) as u32,
        });
    }
    if rng.chance(4) {
        p.style = Some(format!("S{}", rng.below(3)));
    }
    if rng.chance(4) {
        p.list = Some(ListRef {
            list_id: rng.below(3) as u32,
            level: rng.below(11) as u8,
        });
    }
    if rng.chance(8) {
        p.opaque.push(OpaqueAttr {
            namespace: "w".into(),
            name: "p".into(),
            value: vec![],
        });
    }
    p
}

fn random_inline(rng: &mut Rng, doc: &mut RichDocument, para: NodeId, id: NodeId) -> Inline {
    let (a, b) = range(rng, doc, para);
    match rng.below(4) {
        0 => Inline::Anchored(AnchoredObject {
            id,
            offset: a,
            kind: "pic".into(),
            data: vec![rng.below(256) as u8],
        }),
        1 => Inline::Field(Field {
            id,
            start: a,
            end: b,
            instruction: format!("F{}", rng.below(3)),
        }),
        2 => Inline::Bookmark(Bookmark {
            id,
            name: rng.pick(NAMES).unwrap().to_owned(),
            start: a,
            end: b,
        }),
        _ => Inline::Link(Link {
            id,
            start: a,
            end: b,
            target: if rng.chance(2) {
                LinkTarget::External("https://example.org".into())
            } else {
                LinkTarget::Bookmark("alpha".into())
            },
            tooltip: if rng.chance(2) {
                Some("t".into())
            } else {
                None
            },
        }),
    }
}

fn grid_size(doc: &RichDocument, table: NodeId) -> (usize, usize) {
    match doc.node(table) {
        Some(Node::Table(t)) => (t.rows.len(), t.columns),
        _ => (0, 0),
    }
}

fn gen_op(rng: &mut Rng, doc: &mut RichDocument) -> RichOp {
    let sc = scan_doc(doc);
    let paras = doc.paragraph_ids();
    let Some(para) = rng.pick(&paras) else {
        let section = sc.sections[0];
        let subtree = doc.new_paragraph("p").unwrap();
        return RichOp::InsertBlock {
            parent: Some(section),
            position: 0,
            subtree,
        };
    };
    let paragraph_count = paras.len();
    match rng.below(34) {
        0..=4 => {
            let text = rng.pick(TEXTS).unwrap().to_owned();
            RichOp::InsertText {
                para,
                offset: offset(rng, doc, para),
                text,
            }
        }
        5..=6 => {
            let (start, end) = range(rng, doc, para);
            RichOp::DeleteText { para, start, end }
        }
        7..=9 => {
            let (start, end) = range(rng, doc, para);
            RichOp::SetCharAttrs {
                para,
                start,
                end,
                set: random_char_attrs(rng),
                clear: random_char_attrs(rng),
            }
        }
        10 => RichOp::SetParaProps {
            para,
            set: random_para_props(rng),
            clear: random_para_props(rng),
        },
        11..=12 => {
            let id = doc.alloc_id().unwrap();
            let item = random_inline(rng, doc, para, id);
            RichOp::InsertInline { para, item }
        }
        13 => {
            let p = doc.paragraph(para).unwrap();
            let ids = p.inline_ids();
            let (a, b) = range(rng, doc, para);
            match rng.pick(&ids).and_then(|id| p.inline(id)) {
                Some(Inline::Anchored(mut x)) => {
                    x.offset = a;
                    x.data.push(1);
                    RichOp::ModifyInline {
                        para,
                        item: Inline::Anchored(x),
                    }
                }
                Some(Inline::Field(mut x)) => {
                    x.start = a;
                    x.end = b;
                    RichOp::ModifyInline {
                        para,
                        item: Inline::Field(x),
                    }
                }
                Some(Inline::Bookmark(mut x)) => {
                    x.start = a;
                    x.end = b;
                    if rng.chance(2) {
                        x.name = rng.pick(NAMES).unwrap().to_owned();
                    }
                    RichOp::ModifyInline {
                        para,
                        item: Inline::Bookmark(x),
                    }
                }
                Some(Inline::Link(mut x)) => {
                    x.start = a;
                    x.end = b;
                    RichOp::ModifyInline {
                        para,
                        item: Inline::Link(x),
                    }
                }
                None => RichOp::RemoveInline {
                    para,
                    id: doc.alloc_id().unwrap(),
                },
            }
        }
        14 => {
            let ids = doc.paragraph(para).unwrap().inline_ids();
            let id = rng.pick(&ids).unwrap_or(para);
            RichOp::RemoveInline { para, id }
        }
        15..=17 => {
            let parent = rng.pick(&sc.containers).unwrap();
            let blocks = doc.block_ids(parent).unwrap();
            let position = rng.below(blocks.len() + 1);
            let new_id = doc.alloc_id().unwrap();
            let target = blocks.get(position).copied().unwrap_or(para);
            let offset = if let Ok(p) = doc.paragraph(target) {
                let _ = p;
                offset(rng, doc, target)
            } else {
                0
            };
            RichOp::SplitParagraph {
                parent,
                position,
                offset,
                new_id,
            }
        }
        18..=19 => {
            let parent = rng.pick(&sc.containers).unwrap();
            let len = doc.block_ids(parent).unwrap().len();
            RichOp::MergeParagraphs {
                parent,
                position: rng.below(len + 1),
            }
        }
        20..=21 => {
            let parent = rng.pick(&sc.containers).unwrap();
            let position = rng.below(doc.block_ids(parent).unwrap().len() + 1);
            let subtree = if parent.is_none() && !rng.chance(10) {
                doc.new_section().unwrap()
            } else if paragraph_count > 14 {
                doc.new_opaque("w:x", &[1]).unwrap()
            } else {
                match rng.below(5) {
                    0 => doc.new_table(1 + rng.below(3), 1 + rng.below(3)).unwrap(),
                    1 => doc.new_opaque("w:sdt", &[rng.below(256) as u8]).unwrap(),
                    _ => doc.new_paragraph(rng.pick(TEXTS).unwrap()).unwrap(),
                }
            };
            RichOp::InsertBlock {
                parent,
                position,
                subtree,
            }
        }
        22..=23 => {
            let parent = rng.pick(&sc.containers).unwrap();
            let len = doc.block_ids(parent).unwrap().len();
            RichOp::RemoveBlock {
                parent,
                position: rng.below(len + 1),
            }
        }
        24 => {
            let section = rng.pick(&sc.sections).unwrap();
            let kind = if rng.chance(2) {
                HfKind::Header
            } else {
                HfKind::Footer
            };
            let variant = rng
                .pick(&[HfVariant::Default, HfVariant::First, HfVariant::Even])
                .unwrap();
            RichOp::AttachHeaderFooter {
                section,
                kind,
                variant,
                subtree: doc.new_header_footer("hf").unwrap(),
            }
        }
        25 => {
            let section = rng.pick(&sc.sections).unwrap();
            let kind = if rng.chance(2) {
                HfKind::Header
            } else {
                HfKind::Footer
            };
            let variant = rng
                .pick(&[HfVariant::Default, HfVariant::First, HfVariant::Even])
                .unwrap();
            RichOp::DetachHeaderFooter {
                section,
                kind,
                variant,
            }
        }
        26..=33 => {
            let Some(table) = rng.pick(&sc.tables) else {
                return RichOp::InsertText {
                    para,
                    offset: 0,
                    text: "t".into(),
                };
            };
            let (rows, cols) = grid_size(doc, table);
            match rng.below(8) {
                0 => {
                    let i = rng.below(rows + 1);
                    doc.insert_row_op(table, i).unwrap()
                }
                1 => RichOp::DeleteRow {
                    table,
                    index: rng.below(rows + 1),
                },
                2 => {
                    let i = rng.below(cols + 1);
                    doc.insert_column_op(table, i).unwrap()
                }
                3 => RichOp::DeleteColumn {
                    table,
                    index: rng.below(cols + 1),
                },
                4 | 5 => {
                    let (row, col) = (rng.below(rows), rng.below(cols));
                    RichOp::MergeCells {
                        table,
                        row,
                        col,
                        rows: 1 + rng.below(rows - row),
                        cols: 1 + rng.below(cols - col),
                    }
                }
                _ => {
                    let g = cells(doc, table);
                    let merged: Vec<NodeId> = g
                        .iter()
                        .flatten()
                        .copied()
                        .filter(|c| span(doc, *c) != (1, 1))
                        .collect();
                    let cell = rng.pick(&merged).or_else(|| rng.pick(&g[0])).unwrap();
                    doc.split_cell_op(table, cell).unwrap()
                }
            }
        }
        _ => unreachable!(),
    }
}

/// Operations that are valid by construction and must never be rejected: a
/// rejection there would mean the implementation refuses legitimate edits.
fn must_succeed(op: &RichOp, doc: &RichDocument) -> bool {
    match op {
        RichOp::SetCharAttrs {
            para, start, end, ..
        } => {
            let Ok(p) = doc.paragraph(*para) else {
                return false;
            };
            let b = grapheme_boundaries(&p.text);
            start <= end
                && b.is_boundary(*start).unwrap_or(false)
                && b.is_boundary(*end).unwrap_or(false)
        }
        RichOp::SetParaProps { set, .. } => set.list.is_none_or(|l| l.level <= MAX_LIST_LEVEL),
        RichOp::InsertBlock {
            parent,
            position,
            subtree,
        } => {
            let ok_kind = match (parent, subtree.nodes.get(&subtree.root)) {
                (None, Some(Node::Section(_))) => true,
                (Some(_), Some(n)) => n.is_block(),
                _ => false,
            };
            ok_kind && doc.block_ids(*parent).is_ok_and(|b| *position <= b.len())
        }
        RichOp::InsertRow { .. } | RichOp::InsertColumn { .. } => true,
        _ => false,
    }
}

fn kind(op: &RichOp) -> String {
    let d = format!("{op:?}");
    d.split([' ', '{', '(']).next().unwrap().to_owned()
}

#[derive(Default)]
struct Stats {
    sequences: usize,
    transactions: usize,
    operations_ok: usize,
    operations_rejected: usize,
    failed_transactions: usize,
    by_kind: BTreeMap<String, (usize, usize)>,
}

fn invalid_tail(rng: &mut Rng, doc: &mut RichDocument) -> RichOp {
    let para = doc
        .paragraph_ids()
        .first()
        .copied()
        .unwrap_or(doc.section_ids()[0]);
    match rng.below(4) {
        0 => RichOp::DeleteText {
            para,
            start: 5,
            end: 2,
        },
        1 => RichOp::RemoveBlock {
            parent: None,
            position: 999,
        },
        2 => RichOp::RemoveInline {
            para,
            id: doc.alloc_id().unwrap(),
        },
        _ => RichOp::InsertText {
            para: doc.alloc_id().unwrap(),
            offset: 0,
            text: "x".into(),
        },
    }
}

#[test]
fn random_sequences_keep_invariants_and_undo_is_identity() {
    let mut stats = Stats::default();
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        let mut doc = RichDocument::new();
        stats.sequences += 1;
        for _ in 0..STEPS {
            let before = doc.clone();
            let mut ops = Vec::new();
            let mut inverses: Vec<Vec<RichOp>> = Vec::new();
            for _ in 0..1 + rng.below(4) {
                let op = gen_op(&mut rng, &mut doc);
                let pre = doc.clone();
                let required = must_succeed(&op, &doc);
                let entry = stats.by_kind.entry(kind(&op)).or_default();
                match run(&mut doc, vec![op.clone()]) {
                    Ok(inverse) => {
                        entry.0 += 1;
                        stats.operations_ok += 1;
                        doc.validate().unwrap_or_else(|e| {
                            panic!("seed {seed}: {op:?} broke an invariant: {e}")
                        });
                        inverses.push(inverse.operations().to_vec());
                        ops.push(op);
                    }
                    Err(e) => {
                        assert!(!required, "seed {seed}: valid {op:?} was rejected: {e}");
                        entry.1 += 1;
                        stats.operations_rejected += 1;
                        assert_eq!(
                            doc, pre,
                            "seed {seed}: rejected {op:?} mutated the document"
                        );
                    }
                }
            }
            if ops.is_empty() {
                continue;
            }
            stats.transactions += 1;
            let after = doc.clone();
            // A transaction ending with an invalid operation changes nothing.
            if rng.chance(4) {
                let mut with_bad = ops.clone();
                with_bad.push(invalid_tail(&mut rng, &mut doc));
                let snapshot = doc.clone();
                assert!(run(&mut doc, with_bad).is_err());
                assert_eq!(
                    doc, snapshot,
                    "seed {seed}: failed transaction mutated the document"
                );
                stats.failed_transactions += 1;
            }
            // apply then invert = identity
            let undo: Vec<RichOp> = inverses.into_iter().rev().flatten().collect();
            for op in &undo {
                *stats.by_kind.entry(kind(op)).or_default() = {
                    let e = stats.by_kind.get(&kind(op)).copied().unwrap_or_default();
                    (e.0 + 1, e.1)
                };
            }
            run(&mut doc, undo).unwrap_or_else(|e| panic!("seed {seed}: undo failed: {e}"));
            doc.validate().unwrap();
            assert_eq!(doc, before, "seed {seed}: undo is not the identity");
            // redo
            run(&mut doc, ops).unwrap_or_else(|e| panic!("seed {seed}: redo failed: {e}"));
            doc.validate().unwrap();
            assert_eq!(doc, after, "seed {seed}: redo differs");
        }
    }
    println!(
        "sequences={} transactions={} ops_ok={} ops_rejected={} failed_transactions={}",
        stats.sequences,
        stats.transactions,
        stats.operations_ok,
        stats.operations_rejected,
        stats.failed_transactions
    );
    for (k, (ok, rejected)) in &stats.by_kind {
        println!("  {k}: applied {ok}, rejected {rejected}");
    }
    for needed in [
        "InsertText",
        "DeleteText",
        "SetCharAttrs",
        "SetParaProps",
        "InsertInline",
        "ModifyInline",
        "RemoveInline",
        "SplitParagraph",
        "MergeParagraphs",
        "InsertBlock",
        "RemoveBlock",
        "AttachHeaderFooter",
        "DetachHeaderFooter",
        "InsertRow",
        "DeleteRow",
        "InsertColumn",
        "DeleteColumn",
        "MergeCells",
        "SplitCell",
        "RestoreSubtree",
        "CollapsePair",
        "ExpandPair",
    ] {
        let (ok, _) = stats.by_kind.get(needed).copied().unwrap_or_default();
        assert!(ok > 0, "operation {needed} was never applied successfully");
    }
    assert!(stats.transactions > 3_000);
}

#[test]
fn the_generator_is_deterministic() {
    let play = || {
        let mut rng = Rng::new(7);
        let mut doc = RichDocument::new();
        for _ in 0..60 {
            let op = gen_op(&mut rng, &mut doc);
            let _ = run(&mut doc, vec![op]);
        }
        doc
    };
    assert_eq!(play(), play());
}
