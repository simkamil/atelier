// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic pagination for already laid out rich paragraphs (US-212).
//!
//! All dimensions use 1/64 typographic point. Paragraph and line indices refer
//! to the input slices and are retained in every placement.

use crate::ParagraphLayout;
use core::ops::Range;
use std::error::Error;
use std::fmt;

/// Resource ceilings and page height. Dimensions use 1/64 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaginationOptions {
    pub page_height: i64,
    pub max_pages: usize,
    pub max_work: usize,
}

/// Paragraph pagination properties. Spacing uses 1/64 pt; `line_breaks_before`
/// contains zero-based line indices at which a manual page break occurs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParagraphRules {
    pub style_id: u64,
    pub space_before: i64,
    pub space_after: i64,
    pub contextual_spacing: bool,
    pub keep_with_next: bool,
    pub keep_together: bool,
    pub widow_min: usize,
    pub orphan_min: usize,
    pub break_before: bool,
    pub break_after: bool,
    pub line_breaks_before: Vec<usize>,
}

impl Default for ParagraphRules {
    fn default() -> Self {
        Self {
            style_id: 0,
            space_before: 0,
            space_after: 0,
            contextual_spacing: false,
            keep_with_next: false,
            keep_together: false,
            widow_min: 2,
            orphan_min: 2,
            break_before: false,
            break_after: false,
            line_breaks_before: Vec::new(),
        }
    }
}

/// One line placed on a page. `top` is the line box top; baseline is page-local.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacedLine {
    pub page_index: usize,
    pub paragraph_index: usize,
    pub line_index: usize,
    pub top: i64,
    pub baseline: i64,
}

/// Half-open range of placements belonging to one page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    pub placements: Range<usize>,
}

/// Pagination relaxation or overflow explicitly reported to the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaginationDiagnostic {
    KeepTogetherRelaxed {
        paragraph_index: usize,
    },
    KeepWithNextRelaxed {
        paragraph_index: usize,
    },
    WidowOrphanRelaxed {
        paragraph_index: usize,
    },
    LineExceedsPage {
        paragraph_index: usize,
        line_index: usize,
    },
    KeepChainExceedsPage {
        paragraph_index: usize,
    },
    ParagraphSpacingExceedsPage {
        paragraph_index: usize,
    },
}

/// Complete pagination result. Every source line occurs once in `placements`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pagination {
    pub pages: Vec<Page>,
    pub placements: Vec<PlacedLine>,
    pub diagnostics: Vec<PaginationDiagnostic>,
}

/// Malformed inputs, resource limits, or checked-arithmetic failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaginationError {
    InvalidOptions(&'static str),
    InvalidInput(&'static str),
    InvalidLineBreak {
        paragraph_index: usize,
        line_index: usize,
    },
    LimitExceeded(&'static str),
    ArithmeticOverflow,
}

impl fmt::Display for PaginationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(s) => write!(f, "invalid pagination options: {s}"),
            Self::InvalidInput(s) => write!(f, "invalid pagination input: {s}"),
            Self::InvalidLineBreak {
                paragraph_index,
                line_index,
            } => write!(
                f,
                "manual page break line {line_index} is invalid in paragraph {paragraph_index}"
            ),
            Self::LimitExceeded(s) => write!(f, "pagination limit exceeded: {s}"),
            Self::ArithmeticOverflow => f.write_str("pagination arithmetic overflow"),
        }
    }
}
impl Error for PaginationError {}

