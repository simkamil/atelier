// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only DOCX import and paginated A4 display-list generation.

use atelier_fmt_ooxml::{DocxLimits, import_docx_with_limits};
use atelier_font::manager::{FontManager, FontManagerLimits, FontRequest, FontSource, FontStyle};
use atelier_geom::{Point, Rect};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, LineCap, LineJoin, PageInfo, PathElement,
    StrokeStyle,
};
use atelier_text_layout::{
    LayoutOptions, layout_paragraph,
    pagination::{self, PaginationOptions, ParagraphRules},
};
use atelier_text_model::rich::Underline;
use core::fmt;

const MAX_DOCX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_PARAGRAPHS: usize = 1024;
const MAX_PAGES: usize = 256;
const MAX_GLYPHS: usize = 65_536;
const MAX_ATDL_BYTES: usize = 26 * 1024 * 1024;
const MAX_EMBEDDED_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_WORK: usize = 8_000_000;
const MIN_PARAGRAPH_WORK: usize = 1_024;
const ATDL_SCALE: f64 = 1.0 / 64.0;
const PAGE_WIDTH: i64 = 38_098; // 210 mm, rounded to 1/64 pt
const PAGE_HEIGHT: i64 = 53_881; // 297 mm, rounded to 1/64 pt
const MARGIN: i64 = 4_535; // 25 mm, rounded to 1/64 pt
const CONTENT_WIDTH: i64 = PAGE_WIDTH - 2 * MARGIN;
const CONTENT_HEIGHT: i64 = PAGE_HEIGHT - 2 * MARGIN;

const CARLITO_REGULAR: &[u8] = include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf");
const CARLITO_BOLD: &[u8] = include_bytes!("../../../data/fonts/carlito/Carlito-Bold.ttf");
const CARLITO_ITALIC: &[u8] = include_bytes!("../../../data/fonts/carlito/Carlito-Italic.ttf");
const CARLITO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../data/fonts/carlito/Carlito-BoldItalic.ttf");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RichPreview {
    pub pages: Vec<Vec<u8>>,
    pub text: String,
    pub notice_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewError {
    InputTooLarge,
    Limit(&'static str),
    Import(String),
    Layout(String),
    Render(String),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rich preview error: {self:?}")
    }
}
impl std::error::Error for PreviewError {}

