// SPDX-License-Identifier: AGPL-3.0-or-later
//! Adapter from rich pagination to validated caret geometry.

use crate::ParagraphLayout;
use crate::caret::{CaretError, CaretLayout, CaretOptions, PositionedLine};
use crate::pagination::Pagination;
use std::error::Error;
use std::fmt;

/// A pagination object does not describe the supplied paragraph layouts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlowError {
    InvalidPagination(&'static str),
    Caret(CaretError),
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPagination(message) => write!(f, "invalid rich flow: {message}"),
            Self::Caret(error) => error.fmt(f),
        }
    }
}

impl Error for FlowError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Caret(error) => Some(error),
            Self::InvalidPagination(_) => None,
        }
    }
}

/// Build caret geometry using page-local line tops and a horizontal translation.
/// `texts` and `layouts` use the original paragraph order passed to `paginate`.
/// Public pagination objects are checked for complete, ordered, unique coverage
/// before any view is constructed. Caret construction validates text and glyphs.
pub fn caret_from_pagination(
    texts: &[&str],
    layouts: &[ParagraphLayout],
    pagination: &Pagination,
    x_translation: i64,
    options: CaretOptions,
) -> Result<CaretLayout, FlowError> {
    let invalid = FlowError::InvalidPagination;
    if texts.len() != layouts.len() {
        return Err(invalid("text and layout paragraph counts differ"));
    }
    if pagination.placements.len() > options.max_lines {
        return Err(FlowError::Caret(CaretError::LimitExceeded("lines")));
    }
    // A limit on paragraphs also bounds work on empty/malformed layouts.
    if layouts.len() > options.max_lines {
        return Err(FlowError::Caret(CaretError::LimitExceeded("paragraphs")));
    }
    let expected_count = layouts.iter().try_fold(0usize, |sum, layout| {
        sum.checked_add(layout.lines.len())
            .ok_or_else(|| invalid("line count overflow"))
    })?;
    if expected_count != pagination.placements.len() {
        return Err(invalid("pagination does not cover every layout line"));
    }
    let mut next_placement = 0usize;
    for (page_index, page) in pagination.pages.iter().enumerate() {
        let range = &page.placements;
        if range.start != next_placement
            || range.start >= range.end
            || range.end > pagination.placements.len()
        {
            return Err(invalid("page placement ranges are not contiguous"));
        }
        if pagination.placements[range.clone()]
            .iter()
            .any(|placement| placement.page_index != page_index)
        {
            return Err(invalid("placement page index does not match its page"));
        }
        next_placement = range.end;
    }
    if next_placement != pagination.placements.len() {
        return Err(invalid("page ranges do not cover all placements"));
    }
    let mut views = Vec::with_capacity(expected_count);
    let mut placements = pagination.placements.iter();
    for (paragraph_index, layout) in layouts.iter().enumerate() {
        for (line_index, line) in layout.lines.iter().enumerate() {
            let placed = placements
                .next()
                .ok_or_else(|| invalid("missing line placement"))?;
            if placed.paragraph_index != paragraph_index || placed.line_index != line_index {
                return Err(invalid("line placements are out of source order"));
            }
            views.push(PositionedLine {
                page_index: placed.page_index,
                paragraph_index,
                text: texts[paragraph_index],
                layout: line,
                top: placed.top,
                x_translation,
            });
        }
    }
    CaretLayout::new(&views, options).map_err(FlowError::Caret)
}
