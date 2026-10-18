// SPDX-License-Identifier: AGPL-3.0-or-later
//! Page-cache contract of the rich editor layout (US-203, implemented by
//! US-216).
//!
//! Target: under 4 ms per keystroke on 300 pages, no global pagination
//! pre-validation (layout is incremental, the tail paginates in `step`).
//!
//! Only declarations: nothing here is implemented yet. R2 implements
//! [`RichPageCache`]; the editor (R3a) and the protocol replies depend only on
//! these signatures. Positions are the flat byte positions of
//! `atelier_text_model::rich::flat`; display lists are `ATDL` byte vectors
//! (`docs/us-080-display-list.md`) ready to be copied into a reply.

use atelier_text_model::rich::flat::EditOutcome;

/// Caret geometry in page coordinates (points, as in the display list).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretGeometry {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

/// Pages affected by one layout update. `changed` pages are the ones whose
/// display list differs and must be re-sent; `invalidated` are half-open
/// ranges whose cached lists the client must drop (shifted or removed pages
/// that are not re-sent, for instance beyond the per-response page limit).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageDelta {
    pub page_count: u32,
    pub layout_complete: bool,
    pub changed: Vec<u32>,
    pub invalidated: Vec<(u32, u32)>,
}

/// Why a cache query failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    PageOutOfRange,
    PositionOutOfRange,
    /// Layout of the requested area is not finished yet.
    NotReady,
}

/// Incremental layout and per-page paint cache of one document.
pub trait RichPageCache {
    fn page_count(&self) -> u32;
    fn layout_complete(&self) -> bool;
    /// Re-lays out from the paragraph touched by `edit`, reusing every
    /// untouched paragraph, and reports the pages that changed.
    fn apply_edit(&mut self, edit: &EditOutcome) -> PageDelta;
    /// Runs a bounded amount of pending background layout (pagination of the
    /// tail of a long document) and reports what it changed.
    fn step(&mut self, budget_micros: u32) -> PageDelta;
    /// `ATDL` display list of one page, painted lazily and memoised until the
    /// page changes.
    fn page_display(&mut self, index: u32) -> Result<Vec<u8>, LayoutError>;
    fn caret(&self, position: usize) -> Result<CaretGeometry, LayoutError>;
    /// Flat position nearest to a point of a page.
    fn hit_test(&self, page: u32, x: f64, y: f64) -> Result<usize, LayoutError>;
    /// Vertical and line movement need layout; returns the new flat position.
    fn move_vertical(
        &self,
        position: usize,
        lines: i32,
        preferred_x: Option<f64>,
    ) -> Result<(usize, f64), LayoutError>;
}
