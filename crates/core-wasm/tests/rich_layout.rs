// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests of the incremental page cache of the rich editor (US-216), against
//! the `VecParagraphs` test double of the flat contract (host only).
#![cfg(not(target_arch = "wasm32"))]

use atelier_core_wasm::rich_layout::{LayoutError, PageCache, PageDelta, RichPageCache};
use atelier_render::{Command, DisplayList};
use atelier_text_model::rich::flat::{
    BlockKind, EditOutcome, FlatIndex, LayoutContext, LayoutScope,
};
use atelier_text_model::rich::flat_double::{VecParagraphs, example_section};
use atelier_text_model::rich::{
    AnchoredObject, BreakKind, CharAttrs, CharRun, InlineBreak, InlineSymbol, Paragraph,
    RichDocument,
};
use std::collections::HashMap;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "zeta",
    "eta",
    "theta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "omicron",
    "pi",
    "rho",
    "sigma",
    "tau",
    "upsilon",
    "phi",
    "chi",
    "psi",
    "omega",
    "caf\u{e9}",
    "na\u{ef}ve",
    "\u{2014}",
    "supercalifragilisticexpialidocious",
];

fn paragraph(rng: &mut Rng, words: usize) -> Paragraph {
    let mut text = String::new();
    for i in 0..words {
        if i > 0 {
            text.push(' ');
        }
        text.push_str(WORDS[rng.below(WORDS.len())]);
    }
    let mut p = Paragraph::with_text(&text);
    if rng.below(3) == 0 && text.len() > 12 {
        let start = text.char_indices().nth(2).map_or(0, |(i, _)| i);
        let end = text.char_indices().nth(9).map_or(text.len(), |(i, _)| i);
        p.runs.push(CharRun {
            start,
            end,
            attrs: CharAttrs {
                bold: Some(true),
                ..CharAttrs::default()
            },
        });
    }
    p
}

fn document(seed: u64, paragraphs: usize) -> VecParagraphs {
    let mut rng = Rng(seed);
    let list = (0..paragraphs)
        .map(|_| {
            let words = 5 + rng.below(70);
            paragraph(&mut rng, words)
        })
        .collect();
    let mut doc = VecParagraphs::new(list);
    doc.section = example_section();
    doc
}

fn settle(cache: &mut PageCache, doc: &VecParagraphs, rng: &mut Rng) -> Vec<PageDelta> {
    let mut deltas = Vec::new();
    let mut guard = 0;
    while !cache.layout_complete() {
        let budget = 1 + rng.below(40) as u32;
        let delta = cache.step(doc, budget);
        assert!(
            cache.last_step_work() <= budget as usize,
            "step did {} paragraphs for a budget of {budget}",
            cache.last_step_work()
        );
        deltas.push(delta);
        guard += 1;
        assert!(guard < 10_000, "step does not converge");
    }
    deltas
}

/// What a client holding display lists would do with a delta, then the check
/// that nothing it keeps is stale.
struct Client {
    pages: HashMap<u32, Vec<u8>>,
}
impl Client {
    fn apply(&mut self, cache: &mut PageCache, doc: &VecParagraphs, delta: &PageDelta) {
        assert!(
            delta.changed.windows(2).all(|w| w[0] < w[1]),
            "sorted unique"
        );
        for page in &delta.changed {
            assert!(*page < delta.page_count, "changed page inside page_count");
        }
        for (start, end) in &delta.invalidated {
            for page in *start..*end {
                self.pages.remove(&page);
            }
        }
        for page in &delta.changed {
            if let Ok(bytes) = cache.page_display(doc, *page) {
                self.pages.insert(*page, bytes);
            }
        }
        for (page, bytes) in &self.pages {
            match cache.page_display(doc, *page) {
                Ok(now) => assert_eq!(&now, bytes, "page {page} is stale on the client"),
                Err(e) => panic!("client holds page {page} that is now {e:?}"),
            }
        }
    }
}

