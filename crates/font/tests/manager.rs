// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{
    FontFace,
    embedded::EMBEDDED_FONTS,
    manager::{
        FontManager, FontManagerLimits, FontRequest, FontSource, FontStyle, GenericFamily,
        ManagerError,
    },
};

const TTC: &[u8] = include_bytes!("fixtures/two-faces.ttc");

fn request(family: &str, weight: u16, style: FontStyle) -> FontRequest {
    FontRequest::new([family], weight, style)
}
fn family_name(face: &FontFace<'_>) -> String {
    let id = if face.names().iter().any(|name| name.name_id == 16) {
        16
    } else {
        1
    };
    face.names()
        .iter()
        .find(|name| name.name_id == id && name.platform_id == 3 && name.language_id == 0x0409)
        .or_else(|| face.names().iter().find(|name| name.name_id == id))
        .unwrap()
        .value
        .clone()
}

#[test]
fn default_manager_registers_all_twenty_embedded_faces_and_alias_variants() {
    let manager = FontManager::new();
    assert_eq!(manager.len(), 20);
    for (alias, embedded_family) in [
        ("Calibri", "Carlito"),
        ("Cambria", "Caladea"),
        ("Arial", "Liberation Sans"),
        ("Times New Roman", "Liberation Serif"),
        ("Courier New", "Liberation Mono"),
    ] {
        for (weight, style) in [
            (400, FontStyle::Normal),
            (400, FontStyle::Italic),
            (700, FontStyle::Normal),
            (700, FontStyle::Italic),
        ] {
            let found = manager.resolve(&request(alias, weight, style)).unwrap();
            assert_eq!(found.family, embedded_family, "{alias} {weight} {style:?}");
            assert_eq!(found.weight, weight);
            assert_eq!(found.style, style);
            assert_eq!(found.source, FontSource::Embedded);
        }
    }
}

#[test]
fn explicit_sources_have_stable_precedence_and_ttc_face_index_is_retained() {
    let mut manager = FontManager::empty();
    let id0 = manager.register(TTC, 0, FontSource::System).unwrap();
    let id1 = manager.register(TTC, 0, FontSource::Document).unwrap();
    let face0 = FontFace::parse_collection(TTC, 0).unwrap();
    let family = family_name(&face0);
    let selected = manager
        .resolve(&request(&family, 400, FontStyle::Normal))
        .unwrap();
    assert_eq!(selected.id, id1);
    assert_eq!(selected.source, FontSource::Document);
    assert_eq!(selected.face_index, 0);
    assert_eq!(id0.get(), 1);
    assert_eq!(id1.get(), 2);
    let face1 = FontFace::parse_collection(TTC, 1).unwrap();
    let family = family_name(&face1);
    let mut index_manager = FontManager::empty();
    let id2 = index_manager.register(TTC, 1, FontSource::System).unwrap();
    let selected = index_manager
        .resolve(&request(&family, 400, FontStyle::Normal))
        .unwrap();
    assert_eq!(selected.id, id2);
    assert_eq!(selected.face_index, 1);
    assert!(matches!(
        index_manager.register(TTC, 2, FontSource::System),
        Err(ManagerError::Font(_))
    ));
}

#[test]
fn css_weight_and_style_proximity_are_deterministic() {
    let manager = FontManager::new();
    let selected = manager
        .resolve(&request("Carlito", 600, FontStyle::Normal))
        .unwrap();
    assert_eq!(selected.weight, 700);
    let selected = manager
        .resolve(&request("Carlito", 450, FontStyle::Normal))
        .unwrap();
    assert_eq!(
        selected.weight, 400,
        "without a 500 face, the search continues downward before considering weights above 500"
    );
    let selected = manager
        .resolve(&request("Carlito", 400, FontStyle::Oblique))
        .unwrap();
    assert_eq!(selected.style, FontStyle::Italic);
    assert_eq!(
        manager
            .resolve(&request("Carlito", 0, FontStyle::Normal))
            .unwrap_err(),
        ManagerError::InvalidRequest
    );
}

#[test]
fn request_inside_400_to_500_prefers_exact_then_searches_upward() {
    fn with_weight(source: &[u8], weight: u16) -> Vec<u8> {
        let mut bytes = source.to_vec();
        let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        for i in 0..tables {
            let record = 12 + i * 16;
            if &bytes[record..record + 4] == b"OS/2" {
                let offset =
                    u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
                bytes[offset + 4..offset + 6].copy_from_slice(&weight.to_be_bytes());
                return bytes;
            }
        }
        panic!("OS/2 table missing")
    }
    let regular = with_weight(EMBEDDED_FONTS[0].data, 450);
    let heavier = with_weight(EMBEDDED_FONTS[0].data, 500);
    let mut manager = FontManager::empty();
    let exact = manager.register(&regular, 0, FontSource::Embedded).unwrap();
    manager.register(&heavier, 0, FontSource::Embedded).unwrap();
    assert_eq!(
        manager
            .resolve(&request("Carlito", 450, FontStyle::Normal))
            .unwrap()
            .id,
        exact
    );
}

