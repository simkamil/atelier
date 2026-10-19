// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic rich paragraph line layout (US-210).
//!
//! Input paragraph offsets and output source ranges are UTF-8 byte offsets.
//! Font sizes, positions, advances and line metrics use 1/64 typographic point;
//! paragraph indents and explicit line spacing remain in twips at the API.

#![forbid(unsafe_code)]

pub mod borders;
pub mod caret;
pub mod flow;
pub mod incremental;
pub mod inline_object;
pub mod pagination;
pub mod stream;

use atelier_font::FontFace;
use atelier_font::manager::{
    FontId, FontManager, FontRequest, FontStyle, GenericFamily, ManagerError,
};
use atelier_shape::{LatinShaper, ShapeError, ShapeOptions};
use atelier_text_model::rich::{
    Alignment, CharAttrs, LineRule, LineSpacing, Paragraph, ResolvedProperties, VerticalAlign,
};
use atelier_unicode::line_break::{BreakKind, line_breaks};
use atelier_unicode::segmentation::grapheme_boundaries;
use core::ops::Range;
use std::error::Error;
use std::fmt;

pub mod tabs;

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_GLYPHS: usize = 65_536;
const MAX_LINES: usize = 16_384;
const MAX_WORK: usize = 4_000_000;

/// Resource ceilings and available paragraph width. Width uses 1/64 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutOptions {
    pub width: i64,
    pub max_text_bytes: usize,
    pub max_glyphs: usize,
    pub max_lines: usize,
    pub max_work: usize,
}

impl LayoutOptions {
    pub fn new(width: i64) -> Self {
        Self {
            width,
            max_text_bytes: MAX_TEXT_BYTES,
            max_glyphs: MAX_GLYPHS,
            max_lines: MAX_LINES,
            max_work: MAX_WORK,
        }
    }
}

/// Glyph id of the glyph an [`InlineBox`] produces: no font has it, painters
/// draw the box themselves (R7b).
pub const INLINE_BOX_GLYPH: u16 = u16::MAX;
/// The character that stands for an [`InlineBox`] in the paragraph text.
pub const INLINE_BOX_CHAR: char = '\u{FFFC}';

/// A box of fixed size on the baseline (a picture, R7b). The paragraph text
/// holds [`INLINE_BOX_CHAR`] at `offset`; the line gets one glyph of
/// [`INLINE_BOX_GLYPH`] there, `width` wide, which raises the line ascent to
/// `height` and keeps the descent of the text. Units are 1/64 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlineBox {
    pub offset: usize,
    pub width: i64,
    pub height: i64,
}

/// Typed layout failures. No partial paragraph layout is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    InvalidOptions(&'static str),
    LimitExceeded(&'static str),
    InvalidParagraph(&'static str),
    MissingGlyph(char),
    ArithmeticOverflow,
    Manager(ManagerError),
    Shaping(ShapeError),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(s) => write!(f, "invalid line-layout options: {s}"),
            Self::LimitExceeded(s) => write!(f, "line-layout limit exceeded: {s}"),
            Self::InvalidParagraph(s) => write!(f, "invalid paragraph for line layout: {s}"),
            Self::MissingGlyph(ch) => write!(f, "no registered font covers U+{:04X}", *ch as u32),
            Self::ArithmeticOverflow => f.write_str("line-layout arithmetic overflow"),
            Self::Manager(e) => e.fmt(f),
            Self::Shaping(e) => e.fmt(f),
        }
    }
}
impl Error for LayoutError {}
impl From<ManagerError> for LayoutError {
    fn from(value: ManagerError) -> Self {
        match value {
            ManagerError::MissingGlyph(ch) => Self::MissingGlyph(ch),
            other => Self::Manager(other),
        }
    }
}
impl From<ShapeError> for LayoutError {
    fn from(value: ShapeError) -> Self {
        Self::Shaping(value)
    }
}

/// One positioned glyph. `x` and `baseline` are top-origin paragraph coordinates;
/// source offsets are half-open UTF-8 byte ranges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutGlyph {
    pub glyph_id: u16,
    pub source: Range<usize>,
    pub x: i64,
    pub y: i64,
    pub x_advance: i64,
    pub y_advance: i64,
    pub x_offset: i64,
    pub y_offset: i64,
    pub font_id: FontId,
    pub family: String,
    pub size: i64,
}

/// Consecutive glyphs from a shaped font/style span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutFragment {
    pub source: Range<usize>,
    pub font_id: FontId,
    pub family: String,
    pub size: i64,
    pub glyphs: Vec<LayoutGlyph>,
}

/// One laid out line. `source` includes any consumed mandatory LF/CRLF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutLine {
    pub source: Range<usize>,
    pub x: i64,
    pub baseline: i64,
    pub width: i64,
    pub height: i64,
    pub mandatory_break: bool,
    pub final_line: bool,
    pub justified: bool,
    pub fragments: Vec<LayoutFragment>,
}

/// Complete paragraph geometry. `height` includes before/after paragraph spacing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParagraphLayout {
    pub width: i64,
    pub height: i64,
    pub lines: Vec<LayoutLine>,
    /// Controls that this engine deliberately leaves to later stories.
    pub unsupported: Vec<&'static str>,
}

#[derive(Clone, Debug)]
struct ShapedGlyph {
    glyph_id: u16,
    source: Range<usize>,
    advance: i64,
    y_advance: i64,
    x_offset: i64,
    y_offset: i64,
    font_id: FontId,
    family: String,
    size: i64,
    ascent: i64,
    descent: i64,
    gap: i64,
    control: bool,
}

#[derive(Clone, Debug)]
struct Span {
    source: Range<usize>,
    font_id: FontId,
    family: String,
    size: i64,
    /// Baseline raise in 1/64 pt (superscript > 0, subscript < 0).
    shift: i64,
    representative: char,
    request: FontRequest,
}

struct CachedShaper<'face, 'font> {
    id: FontId,
    face: &'face FontFace<'font>,
    shaper: LatinShaper<'face, 'font>,
}