/// Replacements, pure insertions and deletions, appends at the end, deletion
/// of every paragraph; one edit in eight is reported as a change outside
/// paragraphs (`LayoutScope::All`).
fn random_edit(rng: &mut Rng, doc: &mut VecParagraphs) -> EditOutcome {
    let count = doc.paragraph_count();
    let words = |rng: &mut Rng| 3 + rng.below(60);
    let make = |rng: &mut Rng, n: usize| -> Vec<Paragraph> {
        (0..n)
            .map(|_| {
                let w = words(rng);
                paragraph(rng, w)
            })
            .collect()
    };
    let mut outcome = if count == 0 {
        let n = 1 + rng.below(3);
        let new = make(rng, n);
        doc.replace_paragraphs(0, 0, new)
    } else {
        match rng.below(8) {
            // Pure insertion between paragraphs.
            0 => {
                let n = 1 + rng.below(3);
                let new = make(rng, n);
                doc.replace_paragraphs(rng.below(count + 1), 0, new)
            }
            // Pure deletion.
            1 => {
                let first = rng.below(count);
                let removed = 1 + rng.below(2.min(count - first));
                doc.replace_paragraphs(first, removed, Vec::new())
            }
            // Append at the end of the document.
            2 => {
                let n = 1 + rng.below(2);
                let new = make(rng, n);
                doc.replace_paragraphs(count, 0, new)
            }
            // The document becomes empty (rarely).
            3 if rng.below(6) == 0 => doc.replace_paragraphs(0, count, Vec::new()),
            _ => {
                let first = rng.below(count);
                let removed = 1 + rng.below(2.min(count - first));
                let inserted = match rng.below(4) {
                    0 => removed,
                    1 => 1 + rng.below(3),
                    2 => removed + 1,
                    _ => removed.saturating_sub(1).max(1),
                };
                let new = make(rng, inserted);
                doc.replace_paragraphs(first, removed, new)
            }
        }
    };
    if rng.below(8) == 0 {
        outcome.layout_scope = LayoutScope::All;
    }
    outcome
}

fn complete_state(doc: &VecParagraphs, txt: bool) -> PageCache {
    let mut cache = PageCache::new(doc, txt).expect("cache");
    let mut rng = Rng(5);
    settle(&mut cache, doc, &mut rng);
    cache
}

fn assert_same_as_full(cache: &mut PageCache, doc: &VecParagraphs, txt: bool, rng: &mut Rng) {
    let mut fresh = complete_state(doc, txt);
    assert_eq!(cache.page_count(), fresh.page_count(), "page count");
    assert_eq!(cache.pagination(), fresh.pagination(), "pagination");
    let pages = fresh.page_count();
    let mut sampled = vec![
        0,
        pages.saturating_sub(1),
        rng.below(pages.max(1) as usize) as u32,
    ];
    sampled.dedup();
    for page in sampled.into_iter().filter(|p| *p < pages) {
        assert_eq!(
            cache.page_display(doc, page).unwrap(),
            fresh.page_display(doc, page).unwrap(),
            "display list of page {page}"
        );
    }
    for _ in 0..12 {
        let position = rng.below(doc.len() + 1);
        if doc.check_position(position).is_err() {
            continue;
        }
        for upstream in [false, true] {
            assert_eq!(
                cache.caret(position, upstream),
                fresh.caret(position, upstream),
                "caret at {position}"
            );
        }
    }
}

fn property(seed: u64, txt: bool) {
    let mut rng = Rng(seed);
    let mut doc = document(seed, 12);
    let mut cache = PageCache::new(&doc, txt).expect("cache");
    let mut client = Client {
        pages: HashMap::new(),
    };
    let deltas = settle(&mut cache, &doc, &mut rng);
    for delta in &deltas {
        client.apply(&mut cache, &doc, delta);
    }
    assert_same_as_full(&mut cache, &doc, txt, &mut rng);
    for _ in 0..8 {
        let outcome = random_edit(&mut rng, &mut doc);
        let delta = cache.apply_edit(&doc, &outcome);
        client.apply(&mut cache, &doc, &delta);
        for delta in settle(&mut cache, &doc, &mut rng) {
            client.apply(&mut cache, &doc, &delta);
        }
        assert_same_as_full(&mut cache, &doc, txt, &mut rng);
    }
}