#[test]
fn character_resolution_skips_noncovering_faces_and_reports_missing_glyph() {
    let manager = FontManager::new();
    let request = FontRequest::new(["Carlito", "Caladea"], 400, FontStyle::Normal)
        .with_generic(GenericFamily::Serif);
    let selected = manager.resolve_for_char(&request, 'A').unwrap();
    assert_eq!(selected.family, "Carlito");
    let missing = '\u{10ffff}';
    assert_eq!(
        manager.resolve_for_char(&request, missing).unwrap_err(),
        ManagerError::MissingGlyph(missing)
    );
}

#[test]
fn character_resolution_accepts_the_nonbreaking_hyphen_display_fallback() {
    let manager = FontManager::new();
    let request = FontRequest::new(["Carlito", "Caladea"], 400, FontStyle::Normal);
    let selected = manager.resolve_for_char(&request, '\u{2011}').unwrap();
    assert_eq!(selected.family, "Carlito");
    assert_eq!(
        selected.face.glyph_index('\u{2011}'),
        0,
        "strict cmap coverage remains missing"
    );
    assert_ne!(selected.face.display_glyph_index('\u{2011}'), 0);
}

#[test]
fn display_glyph_fallback_prefers_en_dash_and_is_limited_to_nonbreaking_hyphen() {
    let with_en_dash = EMBEDDED_FONTS
        .iter()
        .map(|font| FontFace::parse(font.data).unwrap())
        .find(|face| face.glyph_index('\u{2011}') == 0 && face.glyph_index('\u{2010}') != 0)
        .expect("an embedded face covers en dash but lacks non-breaking hyphen");
    assert_eq!(
        with_en_dash.display_glyph_index('\u{2011}'),
        with_en_dash.glyph_index('\u{2010}')
    );
    let carlito = EMBEDDED_FONTS
        .iter()
        .find(|font| {
            font.family == "Carlito"
                && font.weight == 400
                && font.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap();
    let face = FontFace::parse(carlito.data).unwrap();
    assert_eq!(face.glyph_index('\u{2011}'), 0);
    assert_eq!(face.glyph_index('\u{2010}'), 0);
    assert_eq!(face.display_glyph_index('\u{2011}'), face.glyph_index('-'));
    assert_eq!(
        face.display_glyph_index('\u{2012}'),
        face.glyph_index('\u{2012}')
    );
}

#[test]
fn aliases_are_case_and_whitespace_insensitive_and_generic_fallback_works() {
    let manager = FontManager::new();
    assert_eq!(
        manager
            .resolve(&request("  TIMES   NEW ROMAN ", 400, FontStyle::Normal))
            .unwrap()
            .family,
        "Liberation Serif"
    );
    let mut empty_named = FontManager::empty();
    let id = empty_named
        .register(EMBEDDED_FONTS[0].data, 0, FontSource::System)
        .unwrap();
    let request = FontRequest::new(["missing"], 400, FontStyle::Normal)
        .with_generic(GenericFamily::Monospace);
    assert_eq!(empty_named.resolve(&request).unwrap().id, id);
}

#[test]
fn english_windows_family_display_is_stable_when_name_records_are_reordered() {
    fn conflicting_names(reverse: bool) -> Vec<u8> {
        let mut bytes = EMBEDDED_FONTS[0].data.to_vec();
        let table_count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let name_offset = (0..table_count)
            .map(|i| 12 + i * 16)
            .find(|&record| &bytes[record..record + 4] == b"name")
            .map(|record| {
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize
            })
            .unwrap();
        let record_count =
            u16::from_be_bytes(bytes[name_offset + 2..name_offset + 4].try_into().unwrap())
                as usize;
        let family = (0..record_count)
            .map(|i| name_offset + 6 + i * 12)
            .find(|&record| {
                u16::from_be_bytes(bytes[record..record + 2].try_into().unwrap()) == 3
                    && u16::from_be_bytes(bytes[record + 4..record + 6].try_into().unwrap())
                        == 0x0409
                    && u16::from_be_bytes(bytes[record + 6..record + 8].try_into().unwrap()) == 1
            })
            .unwrap();
        let subfamily = (0..record_count)
            .map(|i| name_offset + 6 + i * 12)
            .find(|&record| {
                u16::from_be_bytes(bytes[record..record + 2].try_into().unwrap()) == 3
                    && u16::from_be_bytes(bytes[record + 4..record + 6].try_into().unwrap())
                        == 0x0409
                    && u16::from_be_bytes(bytes[record + 6..record + 8].try_into().unwrap()) == 2
            })
            .unwrap();
        bytes[subfamily + 6..subfamily + 8].copy_from_slice(&1u16.to_be_bytes());
        if reverse {
            let a: [u8; 12] = bytes[family..family + 12].try_into().unwrap();
            let b: [u8; 12] = bytes[subfamily..subfamily + 12].try_into().unwrap();
            bytes[family..family + 12].copy_from_slice(&b);
            bytes[subfamily..subfamily + 12].copy_from_slice(&a);
        }
        bytes
    }
    let first = conflicting_names(false);
    let reversed = conflicting_names(true);
    for bytes in [&first, &reversed] {
        let mut manager = FontManager::empty();
        manager.register(bytes, 0, FontSource::System).unwrap();
        assert_eq!(
            manager
                .resolve(&request("Carlito", 400, FontStyle::Normal))
                .unwrap()
                .family,
            "Carlito"
        );
    }
}

#[test]
fn absent_generic_uses_best_style_and_weight_in_first_covering_family() {
    let regular = EMBEDDED_FONTS
        .iter()
        .find(|font| {
            font.family == "Carlito"
                && font.weight == 400
                && font.style == atelier_font::embedded::EmbeddedFontStyle::Normal
        })
        .unwrap();
    let bold_italic = EMBEDDED_FONTS
        .iter()
        .find(|font| {
            font.family == "Carlito"
                && font.weight == 700
                && font.style == atelier_font::embedded::EmbeddedFontStyle::Italic
        })
        .unwrap();
    let mut manager = FontManager::empty();
    manager
        .register(regular.data, 0, FontSource::Embedded)
        .unwrap();
    let expected = manager
        .register(bold_italic.data, 0, FontSource::Document)
        .unwrap();
    let request = FontRequest::new(["missing"], 700, FontStyle::Italic)
        .with_generic(GenericFamily::Monospace);
    let resolved = manager.resolve(&request).unwrap();
    assert_eq!(resolved.family, "Carlito");
    assert_eq!(resolved.id, expected);
}

#[test]
fn rejected_registrations_are_atomic_and_removed_ids_are_not_reused() {
    let bytes = EMBEDDED_FONTS[0].data;
    let mut manager = FontManager::with_limits(FontManagerLimits {
        max_faces: 2,
        max_total_bytes: bytes.len(),
    });
    let first = manager.register(bytes, 0, FontSource::System).unwrap();
    assert_eq!(
        manager.register(bytes, 0, FontSource::Document),
        Err(ManagerError::ByteLimitExceeded)
    );
    assert_eq!(manager.len(), 1);
    assert_eq!(
        manager.register(b"invalid", 0, FontSource::Document),
        Err(ManagerError::ByteLimitExceeded)
    );
    assert_eq!(
        manager.register(bytes, 1, FontSource::Document),
        Err(ManagerError::ByteLimitExceeded)
    );
    manager.remove(first).unwrap();
    assert_eq!(
        manager
            .register(bytes, 0, FontSource::Document)
            .unwrap()
            .get(),
        2
    );
    let mut face_limited = FontManager::with_limits(FontManagerLimits {
        max_faces: 1,
        max_total_bytes: usize::MAX,
    });
    face_limited
        .register(bytes, 0, FontSource::Embedded)
        .unwrap();
    assert_eq!(
        face_limited.register(bytes, 0, FontSource::Embedded),
        Err(ManagerError::FaceLimitExceeded)
    );
    let mut malformed = FontManager::empty();
    assert!(matches!(
        malformed.register(b"invalid", 0, FontSource::Document),
        Err(ManagerError::Font(_))
    ));
    assert_eq!(malformed.len(), 0);
}

#[test]
fn empty_registry_and_invalid_requests_have_typed_errors() {
    let manager = FontManager::empty();
    assert_eq!(
        manager
            .resolve(&request("Arial", 400, FontStyle::Normal))
            .unwrap_err(),
        ManagerError::EmptyRegistry
    );
    assert_eq!(
        manager
            .resolve(&FontRequest {
                families: vec![],
                weight: 400,
                style: FontStyle::Normal,
                generic: None
            })
            .unwrap_err(),
        ManagerError::InvalidRequest
    );
}
