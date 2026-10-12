// SPDX-License-Identifier: AGPL-3.0-or-later
//! Atelier's default rich-document profile (US-202).

use super::{
    CharAttrs, DocumentStyleDefaults, LineRule, LineSpacing, ParaProps, RichDocument, RichOp,
    Style, StyleCatalog, StyleCharAttrs, StyleEngine, StyleKind, Theme,
};
use atelier_model::Transaction;
use std::collections::BTreeMap;

/// Exact page and language metadata for the default document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefaultPresentation {
    pub page_width_hundredths_mm: u32,
    pub page_height_hundredths_mm: u32,
    pub margin_top_hundredths_mm: u32,
    pub margin_right_hundredths_mm: u32,
    pub margin_bottom_hundredths_mm: u32,
    pub margin_left_hundredths_mm: u32,
    pub portrait: bool,
    pub language: &'static str,
    /// Exact default requested by US-202, retained separately from twip Auto.
    pub exact_line_spacing_numerator: u32,
    pub exact_line_spacing_denominator: u32,
}

/// Default rich document, its style resolver, and metadata not represented by
/// the existing document tree. `next_styles` maps stable style IDs to IDs.
#[derive(Clone, Debug)]
pub struct DefaultRichDocument {
    pub document: RichDocument,
    pub styles: StyleEngine,
    pub presentation: DefaultPresentation,
    pub next_styles: BTreeMap<String, String>,
    pub style_names: BTreeMap<String, String>,
}

const NORMAL: &str = "atelier.normal";
const TITLE: &str = "atelier.title";
const SUBTITLE: &str = "atelier.subtitle";
const HEADING: [&str; 9] = [
    "atelier.heading.1",
    "atelier.heading.2",
    "atelier.heading.3",
    "atelier.heading.4",
    "atelier.heading.5",
    "atelier.heading.6",
    "atelier.heading.7",
    "atelier.heading.8",
    "atelier.heading.9",
];

