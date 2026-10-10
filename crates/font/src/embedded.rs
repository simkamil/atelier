// SPDX-License-Identifier: AGPL-3.0-or-later
//! Built-in OFL font faces shipped with Atelier.

/// The slant of an embedded font face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmbeddedFontStyle {
    /// Upright face.
    Normal,
    /// Italic face.
    Italic,
}

/// A statically embedded font and its license notice.
#[derive(Clone, Copy, Debug)]
pub struct EmbeddedFont {
    /// OpenType family name.
    pub family: &'static str,
    /// OpenType weight class (400 regular, 700 bold).
    pub weight: u16,
    /// Upright or italic style.
    pub style: EmbeddedFontStyle,
    /// Original, unmodified TTF bytes.
    pub data: &'static [u8],
    /// Full OFL text, including the font copyright and reserved-name notice.
    pub license: &'static str,
}

/// The complete built-in font catalog, ordered by family then weight and style.
pub static EMBEDDED_FONTS: &[EmbeddedFont] = &[
    EmbeddedFont {
        family: "Carlito",
        weight: 400,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf"),
        license: include_str!("../../../data/fonts/licenses/Carlito-OFL.txt"),
    },
    EmbeddedFont {
        family: "Carlito",
        weight: 400,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/carlito/Carlito-Italic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Carlito-OFL.txt"),
    },
    EmbeddedFont {
        family: "Carlito",
        weight: 700,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/carlito/Carlito-Bold.ttf"),
        license: include_str!("../../../data/fonts/licenses/Carlito-OFL.txt"),
    },
    EmbeddedFont {
        family: "Carlito",
        weight: 700,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/carlito/Carlito-BoldItalic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Carlito-OFL.txt"),
    },
    EmbeddedFont {
        family: "Caladea",
        weight: 400,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/caladea/Caladea-Regular.ttf"),
        license: include_str!("../../../data/fonts/licenses/Caladea-OFL.txt"),
    },
    EmbeddedFont {
        family: "Caladea",
        weight: 400,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/caladea/Caladea-Italic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Caladea-OFL.txt"),
    },
    EmbeddedFont {
        family: "Caladea",
        weight: 700,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/caladea/Caladea-Bold.ttf"),
        license: include_str!("../../../data/fonts/licenses/Caladea-OFL.txt"),
    },
    EmbeddedFont {
        family: "Caladea",
        weight: 700,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/caladea/Caladea-BoldItalic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Caladea-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Sans",
        weight: 400,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSans-Regular.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Sans",
        weight: 400,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSans-Italic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Sans",
        weight: 700,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSans-Bold.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Sans",
        weight: 700,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSans-BoldItalic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Serif",
        weight: 400,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSerif-Regular.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Serif",
        weight: 400,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSerif-Italic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Serif",
        weight: 700,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSerif-Bold.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Serif",
        weight: 700,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationSerif-BoldItalic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Mono",
        weight: 400,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationMono-Regular.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Mono",
        weight: 400,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationMono-Italic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Mono",
        weight: 700,
        style: EmbeddedFontStyle::Normal,
        data: include_bytes!("../../../data/fonts/liberation/LiberationMono-Bold.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
    EmbeddedFont {
        family: "Liberation Mono",
        weight: 700,
        style: EmbeddedFontStyle::Italic,
        data: include_bytes!("../../../data/fonts/liberation/LiberationMono-BoldItalic.ttf"),
        license: include_str!("../../../data/fonts/licenses/Liberation-OFL.txt"),
    },
];
