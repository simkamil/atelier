// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`PageCache`]: the incremental layout and page cache of the rich editor
//! (US-216), the implementation of [`RichPageCache`].
//!
//! Layout of the first lot (decisions of the integrator, documented in
//! `docs/us-216-incremental-layout.md`): table cell paragraphs are laid out as
//! full-width body paragraphs, headers and footers are not drawn, one page
//! setup (the first section's) applies to the whole document, inline items
//! (objects, breaks, symbols) take no room, list labels are not drawn.
//!
//! Work per edit: the changed paragraphs are shaped again, the pagination
//! restarts on the page that holds them and stops on the first page that
//! stabilises ([`atelier_text_layout::stream::PageStream`]); when the flow
//! moves (a line was added) it gives up after two pages and [`step`] paginates
//! the rest in slices of paragraphs. Pages are painted on demand and memoised
//! by page signature.
//!
//! [`step`]: RichPageCache::step

use std::collections::BTreeMap;

use atelier_font::manager::{FontManager, FontRequest, FontStyle};
use atelier_text_layout::caret::{
    CaretAffinity, CaretLayout, CaretOptions, PositionedLine, TextPosition, VerticalDirection,
};
use atelier_text_layout::pagination::{PaginationOptions, ParagraphRules};
use atelier_text_layout::stream::{
    PageStream, RunLimit, StreamParagraph, StreamStats, StreamUpdate,
};
use atelier_text_layout::{LayoutError as ShapeError, LayoutOptions, layout_paragraph};
use atelier_text_model::rich::flat::{
    EditOutcome, LayoutContext, LayoutScope, OBJECT_LEN, PageSetup,
};
use atelier_text_model::rich::{CharAttrs, CharRun, Paragraph, ResolvedProperties};

use super::prepare::{Mark, Prepared, page_break_lines, prepare};
use super::rich_paint::{FastLine, GlyphCache, PageWriter, StyleTable, paint_line_fast};
use super::{CaretGeometry, LayoutError, PageDelta, RichPageCache};
use crate::rich_preview::build_fonts;

/// Paragraphs shaped eagerly by one edit; a larger paste is finished by `step`.
const EAGER_EDIT_PARAGRAPHS: usize = 256;
/// Pages repaginated by an edit that does not stabilise.
const EDIT_PAGES: usize = 2;
/// Complete pages laid out by `new` before it returns.
const INITIAL_PAGES: usize = 2;
/// Paragraphs shaped per chunk while `new` looks for its first pages.
const INITIAL_CHUNK: usize = 32;
/// Painted pages kept (their display lists can be large).
const DISPLAY_MEMO_PAGES: usize = 4;
const MAX_PAGES: usize = 100_000;
/// Margins of the TXT profile (54 pt), twips.
const TXT_MARGIN_TWIPS: u32 = 1_080;
/// Carlito 11 pt, in half-points.
const TXT_SIZE_HALF_POINTS: u32 = 22;

const CARET_OPTIONS: CaretOptions = CaretOptions {
    max_lines: 1 << 20,
    max_stops: 1 << 24,
    max_work: usize::MAX / 4,
};

/// A selection rectangle on one page, points as in the display list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy)]
struct Geometry {
    page_width: i64,
    page_height: i64,
    left: i64,
    top: i64,
    content_width: i64,
    content_height: i64,
}

/// What the cache keeps of a paragraph besides its layout.
#[derive(Debug, Clone)]
struct ParaInfo {
    /// Bytes of the paragraph in the flat text.
    flat_len: usize,
    /// Inline items: place in the flat text and in the laid-out text.
    marks: Vec<Mark>,
    /// The paragraph could not be laid out and stands for an empty line.
    degraded: bool,
    /// The text and runs that were laid out, for painting and caret queries.
    painted: Paragraph,
    /// Colours and underlines of its runs, resolved once.
    styles: StyleTable,
}

impl ParaInfo {
    fn text_len(&self) -> usize {
        self.painted.text.len()
    }

    /// Offset in the laid-out text for an offset in the paragraph's flat text;
    /// `None` inside an item. Before and after an item are different offsets
    /// when the item has text (a break, a symbol).
    fn flat_to_text(&self, flat: usize) -> Option<usize> {
        if self.degraded {
            return Some(0);
        }
        let (mut flat_end, mut layout_end) = (0usize, 0usize);
        for mark in &self.marks {
            if flat < mark.flat {
                break;
            }
            if flat < mark.flat + OBJECT_LEN {
                return (flat == mark.flat).then_some(mark.layout);
            }
            flat_end = mark.flat + OBJECT_LEN;
            layout_end = mark.layout + mark.len;
        }
        let t = layout_end + (flat - flat_end);
        (t <= self.text_len() && self.painted.text.is_char_boundary(t)).then_some(t)
    }

    /// Offset in the flat text for a laid-out text offset: before the items
    /// that start there, except at the paragraph end where the caret stands
    /// after them.
    fn text_to_flat(&self, offset: usize) -> usize {
        if self.degraded {
            return 0;
        }
        let after_all = offset >= self.text_len();
        let (mut flat_end, mut layout_end) = (0usize, 0usize);
        for mark in &self.marks {
            if mark.layout < offset || after_all {
                flat_end = mark.flat + OBJECT_LEN;
                layout_end = mark.layout + mark.len;
            } else {
                break;
            }
        }
        flat_end + offset.saturating_sub(layout_end)
    }
}

/// Twips to 1/64 pt, to the nearest (the rule of the line layout).
fn twips(value: i64) -> i64 {
    let n = value * 16;
    let (q, r) = (n / 5, n % 5);
    if r.abs() * 2 >= 5 { q + n.signum() } else { q }
}

fn stable_style_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

fn geometry_of(document: &dyn LayoutContext, txt: bool) -> Geometry {
    let setup = if txt {
        PageSetup {
            margin_top: TXT_MARGIN_TWIPS,
            margin_right: TXT_MARGIN_TWIPS,
            margin_bottom: TXT_MARGIN_TWIPS,
            margin_left: TXT_MARGIN_TWIPS,
            gutter: 0,
            ..PageSetup::standard()
        }
    } else {
        document
            .section_of(0)
            .and_then(|(section, _)| document.page_setup(section))
            .unwrap_or_else(|_| PageSetup::standard())
    };
    let t = |v: u32| twips(i64::from(v));
    let left = t(setup.margin_left) + t(setup.gutter);
    Geometry {
        page_width: t(setup.width),
        page_height: t(setup.height),
        left,
        top: t(setup.margin_top),
        content_width: t(setup.width) - left - t(setup.margin_right),
        content_height: t(setup.height) - t(setup.margin_top) - t(setup.margin_bottom),
    }
}

