// SPDX-License-Identifier: AGPL-3.0-or-later
//! Agent-only driver: export actual public Paths without modifying the API.
use atelier_font::FontFace;
use std::io::{BufWriter, Write};

fn main() {
    let mut output = BufWriter::new(std::io::stdout().lock());
    for (index, filename) in std::env::args().skip(1).enumerate() {
        let bytes = std::fs::read(filename).unwrap();
        let face = FontFace::parse(&bytes).unwrap();
        for glyph in 0..face.metrics().number_of_glyphs {
            let path = face.outline(glyph).unwrap();
            writeln!(output, "{index}\t{glyph}\t{path:?}").unwrap();
        }
    }
}
