// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_model::Transaction;
use atelier_text_model::rich::{
    CharAttrs, Color, DocumentStyleDefaults, Node, ParaProps, RichDocument, RichOp, Style,
    StyleCatalog, StyleCharAttrs, StyleEngine, StyleError, StyleKind, Theme, ThemeColor, ThemeFont,
    Underline,
};

fn style(id: &str, kind: StyleKind) -> Style {
    Style {
        id: id.to_owned(),
        kind,
        ..Style::default()
    }
}

fn transaction(op: RichOp) -> Transaction<RichOp> {
    Transaction::new(vec![op]).unwrap()
}

fn paragraph_document() -> (RichDocument, atelier_text_model::rich::NodeId) {
    let document = RichDocument::new();
    let paragraph = document.paragraph_ids()[0];
    (document, paragraph)
}

#[test]
fn resolves_independent_properties_across_full_cascade_and_linked_style() {
    let mut defaults = DocumentStyleDefaults::default();
    defaults.character.values.bold = Some(false);
    defaults.paragraph.indent_left = Some(120);
    let catalog = StyleCatalog::new(defaults);
    let mut engine = StyleEngine::new(catalog, Theme::default(), 8);

    let mut table = style("table", StyleKind::Table);
    table.character.values.italic = Some(true);
    table.paragraph.alignment = Some(atelier_text_model::rich::Alignment::Center);
    engine.set_style(table).unwrap();
    let mut base = style("base", StyleKind::Paragraph);
    base.character.values.strike = Some(true);
    base.paragraph.space_before = Some(80);
    engine.set_style(base).unwrap();
    let mut heading = style("heading", StyleKind::Paragraph);
    heading.based_on = Some("base".into());
    heading.linked_style = Some("heading-char".into());
    heading.paragraph.space_after = Some(160);
    engine.set_style(heading).unwrap();
    let mut linked = style("heading-char", StyleKind::Character);
    linked.character.values.size_half_points = Some(28);
    engine.set_style(linked).unwrap();
    let mut run = style("run", StyleKind::Character);
    run.character.values.underline = Some(Underline::None);
    run.character.theme_color = Some(ThemeColor::Accent1);
    engine.set_style(run).unwrap();

    let mut document = RichDocument::new();
    let section = document.section_ids()[0];
    let table_branch = document.new_table(1, 1).unwrap();
    let table_id = table_branch.root;
    document
        .apply(&transaction(RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: table_branch,
        }))
        .unwrap();
    document
        .apply(&transaction(RichOp::SetTableStyle {
            table: table_id,
            style: Some("table".into()),
        }))
        .unwrap();
    let paragraph = document.paragraph_ids().last().copied().unwrap();
    let props = ParaProps {
        style: Some("heading".into()),
        ..ParaProps::default()
    };
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: props,
            clear: ParaProps::default(),
        }))
        .unwrap();

    let theme_color = Color {
        r: 30,
        g: 90,
        b: 210,
    };
    let theme = Theme {
        colors: [(ThemeColor::Accent1, theme_color)].into_iter().collect(),
        fonts: [(ThemeFont::Major, "Atelier Sans".to_owned())]
            .into_iter()
            .collect(),
    };
    engine.set_theme(theme).unwrap();
    let direct = CharAttrs {
        bold: Some(true),
        size_half_points: Some(22),
        ..CharAttrs::default()
    };
    let resolved = engine
        .resolve(&document, paragraph, Some("run"), &direct)
        .unwrap();

    assert_eq!(resolved.character.bold, Some(true)); // direct overrides default false
    assert_eq!(resolved.character.italic, Some(true)); // table style
    assert_eq!(resolved.character.strike, Some(true)); // basedOn paragraph style
    assert_eq!(resolved.character.size_half_points, Some(22)); // direct over linked style
    assert_eq!(resolved.character.underline, Some(Underline::None)); // explicit clear value
    assert_eq!(resolved.character.color, Some(theme_color));
    assert_eq!(
        resolved.paragraph.alignment,
        Some(atelier_text_model::rich::Alignment::Center)
    );
    assert_eq!(resolved.paragraph.indent_left, Some(120));
    assert_eq!(resolved.paragraph.space_before, Some(80));
    assert_eq!(resolved.paragraph.space_after, Some(160));
    assert_eq!(resolved.paragraph.style, None);
}