/// Paginate laid-out paragraphs without changing their source geometry.
pub fn paginate(
    layouts: &[ParagraphLayout],
    rules: &[ParagraphRules],
    options: PaginationOptions,
) -> Result<Pagination, PaginationError> {
    if options.page_height <= 0 {
        return Err(PaginationError::InvalidOptions(
            "page height must be positive",
        ));
    }
    if options.max_pages == 0 || options.max_work == 0 {
        return Err(PaginationError::InvalidOptions(
            "page and work budgets must be positive",
        ));
    }
    if layouts.len() != rules.len() {
        return Err(PaginationError::InvalidInput(
            "rules must match paragraph count",
        ));
    }
    let mut work = 0usize;
    let mut counts = Vec::with_capacity(layouts.len());
    let mut prefixes: Vec<Vec<i64>> = Vec::with_capacity(layouts.len());
    for (paragraph_index, (layout, rule)) in layouts.iter().zip(rules).enumerate() {
        work = add_work(work, 1, options.max_work)?;
        if rule.space_before < 0 || rule.space_after < 0 {
            return Err(PaginationError::InvalidInput(
                "paragraph spacing cannot be negative",
            ));
        }
        if rule.widow_min == 0 || rule.orphan_min == 0 {
            return Err(PaginationError::InvalidInput(
                "widow/orphan limits must be positive",
            ));
        }
        let mut sum = rule
            .space_before
            .checked_add(rule.space_after)
            .ok_or(PaginationError::ArithmeticOverflow)?;
        let mut prefix = Vec::with_capacity(layout.lines.len() + 1);
        prefix.push(0i64);
        for line in &layout.lines {
            if line.height <= 0 {
                return Err(PaginationError::InvalidInput(
                    "line height must be positive",
                ));
            }
            sum = sum
                .checked_add(line.height)
                .ok_or(PaginationError::ArithmeticOverflow)?;
            let prefix_sum = prefix
                .last()
                .copied()
                .unwrap_or(0i64)
                .checked_add(line.height)
                .ok_or(PaginationError::ArithmeticOverflow)?;
            prefix.push(prefix_sum);
            work = add_work(work, 1, options.max_work)?;
        }
        if layout.height != sum {
            return Err(PaginationError::InvalidInput(
                "paragraph height does not equal before + lines + after",
            ));
        }
        let mut last = None;
        for &line_index in &rule.line_breaks_before {
            work = add_work(work, 1, options.max_work)?;
            if line_index == 0
                || line_index >= layout.lines.len()
                || last.is_some_and(|v| line_index <= v)
            {
                return Err(PaginationError::InvalidLineBreak {
                    paragraph_index,
                    line_index,
                });
            }
            last = Some(line_index);
        }
        counts.push(layout.lines.len());
        prefixes.push(prefix);
    }

    let mut has_following_nonempty = vec![false; layouts.len()];
    let mut seen_nonempty = false;
    for index in (0..layouts.len()).rev() {
        work = add_work(work, 1, options.max_work)?;
        has_following_nonempty[index] = seen_nonempty;
        seen_nonempty |= counts[index] > 0;
    }

    let mut keep_chain_heights = vec![0i64; layouts.len()];
    for index in (0..layouts.len()).rev() {
        work = add_work(work, 1, options.max_work)?;
        let own_height = *prefixes[index].last().unwrap_or(&0);
        let explicit_boundary_after = rules[index].break_after
            || !rules[index].line_breaks_before.is_empty()
            || rules
                .get(index + 1)
                .is_some_and(|next| next.break_before || !next.line_breaks_before.is_empty());
        keep_chain_heights[index] =
            if rules[index].keep_with_next && !explicit_boundary_after && index + 1 < layouts.len()
            {
                own_height
                    .checked_add(paragraph_gap(&rules[index], &rules[index + 1]))
                    .and_then(|height| height.checked_add(keep_chain_heights[index + 1]))
                    .ok_or(PaginationError::ArithmeticOverflow)?
            } else {
                own_height
            };
    }

    let mut result = Pagination {
        pages: Vec::new(),
        placements: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut page_start = 0usize;
    let mut page_y = 0i64;
    let mut previous_paragraph: Option<usize> = None;
    let mut page_paragraph_lines: Vec<(usize, usize)> = Vec::new();
    let mut break_after_pending = false;

    let new_page = |result: &mut Pagination,
                    page_start: &mut usize,
                    page_y: &mut i64,
                    page_paragraph_lines: &mut Vec<(usize, usize)>,
                    options: PaginationOptions|
     -> Result<(), PaginationError> {
        if result.placements.len() > *page_start {
            finish_page(result, page_start, page_y, page_paragraph_lines);
        }
        if result.pages.len() >= options.max_pages {
            return Err(PaginationError::LimitExceeded("pages"));
        }
        Ok(())
    };

    for (paragraph_index, ((layout, rule), &line_count)) in
        layouts.iter().zip(rules).zip(&counts).enumerate()
    {
        work = add_work(work, 1, options.max_work)?;
        if line_count == 0 {
            break_after_pending |= rule.break_after;
            continue;
        }
        let gap = previous_paragraph.map_or(0, |prev| paragraph_gap(&rules[prev], rule));
        let mut explicit_before = rule.break_before || break_after_pending;
        if explicit_before || break_after_pending {
            new_page(
                &mut result,
                &mut page_start,
                &mut page_y,
                &mut page_paragraph_lines,
                options,
            )?;
            break_after_pending = false;
        }
        if rule.keep_with_next {
            let chain = keep_chain_heights[paragraph_index];
            let explicit_boundary_after = rule.break_after
                || !rule.line_breaks_before.is_empty()
                || rules
                    .get(paragraph_index + 1)
                    .is_some_and(|next| next.break_before || !next.line_breaks_before.is_empty());
            if explicit_boundary_after {
                result
                    .diagnostics
                    .push(PaginationDiagnostic::KeepWithNextRelaxed { paragraph_index });
            } else if chain > options.page_height {
                result
                    .diagnostics
                    .push(PaginationDiagnostic::KeepChainExceedsPage { paragraph_index });
            } else if !explicit_before
                && result.placements.len() > page_start
                && page_y
                    .checked_add(gap)
                    .and_then(|v| v.checked_add(chain))
                    .ok_or(PaginationError::ArithmeticOverflow)?
                    > options.page_height
            {
                new_page(
                    &mut result,
                    &mut page_start,
                    &mut page_y,
                    &mut page_paragraph_lines,
                    options,
                )?;
            }
        }
        let paragraph_height = *prefixes[paragraph_index].last().unwrap_or(&0);
        if rule.keep_together && paragraph_height > options.page_height {
            result
                .diagnostics
                .push(PaginationDiagnostic::KeepTogetherRelaxed { paragraph_index });
        } else if rule.keep_together
            && !explicit_before
            && rule.line_breaks_before.is_empty()
            && result.placements.len() > page_start
            && page_y
                .checked_add(gap)
                .and_then(|v| v.checked_add(paragraph_height))
                .ok_or(PaginationError::ArithmeticOverflow)?
                > options.page_height
        {
            new_page(
                &mut result,
                &mut page_start,
                &mut page_y,
                &mut page_paragraph_lines,
                options,
            )?;
        }
        let mut start = 0usize;
        while start < line_count {
            work = add_work(work, 1, options.max_work)?;
            let break_position = rule.line_breaks_before.partition_point(|&i| i <= start);
            let manual = rule
                .line_breaks_before
                .get(break_position)
                .copied()
                .unwrap_or(line_count);
            let end = manual;
            // A paragraph break after is handled only after all its lines, while
            // explicit line breaks still divide its lines at their exact indices.
            let group_height = range_height(&prefixes[paragraph_index], start, end)?;
            let mut spacing = if start == 0 {
                if result.placements.len() == page_start {
                    if explicit_before {
                        rule.space_before
                    } else {
                        0
                    }
                } else {
                    gap
                }
            } else {
                0
            };
            let keep = start == 0 && rule.keep_together;
            if keep
                && !rule.line_breaks_before.is_empty()
                && paragraph_height <= options.page_height
            {
                result
                    .diagnostics
                    .push(PaginationDiagnostic::KeepTogetherRelaxed { paragraph_index });
            }
            if keep
                && page_y
                    .checked_add(spacing)
                    .and_then(|v| v.checked_add(group_height))
                    .ok_or(PaginationError::ArithmeticOverflow)?
                    > options.page_height
                && page_y > 0
                && group_height <= options.page_height
            {
                new_page(
                    &mut result,
                    &mut page_start,
                    &mut page_y,
                    &mut page_paragraph_lines,
                    options,
                )?;
                page_y = 0;
                spacing = 0;
            }
            if start == 0 {
                page_y = page_y
                    .checked_add(spacing)
                    .ok_or(PaginationError::ArithmeticOverflow)?;
            }
            let remain_height = options
                .page_height
                .checked_sub(page_y)
                .ok_or(PaginationError::ArithmeticOverflow)?;
            let mut take = start;
            let mut used = 0i64;
            while take < end {
                work = add_work(work, 1, options.max_work)?;
                let h = layout.lines[take].height;
                if used
                    .checked_add(h)
                    .ok_or(PaginationError::ArithmeticOverflow)?
                    > remain_height
                    && take > start
                {
                    break;
                }
                if used + h > remain_height && page_y > 0 {
                    break;
                }
                used = used
                    .checked_add(h)
                    .ok_or(PaginationError::ArithmeticOverflow)?;
                take += 1;
                if take < end && rule.keep_with_next && take == line_count {
                    break;
                }
            }
            if take == start {
                if page_y > 0 && result.placements.len() > page_start {
                    new_page(
                        &mut result,
                        &mut page_start,
                        &mut page_y,
                        &mut page_paragraph_lines,
                        options,
                    )?;
                    page_y = 0;
                    continue;
                }
                if page_y > 0 {
                    result
                        .diagnostics
                        .push(PaginationDiagnostic::ParagraphSpacingExceedsPage {
                            paragraph_index,
                        });
                }
                // Oversized line (or unbreakable group): make finite progress and report it.
                take += 1;
                // The line is emitted below, where any overheight line is reported.
            }
            let before_count = take - start;
            let after_count = end - take;
            let explicit_manual_boundary = end < line_count;
            if explicit_manual_boundary && line_count - end < rule.widow_min {
                result
                    .diagnostics
                    .push(PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index });
            }
            if start == 0 && after_count > 0 && before_count < rule.orphan_min {
                if page_y > 0 && result.placements.len() > page_start {
                    new_page(
                        &mut result,
                        &mut page_start,
                        &mut page_y,
                        &mut page_paragraph_lines,
                        options,
                    )?;
                    page_y = 0;
                    continue;
                }
                result
                    .diagnostics
                    .push(PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index });
            }
            if after_count > 0 && after_count < rule.widow_min && !explicit_manual_boundary {
                if take - start > rule.orphan_min {
                    let move_count = rule.widow_min - after_count;
                    take = take.saturating_sub(move_count).max(start + rule.orphan_min);
                    // `take` is adjusted below before the placement prefix is computed.
                } else {
                    result
                        .diagnostics
                        .push(PaginationDiagnostic::WidowOrphanRelaxed { paragraph_index });
                }
            }
            if after_count == 0
                && !explicit_manual_boundary
                && rule.line_breaks_before.binary_search(&start).is_err()
                && start > 0
                && take - start < rule.widow_min
                && page_y == 0
            {
                let mut same_paragraph = 0usize;
                if let Some(page) = result.pages.last() {
                    for placement in &result.placements[page.placements.clone()] {
                        work = add_work(work, 1, options.max_work)?;
                        same_paragraph += usize::from(placement.paragraph_index == paragraph_index);
                    }
                }
                let previous_page_has_room_to_give = same_paragraph > rule.orphan_min;
                let previous_is_adjacent = result.placements.last().is_some_and(|placement| {
                    placement.paragraph_index == paragraph_index
                        && placement.line_index + 1 == start
                });
                if previous_page_has_room_to_give && previous_is_adjacent {
                    let mut moved =
                        result
                            .placements
                            .pop()
                            .ok_or(PaginationError::InvalidInput(
                                "widow/orphan placement is missing",
                            ))?;
                    let previous_page = result
                        .pages
                        .last_mut()
                        .ok_or(PaginationError::InvalidInput("previous page is missing"))?;
                    previous_page.placements.end -= 1;
                    page_start -= 1;
                    moved.page_index = result.pages.len();
                    moved.top = 0;
                    moved.baseline = layout.lines[moved.line_index]
                        .baseline
                        .checked_sub(rule.space_before)
                        .and_then(|v| v.checked_sub(prefixes[paragraph_index][moved.line_index]))
                        .ok_or(PaginationError::ArithmeticOverflow)?;
                    page_y = layout.lines[moved.line_index].height;
                    result.placements.push(moved);
                }
            }
            let prefix = range_height(&prefixes[paragraph_index], start, take)?;
            let mut line_top = page_y;
            for (line_index, line) in layout.lines.iter().enumerate().take(take).skip(start) {
                if line.height > options.page_height {
                    result
                        .diagnostics
                        .push(PaginationDiagnostic::LineExceedsPage {
                            paragraph_index,
                            line_index,
                        });
                }
                let prefix_line = prefixes[paragraph_index][line_index];
                let local_baseline = line
                    .baseline
                    .checked_sub(rule.space_before)
                    .and_then(|v| v.checked_sub(prefix_line))
                    .ok_or(PaginationError::ArithmeticOverflow)?;
                result.placements.push(PlacedLine {
                    page_index: result.pages.len(),
                    paragraph_index,
                    line_index,
                    top: line_top,
                    baseline: line_top
                        .checked_add(local_baseline)
                        .ok_or(PaginationError::ArithmeticOverflow)?,
                });
                line_top = line_top
                    .checked_add(line.height)
                    .ok_or(PaginationError::ArithmeticOverflow)?;
                work = add_work(work, 1, options.max_work)?;
            }
            page_paragraph_lines.push((paragraph_index, take - start));
            page_y = page_y
                .checked_add(prefix)
                .ok_or(PaginationError::ArithmeticOverflow)?;
            start = take;
            if start < line_count {
                new_page(
                    &mut result,
                    &mut page_start,
                    &mut page_y,
                    &mut page_paragraph_lines,
                    options,
                )?;
                page_y = 0;
                explicit_before = false;
            }
        }
        break_after_pending |= rule.break_after && has_following_nonempty[paragraph_index];
        previous_paragraph = Some(paragraph_index);
    }
    if result.placements.len() > page_start {
        finish_page(
            &mut result,
            &mut page_start,
            &mut page_y,
            &mut page_paragraph_lines,
        );
    }
    if result.pages.len() > options.max_pages {
        return Err(PaginationError::LimitExceeded("pages"));
    }
    Ok(result)
}