fn txt_defaults() -> ResolvedProperties {
    ResolvedProperties {
        character: CharAttrs {
            font: Some("Carlito".into()),
            size_half_points: Some(TXT_SIZE_HALF_POINTS),
            ..CharAttrs::default()
        },
        paragraph: Default::default(),
    }
}

/// Replaces each character no registered font covers by as many `?` as it has
/// bytes, so that every byte offset keeps its meaning.
fn sanitize(text: &str, fonts: &FontManager<'_>) -> String {
    let request = FontRequest::new(["Carlito"], 400, FontStyle::Normal);
    text.chars()
        .map(|c| {
            if c.is_ascii() || fonts.resolve_for_char(&request, c).is_ok() {
                c.to_string()
            } else {
                "?".repeat(c.len_utf8())
            }
        })
        .collect()
}

fn rules_of(defaults: &ResolvedProperties, paragraph: &Paragraph) -> ParagraphRules {
    let mut props = defaults.paragraph.clone();
    props.overlay(&paragraph.props);
    ParagraphRules {
        style_id: props.style.as_deref().map(stable_style_id).unwrap_or(0),
        space_before: twips(i64::from(props.space_before.unwrap_or(0))),
        space_after: twips(i64::from(props.space_after.unwrap_or(0))),
        ..ParagraphRules::default()
    }
}

fn merge_equal_runs(runs: &mut Vec<CharRun>) {
    let mut merged: Vec<CharRun> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        match merged.last_mut() {
            Some(last) if last.end == run.start && last.attrs == run.attrs => last.end = run.end,
            _ => merged.push(run),
        }
    }
    *runs = merged;
}

/// Incremental layout and per-page paint cache of one document.
pub struct PageCache {
    txt: bool,
    geometry: Geometry,
    fonts: FontManager<'static>,
    stream: PageStream,
    infos: Vec<ParaInfo>,
    /// Flat start of each laid paragraph.
    starts: Vec<usize>,
    total_paragraphs: usize,
    doc_len: usize,
    pages_hint: u32,
    failed: bool,
    glyphs: GlyphCache,
    memo: BTreeMap<u32, (u64, Vec<u8>)>,
    memo_order: Vec<u32>,
    last_step_work: usize,
    last_stats: StreamStats,
}

impl PageCache {
    fn options(geometry: &Geometry) -> PaginationOptions {
        PaginationOptions {
            page_height: geometry.content_height,
            max_pages: MAX_PAGES,
            max_work: usize::MAX / 2,
        }
    }

    /// Shapes paragraph `index`, falling back to degraded layouts rather than
    /// failing: unknown characters become `?`, an unlayable paragraph becomes
    /// an empty line.
    fn lay_out_one(
        &self,
        document: &dyn LayoutContext,
        index: usize,
    ) -> Result<(StreamParagraph, ParaInfo), ShapeError> {
        let source = document.paragraph(index);
        let (resolved_source, defaults) = if self.txt {
            (source.cloned(), txt_defaults())
        } else {
            let defaults = document.resolved(index).unwrap_or_default();
            let mut owned = source.cloned();
            if let Some(p) = owned.as_mut() {
                if p.runs.iter().any(|r| r.attrs.char_style.is_some()) {
                    for run in &mut p.runs {
                        if run.attrs.char_style.is_some() {
                            if let Ok(resolved) = document.resolved_run(index, &run.attrs) {
                                run.attrs = resolved;
                            }
                        }
                    }
                    merge_equal_runs(&mut p.runs);
                }
            }
            (owned, defaults)
        };
        let (mut prepared, flat_len) = match &resolved_source {
            Some(p) => {
                let items = p.anchored.len() + p.breaks.len() + p.symbols.len();
                (
                    prepare(p, &self.fonts, !self.txt),
                    p.text.len() + OBJECT_LEN * items,
                )
            }
            None => (
                Prepared {
                    paragraph: Paragraph::default(),
                    marks: vec![Mark {
                        flat: 0,
                        layout: 0,
                        len: 0,
                    }],
                    page_breaks: Vec::new(),
                },
                OBJECT_LEN,
            ),
        };
        if self.txt {
            prepared.paragraph.props = Default::default();
        }
        let mut paragraph = std::mem::take(&mut prepared.paragraph);
        let had_text = !paragraph.text.is_empty();
        let options = LayoutOptions::new(self.geometry.content_width);
        let layout = match layout_paragraph(&paragraph, &defaults, &self.fonts, options) {
            Ok(layout) => layout,
            Err(_) => {
                paragraph.text = sanitize(&paragraph.text, &self.fonts);
                match layout_paragraph(&paragraph, &defaults, &self.fonts, options) {
                    Ok(layout) => layout,
                    Err(_) => {
                        paragraph.text.clear();
                        paragraph.runs.clear();
                        layout_paragraph(&paragraph, &defaults, &self.fonts, options)?
                    }
                }
            }
        };
        let degraded = had_text && paragraph.text.is_empty();
        let mut rules = rules_of(&defaults, &paragraph);
        if !degraded {
            rules.line_breaks_before = page_break_lines(&layout, &prepared.page_breaks);
        }
        let styles = StyleTable::build(&defaults.character, &paragraph)
            .map_err(|_| ShapeError::InvalidParagraph("character attributes"))?;
        let info = ParaInfo {
            flat_len,
            marks: if degraded { Vec::new() } else { prepared.marks },
            degraded,
            painted: Paragraph {
                text: paragraph.text,
                runs: paragraph.runs,
                ..Paragraph::default()
            },
            styles,
        };
        Ok((StreamParagraph { layout, rules }, info))
    }

    fn lay_out_range(
        &self,
        document: &dyn LayoutContext,
        from: usize,
        count: usize,
    ) -> Result<(Vec<StreamParagraph>, Vec<ParaInfo>), ShapeError> {
        let mut layouts = Vec::with_capacity(count);
        let mut infos = Vec::with_capacity(count);
        for index in from..from + count {
            let (layout, info) = self.lay_out_one(document, index)?;
            layouts.push(layout);
            infos.push(info);
        }
        Ok((layouts, infos))
    }

    fn reindex_starts(&mut self, from: usize) {
        self.starts.truncate(from);
        let mut next = match from.checked_sub(1) {
            None => 0,
            Some(previous) => match (self.starts.get(previous), self.infos.get(previous)) {
                (Some(start), Some(info)) => start.saturating_add(info.flat_len).saturating_add(1),
                _ => 0,
            },
        };
        for info in self.infos.get(from..).unwrap_or(&[]) {
            self.starts.push(next);
            next = next.saturating_add(info.flat_len).saturating_add(1);
        }
    }

