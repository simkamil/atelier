// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, GlyphCluster, LayoutLimits};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    let mut state = 0x1234_abcd_5678_90efu64;
    let mut rejected = 0;
    let mut accepted = 0;
    for path in &paths {
        let original = std::fs::read(path).unwrap();
        let tables = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
        let mut ranges = Vec::new();
        for i in 0..tables {
            let at = 12 + 16 * i;
            let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
            if [*b"GSUB", *b"GPOS", *b"GDEF", *b"kern"].contains(&tag) {
                let offset =
                    u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
                let length =
                    u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
                ranges.push((at, offset, length));
            }
        }
        for case in 0..500 {
            let mut data = original.clone();
            let (record, start, length) = ranges[case % ranges.len()];
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            if case % 10 == 0 {
                let short = (state as usize % length) as u32;
                data[record + 12..record + 16].copy_from_slice(&short.to_be_bytes());
            } else {
                let at = start + state as usize % length;
                data[at] ^= 1 << (case % 8);
            }
            let result = catch_unwind(AssertUnwindSafe(|| {
                let face = FontFace::parse(&data).map_err(|_| ())?;
                let limits = LayoutLimits {
                    validation_units: 100_000,
                    work_units: 20_000,
                    input_glyphs: 64,
                    selected_lookups: 1024,
                };
                let layout = face.layout_with_limits(limits).map_err(|_| ())?;
                let glyphs: Vec<_> = "AV To office ffi ffl"
                    .chars()
                    .enumerate()
                    .map(|(i, c)| GlyphCluster {
                        glyph_id: face.glyph_index(c),
                        cluster: i..i + 1,
                    })
                    .collect();
                let output = layout.apply_ligatures(&glyphs).map_err(|_| ())?;
                layout.pair_adjustments(&output).map_err(|_| ())?;
                Ok::<_, ()>(())
            }));
            match result {
                Ok(Ok(())) => accepted += 1,
                Ok(Err(())) => rejected += 1,
                Err(_) => panic!("layout panic in {path}, mutation {case}"),
            }
        }
    }
    println!(
        "{{\"cases\":{},\"faces\":{},\"accepted\":{accepted},\"rejected\":{rejected},\"panics\":0}}",
        accepted + rejected,
        paths.len()
    );
}