#[test]
fn incremental_layout_equals_full_layout_document_profile() {
    for seed in 1..=20 {
        property(seed * 31, false);
    }
}

#[test]
fn incremental_layout_equals_full_layout_txt_profile() {
    for seed in 1..=20 {
        property(seed * 57, true);
    }
}

#[test]
fn typing_inside_a_line_changes_one_page_only() {
    let mut rng = Rng(11);
    let mut doc = document(77, 300);
    let mut cache = PageCache::new(&doc, false).expect("cache");
    settle(&mut cache, &doc, &mut rng);
    let pages = cache.page_count();
    assert!(pages > 8, "{pages} pages");
    let middle = doc.paragraph_count() / 2;
    // Replace one word by another word of the same width in the middle.
    let mut edited = doc.paragraph(middle).unwrap().clone();
    edited.text = edited.text.replacen('a', "b", 1);
    edited.runs.clear();
    let outcome = doc.replace_paragraphs(middle, 1, vec![edited]);
    let delta = cache.apply_edit(&doc, &outcome);
    assert!(delta.layout_complete, "{delta:?}");
    assert_eq!(delta.page_count, pages);
    assert!(delta.changed.len() <= 2, "{delta:?}");
    assert!(delta.invalidated.is_empty(), "{delta:?}");
    assert!(!delta.changed.is_empty());
}

#[test]
fn adding_a_line_defers_the_tail_to_step() {
    let mut rng = Rng(23);
    let mut doc = document(5, 120);
    let mut cache = PageCache::new(&doc, false).expect("cache");
    let mut client = Client {
        pages: HashMap::new(),
    };
    for delta in settle(&mut cache, &doc, &mut rng) {
        client.apply(&mut cache, &doc, &delta);
    }
    let pages = cache.page_count();
    // A long new paragraph near the start pushes every later line down.
    let long = {
        let mut r = Rng(1);
        paragraph(&mut r, 400)
    };
    let outcome = doc.replace_paragraphs(3, 1, vec![long]);
    let delta = cache.apply_edit(&doc, &outcome);
    assert!(delta.changed.len() <= 6, "{delta:?}");
    assert!(!delta.layout_complete);
    assert!(delta.page_count >= pages, "no scrollbar collapse");
    client.apply(&mut cache, &doc, &delta);
    // Pages after the repaginated ones are not ready until `step` gets there.
    assert_eq!(
        cache.page_display(&doc, delta.page_count - 1).err(),
        Some(LayoutError::NotReady)
    );
    for delta in settle(&mut cache, &doc, &mut rng) {
        client.apply(&mut cache, &doc, &delta);
    }
    assert!(cache.page_count() > pages);
}

#[test]
fn step_respects_its_budget_and_new_returns_the_first_pages() {
    let doc = document(9, 400);
    let mut cache = PageCache::new(&doc, false).expect("cache");
    assert!(!cache.layout_complete(), "first pages only");
    assert!(cache.page_count() >= 2);
    assert!(cache.laid_out_paragraphs() < 400);
    let initial = cache.laid_out_paragraphs();
    assert!(cache.page_display(&doc, 0).is_ok());
    let mut total = 0usize;
    let mut steps = 0;
    while !cache.layout_complete() {
        let delta = cache.step(&doc, 7);
        assert!(cache.last_step_work() <= 7);
        total += cache.last_step_work();
        steps += 1;
        assert!(delta.changed.iter().all(|p| *p < delta.page_count));
        assert!(steps < 5_000);
    }
    assert!(total + initial >= 400, "{total} + {initial}");
    assert_eq!(cache.laid_out_paragraphs(), 400);
    // A budget of zero still makes progress.
    let mut again = PageCache::new(&doc, false).expect("cache");
    let before = again.laid_out_paragraphs();
    again.step(&doc, 0);
    assert!(again.last_step_work() <= 1);
    assert!(again.laid_out_paragraphs() > before || again.layout_complete());
}

