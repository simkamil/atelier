// SPDX-License-Identifier: AGPL-3.0-or-later
//! Page painting of the rich editor layout (US-203 declares the module,
//! US-216 fills it): turns laid-out lines into `ATDL` display-list commands.
//!
//! The drawing code is the one the DOCX preview (`rich_preview.rs`) always
//! used, extracted unchanged: [`paint_line`] draws the glyphs of one placed
//! line, [`encode_page`] wraps the commands of one page in a display list.
//! The preview and the editor page cache both call these two functions.

use atelier_font::manager::{FontManager, FontRequest, FontStyle};
use atelier_geom::{PathCommand, Point, Rect};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, LineCap, LineJoin, PageInfo, PathElement,
    StrokeStyle,
};
use atelier_text_layout::LayoutLine;
use atelier_text_model::rich::{CharAttrs, Paragraph, Underline};
use core::fmt;
use std::collections::BTreeMap;

use crate::rich_preview::PreviewError;

pub(crate) const MAX_GLYPHS: usize = 128 * 1024;
pub(crate) const MAX_ATDL_BYTES: usize = 26 * 1024 * 1024;
pub(crate) const MAX_ATLAS_ENTRIES: usize = 4_096;
pub(crate) const MAX_ATLAS_ELEMENTS: usize = 1_000_000;
/// Display-list units are points; layout units are 1/64 point.
pub(crate) const ATDL_SCALE: f64 = 1.0 / 64.0;

/// Counters and glyph atlas shared by the lines of one painting job.
pub(crate) struct PaintSink {
    pub use_glyph_atlas: bool,
    pub atlas_indices: BTreeMap<(u64, u32, i64), Option<u32>>,
    pub atlas_commands: Vec<Command>,
    pub atlas_elements: usize,
    pub glyph_count: usize,
    pub estimated_atdl_bytes: usize,
}

impl PaintSink {
    pub(crate) fn new(use_glyph_atlas: bool, estimated_atdl_bytes: usize) -> Self {
        Self {
            use_glyph_atlas,
            atlas_indices: BTreeMap::new(),
            atlas_commands: Vec::new(),
            atlas_elements: 0,
            glyph_count: 0,
            estimated_atdl_bytes,
        }
    }
}

/// One placed line: its paragraph (text and runs), the character defaults of
/// the paragraph, the laid-out line, the page-local baseline pagination gave
/// it and the page offset of the text area (1/64 pt).
pub(crate) struct PlacedLineToPaint<'a> {
    pub paragraph: &'a Paragraph,
    pub defaults: &'a CharAttrs,
    pub line: &'a LayoutLine,
    pub baseline: i64,
    pub origin_x: i64,
    pub origin_y: i64,
}

