// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{
    FontFace,
    embedded::{EMBEDDED_FONTS, EmbeddedFontStyle},
};
use std::collections::BTreeSet;

const FAMILIES: [&str; 5] = [
    "Carlito",
    "Caladea",
    "Liberation Sans",
    "Liberation Serif",
    "Liberation Mono",
];

#[test]
fn catalog_has_each_family_weight_and_slant_once() {
    assert_eq!(EMBEDDED_FONTS.len(), 20);
    let mut entries = BTreeSet::new();
    for font in EMBEDDED_FONTS {
        assert!(FAMILIES.contains(&font.family));
        assert!(matches!(font.weight, 400 | 700));
        assert!(!font.data.is_empty());
        assert!(entries.insert((
            font.family,
            font.weight,
            font.style == EmbeddedFontStyle::Italic
        )));
    }
    assert_eq!(entries.len(), 20);
    for family in FAMILIES {
        for weight in [400, 700] {
            for style in [EmbeddedFontStyle::Normal, EmbeddedFontStyle::Italic] {
                assert!(entries.contains(&(family, weight, style == EmbeddedFontStyle::Italic)));
            }
        }
    }
}

#[test]
fn every_face_parses_and_matches_catalog_metadata() {
    for embedded in EMBEDDED_FONTS {
        let face = FontFace::parse(embedded.data).unwrap_or_else(|error| {
            panic!(
                "{} {:?} {}: {error}",
                embedded.family, embedded.style, embedded.weight
            )
        });
        assert_eq!(face.metrics().weight_class, Some(embedded.weight));
        let expected_family = embedded.family;
        assert!(
            face.names()
                .iter()
                .any(|name| { matches!(name.name_id, 1 | 16) && name.value == expected_family }),
            "family name mismatch for {}",
            embedded.family
        );
        let italic = embedded.style == EmbeddedFontStyle::Italic;
        assert!(
            face.names().iter().any(|name| {
                name.name_id == 2
                    && if italic {
                        name.value.contains("Italic")
                    } else {
                        name.value == "Regular" || name.value == "Bold"
                    }
            }),
            "subfamily name mismatch for {} {:?}",
            embedded.family,
            embedded.style
        );
    }
}

#[test]
fn each_catalog_entry_exposes_its_complete_license_notice() {
    for embedded in EMBEDDED_FONTS {
        assert!(
            embedded
                .license
                .contains("SIL OPEN FONT LICENSE Version 1.1")
        );
        assert!(embedded.license.contains("Copyright"));
        assert!(embedded.license.contains("PERMISSION & CONDITIONS"));
        assert!(embedded.license.contains("DISCLAIMER"));
        match embedded.family {
            "Carlito" => assert!(embedded.license.contains("Reserved Font Name \"Carlito\"")),
            "Caladea" => assert!(embedded.license.contains("Caladea Project Authors")),
            family if family.starts_with("Liberation ") => {
                assert!(embedded.license.contains("Reserved Font Name Liberation"))
            }
            _ => unreachable!(),
        }
    }
}
