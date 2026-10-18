// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(dead_code)]

use atelier_model::Transaction;
use atelier_text_model::rich::{
    AnchoredObject, BreakKind, Inline, InlineBreak, InlineSymbol, Node, NodeId, RichDocument,
    RichOp,
};
use atelier_unicode::segmentation::grapheme_boundaries;

/// Fixed-seed generator (same family as `rich_property.rs`).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5851_f42d_4c95_7f2d)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

/// Pieces of text: ASCII, accents, combining marks, CJK, emoji, ZWJ
/// sequences, flags, spaces, tabs.
pub const PIECES: &[&str] = &[
    "a",
    "bc",
    "hello",
    "world",
    " ",
    "  ",
    "\t",
    "\u{e9}",
    "e\u{301}",
    "\u{301}",
    "\u{65e5}\u{672c}",
    "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
    "\u{1f1eb}\u{1f1f7}",
    "x-y",
    "don't",
    "3.14",
];

/// Text sent by an edit: pieces and `\n`.
pub const EDIT_PIECES: &[&str] = &[
    "a",
    "bc",
    "hello",
    " ",
    "\n",
    "\n",
    "\u{301}",
    "e\u{301}",
    "\u{e9}",
    "\u{65e5}",
    "\u{1f468}\u{200d}\u{1f469}",
    "\u{1f1eb}\u{1f1f7}",
    "\t",
];

pub fn random_text(rng: &mut Rng, pieces: &[&str], max: usize) -> String {
    let mut out = String::new();
    for _ in 0..rng.below(max + 1) {
        out.push_str(pieces[rng.below(pieces.len())]);
    }
    out
}

pub fn apply(doc: &mut RichDocument, ops: Vec<RichOp>) {
    doc.apply(&Transaction::new(ops).unwrap()).unwrap();
}

/// Adds a random non-text item to a paragraph.
pub fn add_item(doc: &mut RichDocument, rng: &mut Rng, para: NodeId) {
    let text = doc.paragraph(para).unwrap().text.clone();
    let bounds = grapheme_boundaries(&text);
    let boundaries = bounds.boundaries().to_vec();
    let offset = boundaries[rng.below(boundaries.len())];
    let id = doc.alloc_id().unwrap();
    let item = match rng.below(3) {
        0 => Inline::Anchored(AnchoredObject {
            id,
            offset,
            kind: "pic".into(),
            data: vec![id_byte(rng), 7],
        }),
        1 => Inline::Break(InlineBreak {
            id,
            offset,
            kind: [BreakKind::Line, BreakKind::Page, BreakKind::Column][rng.below(3)],
        }),
        _ => Inline::Symbol(InlineSymbol {
            id,
            offset,
            font: "Symbol".into(),
            code: 0xf0b7,
        }),
    };
    apply(doc, vec![RichOp::InsertInline { para, item }]);
}

fn id_byte(rng: &mut Rng) -> u8 {
    rng.below(256) as u8
}

/// A document of `count` body paragraphs of random text, some with items.
pub fn random_body(rng: &mut Rng, count: usize, items: bool) -> RichDocument {
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let first = doc.paragraph_ids()[0];
    let text = random_text(rng, PIECES, 6);
    apply(
        &mut doc,
        vec![RichOp::InsertText {
            para: first,
            offset: 0,
            text,
        }],
    );
    for i in 1..count {
        let sub = doc.new_paragraph(&random_text(rng, PIECES, 6)).unwrap();
        apply(
            &mut doc,
            vec![RichOp::InsertBlock {
                parent: Some(section),
                position: i,
                subtree: sub,
            }],
        );
    }
    if items {
        for para in doc.paragraph_ids() {
            for _ in 0..rng.below(3) {
                add_item(&mut doc, rng, para);
            }
        }
    }
    doc.validate().unwrap();
    doc
}

/// Like [`random_body`], plus a table and an opaque block among the blocks.
pub fn random_structured(rng: &mut Rng, count: usize) -> RichDocument {
    let mut doc = random_body(rng, count, true);
    let section = doc.section_ids()[0];
    let rows = 1 + rng.below(2);
    let cols = 1 + rng.below(2);
    let table = doc.new_table(rows, cols).unwrap();
    let position = rng.below(count + 1);
    apply(
        &mut doc,
        vec![RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree: table,
        }],
    );
    let opaque = doc.new_opaque("w:sdt", b"<w:sdt/>").unwrap();
    let position = rng.below(doc.block_ids(Some(section)).unwrap().len() + 1);
    apply(
        &mut doc,
        vec![RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree: opaque,
        }],
    );
    for para in doc.paragraph_ids() {
        if doc.paragraph(para).unwrap().text.is_empty() && rng.chance(2) {
            let text = random_text(rng, PIECES, 3);
            apply(
                &mut doc,
                vec![RichOp::InsertText {
                    para,
                    offset: 0,
                    text,
                }],
            );
        }
    }
    doc.validate().unwrap();
    doc
}

/// Number of nodes of each kind, to spot leaks.
pub fn node_kinds(doc: &RichDocument) -> (usize, usize) {
    let mut paragraphs = 0;
    let mut others = 0;
    for id in doc.paragraph_ids() {
        if matches!(doc.node(id), Some(Node::Paragraph(_))) {
            paragraphs += 1;
        } else {
            others += 1;
        }
    }
    (paragraphs, others)
}
