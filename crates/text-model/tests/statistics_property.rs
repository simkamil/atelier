// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests of the statistics (US-343), with a fixed-seed generator.
//!
//! - the total of a document is the sum of its paragraphs' counts;
//! - the incremental caches equal a full recount after random edits;
//! - the statistics of two adjacent selections agree with those of their
//!   union: characters add up exactly; a word cut by the common bound counts
//!   in both halves (words and paragraphs differ by at most one).

mod common;

use atelier_model::selection::{Affinity, Selection, SelectionRange, TextPosition};
use atelier_text_model::rich::{NodeId, RichDocument, RichOp};
use atelier_text_model::statistics::{
    CountingRules, PlainStatisticsCache, StatisticsCache, StatisticsOptions, count_paragraph,
    document, plain_text, selection,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use common::*;

const SEEDS: u64 = 48;
const STEPS: usize = 80;

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
    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }
}

const TEXTS: &[&str] = &[
    "a",
    "bc",
    "é",
    "e\u{301}",
    "日本",
    "こんにちは。",
    "👨\u{200d}👩\u{200d}👧",
    "🇫🇷",
    " ",
    "  ",
    "\t",
    "x y",
    "\n",
    "Hello",
    "a - b",
    "—",
    "l'école",
    "10\u{a0}km",
    "1,234.56",
    "\u{3000}",
    "안녕 ",
    "\u{200b}",
];

fn boundaries(text: &str) -> Vec<usize> {
    grapheme_boundaries(text).boundaries().to_vec()
}

fn rich_paragraphs(doc: &RichDocument) -> Vec<NodeId> {
    doc.paragraph_ids()
}

fn text_of(doc: &RichDocument, id: NodeId) -> String {
    doc.paragraph(id).unwrap().text.clone()
}

/// Plays one random operation and returns the paragraphs whose text changed
/// (structure changes are picked up by `sync`).
fn random_op(rng: &mut Rng, doc: &mut RichDocument) -> Vec<NodeId> {
    let paras = rich_paragraphs(doc);
    if paras.is_empty() {
        let section = first_section(doc);
        let subtree = doc.new_paragraph("renaît").unwrap();
        let _ = run(
            doc,
            vec![RichOp::InsertBlock {
                parent: Some(section),
                position: 0,
                subtree,
            }],
        );
        return Vec::new();
    }
    let para = rng.pick(&paras);
    let text = text_of(doc, para);
    let b = boundaries(&text);
    let section = first_section(doc);
    let op = match rng.below(8) {
        0..=2 => RichOp::InsertText {
            para,
            offset: rng.pick(&b),
            text: rng.pick(TEXTS).to_owned(),
        },
        3 => {
            let (x, y) = (rng.pick(&b), rng.pick(&b));
            RichOp::DeleteText {
                para,
                start: x.min(y),
                end: x.max(y),
            }
        }
        4 => {
            let Some(position) = doc
                .block_ids(Some(section))
                .unwrap()
                .iter()
                .position(|i| *i == para)
            else {
                return Vec::new(); // a paragraph inside a table cell
            };
            RichOp::SplitParagraph {
                parent: Some(section),
                position,
                offset: rng.pick(&b),
                new_id: doc.alloc_id().unwrap(),
            }
        }
        5 => {
            let ids = doc.block_ids(Some(section)).unwrap();
            if ids.len() < 2 {
                return Vec::new();
            }
            RichOp::MergeParagraphs {
                parent: Some(section),
                position: rng.below(ids.len() - 1),
            }
        }
        6 => {
            let ids = doc.block_ids(Some(section)).unwrap();
            if rng.below(3) == 0 && !ids.is_empty() {
                RichOp::RemoveBlock {
                    parent: Some(section),
                    position: rng.below(ids.len()),
                }
            } else {
                let subtree = if rng.below(4) == 0 {
                    doc.new_table(1 + rng.below(2), 1 + rng.below(3)).unwrap()
                } else {
                    doc.new_paragraph(&rng.pick(TEXTS).repeat(1 + rng.below(3)))
                        .unwrap()
                };
                RichOp::InsertBlock {
                    parent: Some(section),
                    position: rng.below(ids.len() + 1),
                    subtree,
                }
            }
        }
        _ => {
            // text in a table cell, if any
            let Some(cell_para) = paras
                .iter()
                .rev()
                .copied()
                .find(|p| !doc.block_ids(Some(section)).unwrap().contains(p))
            else {
                return Vec::new();
            };
            RichOp::InsertText {
                para: cell_para,
                offset: 0,
                text: rng.pick(TEXTS).to_owned(),
            }
        }
    };
    let touched = match &op {
        RichOp::InsertText { para, .. } | RichOp::DeleteText { para, .. } => vec![*para],
        RichOp::SplitParagraph { .. } | RichOp::MergeParagraphs { .. } => paras.clone(),
        _ => Vec::new(),
    };
    if run(doc, vec![op]).is_ok() {
        touched
    } else {
        Vec::new()
    }
}