    /// Replaces `removed` laid paragraphs at `first` by freshly shaped ones
    /// (`count` paragraphs of `document` from `first`).
    fn splice(
        &mut self,
        document: &dyn LayoutContext,
        first: usize,
        removed: usize,
        count: usize,
        limit: RunLimit,
    ) -> Result<StreamUpdate, ShapeError> {
        let (layouts, infos) = self.lay_out_range(document, first, count)?;
        let update = self
            .stream
            .replace(first, removed, layouts, limit)
            .map_err(|_| ShapeError::InvalidParagraph("pagination failed"))?;
        self.infos.splice(first..first + removed, infos);
        self.reindex_starts(first);
        Ok(update)
    }

    fn give_up(&mut self) {
        self.failed = true;
        self.stream.clear(Self::options(&self.geometry));
        self.infos.clear();
        self.starts.clear();
        self.memo.clear();
        self.memo_order.clear();
    }

    fn pages_ready(&self) -> usize {
        self.stream.page_count()
    }

    fn refresh_hint(&mut self) {
        let ready = self.pages_ready() as u32;
        self.pages_hint = if self.layout_complete() {
            ready
        } else {
            self.pages_hint.max(ready)
        };
    }

    fn delta_of(&mut self, update: Option<&StreamUpdate>) -> PageDelta {
        self.refresh_hint();
        let mut delta = PageDelta {
            page_count: self.page_count(),
            layout_complete: self.layout_complete(),
            ..PageDelta::default()
        };
        if let Some(update) = update {
            self.last_stats = update.stats.clone();
            delta.changed = update.changed_pages.iter().map(|p| *p as u32).collect();
            if update.page_count < update.old_page_count {
                delta
                    .invalidated
                    .push((update.page_count as u32, update.old_page_count as u32));
            }
        }
        delta
    }

    /// First pages of a document: shapes chunks until `INITIAL_PAGES` complete
    /// pages exist or the document ends.
    fn lay_out_first_pages(&mut self, document: &dyn LayoutContext) -> Result<(), ShapeError> {
        while self.infos.len() < self.total_paragraphs
            && self.stream.page_count() < INITIAL_PAGES + 1
        {
            let first = self.infos.len();
            let count = INITIAL_CHUNK.min(self.total_paragraphs - first);
            self.splice(document, first, 0, count, RunLimit::Pages(usize::MAX))?;
        }
        Ok(())
    }

    fn rebuild(&mut self, document: &dyn LayoutContext) -> Option<StreamUpdate> {
        let old_pages = self.stream.page_count();
        self.geometry = geometry_of(document, self.txt);
        self.stream.clear(Self::options(&self.geometry));
        self.infos.clear();
        self.starts.clear();
        self.memo.clear();
        self.memo_order.clear();
        self.failed = false;
        self.total_paragraphs = document.paragraph_count();
        self.doc_len = document.len();
        if self.lay_out_first_pages(document).is_err() {
            self.give_up();
            return None;
        }
        Some(StreamUpdate {
            old_page_count: old_pages,
            page_count: self.stream.page_count(),
            changed_pages: (0..self.stream.page_count()).collect(),
            ..StreamUpdate::default()
        })
    }

    /// Paragraphs shaped plus paragraphs paginated by the last `step`, for
    /// tests and benches of the budget.
    pub fn last_step_work(&self) -> usize {
        self.last_step_work
    }

    /// Paragraphs shaped so far (the rest is laid out by `step`).
    pub fn laid_out_paragraphs(&self) -> usize {
        self.infos.len()
    }

    /// The pagination of the paragraphs laid out so far.
    pub fn pagination(&self) -> &atelier_text_layout::pagination::Pagination {
        self.stream.pagination()
    }

    /// Counters of the last pagination run (pages visited, early stop), for
    /// the bench.
    pub fn last_stats(&self) -> &StreamStats {
        &self.last_stats
    }

    /// Paragraphs covered by the pagination.
    pub fn paginated_paragraphs(&self) -> usize {
        self.stream.paginated_paragraphs()
    }

    // ---- positions ----

    /// Paragraph and offset in the paragraph's flat text.
    fn locate(&self, position: usize) -> Result<(usize, usize), LayoutError> {
        if position > self.doc_len {
            return Err(LayoutError::PositionOutOfRange);
        }
        if self.infos.is_empty() {
            return Err(LayoutError::NotReady);
        }
        let paragraph = self
            .starts
            .partition_point(|s| *s <= position)
            .checked_sub(1)
            .ok_or(LayoutError::NotReady)?;
        let start = *self.starts.get(paragraph).ok_or(LayoutError::NotReady)?;
        let offset = position.checked_sub(start).ok_or(LayoutError::NotReady)?;
        let info = self.infos.get(paragraph).ok_or(LayoutError::NotReady)?;
        if offset > info.flat_len || paragraph >= self.stream.paginated_paragraphs() {
            return Err(LayoutError::NotReady);
        }
        Ok((paragraph, offset))
    }

    fn text_position(&self, position: usize) -> Result<TextPosition, LayoutError> {
        let (paragraph, offset) = self.locate(position)?;
        let byte_offset = self
            .infos
            .get(paragraph)
            .and_then(|info| info.flat_to_text(offset))
            .ok_or(LayoutError::PositionOutOfRange)?;
        Ok(TextPosition {
            paragraph_index: paragraph,
            byte_offset,
        })
    }

    fn flat_position(&self, position: TextPosition) -> usize {
        let at = position.paragraph_index;
        self.starts.get(at).copied().unwrap_or(0).saturating_add(
            self.infos
                .get(at)
                .map_or(0, |i| i.text_to_flat(position.byte_offset)),
        )
    }

    /// Caret geometry over whole paragraphs `first..=last`.
    fn caret_window(&self, first: usize, last: usize) -> Result<CaretLayout, LayoutError> {
        let mut lines = Vec::new();
        for paragraph in first..=last {
            let info = self.infos.get(paragraph).ok_or(LayoutError::NotReady)?;
            let layout = self.stream.layout(paragraph).ok_or(LayoutError::NotReady)?;
            let placed = self.stream.placements_of(paragraph);
            if placed.len() != layout.lines.len() {
                return Err(LayoutError::NotReady);
            }
            for (placement, line) in placed.iter().zip(&layout.lines) {
                lines.push(PositionedLine {
                    page_index: placement.page_index,
                    paragraph_index: paragraph,
                    text: &info.painted.text,
                    layout: line,
                    top: placement.top + self.geometry.top,
                    x_translation: self.geometry.left,
                });
            }
        }
        CaretLayout::new(&lines, CARET_OPTIONS).map_err(|_| LayoutError::NotReady)
    }

