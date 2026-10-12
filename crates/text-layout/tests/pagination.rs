// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_text_layout::pagination::{
    Page, PaginationDiagnostic, PaginationError, PaginationOptions, ParagraphRules, paginate,
};
use atelier_text_layout::{LayoutLine, ParagraphLayout};

fn line(height: i64, baseline: i64) -> LayoutLine {
    LayoutLine {
        source: 0..1,
        x: 0,
        baseline,
        width: 0,
        height,
        mandatory_break: false,
        final_line: true,
        justified: false,
        fragments: Vec::new(),
    }
}

fn layout(before: i64, heights: &[i64], after: i64) -> ParagraphLayout {
    let lines = heights
        .iter()
        .map(|&height| line(height, before + 4))
        .collect::<Vec<_>>();
    ParagraphLayout {
        width: 0,
        height: before + heights.iter().sum::<i64>() + after,
        lines,
        unsupported: Vec::new(),
    }
}

fn options(page_height: i64) -> PaginationOptions {
    PaginationOptions {
        page_height,
        max_pages: 32,
        max_work: 10_000,
    }
}

fn rules(before: i64, after: i64) -> ParagraphRules {
    ParagraphRules {
        space_before: before,
        space_after: after,
        ..ParagraphRules::default()
    }
}

fn tops(result: &atelier_text_layout::pagination::Pagination) -> Vec<i64> {
    result.placements.iter().map(|p| p.top).collect()
}

#[test]
fn normal_and_contextual_spacing_use_resolved_maximum() {
    let layouts = [layout(0, &[10], 200), layout(240, &[10], 0)];
    let mut r = [rules(0, 200), rules(240, 0)];
    let normal = paginate(&layouts, &r, options(1000)).unwrap();
    assert_eq!(tops(&normal), [0, 250]);
    r[0].contextual_spacing = true;
    let previous_contextual = paginate(&layouts, &r, options(1000)).unwrap();
    assert_eq!(tops(&previous_contextual), [0, 50]);
    r[1].contextual_spacing = true;
    let both_contextual = paginate(&layouts, &r, options(1000)).unwrap();
    assert_eq!(tops(&both_contextual), [0, 10]);
}

#[test]
fn automatic_page_top_drops_spacing_but_explicit_break_preserves_it() {
    let layouts = [layout(0, &[90], 0), layout(20, &[30], 0)];
    let paragraph_rules = [rules(0, 0), rules(20, 0)];
    let result = paginate(&layouts, &paragraph_rules, options(100)).unwrap();
    assert_eq!(result.placements[1].page_index, 1);
    assert_eq!(result.placements[1].top, 0);

    let mut explicit = paragraph_rules.clone();
    explicit[1].break_before = true;
    let result = paginate(&layouts, &explicit, options(100)).unwrap();
    assert_eq!(result.placements[1].top, 20);

    let oversized_before = [layout(100, &[10], 0)];
    let mut explicit = [rules(100, 0)];
    explicit[0].break_before = true;
    let result = paginate(&oversized_before, &explicit, options(100)).unwrap();
    assert_eq!(result.placements.len(), 1);
    assert_eq!(result.placements[0].top, 100);
    assert!(
        result
            .diagnostics
            .contains(&PaginationDiagnostic::ParagraphSpacingExceedsPage { paragraph_index: 0 })
    );

    for before in [100, 110] {
        let layouts = [layout(before, &[10, 10, 10], 0)];
        let mut explicit = [rules(before, 0)];
        explicit[0].break_before = true;
        let result = paginate(&layouts, &explicit, options(100)).unwrap();
        assert_eq!(result.placements.len(), 3);
        assert_eq!(result.placements[0].top, before);
        assert_eq!(result.placements[0].page_index, 0);
        assert_eq!(result.placements[1].page_index, 1);
        assert!(
            result
                .diagnostics
                .contains(&PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index: 0 })
        );
    }
}

#[test]
fn manual_line_break_splits_exactly_once_and_page_ranges_are_contiguous() {
    let layouts = [layout(0, &[60, 60, 20], 0)];
    let mut rule = rules(0, 0);
    rule.line_breaks_before = vec![1];
    let result = paginate(&layouts, &[rule], options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.line_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 1, 1]
    );
    assert_eq!(
        result.pages,
        [Page { placements: 0..1 }, Page { placements: 1..3 }]
    );
}

