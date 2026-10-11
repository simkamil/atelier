// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::{FontManager, FontRequest, FontSource, FontStyle, ManagerError};
use atelier_font::{
    FontFace,
    embedded::{EMBEDDED_FONTS, EmbeddedFontStyle},
};

fn regular() -> &'static [u8] {
    EMBEDDED_FONTS
        .iter()
        .find(|f| f.family == "Carlito" && f.weight == 400 && f.style == EmbeddedFontStyle::Normal)
        .unwrap()
        .data
}
fn os2_variant(weight: u16, version: u16, selection: u16) -> Vec<u8> {
    let source = regular();
    let face = FontFace::parse(source).unwrap();
    let table = face.table(*b"OS/2").unwrap();
    let offset = table.as_ptr() as usize - source.as_ptr() as usize;
    let mut bytes = source.to_vec();
    bytes[offset..offset + 2].copy_from_slice(&version.to_be_bytes());
    bytes[offset + 4..offset + 6].copy_from_slice(&weight.to_be_bytes());
    bytes[offset + 62..offset + 64].copy_from_slice(&selection.to_be_bytes());
    bytes
}

#[test]
fn css_search_uses_each_weight_interval_and_exact_match() {
    let weights = [900, 100, 500, 350, 800, 450, 200, 600, 400];
    let bytes: Vec<_> = weights.iter().map(|&w| os2_variant(w, 4, 0)).collect();
    let mut manager = FontManager::empty();
    for data in &bytes {
        manager.register(data, 0, FontSource::Document).unwrap();
    }
    // Explicit expectations from CSS Fonts4, deliberately not computed by
    // a sorting/ranking helper resembling the production implementation.
    for (requested, expected) in [
        (1, 100),
        (199, 100),
        (201, 200),
        (399, 350),
        (400, 400),
        (425, 450),
        (450, 450),
        (475, 500),
        (500, 500),
        (501, 600),
        (601, 800),
        (899, 900),
        (1000, 900),
    ] {
        let found = manager
            .resolve(&FontRequest::new(["Carlito"], requested, FontStyle::Normal))
            .unwrap();
        assert_eq!(found.weight, expected, "weight request {requested}");
    }
}

#[test]
fn oblique_metadata_requires_version_four_and_does_not_come_from_name() {
    let old = os2_variant(400, 3, 1 << 9);
    let oblique = os2_variant(400, 4, 1 << 9);
    let italic = os2_variant(400, 4, 1);
    for (data, expected) in [
        (&old, FontStyle::Normal),
        (&oblique, FontStyle::Oblique),
        (&italic, FontStyle::Italic),
    ] {
        let mut manager = FontManager::empty();
        manager.register(data, 0, FontSource::Document).unwrap();
        let found = manager
            .resolve(&FontRequest::new(["Carlito"], 400, FontStyle::Normal))
            .unwrap();
        assert_eq!(found.style, expected);
    }
}

#[test]
fn invalid_weight_and_face_index_fail_without_consuming_ids_or_bytes() {
    let bad = os2_variant(1001, 4, 0);
    let mut manager = FontManager::empty();
    assert_eq!(
        manager.register(&bad, 0, FontSource::Document),
        Err(ManagerError::InvalidFontWeight(1001))
    );
    assert!(matches!(
        manager.register(regular(), 1, FontSource::System),
        Err(ManagerError::Font(_))
    ));
    assert!(manager.is_empty());
    assert_eq!(manager.registered_bytes(), 0);
    assert_eq!(
        manager
            .register(regular(), 0, FontSource::System)
            .unwrap()
            .get(),
        1
    );
}

#[test]
fn uncovered_family_falls_through_before_source_and_style_matching() {
    let manager = FontManager::new();
    let caladea = EMBEDDED_FONTS
        .iter()
        .find(|f| f.family == "Caladea" && f.weight == 400 && f.style == EmbeddedFontStyle::Normal)
        .unwrap();
    let first = FontFace::parse(caladea.data).unwrap();
    let second = FontFace::parse(regular()).unwrap();
    let character = (0..=0x10ffff)
        .filter_map(char::from_u32)
        .find(|&c| first.glyph_index(c) == 0 && second.glyph_index(c) != 0)
        .unwrap();
    let request = FontRequest::new(["Caladea", "Carlito"], 400, FontStyle::Normal);
    assert_eq!(manager.resolve(&request).unwrap().family, "Caladea");
    let found = manager.resolve_for_char(&request, character).unwrap();
    assert_eq!(found.family, "Carlito");
    assert_ne!(found.face.glyph_index(character), 0);
}