    fn paginated_last(&self) -> Option<usize> {
        self.stream.paginated_paragraphs().checked_sub(1)
    }

    fn units(points: f64) -> i64 {
        (points * 64.0).round() as i64
    }

    /// Selection rectangles of `start..end` on one page.
    pub fn selection_rects(
        &self,
        page: u32,
        start: usize,
        end: usize,
    ) -> Result<Vec<SelectionRect>, LayoutError> {
        if page >= self.page_count() {
            return Err(LayoutError::PageOutOfRange);
        }
        let placements = self.stream.page_placements(page as usize);
        let (Some(first), Some(last)) = (placements.first(), placements.last()) else {
            return Err(LayoutError::NotReady);
        };
        let (start, end) = (start.min(end), start.max(end));
        let a = self.text_position(start)?;
        let b = self.text_position(end)?;
        let (w0, w1) = (first.paragraph_index, last.paragraph_index);
        let window_start = TextPosition {
            paragraph_index: w0,
            byte_offset: 0,
        };
        let window_end = TextPosition {
            paragraph_index: w1,
            byte_offset: self.infos.get(w1).ok_or(LayoutError::NotReady)?.text_len(),
        };
        let (a, b) = (a.max(window_start), b.min(window_end));
        if a >= b {
            return Ok(Vec::new());
        }
        let layout = self.caret_window(w0, w1)?;
        let rects = layout
            .selection_rects(a..b)
            .map_err(|_| LayoutError::PositionOutOfRange)?;
        Ok(rects
            .into_iter()
            .filter(|r| r.page_index == page as usize)
            .map(|r| SelectionRect {
                page,
                x: r.x as f64 / 64.0,
                y: r.y as f64 / 64.0,
                width: r.width as f64 / 64.0,
                height: r.height as f64 / 64.0,
            })
            .collect())
    }

    /// Highlight rectangles of several flat ranges on one page (find and
    /// replace): the caret window of the page is built once. Returns
    /// `(index in ranges, rectangle)` in range order and stops after `limit`
    /// rectangles. A range that is not laid out yet, or that does not touch
    /// the page, gives nothing.
    pub fn range_rects(
        &self,
        page: u32,
        ranges: &[(usize, usize)],
        limit: usize,
    ) -> Result<Vec<(usize, SelectionRect)>, LayoutError> {
        if page >= self.page_count() {
            return Err(LayoutError::PageOutOfRange);
        }
        let placements = self.stream.page_placements(page as usize);
        let (Some(first), Some(last)) = (placements.first(), placements.last()) else {
            return Err(LayoutError::NotReady);
        };
        let (w0, w1) = (first.paragraph_index, last.paragraph_index);
        let window_start = TextPosition {
            paragraph_index: w0,
            byte_offset: 0,
        };
        let window_end = TextPosition {
            paragraph_index: w1,
            byte_offset: self.infos.get(w1).ok_or(LayoutError::NotReady)?.text_len(),
        };
        let layout = self.caret_window(w0, w1)?;
        let mut out = Vec::new();
        for (index, &(start, end)) in ranges.iter().enumerate() {
            let (start, end) = (start.min(end), start.max(end));
            let (Ok(a), Ok(b)) = (self.text_position(start), self.text_position(end)) else {
                continue;
            };
            let (a, b) = (a.max(window_start), b.min(window_end));
            if a >= b {
                continue;
            }
            let Ok(rects) = layout.selection_rects(a..b) else {
                continue;
            };
            for r in rects.into_iter().filter(|r| r.page_index == page as usize) {
                if out.len() >= limit {
                    return Ok(out);
                }
                out.push((
                    index,
                    SelectionRect {
                        page,
                        x: r.x as f64 / 64.0,
                        y: r.y as f64 / 64.0,
                        width: r.width as f64 / 64.0,
                        height: r.height as f64 / 64.0,
                    },
                ));
            }
        }
        Ok(out)
    }

    /// Lines laid out so far (all of them once the layout is complete).
    pub fn line_count(&self) -> usize {
        (0..self.stream.paginated_paragraphs())
            .map(|p| self.stream.layout(p).map_or(0, |l| l.lines.len()))
            .sum()
    }

    /// Lines and pages that the flat range `start..end` touches: a line
    /// counts when the range holds one of its characters or the paragraph
    /// break that ends it.
    pub fn lines_in_range(&self, start: usize, end: usize) -> Result<(usize, usize), LayoutError> {
        let (start, end) = (start.min(end), start.max(end));
        let a = self.text_position(start)?;
        let b = self.text_position(end)?;
        let mut pages = std::collections::BTreeSet::new();
        let mut lines = 0usize;
        for paragraph in a.paragraph_index..=b.paragraph_index {
            let layout = self.stream.layout(paragraph).ok_or(LayoutError::NotReady)?;
            let placed = self.stream.placements_of(paragraph);
            if placed.len() != layout.lines.len() {
                return Err(LayoutError::NotReady);
            }
            let text_len = self
                .infos
                .get(paragraph)
                .ok_or(LayoutError::NotReady)?
                .text_len();
            let from = if paragraph == a.paragraph_index {
                a.byte_offset
            } else {
                0
            };
            let to = if paragraph == b.paragraph_index {
                b.byte_offset
            } else {
                text_len
            };
            let separator = paragraph < b.paragraph_index;
            for (line, placement) in layout.lines.iter().zip(placed) {
                let content = line.source.start < to && line.source.end > from;
                let ends = separator && line.final_line && line.source.end >= from;
                if content || ends {
                    lines += 1;
                    pages.insert(placement.page_index);
                }
            }
        }
        Ok((lines, pages.len()))
    }

    /// Keeps a copy of a painted page; the buffer of the page it evicts is
    /// reused, so that the copy is a plain memcpy into warm memory.
    fn memoise(&mut self, index: u32, signature: u64, bytes: &[u8]) {
        let mut buffer = Vec::new();
        if let Some((_, old)) = self.memo.remove(&index) {
            buffer = old;
        }
        self.memo_order.retain(|p| *p != index);
        while self.memo_order.len() >= DISPLAY_MEMO_PAGES {
            let oldest = self.memo_order.remove(0);
            if let Some((_, old)) = self.memo.remove(&oldest) {
                if old.capacity() > buffer.capacity() {
                    buffer = old;
                }
            }
        }
        buffer.clear();
        buffer.extend_from_slice(bytes);
        self.memo_order.push(index);
        self.memo.insert(index, (signature, buffer));
    }