#[test]
fn widow_backtracking_never_crosses_an_explicit_manual_break() {
    let layouts = [layout(0, &[10, 10, 10, 10], 0)];
    let mut rule = rules(0, 0);
    rule.line_breaks_before = vec![3];
    let result = paginate(&layouts, &[rule], options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 0, 0, 1]
    );
    assert_eq!(
        result.pages,
        [Page { placements: 0..3 }, Page { placements: 3..4 }]
    );
    assert!(
        result
            .diagnostics
            .contains(&PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index: 0 })
    );
}

#[test]
fn keep_with_next_moves_a_fitting_chain_and_reports_an_impossible_one() {
    let layouts = [
        layout(0, &[40], 0),
        layout(0, &[40], 0),
        layout(0, &[30], 0),
    ];
    let mut r = [rules(0, 0), rules(0, 0), rules(0, 0)];
    r[1].keep_with_next = true;
    let result = paginate(&layouts, &r, options(100)).unwrap();
    assert_eq!(result.placements[0].page_index, 0);
    assert_eq!(result.placements[1].page_index, 1);
    assert_eq!(result.placements[2].page_index, 1);

    let impossible = [layout(0, &[60], 0), layout(0, &[60], 0)];
    let mut r = [rules(0, 0), rules(0, 0)];
    r[0].keep_with_next = true;
    let result = paginate(&impossible, &r, options(100)).unwrap();
    assert!(
        result
            .diagnostics
            .contains(&PaginationDiagnostic::KeepChainExceedsPage { paragraph_index: 0 })
    );
}

#[test]
fn explicit_breaks_take_precedence_over_keep_with_next() {
    let layouts = [layout(0, &[30], 0), layout(0, &[30], 0)];
    let mut r = [rules(0, 0), rules(0, 0)];
    r[0].keep_with_next = true;
    r[1].break_before = true;
    let result = paginate(&layouts, &r, options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(
        result
            .diagnostics
            .contains(&PaginationDiagnostic::KeepWithNextRelaxed { paragraph_index: 0 })
    );
}

#[test]
fn break_after_survives_empty_paragraphs_without_creating_a_trailing_page() {
    let empty = ParagraphLayout {
        width: 0,
        height: 0,
        lines: Vec::new(),
        unsupported: Vec::new(),
    };
    let layouts = [
        layout(0, &[10], 0),
        empty.clone(),
        layout(20, &[10], 0),
        empty,
    ];
    let mut r = [rules(0, 0), rules(0, 0), rules(0, 0), rules(0, 0)];
    r[0].break_after = true;
    r[2].space_before = 20;
    r[2].break_after = true;
    let result = paginate(&layouts, &r, options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        result.placements.iter().map(|p| p.top).collect::<Vec<_>>(),
        [0, 20]
    );
    assert_eq!(
        result.pages,
        [Page { placements: 0..1 }, Page { placements: 1..2 }]
    );
}

#[test]
fn overheight_line_and_unavoidable_widow_are_reported_without_losing_lines() {
    let layouts = [layout(0, &[120], 0), layout(0, &[60, 60, 60], 0)];
    let result = paginate(&layouts, &[rules(0, 0), rules(0, 0)], options(100)).unwrap();
    assert_eq!(result.placements.len(), 4);
    assert!(
        result
            .diagnostics
            .contains(&PaginationDiagnostic::LineExceedsPage {
                paragraph_index: 0,
                line_index: 0
            })
    );
    assert!(result.diagnostics.iter().any(|d| matches!(
        d,
        PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index: 1 }
    )));
}

#[test]
fn widow_rule_moves_a_line_when_the_previous_page_can_keep_its_minimum() {
    let layouts = [layout(0, &[30, 30, 30, 30], 0)];
    let result = paginate(&layouts, &[rules(0, 0)], options(90)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 0, 1, 1]
    );
    assert_eq!(
        result.placements.iter().map(|p| p.top).collect::<Vec<_>>(),
        [0, 30, 0, 30]
    );
    assert_eq!(
        result.pages,
        [Page { placements: 0..2 }, Page { placements: 2..4 }]
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| matches!(d, PaginationDiagnostic::WidowOrphanRelaxed { .. }))
    );
}

#[test]
fn five_short_lines_split_three_and_two_without_relaxation() {
    let layouts = [layout(0, &[10, 10, 10, 10, 10], 0)];
    let result = paginate(&layouts, &[rules(0, 0)], options(30)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.line_index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 0, 0, 1, 1]
    );
    assert!(
        !result.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            PaginationDiagnostic::WidowOrphanRelaxed { .. }
        ))
    );
}

