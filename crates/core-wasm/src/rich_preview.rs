// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded DOCX import and paginated A4 display-list generation.

use atelier_fmt_ooxml::{DocxLimits, ImportedDocx, import_docx_with_limits};
use atelier_font::manager::{FontManager, FontManagerLimits, FontRequest, FontSource, FontStyle};
use atelier_geom::{PathCommand, Point, Rect};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, FillRule, LineCap, LineJoin, PageInfo, PathElement,
    StrokeStyle,
};
use atelier_text_layout::{
    LayoutOptions, ParagraphLayout, layout_paragraph,
    pagination::{self, PaginationOptions, ParagraphRules},
};
use atelier_text_model::rich::{CharAttrs, NodeId, Paragraph, Underline};
use core::fmt;
use std::collections::BTreeMap;

const MAX_DOCX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_PARAGRAPHS: usize = 1024;
const MAX_PAGES: usize = 256;
const MAX_GLYPHS: usize = 128 * 1024;
const MAX_ATDL_BYTES: usize = 26 * 1024 * 1024;
const MAX_ATLAS_ENTRIES: usize = 4_096;
const MAX_ATLAS_ELEMENTS: usize = 1_000_000;
const ATLAS_TEXT_THRESHOLD: usize = 65_536;
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
    pub glyph_atlas: Option<Vec<u8>>,
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

/// Imports a bounded DOCX into a retained semantic model using the preview budgets.
pub fn import_preview_docx(bytes: &[u8]) -> Result<ImportedDocx, PreviewError> {
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
    let imported =
        import_docx_with_limits(bytes, limits).map_err(|e| PreviewError::Import(e.to_string()))?;
    validate_retained_text(&imported)?;
    Ok(imported)
}

/// Fixed progress scale reported by [`RenderJob::progress`].
pub const PROGRESS_TOTAL: u32 = 1_000;

/// Upper bounds of work done by one cooperative [`RenderJob::step`] call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepBudget {
    pub paragraphs: usize,
    pub placements: usize,
    pub pages: usize,
}

impl StepBudget {
    pub const UNLIMITED: Self = Self {
        paragraphs: usize::MAX,
        placements: usize::MAX,
        pages: usize::MAX,
    };
}

struct PaintState {
    pagination: pagination::Pagination,
    commands: Vec<Vec<Command>>,
    use_glyph_atlas: bool,
    atlas_indices: BTreeMap<(u64, u32, i64), Option<u32>>,
    atlas_commands: Vec<Command>,
    atlas_elements: usize,
    glyph_count: usize,
    path_elements: Vec<usize>,
    estimated_atdl_bytes: usize,
    next_placement: usize,
}

struct EncodeState {
    glyph_atlas: Option<Vec<u8>>,
    commands: Vec<Vec<Command>>,
    pages: Vec<Vec<u8>>,
    total: usize,
    next_page: usize,
    diagnostics: usize,
}

enum Phase {
    Layout,
    Paint(PaintState),
    Encode(EncodeState),
    Done,
}

/// Resumable form of [`render_imported_docx`]: layout, painting and page
/// encoding advance in bounded steps, so a caller can stop between steps and
/// drop the job. Running every step to the end gives the one-shot result.
pub struct RenderJob {
    ids: Vec<NodeId>,
    paragraph_bytes: Vec<usize>,
    text: String,
    paragraphs: Vec<Paragraph>,
    character_defaults: Vec<CharAttrs>,
    layouts: Vec<ParagraphLayout>,
    rules: Vec<ParagraphRules>,
    total_glyphs: usize,
    remaining_work: usize,
    remaining_weight: usize,
    next_paragraph: usize,
    phase: Phase,
}

fn build_fonts(embedded_fonts: &[Vec<u8>]) -> Result<FontManager<'_>, PreviewError> {
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
    for data in embedded_fonts {
        fonts
            .register(data, 0, FontSource::Document)
            .map_err(|e| PreviewError::Import(e.to_string()))?;
    }
    Ok(fonts)
}

/// Renders the current retained DOCX model without re-importing its package bytes.
pub fn render_imported_docx(imported: &mut ImportedDocx) -> Result<RichPreview, PreviewError> {
    let mut job = RenderJob::new(imported)?;
    loop {
        if let Some(preview) = job.step(imported, StepBudget::UNLIMITED)? {
            return Ok(preview);
        }
    }
}