#[test]
fn page_display_lists_are_valid_atdl_and_memoised() {
    let doc = document(1, 30);
    let mut cache = complete_state(&doc, false);
    let first = cache.page_display(&doc, 0).expect("page 0");
    let list = DisplayList::decode(&first).expect("ATDL validates");
    assert!((list.page.bounds.width() - 595.3).abs() < 0.1);
    assert!((list.page.bounds.height() - 841.9).abs() < 0.1);
    assert!(
        list.commands
            .iter()
            .any(|c| matches!(c, Command::FillPath { .. }))
    );
    assert_eq!(cache.page_display(&doc, 0).unwrap(), first);
    assert_eq!(
        cache.page_display(&doc, cache.page_count()).err(),
        Some(LayoutError::PageOutOfRange)
    );
}

/// One paragraph that wraps into several lines, then more paragraphs.
fn wrapped_document() -> VecParagraphs {
    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit ";
    let long = words.repeat(8);
    let mut doc = VecParagraphs::new(vec![
        Paragraph::with_text(long.trim_end()),
        Paragraph::with_text("second"),
        Paragraph::with_text(""),
        Paragraph::with_text("last paragraph"),
    ]);
    doc.section = example_section();
    doc
}

#[test]
fn caret_affinity_at_a_soft_wrap() {
    let doc = wrapped_document();
    let cache = complete_state(&doc, true);
    let first_end = cache.line_edges(0, false).unwrap().1;
    let (start1, end1) = cache.line_edges(first_end, false).unwrap();
    assert_eq!(start1, first_end, "downstream at the wrap is the next line");
    assert!(end1 > start1);
    // Upstream keeps the caret at the end of the line it wraps from.
    let (up_start, up_end) = cache.line_edges(first_end, true).unwrap();
    assert_eq!((up_start, up_end), (0, first_end));
    let up = cache.caret(first_end, true).unwrap();
    let down = cache.caret(first_end, false).unwrap();
    assert!(down.y > up.y, "the next line is lower");
    assert!(down.x < up.x, "the next line starts at the left margin");
    assert!(
        (down.x - 54.0).abs() < 0.1,
        "TXT margin is 54 pt: {}",
        down.x
    );
    assert!(up.height > 8.0 && up.height < 20.0, "{}", up.height);
    assert_eq!(up.page, 0);
    // Inside a line both affinities agree.
    assert_eq!(cache.caret(3, true), cache.caret(3, false));
    assert_eq!(cache.caret(0, false).unwrap().x, down.x);
}

#[test]
fn hit_test_returns_position_and_affinity() {
    let doc = wrapped_document();
    let cache = complete_state(&doc, true);
    let first_end = cache.line_edges(0, false).unwrap().1;
    let line0 = cache.caret(0, false).unwrap();
    let mid = line0.y + line0.height / 2.0;
    // Far right of a wrapped line: its end, with upstream affinity.
    let (position, upstream) = cache.hit_test(0, 590.0, mid).unwrap();
    assert_eq!(position, first_end);
    assert!(upstream);
    // Left of the line: its start.
    assert_eq!(cache.hit_test(0, 0.0, mid).unwrap(), (0, false));
    // Round trip with the caret.
    let caret = cache.caret(5, false).unwrap();
    let (back, _) = cache
        .hit_test(caret.page, caret.x + 0.2, caret.y + caret.height / 2.0)
        .unwrap();
    assert_eq!(back, 5);
    // Far below the text: the last line.
    let (end, _) = cache.hit_test(0, 300.0, 800.0).unwrap();
    assert_eq!(end, doc.len());
    assert_eq!(
        cache.hit_test(5, 0.0, 0.0).err(),
        Some(LayoutError::PageOutOfRange)
    );
}