#[test]
fn theme_font_and_color_references_resolve_to_concrete_values() {
    let defaults = DocumentStyleDefaults {
        character: StyleCharAttrs {
            theme_font: Some(ThemeFont::Major),
            theme_color: Some(ThemeColor::Hyperlink),
            ..StyleCharAttrs::default()
        },
        ..DocumentStyleDefaults::default()
    };
    let theme = Theme {
        colors: [(ThemeColor::Hyperlink, Color { r: 1, g: 2, b: 3 })]
            .into_iter()
            .collect(),
        fonts: [(ThemeFont::Major, "Carlito".into())].into_iter().collect(),
    };
    let mut engine = StyleEngine::new(StyleCatalog::new(defaults), theme, 4);
    let (document, paragraph) = paragraph_document();
    let resolved = engine
        .resolve(&document, paragraph, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(resolved.character.font.as_deref(), Some("Carlito"));
    assert_eq!(resolved.character.color, Some(Color { r: 1, g: 2, b: 3 }));
}

#[test]
fn missing_styles_cycles_and_missing_theme_slots_are_typed_and_bounded() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 4);
    let (mut document, paragraph) = paragraph_document();
    let props = ParaProps {
        style: Some("missing".into()),
        ..ParaProps::default()
    };
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: props,
            clear: ParaProps::default(),
        }))
        .unwrap();
    assert_eq!(
        engine.resolve(&document, paragraph, None, &CharAttrs::default()),
        Err(StyleError::MissingStyle("missing".into()))
    );
    assert_eq!(engine.cache_len(), 0);

    let mut a = style("a", StyleKind::Paragraph);
    a.based_on = Some("b".into());
    let mut b = style("b", StyleKind::Paragraph);
    b.based_on = Some("a".into());
    engine.set_style(a).unwrap();
    engine.set_style(b).unwrap();
    let props = ParaProps {
        style: Some("a".into()),
        ..ParaProps::default()
    };
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: props,
            clear: ParaProps::default(),
        }))
        .unwrap();
    assert_eq!(
        engine.resolve(&document, paragraph, None, &CharAttrs::default()),
        Err(StyleError::StyleCycle("a".into()))
    );

    engine.remove_style("a");
    engine.remove_style("b");
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: ParaProps::default(),
            clear: ParaProps {
                style: Some("a".into()),
                ..ParaProps::default()
            },
        }))
        .unwrap();
    let mut default = DocumentStyleDefaults::default();
    default.character.theme_color = Some(ThemeColor::Accent2);
    engine.set_defaults(default);
    assert_eq!(
        engine.resolve(&document, paragraph, None, &CharAttrs::default()),
        Err(StyleError::MissingThemeColor(ThemeColor::Accent2))
    );
}

#[test]
fn linked_style_pair_is_selected_for_the_application_target_and_missing_link_is_ignored() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 8);
    let mut paragraph_style = style("heading", StyleKind::Paragraph);
    paragraph_style.linked_style = Some("heading-char".into());
    paragraph_style.character.values.bold = Some(true);
    engine.set_style(paragraph_style).unwrap();
    let mut character_style = style("heading-char", StyleKind::Character);
    character_style.character.values.italic = Some(true);
    engine.set_style(character_style).unwrap();

    let (mut document, paragraph) = paragraph_document();
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: ParaProps {
                style: Some("heading".into()),
                ..ParaProps::default()
            },
            clear: ParaProps::default(),
        }))
        .unwrap();

    assert_eq!(engine.linked_style_id("heading"), Some("heading-char"));
    let paragraph_result = engine
        .resolve(&document, paragraph, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(paragraph_result.character.bold, Some(true));
    assert_eq!(paragraph_result.character.italic, None);
    let linked_id = engine.linked_style_id("heading").map(str::to_owned);
    let run_result = engine
        .resolve(
            &document,
            paragraph,
            linked_id.as_deref(),
            &CharAttrs::default(),
        )
        .unwrap();
    assert_eq!(run_result.character.italic, Some(true));

    engine.remove_style("heading-char");
    assert_eq!(engine.linked_style_id("heading"), None);
    assert!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .is_ok()
    );
}

