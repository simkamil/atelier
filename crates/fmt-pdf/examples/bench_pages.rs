// SPDX-License-Identifier: AGPL-3.0-or-later
//! Time and size of a long text document (US-332: 100 pages in under 3 s).
//!
//! `cargo run --release -p atelier-fmt-pdf --example bench_pages -- [pages]`
//! The display lists are built first; only `render_pdf` is timed.

#[path = "../tests/common/mod.rs"]
mod common;

use atelier_fmt_pdf::{PdfOptions, render_pdf};
use common::*;
use std::time::Instant;

const PARAGRAPH: &str = "Voilà un texte accentué : déjà, où, naïve, garçon, Noël, Œuvre. \
Ligatures : office, difficile, affluent, final, flux. Le rapport annuel présente les résultats \
de l’exercice, les perspectives et les risques identifiés par la direction.";

fn main() {
    let pages: usize = std::env::args()
        .nth(1)
        .map_or(100, |s| s.parse().expect("page count"));
    let mut res = Res::default();
    standard_fonts(&mut res);
    let mut lists = Vec::new();
    for n in 0..pages {
        let mut commands = vec![res.text(
            CARLITO_BOLD,
            &format!("Chapitre {} — Résultats", n + 1),
            56.0,
            72.0,
            BLACK,
        )];
        // 46 lines of about 85 characters: a full A4 page of 12 pt text.
        let words: Vec<&str> = PARAGRAPH.split(' ').collect();
        for line in 0..46 {
            let mut text = String::new();
            let mut k = (n * 7 + line * 5) % words.len();
            while text.chars().count() < 80 {
                text.push_str(words[k % words.len()]);
                text.push(' ');
                k += 1;
            }
            let font = if line % 9 == 8 {
                CARLITO_ITALIC
            } else {
                CARLITO
            };
            commands.push(res.text(
                font,
                text.trim_end(),
                56.0,
                110.0 + 15.0 * line as f64,
                BLACK,
            ));
        }
        lists.push(list(595.0, 842.0, commands));
    }
    let start = Instant::now();
    let out = render_pdf(&lists, &res, &PdfOptions::default()).unwrap();
    let elapsed = start.elapsed();
    let glyphs: usize = lists
        .iter()
        .flat_map(|l| &l.commands)
        .map(|c| match c {
            atelier_render::Command::DrawGlyphRun { glyphs, .. } => glyphs.len(),
            _ => 0,
        })
        .sum();
    println!(
        "{pages} pages, {glyphs} glyphs: {:.1} ms, {} bytes ({:.1} KiB per page), notices {:?}",
        elapsed.as_secs_f64() * 1000.0,
        out.bytes.len(),
        out.bytes.len() as f64 / 1024.0 / pages as f64,
        out.notices
    );
    if let Some(path) = std::env::args().nth(2) {
        std::fs::write(path, &out.bytes).unwrap();
    }
}
