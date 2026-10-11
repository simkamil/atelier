// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, GlyphCluster};
use atelier_shape::{LatinShaper, ShapeOptions};
use std::collections::BTreeMap;
use std::path::Path;

const REFERENCE: &str = include_str!("../../../tests/font-layout-reference/layout.tsv");

#[test]
fn generic_overlapping_and_unordered_ranges_keep_consecutive_spacing_groups() {
    let face = FontFace::parse(atelier_font::embedded::EMBEDDED_FONTS[0].data).unwrap();
    let shaper = LatinShaper::new(&face).unwrap();
    let ranges = [8..12, 3..10, 3..10, 1..2];
    let glyphs: Vec<_> = ranges
        .iter()
        .enumerate()
        .map(|(i, range)| GlyphCluster {
            glyph_id: face.glyph_index(['A', 'B', 'C', 'D'][i]),
            cluster: range.clone(),
        })
        .collect();
    // Both calls disable optional liga; the difference is only output spacing.
    let first = shaper
        .shape_glyphs(
            &glyphs,
            ShapeOptions {
                letter_spacing: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let second = shaper
        .shape_glyphs(
            &glyphs,
            ShapeOptions {
                letter_spacing: 18,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        second
            .glyphs
            .iter()
            .map(|g| g.cluster.clone())
            .collect::<Vec<_>>(),
        ranges
    );
    assert_eq!(
        second
            .glyphs
            .iter()
            .zip(&first.glyphs)
            .map(|(a, b)| a.x_advance - b.x_advance)
            .collect::<Vec<_>>(),
        [17, 0, 17, 0]
    );
    assert_eq!(second.x_advance - first.x_advance, 34);
}

#[test]
fn spacing_disables_optional_ligatures_on_all_faces() {
    for font in atelier_font::embedded::EMBEDDED_FONTS {
        let face = FontFace::parse(font.data).unwrap();
        let shaper = LatinShaper::new(&face).unwrap();
        for spacing in [-23, 23] {
            let run = shaper
                .shape_text(
                    "ffi",
                    ShapeOptions {
                        letter_spacing: spacing,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(
                run.glyphs.iter().map(|g| g.glyph_id).collect::<Vec<_>>(),
                [
                    face.glyph_index('f'),
                    face.glyph_index('f'),
                    face.glyph_index('i')
                ]
            );
            assert_eq!(
                run.glyphs
                    .iter()
                    .map(|g| g.cluster.clone())
                    .collect::<Vec<_>>(),
                [0..1, 1..2, 2..3]
            );
        }
        let text = "a\u{301} 👩‍💻b";
        let base = shaper.shape_text(text, ShapeOptions::default()).unwrap();
        for spacing in [-23, 23] {
            let run = shaper
                .shape_text(
                    text,
                    ShapeOptions {
                        letter_spacing: spacing,
                        ..Default::default()
                    },
                )
                .unwrap();
            let deltas: Vec<_> = run
                .glyphs
                .iter()
                .zip(&base.glyphs)
                .map(|(g, b)| g.x_advance - b.x_advance)
                .collect();
            assert_eq!(deltas, [0, spacing, spacing, 0, 0, spacing, 0]);
        }
    }
}

// Independent rational rounding: Euclidean floor plus a remainder threshold.
// Negative exact halves stay at the floor, positive halves round upwards.
fn rounded(numerator: i128, denominator: i128) -> i64 {
    let floor = numerator.div_euclid(denominator);
    let remainder = numerator.rem_euclid(denominator);
    let bump = remainder * 2 > denominator || (numerator >= 0 && remainder * 2 == denominator);
    i64::try_from(floor + i128::from(bump)).unwrap()
}

#[test]
fn complete_harfbuzz_corpus_matches_shaper_at_fractional_size_and_scale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut grouped = BTreeMap::<&str, Vec<(&str, &str, usize)>>::new();
    for (line_number, line) in REFERENCE.lines().enumerate() {
        let mut columns = line.splitn(3, '\t');
        grouped.entry(columns.next().unwrap()).or_default().push((
            columns.next().unwrap(),
            columns.next().unwrap(),
            line_number + 1,
        ));
    }
    assert_eq!(grouped.len(), 20);
    let mut cases = 0;
    for (font, records) in grouped {
        let bytes = std::fs::read(root.join(font)).unwrap();
        let face = FontFace::parse(&bytes).unwrap();
        let shaper = LatinShaper::new(&face).unwrap();
        let upem = i128::from(face.metrics().units_per_em);
        // A fractional point size and a fractional horizontal scale exercise
        // rounding, beyond simply forwarding integer font units.
        let options = ShapeOptions {
            size: 721,
            horizontal_scale: 1137,
            ..ShapeOptions::default()
        };
        for (codepoints, expected, line) in records {
            let text: String = codepoints
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| char::from_u32(u32::from_str_radix(s, 16).unwrap()).unwrap())
                .collect();
            let run = shaper.shape_text(&text, options).unwrap();
            let reference: Vec<Vec<i128>> = expected
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|record| record.split(',').map(|s| s.parse().unwrap()).collect())
                .collect();
            assert_eq!(run.glyphs.len(), reference.len(), "{font}:{line}");
            let mut totals = (0, 0);
            for (actual, expected) in run.glyphs.iter().zip(reference) {
                assert_eq!(expected.len(), 7);
                let x = |v| rounded(v * 721 * 1137, upem * 1000);
                let y = |v| rounded(v * 721, upem);
                assert_eq!(i128::from(actual.glyph_id), expected[0], "{font}:{line}");
                assert_eq!(
                    actual.cluster,
                    expected[1] as usize..expected[2] as usize,
                    "{font}:{line}"
                );
                assert_eq!(
                    (
                        actual.x_offset,
                        actual.y_offset,
                        actual.x_advance,
                        actual.y_advance
                    ),
                    (
                        x(expected[3]),
                        y(expected[4]),
                        x(expected[5]),
                        y(expected[6])
                    ),
                    "{font}:{line}"
                );
                totals.0 += actual.x_advance;
                totals.1 += actual.y_advance;
            }
            assert_eq!((run.x_advance, run.y_advance), totals, "{font}:{line}");
            let boundaries = run.grapheme_boundaries.unwrap();
            assert_eq!(boundaries.first(), Some(&0));
            assert_eq!(boundaries.last(), Some(&text.len()));
            assert!(boundaries.iter().all(|&b| text.is_char_boundary(b)));
            cases += 1;
        }
    }
    assert_eq!(cases, 54_716);
}
