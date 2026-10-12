// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::{FontManager, FontRequest, FontStyle};
use atelier_text_layout::caret::{
    CaretAffinity, CaretError, CaretLayout, CaretOptions, PositionedLine, TextPosition,
    VerticalDirection,
};
use atelier_text_layout::{LayoutFragment, LayoutGlyph, LayoutLine};
use core::ops::Range;

fn glyph(manager: &FontManager<'_>, source: Range<usize>, x: i64, advance: i64) -> LayoutGlyph {
    let face = manager
        .resolve(&FontRequest::new(["Calibri"], 400, FontStyle::Normal))
        .unwrap();
    LayoutGlyph {
        glyph_id: 1,
        source,
        x,
        y: 80,
        x_advance: advance,
        y_advance: 0,
        x_offset: 0,
        y_offset: 0,
        font_id: face.id,
        family: "Calibri".into(),
        size: 640,
    }
}

fn line(source: Range<usize>, height: i64, glyphs: Vec<LayoutGlyph>) -> LayoutLine {
    let fragments = if glyphs.is_empty() {
        Vec::new()
    } else {
        vec![LayoutFragment {
            source: glyphs.first().unwrap().source.start..glyphs.last().unwrap().source.end,
            font_id: glyphs[0].font_id,
            family: "Calibri".into(),
            size: 640,
            glyphs,
        }]
    };
    LayoutLine {
        source,
        x: 0,
        baseline: height - 10,
        width: 200,
        height,
        mandatory_break: false,
        final_line: true,
        justified: false,
        fragments,
    }
}

fn pos(paragraph_index: usize, byte_offset: usize) -> TextPosition {
    TextPosition {
        paragraph_index,
        byte_offset,
    }
}

#[test]
fn combining_grapheme_rejects_internal_byte_and_ligature_stops_are_interpolated() {
    let manager = FontManager::new();
    let combining = "e\u{301}";
    let l1 = line(0..3, 100, vec![glyph(&manager, 0..3, 0, 90)]);
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text: combining,
        layout: &l1,
        top: 5,
        x_translation: 0,
    }];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    assert_eq!(
        layout
            .caret_rect(pos(0, 3), CaretAffinity::Downstream)
            .unwrap()
            .x,
        90
    );
    assert_eq!(
        layout.caret_rect(pos(0, 1), CaretAffinity::Downstream),
        Err(CaretError::InvalidPosition)
    );

    let ligature = "fi";
    let l2 = line(0..2, 100, vec![glyph(&manager, 0..2, 0, 120)]);
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text: ligature,
        layout: &l2,
        top: 0,
        x_translation: 0,
    }];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    assert_eq!(
        layout
            .caret_rect(pos(0, 1), CaretAffinity::Downstream)
            .unwrap()
            .x,
        60
    );
}

#[test]
fn independent_glyph_advances_are_not_interpolated_as_one_cluster() {
    let manager = FontManager::new();
    let text = "ab";
    let layout_line = line(
        0..2,
        100,
        vec![glyph(&manager, 0..1, 0, 10), glyph(&manager, 1..2, 10, 30)],
    );
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &layout_line,
        top: 0,
        x_translation: 0,
    }];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    assert_eq!(
        layout
            .caret_rect(pos(0, 1), CaretAffinity::Downstream)
            .unwrap()
            .x,
        10
    );
    assert_eq!(
        layout
            .caret_rect(pos(0, 2), CaretAffinity::Downstream)
            .unwrap()
            .x,
        40
    );
}

#[test]
fn same_source_multiglyph_cluster_aggregates_advance_once() {
    let manager = FontManager::new();
    let text = "a";
    let layout_line = line(
        0..1,
        100,
        vec![glyph(&manager, 0..1, 0, 12), glyph(&manager, 0..1, 12, 28)],
    );
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &layout_line,
        top: 0,
        x_translation: 0,
    }];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    assert_eq!(
        layout
            .caret_rect(pos(0, 1), CaretAffinity::Downstream)
            .unwrap()
            .x,
        40
    );
}

