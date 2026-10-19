// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::FontManager;
use atelier_text_layout::{LayoutError, LayoutOptions, layout_paragraph};
use atelier_text_model::rich::{
    Alignment, CharAttrs, CharRun, LineRule, LineSpacing, Paragraph, ResolvedProperties,
};
use atelier_unicode::segmentation::grapheme_boundaries;

fn fonts() -> FontManager<'static> {
    FontManager::new()
}

fn layout(text: &str, width: i64) -> atelier_text_layout::ParagraphLayout {
    let paragraph = Paragraph::with_text(text);
    layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(width),
    )
    .unwrap()
}

#[test]
fn empty_paragraph_produces_one_metric_line() {
    let result = layout("", 6400);
    assert_eq!(result.lines.len(), 1);
    assert_eq!(result.lines[0].source, 0..0);
    assert!(result.lines[0].height > 0);
    assert!(result.lines[0].baseline > 0);
}

#[test]
fn mandatory_lf_and_crlf_are_consumed_without_glyphs() {
    for text in ["ab\ncd", "ab\r\ncd"] {
        let result = layout(text, 6400);
        assert_eq!(result.lines.len(), 2);
        assert_eq!(
            &text[result.lines[0].source.clone()],
            if text.contains("\r\n") {
                "ab\r\n"
            } else {
                "ab\n"
            }
        );
        assert_eq!(&text[result.lines[1].source.clone()], "cd");
        assert!(result.lines[0].mandatory_break);
        assert!(
            result.lines[0]
                .fragments
                .iter()
                .flat_map(|f| &f.glyphs)
                .all(|g| !text[g.source.clone()].contains(['\r', '\n']))
        );
    }
}

#[test]
fn terminal_mandatory_breaks_keep_the_following_empty_line() {
    for text in ["x\n", "\n", "x\r\n", "x\n\n"] {
        let expected = if text == "x\n\n" { 3 } else { 2 };
        let mut paragraph = Paragraph::with_text(text);
        paragraph.props.line_spacing = Some(LineSpacing {
            rule: LineRule::Exact,
            value: 240,
        });
        let result = layout_paragraph(
            &paragraph,
            &ResolvedProperties::default(),
            &fonts(),
            LayoutOptions::new(6400),
        )
        .unwrap();
        assert_eq!(result.lines.len(), expected, "{text:?}");
        assert!(result.lines.last().unwrap().final_line);
        assert!(!result.lines.last().unwrap().mandatory_break);
        assert_eq!(result.lines.last().unwrap().source.end, text.len());
        assert!(result.lines[0].mandatory_break);
        assert!(!result.lines[0].final_line);

        let mut options = LayoutOptions::new(6400);
        options.max_lines = expected;
        assert!(
            layout_paragraph(
                &paragraph,
                &ResolvedProperties::default(),
                &fonts(),
                options
            )
            .is_ok()
        );
        options.max_lines = expected - 1;
        assert!(matches!(
            layout_paragraph(
                &paragraph,
                &ResolvedProperties::default(),
                &fonts(),
                options
            ),
            Err(LayoutError::LimitExceeded("lines"))
        ));
    }
}

#[test]
fn uax_breaks_nonbreaking_spaces_and_zero_width_space_differ() {
    let nbsp = layout("a\u{00a0}b", 450);
    let ordinary_space = layout("a b", 450);
    assert_eq!(&"a\u{00a0}b"[nbsp.lines[0].source.clone()], "a");
    assert_eq!(&"a b"[ordinary_space.lines[0].source.clone()], "a ");

    let zero_width = layout("ab\u{200b}cd", 1200);
    assert!(zero_width.lines.len() >= 2);
    assert!(zero_width.lines.iter().any(|line| !line.mandatory_break));
}

#[test]
fn overlong_words_progress_at_grapheme_boundaries() {
    let text = "a\u{301}bcdef";
    let result = layout(text, 1);
    let boundaries = grapheme_boundaries(text);
    assert!(result.lines.len() >= 5);
    assert!(result.lines.iter().any(|line| line.width > 1)); // One grapheme may overflow; it is never split.
    let mut previous = 0;
    for line in &result.lines {
        assert!(boundaries.is_boundary(line.source.start).unwrap());
        assert!(boundaries.is_boundary(line.source.end).unwrap());
        assert_eq!(line.source.start, previous);
        previous = line.source.end;
    }
    assert_eq!(previous, text.len());
    for glyph in result
        .lines
        .iter()
        .flat_map(|line| &line.fragments)
        .flat_map(|f| &f.glyphs)
    {
        assert!(boundaries.is_boundary(glyph.source.start).unwrap());
        assert!(boundaries.is_boundary(glyph.source.end).unwrap());
    }
}

#[test]
fn trailing_spaces_keep_source_offsets_but_do_not_add_width_or_glyphs() {
    let plain = layout("word", 6400);
    let padded = layout("word   ", 6400);
    assert_eq!(plain.lines[0].width, padded.lines[0].width);
    assert_eq!(padded.lines[0].source, 0..7);
    assert!(
        padded.lines[0]
            .fragments
            .iter()
            .flat_map(|f| &f.glyphs)
            .all(|g| g.source.end <= 4)
    );
}

