// SPDX-License-Identifier: AGPL-3.0-or-later
//! Indicative performance and pathological inputs of the text search (US-340).
//! Run `cargo test -p atelier-text-model --release --test search_perf --
//! --nocapture` for the timings.

use atelier_text_model::rich::{RichDocument, RichOp};
use atelier_text_model::search::{Position, SearchOptions, Searcher, TextLines};
use std::time::Instant;

fn opts() -> SearchOptions {
    SearchOptions::default()
}

#[test]
fn quadratic_traps_stay_linear() {
    let n = 200_000;
    let a = "a".repeat(n);
    let t = Instant::now();
    // The whole text.
    let lines = TextLines(&a);
    let s = Searcher::new(&lines, &a, opts());
    assert_eq!(s.find_all(10).len(), 1);
    // Half the text: two non-overlapping occurrences.
    let half = "a".repeat(n / 2);
    assert_eq!(Searcher::new(&lines, &half, opts()).count(10), (2, false));
    // Almost matching everywhere.
    let miss = format!("{}b", "a".repeat(n / 2));
    assert_eq!(Searcher::new(&lines, &miss, opts()).count(10), (0, false));
    // Many tiny occurrences, capped.
    let s = Searcher::new(&lines, "a", opts());
    assert_eq!(s.count(1000), (1000, true));
    assert_eq!(s.find_all(1000).len(), 1000);
    // A very long run of combining marks (one grapheme).
    let marks = format!("e{}", "\u{301}".repeat(n));
    let lines = TextLines(&marks);
    assert_eq!(Searcher::new(&lines, "e", opts()).count(10), (0, false));
    let ia = SearchOptions {
        ignore_accents: true,
        ..opts()
    };
    assert_eq!(Searcher::new(&lines, "e", ia).count(10), (1, false));
    // Many graphemes made of marks, whole word.
    let ww = SearchOptions {
        whole_word: true,
        ignore_accents: true,
        ..opts()
    };
    let many = "e\u{301} ".repeat(n / 4);
    let lines = TextLines(&many);
    assert_eq!(Searcher::new(&lines, "e", ww).count(usize::MAX).0, n / 4);
    // Navigation in a huge paragraph of matches stays cheap enough.
    let lines = TextLines(&a);
    let s = Searcher::new(&lines, "aa", opts());
    let step = s
        .next_from(&Position {
            paragraph: lines.position(0).paragraph,
            offset: n - 10,
        })
        .unwrap();
    assert_eq!(step.occurrence.range.start, n - 10);
    println!("pathological inputs: {:?}", t.elapsed());
    assert!(t.elapsed().as_secs() < 60);
}

#[test]
fn three_hundred_pages() {
    // About 1.5 MB: 300 pages of 5 KB, paragraphs of about 500 bytes.
    let words = [
        "le",
        "chat",
        "mange",
        "la",
        "souris",
        "\u{e9}l\u{e8}ve",
        "stra\u{df}e",
        "maison",
        "Needle",
        "tr\u{e8}s",
        "grand",
        "petit",
        "jardin",
        "avec",
        "des",
        "fleurs",
    ];
    let mut text = String::new();
    let mut seed = 12345u64;
    while text.len() < 1_500_000 {
        for _ in 0..80 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            text.push_str(words[(seed >> 33) as usize % words.len()]);
            text.push(' ');
        }
        text.push('\n');
    }
    let lines = TextLines(&text);
    for (label, query, o) in [
        ("ascii, default options", "needle", opts()),
        (
            "whole word, ignore accents",
            "eleve",
            SearchOptions {
                whole_word: true,
                ignore_accents: true,
                ..opts()
            },
        ),
        ("fold ss", "STRASSE", opts()),
    ] {
        let t = Instant::now();
        let s = Searcher::new(&lines, query, o);
        let prep = t.elapsed();
        let all = s.find_all(usize::MAX);
        let total = t.elapsed();
        println!(
            "300 pages ({} bytes, {} paragraphs), {label}: {} hits, prepare {prep:?}, total {total:?}",
            text.len(),
            lines_count(&text),
            all.len()
        );
        assert!(!all.is_empty());
        assert!(total.as_secs() < 60);
    }
    let t = Instant::now();
    let s = Searcher::new(&lines, "needle", opts());
    let step = s.next_from(&lines.position(text.len() / 2)).unwrap();
    println!("one next_from in the middle: {:?}", t.elapsed());
    assert!(!step.wrapped);

    // The same on a rich document of many paragraphs.
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let first = doc.paragraph_ids()[0];
    let mut ops = vec![RichOp::InsertText {
        para: first,
        offset: 0,
        text: "needle".to_owned(),
    }];
    for (i, line) in text.lines().take(2_000).enumerate() {
        let sub = doc.new_paragraph(line).unwrap();
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position: i + 1,
            subtree: sub,
        });
    }
    doc.apply(&atelier_model::Transaction::new(ops).unwrap())
        .unwrap();
    let t = Instant::now();
    let r = Searcher::new(&doc, "needle", opts()).find_all(usize::MAX);
    println!(
        "rich document, 2001 paragraphs: {} hits in {:?}",
        r.len(),
        t.elapsed()
    );
}

fn lines_count(t: &str) -> usize {
    t.matches('\n').count() + 1
}
