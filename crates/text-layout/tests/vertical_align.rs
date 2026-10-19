// SPDX-License-Identifier: AGPL-3.0-or-later
//! Superscript and subscript (US-240, T240-C): two thirds of the size, the
//! baseline raised by a third of the size or lowered by 14 %, the neighbouring
//! text left where it was.

use atelier_font::manager::FontManager;
use atelier_text_layout::{LayoutOptions, ParagraphLayout, layout_paragraph};
use atelier_text_model::rich::{CharAttrs, CharRun, Paragraph, ResolvedProperties, VerticalAlign};

fn layout(shift: Option<VerticalAlign>) -> ParagraphLayout {
    let mut paragraph = Paragraph::with_text("ab cd");
    if shift.is_some() {
        paragraph.runs.push(CharRun {
            start: 3,
            end: 5,
            attrs: CharAttrs {
                vertical_align: shift,
                ..CharAttrs::default()
            },
        });
    }
    layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &FontManager::new(),
        LayoutOptions::new(64 * 400),
    )
    .unwrap()
}

/// `(size, y)` of the glyphs of the paragraph text `range` of the first line.
fn glyphs(layout: &ParagraphLayout, from: usize) -> Vec<(i64, i64, i64)> {
    layout.lines[0]
        .fragments
        .iter()
        .flat_map(|f| &f.glyphs)
        .filter(|g| g.source.start >= from)
        .map(|g| (g.size, g.y, g.x_advance))
        .collect()
}

#[test]
fn a_superscript_is_smaller_and_raised_and_a_subscript_smaller_and_lowered() {
    let plain = layout(None);
    let up = layout(Some(VerticalAlign::Superscript));
    let down = layout(Some(VerticalAlign::Subscript));
    let (plain_size, plain_y, plain_advance) = glyphs(&plain, 3)[0];
    let (up_size, up_y, up_advance) = glyphs(&up, 3)[0];
    let (down_size, down_y, down_advance) = glyphs(&down, 3)[0];
    assert_eq!(plain_size, 22 * 32);
    assert_eq!(up_size, plain_size * 2 / 3);
    assert_eq!(down_size, plain_size * 2 / 3);
    assert!(
        up_advance < plain_advance && down_advance < plain_advance,
        "the glyphs are narrower"
    );
    // y is the baseline of the glyph in the paragraph: smaller is higher.
    assert!(
        up_y < up.lines[0].baseline,
        "raised above the baseline of the line"
    );
    assert!(down_y > down.lines[0].baseline, "lowered below it");
    assert_eq!(up.lines[0].baseline - up_y, plain_size * 33 / 100);
    assert_eq!(down_y - down.lines[0].baseline, plain_size * 14 / 100);
    let _ = plain_y;
    // The text before is not moved: same size, on the baseline of the line.
    for layout in [&up, &down] {
        let before = glyphs(layout, 0)[0];
        assert_eq!(before.0, plain_size);
        assert_eq!(before.1, layout.lines[0].baseline);
    }
    assert!(
        plain.unsupported.is_empty() && up.unsupported.is_empty(),
        "no baseline shift is reported as unsupported any more"
    );
}