    /// Atlas entries no reply carried yet, with the index of the first;
    /// marks them sent. Pages painted by `page_display` draw glyphs by atlas
    /// index (`DrawGlyphRun`, font 0), so the entries of every page of a
    /// reply must be taken after painting them.
    pub fn take_atlas(&mut self) -> (u32, Vec<Vec<u8>>) {
        self.glyphs.take_atlas()
    }

    /// Every atlas entry of the session, sent or not (host tests expand pages
    /// with it).
    pub fn atlas(&self) -> &[Vec<u8>] {
        self.glyphs.atlas()
    }

    /// Starts a new atlas: entries and painted pages (which refer to the old
    /// indices) are dropped, and the next entry is number 0.
    pub fn reset_atlas(&mut self) {
        self.glyphs.reset_atlas();
        self.memo.clear();
        self.memo_order.clear();
    }

    fn paint_page(&mut self, index: usize) -> Result<Vec<u8>, LayoutError> {
        let Self {
            fonts,
            glyphs,
            stream,
            infos,
            geometry,
            ..
        } = self;
        let mut writer = PageWriter::new(geometry.page_width, geometry.page_height)
            .map_err(|_| LayoutError::NotReady)?;
        for placed in stream.page_placements(index) {
            let info = infos
                .get(placed.paragraph_index)
                .ok_or(LayoutError::NotReady)?;
            let layout = stream
                .layout(placed.paragraph_index)
                .ok_or(LayoutError::NotReady)?;
            let line = layout
                .lines
                .get(placed.line_index)
                .ok_or(LayoutError::NotReady)?;
            paint_line_fast(
                fonts,
                glyphs,
                &mut writer,
                FastLine {
                    styles: &info.styles,
                    paragraph: &info.painted,
                    line,
                    baseline: placed.baseline,
                    margin: (geometry.left, geometry.top),
                },
            )
            .map_err(|_| LayoutError::NotReady)?;
        }
        writer.finish().map_err(|_| LayoutError::NotReady)
    }
}

impl RichPageCache for PageCache {
    fn new(document: &dyn LayoutContext, txt_profile: bool) -> Result<Self, LayoutError>
    where
        Self: Sized,
    {
        let geometry = geometry_of(document, txt_profile);
        let fonts = build_fonts(&[]).map_err(|_| LayoutError::NotReady)?;
        let mut cache = Self {
            txt: txt_profile,
            geometry,
            fonts,
            stream: PageStream::new(Self::options(&geometry)),
            infos: Vec::new(),
            starts: Vec::new(),
            total_paragraphs: document.paragraph_count(),
            doc_len: document.len(),
            pages_hint: 0,
            failed: false,
            glyphs: {
                let mut glyphs = GlyphCache::default();
                glyphs.set_page(geometry.page_width, geometry.page_height);
                glyphs
            },
            memo: BTreeMap::new(),
            memo_order: Vec::new(),
            last_step_work: 0,
            last_stats: StreamStats::default(),
        };
        cache
            .lay_out_first_pages(document)
            .map_err(|_| LayoutError::NotReady)?;
        cache.refresh_hint();
        Ok(cache)
    }

    fn page_count(&self) -> u32 {
        if self.layout_complete() {
            self.pages_ready() as u32
        } else {
            self.pages_hint.max(self.pages_ready() as u32)
        }
    }

    fn layout_complete(&self) -> bool {
        self.failed
            || (self.infos.len() == self.total_paragraphs
                && self.stream.paginated_paragraphs() == self.infos.len())
    }

    fn apply_edit(&mut self, document: &dyn LayoutContext, edit: &EditOutcome) -> PageDelta {
        if self.failed {
            return PageDelta::default();
        }
        let old_pages = self.stream.page_count();
        if edit.layout_scope == LayoutScope::All {
            let update = self.rebuild(document);
            self.pages_hint = self.pages_hint.max(old_pages as u32);
            let mut delta = self.delta_of(update.as_ref());
            if old_pages > 0 && !delta.invalidated.iter().any(|(a, _)| *a == 0) {
                delta.invalidated.push((0, old_pages as u32));
            }
            return delta;
        }
        self.total_paragraphs = document.paragraph_count();
        self.doc_len = document.len();
        let laid = self.infos.len();
        let first = edit.first_paragraph;
        if first >= laid {
            // The edit is beyond the shaped prefix: `step` will shape it.
            return self.delta_of(None);
        }
        let crosses_end = first + edit.removed_paragraphs > laid;
        let eager = edit.inserted_paragraphs.min(EAGER_EDIT_PARAGRAPHS);
        let keeps_suffix = !crosses_end && eager == edit.inserted_paragraphs;
        let removed = if keeps_suffix {
            edit.removed_paragraphs
        } else {
            laid - first
        };
        let count = eager.min(self.total_paragraphs.saturating_sub(first));
        match self.splice(document, first, removed, count, RunLimit::Pages(EDIT_PAGES)) {
            Ok(update) => self.delta_of(Some(&update)),
            Err(_) => {
                self.give_up();
                PageDelta {
                    layout_complete: true,
                    invalidated: vec![(0, old_pages as u32)],
                    ..PageDelta::default()
                }
            }
        }
    }

    fn step(&mut self, document: &dyn LayoutContext, budget_paragraphs: u32) -> PageDelta {
        self.last_step_work = 0;
        if self.failed || self.layout_complete() {
            return self.delta_of(None);
        }
        let mut budget = (budget_paragraphs as usize).max(1);
        let old_pages = self.stream.page_count();
        let mut changed: Vec<u32> = Vec::new();
        let mut merge = |update: &StreamUpdate| {
            changed.extend(update.changed_pages.iter().map(|p| *p as u32));
        };
        let mut last_update: Option<StreamUpdate> = None;
        // Paragraphs shaped but not yet paginated first.
        if self.stream.paginated_paragraphs() < self.infos.len() {
            let before = self.stream.paginated_paragraphs();
            match self.stream.advance(budget) {
                Ok(update) => {
                    let done = self.stream.paginated_paragraphs() - before;
                    self.last_step_work += done;
                    budget = budget.saturating_sub(done);
                    merge(&update);
                    last_update = Some(update);
                }
                Err(_) => {
                    self.give_up();
                    return PageDelta {
                        layout_complete: true,
                        invalidated: vec![(0, old_pages as u32)],
                        ..PageDelta::default()
                    };
                }
            }
        }
        if budget > 0
            && self.stream.paginated_paragraphs() == self.infos.len()
            && self.infos.len() < self.total_paragraphs
        {
            let first = self.infos.len();
            let count = budget.min(self.total_paragraphs - first);
            match self.splice(document, first, 0, count, RunLimit::Pages(usize::MAX)) {
                Ok(update) => {
                    self.last_step_work += count;
                    merge(&update);
                    last_update = Some(update);
                }
                Err(_) => {
                    self.give_up();
                    return PageDelta {
                        layout_complete: true,
                        invalidated: vec![(0, old_pages as u32)],
                        ..PageDelta::default()
                    };
                }
            }
        }
        changed.sort_unstable();
        changed.dedup();
        let mut delta = self.delta_of(last_update.as_ref());
        delta.changed = changed;
        delta.invalidated.clear();
        let ready = self.stream.page_count();
        if ready < old_pages {
            delta.invalidated.push((ready as u32, old_pages as u32));
        }
        delta
    }

