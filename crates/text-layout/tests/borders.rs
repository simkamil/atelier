// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_text_layout::borders::{
    BorderLine, BorderPlanError, BorderPlanLimits, BorderPrimitive, BorderStyle, ColumnBounds,
    ParagraphBorderInput, ParagraphBorderPolicy, plan_paragraph_borders,
};
use atelier_text_layout::pagination::{Page, Pagination, PlacedLine};
use atelier_text_layout::{LayoutFragment, LayoutLine, ParagraphLayout};
use atelier_text_model::rich::Color;
use core::ops::Range;

fn color(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b }
}

fn border(style: BorderStyle, width: i64, spacing: i64, color: Color) -> BorderLine {
    BorderLine::new(width, color, style, spacing).unwrap()
}

fn line(source: Range<usize>, height: i64, x: i64, width: i64) -> LayoutLine {
    LayoutLine {
        source,
        x,
        baseline: height * 3 / 4,
        width,
        height,
        mandatory_break: false,
        final_line: true,
        justified: false,
        fragments: Vec::<LayoutFragment>::new(),
    }
}

fn layout(lines: Vec<LayoutLine>) -> ParagraphLayout {
    let height = lines.iter().map(|line| line.height).sum();
    ParagraphLayout {
        width: lines.iter().map(|line| line.width).max().unwrap_or(0),
        height,
        lines,
        unsupported: Vec::new(),
    }
}

fn input(policy: ParagraphBorderPolicy) -> ParagraphBorderInput {
    ParagraphBorderInput {
        column: ColumnBounds::new(100, 6400).unwrap(),
        policy,
    }
}

fn one_page(placements: Vec<PlacedLine>) -> Pagination {
    let len = placements.len();
    Pagination {
        pages: vec![Page { placements: 0..len }],
        placements,
        diagnostics: Vec::new(),
    }
}

fn placement(
    page_index: usize,
    paragraph_index: usize,
    line_index: usize,
    top: i64,
    height: i64,
) -> PlacedLine {
    PlacedLine {
        page_index,
        paragraph_index,
        line_index,
        top,
        baseline: top + height * 3 / 4,
    }
}

fn frame_policy(style: BorderStyle) -> ParagraphBorderPolicy {
    let edge = border(style, 64, 32, color(20, 40, 60));
    ParagraphBorderPolicy {
        top: Some(edge),
        bottom: Some(edge),
        left: Some(edge),
        right: Some(edge),
        between: Some(edge),
        shading: None,
    }
}

#[test]
fn isolated_frame_uses_explicit_column_bounds_and_border_spacing() {
    let layouts = vec![layout(vec![line(0..5, 640, 740, 3000)])];
    let mut policy = frame_policy(BorderStyle::Solid);
    policy.between = None;
    let plan = plan_paragraph_borders(
        &layouts,
        &one_page(vec![placement(0, 0, 0, 100, 640)]),
        &[input(policy)],
        BorderPlanLimits::default(),
    )
    .unwrap();
    let primitives = &plan.pages[0].primitives;
    assert_eq!(primitives.len(), 4);
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        BorderPrimitive::Stroke {
            start,
            end,
            width: 64,
            ..
        } if start.x == 36 && end.x == 6564 && start.y == 36 && end.y == 36
    )));
    assert!(primitives.iter().any(|primitive| matches!(
        primitive,
        BorderPrimitive::Stroke { start, end, .. }
            if start.x == 36 && end.x == 36 && start.y == 36 && end.y == 804
    )));
    // The text width and x are deliberately much narrower/different than the
    // explicit column rectangle used by the frame.
    assert_eq!(layouts[0].lines[0].x, 740);
    assert_eq!(layouts[0].lines[0].width, 3000);
}

#[test]
fn frame_horizontal_edges_reach_side_centerlines_with_different_spacings() {
    let layouts = vec![layout(vec![line(0..5, 640, 740, 3000)])];
    let mut policy = frame_policy(BorderStyle::Solid);
    policy.top = Some(border(BorderStyle::Solid, 40, 10, color(1, 2, 3)));
    policy.bottom = Some(border(BorderStyle::Solid, 50, 15, color(1, 2, 3)));
    policy.left = Some(border(BorderStyle::Solid, 20, 7, color(1, 2, 3)));
    policy.right = Some(border(BorderStyle::Solid, 30, 13, color(1, 2, 3)));
    policy.between = None;
    let plan = plan_paragraph_borders(
        &layouts,
        &one_page(vec![placement(0, 0, 0, 100, 640)]),
        &[input(policy)],
        BorderPlanLimits::default(),
    )
    .unwrap();
    let primitives = &plan.pages[0].primitives;
    assert!(primitives.iter().any(|primitive| matches!(primitive,
        BorderPrimitive::Stroke { start, end, width: 40, .. }
            if start.x == 83 && end.x == 6528 && start.y == 70 && end.y == 70)));
    assert!(primitives.iter().any(|primitive| matches!(primitive,
        BorderPrimitive::Stroke { start, end, width: 50, .. }
            if start.x == 83 && end.x == 6528 && start.y == 780 && end.y == 780)));
}

