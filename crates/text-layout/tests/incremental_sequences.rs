// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::FontManager;
use atelier_text_layout::{
    LayoutOptions,
    incremental::{IncrementalOptions, IncrementalSession, ParagraphInput},
    pagination::{PaginationOptions, ParagraphRules, paginate},
    tabs::{TabSettings, layout_paragraph_with_tabs},
};
use atelier_text_model::rich::{
    CharAttrs, LineRule, LineSpacing, Paragraph, ResolvedProperties, RichDocument,
};

#[test]
fn deterministic_mixed_edit_sequence_matches_fresh_shaping_and_pagination() {
    let fonts = FontManager::new();
    let mut document = RichDocument::new();
    let ids: Vec<_> = (0..24).map(|_| document.alloc_id().unwrap()).collect();
    let mut paragraphs: Vec<_> = (0..24)
        .map(|index| Paragraph::with_text(&"Texte français\n".repeat(1 + index % 7)))
        .collect();
    let mut rules: Vec<_> = (0..24)
        .map(|index| ParagraphRules {
            style_id: (index % 3) as u64,
            keep_with_next: index % 5 == 0,
            keep_together: index % 7 == 0,
            break_before: index % 11 == 0,
            break_after: index % 13 == 0,
            ..ParagraphRules::default()
        })
        .collect();
    let properties = ResolvedProperties {
        character: CharAttrs {
            size_half_points: Some(22),
            ..CharAttrs::default()
        },
        ..ResolvedProperties::default()
    };
    let mut options = IncrementalOptions {
        layout: LayoutOptions::new(20_000),
        pagination: PaginationOptions {
            page_height: 4_000,
            max_pages: 1_000,
            max_work: 2_000_000,
        },
        tabs: TabSettings::default(),
    };
    let mut session = IncrementalSession::new();
    for step in 0..36 {
        let index = (step * 7 + 3) % paragraphs.len();
        match step % 6 {
            0 => paragraphs[index].text = "Français\n".repeat(1 + step % 9),
            1 => rules[index].keep_with_next = !rules[index].keep_with_next,
            2 => rules[index].break_before = !rules[index].break_before,
            3 => {
                options.pagination.page_height = 4_000 + (step % 4) as i64 * 500;
                paragraphs[index].text.push_str("Suite\n");
            }
            4 => {
                paragraphs[index].props.line_spacing = Some(LineSpacing {
                    rule: LineRule::Exact,
                    value: 240 + step as u32,
                });
                rules[index].keep_together = !rules[index].keep_together;
            }
            _ => {
                paragraphs[index].text = "A\tB\nC\nD\nE".into();
                rules[index].line_breaks_before = vec![1];
            }
        }
        // A prior manual break must stay interior when a later edit shortens a paragraph.
        if paragraphs[index].text.lines().count() <= 1 {
            rules[index].line_breaks_before.clear();
        }
        let inputs: Vec<_> = ids
            .iter()
            .zip(&paragraphs)
            .zip(&rules)
            .map(|((&id, paragraph), rule)| ParagraphInput {
                id,
                paragraph,
                properties: &properties,
                rules: rule,
            })
            .collect();
        let incremental = session.update(&inputs, &fonts, options.clone()).unwrap();
        let fresh: Vec<_> = paragraphs
            .iter()
            .map(|paragraph| {
                layout_paragraph_with_tabs(
                    paragraph,
                    &properties,
                    &fonts,
                    options.layout,
                    &options.tabs,
                )
                .unwrap()
            })
            .collect();
        assert_eq!(
            incremental.layouts, fresh,
            "shaping diverged at step {step}"
        );
        let layouts: Vec<_> = fresh.iter().map(|value| value.layout.clone()).collect();
        assert_eq!(
            incremental.pagination,
            paginate(&layouts, &rules, options.pagination).unwrap(),
            "pagination diverged at step {step}"
        );
    }
}