    fn page_display(
        &mut self,
        _document: &dyn LayoutContext,
        index: u32,
    ) -> Result<Vec<u8>, LayoutError> {
        if index >= self.page_count() {
            return Err(LayoutError::PageOutOfRange);
        }
        let page = index as usize;
        let Some(signature) = self.stream.page_signature(page) else {
            return Err(LayoutError::NotReady);
        };
        if let Some((cached, bytes)) = self.memo.get(&index) {
            if *cached == signature {
                return Ok(bytes.clone());
            }
        }
        let bytes = self.paint_page(page)?;
        self.memoise(index, signature, &bytes);
        Ok(bytes)
    }

    fn caret(&self, position: usize, upstream: bool) -> Result<CaretGeometry, LayoutError> {
        let at = self.text_position(position)?;
        let window = self.caret_window(at.paragraph_index, at.paragraph_index)?;
        let rect = window
            .caret_rect(at, affinity(upstream))
            .map_err(|_| LayoutError::PositionOutOfRange)?;
        Ok(CaretGeometry {
            page: rect.page_index as u32,
            x: rect.x as f64 / 64.0,
            y: rect.y as f64 / 64.0,
            height: rect.height as f64 / 64.0,
        })
    }

    fn line_edges(&self, position: usize, upstream: bool) -> Result<(usize, usize), LayoutError> {
        let at = self.text_position(position)?;
        let window = self.caret_window(at.paragraph_index, at.paragraph_index)?;
        let rect = window
            .caret_rect(at, affinity(upstream))
            .map_err(|_| LayoutError::PositionOutOfRange)?;
        let layout = self
            .stream
            .layout(at.paragraph_index)
            .ok_or(LayoutError::NotReady)?;
        let line = layout
            .lines
            .get(rect.line_index)
            .ok_or(LayoutError::NotReady)?;
        let start = TextPosition {
            paragraph_index: at.paragraph_index,
            byte_offset: line.source.start,
        };
        let end = TextPosition {
            paragraph_index: at.paragraph_index,
            // A line that ends with a forced break ends before the break.
            byte_offset: if line.mandatory_break {
                line.source.end.saturating_sub(1)
            } else {
                line.source.end
            },
        };
        Ok((self.flat_position(start), self.flat_position(end)))
    }

    fn move_page(
        &self,
        position: usize,
        pages: i32,
        preferred_x: Option<f64>,
    ) -> Result<(usize, f64), LayoutError> {
        let at = self.text_position(position)?;
        let window = self.caret_window(at.paragraph_index, at.paragraph_index)?;
        let rect = window
            .caret_rect(at, CaretAffinity::Downstream)
            .map_err(|_| LayoutError::PositionOutOfRange)?;
        let x = preferred_x.unwrap_or(rect.x as f64 / 64.0);
        if pages == 0 {
            return Ok((position, x));
        }
        let last_page = self.page_count().saturating_sub(1) as i64;
        let wanted = rect.page_index as i64 + i64::from(pages);
        if wanted < 0 {
            return Ok((0, x));
        }
        if wanted > last_page {
            // Past the last page: the end of the document, once it is known.
            return if self.layout_complete() {
                Ok((self.doc_len, x))
            } else {
                Err(LayoutError::NotReady)
            };
        }
        let (target, upstream) = self.hit_test(
            wanted as u32,
            x,
            (rect.y as f64 + rect.height as f64 / 2.0) / 64.0,
        )?;
        let _ = upstream;
        Ok((target, x))
    }

    fn hit_test(&self, page: u32, x: f64, y: f64) -> Result<(usize, bool), LayoutError> {
        if page >= self.page_count() {
            return Err(LayoutError::PageOutOfRange);
        }
        let placements = self.stream.page_placements(page as usize);
        let (Some(first), Some(last)) = (placements.first(), placements.last()) else {
            return Err(LayoutError::NotReady);
        };
        let window = self.caret_window(first.paragraph_index, last.paragraph_index)?;
        let (xu, yu) = (Self::units(x), Self::units(y));
        let hit = window
            .hit_test(page as usize, xu, yu)
            .map_err(|_| LayoutError::NotReady)?;
        // The line the point fell on (nearest by vertical distance, earlier on
        // ties): the position has upstream affinity when it is the end of a
        // line that wraps softly.
        let mut best: Option<(i64, usize, usize)> = None;
        for placed in placements {
            let layout = self
                .stream
                .layout(placed.paragraph_index)
                .ok_or(LayoutError::NotReady)?;
            let line = layout
                .lines
                .get(placed.line_index)
                .ok_or(LayoutError::NotReady)?;
            let top = placed.top + self.geometry.top;
            let bottom = top + line.height;
            let distance = if yu < top {
                top - yu
            } else if yu > bottom {
                yu - bottom
            } else {
                0
            };
            if best.is_none_or(|(d, _, _)| distance < d) {
                best = Some((distance, placed.paragraph_index, placed.line_index));
            }
        }
        // Past the end of a soft-wrapped line: its end, upstream.
        let mut result = hit;
        let mut upstream = false;
        if let Some((_, paragraph, line_index)) = best {
            let line = self
                .stream
                .layout(paragraph)
                .and_then(|layout| layout.lines.get(line_index))
                .ok_or(LayoutError::NotReady)?;
            if !line.final_line && !line.mandatory_break {
                let end = TextPosition {
                    paragraph_index: paragraph,
                    byte_offset: line.source.end,
                };
                let end_rect = window
                    .caret_rect(end, CaretAffinity::Upstream)
                    .map_err(|_| LayoutError::NotReady)?;
                if xu >= end_rect.x || hit == end {
                    result = end;
                    upstream = true;
                }
            }
        }
        Ok((self.flat_position(result), upstream))
    }