impl RenderJob {
    pub fn new(imported: &ImportedDocx) -> Result<Self, PreviewError> {
        let ids = imported.paragraph_ids().to_vec();
        validate_retained_text(imported)?;
        let paragraph_bytes = ids
            .iter()
            .map(|id| imported.document.paragraph(*id).map(|p| p.text.len()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PreviewError::Import(e.to_string()))?;
        let reserved_work = ids
            .len()
            .checked_mul(MIN_PARAGRAPH_WORK)
            .ok_or(PreviewError::Limit("layout work"))?;
        let remaining_weight = paragraph_bytes.iter().map(|len| len + 1).sum::<usize>();
        Ok(Self {
            paragraphs: Vec::with_capacity(ids.len()),
            character_defaults: Vec::with_capacity(ids.len()),
            layouts: Vec::with_capacity(ids.len()),
            rules: Vec::with_capacity(ids.len()),
            ids,
            paragraph_bytes,
            text: String::new(),
            total_glyphs: 0,
            remaining_work: MAX_WORK.saturating_sub(reserved_work),
            remaining_weight,
            next_paragraph: 0,
            phase: Phase::Layout,
        })
    }

    /// Completed work on a fixed `0..=PROGRESS_TOTAL` scale (layout 100-500,
    /// painting 500-900, page encoding 900-1000); it never decreases.
    pub fn progress(&self) -> u32 {
        let fraction = |done: usize, total: usize, span: u32| -> u32 {
            if total == 0 {
                span
            } else {
                (u64::from(span) * done.min(total) as u64 / total as u64) as u32
            }
        };
        match &self.phase {
            Phase::Layout => 100 + fraction(self.next_paragraph, self.ids.len(), 400),
            Phase::Paint(p) => 500 + fraction(p.next_placement, p.pagination.placements.len(), 400),
            Phase::Encode(e) => 900 + fraction(e.next_page, e.commands.len(), 100),
            Phase::Done => PROGRESS_TOTAL,
        }
    }

    /// Runs one bounded unit of work. Returns the preview after the last unit.
    pub fn step(
        &mut self,
        imported: &mut ImportedDocx,
        budget: StepBudget,
    ) -> Result<Option<RichPreview>, PreviewError> {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Layout => {
                self.layout_step(imported, budget.paragraphs.max(1))?;
                Ok(None)
            }
            Phase::Paint(state) => {
                self.paint_step(imported, state, budget.placements.max(1))?;
                Ok(None)
            }
            Phase::Encode(state) => self.encode_step(imported, state, budget.pages.max(1)),
            Phase::Done => Err(PreviewError::Limit("finished job")),
        }
    }

