// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::FontManager;
use atelier_text_layout::{
    LayoutOptions, layout_paragraph,
    tabs::{
        TabAlignment, TabDecorationKind, TabLeader, TabSettings, TabStop,
        layout_paragraph_with_tabs,
    },
};
use atelier_text_model::rich::{CharAttrs, Paragraph, ResolvedProperties};

fn fonts() -> FontManager<'static> {
    FontManager::new()
}

#[test]
fn default_tab_uses_2268_grid_and_keeps_terminal_control_source() {
    let manager = fonts();
    for text in ["a\tb", "a\t"] {
        let result = layout_paragraph(
            &Paragraph::with_text(text),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(12_000),
        )
        .unwrap();
        let glyphs = result.lines[0]
            .fragments
            .iter()
            .flat_map(|fragment| &fragment.glyphs)
            .collect::<Vec<_>>();
        let control = glyphs
            .iter()
            .find(|glyph| &text[glyph.source.clone()] == "\t")
            .unwrap();
        assert_eq!(control.glyph_id, 0);
        assert_eq!(control.x_advance, 2268 - control.x);
        assert_eq!(control.source, 1..2);
        assert!(
            !result
                .unsupported
                .iter()
                .any(|item| item.contains("tab stops"))
        );
    }
}

#[test]
fn tab_only_lines_retain_default_and_resolved_font_metrics() {
    let manager = fonts();
    let empty = layout_paragraph(
        &Paragraph::with_text(""),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(8000),
    )
    .unwrap();
    let tabs = layout_paragraph(
        &Paragraph::with_text("\t\t"),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(8000),
    )
    .unwrap();
    assert_eq!(tabs.lines[0].height, empty.lines[0].height);
    assert_eq!(tabs.lines[0].baseline, empty.lines[0].baseline);

    let large_defaults = ResolvedProperties {
        character: CharAttrs {
            size_half_points: Some(48),
            ..CharAttrs::default()
        },
        ..ResolvedProperties::default()
    };
    let large_empty = layout_paragraph(
        &Paragraph::with_text(""),
        &large_defaults,
        &manager,
        LayoutOptions::new(8000),
    )
    .unwrap();
    let large_tabs = layout_paragraph(
        &Paragraph::with_text("\t"),
        &large_defaults,
        &manager,
        LayoutOptions::new(8000),
    )
    .unwrap();
    assert_eq!(large_tabs.lines[0].height, large_empty.lines[0].height);
    assert_eq!(large_tabs.lines[0].baseline, large_empty.lines[0].baseline);
    assert!(large_tabs.lines[0].height > tabs.lines[0].height);
}

#[test]
fn default_tab_grid_accounts_for_first_line_and_hanging_indents() {
    let mut paragraph = Paragraph::with_text("a\tb\nc\td");
    paragraph.props.indent_left = Some(120);
    paragraph.props.indent_first_line = Some(-40);
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(9000),
    )
    .unwrap();
    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].x, 256); // 80 twips.
    assert_eq!(result.lines[1].x, 384); // hanging continuation at 120 twips.
    for line in &result.lines {
        let tab = line
            .fragments
            .iter()
            .flat_map(|fragment| &fragment.glyphs)
            .find(|glyph| &paragraph.text[glyph.source.clone()] == "\t")
            .unwrap();
        let expected_stop = (tab.x.div_euclid(2268) + 1) * 2268;
        assert_eq!(tab.x + tab.x_advance, expected_stop);
    }
}