    fn move_vertical(
        &self,
        position: usize,
        lines: i32,
        preferred_x: Option<f64>,
    ) -> Result<(usize, f64), LayoutError> {
        let mut at = self.text_position(position)?;
        let mut x = preferred_x.map(Self::units);
        let mut current_affinity = CaretAffinity::Downstream;
        let direction = if lines < 0 {
            VerticalDirection::Up
        } else {
            VerticalDirection::Down
        };
        for _ in 0..lines.unsigned_abs() {
            let first = at.paragraph_index.saturating_sub(1);
            let last = self
                .paginated_last()
                .ok_or(LayoutError::NotReady)?
                .min(at.paragraph_index + 1);
            let window = self.caret_window(first, last)?;
            match window.move_vertical_with_affinity(at, current_affinity, x, direction) {
                Ok((next, next_affinity, next_x)) => {
                    at = next;
                    current_affinity = next_affinity;
                    x = Some(next_x);
                }
                Err(atelier_text_layout::caret::CaretError::NoAdjacentLine) => {
                    // First or last line of what is laid out: Up goes to the
                    // start of the document, Down to its end once it is known.
                    let kept = x.unwrap_or(0) as f64 / 64.0;
                    return match direction {
                        VerticalDirection::Up if first == 0 => Ok((0, kept)),
                        VerticalDirection::Down if self.layout_complete() => {
                            Ok((self.doc_len, kept))
                        }
                        _ => Err(LayoutError::NotReady),
                    };
                }
                Err(_) => return Err(LayoutError::NotReady),
            }
        }
        let kept = match x {
            Some(x) => x,
            None => {
                let window = self.caret_window(at.paragraph_index, at.paragraph_index)?;
                window
                    .caret_rect(at, current_affinity)
                    .map_err(|_| LayoutError::PositionOutOfRange)?
                    .x
            }
        };
        Ok((self.flat_position(at), kept as f64 / 64.0))
    }
}

fn affinity(upstream: bool) -> CaretAffinity {
    if upstream {
        CaretAffinity::Upstream
    } else {
        CaretAffinity::Downstream
    }
}