#[derive(Clone, Copy, Debug, Default)]
struct FaceExtents {
    ascent: i64,
    descent: i64,
    gap: i64,
}

struct ShapeContext<'ctx, 'face, 'font> {
    paragraph: &'ctx Paragraph,
    spans: &'ctx [Span],
    shapers: &'ctx [CachedShaper<'face, 'font>],
    boundaries: &'ctx [usize],
    boxes: &'ctx [InlineBox],
    fallback: FaceExtents,
    options: LayoutOptions,
    control_font_id: FontId,
    control_family: String,
    control_size: i64,
}

struct FragmentPlacement<'a> {
    visible: Range<usize>,
    boundaries: &'a [usize],
    line_x: i64,
    baseline: i64,
    spaces: &'a [usize],
    each_extra: i64,
    extra_remainder: i64,
}

/// Lay out a paragraph using resolved style defaults and a caller-owned font registry.
/// Character runs are expected to carry their effective US-201/US-301 properties;
/// unset properties inherit from `defaults.character`.
pub fn layout_paragraph(
    paragraph: &Paragraph,
    defaults: &ResolvedProperties,
    fonts: &FontManager<'_>,
    options: LayoutOptions,
) -> Result<ParagraphLayout, LayoutError> {
    layout_paragraph_boxes(paragraph, defaults, fonts, options, &[])
}

/// [`layout_paragraph`] for a text that holds [`INLINE_BOX_CHAR`] at the
/// offset of each of `boxes` (sorted by offset, no duplicates, widths and
/// heights positive).
pub fn layout_paragraph_boxes(
    paragraph: &Paragraph,
    defaults: &ResolvedProperties,
    fonts: &FontManager<'_>,
    options: LayoutOptions,
    boxes: &[InlineBox],
) -> Result<ParagraphLayout, LayoutError> {
    validate_boxes(paragraph, boxes)?;
    Ok(layout_paragraph_impl(
        paragraph,
        defaults,
        fonts,
        options,
        &tabs::TabSettings::default(),
        boxes,
    )?
    .layout)
}

fn validate_boxes(paragraph: &Paragraph, boxes: &[InlineBox]) -> Result<(), LayoutError> {
    let mut floor = 0usize;
    for item in boxes {
        let end = item.offset.saturating_add(INLINE_BOX_CHAR.len_utf8());
        if item.offset < floor
            || item.width <= 0
            || item.height <= 0
            || paragraph.text.get(item.offset..end) != Some("\u{FFFC}")
        {
            return Err(LayoutError::InvalidParagraph("inline box is invalid"));
        }
        floor = end;
    }
    Ok(())
}

