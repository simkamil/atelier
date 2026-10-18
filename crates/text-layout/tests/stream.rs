// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests of the incremental page stream (US-216): after any sequence
//! of paragraph replacements the stream holds exactly the pagination of the
//! whole document.

use atelier_text_layout::pagination::{PaginationOptions, ParagraphRules, paginate};
use atelier_text_layout::stream::{PageStream, RunLimit, StreamParagraph};
use atelier_text_layout::{LayoutLine, ParagraphLayout};

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

const LINE: i64 = 14 * 64;
const PAGE: i64 = 40 * LINE;

fn paragraph(rng: &mut Rng, rich_rules: bool) -> StreamParagraph {
    let lines = 1 + rng.below(9);
    let (before, after) = if rich_rules {
        (rng.below(3) as i64 * 64 * 6, rng.below(3) as i64 * 64 * 6)
    } else {
        (0, 0)
    };
    let mut layout_lines = Vec::new();
    let mut top = before;
    for i in 0..lines {
        let height = if rich_rules && rng.below(7) == 0 {
            LINE + 64 * 6
        } else {
            LINE
        };
        layout_lines.push(LayoutLine {
            source: i..i + 1,
            x: 0,
            baseline: top + height - 3 * 64,
            width: 0,
            height,
            mandatory_break: false,
            final_line: i + 1 == lines,
            justified: false,
            fragments: Vec::new(),
        });
        top += height;
    }
    let layout = ParagraphLayout {
        width: 0,
        height: top + after,
        lines: layout_lines,
        unsupported: Vec::new(),
    };
    let mut rules = ParagraphRules {
        space_before: before,
        space_after: after,
        ..ParagraphRules::default()
    };
    if rich_rules {
        rules.keep_with_next = rng.below(5) == 0;
        rules.keep_together = rng.below(8) == 0;
        rules.break_before = rng.below(40) == 0;
        rules.contextual_spacing = rng.below(4) == 0;
        rules.style_id = rng.below(2) as u64;
        rules.widow_min = 1 + rng.below(2);
        rules.orphan_min = 1 + rng.below(2);
    }
    StreamParagraph { layout, rules }
}

fn options() -> PaginationOptions {
    PaginationOptions {
        page_height: PAGE,
        max_pages: 10_000,
        max_work: usize::MAX / 4,
    }
}

fn full(mirror: &[StreamParagraph]) -> atelier_text_layout::pagination::Pagination {
    let layouts: Vec<_> = mirror.iter().map(|p| p.layout.clone()).collect();
    let rules: Vec<_> = mirror.iter().map(|p| p.rules.clone()).collect();
    paginate(&layouts, &rules, options()).expect("full pagination")
}

fn run(seed: u64, paragraphs: usize, edits: usize, rich_rules: bool) -> (usize, usize) {
    let mut rng = Rng(seed);
    let mut stream = PageStream::new(options());
    let mut mirror: Vec<StreamParagraph> = Vec::new();
    // Build the document by appends of random size, as the tail layout does.
    while mirror.len() < paragraphs {
        let n = (1 + rng.below(40)).min(paragraphs - mirror.len());
        let batch: Vec<_> = (0..n).map(|_| paragraph(&mut rng, rich_rules)).collect();
        let at = mirror.len();
        mirror.extend(batch.iter().cloned());
        stream
            .replace(at, 0, batch, RunLimit::Pages(3))
            .expect("append");
        settle(&mut stream, &mirror, &mut rng);
        assert_eq!(*stream.pagination(), full(&mirror), "after append at {at}");
    }
    let (mut early, mut total, mut gave_up) = (0, 0, 0);
    for step in 0..edits {
        let first = rng.below(mirror.len());
        let removed = 1 + rng.below(3.min(mirror.len() - first));
        let inserted = match rng.below(4) {
            0 => removed,
            1 => 1 + rng.below(4),
            2 => removed + 1,
            _ => removed.saturating_sub(1).max(1),
        };
        let batch: Vec<_> = (0..inserted)
            .map(|_| paragraph(&mut rng, rich_rules))
            .collect();
        mirror.splice(first..first + removed, batch.iter().cloned());
        let update = stream
            .replace(first, removed, batch, RunLimit::Pages(2))
            .expect("replace");
        assert_eq!(update.page_count, stream.page_count());
        check_prefix(&stream, &mirror);
        gave_up += usize::from(update.stats.gave_up);
        assert_eq!(
            update.complete,
            stream.paginated_paragraphs() == mirror.len()
        );
        settle(&mut stream, &mirror, &mut rng);
        assert_eq!(
            *stream.pagination(),
            full(&mirror),
            "step {step}: replace {removed} at {first} by {inserted}"
        );
        assert_eq!(stream.paragraph_count(), mirror.len());
        total += 1;
        early += usize::from(update.stats.stopped_early);
        for page in &update.changed_pages {
            assert!(*page < update.page_count);
        }
    }
    assert!(gave_up <= total);
    (early, total)
}