fn full(doc: &RichDocument, options: &StatisticsOptions) -> atelier_text_model::statistics::Counts {
    document(doc, options).counts()
}

#[test]
fn total_is_the_sum_of_paragraphs_and_cache_matches_recount() {
    let options = StatisticsOptions::default();
    let (mut applied, mut multi_paragraph) = (0, 0);
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        let (mut doc, _) = with_text("départ");
        let mut cache = StatisticsCache::build(&doc, options);
        for step in 0..STEPS {
            let touched = random_op(&mut rng, &mut doc);
            applied += usize::from(!touched.is_empty());
            multi_paragraph += usize::from(doc.paragraph_ids().len() > 3);
            doc.validate().unwrap();
            // sum of the per-paragraph statistics = global statistic
            let rules = CountingRules::default();
            let sum: atelier_text_model::statistics::Counts = doc
                .paragraph_ids()
                .iter()
                .map(|p| count_paragraph(&doc.paragraph(*p).unwrap().text, &rules))
                .sum();
            assert_eq!(sum, full(&doc, &options), "seed {seed} step {step}");
            // incremental update = full recount
            if rng.below(2) == 0 {
                cache.sync(&doc, &touched);
            } else if touched.len() == 1 && cache.paragraph_counts(touched[0]).is_some() {
                cache.update_paragraph(&doc, touched[0]).unwrap();
                cache.sync(&doc, &[]);
            } else {
                cache.sync(&doc, &touched);
            }
            assert_eq!(
                cache.counts(),
                full(&doc, &options),
                "seed {seed} step {step}"
            );
        }
        // a cache built from scratch agrees too
        assert_eq!(
            StatisticsCache::build(&doc, options).counts(),
            cache.counts()
        );
    }
    // the generator really edits (not only failing operations)
    assert!(applied > SEEDS as usize * STEPS / 3, "{applied} edits");
    assert!(
        multi_paragraph > SEEDS as usize * STEPS / 3,
        "{multi_paragraph} steps"
    );
}

fn position(rng: &mut Rng, doc: &RichDocument) -> (NodeId, usize) {
    let para = rng.pick(&rich_paragraphs(doc));
    let offset = rng.pick(&boundaries(&text_of(doc, para)));
    (para, offset)
}

fn order_key(doc: &RichDocument, p: (NodeId, usize)) -> (usize, usize) {
    let i = rich_paragraphs(doc).iter().position(|x| *x == p.0).unwrap();
    (i, p.1)
}

fn sel(a: (NodeId, usize), b: (NodeId, usize)) -> Selection {
    let pos = |(n, o)| TextPosition::new(n, o, Affinity::After);
    Selection::new(vec![SelectionRange::new(pos(a), pos(b))], 0).unwrap()
}