fn layout_paragraph_impl(
    paragraph: &Paragraph,
    defaults: &ResolvedProperties,
    fonts: &FontManager<'_>,
    options: LayoutOptions,
    tab_settings: &tabs::TabSettings,
    boxes: &[InlineBox],
) -> Result<tabs::TabLayout, LayoutError> {
    validate_input(paragraph, options)?;
    validate_tab_settings(tab_settings, options.max_work)?;
    paragraph
        .validate_local()
        .map_err(|_| LayoutError::InvalidParagraph("model invariants are not satisfied"))?;

    let mut para = defaults.paragraph.clone();
    para.overlay(&paragraph.props);
    let boundaries = grapheme_boundaries(&paragraph.text).boundaries().to_vec();
    let mut work = 0usize;
    work = add_work(work, boundaries.len(), options.max_work)?;
    let spans = build_spans(
        paragraph,
        &defaults.character,
        fonts,
        &boundaries,
        options,
        boxes,
        &mut work,
    )?;
    let fallback_extents = default_extents(&defaults.character, fonts)?;
    let control_attrs = &defaults.character;
    let control_size = i64::from(control_attrs.size_half_points.unwrap_or(22)) * 32;
    let control_request = request_for_attrs(control_attrs);
    let control_face = fonts.resolve(&control_request)?;
    let shapers = build_shapers(fonts, &spans)?;
    let shape_context = ShapeContext {
        paragraph,
        spans: &spans,
        shapers: &shapers,
        boundaries: &boundaries,
        boxes,
        fallback: fallback_extents,
        options,
        control_font_id: control_face.id,
        control_family: control_face.family.to_owned(),
        control_size,
    };
    let (whole_shape, _) = shape_range(&shape_context, 0..paragraph.text.len(), &mut work)?;
    let mut forbidden = vec![false; boundaries.len()];
    for glyph in &whole_shape {
        let a = boundary_index(&boundaries, glyph.source.start)?;
        let b = boundary_index(&boundaries, glyph.source.end)?;
        for blocked in forbidden.iter_mut().take(b).skip(a + 1) {
            *blocked = true;
        }
    }

    // Advance of the whole-paragraph shaping up to each glyph end: a cheap
    // estimate of a prefix width, used only to guess where a line ends (the
    // exact shaping of the guess and of its neighbour decides). Not used with
    // tabs, whose advances depend on the position.
    let estimate: Option<Vec<(usize, i64)>> = if whole_shape.iter().any(|glyph| glyph.control) {
        None
    } else {
        let mut total = 0i64;
        let mut ends = Vec::with_capacity(whole_shape.len());
        for glyph in &whole_shape {
            total = total
                .checked_add(glyph.advance)
                .ok_or(LayoutError::ArithmeticOverflow)?;
            ends.push((glyph.source.end, total));
        }
        Some(ends)
    };
    // The bisection relies on prefix widths that never shrink as the prefix
    // grows: no negative advance (a kerning pair that outweighs the glyph it
    // follows) and only left tab stops, whose position does not depend on the
    // text that follows. Otherwise lines are chosen by the linear scan. The
    // guided search shapes fewer prefixes, so the `work` counter of a layout
    // is lower than before and a `max_work` that was just enough still is.
    #[cfg(test)]
    let force_linear = tests::FORCE_LINEAR.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(test))]
    let force_linear = false;
    let monotone = !force_linear
        && whole_shape.iter().all(|glyph| glyph.advance >= 0)
        && tab_settings.stops.iter().all(|stop| {
            matches!(
                stop.alignment,
                tabs::TabAlignment::Left | tabs::TabAlignment::Bar
            )
        });
    let estimate_at = |offset: usize| -> i64 {
        estimate.as_ref().map_or(0, |ends| {
            let index = ends.partition_point(|(end, _)| *end <= offset);
            index.checked_sub(1).map_or(0, |i| ends[i].1)
        })
    };

    let mut opportunities = vec![None; paragraph.text.len() + 1];
    for item in line_breaks(&paragraph.text) {
        opportunities[item.offset] = Some(item.kind);
        work = add_work(work, 1, options.max_work)?;
    }
    let alignment = para.alignment.unwrap_or(Alignment::Left);
    let indent_left = twips_to_units(i64::from(para.indent_left.unwrap_or(0)))?;
    let indent_right = twips_to_units(i64::from(para.indent_right.unwrap_or(0)))?;
    let first_indent = twips_to_units(i64::from(para.indent_first_line.unwrap_or(0)))?;

    let mut lines = Vec::new();
    let mut decorations = Vec::new();
    let mut start_slot = 0usize;
    let mut vertical = twips_to_units(i64::from(para.space_before.unwrap_or(0)))?;
    let mut paragraph_width = 0i64;
    let mut total_glyphs = 0usize;
    loop {
        if lines.len() >= options.max_lines {
            return Err(LayoutError::LimitExceeded("lines"));
        }
        let start = boundaries[start_slot];
        let is_first = lines.is_empty();
        let left = if is_first {
            indent_left
                .checked_add(first_indent)
                .ok_or(LayoutError::ArithmeticOverflow)?
        } else {
            indent_left
        };
        let available = options
            .width
            .checked_sub(left)
            .and_then(|v| v.checked_sub(indent_right))
            .ok_or(LayoutError::ArithmeticOverflow)?;
        if available <= 0 {
            return Err(LayoutError::InvalidOptions(
                "indents leave no usable line width",
            ));
        }

        let mut mandatory_slot = boundaries.len() - 1;
        for slot in start_slot + 1..boundaries.len() {
            if opportunities[boundaries[slot]] == Some(BreakKind::Mandatory) {
                mandatory_slot = slot;
                break;
            }
        }
        // A candidate fits when its prefix, shaped in its line context, is not
        // wider than the line. Prefix widths do not decrease with the prefix
        // (advances are non-negative after pair adjustment in the supported
        // Latin layout), so the candidates that fit form a prefix of the list.
        // The length of that prefix is guessed from the estimate, checked on
        // the guess and its neighbour, and bisected when the guess was wrong:
        // two shapings per line in the usual case instead of one per candidate.
        let fits = |slot: usize, work: &mut usize| -> Result<bool, LayoutError> {
            let candidate_end = trim_trailing_space(&paragraph.text, &boundaries, start_slot, slot);
            let (mut candidate_glyphs, _) =
                shape_range(&shape_context, start..candidate_end, work)?;
            resolve_tab_advances(
                &mut candidate_glyphs,
                left,
                tab_settings,
                &paragraph.text,
                work,
                options,
                None,
            )?;
            *work = add_work(*work, 1, options.max_work)?;
            Ok(glyph_advance(&candidate_glyphs)? <= available)
        };
        let guess = |slots: &[usize]| -> Option<usize> {
            estimate.as_ref()?;
            let base = estimate_at(start);
            Some(slots.partition_point(|&slot| {
                let end = trim_trailing_space(&paragraph.text, &boundaries, start_slot, slot);
                estimate_at(end) - base <= available
            }))
        };
        // Number of leading candidates that fit.
        let leading_fit = |slots: &[usize], work: &mut usize| -> Result<usize, LayoutError> {
            if !monotone {
                // Prefix widths may shrink: the original scan, one shaping per
                // candidate, stopping at the first one that overflows.
                let mut count = 0;
                for &slot in slots {
                    if !fits(slot, work)? {
                        break;
                    }
                    count += 1;
                }
                return Ok(count);
            }
            let (mut low, mut high) = (0usize, slots.len());
            if let Some(hint) = guess(slots) {
                if hint == 0 || fits(slots[hint - 1], work)? {
                    if hint == slots.len() || !fits(slots[hint], work)? {
                        return Ok(hint);
                    }
                    low = hint + 1;
                } else {
                    high = hint - 1;
                }
            }
            while low < high {
                let middle = low + (high - low) / 2;
                if fits(slots[middle], work)? {
                    low = middle + 1;
                } else {
                    high = middle;
                }
            }
            Ok(low)
        };
        let mut chosen_legal: Option<(usize, BreakKind)> = None;
        let legal_slots = (start_slot + 1..=mandatory_slot)
            .filter(|&slot| {
                !forbidden[slot]
                    && (opportunities[boundaries[slot]].is_some() || slot == boundaries.len() - 1)
            })
            .collect::<Vec<_>>();
        let low = leading_fit(&legal_slots, &mut work)?;
        if low > 0 {
            let slot = legal_slots[low - 1];
            chosen_legal = Some((
                slot,
                opportunities[boundaries[slot]].unwrap_or(BreakKind::Mandatory),
            ));
        }
        let mut chosen_grapheme = None;
        let mut first_grapheme = None;
        if chosen_legal.is_none() {
            let grapheme_slots = (start_slot + 1..=mandatory_slot)
                .filter(|&slot| !forbidden[slot])
                .collect::<Vec<_>>();
            first_grapheme = grapheme_slots.first().copied();
            let low = leading_fit(&grapheme_slots, &mut work)?;
            if low > 0 {
                chosen_grapheme = Some(grapheme_slots[low - 1]);
            }
        }
        let (end_slot, break_kind) = if start_slot == boundaries.len() - 1 {
            (start_slot, BreakKind::Allowed)
        } else if let Some(value) = chosen_legal {
            value
        } else if let Some(slot) = chosen_grapheme {
            (
                slot,
                opportunities[boundaries[slot]].unwrap_or(BreakKind::Allowed),
            )
        } else if let Some(slot) = first_grapheme {
            (
                slot,
                opportunities[boundaries[slot]].unwrap_or(BreakKind::Allowed),
            )
        } else if paragraph.text.is_empty() {
            (start_slot, BreakKind::Mandatory)
        } else if mandatory_slot > start_slot {
            (mandatory_slot, BreakKind::Mandatory)
        } else {
            return Err(LayoutError::InvalidParagraph(
                "line breaker made no progress",
            ));
        };
        let source_end = boundaries[end_slot];
        let terminal_control = end_slot == boundaries.len() - 1
            && break_kind == BreakKind::Mandatory
            && paragraph.text[..source_end]
                .chars()
                .next_back()
                .is_some_and(|ch| {
                    matches!(
                        ch,
                        '\r' | '\n'
                            | '\u{000b}'
                            | '\u{000c}'
                            | '\u{0085}'
                            | '\u{2028}'
                            | '\u{2029}'
                    )
                });
        let visible_end = trim_trailing_space(&paragraph.text, &boundaries, start_slot, end_slot);
        let visible_end_slot = boundary_index(&boundaries, visible_end)?;
        let (mut shaped_line, line_extents) =
            shape_range(&shape_context, start..visible_end, &mut work)?;
        resolve_tab_advances(
            &mut shaped_line,
            left,
            tab_settings,
            &paragraph.text,
            &mut work,
            options,
            Some((lines.len(), &mut decorations)),
        )?;
        total_glyphs = total_glyphs
            .checked_add(shaped_line.len())
            .ok_or(LayoutError::ArithmeticOverflow)?;
        if total_glyphs > options.max_glyphs {
            return Err(LayoutError::LimitExceeded("glyphs"));
        }
        let natural_width = glyph_advance(&shaped_line)?;
        let final_line = end_slot == boundaries.len() - 1 && !terminal_control;
        let mandatory_break = break_kind == BreakKind::Mandatory && !final_line;
        let has_tabs = shaped_line.iter().any(|glyph| glyph.control);
        let justify =
            alignment == Alignment::Justify && !has_tabs && !final_line && !mandatory_break;
        let space_slots = if justify {
            (start_slot..visible_end_slot)
                .filter(|&slot| &paragraph.text[boundaries[slot]..boundaries[slot + 1]] == " ")
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let extra = if space_slots.is_empty() {
            0
        } else {
            available.saturating_sub(natural_width)
        };
        let each_extra = if space_slots.is_empty() {
            0
        } else {
            extra / space_slots.len() as i64
        };
        let extra_remainder = if space_slots.is_empty() {
            0
        } else {
            extra % space_slots.len() as i64
        };
        let line_width = natural_width
            .checked_add(extra)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        work = add_work(work, tab_settings.stops.len(), options.max_work)?;
        for stop in tab_settings.stops.iter().filter(|stop| {
            stop.alignment == tabs::TabAlignment::Bar
                && stop.position >= left
                && stop.position <= options.width.saturating_sub(indent_right)
        }) {
            if decorations.len() >= options.max_glyphs {
                return Err(LayoutError::LimitExceeded("tab decorations"));
            }
            decorations.push(tabs::TabDecoration {
                line: lines.len(),
                x: stop.position,
                width: 0,
                kind: tabs::TabDecorationKind::Bar,
            });
        }
        let remaining = available.saturating_sub(natural_width).max(0);
        let x = if has_tabs {
            left
        } else {
            match alignment {
                Alignment::Left | Alignment::Justify => left,
                Alignment::Center => left
                    .checked_add(remaining / 2)
                    .ok_or(LayoutError::ArithmeticOverflow)?,
                Alignment::Right => left
                    .checked_add(remaining)
                    .ok_or(LayoutError::ArithmeticOverflow)?,
            }
        };

        let (ascent, descent, gap) =
            extents_for_range(&shaped_line, line_extents, fallback_extents);
        let base_height = ascent
            .checked_add(descent)
            .and_then(|v| v.checked_add(gap))
            .ok_or(LayoutError::ArithmeticOverflow)?
            .max(1);
        let height = line_height(base_height, para.line_spacing)?;
        let baseline = vertical
            .checked_add(ascent)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let fragments = make_fragments(
            &mut shaped_line,
            FragmentPlacement {
                visible: start..visible_end,
                boundaries: &boundaries,
                line_x: x,
                baseline,
                spaces: &space_slots,
                each_extra,
                extra_remainder,
            },
        )?;
        paragraph_width = paragraph_width.max(x.saturating_add(line_width));
        lines.push(LayoutLine {
            source: start..source_end,
            x,
            baseline,
            width: line_width,
            height,
            mandatory_break,
            final_line,
            justified: justify && !space_slots.is_empty(),
            fragments,
        });
        vertical = vertical
            .checked_add(height)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        if final_line {
            break;
        }
        start_slot = end_slot;
    }
    vertical = vertical
        .checked_add(twips_to_units(i64::from(para.space_after.unwrap_or(0)))?)
        .ok_or(LayoutError::ArithmeticOverflow)?;

    let mut unsupported = Vec::new();
    if paragraph
        .anchored
        .iter()
        .any(|x| x.offset <= paragraph.text.len())
    {
        unsupported.push("anchored objects");
    }
    if !paragraph.fields.is_empty() {
        unsupported.push("field replacement/rendering");
    }
    if !paragraph.links.is_empty() {
        unsupported.push("hyperlink decoration");
    }
    let layout = ParagraphLayout {
        width: paragraph_width,
        height: vertical,
        lines,
        unsupported,
    };
    Ok(tabs::TabLayout {
        layout,
        decorations,
    })
}

