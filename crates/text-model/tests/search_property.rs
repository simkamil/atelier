// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests of the text search (US-340).
//!
//! A fixed-seed generator draws small texts and queries from a tricky
//! alphabet (sharp s, ligature, composed and combining accents, dotted I,
//! no-break spaces, apostrophe, hyphen, emoji, ZWJ, line breaks) and random
//! options. For each case: (1) every range is on grapheme boundaries and
//! inside its paragraph, (2) the folded text of a range equals the folded
//! query, (3) the result equals that of an obvious quadratic reference written
//! here, (4) next/previous walk exactly the complete list.

use atelier_text_model::search::{Position, SearchOptions, Searcher, TextLines};
use atelier_unicode::case::to_case_folded;
use atelier_unicode::normalization::{nfc, nfd};
use atelier_unicode::properties::general_category;
use atelier_unicode::segmentation::{grapheme_boundaries, word_boundaries};
use std::ops::Range;

const CASES: u64 = 30_000;

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
        (self.next() % n as u64) as usize
    }
}

const ALPHABET: &[&str] = &[
    "a",
    "A",
    "e",
    "E",
    "\u{e9}",
    "e\u{301}",
    "\u{301}",
    "\u{e8}",
    "s",
    "S",
    "\u{df}",
    "ss",
    "SS",
    "f",
    "i",
    "\u{fb01}",
    "\u{130}",
    "i\u{307}",
    " ",
    " ",
    "\u{a0}",
    "\u{202f}",
    "'",
    "-",
    "x",
    "1",
    "\u{1f600}",
    "\u{1f468}",
    "\u{200d}",
    "\u{1f1eb}",
    "\u{1f1f7}",
    "\n",
];

fn draw(rng: &mut Rng, max: usize, newlines: bool) -> String {
    let mut s = String::new();
    for _ in 0..rng.below(max + 1) {
        let piece = ALPHABET[rng.below(ALPHABET.len())];
        if piece == "\n" && !newlines {
            continue;
        }
        s.push_str(piece);
    }
    s
}