#[test]
fn line_wrapping_does_not_split_a_shaping_cluster() {
    let text = "office";
    let result = layout(text, 1);
    let bytes = text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let shaped = atelier_shape::LatinShaper::new(
        fonts()
            .resolve(
                &atelier_font::manager::FontRequest::new(
                    ["Calibri"],
                    400,
                    atelier_font::manager::FontStyle::Normal,
                )
                .with_generic(atelier_font::manager::GenericFamily::SansSerif),
            )
            .unwrap()
            .face,
    )
    .unwrap()
    .shape_text(text, atelier_shape::ShapeOptions::default())
    .unwrap();
    for glyph in shaped.glyphs {
        let cluster_start = bytes[glyph.cluster.start];
        let cluster_end = bytes[glyph.cluster.end];
        for line in &result.lines {
            assert!(!(line.source.start > cluster_start && line.source.start < cluster_end));
            assert!(!(line.source.end > cluster_start && line.source.end < cluster_end));
        }
    }
}

#[test]
fn alignment_and_twip_indents_have_numeric_geometry() {
    let text = "a b c";
    let width = 6400;
    let mut paragraph = Paragraph::with_text(text);
    paragraph.props.alignment = Some(Alignment::Left);
    paragraph.props.indent_left = Some(100);
    let left = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(width),
    )
    .unwrap();
    assert_eq!(left.lines[0].x, 320); // 100 twips = 5 pt = 320/64 pt.

    paragraph.props.indent_left = None;
    paragraph.props.indent_right = Some(100);
    paragraph.props.alignment = Some(Alignment::Right);
    let right = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(width),
    )
    .unwrap();
    assert_eq!(right.lines[0].x + right.lines[0].width, width - 320);

    paragraph.props.indent_right = None;
    paragraph.props.alignment = Some(Alignment::Center);
    let center = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(width),
    )
    .unwrap();
    assert_eq!(center.lines[0].x, (width - center.lines[0].width) / 2);
}

#[test]
fn hanging_indent_applies_left_indent_to_continuation_lines() {
    let mut paragraph = Paragraph::with_text("one two three four five six");
    paragraph.props.indent_left = Some(120);
    paragraph.props.indent_first_line = Some(-40);
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(1400),
    )
    .unwrap();
    assert!(result.lines.len() > 1);
    assert_eq!(result.lines[0].x, 256); // 80 twips.
    assert!(result.lines[1..].iter().all(|line| line.x == 384)); // 120 twips.
}

#[test]
fn justification_expands_only_nonfinal_lines() {
    let text = "one two three four five";
    let mut paragraph = Paragraph::with_text(text);
    paragraph.props.alignment = Some(Alignment::Justify);
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(2500),
    )
    .unwrap();
    assert!(result.lines.len() > 1);
    assert!(
        result.lines[..result.lines.len() - 1]
            .iter()
            .any(|line| line.justified)
    );
    assert!(!result.lines.last().unwrap().justified);
    assert!(
        result.lines[..result.lines.len() - 1]
            .iter()
            .filter(|line| line.justified)
            .all(|line| line.width == 2500)
    );
}

#[test]
fn paragraph_spacing_and_mandatory_justification_have_numeric_geometry() {
    let mut paragraph = Paragraph::with_text("a b\nc d");
    paragraph.props.alignment = Some(Alignment::Justify);
    paragraph.props.space_before = Some(120);
    paragraph.props.space_after = Some(240);
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Exact,
        value: 240,
    });
    let base_paragraph = Paragraph::with_text("a b\nc d");
    let base = layout_paragraph(
        &base_paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].baseline - base.lines[0].baseline, 384);
    assert_eq!(
        result.height,
        384 + result.lines.iter().map(|line| line.height).sum::<i64>() + 768
    );
    assert!(result.lines[0].mandatory_break);
    assert!(!result.lines[0].justified);
    assert!(result.lines[1].final_line);
    assert!(!result.lines[1].justified);
}

#[test]
fn requested_font_fallback_keeps_the_face_selected_for_the_scalar() {
    let manager = fonts();
    let request = atelier_font::manager::FontRequest::new(
        ["Calibri"],
        400,
        atelier_font::manager::FontStyle::Normal,
    )
    .with_generic(atelier_font::manager::GenericFamily::SansSerif);
    let requested = manager.resolve(&request).unwrap().id;
    let fallback_char = (0x20..=0x10ffff)
        .filter_map(char::from_u32)
        .find(|&character| {
            manager
                .resolve_for_char(&request, character)
                .is_ok_and(|resolved| resolved.id != requested)
        })
        .expect("embedded registry includes a fallback face with distinct coverage");
    let mut paragraph = Paragraph::with_text(&fallback_char.to_string());
    paragraph.runs.push(CharRun {
        start: 0,
        end: paragraph.text.len(),
        attrs: CharAttrs {
            font: Some("Calibri".into()),
            ..CharAttrs::default()
        },
    });
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &manager,
        LayoutOptions::new(6400),
    )
    .unwrap();
    let glyph = &result.lines[0].fragments[0].glyphs[0];
    assert_eq!(
        glyph.font_id,
        manager
            .resolve_for_char(&request, fallback_char)
            .unwrap()
            .id
    );
}

