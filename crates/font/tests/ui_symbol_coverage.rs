// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::FontFace;

#[test]
fn ui_symbol_catalogue_matches_the_engine_font_cmap() {
    let face = FontFace::parse(include_bytes!(
        "../../../data/fonts/carlito/Carlito-Regular.ttf"
    ))
    .unwrap();
    let catalogue = include_str!("../../../web/symbol-catalogue.mjs");
    let mut count = 0;
    let mut previous = None;
    for field in catalogue.split("\"codePoint\":").skip(1) {
        let digits = field
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let value: u32 = digits.parse().unwrap();
        let scalar = char::from_u32(value).unwrap();
        assert_ne!(
            face.glyph_index(scalar),
            0,
            "U+{value:04X} must be covered by the actual engine font"
        );
        assert!(
            previous.is_none_or(|last| value > last),
            "catalogue has ordered unique scalars"
        );
        previous = Some(value);
        count += 1;
    }
    assert!(count > 2000, "the full named font coverage is distributed");
    assert_eq!(
        face.glyph_index('\u{2011}'),
        0,
        "strict cmap coverage does not fabricate a glyph for U+2011"
    );
    for scalar in [
        '\u{2014}', '\u{2013}', '\u{00a0}', '\u{202f}', '©', '®', '™', '§', '¶', '…',
    ] {
        assert_ne!(
            face.glyph_index(scalar),
            0,
            "special U+{:04X}",
            scalar as u32
        );
    }
}