#[test]
fn adjacent_selections_agree_with_their_union() {
    let options = StatisticsOptions::default();
    for seed in 0..SEEDS {
        let mut rng = Rng::new(1000 + seed);
        let (mut doc, _) = with_text("départ");
        for _ in 0..STEPS {
            random_op(&mut rng, &mut doc);
        }
        if doc.paragraph_ids().is_empty() {
            random_op(&mut rng, &mut doc);
        }
        for _ in 0..40 {
            let mut p = [
                position(&mut rng, &doc),
                position(&mut rng, &doc),
                position(&mut rng, &doc),
            ];
            p.sort_by_key(|x| order_key(&doc, *x));
            let left = selection(&doc, &sel(p[0], p[1]), &options).unwrap();
            let right = selection(&doc, &sel(p[1], p[2]), &options).unwrap();
            let union = selection(&doc, &sel(p[0], p[2]), &options).unwrap();
            assert_eq!(
                left.characters + right.characters,
                union.characters,
                "seed {seed}"
            );
            assert_eq!(
                left.characters_with_spaces + right.characters_with_spaces,
                union.characters_with_spaces,
                "seed {seed}"
            );
            // A word (or a paragraph) cut by the common bound counts twice.
            let words = (left.words + right.words).checked_sub(union.words);
            assert!(
                matches!(words, Some(0 | 1)),
                "seed {seed}: {left:?} {right:?} {union:?}"
            );
            let paras = (left.paragraphs + right.paragraphs).checked_sub(union.paragraphs);
            assert!(
                matches!(paras, Some(0 | 1)),
                "seed {seed}: {left:?} {right:?} {union:?}"
            );
            // the whole reversed selection gives the same figures
            assert_eq!(union, selection(&doc, &sel(p[2], p[0]), &options).unwrap());
        }
        // A selection from the very start to the very end is the whole body.
        let paras = rich_paragraphs(&doc);
        let (first, last) = (paras[0], *paras.last().unwrap());
        let end = text_of(&doc, last).len();
        let all = selection(&doc, &sel((first, 0), (last, end)), &options).unwrap();
        assert_eq!(all.counts(), full(&doc, &options), "seed {seed}");
    }
}

fn random_plain_edit(rng: &mut Rng, text: &mut String) -> (usize, usize, usize) {
    const PIECES: &[&str] = &[
        "a", "bc ", "\n", "\r\n", "\r", "\u{2029}", "日本", " ", "x - y", "é", "\n\n", "\u{a0}",
    ];
    let b = boundaries(text);
    let (x, y) = (rng.pick(&b), rng.pick(&b));
    let (start, end) = (x.min(y), x.max(y));
    let end = if rng.below(2) == 0 { start } else { end };
    let mut piece = String::new();
    for _ in 0..rng.below(4) {
        piece += PIECES[rng.below(PIECES.len())];
    }
    text.replace_range(start..end, &piece);
    (start, end, piece.len())
}

#[test]
fn plain_cache_update_equals_full_recount() {
    let options = StatisticsOptions::default();
    for seed in 0..SEEDS {
        let mut rng = Rng::new(5000 + seed);
        let mut text = String::from("Un deux\r\ntrois\n\nquatre");
        let mut cache = PlainStatisticsCache::build(&text, options);
        for step in 0..STEPS * 3 {
            let (start, old_end, new_len) = random_plain_edit(&mut rng, &mut text);
            cache.update(&text, start, old_end, new_len);
            let fresh = PlainStatisticsCache::build(&text, options);
            assert_eq!(
                cache.counts(),
                fresh.counts(),
                "seed {seed} step {step} {text:?}"
            );
            assert_eq!(
                cache.paragraph_len(),
                fresh.paragraph_len(),
                "seed {seed} step {step}"
            );
            assert_eq!(cache.statistics(), plain_text(&text, &options));
        }
    }
}

#[test]
fn plain_cache_survives_wrong_arguments_by_recounting() {
    let options = StatisticsOptions::default();
    let mut cache = PlainStatisticsCache::build("a b\nc", options);
    cache.update("completely different text", 0, 0, 0);
    assert_eq!(
        cache.counts(),
        PlainStatisticsCache::build("completely different text", options).counts()
    );
}