#[test]
fn toggle_values_in_based_on_chain_inherit_and_child_values_reset_them() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 4);
    let mut parent = style("parent", StyleKind::Paragraph);
    parent.character.values.bold = Some(true);
    parent.character.values.italic = Some(true);
    parent.character.values.strike = Some(true);
    engine.set_style(parent).unwrap();
    let mut child = style("child", StyleKind::Paragraph);
    child.based_on = Some("parent".into());
    child.character.values.bold = Some(false);
    child.character.values.strike = Some(false);
    engine.set_style(child).unwrap();

    let (mut document, paragraph) = paragraph_document();
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: ParaProps {
                style: Some("child".into()),
                ..ParaProps::default()
            },
            clear: ParaProps::default(),
        }))
        .unwrap();
    let inherited = engine
        .resolve(&document, paragraph, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(inherited.character.bold, Some(false));
    assert_eq!(inherited.character.italic, Some(true));
    assert_eq!(inherited.character.strike, Some(false));

    let direct_off = CharAttrs {
        italic: Some(false),
        ..CharAttrs::default()
    };
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &direct_off)
            .unwrap()
            .character
            .italic,
        Some(false)
    );
}

#[test]
fn toggle_levels_follow_word_table_paragraph_character_and_direct_rules() {
    for default in [false, true] {
        for table_value in [false, true] {
            for paragraph_value in [None, Some(false), Some(true)] {
                for character_value in [false, true] {
                    for direct_value in [None, Some(false), Some(true)] {
                        let defaults = DocumentStyleDefaults {
                            table_style: Some("table".into()),
                            character: StyleCharAttrs {
                                values: CharAttrs {
                                    bold: Some(default),
                                    ..CharAttrs::default()
                                },
                                ..StyleCharAttrs::default()
                            },
                            ..DocumentStyleDefaults::default()
                        };
                        let mut engine =
                            StyleEngine::new(StyleCatalog::new(defaults), Theme::default(), 4);
                        let mut table = style("table", StyleKind::Table);
                        table.character.values.bold = Some(table_value);
                        engine.set_style(table).unwrap();
                        let mut paragraph_style = style("paragraph", StyleKind::Paragraph);
                        paragraph_style.character.values.bold = paragraph_value;
                        engine.set_style(paragraph_style).unwrap();
                        let mut character_style = style("character", StyleKind::Character);
                        character_style.character.values.bold = Some(character_value);
                        engine.set_style(character_style).unwrap();

                        let mut document = RichDocument::new();
                        let section = document.section_ids()[0];
                        let table_branch = document.new_table(1, 1).unwrap();
                        document
                            .apply(&transaction(RichOp::InsertBlock {
                                parent: Some(section),
                                position: 1,
                                subtree: table_branch,
                            }))
                            .unwrap();
                        let paragraph = *document.paragraph_ids().last().unwrap();
                        if paragraph_value.is_some() {
                            document
                                .apply(&transaction(RichOp::SetParaProps {
                                    para: paragraph,
                                    set: ParaProps {
                                        style: Some("paragraph".into()),
                                        ..ParaProps::default()
                                    },
                                    clear: ParaProps::default(),
                                }))
                                .unwrap();
                        }

                        let mut expected = table_value;
                        if let Some(value) = paragraph_value {
                            expected = value;
                        }
                        if default && !character_value || !default && character_value {
                            expected = !expected;
                        }
                        let direct = CharAttrs {
                            bold: direct_value,
                            ..CharAttrs::default()
                        };
                        let resolved = engine
                            .resolve(&document, paragraph, Some("character"), &direct)
                            .unwrap();
                        assert_eq!(
                            resolved.character.bold,
                            Some(direct_value.unwrap_or(expected)),
                            "default={default}, table={table_value}, paragraph={paragraph_value:?}, character={character_value}, direct={direct_value:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cache_is_bounded_and_successful_operations_invalidate_only_the_affected_paragraph() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 1);
    let mut document = RichDocument::new();
    let first = document.paragraph_ids()[0];
    let section = document.section_ids()[0];
    let branch = document.new_paragraph("second").unwrap();
    let second = branch.root;
    document
        .apply(&transaction(RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: branch,
        }))
        .unwrap();
    engine
        .resolve(&document, first, None, &CharAttrs::default())
        .unwrap();
    engine
        .resolve(&document, first, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(engine.cache_hits(), 1);
    engine
        .resolve(&document, second, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(engine.cache_len(), 1);

    // Refill the first entry, then edit the second paragraph.
    engine
        .resolve(&document, first, None, &CharAttrs::default())
        .unwrap();
    let before = engine.cache_len();
    let bad = transaction(RichOp::SetParaProps {
        para: second,
        set: ParaProps {
            alignment: Some(atelier_text_model::rich::Alignment::Right),
            ..ParaProps::default()
        },
        clear: ParaProps::default(),
    });
    let _ = engine.apply(&mut document, &bad);
    // Cache was invalidated for second only, so the first entry remains.
    assert_eq!(engine.cache_len(), before);
    engine
        .resolve(&document, first, None, &CharAttrs::default())
        .unwrap();
    assert!(engine.cache_hits() >= 2);
}

#[test]
fn failed_operation_keeps_cached_result_and_style_edits_invalidate_dependents() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 4);
    let mut document = RichDocument::new();
    let paragraph = document.paragraph_ids()[0];
    let mut p = style("p", StyleKind::Paragraph);
    p.character.values.italic = Some(false);
    engine.set_style(p.clone()).unwrap();
    let props = ParaProps {
        style: Some("p".into()),
        ..ParaProps::default()
    };
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: props,
            clear: ParaProps::default(),
        }))
        .unwrap();
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .italic,
        Some(false)
    );
    assert_eq!(engine.cache_len(), 1);

    let missing_id = document.alloc_id().unwrap();
    let invalid = transaction(RichOp::SetParaProps {
        para: missing_id,
        set: ParaProps::default(),
        clear: ParaProps::default(),
    });
    let cached = engine.cache_len();
    assert!(engine.apply(&mut document, &invalid).is_err());
    assert_eq!(engine.cache_len(), cached);
    let hits = engine.cache_hits();
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .italic,
        Some(false)
    );
    assert_eq!(engine.cache_hits(), hits + 1);

    p.character.values.italic = Some(true);
    engine.set_style(p).unwrap();
    assert_eq!(engine.cache_len(), 0);
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .italic,
        Some(true)
    );
}