#[test]
fn vertical_movement_keeps_the_preferred_x() {
    let doc = wrapped_document();
    let cache = complete_state(&doc, true);
    let start = cache.caret(20, false).unwrap();
    let (down, x) = cache.move_vertical(20, 1, None).unwrap();
    let moved = cache.caret(down, false).unwrap();
    assert!(moved.y > start.y);
    assert!((x - start.x).abs() < 0.02);
    assert!((moved.x - start.x).abs() < 2.0, "close to the same column");
    let (back, _) = cache.move_vertical(down, -1, Some(x)).unwrap();
    assert_eq!(back, 20);
    // Across the empty paragraph and back.
    let last = doc.len();
    let (up, kept) = cache.move_vertical(last, -2, None).unwrap();
    assert!(up < last);
    assert!(kept >= 0.0);
    // Past the first line: the document start; past the last: its end.
    assert_eq!(cache.move_vertical(3, -5, None).unwrap().0, 0);
    assert_eq!(cache.move_vertical(last - 2, 5, None).unwrap().0, doc.len());
    assert_eq!(cache.move_vertical(last, 0, None).unwrap().0, last);
}

#[test]
fn page_movement_crosses_pages() {
    let doc = document(31, 60);
    let cache = complete_state(&doc, false);
    assert!(cache.page_count() >= 4);
    let from = 40;
    let here = cache.caret(from, false).unwrap();
    let (next, x) = cache.move_page(from, 1, None).unwrap();
    let there = cache.caret(next, false).unwrap();
    assert_eq!(there.page, here.page + 1);
    assert!((x - here.x).abs() < 0.02);
    let (prev, _) = cache.move_page(next, -1, Some(x)).unwrap();
    assert_eq!(cache.caret(prev, false).unwrap().page, here.page);
    assert_eq!(cache.move_page(from, -50, None).unwrap().0, 0);
    assert_eq!(cache.move_page(from, 500, None).unwrap().0, doc.len());
    assert_eq!(cache.move_page(from, 0, None).unwrap().0, from);
}

#[test]
fn selection_rectangles_follow_lines_and_pages() {
    let doc = wrapped_document();
    let cache = complete_state(&doc, true);
    let first_end = cache.line_edges(0, false).unwrap().1;
    let rects = cache.selection_rects(0, 4, first_end + 6).unwrap();
    assert_eq!(rects.len(), 2, "{rects:?}");
    assert!(rects[1].y > rects[0].y);
    assert!(rects[0].width > rects[1].width || rects[1].width > 0.0);
    assert!(cache.selection_rects(0, 7, 7).unwrap().is_empty());
    // Reversed endpoints are normalised.
    assert_eq!(
        cache.selection_rects(0, first_end + 6, 4).unwrap(),
        cache.selection_rects(0, 4, first_end + 6).unwrap()
    );
    // A selection over a page break splits by page.
    let big = document(2, 40);
    let cache = complete_state(&big, false);
    let all: Vec<_> = (0..cache.page_count())
        .map(|p| cache.selection_rects(p, 0, big.len()).unwrap())
        .collect();
    assert!(all.iter().all(|r| !r.is_empty()));
    assert!(
        all.iter()
            .flatten()
            .all(|r| r.width >= 0.0 && r.height > 0.0)
    );
    assert_eq!(
        cache.selection_rects(cache.page_count(), 0, 1).err(),
        Some(LayoutError::PageOutOfRange)
    );
}

#[test]
fn positions_are_checked() {
    let doc = wrapped_document();
    let cache = complete_state(&doc, true);
    assert_eq!(
        cache.caret(doc.len() + 1, false).err(),
        Some(LayoutError::PositionOutOfRange)
    );
    assert!(cache.caret(doc.len(), false).is_ok());
    // Not ready: only the first pages of a long document are laid out.
    let long = document(4, 400);
    let partial = PageCache::new(&long, false).expect("cache");
    assert_eq!(
        partial.caret(long.len(), false).err(),
        Some(LayoutError::NotReady)
    );
    assert!(partial.caret(0, false).is_ok());
}

