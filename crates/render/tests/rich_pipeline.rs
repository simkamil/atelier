// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::FontManager;
use atelier_geom::Rect;
use atelier_render::{
    Command, DisplayList, PageInfo, paragraph_borders::paragraph_border_commands,
};
use atelier_text_layout::{
    LayoutOptions,
    borders::{
        BorderLine, BorderPlanLimits, BorderStyle, ColumnBounds, ParagraphBorderInput,
        ParagraphBorderPolicy, plan_paragraph_borders,
    },
    incremental::{IncrementalOptions, IncrementalSession, ParagraphInput},
    pagination::{PaginationOptions, ParagraphRules, paginate},
    tabs::TabSettings,
};
use atelier_text_model::rich::{CharAttrs, Color, default_rich_document};

#[test]
fn default_profile_incremental_pages_and_border_display_list_share_geometry() {
    let mut profile = default_rich_document();
    let id = profile.document.paragraph_ids()[0];
    let properties = profile
        .styles
        .resolve(&profile.document, id, None, &CharAttrs::default())
        .unwrap();
    let mut paragraph = profile.document.paragraph(id).unwrap().clone();
    paragraph.text = "Présentation du document français".into();
    let fonts = FontManager::new();
    let rules = ParagraphRules {
        space_after: 512,
        ..ParagraphRules::default()
    };
    let options = IncrementalOptions {
        layout: LayoutOptions::new(200 * 64),
        pagination: PaginationOptions {
            page_height: 700 * 64,
            max_pages: 10,
            max_work: 10_000,
        },
        tabs: TabSettings::default(),
    };
    let inputs = [ParagraphInput {
        id,
        paragraph: &paragraph,
        properties: &properties,
        rules: &rules,
    }];
    let mut session = IncrementalSession::default();
    let initial = session
        .update(&inputs, &fonts, options.clone())
        .unwrap()
        .clone();
    let unchanged = session.update(&inputs, &fonts, options).unwrap();
    assert_eq!(initial.layouts, unchanged.layouts);
    assert_eq!(initial.pagination, unchanged.pagination);
    let layouts: Vec<_> = unchanged
        .layouts
        .iter()
        .map(|layout| layout.layout.clone())
        .collect();
    assert_eq!(
        unchanged.pagination,
        paginate(
            &layouts,
            &[rules],
            PaginationOptions {
                page_height: 700 * 64,
                max_pages: 10,
                max_work: 10_000,
            }
        )
        .unwrap()
    );
    let edge = BorderLine::new(
        64,
        Color {
            r: 12,
            g: 34,
            b: 56,
        },
        BorderStyle::Solid,
        128,
    )
    .unwrap();
    let border_inputs = [ParagraphBorderInput {
        column: ColumnBounds::new(20 * 64, 200 * 64).unwrap(),
        policy: ParagraphBorderPolicy {
            top: Some(edge),
            bottom: Some(edge),
            left: Some(edge),
            right: Some(edge),
            shading: Some(Color {
                r: 240,
                g: 241,
                b: 242,
            }),
            ..ParagraphBorderPolicy::default()
        },
    }];
    let plan = plan_paragraph_borders(
        &layouts,
        &unchanged.pagination,
        &border_inputs,
        BorderPlanLimits::default(),
    )
    .unwrap();
    let commands = paragraph_border_commands(&plan, 0).unwrap();
    assert_eq!(commands.len(), 5);
    assert!(matches!(commands[0], Command::FillPath { .. }));
    assert!(
        commands[1..]
            .iter()
            .all(|command| matches!(command, Command::StrokePath { .. }))
    );
    let list = DisplayList::new(
        PageInfo::new(Rect::from_xywh(0.0, 0.0, 595.0, 842.0).unwrap()).unwrap(),
        commands,
    )
    .unwrap();
    list.validate().unwrap();
}