#[test]
fn crlf_before_and_after_map_to_previous_and_following_lines() {
    let text = "a\r\nb";
    let manager = FontManager::new();
    let mut first = line(0..3, 100, vec![glyph(&manager, 0..1, 0, 40)]);
    first.mandatory_break = true;
    let second = line(3..4, 120, vec![glyph(&manager, 3..4, 0, 50)]);
    let input = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text,
            layout: &first,
            top: 2,
            x_translation: 7,
        },
        PositionedLine {
            page_index: 1,
            paragraph_index: 0,
            text,
            layout: &second,
            top: 2,
            x_translation: 7,
        },
    ];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    let before = layout
        .caret_rect(pos(0, 1), CaretAffinity::Downstream)
        .unwrap();
    let after = layout
        .caret_rect(pos(0, 3), CaretAffinity::Upstream)
        .unwrap();
    assert_eq!((before.page_index, before.x), (0, 47));
    assert_eq!((after.page_index, after.x), (1, 7));
    assert_eq!(layout.hit_test(1, 30, 50).unwrap(), pos(0, 3));
    assert_eq!(layout.hit_test(2, 0, 0), Err(CaretError::InvalidPage));
}

#[test]
fn hit_test_chooses_the_y_containing_line_before_considering_x() {
    let manager = FontManager::new();
    let first = line(0..1, 20, vec![glyph(&manager, 0..1, 10_000, 10)]);
    let second = line(0..1, 20, vec![glyph(&manager, 0..1, 0, 10)]);
    let input = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "a",
            layout: &first,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 1,
            text: "b",
            layout: &second,
            top: 30,
            x_translation: 0,
        },
    ];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    assert_eq!(layout.hit_test(0, 0, 10).unwrap(), pos(0, 0));
}

#[test]
fn trailing_spaces_have_distinct_zero_advance_positions_and_selection_keeps_pages() {
    let text = "a   ";
    let manager = FontManager::new();
    let only_a = line(0..4, 100, vec![glyph(&manager, 0..1, 0, 40)]);
    let input = [PositionedLine {
        page_index: 3,
        paragraph_index: 0,
        text,
        layout: &only_a,
        top: 10,
        x_translation: 5,
    }];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    let xs = (1..=4)
        .map(|byte| {
            layout
                .caret_rect(pos(0, byte), CaretAffinity::Downstream)
                .unwrap()
                .x
        })
        .collect::<Vec<_>>();
    assert_eq!(xs, [45, 45, 45, 45]);
    let rects = layout.selection_rects(pos(0, 4)..pos(0, 0)).unwrap();
    assert_eq!(rects.len(), 1);
    assert_eq!(
        (rects[0].page_index, rects[0].x, rects[0].width),
        (3, 5, 40)
    );
}

#[test]
fn vertical_motion_retains_preferred_column_across_a_short_line() {
    let manager = FontManager::new();
    let long1 = line(
        0..6,
        100,
        vec![
            glyph(&manager, 0..1, 0, 20),
            glyph(&manager, 1..2, 20, 20),
            glyph(&manager, 2..3, 40, 20),
            glyph(&manager, 3..4, 60, 20),
            glyph(&manager, 4..5, 80, 20),
            glyph(&manager, 5..6, 100, 20),
        ],
    );
    let short = line(0..1, 100, vec![glyph(&manager, 0..1, 0, 20)]);
    let long2 = long1.clone();
    let texts = ["abcdef", "x", "abcdef"];
    let input = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: texts[0],
            layout: &long1,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 1,
            text: texts[1],
            layout: &short,
            top: 100,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 2,
            text: texts[2],
            layout: &long2,
            top: 200,
            x_translation: 0,
        },
    ];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    let (middle, preferred) = layout
        .move_vertical(pos(0, 5), None, VerticalDirection::Down)
        .unwrap();
    assert_eq!(preferred, 100);
    assert_eq!(middle, pos(1, 1));
    let (last, preferred) = layout
        .move_vertical(middle, Some(preferred), VerticalDirection::Down)
        .unwrap();
    assert_eq!(preferred, 100);
    assert_eq!(last, pos(2, 5));
    let (left, x) = layout
        .move_vertical(middle, Some(i64::MIN), VerticalDirection::Down)
        .unwrap();
    assert_eq!((left, x), (pos(2, 0), i64::MIN));
    let (right, x) = layout
        .move_vertical(middle, Some(i64::MAX), VerticalDirection::Down)
        .unwrap();
    assert_eq!((right, x), (pos(2, 6), i64::MAX));
}