#[test]
fn line_spacing_rules_use_twips_and_auto_240ths() {
    let mut paragraph = Paragraph::with_text("a\nb");
    let base = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Exact,
        value: 200,
    });
    let exact = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert_eq!(exact.lines[0].height, 640); // 200 twips = 10 pt.
    assert_eq!(exact.lines[1].baseline - exact.lines[0].baseline, 640);
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::AtLeast,
        value: 2000,
    });
    let minimum = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert_eq!(minimum.lines[0].height, 6400);
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Auto,
        value: 480,
    });
    let double = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert_eq!(double.lines[0].height, base.lines[0].height * 2);
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Auto,
        value: 360,
    });
    let one_half = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert_eq!(one_half.lines[0].height, (base.lines[0].height * 3 + 1) / 2);
}

#[test]
fn mixed_font_sizes_change_only_their_line_extents() {
    let text = "Big\nsmall";
    let mut paragraph = Paragraph::with_text(text);
    paragraph.runs.push(CharRun {
        start: 0,
        end: 3,
        attrs: CharAttrs {
            size_half_points: Some(48),
            font: Some("Carlito".into()),
            ..CharAttrs::default()
        },
    });
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    assert!(result.lines[0].height > result.lines[1].height);
    assert!(
        result.lines[0].fragments[0]
            .glyphs
            .iter()
            .all(|glyph| glyph.size == 1536)
    );
    assert!(
        result.lines[1].fragments[0]
            .glyphs
            .iter()
            .all(|glyph| glyph.size == 704)
    );
}

#[test]
fn rich_controls_laid_out_are_not_reported_and_tab_stops_are_not_unsupported() {
    let mut paragraph = Paragraph::with_text("link");
    paragraph.runs.push(CharRun {
        start: 0,
        end: 4,
        attrs: CharAttrs {
            vertical_align: Some(atelier_text_model::rich::VerticalAlign::Superscript),
            ..CharAttrs::default()
        },
    });
    let result = layout_paragraph(
        &paragraph,
        &ResolvedProperties::default(),
        &fonts(),
        LayoutOptions::new(6400),
    )
    .unwrap();
    // US-240 (T240-C): baseline shifts are laid out now (tests/vertical_align.rs), no longer reported.
    assert!(result.unsupported.is_empty());

    let tabs = layout("a\tb", 6400);
    assert!(!tabs.unsupported.contains(&"tab stops (US-211)"));
}

#[test]
fn malformed_width_and_budgets_return_typed_errors() {
    let paragraph = Paragraph::with_text("abc");
    assert!(matches!(
        layout_paragraph(
            &paragraph,
            &ResolvedProperties::default(),
            &fonts(),
            LayoutOptions::new(0)
        ),
        Err(LayoutError::InvalidOptions(_))
    ));
    let mut options = LayoutOptions::new(6400);
    options.max_text_bytes = 2;
    assert!(matches!(
        layout_paragraph(
            &paragraph,
            &ResolvedProperties::default(),
            &fonts(),
            options
        ),
        Err(LayoutError::LimitExceeded("text bytes"))
    ));
    let mut options = LayoutOptions::new(6400);
    options.max_work = 1;
    assert!(matches!(
        layout_paragraph(
            &paragraph,
            &ResolvedProperties::default(),
            &fonts(),
            options
        ),
        Err(LayoutError::LimitExceeded("layout work"))
    ));
    let missing = Paragraph::with_text("\u{10ffff}");
    assert!(matches!(
        layout_paragraph(
            &missing,
            &ResolvedProperties::default(),
            &fonts(),
            LayoutOptions::new(6400)
        ),
        Err(LayoutError::MissingGlyph('\u{10ffff}'))
    ));
    let mut paragraph = Paragraph::with_text("a");
    paragraph.props.line_spacing = Some(LineSpacing {
        rule: LineRule::Exact,
        value: 0,
    });
    assert!(matches!(
        layout_paragraph(
            &paragraph,
            &ResolvedProperties::default(),
            &fonts(),
            LayoutOptions::new(6400)
        ),
        Err(LayoutError::InvalidOptions(_))
    ));
    let mut options = LayoutOptions::new(6400);
    options.max_glyphs = 1;
    assert!(matches!(
        layout_paragraph(
            &Paragraph::with_text("abc"),
            &ResolvedProperties::default(),
            &fonts(),
            options
        ),
        Err(LayoutError::LimitExceeded("glyphs"))
    ));
    let mut options = LayoutOptions::new(6400);
    options.max_lines = 1;
    assert!(matches!(
        layout_paragraph(
            &Paragraph::with_text("a\nb"),
            &ResolvedProperties::default(),
            &fonts(),
            options
        ),
        Err(LayoutError::LimitExceeded("lines"))
    ));
}