/// Draws the glyphs of one placed line into `page_commands` (the commands of
/// its page); `page_path_elements` counts the path elements of that page.
pub(crate) fn paint_line(
    fonts: &FontManager<'_>,
    sink: &mut PaintSink,
    page_commands: &mut Vec<Command>,
    page_path_elements: &mut usize,
    placed: PlacedLineToPaint<'_>,
) -> Result<(), PreviewError> {
    let PlacedLineToPaint {
        paragraph,
        defaults,
        line,
        baseline,
        origin_x: margin_x,
        origin_y: margin_y,
    } = placed;
    for glyph in line.fragments.iter().flat_map(|fragment| &fragment.glyphs) {
        let glyph_text = paragraph
            .text
            .get(glyph.source.clone())
            .ok_or_else(|| PreviewError::Layout("glyph source range is invalid".into()))?;
        // Tabs and mandatory line breaks keep their layout advance, but
        // glyph id zero is a shaping control and must not draw .notdef.
        if glyph_text
            .chars()
            .all(|character| matches!(character, '\t' | '\r' | '\n'))
        {
            continue;
        }
        sink.glyph_count = sink
            .glyph_count
            .checked_add(1)
            .ok_or(PreviewError::Limit("glyphs"))?;
        if sink.glyph_count > MAX_GLYPHS {
            return Err(PreviewError::Limit("glyphs"));
        }
        let mut attrs = defaults.clone();
        attrs.overlay(&paragraph.attrs_at(glyph.source.start));
        let color = attrs
            .color
            .map(|c| color(c.r, c.g, c.b))
            .transpose()?
            .unwrap_or(Color::new(0.0, 0.0, 0.0, 1.0).map_err(render_error)?);
        let source_char =
            paragraph.text[glyph.source.clone()]
                .chars()
                .next()
                .ok_or(PreviewError::Layout(
                    "glyph has an empty source range".into(),
                ))?;
        let face = fonts
            .resolve_for_char(
                &FontRequest::new(
                    [glyph.family.as_str()],
                    if attrs.bold == Some(true) { 700 } else { 400 },
                    if attrs.italic == Some(true) {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                ),
                source_char,
            )
            .map_err(|e| PreviewError::Layout(e.to_string()))?;
        if face.id != glyph.font_id {
            return Err(PreviewError::Layout(
                "resolved font differs from shaped glyph face".into(),
            ));
        }
        let scale = glyph.size as f64 * ATDL_SCALE / f64::from(face.face.metrics().units_per_em);
        let origin_x = (margin_x + glyph.x) as f64 * ATDL_SCALE;
        let origin_y = (margin_y + glyph.y + baseline - line.baseline) as f64 * ATDL_SCALE;
        if sink.use_glyph_atlas {
            let key = (face.id.get(), u32::from(glyph.glyph_id), glyph.size);
            let atlas_index = if let Some(index) = sink.atlas_indices.get(&key) {
                *index
            } else {
                if sink.atlas_indices.len() >= MAX_ATLAS_ENTRIES {
                    return Err(PreviewError::Limit("glyph atlas entries"));
                }
                let outline = face
                    .face
                    .outline(glyph.glyph_id)
                    .map_err(|e| PreviewError::Layout(e.to_string()))?;
                let (count, bytes) = outline_size(&outline);
                if count == 0 {
                    sink.atlas_indices.insert(key, None);
                    None
                } else {
                    let next = sink
                        .atlas_elements
                        .checked_add(count)
                        .ok_or(PreviewError::Limit("glyph atlas elements"))?;
                    if next > MAX_ATLAS_ELEMENTS {
                        return Err(PreviewError::Limit("glyph atlas elements"));
                    }
                    charge_estimate(&mut sink.estimated_atdl_bytes, bytes, 22)?;
                    if sink.atlas_commands.len() >= atelier_render::limits::MAX_COMMANDS {
                        return Err(PreviewError::Limit("commands"));
                    }
                    let elements = convert_outline(&outline, scale, 0.0, 0.0)?;
                    if elements.len() != count {
                        return Err(PreviewError::Render("invalid outline coordinates".into()));
                    }
                    let index = sink.atlas_commands.len() as u32;
                    sink.atlas_commands.push(Command::FillPath {
                        path: DisplayPath::new(elements).map_err(render_error)?,
                        color: Color::new(0.0, 0.0, 0.0, 1.0).map_err(render_error)?,
                        rule: FillRule::NonZero,
                    });
                    sink.atlas_indices.insert(key, Some(index));
                    sink.atlas_elements = next;
                    Some(index)
                }
            };
            if let Some(index) = atlas_index {
                if page_commands.len() >= atelier_render::limits::MAX_COMMANDS {
                    return Err(PreviewError::Limit("commands"));
                }
                charge_estimate(&mut sink.estimated_atdl_bytes, 0, 65)?;
                page_commands.push(Command::DrawGlyphRun {
                    font_id: 0,
                    color,
                    glyphs: vec![atelier_render::PositionedGlyph {
                        glyph_id: index,
                        position: Point::try_new(origin_x, origin_y).map_err(render_error)?,
                        advance: Point::try_new(0.0, 0.0).map_err(render_error)?,
                    }],
                });
            }
        } else {
            let outline = face
                .face
                .outline(glyph.glyph_id)
                .map_err(|e| PreviewError::Layout(e.to_string()))?;
            // Count and charge the exact curve commands before allocating the
            // display path. Commands are immutable snapshots from geom's visitor.
            let (glyph_path_elements, glyph_path_bytes) = outline_size(&outline);
            *page_path_elements = (*page_path_elements)
                .checked_add(glyph_path_elements)
                .ok_or(PreviewError::Limit("path elements"))?;
            if *page_path_elements > atelier_render::limits::MAX_PATH_ELEMENTS {
                return Err(PreviewError::Limit("path elements"));
            }
            if glyph_path_elements > 0 {
                charge_estimate(&mut sink.estimated_atdl_bytes, glyph_path_bytes, 22)?;
                if page_commands.len() >= atelier_render::limits::MAX_COMMANDS {
                    return Err(PreviewError::Limit("commands"));
                }
            }
            let elements = convert_outline(&outline, scale, origin_x, origin_y)?;
            if elements.len() != glyph_path_elements {
                return Err(PreviewError::Render("invalid outline coordinates".into()));
            }
            if !elements.is_empty() {
                page_commands.push(Command::FillPath {
                    path: DisplayPath::new(elements).map_err(render_error)?,
                    color,
                    rule: FillRule::NonZero,
                });
            }
        }
        if attrs.underline.is_some_and(|u| u != Underline::None) && glyph.x_advance > 0 {
            let glyph_size_pt = glyph.size as f64 * ATDL_SCALE;
            let y = origin_y + glyph_size_pt * 0.12;
            let path = DisplayPath::new(vec![
                PathElement::MoveTo(Point::try_new(origin_x, y).map_err(render_error)?),
                PathElement::LineTo(
                    Point::try_new(origin_x + glyph.x_advance as f64 * ATDL_SCALE, y)
                        .map_err(render_error)?,
                ),
            ])
            .map_err(render_error)?;
            *page_path_elements = (*page_path_elements)
                .checked_add(2)
                .ok_or(PreviewError::Limit("path elements"))?;
            if *page_path_elements > atelier_render::limits::MAX_PATH_ELEMENTS {
                return Err(PreviewError::Limit("path elements"));
            }
            if page_commands.len() >= atelier_render::limits::MAX_COMMANDS {
                return Err(PreviewError::Limit("commands"));
            }
            charge_estimate(&mut sink.estimated_atdl_bytes, 34, 39)?;
            page_commands.push(Command::StrokePath {
                path,
                color,
                style: StrokeStyle::new(
                    (glyph_size_pt * 0.045).max(1.0 / 64.0),
                    LineCap::Butt,
                    LineJoin::Miter,
                    4.0,
                )
                .map_err(render_error)?,
            });
        }
    }
    Ok(())
}

/// Wraps the commands of one page in a display list of `width` x `height`
/// (1/64 pt) and encodes it.
pub(crate) fn encode_page(
    commands: Vec<Command>,
    width: i64,
    height: i64,
) -> Result<Vec<u8>, PreviewError> {
    let page = PageInfo::new(
        Rect::from_xywh(
            0.0,
            0.0,
            width as f64 * ATDL_SCALE,
            height as f64 * ATDL_SCALE,
        )
        .map_err(render_error)?,
    )
    .map_err(render_error)?;
    DisplayList::new(page, commands)
        .map_err(render_error)?
        .encode()
        .map_err(render_error)
}

fn charge_estimate(
    total: &mut usize,
    path_bytes: usize,
    command_bytes: usize,
) -> Result<(), PreviewError> {
    // path_bytes accounts for exact point/curve/close encodings. Command
    // overhead differs between FillPath and StrokePath.
    let amount = path_bytes
        .checked_add(command_bytes)
        .ok_or(PreviewError::Limit("ATDL bytes"))?;
    *total = total
        .checked_add(amount)
        .ok_or(PreviewError::Limit("ATDL bytes"))?;
    if *total > MAX_ATDL_BYTES {
        return Err(PreviewError::Limit("ATDL bytes"));
    }
    Ok(())
}
fn outline_size(outline: &atelier_geom::Path) -> (usize, usize) {
    let (mut elements, mut bytes) = (0usize, 0usize);
    let (mut contour_elements, mut contour_bytes, mut has_segment) = (0usize, 0usize, false);
    let mut finish = |count: usize, size: usize, has_segment: bool| {
        if has_segment {
            elements = elements.saturating_add(count);
            bytes = bytes.saturating_add(size);
        }
    };
    outline.visit_commands(|command| {
        if matches!(command, PathCommand::MoveTo(_)) {
            finish(contour_elements, contour_bytes, has_segment);
            contour_elements = 1;
            contour_bytes = 17;
            has_segment = false;
            return;
        }
        let size = match command {
            PathCommand::LineTo(_) => {
                has_segment = true;
                17
            }
            PathCommand::QuadraticTo { .. } => {
                has_segment = true;
                33
            }
            PathCommand::CubicTo { .. } => {
                has_segment = true;
                49
            }
            PathCommand::Close => 1,
            PathCommand::MoveTo(_) => unreachable!(),
        };
        contour_elements = contour_elements.saturating_add(1);
        contour_bytes = contour_bytes.saturating_add(size);
    });
    finish(contour_elements, contour_bytes, has_segment);
    (elements, bytes)
}

fn convert_outline(
    outline: &atelier_geom::Path,
    scale: f64,
    origin_x: f64,
    origin_y: f64,
) -> Result<Vec<PathElement>, PreviewError> {
    let (count, _) = outline_size(outline);
    let mut elements = Vec::with_capacity(count);
    let mut pending_move = None;
    let mut contour_started = false;
    let mut error = None;
    let convert = |p: Point| {
        Point::try_new(origin_x + p.x() * scale, origin_y - p.y() * scale)
            .map_err(|e| PreviewError::Render(e.to_string()))
    };
    outline.visit_commands(|command| {
        let ensure_start = |elements: &mut Vec<PathElement>,
                            pending: &mut Option<Point>,
                            error: &mut Option<PreviewError>| {
            if let Some(start) = pending.take() {
                match convert(start) {
                    Ok(start) => elements.push(PathElement::MoveTo(start)),
                    Err(e) => *error = Some(e),
                }
            }
        };
        match command {
            PathCommand::MoveTo(p) => {
                pending_move = Some(p);
                contour_started = false;
            }
            PathCommand::LineTo(p) => {
                if !contour_started {
                    ensure_start(&mut elements, &mut pending_move, &mut error);
                    contour_started = true;
                }
                match convert(p) {
                    Ok(p) => elements.push(PathElement::LineTo(p)),
                    Err(e) => error = Some(e),
                }
            }
            PathCommand::QuadraticTo { control, to } => {
                if !contour_started {
                    ensure_start(&mut elements, &mut pending_move, &mut error);
                    contour_started = true;
                }
                match (convert(control), convert(to)) {
                    (Ok(control), Ok(to)) => {
                        elements.push(PathElement::QuadraticTo { control, to })
                    }
                    (Err(e), _) | (_, Err(e)) => error = Some(e),
                }
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                if !contour_started {
                    ensure_start(&mut elements, &mut pending_move, &mut error);
                    contour_started = true;
                }
                match (convert(control1), convert(control2), convert(to)) {
                    (Ok(control1), Ok(control2), Ok(to)) => elements.push(PathElement::CubicTo {
                        control1,
                        control2,
                        to,
                    }),
                    (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => error = Some(e),
                }
            }
            PathCommand::Close if contour_started => elements.push(PathElement::Close),
            PathCommand::Close => {}
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    Ok(elements)
}
fn color(r: u8, g: u8, b: u8) -> Result<Color, PreviewError> {
    Color::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    )
    .map_err(render_error)
}
pub(crate) fn render_error(e: impl fmt::Display) -> PreviewError {
    PreviewError::Render(e.to_string())
}