#[test]
fn affinity_aware_upward_moves_keep_the_preceding_soft_wrap_line() {
    let manager = FontManager::new();
    let text = "abcdefghi";
    let first = line(
        0..3,
        100,
        vec![
            glyph(&manager, 0..1, 0, 20),
            glyph(&manager, 1..2, 20, 20),
            glyph(&manager, 2..3, 40, 20),
        ],
    );
    let second = line(
        3..6,
        100,
        vec![
            glyph(&manager, 3..4, 0, 20),
            glyph(&manager, 4..5, 20, 20),
            glyph(&manager, 5..6, 40, 20),
        ],
    );
    let third = line(
        6..9,
        100,
        vec![
            glyph(&manager, 6..7, 0, 20),
            glyph(&manager, 7..8, 20, 20),
            glyph(&manager, 8..9, 40, 20),
        ],
    );
    let input = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text,
            layout: &first,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text,
            layout: &second,
            top: 100,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text,
            layout: &third,
            top: 200,
            x_translation: 0,
        },
    ];
    let layout = CaretLayout::new(&input, CaretOptions::default()).unwrap();
    let (position, affinity, preferred) = layout
        .move_vertical_with_affinity(
            pos(0, 8),
            CaretAffinity::Downstream,
            Some(1_000),
            VerticalDirection::Up,
        )
        .unwrap();
    assert_eq!(
        (position, affinity, preferred),
        (pos(0, 6), CaretAffinity::Upstream, 1_000)
    );
    assert_eq!(layout.caret_rect(position, affinity).unwrap().line_index, 1);

    let (position, affinity, preferred) = layout
        .move_vertical_with_affinity(position, affinity, Some(preferred), VerticalDirection::Up)
        .unwrap();
    assert_eq!(
        (position, affinity, preferred),
        (pos(0, 3), CaretAffinity::Upstream, 1_000)
    );
    assert_eq!(layout.caret_rect(position, affinity).unwrap().line_index, 0);

    let (position, affinity, preferred) = layout
        .move_vertical_with_affinity(position, affinity, Some(preferred), VerticalDirection::Down)
        .unwrap();
    assert_eq!((position, affinity), (pos(0, 6), CaretAffinity::Upstream));
    assert_eq!(layout.caret_rect(position, affinity).unwrap().line_index, 1);
    let (position, affinity, preferred) = layout
        .move_vertical_with_affinity(position, affinity, Some(preferred), VerticalDirection::Down)
        .unwrap();
    assert_eq!(position, pos(0, 9));
    assert_eq!(preferred, 1_000);
    assert_eq!(layout.caret_rect(position, affinity).unwrap().line_index, 2);
}

