// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::FontManager;
use atelier_text_layout::caret::{CaretAffinity, CaretOptions, TextPosition};
use atelier_text_layout::flow::{FlowError, caret_from_pagination};
use atelier_text_layout::pagination::{PaginationOptions, ParagraphRules, paginate};
use atelier_text_layout::{LayoutOptions, ParagraphLayout, layout_paragraph};
use atelier_text_model::rich::{LineRule, LineSpacing, Paragraph, ResolvedProperties};

fn laid_out(text: &str, before: u32, after: u32) -> ParagraphLayout {
    let mut paragraph = Paragraph::with_text(text);
    paragraph.props.space_before = Some(before);
    paragraph.props.space_after = Some(after);
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Exact,
        value: 240,
    });
    layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &FontManager::new(),
        LayoutOptions::new(6400),
    )
    .unwrap()
}

fn page_options(height: i64) -> PaginationOptions {
    PaginationOptions {
        page_height: height,
        max_pages: 100,
        max_work: 100_000,
    }
}

#[test]
fn real_tabbed_layout_paginates_and_maps_carets_between_pages() {
    let text = "a\tb\nc\nd\n";
    let layouts = [laid_out(text, 0, 0)];
    let rules = [ParagraphRules {
        widow_min: 1,
        orphan_min: 1,
        ..ParagraphRules::default()
    }];
    assert_eq!(layouts[0].lines.len(), 4);
    let pagination = paginate(&layouts, &rules, page_options(1536)).unwrap();
    assert_eq!(pagination.pages.len(), 2);
    assert_eq!(pagination.pages[0].placements, 0..2);
    assert_eq!(pagination.pages[1].placements, 2..4);
    let geometry =
        caret_from_pagination(&[text], &layouts, &pagination, 100, CaretOptions::default())
            .unwrap();
    let tab = layouts[0].lines[0]
        .fragments
        .iter()
        .flat_map(|f| &f.glyphs)
        .find(|g| &text[g.source.clone()] == "\t")
        .expect("tabs retain a control glyph with their computed advance");
    assert!(tab.x_advance > 0);
    let after_tab = TextPosition {
        paragraph_index: 0,
        byte_offset: 2,
    };
    let rect = geometry
        .caret_rect(after_tab, CaretAffinity::Downstream)
        .unwrap();
    assert_eq!(rect.x, tab.x + tab.x_advance + 100);
    assert_eq!(rect.y, 0);
    assert_eq!(
        geometry.hit_test(0, rect.x, rect.y + 384).unwrap(),
        after_tab
    );
    let end = TextPosition {
        paragraph_index: 0,
        byte_offset: text.len(),
    };
    let terminal = geometry.caret_rect(end, CaretAffinity::Downstream).unwrap();
    assert_eq!(terminal.page_index, 1);
    assert_eq!(terminal.y, 768);
    let selection = geometry
        .selection_rects(
            TextPosition {
                paragraph_index: 0,
                byte_offset: 0,
            }..end,
        )
        .unwrap();
    assert!(selection.iter().any(|rect| rect.page_index == 0));
    assert!(selection.iter().any(|rect| rect.page_index == 1));
}

#[test]
fn included_paragraph_spacing_is_counted_once_across_the_pipeline() {
    let texts = ["office", "a\u{301}"];
    let layouts = texts.map(|text| laid_out(text, 120, 240));
    let rules = [0, 1].map(|_| ParagraphRules {
        space_before: 384,
        space_after: 768,
        ..ParagraphRules::default()
    });
    let pagination = paginate(&layouts, &rules, page_options(5000)).unwrap();
    assert_eq!(pagination.pages.len(), 1);
    assert_eq!(pagination.placements[0].top, 0);
    assert_eq!(pagination.placements[1].top, 1536); // 768 line + max(768,384)
    let geometry =
        caret_from_pagination(&texts, &layouts, &pagination, 0, CaretOptions::default()).unwrap();
    let position = TextPosition {
        paragraph_index: 1,
        byte_offset: 0,
    };
    assert_eq!(
        geometry
            .caret_rect(position, CaretAffinity::Downstream)
            .unwrap()
            .y,
        1536
    );
    assert!(
        geometry
            .caret_rect(
                TextPosition {
                    paragraph_index: 1,
                    byte_offset: 1
                },
                CaretAffinity::Downstream
            )
            .is_err()
    );
    let selected = geometry
        .selection_rects(
            TextPosition {
                paragraph_index: 1,
                byte_offset: texts[1].len(),
            }..TextPosition {
                paragraph_index: 0,
                byte_offset: 0,
            },
        )
        .unwrap();
    assert_eq!(selected.len(), 2);
    assert_eq!(
        selected
            .iter()
            .map(|rect| (rect.paragraph_index, rect.page_index, rect.y))
            .collect::<Vec<_>>(),
        [(0, 0, 0), (1, 0, 1536)]
    );
    assert!(selected.iter().all(|rect| rect.width > 0));
}

#[test]
fn adapter_rejects_forged_page_ranges_and_source_indices() {
    let layouts = [laid_out("ab", 0, 0)];
    let mut pagination =
        paginate(&layouts, &[ParagraphRules::default()], page_options(2000)).unwrap();
    pagination.pages[0].placements = 0..usize::MAX;
    assert!(matches!(
        caret_from_pagination(&["ab"], &layouts, &pagination, 0, CaretOptions::default()),
        Err(FlowError::InvalidPagination(_))
    ));
    pagination.pages[0].placements = 0..1;
    pagination.placements[0].line_index = 1;
    assert!(matches!(
        caret_from_pagination(&["ab"], &layouts, &pagination, 0, CaretOptions::default()),
        Err(FlowError::InvalidPagination(_))
    ));
}
