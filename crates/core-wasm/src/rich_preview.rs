// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded DOCX import and paginated A4 display-list generation.

use atelier_fmt_ooxml::{DocxLimits, ImportedDocx, import_docx_with_limits};
use atelier_font::manager::{FontManager, FontManagerLimits, FontSource};
use atelier_render::Command;
use atelier_text_layout::{
    LayoutOptions, ParagraphLayout, layout_paragraph,
    pagination::{self, PaginationOptions, ParagraphRules},
};
use atelier_text_model::rich::{CharAttrs, NodeId, Paragraph};
use core::fmt;

use crate::rich_layout::rich_paint::{
    MAX_ATDL_BYTES, MAX_GLYPHS, PaintSink, PlacedLineToPaint, encode_page, paint_line,
};

const MAX_DOCX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_PARAGRAPHS: usize = 1024;
const MAX_PAGES: usize = 256;
const ATLAS_TEXT_THRESHOLD: usize = 65_536;
const MAX_EMBEDDED_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_WORK: usize = 8_000_000;
const MIN_PARAGRAPH_WORK: usize = 1_024;
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
    sink: PaintSink,
    path_elements: Vec<usize>,
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

pub(crate) fn build_fonts(embedded_fonts: &[Vec<u8>]) -> Result<FontManager<'_>, PreviewError> {
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
            sink: PaintSink::new(
                use_glyph_atlas,
                page_count
                    .saturating_mul(48)
                    .saturating_add(usize::from(use_glyph_atlas) * 48),
            ),
            path_elements: vec![0usize; page_count],
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
            mut sink,
            mut path_elements,
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
            paint_line(
                &fonts,
                &mut sink,
                &mut commands[placed.page_index],
                &mut path_elements[placed.page_index],
                PlacedLineToPaint {
                    paragraph,
                    defaults: &self.character_defaults[placed.paragraph_index],
                    line,
                    baseline: placed.baseline,
                    origin_x: MARGIN,
                    origin_y: MARGIN,
                },
            )?;
            next_placement += 1;
        }
        if next_placement < pagination.placements.len() {
            self.phase = Phase::Paint(PaintState {
                pagination,
                commands,
                sink,
                path_elements,
                next_placement,
            });
            return Ok(());
        }
        let glyph_atlas = if sink.use_glyph_atlas {
            Some(encode_page(sink.atlas_commands, PAGE_WIDTH, PAGE_HEIGHT)?)
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
            let bytes = encode_page(commands, PAGE_WIDTH, PAGE_HEIGHT)?;
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
fn twips(v: i64) -> Result<i64, PreviewError> {
    v.checked_mul(16)
        .map(|n| n / 5)
        .ok_or(PreviewError::Limit("geometry"))
}
