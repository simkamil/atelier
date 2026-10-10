// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{FontFace, HorizontalMetric, VariationGlyph};

const FONT_0_4: &[u8] = include_bytes!("fixtures/font-0-4.font");
const FONT_0_6: &[u8] = include_bytes!("fixtures/font-0-6.font");
const FONT_0_12: &[u8] = include_bytes!("fixtures/font-0-12.font");
const FONT_1_4: &[u8] = include_bytes!("fixtures/font-1-4.font");
const FONT_1_6: &[u8] = include_bytes!("fixtures/font-1-6.font");
const FONT_1_12: &[u8] = include_bytes!("fixtures/font-1-12.font");
const TTC: &[u8] = include_bytes!("fixtures/two-faces.ttc");

#[test]
fn parses_generated_sfnt_faces_and_cmap_formats() {
    for bytes in [FONT_0_4, FONT_0_6, FONT_0_12, FONT_1_4, FONT_1_6, FONT_1_12] {
        let face = FontFace::parse(bytes).expect("generated face parses");
        assert_eq!(face.glyph_index('A'), 1);
        assert_eq!(face.glyph_index('B'), 3);
        assert_eq!(face.glyph_index('C'), 2);
        assert_eq!(face.glyph_index('D'), 0);
        assert_eq!(face.glyph_index(char::MAX), 0);
        assert_eq!(face.metrics().number_of_glyphs, 6);
        assert_eq!(face.metrics().units_per_em, 1000);
    }
    assert_eq!(FontFace::parse(FONT_0_12).unwrap().glyph_index('😀'), 5);
    assert_eq!(FontFace::parse(FONT_1_12).unwrap().glyph_index('😀'), 5);
}

#[test]
fn parses_generated_ttc_faces_and_variation_sequences() {
    let face0 = FontFace::parse_collection(TTC, 0).unwrap();
    let face1 = FontFace::parse_collection(TTC, 1).unwrap();
    assert_eq!(face0.glyph_index('😀'), 0);
    assert_eq!(face1.glyph_index('😀'), 5);
    assert_eq!(
        face0.variation_glyph('A', '\u{fe0f}'),
        VariationGlyph::Default
    );
    assert_eq!(
        face0.variation_glyph('B', '\u{fe0f}'),
        VariationGlyph::NonDefault(4)
    );
    assert_eq!(
        face0.variation_glyph('C', '\u{fe0f}'),
        VariationGlyph::Unsupported
    );
}

#[test]
fn repeated_horizontal_advance_and_name_records_match_fixture_contract() {
    let face = FontFace::parse(FONT_0_4).unwrap();
    assert_eq!(
        face.horizontal_metric(0).unwrap(),
        HorizontalMetric {
            advance_width: 600,
            left_side_bearing: -14
        }
    );
    assert_eq!(
        face.horizontal_metric(5).unwrap(),
        HorizontalMetric {
            advance_width: 500,
            left_side_bearing: 21
        }
    );
    assert!(!face.names().is_empty());
}
