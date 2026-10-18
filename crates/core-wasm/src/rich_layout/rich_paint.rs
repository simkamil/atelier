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

// ---------------------------------------------------------------------------
// Direct page writer (editor page cache)
//
// `paint_line` builds a `Command` per glyph, which `DisplayList::encode`
// validates twice before writing it: about 5 us per glyph, 25 ms for a page of
// text. The editor repaints a page per keystroke, so it uses the functions
// below instead. They do the same arithmetic as `paint_line` (the outline of a
// glyph is converted once, at the origin, and translated when written; sums
// are the same IEEE operations in the same order, so the bytes are identical,
// which the tests of `rich_layout` check page by page) and write the `ATDL`
// commands straight into a byte vector.
// ---------------------------------------------------------------------------

/// Colour, weight, slant and underline of the glyphs of one run.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphStyle {
    color: Color,
    bold: bool,
    italic: bool,
    underline: bool,
}

fn glyph_style(
    defaults: &CharAttrs,
    paragraph: &Paragraph,
    offset: usize,
) -> Result<GlyphStyle, PreviewError> {
    let mut attrs = defaults.clone();
    attrs.overlay(&paragraph.attrs_at(offset));
    Ok(GlyphStyle {
        color: attrs
            .color
            .map(|c| color(c.r, c.g, c.b))
            .transpose()?
            .unwrap_or(Color::new(0.0, 0.0, 0.0, 1.0).map_err(render_error)?),
        bold: attrs.bold == Some(true),
        italic: attrs.italic == Some(true),
        underline: attrs.underline.is_some_and(|u| u != Underline::None),
    })
}

/// Styles of the runs of one paragraph, resolved once.
#[derive(Clone, Debug)]
pub(crate) struct StyleTable {
    default: GlyphStyle,
    runs: Vec<(usize, usize, GlyphStyle)>,
}

impl StyleTable {
    pub(crate) fn build(defaults: &CharAttrs, paragraph: &Paragraph) -> Result<Self, PreviewError> {
        let bare = Paragraph::default();
        let mut runs = Vec::with_capacity(paragraph.runs.len());
        for run in &paragraph.runs {
            runs.push((
                run.start,
                run.end,
                glyph_style(defaults, paragraph, run.start)?,
            ));
        }
        Ok(Self {
            default: glyph_style(defaults, &bare, 0)?,
            runs,
        })
    }