#[test]
fn successful_character_format_operation_invalidates_the_paragraph_entry() {
    let mut document = RichDocument::new();
    let paragraph = document.paragraph_ids()[0];
    document
        .apply(&transaction(RichOp::InsertText {
            para: paragraph,
            offset: 0,
            text: "x".into(),
        }))
        .unwrap();
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 4);
    engine
        .resolve(&document, paragraph, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(engine.cache_len(), 1);

    let set = CharAttrs {
        bold: Some(false),
        ..CharAttrs::default()
    };
    engine
        .apply(
            &mut document,
            &transaction(RichOp::SetCharAttrs {
                para: paragraph,
                start: 0,
                end: 1,
                set,
                clear: CharAttrs::default(),
            }),
        )
        .unwrap();
    assert_eq!(engine.cache_len(), 0);
    let direct = document.paragraph(paragraph).unwrap().attrs_at(0);
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &direct)
            .unwrap()
            .character
            .bold,
        Some(false)
    );
}

#[test]
fn table_style_operation_is_reversible_and_refreshes_descendant_cache() {
    let mut document = RichDocument::new();
    let section = document.section_ids()[0];
    let table_branch = document.new_table(1, 1).unwrap();
    let table_id = table_branch.root;
    document
        .apply(&transaction(RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: table_branch,
        }))
        .unwrap();
    let paragraph = *document.paragraph_ids().last().unwrap();
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 4);
    let mut table = style("t", StyleKind::Table);
    table.character.values.bold = Some(true);
    engine.set_style(table).unwrap();
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .bold,
        None
    );
    let set = transaction(RichOp::SetTableStyle {
        table: table_id,
        style: Some("t".into()),
    });
    let inverse = engine.apply(&mut document, &set).unwrap();
    assert_eq!(engine.cache_len(), 0);
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .bold,
        Some(true)
    );
    inverse.apply(&mut document).unwrap();
    engine.clear_cache();
    assert_eq!(
        engine
            .resolve(&document, paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .bold,
        None
    );
    assert!(matches!(document.node(table_id), Some(Node::Table(table)) if table.style.is_none()));
}