/// Creates an independent and deterministic default document profile.
pub fn default_rich_document() -> DefaultRichDocument {
    let auto_108 = LineSpacing {
        rule: LineRule::Auto,
        value: 259,
    };
    let mut defaults = DocumentStyleDefaults::default();
    defaults.character.values.font = Some("Carlito".into());
    defaults.character.values.size_half_points = Some(22);
    defaults.paragraph.line_spacing = Some(auto_108);
    defaults.paragraph.space_after = Some(160);
    let catalog = StyleCatalog::new(defaults);
    let mut styles = StyleEngine::new(catalog, Theme::default(), 128);

    let mut next_styles = BTreeMap::new();
    let mut style_names = BTreeMap::new();
    let mut add = |id: &str,
                   name: &str,
                   kind: StyleKind,
                   based_on: Option<&str>,
                   next: &str,
                   character: CharAttrs,
                   paragraph: ParaProps| {
        let style = Style {
            id: id.into(),
            kind,
            based_on: based_on.map(str::to_owned),
            character: StyleCharAttrs {
                values: character,
                ..StyleCharAttrs::default()
            },
            paragraph,
            ..Style::default()
        };
        styles.set_style(style).expect("built-in styles are valid");
        style_names.insert(id.into(), name.into());
        next_styles.insert(id.into(), next.into());
    };

    add(
        NORMAL,
        "Normal",
        StyleKind::Paragraph,
        None,
        NORMAL,
        CharAttrs::default(),
        ParaProps::default(),
    );
    add(
        TITLE,
        "Titre",
        StyleKind::Paragraph,
        Some(NORMAL),
        NORMAL,
        CharAttrs {
            bold: Some(true),
            size_half_points: Some(36),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    add(
        SUBTITLE,
        "Sous-titre",
        StyleKind::Paragraph,
        Some(NORMAL),
        NORMAL,
        CharAttrs {
            italic: Some(true),
            size_half_points: Some(24),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    for (index, id) in HEADING.iter().enumerate() {
        add(
            id,
            &format!("Titre {}", index + 1),
            StyleKind::Paragraph,
            Some(NORMAL),
            NORMAL,
            CharAttrs {
                bold: Some(true),
                size_half_points: Some((32_u32.saturating_sub(index as u32 * 2)).max(20)),
                ..CharAttrs::default()
            },
            ParaProps::default(),
        );
    }

    let quote = ParaProps {
        indent_left: Some(360),
        indent_right: Some(360),
        ..ParaProps::default()
    };
    add(
        "atelier.quote",
        "Citation",
        StyleKind::Paragraph,
        Some(NORMAL),
        NORMAL,
        CharAttrs::default(),
        quote.clone(),
    );
    add(
        "atelier.quote.intense",
        "Citation intense",
        StyleKind::Paragraph,
        Some("atelier.quote"),
        NORMAL,
        CharAttrs {
            bold: Some(true),
            italic: Some(true),
            ..CharAttrs::default()
        },
        quote,
    );
    add(
        "atelier.emphasis",
        "Accentuation",
        StyleKind::Character,
        None,
        "atelier.emphasis",
        CharAttrs {
            italic: Some(true),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    add(
        "atelier.strong",
        "Élevé",
        StyleKind::Character,
        None,
        "atelier.strong",
        CharAttrs {
            bold: Some(true),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    add(
        "atelier.caption",
        "Légende",
        StyleKind::Paragraph,
        Some(NORMAL),
        NORMAL,
        CharAttrs {
            size_half_points: Some(20),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    add(
        "atelier.header",
        "En-tête",
        StyleKind::Paragraph,
        Some(NORMAL),
        "atelier.header",
        CharAttrs::default(),
        ParaProps::default(),
    );
    add(
        "atelier.footer",
        "Pied de page",
        StyleKind::Paragraph,
        Some(NORMAL),
        "atelier.footer",
        CharAttrs::default(),
        ParaProps::default(),
    );
    add(
        "atelier.hyperlink",
        "Lien hypertexte",
        StyleKind::Character,
        None,
        "atelier.hyperlink",
        CharAttrs {
            underline: Some(super::Underline::Single),
            color: Some(super::Color {
                r: 0,
                g: 102,
                b: 204,
            }),
            ..CharAttrs::default()
        },
        ParaProps::default(),
    );
    add(
        "atelier.list.paragraph",
        "Paragraphe de liste",
        StyleKind::Paragraph,
        Some(NORMAL),
        "atelier.list.paragraph",
        CharAttrs::default(),
        ParaProps::default(),
    );
    add(
        "atelier.no_spacing",
        "Sans interligne",
        StyleKind::Paragraph,
        Some(NORMAL),
        NORMAL,
        CharAttrs::default(),
        ParaProps {
            line_spacing: Some(LineSpacing {
                rule: LineRule::Auto,
                value: 240,
            }),
            space_after: Some(0),
            ..ParaProps::default()
        },
    );
    add(
        "atelier.table.grid",
        "Grille du tableau",
        StyleKind::Table,
        None,
        "atelier.table.grid",
        CharAttrs::default(),
        ParaProps::default(),
    );

    let mut document = RichDocument::new();
    let paragraph = document.paragraph_ids()[0];
    document
        .apply(
            &Transaction::new(vec![RichOp::SetParaProps {
                para: paragraph,
                set: ParaProps {
                    style: Some(NORMAL.into()),
                    ..ParaProps::default()
                },
                clear: ParaProps::default(),
            }])
            .expect("one valid operation"),
        )
        .expect("the initial paragraph exists");

    DefaultRichDocument {
        document,
        styles,
        presentation: DefaultPresentation {
            page_width_hundredths_mm: 21000,
            page_height_hundredths_mm: 29700,
            margin_top_hundredths_mm: 2500,
            margin_right_hundredths_mm: 2500,
            margin_bottom_hundredths_mm: 2500,
            margin_left_hundredths_mm: 2500,
            portrait: true,
            language: "fr-FR",
            exact_line_spacing_numerator: 108,
            exact_line_spacing_denominator: 100,
        },
        next_styles,
        style_names,
    }
}
