// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, read-only query responses for the plain-text editor.

use crate::editor::{EditorError, Writer};
use crate::editor_layout::Layout;
use atelier_text_model::{
    Selection,
    search::{SearchOptions, Searcher, TextLines},
    statistics::{StatisticsOptions, plain_text, plain_text_range},
};
use atelier_unicode::segmentation::grapheme_boundaries;
use core::ops::Range;

pub(crate) struct QueryLimits {
    pub max_results: usize,
    pub max_rects: usize,
    pub max_excerpt_bytes: usize,
    pub max_response_bytes: usize,
}

pub(crate) const SEARCH_LIMITS: QueryLimits = QueryLimits {
    max_results: 1000,
    max_rects: 8192,
    max_excerpt_bytes: 512,
    max_response_bytes: 32 * 1024 * 1024,
};

type HighlightRect = (usize, f64, f64, f64, f64);
type HighlightRects = Vec<HighlightRect>;

pub(crate) fn search_response(
    text: &str,
    layout: &Layout,
    query: &str,
    options: SearchOptions,
    revision: u64,
    limits: QueryLimits,
) -> Result<Vec<u8>, EditorError> {
    let source = TextLines(text);
    let searcher = Searcher::new(&source, query, options);
    let found = searcher.find_all(limits.max_results);
    let boundaries: Vec<usize> = grapheme_boundaries(text)
        .segments()
        .map(|range| range.start)
        .chain(core::iter::once(text.len()))
        .collect();
    let mut items = Vec::new();
    items
        .try_reserve_exact(found.items.len())
        .map_err(|_| EditorError("EDITOR_LIMIT"))?;
    let mut total_rects = 0usize;
    let mut projected_bytes = 24usize;
    let mut truncated = found.truncated;
    for occurrence in &found.items {
        let range = occurrence.global_range();
        let Some(excerpt) = bounded_excerpt(
            text,
            &boundaries,
            range.clone(),
            limits.max_excerpt_bytes.max(range.len()),
        ) else {
            // A pathological match cannot fit the per-excerpt wire bound.
            truncated = true;
            break;
        };
        let remaining = limits.max_rects.saturating_sub(total_rects);
        let Some(rects) = range_rects(layout, range, remaining)? else {
            truncated = true;
            break;
        };
        let item_bytes = 28usize
            .checked_add(excerpt.text.len())
            .and_then(|n| n.checked_add(rects.len().checked_mul(36)?))
            .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?;
        let Some(next_bytes) = projected_bytes.checked_add(item_bytes) else {
            truncated = true;
            break;
        };
        if next_bytes > limits.max_response_bytes {
            truncated = true;
            break;
        }
        projected_bytes = next_bytes;
        total_rects += rects.len();
        items.push((occurrence.global_range(), excerpt, rects));
    }
    if items.len() < found.items.len() {
        truncated = true;
    }

    let mut out = Writer::with_capacity(projected_bytes)?;
    out.bytes(b"ATQF");
    out.u16(1);
    out.u16(0);
    out.u64(revision);
    out.u32(u32_len(items.len())?);
    out.u8(u8::from(truncated));
    out.u8(0);
    out.u16(0);
    for (range, excerpt, rects) in items {
        out.u32(u32_len(range.start)?);
        out.u32(u32_len(range.end)?);
        out.u32(u32_len(excerpt.text.len())?);
        out.u32(u32_len(excerpt.start)?);
        out.u32(u32_len(excerpt.end)?);
        let flags = u8::from(excerpt.truncated_before) | (u8::from(excerpt.truncated_after) << 1);
        out.u8(flags);
        out.u8(0);
        out.u16(0);
        out.bytes(excerpt.text.as_bytes());
        out.u32(u32_len(rects.len())?);
        for (page, x, y, width, height) in rects {
            out.u32(u32_len(page)?);
            out.f64(x);
            out.f64(y);
            out.f64(width);
            out.f64(height);
        }
    }
    out.finish(limits.max_response_bytes)
}

struct BoundedExcerpt<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    truncated_before: bool,
    truncated_after: bool,
}