#[test]
fn default_table_style_applies_to_cells_but_not_body_paragraphs() {
    let defaults = DocumentStyleDefaults {
        table_style: Some("default-table".into()),
        ..DocumentStyleDefaults::default()
    };
    let mut engine = StyleEngine::new(StyleCatalog::new(defaults), Theme::default(), 8);
    let mut table_style = style("default-table", StyleKind::Table);
    table_style.character.values.italic = Some(true);
    engine.set_style(table_style).unwrap();

    let mut document = RichDocument::new();
    let body_paragraph = document.paragraph_ids()[0];
    let section = document.section_ids()[0];
    let table_branch = document.new_table(1, 1).unwrap();
    document
        .apply(&transaction(RichOp::InsertBlock {
            parent: Some(section),
            position: 1,
            subtree: table_branch,
        }))
        .unwrap();
    let cell_paragraph = *document.paragraph_ids().last().unwrap();

    assert_eq!(
        engine
            .resolve(&document, body_paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .italic,
        None
    );
    assert_eq!(
        engine
            .resolve(&document, cell_paragraph, None, &CharAttrs::default())
            .unwrap()
            .character
            .italic,
        Some(true)
    );
}

#[test]
fn cached_base_is_safe_for_equal_node_ids_across_documents() {
    let mut engine = StyleEngine::new(StyleCatalog::default(), Theme::default(), 8);
    let (mut first, first_paragraph) = paragraph_document();
    let (mut second, second_paragraph) = paragraph_document();
    assert_eq!(first_paragraph, second_paragraph);
    for (document, paragraph, alignment) in [
        (
            &mut first,
            first_paragraph,
            atelier_text_model::rich::Alignment::Center,
        ),
        (
            &mut second,
            second_paragraph,
            atelier_text_model::rich::Alignment::Right,
        ),
    ] {
        document
            .apply(&transaction(RichOp::SetParaProps {
                para: paragraph,
                set: ParaProps {
                    alignment: Some(alignment),
                    ..ParaProps::default()
                },
                clear: ParaProps::default(),
            }))
            .unwrap();
    }

    let first_result = engine
        .resolve(&first, first_paragraph, None, &CharAttrs::default())
        .unwrap();
    let second_result = engine
        .resolve(&second, second_paragraph, None, &CharAttrs::default())
        .unwrap();
    assert_eq!(
        first_result.paragraph.alignment,
        Some(atelier_text_model::rich::Alignment::Center)
    );
    assert_eq!(
        second_result.paragraph.alignment,
        Some(atelier_text_model::rich::Alignment::Right)
    );
    assert_eq!(engine.cache_hits(), 1);
}

#[test]
fn borders_and_shading_cascade_from_styles_side_by_side() {
    use atelier_text_model::rich::{ParaBorder, ParaBorderStyle, ParaBorders, ParaShading};
    let line = |eighths| {
        Some(ParaBorder {
            style: ParaBorderStyle::Single,
            eighths,
            space: 2,
            color: None,
        })
    };
    let mut engine = StyleEngine::new(
        StyleCatalog::new(DocumentStyleDefaults::default()),
        Theme::default(),
        8,
    );
    let mut base = style("base", StyleKind::Paragraph);
    base.paragraph.borders = ParaBorders {
        top: line(4),
        bottom: line(4),
        ..ParaBorders::default()
    };
    base.paragraph.shading = Some(ParaShading {
        fill: Some(Color { r: 1, g: 2, b: 3 }),
        ..ParaShading::default()
    });
    engine.set_style(base).unwrap();
    let mut child = style("child", StyleKind::Paragraph);
    child.based_on = Some("base".into());
    child.paragraph.borders.bottom = line(12);
    engine.set_style(child).unwrap();

    let (mut document, paragraph) = paragraph_document();
    let direct = ParaProps {
        style: Some("child".into()),
        borders: ParaBorders {
            // "No border" on the top side only, a new left side.
            top: Some(ParaBorder {
                style: ParaBorderStyle::None,
                eighths: 0,
                space: 0,
                color: None,
            }),
            left: line(8),
            ..ParaBorders::default()
        },
        ..ParaProps::default()
    };
    document
        .apply(&transaction(RichOp::SetParaProps {
            para: paragraph,
            set: direct,
            clear: ParaProps::default(),
        }))
        .unwrap();
    let resolved = engine
        .resolve(&document, paragraph, None, &CharAttrs::default())
        .unwrap()
        .paragraph;
    assert!(!resolved.borders.top.unwrap().is_line()); // hidden by the direct value
    assert_eq!(resolved.borders.bottom.unwrap().eighths, 12); // child over base
    assert_eq!(resolved.borders.left.unwrap().eighths, 8); // direct
    assert!(resolved.borders.right.is_none());
    assert_eq!(
        resolved.shading.unwrap().paint_color(),
        Some(Color { r: 1, g: 2, b: 3 }) // inherited whole
    );
}