fn validate_input(paragraph: &Paragraph, options: LayoutOptions) -> Result<(), LayoutError> {
    if options.width <= 0 {
        return Err(LayoutError::InvalidOptions("width must be positive"));
    }
    if paragraph.text.len() > options.max_text_bytes {
        return Err(LayoutError::LimitExceeded("text bytes"));
    }
    if options.max_lines == 0 || options.max_work == 0 {
        return Err(LayoutError::InvalidOptions(
            "line and work budgets must be positive",
        ));
    }
    Ok(())
}

fn build_spans(
    paragraph: &Paragraph,
    default_character: &CharAttrs,
    fonts: &FontManager<'_>,
    boundaries: &[usize],
    options: LayoutOptions,
    boxes: &[InlineBox],
    work: &mut usize,
) -> Result<Vec<Span>, LayoutError> {
    let mut spans: Vec<Span> = Vec::new();
    for pair in boundaries.windows(2) {
        let source = pair[0]..pair[1];
        let text = &paragraph.text[source.clone()];
        if box_at(boxes, source.start).is_some() {
            continue;
        }
        if text.chars().all(|c| {
            matches!(
                c,
                '\t' | '\r' | '\n' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
            )
        }) {
            continue;
        }
        let mut attrs = default_character.clone();
        attrs.overlay(&paragraph.attrs_at(source.start));
        let size_half = attrs.size_half_points.unwrap_or(22);
        let full_size = i64::from(size_half)
            .checked_mul(32)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        // Superscript and subscript (T240-C): two thirds of the size, the
        // baseline raised by a third of the full size or lowered by 14 %.
        let (size, shift) = match attrs.vertical_align {
            Some(VerticalAlign::Superscript) => (full_size * 2 / 3, full_size * 33 / 100),
            Some(VerticalAlign::Subscript) => (full_size * 2 / 3, -(full_size * 14 / 100)),
            _ => (full_size, 0),
        };
        let request = request_for_attrs(&attrs);
        let ch = text
            .chars()
            .next()
            .ok_or(LayoutError::InvalidParagraph("empty grapheme"))?;
        let resolved = fonts.resolve_for_char(&request, ch)?;
        let id = resolved.id;
        let family = resolved.family.to_owned();
        if let Some(last) = spans.last_mut() {
            if last.source.end == source.start
                && last.font_id == id
                && last.size == size
                && last.shift == shift
            {
                last.source.end = source.end;
                *work = add_work(*work, 1, options.max_work)?;
                continue;
            }
        }
        spans.push(Span {
            source,
            font_id: id,
            family,
            size,
            shift,
            representative: ch,
            request,
        });
        *work = add_work(*work, 1, options.max_work)?;
    }
    Ok(spans)
}