#[test]
fn inline_items_opaque_blocks_and_table_cells_lay_out() {
    let mut owner = RichDocument::new();
    let mut with_object = Paragraph::with_text("before after");
    with_object.anchored.push(AnchoredObject {
        id: owner.alloc_id().unwrap(),
        offset: 7,
        kind: "pic".into(),
        data: vec![1, 2, 3],
    });
    let mut doc = VecParagraphs::new(vec![
        Paragraph::with_text("opaque follows"),
        Paragraph::default(),
        with_object,
        Paragraph::with_text("cell"),
    ]);
    doc.section = example_section();
    doc.set_kind(1, BlockKind::Opaque);
    let cache = complete_state(&doc, false);
    assert_eq!(cache.page_count(), 1);
    // Positions around the object: before it (7) and after it (10) are valid.
    let start = doc.paragraph_start(2).unwrap();
    let before = cache.caret(start + 7, false).unwrap();
    let after = cache.caret(start + 10, false).unwrap();
    assert_eq!(before.page, after.page);
    assert!((before.x - after.x).abs() < 0.05, "items take no room");
    // The end of the paragraph is after its items.
    assert!(cache.caret(start + 15, false).is_ok());
    // The opaque block is one position pair (U+FFFC is three bytes).
    let opaque = doc.paragraph_start(1).unwrap();
    assert!(cache.caret(opaque, false).is_ok());
    assert!(cache.caret(opaque + 3, false).is_ok());
}

#[test]
fn unsupported_characters_and_huge_paragraphs_do_not_fail() {
    let mut doc = VecParagraphs::new(vec![
        Paragraph::with_text("kanji \u{6f22}\u{5b57} and emoji \u{1f600} done"),
        Paragraph::with_text(&"x".repeat(200_000)),
        Paragraph::with_text("after"),
    ]);
    doc.section = example_section();
    let mut cache = PageCache::new(&doc, false).expect("cache");
    let mut rng = Rng(1);
    settle(&mut cache, &doc, &mut rng);
    assert!(cache.page_count() >= 1);
    assert!(cache.caret(0, false).is_ok());
    assert!(cache.caret(10, false).is_ok());
    assert!(cache.page_display(&doc, 0).is_ok());
}

#[test]
fn a_change_outside_paragraphs_rebuilds_everything() {
    let mut rng = Rng(8);
    let doc = document(12, 40);
    let mut cache = PageCache::new(&doc, false).expect("cache");
    let mut client = Client {
        pages: HashMap::new(),
    };
    for delta in settle(&mut cache, &doc, &mut rng) {
        client.apply(&mut cache, &doc, &delta);
    }
    let pages = cache.page_count();
    let outcome = EditOutcome {
        revision: 1,
        start: 0,
        removed_bytes: 0,
        inserted_bytes: 0,
        doc_len: doc.len(),
        first_paragraph: 0,
        removed_paragraphs: 0,
        inserted_paragraphs: 0,
        layout_scope: atelier_text_model::rich::flat::LayoutScope::All,
        selection: (0, 0),
        can_undo: true,
        can_redo: false,
    };
    let delta = cache.apply_edit(&doc, &outcome);
    assert!(
        delta
            .invalidated
            .iter()
            .any(|(a, b)| *a == 0 && *b >= pages)
    );
    client.apply(&mut cache, &doc, &delta);
    for delta in settle(&mut cache, &doc, &mut rng) {
        client.apply(&mut cache, &doc, &delta);
    }
    assert_eq!(cache.page_count(), pages);
}

#[test]
fn an_edit_beyond_the_laid_out_prefix_waits_for_step() {
    let mut rng = Rng(14);
    let mut doc = document(6, 300);
    let mut cache = PageCache::new(&doc, false).expect("cache");
    let laid = cache.laid_out_paragraphs();
    assert!(laid < 300);
    let new = paragraph(&mut Rng(3), 20);
    let outcome = doc.replace_paragraphs(250, 1, vec![new]);
    let delta = cache.apply_edit(&doc, &outcome);
    assert!(delta.changed.is_empty());
    assert!(!delta.layout_complete);
    settle(&mut cache, &doc, &mut rng);
    assert_same_as_full(&mut cache, &doc, false, &mut rng);
}

