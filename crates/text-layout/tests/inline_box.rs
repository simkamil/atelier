// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline boxes in a line of text (R7b-1): width, ascent, wrapping, refusals.

use atelier_font::manager::FontManager;
use atelier_text_layout::{
    INLINE_BOX_GLYPH, InlineBox, LayoutError, LayoutOptions, ParagraphLayout, layout_paragraph,
    layout_paragraph_boxes,
};
use atelier_text_model::rich::{Paragraph, ResolvedProperties};

const BOX: &str = "\u{fffc}";

fn lay(text: &str, boxes: &[InlineBox], width: i64) -> Result<ParagraphLayout, LayoutError> {
    layout_paragraph_boxes(
        &Paragraph::with_text(text),
        &ResolvedProperties::default(),
        &FontManager::new(),
        LayoutOptions::new(width),
        boxes,
    )
}

fn at(text: &str, n: usize) -> usize {
    text.match_indices(BOX).nth(n).unwrap().0
}

#[test]
fn a_box_takes_its_width_and_raises_the_line_to_its_height() {
    let text = format!("ab{BOX}cd");
    let plain = layout_paragraph(
        &Paragraph::with_text("abcd"),
        &ResolvedProperties::default(),
        &FontManager::new(),
        LayoutOptions::new(64_000),
    )
    .unwrap();
    let item = InlineBox {
        offset: at(&text, 0),
        width: 30 * 64,
        height: 100 * 64,
    };
    let boxed = lay(&text, &[item], 64_000).unwrap();
    assert_eq!(boxed.lines.len(), 1);
    assert_eq!(boxed.lines[0].width, plain.lines[0].width + 30 * 64);
    // The ascent is the box's; the descent stays the text's.
    let descent = plain.lines[0].height - plain.lines[0].baseline;
    assert_eq!(boxed.lines[0].baseline, 100 * 64);
    assert_eq!(boxed.lines[0].height - boxed.lines[0].baseline, descent);
    let glyph = boxed.lines[0]
        .fragments
        .iter()
        .flat_map(|f| &f.glyphs)
        .find(|g| g.glyph_id == INLINE_BOX_GLYPH)
        .unwrap();
    assert_eq!(glyph.source, 2..5);
    assert_eq!(glyph.x_advance, 30 * 64);
    // The characters after the box follow it.
    let next = boxed.lines[0]
        .fragments
        .iter()
        .flat_map(|f| &f.glyphs)
        .find(|g| g.source.start == 5)
        .unwrap();
    assert_eq!(next.x, glyph.x + 30 * 64);
}

#[test]
fn a_box_lower_than_the_text_keeps_the_text_line() {
    let text = format!("a{BOX}b");
    let plain = lay("ab", &[], 64_000).unwrap();
    let boxed = lay(
        &text,
        &[InlineBox {
            offset: 1,
            width: 64,
            height: 64,
        }],
        64_000,
    )
    .unwrap();
    assert_eq!(boxed.lines[0].height, plain.lines[0].height);
    assert_eq!(boxed.lines[0].baseline, plain.lines[0].baseline);
}

#[test]
fn a_paragraph_that_is_only_a_box_has_one_line_of_its_height() {
    let boxed = lay(
        BOX,
        &[InlineBox {
            offset: 0,
            width: 40 * 64,
            height: 50 * 64,
        }],
        64_000,
    )
    .unwrap();
    assert_eq!(boxed.lines.len(), 1);
    assert_eq!(boxed.lines[0].width, 40 * 64);
    assert_eq!(boxed.lines[0].baseline, 50 * 64);
}

#[test]
fn boxes_wrap_like_words_and_an_oversized_box_gets_its_own_line() {
    let text = format!("{BOX} {BOX} {BOX}");
    let items: Vec<InlineBox> = (0..3)
        .map(|n| InlineBox {
            offset: at(&text, n),
            width: 100 * 64,
            height: 20 * 64,
        })
        .collect();
    // Room for two boxes and the space between them, not three.
    let wrapped = lay(&text, &items, 220 * 64).unwrap();
    assert_eq!(wrapped.lines.len(), 2);
    assert_eq!(wrapped.lines[0].source, 0..8);
    // A box wider than the line is alone on its line, never split or dropped.
    let narrow = lay(&text, &items, 50 * 64).unwrap();
    let per_line: Vec<usize> = narrow
        .lines
        .iter()
        .map(|line| {
            line.fragments
                .iter()
                .flat_map(|f| &f.glyphs)
                .filter(|g| g.glyph_id == INLINE_BOX_GLYPH)
                .count()
        })
        .collect();
    assert!(per_line.iter().all(|n| *n <= 1), "{per_line:?}");
    assert_eq!(per_line.iter().sum::<usize>(), 3);
}

#[test]
fn invalid_boxes_are_refused_without_panic() {
    let text = format!("a{BOX}");
    let good = InlineBox {
        offset: 1,
        width: 64,
        height: 64,
    };
    assert!(lay(&text, &[good], 8000).is_ok());
    // Not on the stand-in, past the end, unsorted, empty.
    for bad in [
        vec![InlineBox { offset: 0, ..good }],
        vec![InlineBox { offset: 2, ..good }],
        vec![InlineBox { offset: 99, ..good }],
        vec![good, good],
        vec![InlineBox { width: 0, ..good }],
        vec![InlineBox { height: -1, ..good }],
    ] {
        assert!(
            matches!(
                lay(&text, &bad, 8000),
                Err(LayoutError::InvalidParagraph(_))
            ),
            "{bad:?}"
        );
    }
}