/// Imports a bounded DOCX and returns one independent ATDL stream per A4 page.
/// The function owns all intermediate state and cannot mutate the TXT editor.
pub fn preview_docx(bytes: &[u8]) -> Result<RichPreview, PreviewError> {
    if bytes.len() > MAX_DOCX_BYTES {
        return Err(PreviewError::InputTooLarge);
    }
    let limits = DocxLimits {
        max_paragraphs: MAX_PARAGRAPHS,
        max_runs: 65_536,
        max_text_bytes: MAX_TEXT_BYTES,
        max_styles: 4_096,
        max_fonts: 16,
        max_font_bytes: MAX_EMBEDDED_FONT_BYTES,
        package: bounded_package_limits(atelier_fmt_ooxml::Limits::default()),
        ..DocxLimits::default()
    };
    let mut imported =
        import_docx_with_limits(bytes, limits).map_err(|e| PreviewError::Import(e.to_string()))?;
    let ids = imported.paragraph_ids().to_vec();
    if ids.len() > MAX_PARAGRAPHS {
        return Err(PreviewError::Limit("paragraphs"));
    }
    let paragraph_bytes = ids
        .iter()
        .map(|id| imported.document.paragraph(*id).map(|p| p.text.len()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PreviewError::Import(e.to_string()))?;
    let total_source_bytes = paragraph_bytes.iter().sum::<usize>() + ids.len().saturating_sub(1);
    if total_source_bytes > MAX_TEXT_BYTES {
        return Err(PreviewError::Limit("text"));
    }
    let mut fonts = FontManager::with_limits(FontManagerLimits {
        max_faces: 4 + 16,
        max_total_bytes: MAX_EMBEDDED_FONT_BYTES
            + CARLITO_REGULAR.len()
            + CARLITO_BOLD.len()
            + CARLITO_ITALIC.len()
            + CARLITO_BOLD_ITALIC.len(),
    });
    for data in [
        CARLITO_REGULAR,
        CARLITO_BOLD,
        CARLITO_ITALIC,
        CARLITO_BOLD_ITALIC,
    ] {
        fonts
            .register(data, 0, FontSource::System)
            .map_err(|e| PreviewError::Layout(e.to_string()))?;
    }
    // Detach the font byte buffers so resolving styles can mutably use the
    // importer while the font manager keeps borrowing the registered faces.
    let embedded_fonts = imported.font_bytes.clone();
    for data in &embedded_fonts {
        fonts
            .register(data, 0, FontSource::Document)
            .map_err(|e| PreviewError::Import(e.to_string()))?;
    }

    let mut text = String::new();
    let mut paragraphs = Vec::with_capacity(ids.len());
    let mut character_defaults = Vec::with_capacity(ids.len());
    let mut layouts = Vec::with_capacity(ids.len());
    let mut rules = Vec::with_capacity(ids.len());
    let mut total_glyphs = 0usize;
    let reserved_work = ids
        .len()
        .checked_mul(MIN_PARAGRAPH_WORK)
        .ok_or(PreviewError::Limit("layout work"))?;
    let mut remaining_work = MAX_WORK.saturating_sub(reserved_work);
    let mut remaining_weight = paragraph_bytes.iter().map(|len| len + 1).sum::<usize>();
    for (index, id) in ids.iter().copied().enumerate() {
        let (paragraph, defaults) = imported
            .resolved_paragraph(id)
            .map_err(|e| PreviewError::Import(e.to_string()))?;
        character_defaults.push(defaults.character.clone());
        let next_len = text
            .len()
            .checked_add(paragraph.text.len())
            .and_then(|n| n.checked_add(usize::from(index > 0)))
            .ok_or(PreviewError::Limit("text"))?;
        if next_len > MAX_TEXT_BYTES {
            return Err(PreviewError::Limit("text"));
        }
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&paragraph.text);
        let weight = paragraph_bytes[index] + 1;
        let extra_work = if index + 1 == ids.len() {
            remaining_work
        } else {
            (remaining_work as u128 * weight as u128 / remaining_weight as u128) as usize
        };
        let paragraph_work = MIN_PARAGRAPH_WORK + extra_work;
        if paragraph_work == 0 {
            return Err(PreviewError::Limit("layout work"));
        }
        remaining_work -= extra_work;
        remaining_weight -= weight;
        let glyph_budget = MAX_GLYPHS.saturating_sub(total_glyphs).max(1);
        let layout = layout_paragraph(
            &paragraph,
            &defaults,
            &fonts,
            LayoutOptions {
                width: CONTENT_WIDTH,
                max_text_bytes: MAX_TEXT_BYTES,
                max_glyphs: glyph_budget,
                max_lines: 16_384,
                max_work: paragraph_work,
            },
        )
        .map_err(|e| PreviewError::Layout(e.to_string()))?;
        let laid_out_glyphs = layout
            .lines
            .iter()
            .flat_map(|line| &line.fragments)
            .map(|fragment| fragment.glyphs.len())
            .sum::<usize>();
        total_glyphs = total_glyphs
            .checked_add(laid_out_glyphs)
            .ok_or(PreviewError::Limit("glyphs"))?;
        if total_glyphs > MAX_GLYPHS {
            return Err(PreviewError::Limit("glyphs"));
        }
        rules.push(ParagraphRules {
            style_id: paragraph
                .props
                .style
                .as_deref()
                .map(stable_style_id)
                .unwrap_or(0),
            space_before: twips(i64::from(paragraph.props.space_before.unwrap_or(0)))?,
            space_after: twips(i64::from(paragraph.props.space_after.unwrap_or(0)))?,
            ..ParagraphRules::default()
        });
        paragraphs.push(paragraph);
        layouts.push(layout);
    }
    let pagination = pagination::paginate(
        &layouts,
        &rules,
        PaginationOptions {
            page_height: CONTENT_HEIGHT,
            max_pages: MAX_PAGES,
            max_work: MAX_WORK,
        },
    )
    .map_err(|e| PreviewError::Layout(e.to_string()))?;
    if pagination.pages.len() > MAX_PAGES {
        return Err(PreviewError::Limit("pages"));
    }

    let mut commands: Vec<Vec<Command>> = (0..pagination.pages.len()).map(|_| Vec::new()).collect();
    let mut glyph_count = 0usize;
    let mut path_elements = vec![0usize; pagination.pages.len()];
    let mut estimated_atdl_bytes = pagination.pages.len().saturating_mul(48);
    for placed in &pagination.placements {
        let paragraph = &paragraphs[placed.paragraph_index];
        let line = &layouts[placed.paragraph_index].lines[placed.line_index];
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
            glyph_count = glyph_count
                .checked_add(1)
                .ok_or(PreviewError::Limit("glyphs"))?;
            if glyph_count > MAX_GLYPHS {
                return Err(PreviewError::Limit("glyphs"));
            }
            let mut attrs = character_defaults[placed.paragraph_index].clone();
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
            let scale =
                glyph.size as f64 * ATDL_SCALE / f64::from(face.face.metrics().units_per_em);
            let origin_x = (MARGIN + glyph.x) as f64 * ATDL_SCALE;
            let origin_y = (MARGIN + glyph.y + placed.baseline - line.baseline) as f64 * ATDL_SCALE;
            let outline = face
                .face
                .outline(glyph.glyph_id)
                .map_err(|e| PreviewError::Layout(e.to_string()))?;
            let flattened = outline
                .flatten(8.0)
                .map_err(|e| PreviewError::Layout(e.to_string()))?;
            let glyph_path_elements = flattened
                .iter()
                .filter(|contour| contour.points.len() >= 3)
                .map(|contour| contour.points.len() + usize::from(contour.closed))
                .sum::<usize>();
            path_elements[placed.page_index] = path_elements[placed.page_index]
                .checked_add(glyph_path_elements)
                .ok_or(PreviewError::Limit("path elements"))?;
            if path_elements[placed.page_index] > atelier_render::limits::MAX_PATH_ELEMENTS {
                return Err(PreviewError::Limit("path elements"));
            }
            if glyph_path_elements > 0 {
                charge_estimate(&mut estimated_atdl_bytes, glyph_path_elements)?;
                if commands[placed.page_index].len() >= atelier_render::limits::MAX_COMMANDS {
                    return Err(PreviewError::Limit("commands"));
                }
            }
            let mut elements = Vec::new();
            for contour in flattened {
                if contour.points.len() < 3 {
                    continue;
                }
                let element_count = contour.points.len() + usize::from(contour.closed);
                elements.reserve(element_count);
                for (i, p) in contour.points.iter().enumerate() {
                    let point = Point::try_new(origin_x + p.x() * scale, origin_y - p.y() * scale)
                        .map_err(|e| PreviewError::Render(e.to_string()))?;
                    elements.push(if i == 0 {
                        PathElement::MoveTo(point)
                    } else {
                        PathElement::LineTo(point)
                    });
                }
                if contour.closed {
                    elements.push(PathElement::Close);
                }
            }
            if !elements.is_empty() {
                commands[placed.page_index].push(Command::FillPath {
                    path: DisplayPath::new(elements).map_err(render_error)?,
                    color,
                    rule: FillRule::NonZero,
                });
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
                path_elements[placed.page_index] = path_elements[placed.page_index]
                    .checked_add(2)
                    .ok_or(PreviewError::Limit("path elements"))?;
                if path_elements[placed.page_index] > atelier_render::limits::MAX_PATH_ELEMENTS {
                    return Err(PreviewError::Limit("path elements"));
                }
                if commands[placed.page_index].len() >= atelier_render::limits::MAX_COMMANDS {
                    return Err(PreviewError::Limit("commands"));
                }
                charge_estimate(&mut estimated_atdl_bytes, 2)?;
                commands[placed.page_index].push(Command::StrokePath {
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
    }
    let mut pages = Vec::with_capacity(commands.len());
    let mut total = 0usize;
    for commands in commands {
        let page = PageInfo::new(
            Rect::from_xywh(
                0.0,
                0.0,
                PAGE_WIDTH as f64 * ATDL_SCALE,
                PAGE_HEIGHT as f64 * ATDL_SCALE,
            )
            .map_err(render_error)?,
        )
        .map_err(render_error)?;
        let bytes = DisplayList::new(page, commands)
            .map_err(render_error)?
            .encode()
            .map_err(render_error)?;
        total = total
            .checked_add(bytes.len())
            .ok_or(PreviewError::Limit("ATDL bytes"))?;
        if total > MAX_ATDL_BYTES {
            return Err(PreviewError::Limit("ATDL bytes"));
        }
        pages.push(bytes);
    }
    Ok(RichPreview {
        pages,
        text,
        notice_count: imported
            .notices
            .len()
            .saturating_add(pagination.diagnostics.len())
            .saturating_add(
                layouts
                    .iter()
                    .map(|layout| layout.unsupported.len())
                    .sum::<usize>(),
            )
            .min(u32::MAX as usize) as u32,
    })
}

fn stable_style_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn bounded_package_limits(mut limits: atelier_fmt_ooxml::Limits) -> atelier_fmt_ooxml::Limits {
    limits.max_total_part_bytes = 32 * 1024 * 1024;
    limits.max_xml_part_bytes = 8 * 1024 * 1024;
    limits.max_parts = 512;
    limits.max_relationships = 4_096;
    limits.zip.max_total_size = 32 * 1024 * 1024;
    limits.zip.max_entry_size = 16 * 1024 * 1024;
    limits
}
fn charge_estimate(total: &mut usize, path_elements: usize) -> Result<(), PreviewError> {
    // One command costs at most 48 bytes before path elements; each path
    // element costs at most 17 bytes in the ATDL encoding.
    let amount = path_elements
        .checked_mul(17)
        .and_then(|n| n.checked_add(48))
        .ok_or(PreviewError::Limit("ATDL bytes"))?;
    *total = total
        .checked_add(amount)
        .ok_or(PreviewError::Limit("ATDL bytes"))?;
    if *total > MAX_ATDL_BYTES {
        return Err(PreviewError::Limit("ATDL bytes"));
    }
    Ok(())
}
fn twips(v: i64) -> Result<i64, PreviewError> {
    v.checked_mul(16)
        .map(|n| n / 5)
        .ok_or(PreviewError::Limit("geometry"))
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
fn render_error(e: impl fmt::Display) -> PreviewError {
    PreviewError::Render(e.to_string())
}