/// Reference folding of a whole string, written independently of the module.
fn fold(s: &str, o: &SearchOptions) -> String {
    let mut s = nfd(s);
    if !o.match_case {
        s = nfd(&to_case_folded(&s));
    }
    s.chars()
        .filter(|&c| !(o.ignore_accents && general_category(c) == "Mn"))
        .map(|c| {
            if !o.match_nbsp && (c == '\u{a0}' || c == '\u{202f}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Obvious reference: leftmost start, shortest end, then no overlap.
fn naive(line: &str, query: &str, o: &SearchOptions) -> Vec<Range<usize>> {
    let qkey = fold(&nfc(query), o);
    let mut out = Vec::new();
    if qkey.is_empty() {
        return out;
    }
    let index = grapheme_boundaries(line);
    let b = index.boundaries();
    let words = word_boundaries(line);
    let mut resume = 0;
    for si in 0..b.len() - 1 {
        if b[si] < resume || fold(&line[b[si]..b[si + 1]], o).is_empty() {
            continue;
        }
        let Some(ej) = (si + 1..b.len()).find(|&ej| fold(&line[b[si]..b[ej]], o) == qkey) else {
            continue;
        };
        let range = b[si]..b[ej];
        if o.whole_word
            && !(words.is_boundary(range.start).unwrap() && words.is_boundary(range.end).unwrap())
        {
            continue;
        }
        resume = range.end;
        out.push(range);
    }
    out
}

fn options(rng: &mut Rng) -> SearchOptions {
    SearchOptions {
        match_case: rng.below(2) == 0,
        whole_word: rng.below(2) == 0,
        ignore_accents: rng.below(2) == 0,
        match_nbsp: rng.below(2) == 0,
        context_graphemes: rng.below(4),
    }
}

#[test]
fn search_matches_the_reference() {
    let mut total = 0usize;
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let text = draw(&mut rng, 14, true);
        // Half of the queries are cut out of the text so that hits are common.
        let query = if rng.below(2) == 0 && !text.is_empty() {
            let chars: Vec<(usize, char)> = text.char_indices().collect();
            let a = rng.below(chars.len());
            let z = (a + 1 + rng.below(4)).min(chars.len());
            let start = chars[a].0;
            let end = chars.get(z).map_or(text.len(), |c| c.0);
            text[start..end].to_owned()
        } else {
            draw(&mut rng, 3, false)
        };
        let o = options(&mut rng);
        let lines = TextLines(&text);
        let searcher = Searcher::new(&lines, &query, o);
        let all = searcher.find_all(usize::MAX);
        assert!(!all.truncated);

        let mut expected = Vec::new();
        for (pi, p) in atelier_text_model::search::ParagraphSource::paragraphs(&lines)
            .iter()
            .enumerate()
        {
            for r in naive(p.text, &query, &o) {
                expected.push((pi, r));
            }
        }
        let got: Vec<(usize, Range<usize>)> = all
            .items
            .iter()
            .map(|x| (x.paragraph_index, x.range.clone()))
            .collect();
        let ctx = format!("seed {seed}: text {text:?} query {query:?} {o:?}");
        assert_eq!(got, expected, "(3) {ctx}");
        assert_eq!(searcher.count(usize::MAX).0, got.len(), "{ctx}");
        total += got.len();

        let qkey = fold(&nfc(&query), &o);
        for x in &all.items {
            let para = atelier_text_model::search::ParagraphSource::paragraphs(&lines)
                [x.paragraph_index]
                .text;
            // (1)
            let g = grapheme_boundaries(para);
            assert!(
                x.range.start < x.range.end && x.range.end <= para.len(),
                "{ctx}"
            );
            assert!(g.is_boundary(x.range.start).unwrap(), "(1) {ctx}");
            assert!(g.is_boundary(x.range.end).unwrap(), "(1) {ctx}");
            // (2)
            assert_eq!(fold(&para[x.range.clone()], &o), qkey, "(2) {ctx}");
            // Excerpt
            if let Some(e) = &x.excerpt {
                assert_eq!(&e.text[e.start..e.end], &para[x.range.clone()], "{ctx}");
                assert!(para.contains(e.text.as_str()), "{ctx}");
            } else {
                assert_eq!(o.context_graphemes, 0, "{ctx}");
            }
        }

        // (4) forward and backward walks over the list.
        let n = all.len();
        if n == 0 {
            let start = lines.position(0);
            assert!(searcher.next_from(&start).is_none(), "{ctx}");
            assert!(searcher.previous_before(&start).is_none(), "{ctx}");
            continue;
        }
        let mut pos = lines.position(0);
        for k in 0..n {
            let step = searcher.next_from(&pos).unwrap();
            assert_eq!(step.occurrence, all.items[k], "(4) next {k} {ctx}");
            assert!(!step.wrapped, "{ctx}");
            assert_eq!(all.index_of(&step.occurrence), Some(k), "{ctx}");
            pos = Position {
                paragraph: step.occurrence.paragraph,
                offset: step.occurrence.range.end,
            };
        }
        let wrap = searcher.next_from(&pos).unwrap();
        assert!(wrap.wrapped, "{ctx}");
        assert_eq!(wrap.occurrence, all.items[0], "{ctx}");

        let last = &all.items[n - 1];
        let mut pos = Position {
            paragraph: last.paragraph,
            offset: last.range.end,
        };
        for k in (0..n).rev() {
            let step = searcher.previous_before(&pos).unwrap();
            assert_eq!(step.occurrence, all.items[k], "(4) prev {k} {ctx}");
            assert!(!step.wrapped, "{ctx}");
            pos = Position {
                paragraph: step.occurrence.paragraph,
                offset: step.occurrence.range.start,
            };
        }
        let wrap = searcher.previous_before(&pos).unwrap();
        assert!(wrap.wrapped, "{ctx}");
        assert_eq!(wrap.occurrence, all.items[n - 1], "{ctx}");
    }
    // The generator must actually produce hits, or the test proves nothing.
    assert!(total > 5_000, "only {total} occurrences generated");
    println!("{CASES} cases, {total} occurrences");
}
