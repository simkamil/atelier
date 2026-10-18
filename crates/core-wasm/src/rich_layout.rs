// SPDX-License-Identifier: AGPL-3.0-or-later
//! Page-cache contract of the rich editor layout (US-203, implemented by
//! US-216).
//!
//! Target: under 4 ms per keystroke on 300 pages, no global pagination
//! pre-validation (layout is incremental, the tail paginates in `step`).
//!
//! Declarations only. R2 implements [`RichPageCache`]; the editor (R3a) and
//! the protocol replies depend only on these signatures. The cache owns no
//! document: every call that needs paragraphs receives the document as a
//! `&dyn LayoutContext` (R1, test double
//! `VecParagraphs`), after the edit it describes has been applied.
//! Positions are the flat byte positions of `atelier_text_model::rich::flat`;
//! display lists are `ATDL` byte vectors (`docs/us-080-display-list.md`)
//! ready to be copied into a reply.

use atelier_text_model::rich::flat::{EditOutcome, LayoutContext};

/// Painting of laid-out pages (US-216). Declared here so that R2 owns the
/// file `rich_layout/rich_paint.rs` without touching `lib.rs`.
pub mod rich_paint;

mod cache;
pub use cache::{PageCache, SelectionRect};

/// Caret geometry in page coordinates (points, as in the display list).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretGeometry {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

/// Pages affected by one layout update. `changed` pages (sorted, unique) are
/// the ones whose display list differs and must be re-sent; `invalidated` are
/// half-open ranges whose cached lists the client must drop. The protocol
/// layer turns it into a reply with `rich_protocol::split_page_delta`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageDelta {
    pub page_count: u32,
    pub layout_complete: bool,
    pub changed: Vec<u32>,
    pub invalidated: Vec<(u32, u32)>,
}

/// Why a cache query failed. Protocol mapping (`rich_protocol`):
/// `PageOutOfRange` to `RICH_PAGE_RANGE`, `PositionOutOfRange` to
/// `RICH_POSITION_OUT_OF_RANGE`, `NotReady` to `RICH_NOT_READY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    PageOutOfRange,
    PositionOutOfRange,
    /// Layout of the requested area is not finished yet.
    NotReady,
}

/// Incremental layout and per-page paint cache of one document.
pub trait RichPageCache {
    /// Lays out the first pages of `document` for a profile (document or TXT)
    /// and leaves the rest to [`RichPageCache::step`].
    fn new(document: &dyn LayoutContext, txt_profile: bool) -> Result<Self, LayoutError>
    where
        Self: Sized;
    fn page_count(&self) -> u32;
    fn layout_complete(&self) -> bool;
    /// Re-lays out from the paragraph touched by `edit` (already applied to
    /// `document`), reusing every untouched paragraph, and reports the pages
    /// that changed.
    fn apply_edit(&mut self, document: &dyn LayoutContext, edit: &EditOutcome) -> PageDelta;
    /// Does at most `budget_paragraphs` paragraphs of pending layout (tail
    /// pagination) and reports what changed. The budget is a count of work,
    /// never a clock: `Instant::now()` panics on wasm32-unknown-unknown.
    fn step(&mut self, document: &dyn LayoutContext, budget_paragraphs: u32) -> PageDelta;
    /// `ATDL` display list of one page, painted lazily and memoised until the
    /// page changes.
    fn page_display(
        &mut self,
        document: &dyn LayoutContext,
        index: u32,
    ) -> Result<Vec<u8>, LayoutError>;
    /// Caret geometry at `position`. `upstream` picks, at a soft line wrap,
    /// the end of the previous line instead of the start of the next.
    fn caret(&self, position: usize, upstream: bool) -> Result<CaretGeometry, LayoutError>;
    /// Flat positions of the start and end of the visual line holding
    /// `position` (Home and End), with the same affinity rule.
    fn line_edges(&self, position: usize, upstream: bool) -> Result<(usize, usize), LayoutError>;
    /// Page Up and Page Down: moves `pages` pages (negative is up), keeping
    /// the preferred x, and returns the position and the x to keep.
    fn move_page(
        &self,
        position: usize,
        pages: i32,
        preferred_x: Option<f64>,
    ) -> Result<(usize, f64), LayoutError>;
    /// Flat position nearest to a point of a page, and whether it has upstream
    /// affinity (the point is past the end of a wrapped line).
    fn hit_test(&self, page: u32, x: f64, y: f64) -> Result<(usize, bool), LayoutError>;
    /// Vertical and line movement need layout; returns the new flat position
    /// and the preferred x to keep.
    fn move_vertical(
        &self,
        position: usize,
        lines: i32,
        preferred_x: Option<f64>,
    ) -> Result<(usize, f64), LayoutError>;
}