fn request_for_attrs(attrs: &CharAttrs) -> FontRequest {
    let families = attrs
        .font
        .as_deref()
        .map(str::to_owned)
        .into_iter()
        .collect::<Vec<_>>();
    FontRequest::new(
        if families.is_empty() {
            vec!["Calibri".to_owned()]
        } else {
            families
        },
        if attrs.bold == Some(true) { 700 } else { 400 },
        if attrs.italic == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    )
    .with_generic(GenericFamily::SansSerif)
}

fn default_extents(attrs: &CharAttrs, fonts: &FontManager<'_>) -> Result<FaceExtents, LayoutError> {
    let request = request_for_attrs(attrs);
    let face = fonts.resolve(&request)?.face;
    let metrics = face.metrics();
    let size = i64::from(attrs.size_half_points.unwrap_or(22)) * 32;
    Ok(FaceExtents {
        ascent: scale_font(i64::from(metrics.ascender), size, metrics.units_per_em)?,
        descent: scale_font(
            i64::from(metrics.descender).saturating_neg(),
            size,
            metrics.units_per_em,
        )?,
        gap: scale_font(
            i64::from(metrics.line_gap).max(0),
            size,
            metrics.units_per_em,
        )?,
    })
}

fn build_shapers<'manager, 'font>(
    fonts: &'manager FontManager<'font>,
    spans: &[Span],
) -> Result<Vec<CachedShaper<'manager, 'font>>, LayoutError> {
    let mut cached = Vec::new();
    for span in spans {
        if cached
            .iter()
            .any(|entry: &CachedShaper<'manager, 'font>| entry.id == span.font_id)
        {
            continue;
        }
        let resolved = fonts.resolve_for_char(&span.request, span.representative)?;
        let face = resolved.face;
        cached.push(CachedShaper {
            id: span.font_id,
            face,
            shaper: LatinShaper::new(face)?,
        });
    }
    Ok(cached)
}

