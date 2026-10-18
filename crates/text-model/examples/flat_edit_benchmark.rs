// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cost on the model side of one flat edit in the middle of a 300-page
//! document (US-204 criterion: under 1 ms).
//!
//! `cargo run --release -p atelier-text-model --example flat_edit_benchmark`
//!
//! The document has 300 pages of 40 paragraphs (12 000 paragraphs, about
//! 1 MB, one in ten with an object, a break or a symbol); a second run uses
//! 10 times more paragraphs to show that the cost does not follow the size.
//! Every step edits at a position taken from a fixed-seed generator in the
//! middle half of the document, so two runs measure the same edits. Times
//! are medians, 99th percentiles and maxima over `ITERATIONS` calls of
//! `replace_range` / `undo`, index construction excluded (built once by the
//! first query, reported on its own).

use atelier_model::Transaction;
use atelier_text_model::rich::flat::{FlatEdit, FlatIndex, LayoutContext};
use atelier_text_model::rich::{AnchoredObject, InlineBreak, InlineSymbol, RichDocument, RichOp};
use std::time::{Duration, Instant};

const ITERATIONS: usize = 2000;
const SEED: u64 = 0x5eed_0204;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn build(pages: usize, paragraphs_per_page: usize) -> Result<RichDocument, String> {
    let mut doc = RichDocument::new();
    let section = doc.section_ids()[0];
    let words = [
        "atelier",
        "paragraphe",
        "document",
        "\u{e9}criture",
        "texte",
        "mise",
        "en",
        "page",
    ];
    let mut rng = Rng(SEED);
    let count = pages * paragraphs_per_page;
    let mut ops = Vec::with_capacity(count);
    for i in 0..count {
        let mut text = String::new();
        for _ in 0..8 + rng.below(8) {
            text.push_str(words[rng.below(words.len())]);
            text.push(' ');
        }
        let mut sub = doc
            .new_paragraph(text.trim_end())
            .map_err(|e| e.to_string())?;
        if i % 10 == 0 {
            let id = doc.alloc_id().map_err(|e| e.to_string())?;
            let offset = text.len() / 2;
            let offset = (0..=offset)
                .rev()
                .find(|o| text.is_char_boundary(*o))
                .unwrap_or(0);
            if let Some(atelier_text_model::rich::Node::Paragraph(p)) = sub.nodes.get_mut(&sub.root)
            {
                match i % 30 {
                    0 => p.anchored.push(AnchoredObject {
                        id,
                        offset,
                        kind: "pic".into(),
                        data: vec![0; 16],
                    }),
                    10 => p.breaks.push(InlineBreak {
                        id,
                        offset,
                        kind: atelier_text_model::rich::BreakKind::Line,
                    }),
                    _ => p.symbols.push(InlineSymbol {
                        id,
                        offset,
                        font: "Symbol".into(),
                        code: 0xf0b7,
                    }),
                }
            }
        }
        ops.push(RichOp::InsertBlock {
            parent: Some(section),
            position: i + 1,
            subtree: sub,
        });
    }
    while !ops.is_empty() {
        let rest = ops.split_off(ops.len().min(4096));
        let batch = std::mem::replace(&mut ops, rest);
        let transaction = Transaction::new(batch).map_err(|e| format!("{e:?}"))?;
        doc.apply(&transaction).map_err(|e| e.to_string())?;
    }
    Ok(doc)
}

fn report(name: &str, mut times: Vec<Duration>) -> Duration {
    times.sort();
    let median = times[times.len() / 2];
    let p99 = times[times.len() * 99 / 100];
    let max = times[times.len() - 1];
    println!(
        "{name:<34} median {:>9.1} us   p99 {:>9.1} us   max {:>9.1} us",
        median.as_secs_f64() * 1e6,
        p99.as_secs_f64() * 1e6,
        max.as_secs_f64() * 1e6
    );
    p99
}