/// The placements known so far are the first placements of the full pagination.
fn check_prefix(stream: &PageStream, mirror: &[StreamParagraph]) {
    let all = full(mirror);
    let known = &stream.pagination().placements;
    assert!(known.len() <= all.placements.len());
    if let Some(i) = (0..known.len()).find(|&i| known[i] != all.placements[i]) {
        panic!(
            "prefix differs at {i}: {:?} vs {:?}; stream pages {} paginated {} of {}; around: {:?} / {:?}",
            known[i],
            all.placements[i],
            stream.page_count(),
            stream.paginated_paragraphs(),
            mirror.len(),
            &known[i.saturating_sub(2)..(i + 2).min(known.len())],
            &all.placements[i.saturating_sub(2)..(i + 2).min(known.len())]
        );
    }
    assert_eq!(stream.page_count(), stream.pagination().pages.len());
}

/// Paginates the pending tail in slices of random size.
fn settle(stream: &mut PageStream, mirror: &[StreamParagraph], rng: &mut Rng) {
    while stream.paginated_paragraphs() < mirror.len() {
        let before = stream.paginated_paragraphs();
        stream.advance(1 + rng.below(30)).expect("advance");
        assert!(stream.paginated_paragraphs() > before, "advance progresses");
        check_prefix(stream, mirror);
    }
}

#[test]
fn incremental_pagination_equals_full_pagination_plain_rules() {
    for seed in 1..=40u64 {
        run(seed * 7919, 120, 60, false);
    }
}

#[test]
fn incremental_pagination_equals_full_pagination_rich_rules() {
    for seed in 1..=40u64 {
        run(seed * 104_729, 150, 60, true);
    }
}

#[test]
fn typing_in_the_middle_stops_on_a_stabilised_page() {
    let mut rng = Rng(42);
    let mut stream = PageStream::new(options());
    let batch: Vec<_> = (0..1500).map(|_| paragraph(&mut rng, false)).collect();
    stream
        .replace(0, 0, batch, RunLimit::Pages(3))
        .expect("build");
    let pages = stream.page_count();
    assert!(pages > 100, "{pages} pages");
    // Same geometry: an edit that changes no line count.
    let middle = 750;
    let lines = stream.layout(middle).expect("paragraph").lines.len();
    let mut same = paragraph(&mut rng, false);
    while same.layout.lines.len() != lines {
        same = paragraph(&mut rng, false);
    }
    let update = stream
        .replace(middle, 1, vec![same], RunLimit::Pages(2))
        .expect("edit");
    assert!(update.stats.stopped_early);
    assert!(update.stats.pages_recalculated <= 2, "{:?}", update.stats);
    assert_eq!(
        update.stats.pages_reused + update.stats.pages_recalculated,
        pages
    );
    assert!(
        update.changed_pages.len() <= 2,
        "{:?}",
        update.changed_pages
    );
    // Splitting a paragraph shifts every later index but stays local.
    let a = paragraph(&mut rng, false);
    let b = paragraph(&mut rng, false);
    let update = stream
        .replace(middle, 1, vec![a, b], RunLimit::Pages(2))
        .expect("split");
    // Adding lines pushes every later line down: the run gives up after two
    // pages and the tail is paginated by `advance`.
    assert!(update.stats.pages_recalculated <= 4, "{:?}", update.stats);
    assert!(update.stats.stopped_early || update.stats.gave_up);
    assert_eq!(update.complete, stream.paginated_paragraphs() == 1501);
}

#[test]
fn a_failed_replacement_leaves_the_stream_untouched() {
    let mut rng = Rng(7);
    let mut stream = PageStream::new(options());
    stream
        .replace(
            0,
            0,
            (0..30).map(|_| paragraph(&mut rng, false)).collect(),
            RunLimit::Pages(3),
        )
        .expect("build");
    let before = stream.pagination().clone();
    let mut bad = paragraph(&mut rng, false);
    bad.layout.height += 1;
    assert!(stream.replace(3, 1, vec![bad], RunLimit::Pages(2)).is_err());
    assert!(
        stream
            .replace(40, 1, Vec::new(), RunLimit::Pages(2))
            .is_err()
    );
    assert_eq!(*stream.pagination(), before);
    assert_eq!(stream.paragraph_count(), 30);
}
