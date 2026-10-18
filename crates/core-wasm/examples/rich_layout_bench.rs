// SPDX-License-Identifier: AGPL-3.0-or-later
//! Host bench of the incremental page cache (US-216): one keystroke in the
//! middle of a document of about 300 pages, `apply_edit` plus the display
//! lists of the changed pages. Run in release:
//! `cargo run --release --locked --offline -p atelier-core-wasm --example rich_layout_bench`
//! (the model double rebuilds its index outside the timed section).

#[cfg(not(target_arch = "wasm32"))]
mod host {
    use atelier_core_wasm::rich_layout::{PageCache, RichPageCache};
    use atelier_text_model::rich::Paragraph;
    use atelier_text_model::rich::flat::{FlatIndex, LayoutContext};
    use atelier_text_model::rich::flat_double::{VecParagraphs, example_section};
    use std::time::Instant;

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
        "document",
        "editor",
        "layout",
        "page",
    ];

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

    fn paragraph(rng: &mut Rng) -> Paragraph {
        let words = 20 + rng.below(80);
        let text: Vec<&str> = (0..words).map(|_| WORDS[rng.below(WORDS.len())]).collect();
        Paragraph::with_text(&text.join(" "))
    }

    fn stats(label: &str, mut micros: Vec<f64>) {
        micros.sort_by(|a, b| a.total_cmp(b));
        let n = micros.len().max(1);
        let mean = micros.iter().sum::<f64>() / n as f64;
        let at = |q: f64| {
            micros
                .get(((n as f64 * q) as usize).min(n - 1))
                .copied()
                .unwrap_or(0.0)
        };
        println!(
            "{label}: n={} mean={:.2} ms p50={:.2} ms p95={:.2} ms max={:.2} ms",
            micros.len(),
            mean / 1000.0,
            at(0.5) / 1000.0,
            at(0.95) / 1000.0,
            micros.last().copied().unwrap_or(0.0) / 1000.0
        );
    }

    pub fn main() {
        let mut rng = Rng(0x5eed);
        let target_pages: u32 = std::env::args()
            .nth(1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(300);
        // Grow the document until it has the wanted number of pages.
        let mut list = Vec::new();
        let mut doc;
        let mut cache;
        let started = Instant::now();
        loop {
            for _ in 0..(if list.is_empty() {
                target_pages as usize * 11
            } else {
                100
            }) {
                list.push(paragraph(&mut rng));
            }
            doc = VecParagraphs::new(list.clone());
            doc.section = example_section();
            cache = PageCache::new(&doc, false).expect("cache");
            while !cache.layout_complete() {
                cache.step(&doc, 256);
            }
            if cache.page_count() >= target_pages {
                break;
            }
        }
        println!(
            "document: {} paragraphs, {} pages, built in {:.2} s",
            doc.paragraph_count(),
            cache.page_count(),
            started.elapsed().as_secs_f64()
        );
        let middle = doc.paragraph_count() / 2;

        // Typing: one character at a time into the middle paragraph.
        let mut text = doc.paragraph_flat(middle).unwrap().into_owned();
        let (mut all, mut stable, mut moved) = (Vec::new(), Vec::new(), Vec::new());
        let (mut pages_visited, mut pages_changed, mut keystrokes) = (0usize, 0usize, 0usize);
        let mut max_visited = 0usize;
        let (mut edit_only, mut paint_only) = (Vec::new(), Vec::new());
        let words_typed = "the quick brown fox jumps over the lazy dog ";
        for key in words_typed.chars().cycle().take(400) {
            let at = text.char_indices().nth(30).map_or(text.len(), |(i, _)| i);
            text.insert(at, key);
            let outcome = doc.replace_paragraphs(middle, 1, vec![Paragraph::with_text(&text)]);
            let t = Instant::now();
            let delta = cache.apply_edit(&doc, &outcome);
            let t_edit = t.elapsed().as_secs_f64() * 1e6;
            for page in &delta.changed {
                let _ = cache.page_display(&doc, *page).expect("display list");
            }
            let t_paint = t.elapsed().as_secs_f64() * 1e6 - t_edit;
            let _ = cache.caret(outcome.selection.0.min(doc.len()), false);
            let micros = t.elapsed().as_secs_f64() * 1e6;
            edit_only.push(t_edit);
            paint_only.push(t_paint);
            all.push(micros);
            if cache.last_stats().gave_up {
                moved.push(micros);
            } else {
                stable.push(micros);
            }
            let visited = cache.last_stats().pages_recalculated;
            pages_visited += visited;
            max_visited = max_visited.max(visited);
            pages_changed += delta.changed.len();
            keystrokes += 1;
            // The pending tail is paginated between keystrokes, untimed here.
            while !cache.layout_complete() {
                cache.step(&doc, 512);
            }
        }
        stats("keystroke (apply_edit + changed page lists + caret)", all);
        stats("  of which apply_edit (layout + pagination)", edit_only);
        stats("  of which page display lists", paint_only);
        stats("  flow unchanged (stopped on a stabilised page)", stable);
        stats("  line added or removed (tail left to step)", moved);
        println!(
            "pages visited by the pagination run: mean {:.2}, max {}; pages re-sent per keystroke: mean {:.2}",
            pages_visited as f64 / keystrokes as f64,
            max_visited,
            pages_changed as f64 / keystrokes as f64
        );

        // Enter in the middle of the paragraph (split into two).
        let mut enter = Vec::new();
        for _ in 0..30 {
            let current = doc.paragraph_flat(middle).unwrap().into_owned();
            let cut = current
                .char_indices()
                .nth(current.chars().count() / 2)
                .map_or(0, |(i, _)| i);
            let (a, b) = current.split_at(cut);
            let outcome = doc.replace_paragraphs(
                middle,
                1,
                vec![Paragraph::with_text(a), Paragraph::with_text(b)],
            );
            let t = Instant::now();
            let delta = cache.apply_edit(&doc, &outcome);
            for page in &delta.changed {
                let _ = cache.page_display(&doc, *page);
            }
            enter.push(t.elapsed().as_secs_f64() * 1e6);
            while !cache.layout_complete() {
                cache.step(&doc, 512);
            }
            // Merge back so the document does not drift.
            let joined = format!("{a}{b}");
            let outcome = doc.replace_paragraphs(middle, 2, vec![Paragraph::with_text(&joined)]);
            cache.apply_edit(&doc, &outcome);
            while !cache.layout_complete() {
                cache.step(&doc, 512);
            }
        }
        stats("Enter (split)", enter);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    host::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