fn measure(label: &str, pages: usize, per_page: usize) -> Result<(), String> {
    let mut doc = build(pages, per_page)?;
    let started = Instant::now();
    let len = doc.len();
    let build_index = started.elapsed();
    println!(
        "\n{label}: {} paragraphs, {} bytes of flat text, index built in {:.2} ms",
        doc.paragraph_count(),
        len,
        build_index.as_secs_f64() * 1e3
    );
    let mut rng = Rng(SEED);
    let mut position = |doc: &RichDocument| -> usize {
        // Middle half of the document, snapped to a valid position.
        let len = doc.len();
        let raw = len / 4 + rng.below(len / 2);
        doc.snap_back(raw).unwrap_or(0)
    };
    let timed =
        |name: &str, doc: &mut RichDocument, mut step: Box<dyn FnMut(&mut RichDocument)>| {
            let mut times = Vec::with_capacity(ITERATIONS);
            for _ in 0..ITERATIONS {
                let t = Instant::now();
                step(doc);
                times.push(t.elapsed());
            }
            report(name, times)
        };
    let mut worst = Duration::ZERO;
    let mut edit_positions = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        edit_positions.push(position(&doc));
    }
    // Typing one character.
    let positions = edit_positions.clone();
    let mut i = 0;
    worst = worst.max(timed(
        "type one character",
        &mut doc,
        Box::new(move |d| {
            let at = d
                .snap_back(positions[i % positions.len()].min(d.len()))
                .unwrap_or(0);
            i += 1;
            let _ = d.replace_range(at, at, "x");
        }),
    ));
    // Enter: split a paragraph.
    let positions = edit_positions.clone();
    let mut i = 0;
    worst = worst.max(timed(
        "Enter (split a paragraph)",
        &mut doc,
        Box::new(move |d| {
            let at = d
                .snap_back(positions[i % positions.len()].min(d.len()))
                .unwrap_or(0);
            i += 1;
            let _ = d.replace_range(at, at, "\n");
        }),
    ));
    // Backspace over a separator: merge two paragraphs.
    let positions = edit_positions.clone();
    let mut i = 0;
    worst = worst.max(timed(
        "delete a separator (merge)",
        &mut doc,
        Box::new(move |d| {
            let at = d
                .snap_back(positions[i % positions.len()].min(d.len()))
                .unwrap_or(0);
            i += 1;
            if let Ok(loc) = d.locate(at) {
                if let Ok(start) = d.paragraph_start(loc.paragraph) {
                    if let Ok(len) = d.paragraph_flat(loc.paragraph).map(|t| t.len()) {
                        let sep = start + len;
                        if sep < d.len() {
                            let _ = d.replace_range(sep, sep + 1, "");
                        }
                    }
                }
            }
        }),
    ));
    // Delete a cluster.
    let positions = edit_positions.clone();
    let mut i = 0;
    worst = worst.max(timed(
        "delete one cluster",
        &mut doc,
        Box::new(move |d| {
            let at = d
                .snap_back(positions[i % positions.len()].min(d.len()))
                .unwrap_or(0);
            i += 1;
            if let Ok(Some(next)) = d.next_grapheme(at) {
                let _ = d.replace_range(at, next, "");
            }
        }),
    ));
    // Paste three lines over a selection spanning a paragraph boundary.
    let positions = edit_positions.clone();
    let mut i = 0;
    worst = worst.max(timed(
        "replace 20 bytes by 3 lines",
        &mut doc,
        Box::new(move |d| {
            let at = d
                .snap_back(positions[i % positions.len()].min(d.len()))
                .unwrap_or(0);
            i += 1;
            let end = d.snap_back((at + 20).min(d.len())).unwrap_or(at);
            let _ = d.replace_range(at, end, "one\ntwo\nthree");
        }),
    ));
    // Undo and redo of those edits.
    let undo_count = ITERATIONS.min(5 * ITERATIONS);
    let mut times = Vec::with_capacity(undo_count);
    for _ in 0..undo_count {
        let t = Instant::now();
        let done = doc.undo().is_some();
        let elapsed = t.elapsed();
        if !done {
            break;
        }
        times.push(elapsed);
    }
    if !times.is_empty() {
        worst = worst.max(report("undo", times));
    }
    let mut times = Vec::new();
    for _ in 0..ITERATIONS {
        let t = Instant::now();
        let done = doc.redo().is_some();
        let elapsed = t.elapsed();
        if !done {
            break;
        }
        times.push(elapsed);
    }
    if !times.is_empty() {
        worst = worst.max(report("redo", times));
    }
    // Pure index queries.
    let mut times = Vec::with_capacity(ITERATIONS);
    for p in edit_positions.iter().take(ITERATIONS) {
        let at = p.min(&doc.len()).to_owned();
        let t = Instant::now();
        let _ = doc.locate(at);
        let _ = doc.check_position(at);
        times.push(t.elapsed());
    }
    report("locate + check_position", times);
    println!(
        "worst 99th percentile of the edits: {:.1} us",
        worst.as_secs_f64() * 1e6
    );
    doc.validate().map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    measure("300 pages (40 paragraphs each)", 300, 40)?;
    measure("3000 pages (10 times more)", 3000, 40)?;
    Ok(())
}
