// SPDX-License-Identifier: AGPL-3.0-or-later
//! Caret geometry over caller-positioned rich-text lines (US-214).
//!
//! Coordinates and advances are 1/64 pt. Offsets are UTF-8 bytes within each
//! paragraph. `PositionedLine::top` is the page-local line-box top; horizontal
//! translation is supplied explicitly in `x_translation`.
//! Ligature interiors are interpolated uniformly between grapheme boundaries:
//! this is deterministic and keeps every grapheme reachable, but is an
//! approximation because this crate does not consume GDEF caret data.

use crate::LayoutLine;
use atelier_unicode::segmentation::grapheme_boundaries;
use core::ops::Range;
use std::error::Error;
use std::fmt;

const DEFAULT_MAX_LINES: usize = 16_384;
const DEFAULT_MAX_STOPS: usize = 1_048_576;
const DEFAULT_MAX_WORK: usize = 4_000_000;
const CARET_WIDTH: i64 = 1;

/// Resource ceilings for untrusted line geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaretOptions {
    pub max_lines: usize,
    pub max_stops: usize,
    pub max_work: usize,
}

impl Default for CaretOptions {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_stops: DEFAULT_MAX_STOPS,
            max_work: DEFAULT_MAX_WORK,
        }
    }
}

/// One caller-positioned line. `text` is the original paragraph text.
#[derive(Clone, Copy, Debug)]
pub struct PositionedLine<'a> {
    pub page_index: usize,
    pub paragraph_index: usize,
    pub text: &'a str,
    pub layout: &'a LayoutLine,
    /// Top edge in page-local 1/64 pt coordinates.
    pub top: i64,
    /// Explicit horizontal translation applied to layout glyph coordinates.
    pub x_translation: i64,
}

/// A byte position in one paragraph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TextPosition {
    pub paragraph_index: usize,
    pub byte_offset: usize,
}

/// Selects the side of a shared line boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

/// Which adjacent visual line to use for an ambiguous byte position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalDirection {
    Up,
    Down,
}

/// Caret rectangle and its owning page/line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaretRect {
    pub page_index: usize,
    pub paragraph_index: usize,
    pub line_index: usize,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// A selection rectangle for one visual line, preserving page identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionRect {
    pub page_index: usize,
    pub paragraph_index: usize,
    pub line_index: usize,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// Typed validation, budget, and query errors. Construction and queries never
/// return partial geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaretError {
    InvalidInput(&'static str),
    InvalidPosition,
    InvalidRange,
    InvalidPage,
    LimitExceeded(&'static str),
    ArithmeticOverflow,
    NoAdjacentLine,
}

impl fmt::Display for CaretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(s) => write!(f, "invalid caret input: {s}"),
            Self::InvalidPosition => {
                f.write_str("position is outside the document or not a grapheme boundary")
            }
            Self::InvalidRange => {
                f.write_str("selection range is outside the document or not on grapheme boundaries")
            }
            Self::InvalidPage => f.write_str("page index has no positioned lines"),
            Self::LimitExceeded(s) => write!(f, "caret geometry limit exceeded: {s}"),
            Self::ArithmeticOverflow => f.write_str("caret geometry arithmetic overflow"),
            Self::NoAdjacentLine => f.write_str("there is no adjacent visual line"),
        }
    }
}
impl Error for CaretError {}

#[derive(Clone, Debug)]
struct Stop {
    position: TextPosition,
    x: i64,
    line: usize,
}

#[derive(Clone, Debug)]
struct LineInfo {
    page: usize,
    paragraph: usize,
    source: Range<usize>,
    mandatory_break: bool,
    y: i64,
    height: i64,
    stops: Range<usize>,
}

#[derive(Clone, Debug)]
struct Cluster {
    start: usize,
    end: usize,
    x: i64,
    advance: i64,
    start_slot: usize,
    end_slot: usize,
}

/// Validated caret/hit-test geometry over positioned lines. Page identity is
/// retained on every returned rectangle; hit testing always names a page.
#[derive(Clone, Debug)]
pub struct CaretLayout {
    lines: Vec<LineInfo>,
    stops: Vec<Stop>,
    paragraphs: Vec<(usize, String, Vec<usize>)>,
}

