// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_model::Transaction;
use atelier_text_model::rich::{
    CharAttrs, LineRule, Node, ParaProps, RichOp, StyleKind, default_rich_document,
};

#[test]
fn factory_builds_resolvable_default_document_and_exact_metadata() {
    let mut value = default_rich_document();
    assert!(value.document.validate().is_ok());
    assert_eq!(value.document.paragraph_ids().len(), 1);
    let paragraph_id = value.document.paragraph_ids()[0];
    let paragraph = value.document.paragraph(paragraph_id).unwrap();
    assert!(paragraph.text.is_empty());
    assert_eq!(paragraph.props.style.as_deref(), Some("atelier.normal"));

    let resolved = value
        .styles
        .resolve(&value.document, paragraph_id, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(resolved.character.font.as_deref(), Some("Carlito"));
    assert_eq!(resolved.character.size_half_points, Some(22));
    assert_eq!(resolved.paragraph.space_after, Some(160));
    assert_eq!(
        resolved.paragraph.line_spacing.unwrap().rule,
        LineRule::Auto
    );
    assert_eq!(resolved.paragraph.line_spacing.unwrap().value, 259);
    assert_eq!(value.presentation.exact_line_spacing_numerator, 108);
    assert_eq!(value.presentation.exact_line_spacing_denominator, 100);
    assert_eq!(value.presentation.page_width_hundredths_mm, 21000);
    assert_eq!(value.presentation.page_height_hundredths_mm, 29700);
    assert_eq!(
        [
            value.presentation.margin_top_hundredths_mm,
            value.presentation.margin_right_hundredths_mm,
            value.presentation.margin_bottom_hundredths_mm,
            value.presentation.margin_left_hundredths_mm
        ],
        [2500; 4]
    );
    assert!(value.presentation.portrait);
    assert_eq!(value.presentation.language, "fr-FR");
}

#[test]
fn catalog_has_all_required_french_names_kinds_and_valid_next_targets() {
    let mut value = default_rich_document();
    let expected = [
        ("atelier.normal", "Normal", StyleKind::Paragraph),
        ("atelier.title", "Titre", StyleKind::Paragraph),
        ("atelier.subtitle", "Sous-titre", StyleKind::Paragraph),
        ("atelier.heading.1", "Titre 1", StyleKind::Paragraph),
        ("atelier.heading.2", "Titre 2", StyleKind::Paragraph),
        ("atelier.heading.3", "Titre 3", StyleKind::Paragraph),
        ("atelier.heading.4", "Titre 4", StyleKind::Paragraph),
        ("atelier.heading.5", "Titre 5", StyleKind::Paragraph),
        ("atelier.heading.6", "Titre 6", StyleKind::Paragraph),
        ("atelier.heading.7", "Titre 7", StyleKind::Paragraph),
        ("atelier.heading.8", "Titre 8", StyleKind::Paragraph),
        ("atelier.heading.9", "Titre 9", StyleKind::Paragraph),
        ("atelier.quote", "Citation", StyleKind::Paragraph),
        (
            "atelier.quote.intense",
            "Citation intense",
            StyleKind::Paragraph,
        ),
        ("atelier.emphasis", "Accentuation", StyleKind::Character),
        ("atelier.strong", "Élevé", StyleKind::Character),
        ("atelier.caption", "Légende", StyleKind::Paragraph),
        ("atelier.header", "En-tête", StyleKind::Paragraph),
        ("atelier.footer", "Pied de page", StyleKind::Paragraph),
        ("atelier.hyperlink", "Lien hypertexte", StyleKind::Character),
        (
            "atelier.list.paragraph",
            "Paragraphe de liste",
            StyleKind::Paragraph,
        ),
        (
            "atelier.no_spacing",
            "Sans interligne",
            StyleKind::Paragraph,
        ),
        ("atelier.table.grid", "Grille du tableau", StyleKind::Table),
    ];
    assert_eq!(value.style_names.len(), expected.len());
    assert_eq!(value.next_styles.len(), expected.len());
    for (id, name, kind) in expected {
        assert_eq!(value.style_names.get(id).map(String::as_str), Some(name));
        assert_eq!(value.styles.catalog().style(id).unwrap().kind, kind);
        assert!(
            value
                .styles
                .catalog()
                .style(value.next_styles.get(id).unwrap())
                .is_some()
        );
    }
    for id in [
        "atelier.title",
        "atelier.heading.1",
        "atelier.heading.2",
        "atelier.heading.9",
    ] {
        assert_eq!(value.next_styles[id], "atelier.normal");
    }
    let paragraph_id = value.document.paragraph_ids()[0];
    for level in 1..=9 {
        let id = format!("atelier.heading.{level}");
        assert_eq!(value.next_styles[&id], "atelier.normal");
        value
            .document
            .apply(
                &Transaction::new(vec![RichOp::SetParaProps {
                    para: paragraph_id,
                    set: ParaProps {
                        style: Some(id),
                        ..ParaProps::default()
                    },
                    clear: ParaProps::default(),
                }])
                .unwrap(),
            )
            .unwrap();
        let resolved = value
            .styles
            .resolve(&value.document, paragraph_id, None, &CharAttrs::default())
            .unwrap();
        assert_eq!(resolved.character.bold, Some(true), "heading level {level}");
    }
    assert_eq!(value.next_styles["atelier.header"], "atelier.header");
    assert_eq!(value.next_styles["atelier.footer"], "atelier.footer");
    assert_eq!(
        value.next_styles["atelier.list.paragraph"],
        "atelier.list.paragraph"
    );
}

#[test]
fn no_spacing_is_independent_and_factory_instances_do_not_share_mutations() {
    let mut first = default_rich_document();
    let second = default_rich_document();
    let no_spacing = first.styles.catalog().style("atelier.no_spacing").unwrap();
    assert_eq!(no_spacing.paragraph.line_spacing.unwrap().value, 240);
    assert_eq!(no_spacing.paragraph.space_after, Some(0));
    let paragraph_id = first.document.paragraph_ids()[0];
    first
        .document
        .apply(
            &Transaction::new(vec![RichOp::SetParaProps {
                para: paragraph_id,
                set: ParaProps {
                    style: Some("atelier.no_spacing".into()),
                    ..ParaProps::default()
                },
                clear: ParaProps::default(),
            }])
            .unwrap(),
        )
        .unwrap();
    let resolved = first
        .styles
        .resolve(&first.document, paragraph_id, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(resolved.paragraph.line_spacing.unwrap().value, 240);
    assert_eq!(resolved.paragraph.space_after, Some(0));
    assert_eq!(
        second
            .styles
            .catalog()
            .style("atelier.normal")
            .unwrap()
            .paragraph
            .space_after,
        None
    );
    assert_eq!(second.document.paragraph_ids().len(), 1);
    assert_eq!(first.style_names, second.style_names);
    assert_eq!(first.next_styles, second.next_styles);
    first.styles.remove_style("atelier.normal");
    assert!(second.styles.catalog().style("atelier.normal").is_some());
    assert!(first.styles.catalog().style("atelier.normal").is_none());
    assert!(matches!(
        second.document.node(second.document.section_ids()[0]),
        Some(Node::Section(_))
    ));
}

#[test]
fn factory_styles_carry_word_names_gallery_flags_and_one_default() {
    let value = default_rich_document();
    let catalog = value.styles.catalog();
    let defaults: Vec<_> = catalog
        .styles()
        .filter(|s| s.is_default)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(defaults, ["atelier.normal"]);
    for (id, name, level, quick) in [
        ("atelier.normal", "Normal", 0, true),
        ("atelier.heading.1", "heading 1", 1, true),
        ("atelier.heading.3", "heading 3", 3, true),
        ("atelier.heading.4", "heading 4", 4, false),
        ("atelier.title", "Title", 0, true),
        ("atelier.hyperlink", "Hyperlink", 0, false),
    ] {
        let style = catalog.style(id).unwrap();
        assert_eq!(style.name.as_deref(), Some(name), "{id}");
        assert_eq!((style.heading_level(), style.quick), (level, quick), "{id}");
        assert!(style.is_builtin(), "{id}");
    }
    // A user style is neither built in nor a heading.
    let custom = atelier_text_model::rich::Style {
        id: "Mine".into(),
        name: Some("Mon style".into()),
        ..Default::default()
    };
    assert!(!custom.is_builtin() && custom.heading_level() == 0);
    assert_eq!(custom.display_name(), "Mon style");
    // Every style of the profile has a name and the table style is excluded from paragraph styles.
    assert!(catalog.styles().all(|s| s.name.is_some()));
    assert_eq!(
        catalog
            .styles()
            .filter(|s| s.kind == StyleKind::Table)
            .count(),
        1
    );
}