#[test]
fn explicit_right_decimal_and_leader_stops_have_numeric_positions() {
    let manager = fonts();
    let settings = TabSettings {
        default_interval: 9000,
        stops: vec![
            TabStop {
                position: 3000,
                alignment: TabAlignment::Right,
                leader: Some(TabLeader::Dots),
            },
            TabStop {
                position: 6000,
                alignment: TabAlignment::Decimal,
                leader: None,
            },
        ],
    };
    let text = "a\t12,3\t4,5";
    let result = layout_paragraph_with_tabs(
        &Paragraph::with_text(text),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(10_000),
        &settings,
    )
    .unwrap();
    let glyphs = result.layout.lines[0]
        .fragments
        .iter()
        .flat_map(|fragment| &fragment.glyphs)
        .collect::<Vec<_>>();
    let tabs = glyphs
        .iter()
        .filter(|glyph| &text[glyph.source.clone()] == "\t")
        .collect::<Vec<_>>();
    assert_eq!(tabs.len(), 2);
    let first_text_end = glyphs
        .iter()
        .find(|glyph| glyph.source.start >= 2)
        .unwrap()
        .x;
    assert_eq!(
        tabs[0].x
            + tabs[0].x_advance
            + glyphs
                .iter()
                .filter(|g| g.source.start > tabs[0].source.start
                    && g.source.start < tabs[1].source.start)
                .map(|g| g.x_advance)
                .sum::<i64>(),
        3000
    );
    assert!(first_text_end >= 3000 - tabs[0].x_advance);
    assert!(
        result
            .decorations
            .iter()
            .any(|decoration| decoration.kind == TabDecorationKind::Leader(TabLeader::Dots))
    );
    let decimal = tabs[1];
    let comma = text[decimal.source.end..].find(',').unwrap() + decimal.source.end;
    let before_comma = glyphs
        .iter()
        .filter(|g| g.source.start > decimal.source.start && g.source.end <= comma)
        .map(|g| g.x_advance)
        .sum::<i64>();
    assert_eq!(decimal.x + decimal.x_advance + before_comma, 6000);
}

#[test]
fn center_and_decimal_without_comma_align_by_segment_width() {
    let manager = fonts();
    for (alignment, text, stop) in [
        (TabAlignment::Center, "a\tbc", 3000),
        (TabAlignment::Decimal, "a\t123", 4000),
    ] {
        let settings = TabSettings {
            default_interval: 9000,
            stops: vec![TabStop {
                position: stop,
                alignment,
                leader: None,
            }],
        };
        let result = layout_paragraph_with_tabs(
            &Paragraph::with_text(text),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(10_000),
            &settings,
        )
        .unwrap();
        let glyphs = result.layout.lines[0]
            .fragments
            .iter()
            .flat_map(|fragment| &fragment.glyphs)
            .collect::<Vec<_>>();
        let tab = glyphs
            .iter()
            .find(|glyph| &text[glyph.source.clone()] == "\t")
            .unwrap();
        let segment = glyphs
            .iter()
            .filter(|glyph| glyph.source.start > tab.source.start)
            .map(|glyph| glyph.x_advance)
            .sum::<i64>();
        let alignment_offset = if alignment == TabAlignment::Center {
            segment / 2
        } else {
            segment
        };
        assert_eq!(tab.x + tab.x_advance + alignment_offset, stop);
    }
}

#[test]
fn custom_stops_precede_the_default_grid_then_automatic_stops_resume() {
    let manager = fonts();
    let settings = TabSettings {
        default_interval: 2268,
        stops: vec![TabStop {
            position: 4000,
            alignment: TabAlignment::Center,
            leader: None,
        }],
    };
    let text = "a\tbc\tde";
    let result = layout_paragraph_with_tabs(
        &Paragraph::with_text(text),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(9000),
        &settings,
    )
    .unwrap();
    let glyphs = result.layout.lines[0]
        .fragments
        .iter()
        .flat_map(|fragment| &fragment.glyphs)
        .collect::<Vec<_>>();
    let tab_glyphs = glyphs
        .iter()
        .filter(|glyph| &text[glyph.source.clone()] == "\t")
        .collect::<Vec<_>>();
    assert_eq!(tab_glyphs.len(), 2);
    let first_segment_width = glyphs
        .iter()
        .filter(|glyph| {
            glyph.source.start > tab_glyphs[0].source.start
                && glyph.source.start < tab_glyphs[1].source.start
        })
        .map(|glyph| glyph.x_advance)
        .sum::<i64>();
    assert_eq!(
        tab_glyphs[0].x + tab_glyphs[0].x_advance + first_segment_width / 2,
        4000
    );
    assert_eq!(tab_glyphs[1].x + tab_glyphs[1].x_advance, 4536);
}