impl CaretLayout {
    /// Validate source continuity, grapheme boundaries, glyph spans, geometry,
    /// and resource budgets before exposing any query.
    pub fn new(input: &[PositionedLine<'_>], options: CaretOptions) -> Result<Self, CaretError> {
        if options.max_lines == 0 || options.max_stops == 0 || options.max_work == 0 {
            return Err(CaretError::InvalidInput("budgets must be positive"));
        }
        if input.is_empty() {
            return Err(CaretError::InvalidInput("at least one line is required"));
        }
        if input.len() > options.max_lines {
            return Err(CaretError::LimitExceeded("lines"));
        }

        let mut work = 0usize;
        work = add_work(work, input.len(), options.max_work)?;
        let mut paragraphs: Vec<(usize, String, Vec<usize>)> = Vec::new();
        let mut line_infos = Vec::with_capacity(input.len());
        let mut stops: Vec<Stop> = Vec::new();
        let mut prior_para = None;
        let mut prior_end = 0usize;
        let mut prior_page_bottom: Option<(usize, i64)> = None;
        for (line_index, positioned) in input.iter().enumerate() {
            let line = positioned.layout;
            if line.height <= 0
                || line.width < 0
                || line.source.start > line.source.end
                || line.source.end > positioned.text.len()
            {
                return Err(CaretError::InvalidInput(
                    "line range, width, or height is invalid",
                ));
            }
            work = add_work(work, positioned.text.len(), options.max_work)?;
            let para_changed = prior_para != Some(positioned.paragraph_index);
            if para_changed {
                if prior_para.is_some_and(|p| p >= positioned.paragraph_index) {
                    return Err(CaretError::InvalidInput("paragraphs are not ordered"));
                }
                if line.source.start != 0 {
                    return Err(CaretError::InvalidInput(
                        "paragraph lines must start at byte zero",
                    ));
                }
                let text_work = positioned
                    .text
                    .len()
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(2))
                    .ok_or(CaretError::ArithmeticOverflow)?;
                work = add_work(work, text_work, options.max_work)?;
                let boundaries = grapheme_boundaries(positioned.text).boundaries().to_vec();
                paragraphs.push((
                    positioned.paragraph_index,
                    positioned.text.to_owned(),
                    boundaries.clone(),
                ));
                prior_para = Some(positioned.paragraph_index);
                prior_end = 0;
            } else {
                work = add_work(work, 1, options.max_work)?;
                let stored = paragraphs
                    .last()
                    .ok_or(CaretError::InvalidInput("missing paragraph"))?;
                if stored.1 != positioned.text {
                    return Err(CaretError::InvalidInput(
                        "paragraph text changes between lines",
                    ));
                }
            }
            let boundaries = &paragraphs
                .last()
                .ok_or(CaretError::InvalidInput("missing paragraph"))?
                .2;
            let search_depth = (usize::BITS - boundaries.len().leading_zeros()) as usize;
            work = add_work(
                work,
                search_depth
                    .checked_mul(2)
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            if line.source.start != prior_end {
                return Err(CaretError::InvalidInput("line ranges are not contiguous"));
            }
            if !is_boundary(boundaries, line.source.start)
                || !is_boundary(boundaries, line.source.end)
            {
                return Err(CaretError::InvalidInput("line range splits a grapheme"));
            }

            work = add_work(work, line.fragments.len(), options.max_work)?;
            let glyph_count = line
                .fragments
                .iter()
                .try_fold(0usize, |count, fragment| {
                    count.checked_add(fragment.glyphs.len())
                })
                .ok_or(CaretError::ArithmeticOverflow)?;
            work = add_work(work, glyph_count, options.max_work)?;
            work = add_work(
                work,
                glyph_count
                    .checked_mul(
                        search_depth
                            .checked_mul(2)
                            .ok_or(CaretError::ArithmeticOverflow)?,
                    )
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            let mut clusters: Vec<Cluster> = Vec::with_capacity(glyph_count);
            let mut prior_glyph: Option<(usize, usize)> = None;
            for g in line.fragments.iter().flat_map(|f| f.glyphs.iter()) {
                if g.source.start < line.source.start
                    || g.source.start >= g.source.end
                    || g.source.end > line.source.end
                    || g.x_advance < 0
                {
                    return Err(CaretError::InvalidInput(
                        "glyph range or advance is invalid",
                    ));
                }
                if prior_glyph.is_some_and(|(start, end)| {
                    g.source.start < end && (g.source.start, g.source.end) != (start, end)
                }) {
                    return Err(CaretError::InvalidInput(
                        "glyph source ranges overlap without sharing a cluster",
                    ));
                }
                prior_glyph = Some((g.source.start, g.source.end));
                if !is_boundary(boundaries, g.source.start)
                    || !is_boundary(boundaries, g.source.end)
                {
                    return Err(CaretError::InvalidInput("glyph range splits a grapheme"));
                }
                let pen_x =
                    g.x.checked_sub(g.x_offset)
                        .ok_or(CaretError::ArithmeticOverflow)?;
                if let Some(last) = clusters.last_mut() {
                    if (g.source.start, g.source.end) == (last.start, last.end) {
                        last.advance = last
                            .advance
                            .checked_add(g.x_advance)
                            .ok_or(CaretError::ArithmeticOverflow)?;
                        continue;
                    }
                }
                clusters.push(Cluster {
                    start: g.source.start,
                    end: g.source.end,
                    x: pen_x,
                    advance: g.x_advance,
                    start_slot: 0,
                    end_slot: 0,
                });
            }
            let source_byte_work = line
                .source
                .end
                .checked_sub(line.source.start)
                .and_then(|n| n.checked_add(1))
                .ok_or(CaretError::ArithmeticOverflow)?;
            work = add_work(work, source_byte_work, options.max_work)?;
            work = add_work(
                work,
                search_depth
                    .checked_mul(4)
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            let required_end =
                required_geometry_end(positioned.text, boundaries, line.source.clone());
            let source_start_slot = boundaries
                .binary_search(&line.source.start)
                .map_err(|_| CaretError::InvalidInput("line start is not a grapheme boundary"))?;
            let source_end_slot = boundaries
                .binary_search(&line.source.end)
                .map_err(|_| CaretError::InvalidInput("line end is not a grapheme boundary"))?;
            let required_end_slot = boundaries
                .binary_search(&required_end)
                .map_err(|_| CaretError::InvalidInput("visible end is not a grapheme boundary"))?;
            let stop_count = source_end_slot
                .checked_sub(source_start_slot)
                .and_then(|n| n.checked_add(1))
                .ok_or(CaretError::ArithmeticOverflow)?;
            let next_stop_count = stops
                .len()
                .checked_add(stop_count)
                .ok_or(CaretError::ArithmeticOverflow)?;
            if next_stop_count > options.max_stops {
                return Err(CaretError::LimitExceeded("stops"));
            }
            let line_bound_count = stop_count;
            work = add_work(
                work,
                line_bound_count
                    .checked_add(clusters.len())
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            let mut boundary_cursor = source_start_slot;
            for cluster in &mut clusters {
                while boundaries[boundary_cursor] < cluster.start {
                    boundary_cursor += 1;
                }
                cluster.start_slot = boundary_cursor;
                while boundaries[boundary_cursor] < cluster.end {
                    boundary_cursor += 1;
                }
                cluster.end_slot = boundary_cursor;
            }
            let visible_segments = required_end_slot.saturating_sub(source_start_slot);
            work = add_work(
                work,
                visible_segments
                    .checked_add(clusters.len())
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            let mut coverage_cluster = 0usize;
            for pair in boundaries[source_start_slot..=required_end_slot].windows(2) {
                while coverage_cluster < clusters.len() && clusters[coverage_cluster].end <= pair[0]
                {
                    coverage_cluster += 1;
                }
                let covered = clusters
                    .get(coverage_cluster)
                    .is_some_and(|cluster| cluster.start <= pair[0] && pair[1] <= cluster.end);
                if !covered {
                    return Err(CaretError::InvalidInput(
                        "visible text is missing glyph geometry",
                    ));
                }
            }
            // Validate translated horizontal coordinates and page-local vertical extent.
            let y = positioned.top;
            let _bottom = y
                .checked_add(line.height)
                .ok_or(CaretError::ArithmeticOverflow)?;
            if prior_page_bottom.is_some_and(|(page, bottom)| {
                positioned.page_index < page || (positioned.page_index == page && y < bottom)
            }) {
                return Err(CaretError::InvalidInput(
                    "positioned lines are not ordered by page and top",
                ));
            }
            prior_page_bottom = Some((positioned.page_index, _bottom));
            for cluster in &clusters {
                cluster
                    .x
                    .checked_add(positioned.x_translation)
                    .and_then(|v| v.checked_add(cluster.advance))
                    .ok_or(CaretError::ArithmeticOverflow)?;
            }
            let first_stop = stops.len();
            work = add_work(
                work,
                stop_count
                    .checked_add(clusters.len())
                    .ok_or(CaretError::ArithmeticOverflow)?,
                options.max_work,
            )?;
            let mut cluster_cursor = 0usize;
            let mut pen = line.x;
            for (slot, &byte) in boundaries
                .iter()
                .enumerate()
                .take(source_end_slot + 1)
                .skip(source_start_slot)
            {
                while cluster_cursor < clusters.len() && byte > clusters[cluster_cursor].end {
                    let cluster = &clusters[cluster_cursor];
                    pen = cluster
                        .x
                        .checked_add(cluster.advance)
                        .ok_or(CaretError::ArithmeticOverflow)?;
                    cluster_cursor += 1;
                }
                let x = if let Some(cluster) = clusters
                    .get(cluster_cursor)
                    .filter(|c| byte >= c.start && byte <= c.end)
                {
                    let part = slot
                        .saturating_sub(cluster.start_slot)
                        .min(cluster.end_slot - cluster.start_slot);
                    let count = cluster.end_slot - cluster.start_slot;
                    let advance = i128::from(cluster.advance)
                        .checked_mul(part as i128)
                        .ok_or(CaretError::ArithmeticOverflow)?
                        / count as i128;
                    i64::try_from(
                        i128::from(cluster.x) + advance + i128::from(positioned.x_translation),
                    )
                    .map_err(|_| CaretError::ArithmeticOverflow)?
                } else {
                    pen.checked_add(positioned.x_translation)
                        .ok_or(CaretError::ArithmeticOverflow)?
                };
                stops.push(Stop {
                    position: TextPosition {
                        paragraph_index: positioned.paragraph_index,
                        byte_offset: byte,
                    },
                    x,
                    line: line_index,
                });
            }
            let stop_range = first_stop..stops.len();
            line_infos.push(LineInfo {
                page: positioned.page_index,
                paragraph: positioned.paragraph_index,
                source: line.source.clone(),
                mandatory_break: line.mandatory_break,
                y,
                height: line.height,
                stops: stop_range,
            });
            prior_end = line.source.end;
        }
        // A paragraph can be complete only when its last supplied line reaches the text end.
        add_work(
            work,
            paragraphs
                .len()
                .checked_add(line_infos.len())
                .ok_or(CaretError::ArithmeticOverflow)?,
            options.max_work,
        )?;
        let mut line_cursor = 0usize;
        for (paragraph_index, text, _) in &paragraphs {
            let mut last = None;
            while line_cursor < line_infos.len()
                && line_infos[line_cursor].paragraph == *paragraph_index
            {
                last = Some(&line_infos[line_cursor]);
                line_cursor += 1;
            }
            let last = last.ok_or(CaretError::InvalidInput("paragraph has no lines"))?;
            if last.source.end != text.len() {
                return Err(CaretError::InvalidInput(
                    "paragraph lines do not cover the original text",
                ));
            }
            if text.chars().next_back().is_some_and(is_mandatory_break)
                && (last.source.start != text.len() || last.mandatory_break)
            {
                return Err(CaretError::InvalidInput(
                    "terminal mandatory break requires a following empty line",
                ));
            }
        }
        Ok(Self {
            lines: line_infos,
            stops,
            paragraphs,
        })
    }

    /// Return the caret rectangle at a paragraph byte offset. Before LF/CRLF
    /// belongs to the preceding line; after it belongs to the following line.
    /// Soft-wrap boundaries use the requested affinity.
    pub fn caret_rect(
        &self,
        position: TextPosition,
        affinity: CaretAffinity,
    ) -> Result<CaretRect, CaretError> {
        let candidates = self.candidates(position)?;
        let stops = match affinity {
            CaretAffinity::Upstream => candidates.first(),
            CaretAffinity::Downstream => candidates.last(),
        };
        let stop = if candidates.len() > 1
            && candidates
                .first()
                .is_some_and(|candidate| self.lines[candidate.line].mandatory_break)
        {
            candidates.last()
        } else {
            stops
        }
        .ok_or(CaretError::InvalidPosition)?;
        let line = &self.lines[stop.line];
        Ok(CaretRect {
            page_index: line.page,
            paragraph_index: line.paragraph,
            line_index: stop.line,
            x: stop.x,
            y: line.y,
            width: stop
                .x
                .checked_add(CARET_WIDTH)
                .ok_or(CaretError::ArithmeticOverflow)?
                - stop.x,
            height: line.height,
        })
    }

    /// Return the closest text position on the closest line of one page. x/y
    /// are page-local coordinates. Ties choose the earlier visual stop.
    pub fn hit_test(&self, page_index: usize, x: i64, y: i64) -> Result<TextPosition, CaretError> {
        let mut best_line: Option<(i128, usize)> = None;
        for (i, line) in self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.page == page_index)
        {
            let y128 = i128::from(y);
            let top = i128::from(line.y);
            let bottom = top + i128::from(line.height);
            let distance = if y128 < top {
                top - y128
            } else if y128 > bottom {
                y128 - bottom
            } else {
                0
            };
            let candidate = (distance, i);
            if best_line.is_none_or(|best| candidate < best) {
                best_line = Some(candidate);
            }
        }
        let (_, line_index) = best_line.ok_or(CaretError::InvalidPage)?;
        self.stops[self.lines[line_index].stops.clone()]
            .iter()
            .min_by_key(|stop| (i128::from(x) - i128::from(stop.x)).abs())
            .map(|stop| stop.position)
            .ok_or(CaretError::InvalidInput("line has no caret stops"))
    }

    /// Rectangles for a selection; reversed endpoints are normalized. Empty
    /// selections produce no rectangles.
    pub fn selection_rects(
        &self,
        range: Range<TextPosition>,
    ) -> Result<Vec<SelectionRect>, CaretError> {
        self.validate_position(range.start)
            .map_err(|_| CaretError::InvalidRange)?;
        self.validate_position(range.end)
            .map_err(|_| CaretError::InvalidRange)?;
        let (start, end) = if range.start <= range.end {
            (range.start, range.end)
        } else {
            (range.end, range.start)
        };
        if start == end {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for (line_index, line) in self.lines.iter().enumerate() {
            let line_start = TextPosition {
                paragraph_index: line.paragraph,
                byte_offset: line.source.start,
            };
            let line_end = TextPosition {
                paragraph_index: line.paragraph,
                byte_offset: line.source.end,
            };
            if end <= line_start || start >= line_end {
                continue;
            }
            let a = if start > line_start {
                start.byte_offset
            } else {
                line.source.start
            };
            let b = if end < line_end {
                end.byte_offset
            } else {
                line.source.end
            };
            if a >= b {
                continue;
            }
            let xa = self.stop_for(line_index, a)?.x;
            let xb = self.stop_for(line_index, b)?.x;
            result.push(SelectionRect {
                page_index: line.page,
                paragraph_index: line.paragraph,
                line_index,
                x: xa.min(xb),
                y: line.y,
                width: xa
                    .abs_diff(xb)
                    .try_into()
                    .map_err(|_| CaretError::ArithmeticOverflow)?,
                height: line.height,
            });
        }
        Ok(result)
    }

    /// Move one visual line using the downstream convention for shared offsets.
    /// This compatibility API returns no affinity; reusing its position treats
    /// a shared soft-wrap offset as belonging to the following line. Keep the
    /// returned x through short lines. Use `move_vertical_with_affinity` for
    /// repeated visual navigation.
    pub fn move_vertical(
        &self,
        position: TextPosition,
        preferred_x: Option<i64>,
        direction: VerticalDirection,
    ) -> Result<(TextPosition, i64), CaretError> {
        let (position, _, preferred_x) = self.move_vertical_with_affinity(
            position,
            CaretAffinity::Downstream,
            preferred_x,
            direction,
        )?;
        Ok((position, preferred_x))
    }

    /// Move to an adjacent visual line while retaining the caller's preferred
    /// x. The returned affinity identifies the destination line when the byte
    /// offset is shared by a soft wrap; pass it into the next move in a sequence.
    pub fn move_vertical_with_affinity(
        &self,
        position: TextPosition,
        current_affinity: CaretAffinity,
        preferred_x: Option<i64>,
        direction: VerticalDirection,
    ) -> Result<(TextPosition, CaretAffinity, i64), CaretError> {
        let candidates = self.candidates(position)?;
        let current_stop = match current_affinity {
            CaretAffinity::Upstream => candidates.first(),
            CaretAffinity::Downstream => candidates.last(),
        };
        let current_stop = if candidates.len() > 1
            && candidates
                .first()
                .is_some_and(|candidate| self.lines[candidate.line].mandatory_break)
        {
            candidates.last()
        } else {
            current_stop
        }
        .ok_or(CaretError::InvalidPosition)?;
        let wanted = preferred_x.unwrap_or(current_stop.x);
        let current = current_stop.line;
        let target_index = match direction {
            VerticalDirection::Up => current.checked_sub(1),
            VerticalDirection::Down => current.checked_add(1).filter(|&i| i < self.lines.len()),
        }
        .ok_or(CaretError::NoAdjacentLine)?;
        let line = &self.lines[target_index];
        let chosen = self.stops[line.stops.clone()]
            .iter()
            .filter(|stop| !(line.mandatory_break && stop.position.byte_offset == line.source.end))
            .min_by_key(|s| {
                (
                    (i128::from(s.x) - i128::from(wanted)).abs(),
                    s.position.byte_offset,
                )
            })
            .ok_or(CaretError::InvalidInput("target line has no stops"))?;
        let destination = self.candidates(chosen.position)?;
        let affinity = if destination.len() > 1
            && destination
                .first()
                .is_some_and(|stop| stop.line == target_index)
        {
            CaretAffinity::Upstream
        } else {
            CaretAffinity::Downstream
        };
        Ok((chosen.position, affinity, wanted))
    }

    fn validate_position(&self, position: TextPosition) -> Result<(), CaretError> {
        let (_, text, boundaries) = self
            .paragraphs
            .iter()
            .find(|(i, _, _)| *i == position.paragraph_index)
            .ok_or(CaretError::InvalidPosition)?;
        if position.byte_offset > text.len() || !is_boundary(boundaries, position.byte_offset) {
            return Err(CaretError::InvalidPosition);
        }
        Ok(())
    }

    fn candidates(&self, position: TextPosition) -> Result<Vec<&Stop>, CaretError> {
        self.validate_position(position)?;
        Ok(self
            .stops
            .iter()
            .filter(|s| s.position == position)
            .collect())
    }

    fn stop_for(&self, line: usize, byte: usize) -> Result<&Stop, CaretError> {
        self.stops[self.lines[line].stops.clone()]
            .iter()
            .find(|s| s.position.byte_offset == byte)
            .ok_or(CaretError::InvalidRange)
    }
}

fn is_boundary(boundaries: &[usize], value: usize) -> bool {
    boundaries.binary_search(&value).is_ok()
}

fn is_mandatory_break(ch: char) -> bool {
    matches!(
        ch,
        '\r' | '\n' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

fn required_geometry_end(text: &str, boundaries: &[usize], source: Range<usize>) -> usize {
    let mut slot = boundaries.partition_point(|&boundary| boundary < source.end);
    // Mandatory line-ending controls are source positions, not glyphs.
    while slot > 0 && boundaries[slot - 1] >= source.start {
        let segment = &text[boundaries[slot - 1]..boundaries[slot]];
        if segment.chars().all(|ch| {
            matches!(
                ch,
                '\r' | '\n' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
            )
        }) {
            slot -= 1;
        } else {
            break;
        }
    }
    // Ordinary terminal whitespace is deliberately trimmed by line layout.
    while slot > 0 && boundaries[slot - 1] >= source.start {
        let segment = &text[boundaries[slot - 1]..boundaries[slot]];
        if segment.chars().all(|ch| {
            ch.is_whitespace() && !matches!(ch, '\t' | '\u{00a0}' | '\u{202f}' | '\u{2060}')
        }) {
            slot -= 1;
        } else {
            break;
        }
    }
    boundaries[slot]
}

fn add_work(current: usize, amount: usize, max: usize) -> Result<usize, CaretError> {
    let next = current
        .checked_add(amount)
        .ok_or(CaretError::ArithmeticOverflow)?;
    if next > max {
        Err(CaretError::LimitExceeded("work"))
    } else {
        Ok(next)
    }
}