#[test]
fn keep_together_moves_intact_and_baseline_uses_each_lines_input_offset() {
    let first = layout(0, &[70], 15);
    let mut second = layout(20, &[20, 20], 0);
    second.lines[1].baseline = 44;
    let mut second_rule = rules(20, 0);
    second_rule.keep_together = true;
    let result = paginate(&[first, second], &[rules(0, 15), second_rule], options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.page_index)
            .collect::<Vec<_>>(),
        [0, 1, 1]
    );
    assert_eq!(
        result.placements.iter().map(|p| p.top).collect::<Vec<_>>(),
        [0, 0, 20]
    );
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| p.baseline)
            .collect::<Vec<_>>(),
        [4, 4, 24]
    );

    let mut cumulative = layout(20, &[10, 15], 0);
    cumulative.lines[0].baseline = 28;
    cumulative.lines[1].baseline = 43;
    let result = paginate(&[cumulative], &[rules(20, 0)], options(100)).unwrap();
    assert_eq!(
        result
            .placements
            .iter()
            .map(|p| (p.top, p.baseline))
            .collect::<Vec<_>>(),
        [(0, 8), (10, 23)]
    );
}

#[test]
fn many_single_line_pages_conserve_sources_under_a_linear_work_budget() {
    let count = 2000;
    let paragraphs = [layout(0, &vec![1; count], 0)];
    let rule = ParagraphRules {
        widow_min: 1,
        orphan_min: 1,
        ..ParagraphRules::default()
    };
    let result = paginate(
        &paragraphs,
        &[rule],
        PaginationOptions {
            page_height: 1,
            max_pages: count,
            max_work: 8 * count + 20,
        },
    )
    .unwrap();
    assert_eq!(result.pages.len(), count);
    assert_eq!(result.placements.len(), count);
    for (index, placement) in result.placements.iter().enumerate() {
        assert_eq!(placement.line_index, index);
        assert_eq!(placement.page_index, index);
        assert_eq!(placement.top, 0);
        assert_eq!(result.pages[index].placements, index..index + 1);
    }
    assert!(result.diagnostics.is_empty());
}

#[test]
fn malformed_inputs_and_limits_return_typed_errors() {
    let one = [layout(0, &[10], 0)];
    for height in [0, -1] {
        assert!(matches!(
            paginate(&[layout(0, &[height], 0)], &[rules(0, 0)], options(100)),
            Err(PaginationError::InvalidInput(_))
        ));
    }
    for invalid_options in [
        options(0),
        options(-1),
        PaginationOptions {
            max_pages: 0,
            ..options(100)
        },
    ] {
        assert!(matches!(
            paginate(&one, &[rules(0, 0)], invalid_options),
            Err(PaginationError::InvalidOptions(_))
        ));
    }
    let mut bad = rules(0, 0);
    bad.line_breaks_before = vec![1];
    assert!(matches!(
        paginate(&one, &[bad], options(100)),
        Err(PaginationError::InvalidLineBreak { .. })
    ));
    for indices in [vec![0], vec![3], vec![1, 1], vec![2, 1]] {
        let mut bad_order = rules(0, 0);
        bad_order.line_breaks_before = indices;
        let three = [layout(0, &[10, 10, 10], 0)];
        assert!(matches!(
            paginate(&three, &[bad_order], options(100)),
            Err(PaginationError::InvalidLineBreak { .. })
        ));
    }
    assert!(matches!(
        paginate(
            &one,
            &[rules(0, 0)],
            PaginationOptions {
                max_work: 0,
                ..options(100)
            }
        ),
        Err(PaginationError::InvalidOptions(_))
    ));
    assert!(matches!(
        paginate(&one, &[], options(100)),
        Err(PaginationError::InvalidInput(_))
    ));
    assert!(matches!(
        paginate(
            &one,
            &[rules(0, 0)],
            PaginationOptions {
                max_work: 1,
                ..options(100)
            }
        ),
        Err(PaginationError::LimitExceeded("work"))
    ));
    assert!(matches!(
        paginate(
            &[layout(0, &[10, 10], 0)],
            &[rules(0, 0)],
            PaginationOptions {
                max_pages: 1,
                ..options(5)
            }
        ),
        Err(PaginationError::LimitExceeded("pages"))
    ));
    let huge = ParagraphLayout {
        width: 0,
        height: i64::MAX,
        lines: vec![line(i64::MAX, 0)],
        unsupported: Vec::new(),
    };
    assert!(matches!(
        paginate(&[huge], &[rules(1, 0)], options(100)),
        Err(PaginationError::ArithmeticOverflow)
    ));
    let mut bad_height = one[0].clone();
    bad_height.height += 1;
    assert!(matches!(
        paginate(&[bad_height], &[rules(0, 0)], options(100)),
        Err(PaginationError::InvalidInput(_))
    ));
}