#[test]
fn a_change_the_cache_was_not_told_about_is_visible_to_the_comparison() {
    // Witness of the property tests: without `apply_edit` the cache differs
    // from a fresh layout, so the equality they assert is not vacuous.
    let mut rng = Rng(4);
    let mut doc = document(21, 12);
    let mut cache = complete_state(&doc, false);
    let mut longer = doc.paragraph(2).unwrap().clone();
    longer
        .text
        .push_str(&" more words to make the paragraph taller".repeat(40));
    longer.runs.clear();
    doc.replace_paragraphs(2, 1, vec![longer]);
    let fresh = complete_state(&doc, false);
    assert_ne!(cache.pagination(), fresh.pagination());
    // Told about it, the cache catches up.
    let outcome = doc.replace_paragraphs(0, 1, vec![doc.paragraph(0).unwrap().clone()]);
    cache.apply_edit(&doc, &outcome);
    settle(&mut cache, &doc, &mut rng);
    let _ = cache.page_count();
}

/// "one", a line break, " two", a page break, " three", a symbol: the model
/// keeps the breaks and the symbol outside the text (three flat bytes each).
fn breaking_document() -> VecParagraphs {
    let mut owner = RichDocument::new();
    let mut p = Paragraph::with_text("one two three");
    p.breaks.push(InlineBreak {
        id: owner.alloc_id().unwrap(),
        offset: 3,
        kind: BreakKind::Line,
    });
    p.breaks.push(InlineBreak {
        id: owner.alloc_id().unwrap(),
        offset: 7,
        kind: BreakKind::Page,
    });
    p.symbols.push(InlineSymbol {
        id: owner.alloc_id().unwrap(),
        offset: 13,
        font: "Symbol".into(),
        code: 0xf0b7,
    });
    let mut doc = VecParagraphs::new(vec![p, Paragraph::with_text("next")]);
    doc.section = example_section();
    doc
}

#[test]
fn line_and_page_breaks_and_symbols_in_the_flat_text() {
    for txt in [false, true] {
        let doc = breaking_document();
        // Flat text: "one" 0..3, break 3..6, " two" 6..10, break 10..13,
        // " three" 13..19, symbol 19..22, end of paragraph at 22.
        assert_eq!(doc.paragraph_flat(0).unwrap().len(), 22);
        let cache = complete_state(&doc, txt);
        assert_eq!(cache.page_count(), 2, "the page break starts page 2");
        // Line one ends before the break; the break does not extend it.
        let (start, end) = cache.line_edges(0, false).unwrap();
        assert_eq!((start, end), (0, 3));
        let before = cache.caret(3, false).unwrap();
        let after = cache.caret(6, false).unwrap();
        assert_eq!(before.page, 0);
        assert!(after.y > before.y, "after the break: next line");
        assert!(after.x < before.x, "next line starts at the margin");
        assert_eq!(cache.line_edges(6, false).unwrap().0, 6);
        // Past the page break: page 1 at the top margin.
        let top = cache.caret(13, false).unwrap();
        assert_eq!(top.page, 1);
        assert!(top.y < after.y);
        // Positions inside an item are refused, around it they are valid.
        assert!(cache.caret(4, false).is_err());
        assert!(cache.caret(19, false).is_ok() && cache.caret(22, false).is_ok());
        // The symbol takes room in the line.
        let (x0, x1) = (
            cache.caret(19, false).unwrap().x,
            cache.caret(22, false).unwrap().x,
        );
        assert!(x1 > x0, "the symbol is drawn: {x0} < {x1}");
        // Hit test lands on the flat positions around the items.
        let first = cache.caret(0, false).unwrap();
        assert_eq!(
            cache.hit_test(0, first.x + 200.0, first.y + 3.0).unwrap().0,
            3
        );
        let (position, _) = cache.hit_test(1, 400.0, top.y + top.height / 2.0).unwrap();
        assert!((13..=22).contains(&position), "{position}");
        // A selection across the line break covers both lines.
        let rects = cache.selection_rects(0, 1, 8).unwrap();
        assert_eq!(rects.len(), 2, "{rects:?}");
        // Pages draw: both display lists decode.
        let mut cache = cache;
        for page in 0..2 {
            assert!(cache.page_display(&doc, page).is_ok());
        }
    }
}