    fn at(&self, offset: usize) -> GlyphStyle {
        let index = self.runs.partition_point(|(_, end, _)| *end <= offset);
        match self.runs.get(index) {
            Some((start, _, style)) if *start <= offset => *style,
            _ => self.default,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ProtoElement {
    tag: u8,
    count: u8,
    points: [[f64; 2]; 3],
}

/// The outline of one glyph at one size, at the origin, as `ATDL` path
/// elements.
#[derive(Clone, Debug)]
struct GlyphProto {
    elements: Vec<ProtoElement>,
    encoded_len: usize,
}

/// Where a glyph is drawn from in an editor page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtlasSlot {
    /// No contour (space): nothing to draw.
    Empty,
    /// Atlas entry of the document.
    Entry(u32),
    /// Atlas full or entry too large: the page carries the contour itself.
    Inline,
}

/// Glyph outlines converted once, keyed by face, glyph and size, and the glyph
/// atlas of the document session: one `ATDL` display list (a black `FillPath`
/// at the origin) per distinct (face, glyph, size), numbered in creation
/// order. Pages draw an atlas glyph with a `DrawGlyphRun` of font 0 whose
/// glyph id is the atlas index; `rich_protocol` sends each entry once.
pub(crate) struct GlyphCache {
    index: std::collections::BTreeMap<(u64, u16, i64), Option<usize>>,
    protos: Vec<GlyphProto>,
    slots: std::collections::BTreeMap<(u64, u16, i64), AtlasSlot>,
    atlas: Vec<Vec<u8>>,
    atlas_bytes: usize,
    /// Entries `..atlas_sent` were handed to a reply.
    atlas_sent: usize,
    /// Entry count bound (`MAX_ATLAS_ENTRIES` unless a test lowers it).
    atlas_limit: usize,
    page: (i64, i64),
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self {
            index: BTreeMap::new(),
            protos: Vec::new(),
            slots: BTreeMap::new(),
            atlas: Vec::new(),
            atlas_bytes: 0,
            atlas_sent: 0,
            atlas_limit: MAX_ATLAS_ENTRIES,
            page: (1, 1),
        }
    }
}

const MAX_CACHED_GLYPHS: usize = 16_384;
// A contour whose entry exceeds MAX_ATLAS_ENTRY_BYTES, or that would take the
// atlas past MAX_ATLAS_TOTAL_BYTES (the bytes of one reply), is drawn inline.
use crate::rich_protocol::{
    MAX_ATLAS_ENTRIES as WIRE_ATLAS_ENTRIES, MAX_ATLAS_ENTRY_BYTES,
    MAX_ATLAS_RESPONSE_BYTES as MAX_ATLAS_TOTAL_BYTES,
};
const _: () = assert!(MAX_ATLAS_ENTRIES == WIRE_ATLAS_ENTRIES as usize);

impl GlyphCache {
    fn lookup(
        &mut self,
        fonts: &FontManager<'_>,
        glyph: &atelier_text_layout::LayoutGlyph,
        style: GlyphStyle,
        source_char: char,
    ) -> Result<Option<usize>, PreviewError> {
        let key = (glyph.font_id.get(), glyph.glyph_id, glyph.size);
        if let Some(found) = self.index.get(&key) {
            return Ok(*found);
        }
        let face = fonts
            .resolve_for_char(
                &FontRequest::new(
                    [glyph.family.as_str()],
                    if style.bold { 700 } else { 400 },
                    if style.italic {
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
        let outline = face
            .face
            .outline(glyph.glyph_id)
            .map_err(|e| PreviewError::Layout(e.to_string()))?;
        let (count, _) = outline_size(&outline);
        let found = if count == 0 {
            None
        } else {
            let elements = convert_outline(&outline, scale, 0.0, 0.0)?;
            if elements.len() != count {
                return Err(PreviewError::Render("invalid outline coordinates".into()));
            }
            // The one validation of this outline.
            DisplayPath::new(elements.clone()).map_err(render_error)?;
            if self.protos.len() >= MAX_CACHED_GLYPHS {
                self.index.clear();
                self.protos.clear();
            }
            let mut encoded_len = 4usize;
            let converted = elements
                .iter()
                .map(|element| {
                    let (tag, points): (u8, Vec<Point>) = match *element {
                        PathElement::MoveTo(p) => (0, vec![p]),
                        PathElement::LineTo(p) => (1, vec![p]),
                        PathElement::QuadraticTo { control, to } => (2, vec![control, to]),
                        PathElement::CubicTo {
                            control1,
                            control2,
                            to,
                        } => (3, vec![control1, control2, to]),
                        PathElement::Close => (4, Vec::new()),
                    };
                    encoded_len += 1 + 16 * points.len();
                    let mut stored = [[0.0; 2]; 3];
                    for (slot, p) in stored.iter_mut().zip(&points) {
                        *slot = [p.x(), p.y()];
                    }
                    ProtoElement {
                        tag,
                        count: points.len() as u8,
                        points: stored,
                    }
                })
                .collect();
            self.protos.push(GlyphProto {
                elements: converted,
                encoded_len,
            });
            Some(self.protos.len() - 1)
        };
        self.index.insert(key, found);
        Ok(found)
    }
}

impl GlyphCache {
    /// Sets the page size (1/64 pt) stamped in the header of atlas entries.
    pub(crate) fn set_page(&mut self, width: i64, height: i64) {
        self.page = (width, height);
    }

    /// Lowers the entry bound (tests of the inline fallback).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn set_atlas_limit(&mut self, limit: usize) {
        self.atlas_limit = limit;
    }

    /// The atlas entries of the session.
    pub(crate) fn atlas(&self) -> &[Vec<u8>] {
        &self.atlas
    }

    /// The entries no reply carried yet, with the index of the first, and
    /// marks them sent.
    pub(crate) fn take_atlas(&mut self) -> (u32, Vec<Vec<u8>>) {
        let first = self.atlas_sent;
        self.atlas_sent = self.atlas.len();
        (first as u32, self.atlas[first..].to_vec())
    }

    /// Forgets the atlas (a new epoch starts at entry 0). Painted pages that
    /// refer to it must be dropped by the caller.
    pub(crate) fn reset_atlas(&mut self) {
        self.slots.clear();
        self.atlas.clear();
        self.atlas_bytes = 0;
        self.atlas_sent = 0;
    }

    fn slot(
        &mut self,
        fonts: &FontManager<'_>,
        glyph: &atelier_text_layout::LayoutGlyph,
        style: GlyphStyle,
        source_char: char,
    ) -> Result<(AtlasSlot, Option<usize>), PreviewError> {
        let key = (glyph.font_id.get(), glyph.glyph_id, glyph.size);
        if let Some(slot) = self.slots.get(&key).copied() {
            let proto = match slot {
                AtlasSlot::Inline => self.lookup(fonts, glyph, style, source_char)?,
                _ => None,
            };
            return Ok((slot, proto));
        }
        let proto = self.lookup(fonts, glyph, style, source_char)?;
        let Some(proto_index) = proto else {
            self.slots.insert(key, AtlasSlot::Empty);
            return Ok((AtlasSlot::Empty, None));
        };
        let entry = {
            let proto = &self.protos[proto_index];
            if self.atlas.len() >= self.atlas_limit
                || proto.encoded_len + 66 > MAX_ATLAS_ENTRY_BYTES
                || self.atlas_bytes + proto.encoded_len + 66 > MAX_ATLAS_TOTAL_BYTES
            {
                None
            } else {
                let mut bytes = encode_page(Vec::new(), self.page.0, self.page.1)?;
                put_fill(&mut bytes, proto, BLACK, 0.0, 0.0);
                bytes[40..44].copy_from_slice(&1u32.to_le_bytes());
                Some(bytes)
            }
        };
        let slot = match entry {
            Some(bytes) => {
                self.atlas_bytes += bytes.len();
                self.atlas.push(bytes);
                AtlasSlot::Entry((self.atlas.len() - 1) as u32)
            }
            None => AtlasSlot::Inline,
        };
        self.slots.insert(key, slot);
        Ok((slot, (slot == AtlasSlot::Inline).then_some(proto_index)))
    }
}

const BLACK: Color = Color {
    red: 0.0,
    green: 0.0,
    blue: 0.0,
    alpha: 1.0,
};

/// One `FillPath` command of `proto` translated by (`x`, `y`).
fn put_fill(out: &mut Vec<u8>, proto: &GlyphProto, color: Color, x: f64, y: f64) {
    out.reserve(18 + proto.encoded_len);
    out.push(1);
    out.push(0); // FillRule::NonZero
    put_color_bytes(out, color);
    out.extend_from_slice(&(proto.elements.len() as u32).to_le_bytes());
    for element in &proto.elements {
        let mut buffer = [0u8; 49];
        buffer[0] = element.tag;
        let mut at = 1;
        for point in &element.points[..usize::from(element.count)] {
            buffer[at..at + 8].copy_from_slice(&(x + point[0]).to_le_bytes());
            buffer[at + 8..at + 16].copy_from_slice(&(y + point[1]).to_le_bytes());
            at += 16;
        }
        out.extend_from_slice(&buffer[..at]);
    }
}

/// Atlas index and position of one glyph of a run.
type AtlasGlyph = (u32, f64, f64);

/// Writes the commands of one page straight into `ATDL` bytes.
pub(crate) struct PageWriter {
    /// The 44-byte header (its command count patched by `finish`), then the commands.
    bytes: Vec<u8>,
    commands: usize,
    path_elements: usize,
    glyphs: usize,
    /// Atlas glyphs of the open run: colour, then (index, x, y) per glyph.
    run: Option<(Color, Vec<AtlasGlyph>)>,
}

fn put_color_bytes(out: &mut Vec<u8>, color: Color) {
    for v in [color.red, color.green, color.blue, color.alpha] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

impl PageWriter {
    /// A writer for a page of `width` x `height` (1/64 pt).
    pub(crate) fn new(width: i64, height: i64) -> Result<Self, PreviewError> {
        let mut bytes = encode_page(Vec::new(), width, height)?;
        bytes.reserve(4 * 1024 * 1024);
        Ok(Self {
            bytes,
            commands: 0,
            path_elements: 0,
            glyphs: 0,
            run: None,
        })
    }

    fn count_command(&mut self, path_elements: usize) -> Result<(), PreviewError> {
        self.commands += 1;
        self.path_elements = self
            .path_elements
            .checked_add(path_elements)
            .ok_or(PreviewError::Limit("path elements"))?;
        if self.commands > atelier_render::limits::MAX_COMMANDS {
            return Err(PreviewError::Limit("commands"));
        }
        if self.path_elements > atelier_render::limits::MAX_PATH_ELEMENTS {
            return Err(PreviewError::Limit("path elements"));
        }
        Ok(())
    }

    fn fill_glyph(
        &mut self,
        proto: &GlyphProto,
        color: Color,
        x: f64,
        y: f64,
    ) -> Result<(), PreviewError> {
        self.flush_run()?;
        self.count_command(proto.elements.len())?;
        put_fill(&mut self.bytes, proto, color, x, y);
        Ok(())
    }

    /// Adds an atlas glyph to the open run, which continues while the colour
    /// does; any other command ends it, so that the paint order is kept.
    fn atlas_glyph(
        &mut self,
        index: u32,
        color: Color,
        x: f64,
        y: f64,
    ) -> Result<(), PreviewError> {
        // Same validation as a `DrawGlyphRun` of `Command`.
        Point::try_new(x, y).map_err(render_error)?;
        if self.run.as_ref().is_some_and(|(open, _)| *open != color) {
            self.flush_run()?;
        }
        self.run
            .get_or_insert_with(|| (color, Vec::new()))
            .1
            .push((index, x, y));
        Ok(())
    }

    /// Writes the open run as one `DrawGlyphRun` of font 0.
    fn flush_run(&mut self) -> Result<(), PreviewError> {
        let Some((color, glyphs)) = self.run.take() else {
            return Ok(());
        };
        self.count_command(0)?;
        let out = &mut self.bytes;
        out.reserve(29 + 36 * glyphs.len());
        out.push(3);
        out.extend_from_slice(&0u64.to_le_bytes());
        put_color_bytes(out, color);
        out.extend_from_slice(&(glyphs.len() as u32).to_le_bytes());
        for (index, x, y) in glyphs {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.extend_from_slice(&0f64.to_le_bytes());
            out.extend_from_slice(&0f64.to_le_bytes());
        }
        Ok(())
    }

    fn underline(
        &mut self,
        color: Color,
        width: f64,
        from: [f64; 2],
        to: [f64; 2],
    ) -> Result<(), PreviewError> {
        self.flush_run()?;
        // Same validation as the display path of `paint_line`.
        DisplayPath::new(vec![
            PathElement::MoveTo(Point::try_new(from[0], from[1]).map_err(render_error)?),
            PathElement::LineTo(Point::try_new(to[0], to[1]).map_err(render_error)?),
        ])
        .map_err(render_error)?;
        let style =
            StrokeStyle::new(width, LineCap::Butt, LineJoin::Miter, 4.0).map_err(render_error)?;
        self.count_command(2)?;
        let out = &mut self.bytes;
        out.push(2);
        put_color_bytes(out, color);
        out.extend_from_slice(&style.width.to_le_bytes());
        out.push(style.cap as u8);
        out.push(style.join as u8);
        out.extend_from_slice(&style.miter_limit.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.push(0);
        out.extend_from_slice(&from[0].to_le_bytes());
        out.extend_from_slice(&from[1].to_le_bytes());
        out.push(1);
        out.extend_from_slice(&to[0].to_le_bytes());
        out.extend_from_slice(&to[1].to_le_bytes());
        Ok(())
    }

    /// The finished display list.
    pub(crate) fn finish(mut self) -> Result<Vec<u8>, PreviewError> {
        self.flush_run()?;
        let count_at = 40;
        let count = u32::try_from(self.commands).map_err(|_| PreviewError::Limit("commands"))?;
        self.bytes[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
        if self.bytes.len() > atelier_render::limits::MAX_BYTES || self.bytes.len() > MAX_ATDL_BYTES
        {
            return Err(PreviewError::Limit("ATDL bytes"));
        }
        Ok(self.bytes)
    }
}

/// A placed line of the editor: its paragraph, style table and layout line,
/// the page-local baseline pagination gave it and the page offset of the
/// text area (1/64 pt).
pub(crate) struct FastLine<'a> {
    pub styles: &'a StyleTable,
    pub paragraph: &'a Paragraph,
    pub line: &'a LayoutLine,
    pub baseline: i64,
    pub margin: (i64, i64),
}

/// [`paint_line`] for the editor: the same glyphs, positions, colours and
/// underlines, written into `writer` with the outlines of `cache`.
pub(crate) fn paint_line_fast(
    fonts: &FontManager<'_>,
    cache: &mut GlyphCache,
    writer: &mut PageWriter,
    placed: FastLine<'_>,
) -> Result<(), PreviewError> {
    let FastLine {
        styles,
        paragraph,
        line,
        baseline,
        margin,
    } = placed;
    for glyph in line.fragments.iter().flat_map(|fragment| &fragment.glyphs) {
        let glyph_text = paragraph
            .text
            .get(glyph.source.clone())
            .ok_or_else(|| PreviewError::Layout("glyph source range is invalid".into()))?;
        if glyph_text
            .chars()
            .all(|character| matches!(character, '\t' | '\r' | '\n'))
        {
            continue;
        }
        writer.glyphs += 1;
        if writer.glyphs > MAX_GLYPHS {
            return Err(PreviewError::Limit("glyphs"));
        }
        let style = styles.at(glyph.source.start);
        let source_char = glyph_text.chars().next().ok_or(PreviewError::Layout(
            "glyph has an empty source range".into(),
        ))?;
        let origin_x = (margin.0 + glyph.x) as f64 * ATDL_SCALE;
        let origin_y = (margin.1 + glyph.y + baseline - line.baseline) as f64 * ATDL_SCALE;
        match cache.slot(fonts, glyph, style, source_char)? {
            (AtlasSlot::Entry(index), _) => {
                writer.atlas_glyph(index, style.color, origin_x, origin_y)?;
            }
            (AtlasSlot::Inline, Some(proto)) => {
                writer.fill_glyph(&cache.protos[proto], style.color, origin_x, origin_y)?;
            }
            _ => {}
        }
        if style.underline && glyph.x_advance > 0 {
            let glyph_size_pt = glyph.size as f64 * ATDL_SCALE;
            let y = origin_y + glyph_size_pt * 0.12;
            writer.underline(
                style.color,
                (glyph_size_pt * 0.045).max(1.0 / 64.0),
                [origin_x, y],
                [origin_x + glyph.x_advance as f64 * ATDL_SCALE, y],
            )?;
        }
    }
    Ok(())
}
