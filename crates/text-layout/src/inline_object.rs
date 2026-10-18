// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline objects in a line of text (US-270).
//!
//! An inline picture is a box of fixed width sitting on the baseline: it adds
//! its width to the line, raises the line ascent to its height, and keeps the
//! font descent below the baseline. When it is wider than the text area (or
//! taller than a page) it shrinks proportionally; it never grows.

use atelier_text_model::inline_image::{fit_within, natural_size_points};

pub use atelier_text_model::inline_image::POINTS_PER_PIXEL;

/// Natural size in points of a `width` x `height` pixel picture once its EXIF
/// `orientation` is applied.
pub fn natural_size(width: u32, height: u32, orientation: u8) -> (f64, f64) {
    natural_size_points(width, height, orientation)
}

/// Shrinks `(width, height)` proportionally to fit `max_width` x `max_height`;
/// see [`atelier_text_model::inline_image::fit_within`].
pub fn fit(width: f64, height: f64, max_width: f64, max_height: f64) -> Option<(f64, f64)> {
    fit_within(width, height, max_width, max_height)
}

/// Vertical extent of one line: distances above and below its baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    pub ascent: f64,
    pub descent: f64,
}

impl LineBox {
    pub fn height(self) -> f64 {
        self.ascent + self.descent
    }
}

/// Line box of a row whose text has `text_ascent`/`text_descent` and that
/// holds objects of the given `heights`, each sitting on the baseline.
pub fn line_box(
    text_ascent: f64,
    text_descent: f64,
    heights: impl IntoIterator<Item = f64>,
) -> LineBox {
    let ascent = heights
        .into_iter()
        .filter(|h| h.is_finite())
        .fold(text_ascent, f64::max);
    LineBox {
        ascent,
        descent: text_descent,
    }
}