// The model double exists on the host only.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::rich_layout::rich_paint::{PaintSink, PlacedLineToPaint, encode_page, paint_line};
    use atelier_render::Command;
    use atelier_text_model::rich::flat_double::{VecParagraphs, example_section};
    use atelier_text_model::rich::{Color, Underline};

    fn styled(text: &str, runs: &[(usize, usize, CharAttrs)]) -> Paragraph {
        let mut paragraph = Paragraph::with_text(text);
        for (start, end, attrs) in runs {
            paragraph.runs.push(CharRun {
                start: *start,
                end: *end,
                attrs: attrs.clone(),
            });
        }
        paragraph
    }

    /// The page as the DOCX preview painter draws it.
    fn slow_page(cache: &PageCache, document: &VecParagraphs, page: usize) -> Vec<u8> {
        let mut commands: Vec<Command> = Vec::new();
        let mut path_elements = 0usize;
        let mut sink = PaintSink::new(false, 0);
        for placed in cache.stream.page_placements(page) {
            let info = &cache.infos[placed.paragraph_index];
            let layout = cache.stream.layout(placed.paragraph_index).unwrap();
            let defaults = document.resolved(placed.paragraph_index).unwrap();
            paint_line(
                &cache.fonts,
                &mut sink,
                &mut commands,
                &mut path_elements,
                PlacedLineToPaint {
                    paragraph: &info.painted,
                    defaults: &defaults.character,
                    line: &layout.lines[placed.line_index],
                    baseline: placed.baseline,
                    origin_x: cache.geometry.left,
                    origin_y: cache.geometry.top,
                },
            )
            .unwrap();
        }
        encode_page(
            commands,
            cache.geometry.page_width,
            cache.geometry.page_height,
        )
        .unwrap()
    }

    /// What the editor drew before the atlas: every glyph of a
    /// `DrawGlyphRun` becomes the `FillPath` of its atlas contour translated
    /// to the glyph position (the sums are the ones the old writer did).
    fn expand(page: &[u8], atlas: &[Vec<u8>], width: i64, height: i64) -> Vec<u8> {
        use atelier_geom::Point;
        use atelier_render::{DisplayList, DisplayPath, FillRule, PathElement};
        let contour = |entry: &[u8]| match DisplayList::decode(entry).unwrap().commands.as_slice() {
            [Command::FillPath { path, rule, color }] => {
                assert_eq!(*rule, FillRule::NonZero);
                assert_eq!(
                    (color.red, color.green, color.blue, color.alpha),
                    (0.0, 0.0, 0.0, 1.0)
                );
                path.elements.clone()
            }
            other => panic!("an atlas entry is one FillPath, got {other:?}"),
        };
        let at = |p: Point, o: Point| Point::try_new(o.x() + p.x(), o.y() + p.y()).unwrap();
        let mut commands = Vec::new();
        for command in DisplayList::decode(page).unwrap().commands {
            match command {
                Command::DrawGlyphRun {
                    font_id,
                    color,
                    glyphs,
                } => {
                    assert_eq!(font_id, 0);
                    for glyph in glyphs {
                        let elements = contour(&atlas[glyph.glyph_id as usize])
                            .into_iter()
                            .map(|e| match e {
                                PathElement::MoveTo(p) => {
                                    PathElement::MoveTo(at(p, glyph.position))
                                }
                                PathElement::LineTo(p) => {
                                    PathElement::LineTo(at(p, glyph.position))
                                }
                                PathElement::QuadraticTo { control, to } => {
                                    PathElement::QuadraticTo {
                                        control: at(control, glyph.position),
                                        to: at(to, glyph.position),
                                    }
                                }
                                PathElement::CubicTo {
                                    control1,
                                    control2,
                                    to,
                                } => PathElement::CubicTo {
                                    control1: at(control1, glyph.position),
                                    control2: at(control2, glyph.position),
                                    to: at(to, glyph.position),
                                },
                                PathElement::Close => PathElement::Close,
                            })
                            .collect();
                        commands.push(Command::FillPath {
                            path: DisplayPath::new(elements).unwrap(),
                            color,
                            rule: FillRule::NonZero,
                        });
                    }
                }
                other => commands.push(other),
            }
        }
        encode_page(commands, width, height).unwrap()
    }

    /// Fourteen styled paragraphs and seven underline styles, a fallback
    /// family, a symbol and forced breaks, laid out completely.
    fn sample() -> (VecParagraphs, PageCache) {
        let filler = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let bold = CharAttrs {
            bold: Some(true),
            ..CharAttrs::default()
        };
        let underlined = CharAttrs {
            italic: Some(true),
            underline: Some(Underline::Single),
            color: Some(Color {
                r: 0x44,
                g: 0x72,
                b: 0xc4,
            }),
            ..CharAttrs::default()
        };
        let mut paragraphs = Vec::new();
        for i in 0..14 {
            paragraphs.push(styled(
                &format!("{filler}caf\u{e9}\ttab {i}"),
                &[(4, 30, bold.clone()), (60, 200, underlined.clone())],
            ));
        }
        // Every underline style, an explicit "no underline", a family no font
        // has (fallback face), a symbol and forced breaks.
        let mut owner = atelier_text_model::rich::RichDocument::new();
        for (i, style) in [
            Underline::None,
            Underline::Single,
            Underline::Double,
            Underline::Thick,
            Underline::Dotted,
            Underline::Dashed,
            Underline::Wave,
        ]
        .into_iter()
        .enumerate()
        {
            let attrs = CharAttrs {
                underline: Some(style),
                font: (i % 2 == 0).then(|| "No Such Family".to_owned()),
                color: (i % 3 == 0).then_some(Color {
                    r: 9,
                    g: 99,
                    b: 199,
                }),
                ..CharAttrs::default()
            };
            let mut p = styled(
                "underline styles and a missing family \u{e9}\u{2014}",
                &[(0, 9, attrs), (10, 16, bold.clone())],
            );
            p.symbols.push(atelier_text_model::rich::InlineSymbol {
                id: owner.alloc_id().unwrap(),
                offset: 9,
                font: "Symbol".into(),
                code: 0xf0b7,
            });
            p.breaks.push(atelier_text_model::rich::InlineBreak {
                id: owner.alloc_id().unwrap(),
                offset: 20,
                kind: atelier_text_model::rich::BreakKind::Line,
            });
            paragraphs.push(p);
        }
        let mut document = VecParagraphs::new(paragraphs);
        document.section = example_section();
        let mut cache = PageCache::new(&document, false).unwrap();
        while !cache.layout_complete() {
            cache.step(&document, 16);
        }
        assert!(cache.page_count() >= 3);
        (document, cache)
    }

    #[test]
    fn the_atlas_pages_expand_to_the_preview_painter_bytes() {
        let (document, mut cache) = sample();
        let (width, height) = (cache.geometry.page_width, cache.geometry.page_height);
        let pages: Vec<Vec<u8>> = (0..cache.page_count() as usize)
            .map(|page| cache.paint_page(page).unwrap())
            .collect();
        let (first, atlas) = cache.take_atlas();
        assert_eq!(first, 0);
        assert!(!atlas.is_empty() && atlas.len() < 400, "{}", atlas.len());
        for (page, bytes) in pages.iter().enumerate() {
            assert_eq!(
                expand(bytes, &atlas, width, height),
                slow_page(&cache, &document, page),
                "page {page}"
            );
        }
        // Nothing is new the second time; a reset starts again at 0 and
        // renumbers the entries in paint order.
        assert_eq!(cache.take_atlas().1.len(), 0);
        assert_eq!(cache.take_atlas().0 as usize, atlas.len());
        cache.reset_atlas();
        assert!(cache.memo.is_empty());
        let again = cache.paint_page(0).unwrap();
        let (first, entries) = cache.take_atlas();
        assert_eq!(first, 0);
        assert_eq!(
            expand(&again, &entries, width, height),
            slow_page(&cache, &document, 0)
        );
    }

    #[test]
    fn entries_are_sent_once_and_in_order() {
        let (_, mut cache) = sample();
        cache.paint_page(0).unwrap();
        let (first, a) = cache.take_atlas();
        cache.paint_page(1).unwrap();
        let (second, b) = cache.take_atlas();
        assert_eq!(first, 0);
        assert_eq!(second as usize, a.len());
        assert!(a.iter().all(|e| !b.contains(e)), "an entry is sent once");
    }

    #[test]
    fn a_full_atlas_falls_back_to_inline_contours() {
        let (document, mut cache) = sample();
        let (width, height) = (cache.geometry.page_width, cache.geometry.page_height);
        cache.glyphs.set_atlas_limit(5);
        let bytes = cache.paint_page(0).unwrap();
        assert_eq!(cache.atlas().len(), 5);
        let (_, atlas) = cache.take_atlas();
        let list = atelier_render::DisplayList::decode(&bytes).unwrap();
        assert!(
            list.commands
                .iter()
                .any(|c| matches!(c, Command::FillPath { .. }))
        );
        assert!(
            list.commands
                .iter()
                .any(|c| matches!(c, Command::DrawGlyphRun { .. }))
        );
        // Inline contours and atlas glyphs together still draw the preview page.
        assert_eq!(
            expand(&bytes, &atlas, width, height),
            slow_page(&cache, &document, 0)
        );
    }

    #[test]
    fn flat_and_text_offsets_agree_with_the_model_conversions() {
        use atelier_text_model::rich::flat::{
            flat_to_text_offset, paragraph_flat, text_to_flat_offset,
        };
        use atelier_text_model::rich::{AnchoredObject, RichDocument};
        let mut owner = RichDocument::new();
        let mut paragraph = Paragraph::with_text("a\u{e9}bcd");
        for offset in [1usize, 1, 3, 6] {
            paragraph.anchored.push(AnchoredObject {
                id: owner.alloc_id().unwrap(),
                offset,
                kind: "pic".into(),
                data: Vec::new(),
            });
        }
        let prepared = prepare(&paragraph, &FontManager::new(), true);
        let info = ParaInfo {
            flat_len: paragraph.text.len() + OBJECT_LEN * 4,
            marks: prepared.marks,
            degraded: false,
            painted: Paragraph::with_text(&paragraph.text),
            styles: StyleTable::build(&CharAttrs::default(), &Paragraph::default()).unwrap(),
        };
        assert_eq!(paragraph_flat(&paragraph).len(), info.flat_len);
        for flat in 0..=info.flat_len {
            assert_eq!(
                info.flat_to_text(flat),
                flat_to_text_offset(&paragraph, flat).ok(),
                "flat {flat}"
            );
        }
        for offset in [0usize, 1, 3, 6] {
            let after = offset == paragraph.text.len();
            assert_eq!(
                info.text_to_flat(offset),
                text_to_flat_offset(&paragraph, offset, after).unwrap(),
                "text {offset}"
            );
        }
    }
}
