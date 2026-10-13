// SPDX-License-Identifier: AGPL-3.0-or-later
//! Indicative timing of the statistics on a document of about 300 pages.
//! Ignored by default: `cargo test -p atelier-text-model --release --test
//! statistics_perf -- --ignored --nocapture`.

mod common;

use atelier_model::selection::{Affinity, Selection, SelectionRange, TextPosition};
use atelier_text_model::rich::RichOp;
use atelier_text_model::statistics::{
    PlainStatisticsCache, StatisticsCache, StatisticsOptions, document, plain_text, selection,
};
use common::*;
use std::time::Instant;

const WORDS: &[&str] = &[
    "le",
    "document",
    "présente",
    "l'école",
    "—",
    "10\u{a0}km",
    "1,234.56",
    "texte",
    "mot,",
    "été",
    "de",
    "la",
    "statistique",
    "日本語",
    "(note)",
    "et",
];

fn paragraph(i: usize) -> String {
    (0..80)
        .map(|k| WORDS[(i * 7 + k * 3) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
#[ignore]
fn three_hundred_pages() {
    // 300 pages x 20 paragraphs of 80 words: about 480 000 words, 3.5 MB.
    let (mut doc, _) = with_text(&paragraph(0));
    let section = first_section(&doc);
    for i in 1..6000 {
        let subtree = doc.new_paragraph(&paragraph(i)).unwrap();
        doc.apply(&tx(vec![RichOp::InsertBlock {
            parent: Some(section),
            position: i,
            subtree,
        }]))
        .unwrap();
    }
    let options = StatisticsOptions::default();
    let t = Instant::now();
    let stats = document(&doc, &options);
    let full = t.elapsed();
    let t = Instant::now();
    let mut cache = StatisticsCache::build(&doc, options);
    let build = t.elapsed();
    let last = doc.paragraph_ids()[3000];
    doc.apply(&tx(vec![RichOp::InsertText {
        para: last,
        offset: 0,
        text: "mot ".into(),
    }]))
    .unwrap();
    let t = Instant::now();
    cache.update_paragraph(&doc, last).unwrap();
    let update = t.elapsed();
    let t = Instant::now();
    cache.sync(&doc, &[last]);
    let sync = t.elapsed();
    let ids = doc.paragraph_ids();
    let pos = |n, o| TextPosition::new(n, o, Affinity::After);
    let end = doc.paragraph(*ids.last().unwrap()).unwrap().text.len();
    let sel = Selection::new(
        vec![SelectionRange::new(
            pos(ids[0], 0),
            pos(*ids.last().unwrap(), end),
        )],
        0,
    )
    .unwrap();
    let t = Instant::now();
    let selected = selection(&doc, &sel, &options).unwrap();
    let select = t.elapsed();
    assert_eq!(selected.counts(), cache.counts());
    assert_eq!(cache.counts().words, stats.words + 1);

    let text: String = (0..6000).map(paragraph).collect::<Vec<_>>().join("\n");
    let t = Instant::now();
    let plain = plain_text(&text, &options);
    let plain_full = t.elapsed();
    let mut pc = PlainStatisticsCache::build(&text, options);
    let mut edited = text.clone();
    edited.insert_str(1_000_000, "mot ");
    let t = Instant::now();
    pc.update(&edited, 1_000_000, 1_000_000, 4);
    let plain_update = t.elapsed();
    assert_eq!(pc.counts(), plain_text(&edited, &options).counts());
    eprintln!(
        "words {} chars {} paragraphs {} | rich: full {full:?}, cache build {build:?}, update_paragraph {update:?}, sync {sync:?}, whole selection {select:?} | plain ({} bytes): full {plain_full:?}, cache update {plain_update:?}",
        stats.words,
        stats.characters_with_spaces,
        stats.paragraphs,
        text.len(),
    );
    assert_eq!(plain.words, stats.words);
}