fn bounded_excerpt<'a>(
    text: &'a str,
    boundaries: &[usize],
    range: Range<usize>,
    cap: usize,
) -> Option<BoundedExcerpt<'a>> {
    let matched = range.end.checked_sub(range.start)?;
    if matched > cap {
        return None;
    }
    let center_start = boundaries.partition_point(|&b| b < range.start);
    let center_end = boundaries.partition_point(|&b| b < range.end);
    let mut from = center_start;
    let mut to = center_end;
    let mut used = matched;
    // Grow both sides in alternating graphemes, so the match remains centered
    // while the excerpt stays within its UTF-8 byte budget.
    let mut left_open = from > 0;
    let mut right_open = to + 1 < boundaries.len();
    let mut choose_left = true;
    while left_open || right_open {
        if choose_left && left_open {
            let bytes = boundaries[from] - boundaries[from - 1];
            if used.checked_add(bytes)? > cap {
                left_open = false;
            } else {
                from -= 1;
                used += bytes;
                left_open = from > 0;
            }
        } else if right_open {
            let bytes = boundaries[to + 1] - boundaries[to];
            if used.checked_add(bytes)? > cap {
                right_open = false;
            } else {
                to += 1;
                used += bytes;
                right_open = to + 1 < boundaries.len();
            }
        } else {
            left_open = false;
        }
        choose_left = !choose_left;
    }
    let byte_start = boundaries[from];
    let byte_end = boundaries[to];
    Some(BoundedExcerpt {
        text: &text[byte_start..byte_end],
        start: range.start - byte_start,
        end: range.end - byte_start,
        truncated_before: byte_start > 0,
        truncated_after: byte_end < text.len(),
    })
}

fn range_rects(
    layout: &Layout,
    range: Range<usize>,
    limit: usize,
) -> Result<Option<HighlightRects>, EditorError> {
    let mut rects: HighlightRects = Vec::new();
    for line in &layout.lines {
        for pair in line.slots.windows(2) {
            if pair[0].0 >= range.end || pair[1].0 <= range.start {
                continue;
            }
            let x = pair[0].1;
            let width = (pair[1].1 - pair[0].1).max(0.5);
            if let Some(last) = rects.last_mut().filter(|last| {
                last.0 == line.page && last.2 == line.top && (last.1 + last.3 - x).abs() < 1e-7
            }) {
                last.3 += width;
            } else {
                if rects.len() >= limit {
                    return Ok(None);
                }
                rects
                    .try_reserve(1)
                    .map_err(|_| EditorError("EDITOR_LIMIT"))?;
                rects.push((line.page, x, line.top, width, 14.0));
            }
        }
    }
    Ok(Some(rects))
}

pub(crate) fn statistics_response(
    text: &str,
    selection: Selection,
    layout: &Layout,
    revision: u64,
) -> Result<Vec<u8>, EditorError> {
    let document_options = StatisticsOptions {
        lines: Some(layout.lines.len()),
        pages: Some(layout.pages.len()),
        ..StatisticsOptions::default()
    };
    let document = plain_text(text, &document_options);
    let (start, end) = (
        selection.anchor.min(selection.focus),
        selection.anchor.max(selection.focus),
    );
    let (selected_lines, selected_pages) = if start == end {
        (0, 0)
    } else {
        let mut touched_lines = 0usize;
        let mut pages = std::collections::BTreeSet::new();
        for line in &layout.lines {
            let content_intersects = line.start < end && line.end > start;
            let bytes = text.as_bytes();
            let break_len = match bytes.get(line.end) {
                Some(b'\r') if bytes.get(line.end + 1) == Some(&b'\n') => 2,
                Some(b'\r' | b'\n') => 1,
                _ => 0,
            };
            let break_end = line.end + break_len;
            let break_intersects = break_len > 0 && line.end < end && break_end > start;
            if content_intersects || break_intersects {
                touched_lines += 1;
                pages.insert(line.page);
            }
        }
        (touched_lines, pages.len())
    };
    let selection_options = StatisticsOptions {
        lines: Some(selected_lines),
        pages: Some(selected_pages),
        ..StatisticsOptions::default()
    };
    let selected = plain_text_range(text, start..end, &selection_options)
        .map_err(|_| EditorError("EDITOR_INVALID_SELECTION"))?;
    let mut out = Writer::with_capacity(72)?;
    out.bytes(b"ATQT");
    out.u16(1);
    out.u16(0);
    out.u64(revision);
    out.u32(u32_len(selection.anchor)?);
    out.u32(u32_len(selection.focus)?);
    write_stats(
        &mut out,
        document.words,
        document.characters,
        document.characters_with_spaces,
        document.paragraphs,
        document.lines.unwrap_or(0),
        document.pages.unwrap_or(0),
    )?;
    write_stats(
        &mut out,
        selected.words,
        selected.characters,
        selected.characters_with_spaces,
        selected.paragraphs,
        selected.lines.unwrap_or(0),
        selected.pages.unwrap_or(0),
    )?;
    out.finish(72)
}

fn write_stats(
    out: &mut Writer,
    words: usize,
    characters: usize,
    with_spaces: usize,
    paragraphs: usize,
    lines: usize,
    pages: usize,
) -> Result<(), EditorError> {
    for value in [words, characters, with_spaces, paragraphs, lines, pages] {
        out.u32(u32_len(value)?);
    }
    Ok(())
}
fn u32_len(value: usize) -> Result<u32, EditorError> {
    u32::try_from(value).map_err(|_| EditorError("EDITOR_LIMIT"))
}