#[test]
fn invalid_ranges_geometry_and_exhausted_budgets_fail_without_partial_results() {
    let text = "ab";
    let manager = FontManager::new();
    let overlapping = line(
        0..2,
        100,
        vec![
            glyph(&manager, 0..2, 0, 100),
            glyph(&manager, 1..2, 100, 20),
        ],
    );
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &overlapping,
        top: 0,
        x_translation: 0,
    }];
    assert!(matches!(
        CaretLayout::new(&input, CaretOptions::default()),
        Err(CaretError::InvalidInput(_))
    ));

    let missing = line(0..2, 100, Vec::new());
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &missing,
        top: 0,
        x_translation: 0,
    }];
    assert!(matches!(
        CaretLayout::new(&input, CaretOptions::default()),
        Err(CaretError::InvalidInput(
            "visible text is missing glyph geometry"
        ))
    ));

    let valid = line(
        0..2,
        100,
        vec![glyph(&manager, 0..1, 0, 40), glyph(&manager, 1..2, 40, 40)],
    );
    for height in [0, -1] {
        let mut invalid_height = valid.clone();
        invalid_height.height = height;
        assert!(matches!(
            CaretLayout::new(
                &[PositionedLine {
                    page_index: 0,
                    paragraph_index: 0,
                    text,
                    layout: &invalid_height,
                    top: 0,
                    x_translation: 0,
                }],
                CaretOptions::default(),
            ),
            Err(CaretError::InvalidInput(_))
        ));
    }
    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &valid,
        top: i64::MAX,
        x_translation: 0,
    }];
    assert!(matches!(
        CaretLayout::new(&input, CaretOptions::default()),
        Err(CaretError::ArithmeticOverflow)
    ));

    let input = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text,
        layout: &valid,
        top: 0,
        x_translation: 0,
    }];
    let options = CaretOptions {
        max_stops: 1,
        ..CaretOptions::default()
    };
    assert!(matches!(
        CaretLayout::new(&input, options),
        Err(CaretError::LimitExceeded("stops"))
    ));

    let options = CaretOptions {
        max_work: 1,
        ..CaretOptions::default()
    };
    assert!(matches!(
        CaretLayout::new(&input, options),
        Err(CaretError::LimitExceeded("work"))
    ));

    let options = CaretOptions {
        max_lines: 1,
        ..CaretOptions::default()
    };
    let mut first = line(0..2, 100, vec![glyph(&manager, 0..1, 0, 40)]);
    first.mandatory_break = true;
    let second = line(2..3, 100, vec![glyph(&manager, 2..3, 0, 40)]);
    let input = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "a\nb",
            layout: &first,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "a\nb",
            layout: &second,
            top: 100,
            x_translation: 0,
        },
    ];
    assert!(matches!(
        CaretLayout::new(&input, options),
        Err(CaretError::LimitExceeded("lines"))
    ));
}

#[test]
fn terminal_break_needs_an_empty_following_line_and_line_boxes_must_not_overlap() {
    let manager = FontManager::new();
    let mut terminal = line(0..2, 100, vec![glyph(&manager, 0..1, 0, 40)]);
    terminal.mandatory_break = true;
    let invalid = [PositionedLine {
        page_index: 0,
        paragraph_index: 0,
        text: "a\n",
        layout: &terminal,
        top: 0,
        x_translation: 0,
    }];
    assert!(matches!(
        CaretLayout::new(&invalid, CaretOptions::default()),
        Err(CaretError::InvalidInput(
            "terminal mandatory break requires a following empty line"
        ))
    ));

    let mut empty = line(2..2, 100, Vec::new());
    empty.mandatory_break = false;
    let valid = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "a\n",
            layout: &terminal,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "a\n",
            layout: &empty,
            top: 100,
            x_translation: 0,
        },
    ];
    assert!(CaretLayout::new(&valid, CaretOptions::default()).is_ok());

    let first = line(0..1, 100, vec![glyph(&manager, 0..1, 0, 10)]);
    let second = line(1..2, 100, vec![glyph(&manager, 1..2, 0, 10)]);
    let overlapping = [
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "ab",
            layout: &first,
            top: 0,
            x_translation: 0,
        },
        PositionedLine {
            page_index: 0,
            paragraph_index: 0,
            text: "ab",
            layout: &second,
            top: 90,
            x_translation: 0,
        },
    ];
    assert!(matches!(
        CaretLayout::new(&overlapping, CaretOptions::default()),
        Err(CaretError::InvalidInput(
            "positioned lines are not ordered by page and top"
        ))
    ));
}