#[test]
fn between_edge_checks_exact_and_insufficient_spacing_envelopes() {
    let layouts = vec![
        layout(vec![line(0..2, 100, 0, 10)]),
        layout(vec![line(0..2, 100, 0, 10)]),
    ];
    let policy = ParagraphBorderPolicy {
        between: Some(border(BorderStyle::Solid, 5, 3, color(0, 0, 0))),
        ..ParagraphBorderPolicy::default()
    };
    let inputs = [input(policy.clone()), input(policy.clone())];

    // Odd width 5: center at previous.bottom + 3 + floor(5/2),
    // with the complete 3 + 5 unit spacing envelope fitting exactly.
    let exact = one_page(vec![
        placement(0, 0, 0, 0, 100),
        placement(0, 1, 0, 108, 100),
    ]);
    let plan =
        plan_paragraph_borders(&layouts, &exact, &inputs, BorderPlanLimits::default()).unwrap();
    assert!(
        matches!(plan.pages[0].primitives.as_slice(), [BorderPrimitive::Stroke {
        start, end, width: 5, ..
    }] if start.y == 105 && end.y == 105)
    );

    let insufficient = one_page(vec![
        placement(0, 0, 0, 0, 100),
        placement(0, 1, 0, 107, 100),
    ]);
    assert!(matches!(
        plan_paragraph_borders(
            &layouts,
            &insufficient,
            &inputs,
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));
}

#[test]
fn adjacent_equal_borders_share_frame_but_keep_independent_shading() {
    let layouts = (0..3)
        .map(|i| layout(vec![line(i * 10..i * 10 + 5, 640, 120, 1000)]))
        .collect::<Vec<_>>();
    let placements = (0..3)
        .map(|i| placement(0, i, 0, i as i64 * 740, 640))
        .collect();
    let mut inputs = Vec::new();
    for shade in [color(255, 0, 0), color(0, 255, 0), color(0, 0, 255)] {
        let mut policy = frame_policy(BorderStyle::Solid);
        policy.shading = Some(shade);
        inputs.push(input(policy));
    }
    let plan = plan_paragraph_borders(
        &layouts,
        &one_page(placements),
        &inputs,
        BorderPlanLimits::default(),
    )
    .unwrap();
    let primitives = &plan.pages[0].primitives;
    assert_eq!(
        primitives
            .iter()
            .filter(|primitive| matches!(primitive, BorderPrimitive::Fill { .. }))
            .count(),
        3
    );
    assert_eq!(
        primitives
            .iter()
            .filter(|primitive| matches!(primitive, BorderPrimitive::Stroke { .. }))
            .count(),
        6 // four outer sides and two between rules
    );
}

#[test]
fn no_border_zero_line_or_changed_column_interrupts_a_group() {
    let layouts = vec![
        layout(vec![line(0..3, 640, 0, 200)]),
        layout(vec![line(0..0, 640, 0, 0)]),
        layout(Vec::new()),
        layout(vec![line(0..3, 640, 0, 200)]),
    ];
    let pagination = one_page(vec![
        placement(0, 0, 0, 0, 640),
        placement(0, 1, 0, 800, 640),
        placement(0, 3, 0, 1600, 640),
    ]);
    let mut policies = vec![frame_policy(BorderStyle::Solid); 4];
    policies[1] = ParagraphBorderPolicy::default();
    let mut inputs = policies.into_iter().map(input).collect::<Vec<_>>();
    inputs[3].column.x += 1;
    let plan = plan_paragraph_borders(&layouts, &pagination, &inputs, BorderPlanLimits::default())
        .unwrap();
    // Paragraph 0 and 3 have independent closed frames; empty and zero-line
    // entries cannot bridge them.
    assert_eq!(
        plan.pages[0]
            .primitives
            .iter()
            .filter(|primitive| matches!(primitive, BorderPrimitive::Stroke { .. }))
            .count(),
        8
    );
}

#[test]
fn changed_border_color_or_spacing_separates_adjacent_frames() {
    let layouts = vec![
        layout(vec![line(0..3, 640, 0, 200)]),
        layout(vec![line(0..3, 640, 0, 200)]),
    ];
    let pagination = one_page(vec![
        placement(0, 0, 0, 0, 640),
        placement(0, 1, 0, 700, 640),
    ]);
    for changed in [
        border(BorderStyle::Solid, 64, 32, color(21, 40, 60)),
        border(BorderStyle::Solid, 64, 33, color(20, 40, 60)),
    ] {
        let mut first = frame_policy(BorderStyle::Solid);
        first.between = None;
        let mut second = first.clone();
        second.top = Some(changed);
        let plan = plan_paragraph_borders(
            &layouts,
            &pagination,
            &[input(first), input(second)],
            BorderPlanLimits::default(),
        )
        .unwrap();
        assert_eq!(
            plan.pages[0]
                .primitives
                .iter()
                .filter(|primitive| matches!(primitive, BorderPrimitive::Stroke { .. }))
                .count(),
            8
        );
    }
}

#[test]
fn malformed_inputs_and_resource_ceilings_return_typed_errors() {
    let one_layout = vec![layout(vec![line(0..3, 640, 0, 100)])];
    let one_placement = one_page(vec![placement(0, 0, 0, 0, 640)]);
    let valid = input(ParagraphBorderPolicy::default());
    let mut zero_column = valid.clone();
    zero_column.column.width = 0;
    assert!(matches!(
        plan_paragraph_borders(
            &one_layout,
            &one_placement,
            &[zero_column],
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));
    let zero_edge = ParagraphBorderPolicy {
        top: Some(BorderLine {
            width: 0,
            color: color(0, 0, 0),
            style: BorderStyle::Solid,
            spacing: 0,
        }),
        ..ParagraphBorderPolicy::default()
    };
    assert!(matches!(
        plan_paragraph_borders(
            &one_layout,
            &one_placement,
            &[input(zero_edge)],
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidPolicy(_))
    ));
    assert_eq!(
        ColumnBounds::new(i64::MAX, 1),
        Err(BorderPlanError::ArithmeticOverflow)
    );

    let two_layouts = vec![
        layout(vec![line(0..2, 640, 0, 100)]),
        layout(vec![line(0..2, 640, 0, 100)]),
    ];
    let both_inputs = [valid.clone(), valid.clone()];
    let duplicate = one_page(vec![
        placement(0, 0, 0, 0, 640),
        placement(0, 0, 0, 640, 640),
    ]);
    assert!(matches!(
        plan_paragraph_borders(
            &two_layouts,
            &duplicate,
            &both_inputs,
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));
    let missing = one_page(vec![placement(0, 0, 0, 0, 640)]);
    assert!(matches!(
        plan_paragraph_borders(
            &two_layouts,
            &missing,
            &both_inputs,
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));
    let out_of_range = one_page(vec![placement(0, 2, 0, 0, 640)]);
    assert!(matches!(
        plan_paragraph_borders(
            &two_layouts,
            &out_of_range,
            &both_inputs,
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));
    let wrong_page = Pagination {
        pages: vec![Page { placements: 0..1 }],
        placements: vec![placement(1, 0, 0, 0, 640)],
        diagnostics: Vec::new(),
    };
    assert!(matches!(
        plan_paragraph_borders(
            &one_layout,
            &wrong_page,
            std::slice::from_ref(&valid),
            BorderPlanLimits::default()
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));

    let two_pages = Pagination {
        pages: vec![Page { placements: 0..1 }, Page { placements: 1..2 }],
        placements: vec![placement(0, 0, 0, 0, 640), placement(1, 1, 0, 0, 640)],
        diagnostics: Vec::new(),
    };
    assert!(matches!(
        plan_paragraph_borders(
            &two_layouts,
            &two_pages,
            &both_inputs,
            BorderPlanLimits {
                max_work: 5,
                ..BorderPlanLimits::default()
            }
        ),
        Err(BorderPlanError::LimitExceeded("work"))
    ));
    assert!(matches!(
        plan_paragraph_borders(
            &two_layouts,
            &two_pages,
            &both_inputs,
            BorderPlanLimits {
                max_fragments: 1,
                ..BorderPlanLimits::default()
            }
        ),
        Err(BorderPlanError::LimitExceeded("fragments"))
    ));
}

#[test]
fn a_group_split_across_pages_gets_closed_local_frames_and_no_intra_paragraph_between() {
    let layouts = vec![
        layout(vec![line(0..4, 640, 0, 100), line(4..8, 640, 0, 100)]),
        layout(vec![line(0..4, 640, 0, 100)]),
    ];
    let placements = vec![
        placement(0, 0, 0, 0, 640),
        placement(1, 0, 1, 0, 640),
        placement(1, 1, 0, 720, 640),
    ];
    let pagination = Pagination {
        pages: vec![Page { placements: 0..1 }, Page { placements: 1..3 }],
        placements,
        diagnostics: Vec::new(),
    };
    let mut policy = frame_policy(BorderStyle::Solid);
    policy.between = Some(border(BorderStyle::Solid, 64, 0, color(20, 40, 60)));
    let plan = plan_paragraph_borders(
        &layouts,
        &pagination,
        &[input(policy.clone()), input(policy)],
        BorderPlanLimits::default(),
    )
    .unwrap();
    let count = |page: usize, predicate: fn(&BorderPrimitive) -> bool| {
        plan.pages[page]
            .primitives
            .iter()
            .filter(|primitive| predicate(primitive))
            .count()
    };
    assert_eq!(count(0, |p| matches!(p, BorderPrimitive::Stroke { .. })), 4);
    assert_eq!(count(1, |p| matches!(p, BorderPrimitive::Stroke { .. })), 5);
}

#[test]
fn solid_double_dashed_and_dotted_styles_have_bounded_deterministic_geometry() {
    let layouts = vec![layout(vec![line(0..2, 640, 0, 10)])];
    let policy = ParagraphBorderPolicy {
        top: Some(border(BorderStyle::Double, 60, 0, color(1, 2, 3))),
        bottom: Some(border(BorderStyle::Dashed, 64, 0, color(1, 2, 3))),
        left: Some(border(BorderStyle::Dotted, 64, 0, color(1, 2, 3))),
        right: Some(border(BorderStyle::Solid, 64, 0, color(1, 2, 3))),
        between: None,
        shading: None,
    };
    let plan = plan_paragraph_borders(
        &layouts,
        &one_page(vec![placement(0, 0, 0, 0, 640)]),
        &[input(policy)],
        BorderPlanLimits::default(),
    )
    .unwrap();
    let primitives = &plan.pages[0].primitives;
    assert!(
        primitives
            .iter()
            .any(|primitive| matches!(primitive, BorderPrimitive::Dot { .. }))
    );
    let dot_centers = primitives
        .iter()
        .filter_map(|primitive| match primitive {
            BorderPrimitive::Dot { center, .. } => Some(center.y),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(dot_centers.len() > 1);
    assert!(dot_centers.windows(2).all(|pair| pair[1] - pair[0] == 192));
    assert!(
        primitives
            .iter()
            .filter(|primitive| matches!(primitive, BorderPrimitive::Stroke { .. }))
            .count()
            > 3
    );
    let too_small = BorderLine::new(2, color(0, 0, 0), BorderStyle::Double, 0);
    assert!(matches!(too_small, Err(BorderPlanError::InvalidPolicy(_))));
}

#[test]
fn malformed_placements_source_order_and_dash_budgets_return_errors() {
    let layouts = vec![layout(vec![
        line(0..3, 640, 0, 100),
        line(2..4, 640, 0, 100),
    ])];
    let pagination = one_page(vec![
        placement(0, 0, 0, 0, 640),
        placement(0, 0, 1, 640, 640),
    ]);
    assert!(matches!(
        plan_paragraph_borders(
            &layouts,
            &pagination,
            &[input(frame_policy(BorderStyle::Solid))],
            BorderPlanLimits::default(),
        ),
        Err(BorderPlanError::InvalidInput(_))
    ));

    let valid_layout = vec![layout(vec![line(0..3, 640, 0, 100)])];
    let valid_pagination = one_page(vec![placement(0, 0, 0, 0, 640)]);
    let dashed = ParagraphBorderPolicy {
        top: Some(border(BorderStyle::Dashed, 1, 0, color(0, 0, 0))),
        ..ParagraphBorderPolicy::default()
    };
    let limits = BorderPlanLimits {
        max_work: 100,
        max_fragments: 10,
        max_primitives: 2,
    };
    let error = plan_paragraph_borders(&valid_layout, &valid_pagination, &[input(dashed)], limits)
        .unwrap_err();
    assert_eq!(error, BorderPlanError::LimitExceeded("primitives"));
}