    fn layout_step(
        &mut self,
        imported: &mut ImportedDocx,
        max_paragraphs: usize,
    ) -> Result<(), PreviewError> {
        // Detach the font byte buffers so resolving styles can mutably use the
        // importer while the font manager keeps borrowing the registered faces.
        let embedded_fonts = imported.font_bytes.clone();
        let fonts = build_fonts(&embedded_fonts)?;
        let end = self
            .next_paragraph
            .saturating_add(max_paragraphs)
            .min(self.ids.len());
        while self.next_paragraph < end {
            let index = self.next_paragraph;
            let id = self.ids[index];
            let (paragraph, defaults) = imported
                .resolved_paragraph(id)
                .map_err(|e| PreviewError::Import(e.to_string()))?;
            self.character_defaults.push(defaults.character.clone());
            let next_len = self
                .text
                .len()
                .checked_add(paragraph.text.len())
                .and_then(|n| n.checked_add(usize::from(index > 0)))
                .ok_or(PreviewError::Limit("self.text"))?;
            if next_len > MAX_TEXT_BYTES {
                return Err(PreviewError::Limit("self.text"));
            }
            if index > 0 {
                self.text.push('\n');
            }
            self.text.push_str(&paragraph.text);
            let weight = self.paragraph_bytes[index] + 1;
            let extra_work = if index + 1 == self.ids.len() {
                self.remaining_work
            } else {
                (self.remaining_work as u128 * weight as u128 / self.remaining_weight as u128)
                    as usize
            };
            let paragraph_work = MIN_PARAGRAPH_WORK + extra_work;
            if paragraph_work == 0 {
                return Err(PreviewError::Limit("layout work"));
            }
            self.remaining_work -= extra_work;
            self.remaining_weight -= weight;
            let glyph_budget = MAX_GLYPHS.saturating_sub(self.total_glyphs).max(1);
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
            self.total_glyphs = self
                .total_glyphs
                .checked_add(laid_out_glyphs)
                .ok_or(PreviewError::Limit("glyphs"))?;
            if self.total_glyphs > MAX_GLYPHS {
                return Err(PreviewError::Limit("glyphs"));
            }
            self.rules.push(ParagraphRules {
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
            self.paragraphs.push(paragraph);
            self.layouts.push(layout);
            self.next_paragraph += 1;
        }
        if self.next_paragraph < self.ids.len() {
            self.phase = Phase::Layout;
            return Ok(());
        }
        let pagination = pagination::paginate(
            &self.layouts,
            &self.rules,
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
        let use_glyph_atlas = self.text.len() > ATLAS_TEXT_THRESHOLD;
        let page_count = pagination.pages.len();
        self.phase = Phase::Paint(PaintState {
            commands: (0..page_count).map(|_| Vec::new()).collect(),
            use_glyph_atlas,
            atlas_indices: BTreeMap::new(),
            atlas_commands: Vec::new(),
            atlas_elements: 0,
            glyph_count: 0,
            path_elements: vec![0usize; page_count],
            estimated_atdl_bytes: page_count
                .saturating_mul(48)
                .saturating_add(usize::from(use_glyph_atlas) * 48),
            next_placement: 0,
            pagination,
        });
        Ok(())
    }

    fn paint_step(
        &mut self,
        imported: &ImportedDocx,
        state: PaintState,
        max_placements: usize,
    ) -> Result<(), PreviewError> {
        let PaintState {
            pagination,
            mut commands,
            use_glyph_atlas,
            mut atlas_indices,
            mut atlas_commands,
            mut atlas_elements,
            mut glyph_count,
            mut path_elements,
            mut estimated_atdl_bytes,
            mut next_placement,
        } = state;
        let fonts = build_fonts(&imported.font_bytes)?;
        let end = next_placement
            .saturating_add(max_placements)
            .min(pagination.placements.len());
        while next_placement < end {
            let placed = &pagination.placements[next_placement];
            let paragraph = &self.paragraphs[placed.paragraph_index];
            let line = &self.layouts[placed.paragraph_index].lines[placed.line_index];
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
                let mut attrs = self.character_defaults[placed.paragraph_index].clone();
                attrs.overlay(&paragraph.attrs_at(glyph.source.start));
                let color = attrs
                    .color
                    .map(|c| color(c.r, c.g, c.b))
                    .transpose()?
                    .unwrap_or(Color::new(0.0, 0.0, 0.0, 1.0).map_err(render_error)?);
                let source_char = paragraph.text[glyph.source.clone()].chars().next().ok_or(
                    PreviewError::Layout("glyph has an empty source range".into()),
                )?;
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
                let origin_y =
                    (MARGIN + glyph.y + placed.baseline - line.baseline) as f64 * ATDL_SCALE;
                if use_glyph_atlas {
                    let key = (face.id.get(), u32::from(glyph.glyph_id), glyph.size);
                    let atlas_index = if let Some(index) = atlas_indices.get(&key) {
                        *index
                    } else {
                        if atlas_indices.len() >= MAX_ATLAS_ENTRIES {
                            return Err(PreviewError::Limit("glyph atlas entries"));
                        }
                        let outline = face
                            .face
                            .outline(glyph.glyph_id)
                            .map_err(|e| PreviewError::Layout(e.to_string()))?;
                        let (count, bytes) = outline_size(&outline);
                        if count == 0 {
                            atlas_indices.insert(key, None);
                            None
                        } else {
                            let next = atlas_elements
                                .checked_add(count)
                                .ok_or(PreviewError::Limit("glyph atlas elements"))?;
                            if next > MAX_ATLAS_ELEMENTS {
                                return Err(PreviewError::Limit("glyph atlas elements"));
                            }
                            charge_estimate(&mut estimated_atdl_bytes, bytes, 22)?;
                            if atlas_commands.len() >= atelier_render::limits::MAX_COMMANDS {
                                return Err(PreviewError::Limit("commands"));
                            }
                            let elements = convert_outline(&outline, scale, 0.0, 0.0)?;
                            if elements.len() != count {
                                return Err(PreviewError::Render(
                                    "invalid outline coordinates".into(),
                                ));
                            }
                            let index = atlas_commands.len() as u32;
                            atlas_commands.push(Command::FillPath {
                                path: DisplayPath::new(elements).map_err(render_error)?,
                                color: Color::new(0.0, 0.0, 0.0, 1.0).map_err(render_error)?,
                                rule: FillRule::NonZero,
                            });
                            atlas_indices.insert(key, Some(index));
                            atlas_elements = next;
                            Some(index)
                        }
                    };
                    if let Some(index) = atlas_index {
                        if commands[placed.page_index].len() >= atelier_render::limits::MAX_COMMANDS
                        {
                            return Err(PreviewError::Limit("commands"));
                        }
                        charge_estimate(&mut estimated_atdl_bytes, 0, 65)?;
                        commands[placed.page_index].push(Command::DrawGlyphRun {
                            font_id: 0,
                            color,
                            glyphs: vec![atelier_render::PositionedGlyph {
                                glyph_id: index,
                                position: Point::try_new(origin_x, origin_y)
                                    .map_err(render_error)?,
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
                    path_elements[placed.page_index] = path_elements[placed.page_index]
                        .checked_add(glyph_path_elements)
                        .ok_or(PreviewError::Limit("path elements"))?;
                    if path_elements[placed.page_index] > atelier_render::limits::MAX_PATH_ELEMENTS
                    {
                        return Err(PreviewError::Limit("path elements"));
                    }
                    if glyph_path_elements > 0 {
                        charge_estimate(&mut estimated_atdl_bytes, glyph_path_bytes, 22)?;
                        if commands[placed.page_index].len() >= atelier_render::limits::MAX_COMMANDS
                        {
                            return Err(PreviewError::Limit("commands"));
                        }
                    }
                    let elements = convert_outline(&outline, scale, origin_x, origin_y)?;
                    if elements.len() != glyph_path_elements {
                        return Err(PreviewError::Render("invalid outline coordinates".into()));
                    }
                    if !elements.is_empty() {
                        commands[placed.page_index].push(Command::FillPath {
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
                    path_elements[placed.page_index] = path_elements[placed.page_index]
                        .checked_add(2)
                        .ok_or(PreviewError::Limit("path elements"))?;
                    if path_elements[placed.page_index] > atelier_render::limits::MAX_PATH_ELEMENTS
                    {
                        return Err(PreviewError::Limit("path elements"));
                    }
                    if commands[placed.page_index].len() >= atelier_render::limits::MAX_COMMANDS {
                        return Err(PreviewError::Limit("commands"));
                    }
                    charge_estimate(&mut estimated_atdl_bytes, 34, 39)?;
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
            next_placement += 1;
        }
        if next_placement < pagination.placements.len() {
            self.phase = Phase::Paint(PaintState {
                pagination,
                commands,
                use_glyph_atlas,
                atlas_indices,
                atlas_commands,
                atlas_elements,
                glyph_count,
                path_elements,
                estimated_atdl_bytes,
                next_placement,
            });
            return Ok(());
        }
        let glyph_atlas = if use_glyph_atlas {
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
            let bytes = DisplayList::new(page, atlas_commands)
                .map_err(render_error)?
                .encode()
                .map_err(render_error)?;
            Some(bytes)
        } else {
            None
        };
        let total = glyph_atlas.as_ref().map_or(0, Vec::len);
        if total > MAX_ATDL_BYTES {
            return Err(PreviewError::Limit("ATDL bytes"));
        }
        self.phase = Phase::Encode(EncodeState {
            glyph_atlas,
            pages: Vec::with_capacity(commands.len()),
            commands,
            total,
            next_page: 0,
            diagnostics: pagination.diagnostics.len(),
        });
        Ok(())
    }

    fn encode_step(
        &mut self,
        imported: &ImportedDocx,
        mut state: EncodeState,
        max_pages: usize,
    ) -> Result<Option<RichPreview>, PreviewError> {
        let end = state
            .next_page
            .saturating_add(max_pages)
            .min(state.commands.len());
        while state.next_page < end {
            let commands = std::mem::take(&mut state.commands[state.next_page]);
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
            state.total = state
                .total
                .checked_add(bytes.len())
                .ok_or(PreviewError::Limit("ATDL bytes"))?;
            if state.total > MAX_ATDL_BYTES {
                return Err(PreviewError::Limit("ATDL bytes"));
            }
            state.pages.push(bytes);
            state.next_page += 1;
        }
        if state.next_page < state.commands.len() {
            self.phase = Phase::Encode(state);
            return Ok(None);
        }
        Ok(Some(RichPreview {
            pages: state.pages,
            glyph_atlas: state.glyph_atlas,
            text: std::mem::take(&mut self.text),
            notice_count: imported
                .notices
                .len()
                .saturating_add(state.diagnostics)
                .saturating_add(
                    self.layouts
                        .iter()
                        .map(|layout| layout.unsupported.len())
                        .sum::<usize>(),
                )
                .min(u32::MAX as usize) as u32,
        }))
    }
}

/// Imports and renders a DOCX using the retained-model seams.
pub fn preview_docx(bytes: &[u8]) -> Result<RichPreview, PreviewError> {
    let mut imported = import_preview_docx(bytes)?;
    render_imported_docx(&mut imported)
}

fn validate_retained_text(imported: &ImportedDocx) -> Result<(), PreviewError> {
    let ids = imported.paragraph_ids();
    if ids.len() > MAX_PARAGRAPHS {
        return Err(PreviewError::Limit("paragraphs"));
    }
    let total = ids
        .iter()
        .enumerate()
        .try_fold(0usize, |total, (index, id)| {
            let text_len = imported
                .document
                .paragraph(*id)
                .map_err(|e| PreviewError::Import(e.to_string()))?
                .text
                .len();
            total
                .checked_add(text_len)
                .and_then(|n| n.checked_add(usize::from(index > 0)))
                .ok_or(PreviewError::Limit("text"))
        })?;
    if total > MAX_TEXT_BYTES {
        return Err(PreviewError::Limit("text"));
    }
    Ok(())
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