fn shape_range(
    context: &ShapeContext<'_, '_, '_>,
    range: Range<usize>,
    work: &mut usize,
) -> Result<(Vec<ShapedGlyph>, FaceExtents), LayoutError> {
    let mut output = Vec::new();
    let mut extents = context.fallback;
    for span in context.spans {
        let piece = span.source.start.max(range.start)..span.source.end.min(range.end);
        if piece.is_empty() {
            continue;
        }
        let cached = context
            .shapers
            .iter()
            .find(|entry| entry.id == span.font_id)
            .ok_or(LayoutError::InvalidParagraph(
                "font shaper cache entry is missing",
            ))?;
        let face = cached.face;
        let metrics = face.metrics();
        let face_extents = FaceExtents {
            ascent: scale_font(i64::from(metrics.ascender), span.size, metrics.units_per_em)?,
            descent: scale_font(
                i64::from(metrics.descender).saturating_neg(),
                span.size,
                metrics.units_per_em,
            )?,
            gap: scale_font(
                i64::from(metrics.line_gap).max(0),
                span.size,
                metrics.units_per_em,
            )?,
        };
        extents.ascent = extents.ascent.max(face_extents.ascent + span.shift.max(0));
        extents.descent = extents
            .descent
            .max(face_extents.descent + (-span.shift).max(0));
        extents.gap = extents.gap.max(face_extents.gap);
        let text = &context.paragraph.text[piece.clone()];
        *work = add_work(*work, text.chars().count(), context.options.max_work)?;
        let run = cached.shaper.shape_text(
            text,
            ShapeOptions {
                size: span.size,
                ..ShapeOptions::default()
            },
        )?;
        let scalar_offsets = scalar_offsets(text);
        for glyph in run.glyphs {
            let local_start =
                *scalar_offsets
                    .get(glyph.cluster.start)
                    .ok_or(LayoutError::InvalidParagraph(
                        "invalid shaped cluster start",
                    ))?;
            let local_end = *scalar_offsets
                .get(glyph.cluster.end)
                .ok_or(LayoutError::InvalidParagraph("invalid shaped cluster end"))?;
            let raw_start = piece.start + local_start;
            let raw_end = piece.start + local_end;
            let source_start = floor_grapheme(context.boundaries, raw_start)?;
            let source_end = ceil_grapheme(context.boundaries, raw_end)?;
            output.push(ShapedGlyph {
                glyph_id: glyph.glyph_id,
                source: source_start..source_end,
                advance: glyph.x_advance,
                y_advance: glyph.y_advance,
                x_offset: glyph.x_offset,
                y_offset: glyph.y_offset + span.shift,
                font_id: span.font_id,
                family: span.family.clone(),
                size: span.size,
                ascent: face_extents.ascent + span.shift.max(0),
                descent: face_extents.descent + (-span.shift).max(0),
                gap: face_extents.gap,
                control: false,
            });
            *work = add_work(*work, 1, context.options.max_work)?;
            if output.len() > context.options.max_glyphs {
                return Err(LayoutError::LimitExceeded("glyphs"));
            }
        }
    }
    let first_boundary = context
        .boundaries
        .partition_point(|&boundary| boundary < range.start);
    let end_boundary = context
        .boundaries
        .partition_point(|&boundary| boundary < range.end);
    let scanned = end_boundary.saturating_sub(first_boundary);
    *work = add_work(*work, scanned, context.options.max_work)?;
    for pair in context.boundaries[first_boundary..=end_boundary].windows(2) {
        let source = pair[0]..pair[1];
        if let Some(item) = box_at(context.boxes, source.start) {
            output.push(ShapedGlyph {
                glyph_id: INLINE_BOX_GLYPH,
                source,
                advance: item.width,
                y_advance: 0,
                x_offset: 0,
                y_offset: 0,
                font_id: context.control_font_id,
                family: context.control_family.clone(),
                size: context.control_size,
                ascent: item.height,
                descent: context.fallback.descent,
                gap: context.fallback.gap,
                control: false,
            });
            *work = add_work(*work, 1, context.options.max_work)?;
            if output.len() > context.options.max_glyphs {
                return Err(LayoutError::LimitExceeded("glyphs"));
            }
        } else if &context.paragraph.text[source.clone()] == "\t" {
            output.push(ShapedGlyph {
                glyph_id: 0,
                source,
                advance: 0,
                y_advance: 0,
                x_offset: 0,
                y_offset: 0,
                font_id: context.control_font_id,
                family: context.control_family.clone(),
                size: context.control_size,
                ascent: 0,
                descent: 0,
                gap: 0,
                control: true,
            });
            *work = add_work(*work, 1, context.options.max_work)?;
            if output.len() > context.options.max_glyphs {
                return Err(LayoutError::LimitExceeded("glyphs"));
            }
        }
    }
    output.sort_by_key(|glyph| glyph.source.start);
    Ok((output, extents))
}

fn box_at(boxes: &[InlineBox], offset: usize) -> Option<&InlineBox> {
    boxes
        .binary_search_by_key(&offset, |b| b.offset)
        .ok()
        .map(|i| &boxes[i])
}

fn scalar_offsets(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect()
}

fn extents_for_range(
    glyphs: &[ShapedGlyph],
    all: FaceExtents,
    fallback: FaceExtents,
) -> (i64, i64, i64) {
    if glyphs.is_empty() || glyphs.iter().all(|glyph| glyph.control) {
        return (fallback.ascent, fallback.descent, fallback.gap);
    }
    let mut result = FaceExtents::default();
    for glyph in glyphs.iter().filter(|glyph| !glyph.control) {
        result.ascent = result.ascent.max(glyph.ascent);
        result.descent = result.descent.max(glyph.descent);
        result.gap = result.gap.max(glyph.gap);
    }
    let _ = all;
    (result.ascent, result.descent, result.gap)
}

fn glyph_advance(glyphs: &[ShapedGlyph]) -> Result<i64, LayoutError> {
    glyphs.iter().try_fold(0i64, |total, glyph| {
        total
            .checked_add(glyph.advance)
            .ok_or(LayoutError::ArithmeticOverflow)
    })
}

fn validate_tab_settings(settings: &tabs::TabSettings, max_work: usize) -> Result<(), LayoutError> {
    if settings.default_interval <= 0 {
        return Err(LayoutError::InvalidOptions("tab interval must be positive"));
    }
    if settings.stops.len() > max_work {
        return Err(LayoutError::LimitExceeded("tab stops"));
    }
    let mut previous = None;
    for stop in &settings.stops {
        if stop.position <= 0
            || stop.position == i64::MAX
            || previous.is_some_and(|value| value >= stop.position)
        {
            return Err(LayoutError::InvalidOptions(
                "tab stops must be positive, sorted and unique",
            ));
        }
        previous = Some(stop.position);
    }
    Ok(())
}

