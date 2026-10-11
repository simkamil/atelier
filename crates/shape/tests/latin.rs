// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, GlyphCluster, LayoutLimits};
use atelier_shape::{LatinShaper, ShapeError, ShapeOptions};

fn shaper() -> (FontFace<'static>, &'static [u8]) {
    let bytes = atelier_font::embedded::EMBEDDED_FONTS[0].data;
    (FontFace::parse(bytes).unwrap(), bytes)
}

#[test]
fn text_maps_scalars_and_reports_utf8_grapheme_boundaries() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let run = shaper.shape_text("Aé", ShapeOptions::default()).unwrap();
    assert_eq!(run.grapheme_boundaries.as_deref(), Some(&[0, 1, 3][..]));
    assert_eq!(run.glyphs.len(), 2);
    assert_eq!(run.glyphs[0].cluster, 0..1);
    assert_eq!(run.glyphs[1].cluster, 1..2);
    assert_eq!(run.x_advance, run.glyphs.iter().map(|g| g.x_advance).sum());
}

#[test]
fn generic_glyphs_preserve_clusters_and_spacing_groups() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let items = [
        GlyphCluster {
            glyph_id: face.glyph_index('A'),
            cluster: 4..7,
        },
        GlyphCluster {
            glyph_id: face.glyph_index('B'),
            cluster: 4..7,
        },
        GlyphCluster {
            glyph_id: face.glyph_index('C'),
            cluster: 9..10,
        },
    ];
    let run = shaper
        .shape_glyphs(
            &items,
            ShapeOptions {
                letter_spacing: 17,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(run.grapheme_boundaries, None);
    assert_eq!(
        run.glyphs
            .iter()
            .map(|g| g.cluster.clone())
            .collect::<Vec<_>>(),
        [4..7, 4..7, 9..10]
    );
    assert_eq!(
        run.glyphs[0].x_advance,
        shaper
            .shape_glyphs(&items[..1], ShapeOptions::default())
            .unwrap()
            .glyphs[0]
            .x_advance
    );
    assert_eq!(
        run.glyphs[1].x_advance,
        shaper
            .shape_glyphs(&items[..2], ShapeOptions::default())
            .unwrap()
            .glyphs[1]
            .x_advance
            + 17
    );
}

#[test]
fn spacing_is_added_between_graphemes_not_inside_combining_or_emoji_sequences() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let text = "a\u{301} 👩‍💻b";
    let plain = shaper.shape_text(text, ShapeOptions::default()).unwrap();
    let spaced = shaper
        .shape_text(
            text,
            ShapeOptions {
                letter_spacing: 23,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        plain.grapheme_boundaries.as_deref(),
        Some(&[0, 3, 4, 15, 16][..])
    );
    assert_eq!(spaced.x_advance, plain.x_advance + 69);
    assert_eq!(spaced.glyphs.len(), plain.glyphs.len());
}

#[test]
fn validates_options_ranges_ids_and_input_limit() {
    let (face, _) = shaper();
    let shaper = LatinShaper::with_layout_limits(
        &face,
        LayoutLimits {
            input_glyphs: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        shaper.shape_text("abc", Default::default()),
        Err(ShapeError::LimitExceeded(_))
    ));
    assert!(matches!(
        shaper.shape_text(
            "",
            ShapeOptions {
                size: 0,
                ..Default::default()
            }
        ),
        Err(ShapeError::InvalidOptions(_))
    ));
    assert_eq!(
        shaper.shape_text("", Default::default()).unwrap().glyphs,
        []
    );
    assert_eq!(
        shaper
            .shape_glyphs(&[], Default::default())
            .unwrap()
            .x_advance,
        0
    );
    assert!(matches!(
        shaper.shape_glyphs(
            &[GlyphCluster {
                glyph_id: 0,
                cluster: 1..1
            }],
            Default::default()
        ),
        Err(ShapeError::InvalidClusterRange)
    ));
    assert!(matches!(
        shaper.shape_glyphs(
            &[GlyphCluster {
                glyph_id: u16::MAX,
                cluster: 0..1
            }],
            Default::default()
        ),
        Err(ShapeError::InvalidGlyph(_))
    ));
}

#[test]
fn fractional_scaling_rounds_ties_away_from_zero() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let upem = i64::from(face.metrics().units_per_em);
    let options = ShapeOptions {
        size: upem / 2,
        horizontal_scale: 1000,
        ..Default::default()
    };
    let one = [GlyphCluster {
        glyph_id: 1,
        cluster: 0..1,
    }];
    let run = shaper.shape_glyphs(&one, options).unwrap();
    assert_eq!(run.x_advance, run.glyphs.iter().map(|g| g.x_advance).sum());
}

fn round_away(numerator: i128, denominator: i128) -> i64 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let rounded = if remainder.abs() * 2 >= denominator {
        quotient + if numerator < 0 { -1 } else { 1 }
    } else {
        quotient
    };
    i64::try_from(rounded).unwrap()
}

#[test]
fn fractional_size_and_non_default_scale_match_independent_rational_result() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let glyph_id = face.glyph_index('A');
    let advance = i128::from(face.horizontal_metric(glyph_id).unwrap().advance_width);
    let upem = i128::from(face.metrics().units_per_em);
    let options = ShapeOptions {
        size: 713,
        horizontal_scale: 1375,
        ..Default::default()
    };
    let run = shaper
        .shape_glyphs(
            &[GlyphCluster {
                glyph_id,
                cluster: 0..1,
            }],
            options,
        )
        .unwrap();
    let expected = round_away(advance * 713 * 1375, upem * 1000);
    assert_eq!(run.glyphs[0].x_advance, expected);
    assert_eq!(run.x_advance, expected);
}

#[test]
fn output_glyph_and_total_advance_overflow_are_reported() {
    let (face, _) = shaper();
    let shaper = LatinShaper::new(&face).unwrap();
    let glyph_id = face.glyph_index('A');
    let advance = i128::from(face.horizontal_metric(glyph_id).unwrap().advance_width);
    let upem = i128::from(face.metrics().units_per_em);
    let single = [GlyphCluster {
        glyph_id,
        cluster: 0..1,
    }];
    assert_eq!(
        shaper.shape_glyphs(
            &single,
            ShapeOptions {
                size: i64::MAX,
                horizontal_scale: u32::MAX,
                ..Default::default()
            }
        ),
        Err(ShapeError::ArithmeticOverflow)
    );

    let scale = i128::from(u32::MAX);
    let size = (i128::from(i64::MAX) * 3 * upem * 1000 / (5 * advance * scale)) as i64;
    let pair = [
        GlyphCluster {
            glyph_id,
            cluster: 0..1,
        },
        GlyphCluster {
            glyph_id,
            cluster: 1..2,
        },
    ];
    let options = ShapeOptions {
        size,
        horizontal_scale: u32::MAX,
        ..Default::default()
    };
    let one = shaper.shape_glyphs(&single, options).unwrap().x_advance;
    assert!(one > i64::MAX / 2);
    assert_eq!(
        shaper.shape_glyphs(&pair, options),
        Err(ShapeError::ArithmeticOverflow)
    );
}
