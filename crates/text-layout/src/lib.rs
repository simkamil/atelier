// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic rich paragraph line layout (US-210).
//!
//! Input paragraph offsets and output source ranges are UTF-8 byte offsets.
//! Font sizes, positions, advances and line metrics use 1/64 typographic point;
//! paragraph indents and explicit line spacing remain in twips at the API.

#![forbid(unsafe_code)]

use atelier_font::FontFace;
use atelier_font::manager::{
    FontId, FontManager, FontRequest, FontStyle, GenericFamily, ManagerError,
};
use atelier_shape::{LatinShaper, ShapeError, ShapeOptions};
use atelier_text_model::rich::{
    Alignment, CharAttrs, LineRule, LineSpacing, Paragraph, ResolvedProperties,
};
use atelier_unicode::line_break::{BreakKind, line_breaks};
use atelier_unicode::segmentation::grapheme_boundaries;
use core::ops::Range;
use std::error::Error;
use std::fmt;

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
}

#[derive(Clone, Debug)]
struct Span {
    source: Range<usize>,
    font_id: FontId,
    family: String,
    size: i64,
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
    fallback: FaceExtents,
    options: LayoutOptions,
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
    validate_input(paragraph, options)?;
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
        &mut work,
    )?;
    let fallback_extents = default_extents(&defaults.character, fonts)?;
    let shapers = build_shapers(fonts, &spans)?;
    let shape_context = ShapeContext {
        paragraph,
        spans: &spans,
        shapers: &shapers,
        boundaries: &boundaries,
        fallback: fallback_extents,
        options,
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
        let mut chosen_legal: Option<(usize, BreakKind)> = None;
        let legal_slots = (start_slot + 1..=mandatory_slot)
            .filter(|&slot| {
                !forbidden[slot]
                    && (opportunities[boundaries[slot]].is_some() || slot == boundaries.len() - 1)
            })
            .collect::<Vec<_>>();
        for slot in legal_slots {
            let candidate_end = trim_trailing_space(&paragraph.text, &boundaries, start_slot, slot);
            let (candidate_glyphs, _) =
                shape_range(&shape_context, start..candidate_end, &mut work)?;
            work = add_work(work, 1, options.max_work)?;
            if glyph_advance(&candidate_glyphs)? <= available {
                chosen_legal = Some((
                    slot,
                    opportunities[boundaries[slot]].unwrap_or(BreakKind::Mandatory),
                ));
            } else {
                // Advances are non-negative after OpenType pair adjustment in the
                // supported Latin layout. Once a legal prefix overflows, later
                // prefixes cannot become a useful greedy break.
                break;
            }
        }
        let mut chosen_grapheme = None;
        let mut first_grapheme = None;
        if chosen_legal.is_none() {
            for (slot, &is_forbidden) in forbidden
                .iter()
                .enumerate()
                .take(mandatory_slot + 1)
                .skip(start_slot + 1)
            {
                if is_forbidden {
                    continue;
                }
                if first_grapheme.is_none() {
                    first_grapheme = Some(slot);
                }
                let candidate_end =
                    trim_trailing_space(&paragraph.text, &boundaries, start_slot, slot);
                let (candidate_glyphs, _) =
                    shape_range(&shape_context, start..candidate_end, &mut work)?;
                work = add_work(work, 1, options.max_work)?;
                if glyph_advance(&candidate_glyphs)? <= available {
                    chosen_grapheme = Some(slot);
                } else {
                    break;
                }
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
        total_glyphs = total_glyphs
            .checked_add(shaped_line.len())
            .ok_or(LayoutError::ArithmeticOverflow)?;
        if total_glyphs > options.max_glyphs {
            return Err(LayoutError::LimitExceeded("glyphs"));
        }
        let natural_width = glyph_advance(&shaped_line)?;
        let final_line = end_slot == boundaries.len() - 1 && !terminal_control;
        let mandatory_break = break_kind == BreakKind::Mandatory && !final_line;
        let justify = alignment == Alignment::Justify && !final_line && !mandatory_break;
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
        let remaining = available.saturating_sub(natural_width).max(0);
        let x = match alignment {
            Alignment::Left | Alignment::Justify => left,
            Alignment::Center => left
                .checked_add(remaining / 2)
                .ok_or(LayoutError::ArithmeticOverflow)?,
            Alignment::Right => left
                .checked_add(remaining)
                .ok_or(LayoutError::ArithmeticOverflow)?,
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
    if paragraph.text.contains('\t') {
        unsupported.push("tab stops (US-211)");
    }
    if paragraph
        .runs
        .iter()
        .any(|r| r.attrs.vertical_align.is_some())
    {
        unsupported.push("superscript/subscript baseline shifts");
    }
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
    Ok(ParagraphLayout {
        width: paragraph_width,
        height: vertical,
        lines,
        unsupported,
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
    work: &mut usize,
) -> Result<Vec<Span>, LayoutError> {
    let mut spans: Vec<Span> = Vec::new();
    for pair in boundaries.windows(2) {
        let source = pair[0]..pair[1];
        let text = &paragraph.text[source.clone()];
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
        let size = i64::from(size_half)
            .checked_mul(32)
            .ok_or(LayoutError::ArithmeticOverflow)?;
        let request = request_for_attrs(&attrs);
        let ch = text
            .chars()
            .next()
            .ok_or(LayoutError::InvalidParagraph("empty grapheme"))?;
        let resolved = fonts.resolve_for_char(&request, ch)?;
        let id = resolved.id;
        let family = resolved.family.to_owned();
        if let Some(last) = spans.last_mut() {
            if last.source.end == source.start && last.font_id == id && last.size == size {
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
        extents.ascent = extents.ascent.max(face_extents.ascent);
        extents.descent = extents.descent.max(face_extents.descent);
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
                y_offset: glyph.y_offset,
                font_id: span.font_id,
                family: span.family.clone(),
                size: span.size,
                ascent: face_extents.ascent,
                descent: face_extents.descent,
                gap: face_extents.gap,
            });
            *work = add_work(*work, 1, context.options.max_work)?;
            if output.len() > context.options.max_glyphs {
                return Err(LayoutError::LimitExceeded("glyphs"));
            }
        }
    }
    Ok((output, extents))
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
    if glyphs.is_empty() {
        return (fallback.ascent, fallback.descent, fallback.gap);
    }
    let mut result = FaceExtents::default();
    for glyph in glyphs {
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
        if grapheme
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