fn resolve_tab_advances(
    glyphs: &mut [ShapedGlyph],
    line_x: i64,
    settings: &tabs::TabSettings,
    text: &str,
    work: &mut usize,
    options: LayoutOptions,
    mut output: Option<(usize, &mut Vec<tabs::TabDecoration>)>,
) -> Result<(), LayoutError> {
    let mut pen = line_x;
    let mut i = 0;
    while i < glyphs.len() {
        if !glyphs[i].control {
            pen = pen
                .checked_add(glyphs[i].advance)
                .ok_or(LayoutError::ArithmeticOverflow)?;
            i += 1;
            continue;
        }
        let next = glyphs[i + 1..]
            .iter()
            .position(|glyph| glyph.control)
            .map_or(glyphs.len(), |n| i + 1 + n);
        let segment_width = glyph_advance(&glyphs[i + 1..next])?;
        *work = add_work(*work, settings.stops.len(), options.max_work)?;
        let mut pre_comma = 0i64;
        let mut comma_seen = false;
        let segment_start = glyphs[i].source.end;
        let segment_end = glyphs.get(next).map_or_else(
            || glyphs.last().map_or(segment_start, |g| g.source.end),
            |g| g.source.start,
        );
        let comma_byte = text
            .get(segment_start..segment_end)
            .and_then(|s| s.find(','))
            .map(|n| segment_start + n);
        if let Some(comma_byte) = comma_byte {
            comma_seen = true;
            for glyph in &glyphs[i + 1..next] {
                if glyph.source.start < comma_byte {
                    pre_comma = pre_comma
                        .checked_add(glyph.advance)
                        .ok_or(LayoutError::ArithmeticOverflow)?;
                }
            }
        }
        let mut selected = None;
        for stop in settings
            .stops
            .iter()
            .copied()
            .filter(|stop| stop.position > pen && stop.alignment != tabs::TabAlignment::Bar)
        {
            let aligned = match stop.alignment {
                tabs::TabAlignment::Left => 0,
                tabs::TabAlignment::Center => segment_width / 2,
                tabs::TabAlignment::Right => segment_width,
                tabs::TabAlignment::Decimal => {
                    if comma_seen {
                        pre_comma
                    } else {
                        segment_width
                    }
                }
                tabs::TabAlignment::Bar => 0,
            };
            let advance = stop
                .position
                .checked_sub(pen)
                .and_then(|v| v.checked_sub(aligned))
                .ok_or(LayoutError::ArithmeticOverflow)?;
            if advance > 0 {
                selected = Some((stop, advance));
                break;
            }
        }
        if selected.is_none() {
            // Automatic stops begin only after all custom stops are passed.
            // Keep the project's 2268-unit default interval independent of
            // Word's separate OOXML default when that property is omitted.
            let last_custom = settings
                .stops
                .iter()
                .rev()
                .find(|stop| stop.alignment != tabs::TabAlignment::Bar)
                .map_or(pen, |stop| stop.position.max(pen));
            let next_grid = last_custom
                .div_euclid(settings.default_interval)
                .checked_add(1)
                .and_then(|n| n.checked_mul(settings.default_interval))
                .ok_or(LayoutError::ArithmeticOverflow)?;
            selected = Some((
                tabs::TabStop {
                    position: next_grid,
                    alignment: tabs::TabAlignment::Left,
                    leader: None,
                },
                next_grid
                    .checked_sub(pen)
                    .ok_or(LayoutError::ArithmeticOverflow)?,
            ));
        }
        let (stop, advance) = selected.ok_or(LayoutError::ArithmeticOverflow)?;
        glyphs[i].advance = advance;
        if let Some((line, decorations)) = output.as_mut() {
            if let Some(leader) = stop.leader {
                if decorations.len() >= options.max_glyphs {
                    return Err(LayoutError::LimitExceeded("tab decorations"));
                }
                decorations.push(tabs::TabDecoration {
                    line: *line,
                    x: pen,
                    width: advance,
                    kind: tabs::TabDecorationKind::Leader(leader),
                });
            }
        }
        pen = pen
            .checked_add(advance)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        i += 1;
    }
    Ok(())
}

fn make_fragments(
    glyphs: &mut [ShapedGlyph],
    placement: FragmentPlacement<'_>,
) -> Result<Vec<LayoutFragment>, LayoutError> {
    let mut extras: Vec<(Range<usize>, i64)> = Vec::new();
    for (ordinal, &slot) in placement.spaces.iter().enumerate() {
        let extra = placement.each_extra + i64::from((ordinal as i64) < placement.extra_remainder);
        extras.push((
            placement.boundaries[slot]..placement.boundaries[slot + 1],
            extra,
        ));
    }
    let mut pen = placement.line_x;
    let mut fragments: Vec<LayoutFragment> = Vec::new();
    for glyph in glyphs.iter().filter(|g| {
        g.source.start >= placement.visible.start && g.source.end <= placement.visible.end
    }) {
        let extra = extras
            .iter()
            .find(|(range, _)| range.start <= glyph.source.start && glyph.source.end <= range.end)
            .map(|(_, extra)| *extra)
            .unwrap_or(0);
        let advance = glyph
            .advance
            .checked_add(extra)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let placed = LayoutGlyph {
            glyph_id: glyph.glyph_id,
            source: glyph.source.clone(),
            x: pen
                .checked_add(glyph.x_offset)
                .ok_or(LayoutError::ArithmeticOverflow)?,
            y: placement
                .baseline
                .checked_sub(glyph.y_offset)
                .ok_or(LayoutError::ArithmeticOverflow)?,
            x_advance: advance,
            y_advance: glyph.y_advance,
            x_offset: glyph.x_offset,
            y_offset: glyph.y_offset,
            font_id: glyph.font_id,
            family: glyph.family.clone(),
            size: glyph.size,
        };
        pen = pen
            .checked_add(advance)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        if let Some(last) = fragments.last_mut() {
            if last.font_id == placed.font_id
                && last.size == placed.size
                && last.source.end == placed.source.start
            {
                last.source.end = placed.source.end;
                last.glyphs.push(placed);
                continue;
            }
        }
        fragments.push(LayoutFragment {
            source: placed.source.clone(),
            font_id: placed.font_id,
            family: placed.family.clone(),
            size: placed.size,
            glyphs: vec![placed],
        });
    }
    Ok(fragments)
}