#[test]
fn bars_are_fixed_and_invalid_settings_fail_without_partial_layout() {
    let manager = fonts();
    let settings = TabSettings {
        default_interval: 2268,
        stops: vec![
            TabStop {
                position: 1000,
                alignment: TabAlignment::Bar,
                leader: None,
            },
            TabStop {
                position: 4000,
                alignment: TabAlignment::Left,
                leader: None,
            },
        ],
    };
    let result = layout_paragraph_with_tabs(
        &Paragraph::with_text("a\tb"),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(8000),
        &settings,
    )
    .unwrap();
    assert!(
        result
            .decorations
            .iter()
            .any(|d| d.kind == TabDecorationKind::Bar && d.x == 1000 && d.width == 0)
    );
    let without_tabs = layout_paragraph_with_tabs(
        &Paragraph::with_text("a\nb"),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(8000),
        &settings,
    )
    .unwrap();
    assert_eq!(without_tabs.decorations.len(), 2);
    for (index, decoration) in without_tabs.decorations.iter().enumerate() {
        assert_eq!(decoration.line, index);
        assert_eq!(decoration.kind, TabDecorationKind::Bar);
        assert_eq!((decoration.x, decoration.width), (1000, 0));
    }
    let bad = TabSettings {
        default_interval: 0,
        stops: Vec::new(),
    };
    assert!(
        layout_paragraph_with_tabs(
            &Paragraph::with_text("a"),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(8000),
            &bad
        )
        .is_err()
    );
    let extreme = TabSettings {
        default_interval: 1,
        stops: vec![TabStop {
            position: i64::MAX,
            alignment: TabAlignment::Left,
            leader: None,
        }],
    };
    assert!(
        layout_paragraph_with_tabs(
            &Paragraph::with_text("a"),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(8000),
            &extreme
        )
        .is_err()
    );
    let duplicate = TabSettings {
        default_interval: 1,
        stops: vec![
            TabStop {
                position: 20,
                alignment: TabAlignment::Left,
                leader: None,
            },
            TabStop {
                position: 20,
                alignment: TabAlignment::Right,
                leader: None,
            },
        ],
    };
    assert!(
        layout_paragraph_with_tabs(
            &Paragraph::with_text("a"),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(8000),
            &duplicate
        )
        .is_err()
    );
}

#[test]
fn tabs_survive_wrapping_with_contiguous_utf8_sources() {
    let text = "é\tune très longue séquence";
    let result = layout_paragraph(
        &Paragraph::with_text(text),
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(1800),
    )
    .unwrap();
    let mut end = 0;
    for line in &result.lines {
        assert_eq!(line.source.start, end);
        end = line.source.end;
    }
    assert_eq!(end, text.len());
    let controls = result
        .lines
        .iter()
        .flat_map(|line| &line.fragments)
        .flat_map(|fragment| &fragment.glyphs)
        .filter(|glyph| &text[glyph.source.clone()] == "\t")
        .count();
    assert_eq!(controls, 1);
}

#[test]
fn mandatory_lines_without_tabs_respect_a_tight_range_scan_budget() {
    let text = "a\n".repeat(48);
    let mut options = LayoutOptions::new(6400);
    options.max_work = 5000;
    let result = layout_paragraph(
        &Paragraph::with_text(&text),
        &ResolvedProperties::default(),
        &fonts(),
        options,
    )
    .unwrap();
    assert_eq!(result.lines.len(), 49);
}

#[test]
fn all_leader_kinds_cover_the_consumed_gap_and_respect_decoration_limits() {
    let manager = fonts();
    let prefix = layout_paragraph(
        &Paragraph::with_text("a"),
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(8000),
    )
    .unwrap()
    .lines[0]
        .width;
    for leader in [TabLeader::Dots, TabLeader::Dashes, TabLeader::Line] {
        let settings = TabSettings {
            default_interval: 2268,
            stops: vec![TabStop {
                position: 4000,
                alignment: TabAlignment::Left,
                leader: Some(leader),
            }],
        };
        let result = layout_paragraph_with_tabs(
            &Paragraph::with_text("a\tb"),
            &ResolvedProperties::default(),
            &manager,
            LayoutOptions::new(8000),
            &settings,
        )
        .unwrap();
        assert_eq!(result.decorations.len(), 1);
        let decoration = result.decorations[0];
        assert_eq!(decoration.kind, TabDecorationKind::Leader(leader));
        assert_eq!(
            (decoration.line, decoration.x, decoration.width),
            (0, prefix, 4000 - prefix)
        );
    }
    let bars = TabSettings {
        default_interval: 2268,
        stops: [1000, 2000, 3000]
            .map(|position| TabStop {
                position,
                alignment: TabAlignment::Bar,
                leader: None,
            })
            .to_vec(),
    };
    let mut options = LayoutOptions::new(8000);
    options.max_glyphs = 1;
    assert!(matches!(
        layout_paragraph_with_tabs(
            &Paragraph::with_text("a"),
            &ResolvedProperties::default(),
            &manager,
            options,
            &bars
        ),
        Err(atelier_text_layout::LayoutError::LimitExceeded(
            "tab decorations"
        ))
    ));
}