fn paragraph_gap(previous: &ParagraphRules, next: &ParagraphRules) -> i64 {
    let mut gap = previous.space_after.max(next.space_before);
    if previous.style_id == next.style_id {
        if previous.contextual_spacing {
            gap = gap.saturating_sub(previous.space_after);
        }
        if next.contextual_spacing {
            gap = gap.saturating_sub(next.space_before);
        }
    }
    gap.max(0)
}

fn range_height(prefix: &[i64], start: usize, end: usize) -> Result<i64, PaginationError> {
    prefix
        .get(end)
        .copied()
        .zip(prefix.get(start).copied())
        .and_then(|(end, start)| end.checked_sub(start))
        .ok_or(PaginationError::ArithmeticOverflow)
}

fn add_work(current: usize, amount: usize, maximum: usize) -> Result<usize, PaginationError> {
    let next = current
        .checked_add(amount)
        .ok_or(PaginationError::ArithmeticOverflow)?;
    if next > maximum {
        Err(PaginationError::LimitExceeded("work"))
    } else {
        Ok(next)
    }
}

fn finish_page(
    result: &mut Pagination,
    page_start: &mut usize,
    page_y: &mut i64,
    page_paragraph_lines: &mut Vec<(usize, usize)>,
) {
    result.pages.push(Page {
        placements: *page_start..result.placements.len(),
    });
    *page_start = result.placements.len();
    *page_y = 0;
    page_paragraph_lines.clear();
}