fn trim_trailing_space(text: &str, boundaries: &[usize], start: usize, end: usize) -> usize {
    let mut slot = end;
    while slot > start {
        let grapheme = &text[boundaries[slot - 1]..boundaries[slot]];
        if grapheme.chars().all(|c| {
            matches!(
                c,
                '\r' | '\n' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
            )
        }) {
            slot -= 1;
        } else {
            break;
        }
    }
    while slot > start {
        let grapheme = &text[boundaries[slot - 1]..boundaries[slot]];
        if grapheme != "\t"
            && grapheme
                .chars()
                .all(|c| c.is_whitespace() && !matches!(c, '\u{00a0}' | '\u{202f}' | '\u{2060}'))
        {
            slot -= 1;
        } else {
            break;
        }
    }
    boundaries[slot]
}

fn line_height(base: i64, spacing: Option<LineSpacing>) -> Result<i64, LayoutError> {
    match spacing.unwrap_or(LineSpacing {
        rule: LineRule::Auto,
        value: 240,
    }) {
        LineSpacing {
            rule: LineRule::Auto,
            value,
        } => {
            if value == 0 {
                return Err(LayoutError::InvalidOptions(
                    "automatic line spacing must be positive",
                ));
            }
            round_ratio(i128::from(base) * i128::from(value), 240)
        }
        LineSpacing {
            rule: LineRule::Exact,
            value,
        } => {
            if value == 0 {
                return Err(LayoutError::InvalidOptions(
                    "exact line spacing must be positive",
                ));
            }
            twips_to_units(i64::from(value))
        }
        LineSpacing {
            rule: LineRule::AtLeast,
            value,
        } => {
            if value == 0 {
                return Err(LayoutError::InvalidOptions(
                    "minimum line spacing must be positive",
                ));
            }
            Ok(base.max(twips_to_units(i64::from(value))?))
        }
    }
}

fn scale_font(value: i64, size: i64, units_per_em: u16) -> Result<i64, LayoutError> {
    round_ratio(
        i128::from(value) * i128::from(size),
        i128::from(units_per_em),
    )
}

fn twips_to_units(value: i64) -> Result<i64, LayoutError> {
    round_ratio(i128::from(value) * 16, 5)
}

fn round_ratio(numerator: i128, denominator: i128) -> Result<i64, LayoutError> {
    if denominator <= 0 {
        return Err(LayoutError::InvalidOptions("non-positive unit denominator"));
    }
    let q = numerator / denominator;
    let r = numerator % denominator;
    let rounded = if r
        .abs()
        .checked_mul(2)
        .ok_or(LayoutError::ArithmeticOverflow)?
        >= denominator
    {
        q.checked_add(if numerator < 0 { -1 } else { 1 })
            .ok_or(LayoutError::ArithmeticOverflow)?
    } else {
        q
    };
    i64::try_from(rounded).map_err(|_| LayoutError::ArithmeticOverflow)
}

fn add_work(current: usize, amount: usize, maximum: usize) -> Result<usize, LayoutError> {
    let next = current
        .checked_add(amount)
        .ok_or(LayoutError::ArithmeticOverflow)?;
    if next > maximum {
        Err(LayoutError::LimitExceeded("layout work"))
    } else {
        Ok(next)
    }
}

fn boundary_index(boundaries: &[usize], byte: usize) -> Result<usize, LayoutError> {
    boundaries
        .binary_search(&byte)
        .map_err(|_| LayoutError::InvalidParagraph("offset is not a grapheme boundary"))
}

fn floor_grapheme(boundaries: &[usize], byte: usize) -> Result<usize, LayoutError> {
    let index = boundaries.partition_point(|&value| value <= byte);
    index
        .checked_sub(1)
        .and_then(|i| boundaries.get(i).copied())
        .ok_or(LayoutError::InvalidParagraph(
            "shaped cluster start is outside the paragraph",
        ))
}

fn ceil_grapheme(boundaries: &[usize], byte: usize) -> Result<usize, LayoutError> {
    boundaries
        .get(boundaries.partition_point(|&value| value < byte))
        .copied()
        .ok_or(LayoutError::InvalidParagraph(
            "shaped cluster end is outside the paragraph",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_font::manager::FontSource;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Makes `layout_paragraph` choose line ends by the original linear scan.
    pub(super) static FORCE_LINEAR: AtomicBool = AtomicBool::new(false);

    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    /// The guided search picks the lines the linear scan picks, on kerning
    /// pairs (AV, To, Ty, LT), zero-width marks and joiners, ZWSP, NBSP, tabs,
    /// ligatures, long words and several widths.
    #[test]
    fn guided_line_search_matches_the_linear_scan() {
        let mut fonts = FontManager::new();
        for data in [
            &include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf")[..],
            &include_bytes!("../../../data/fonts/carlito/Carlito-Bold.ttf")[..],
        ] {
            fonts.register(data, 0, FontSource::System).expect("font");
        }
        let words = [
            "AV",
            "To",
            "Ty",
            "LT",
            "WAVE",
            "Yes.",
            "r.",
            "y,",
            "fi",
            "ffl",
            "office",
            "x",
            "alpha",
            "caf\u{e9}",
            "e\u{301}",
            "a\u{200b}b",
            "a\u{a0}b",
            "\u{200d}",
            "a\tb",
            "-",
            "\u{2014}",
            "supercalifragilisticexpialidocious",
            "\u{200b}",
        ];
        let mut rng = Rng(0x9e3779b97f4a7c15);
        for i in 0..60 {
            let n = 1 + rng.below(40);
            let text: Vec<&str> = (0..n).map(|_| words[rng.below(words.len())]).collect();
            let paragraph = Paragraph::with_text(&text.join(if i % 3 == 0 { "  " } else { " " }));
            for width in [8 * 64, 60 * 64, 300 * 64] {
                let run = |linear: bool| {
                    FORCE_LINEAR.store(linear, Ordering::Relaxed);
                    let result = layout_paragraph(
                        &paragraph,
                        &ResolvedProperties::default(),
                        &fonts,
                        LayoutOptions::new(width),
                    );
                    FORCE_LINEAR.store(false, Ordering::Relaxed);
                    format!("{result:?}")
                };
                let (guided, linear) = (run(false), run(true));
                assert!(guided == linear, "width {width}, text {:?}", paragraph.text);
            }
        }
    }
}
