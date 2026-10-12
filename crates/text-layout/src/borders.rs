//! Typed paragraph border policies and bounded page-local border geometry.
// SPDX-License-Identifier: AGPL-3.0-or-later
//!
//! Lengths in this module use 1/64 typographic point, matching rich layout and
//! pagination. The module deliberately returns geometry rather than renderer
//! commands so that text layout remains independent from `atelier-render`.

use crate::pagination::{Pagination, PlacedLine};
use crate::{LayoutLine, ParagraphLayout};
use atelier_text_model::rich::Color;
use core::fmt;
use std::error::Error;

/// A paragraph column in page-local layout units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnBounds {
    pub x: i64,
    pub width: i64,
}

impl ColumnBounds {
    pub fn new(x: i64, width: i64) -> Result<Self, BorderPlanError> {
        if width <= 0 {
            return Err(BorderPlanError::InvalidInput(
                "column width must be positive",
            ));
        }
        x.checked_add(width)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        Ok(Self { x, width })
    }
}

/// A visible paragraph border style. Dash and dot patterns are expanded into
/// geometry by the planner; the renderer therefore needs no dash extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderStyle {
    Solid,
    Double,
    Dashed,
    Dotted,
}

/// One typed border edge. `width` and `spacing` are 1/64 pt; width is positive
/// and spacing is the distance from the paragraph content box to the nearest
/// edge of the stroke.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorderLine {
    pub width: i64,
    pub color: Color,
    pub style: BorderStyle,
    pub spacing: i64,
}

impl BorderLine {
    pub fn new(
        width: i64,
        color: Color,
        style: BorderStyle,
        spacing: i64,
    ) -> Result<Self, BorderPlanError> {
        if width <= 0 {
            return Err(BorderPlanError::InvalidPolicy(
                "border width must be positive",
            ));
        }
        if spacing < 0 {
            return Err(BorderPlanError::InvalidPolicy(
                "border spacing cannot be negative",
            ));
        }
        if style == BorderStyle::Double && width < 3 {
            return Err(BorderPlanError::InvalidPolicy(
                "double border width must be at least three layout units",
            ));
        }
        Ok(Self {
            width,
            color,
            style,
            spacing,
        })
    }
}

/// Border and shading policy kept separate from public `ParaProps` literals.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParagraphBorderPolicy {
    pub top: Option<BorderLine>,
    pub bottom: Option<BorderLine>,
    pub left: Option<BorderLine>,
    pub right: Option<BorderLine>,
    pub between: Option<BorderLine>,
    /// RGB shading; it never changes border-group equivalence.
    pub shading: Option<Color>,
}

impl ParagraphBorderPolicy {
    fn has_frame(&self) -> bool {
        self.top.is_some()
            || self.bottom.is_some()
            || self.left.is_some()
            || self.right.is_some()
            || self.between.is_some()
    }

    fn same_frame(&self, other: &Self) -> bool {
        self.top == other.top
            && self.bottom == other.bottom
            && self.left == other.left
            && self.right == other.right
            && self.between == other.between
    }
}

/// All rendering inputs for one laid-out paragraph. `column` is explicit and
/// must describe the available paragraph column, not the widest text line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParagraphBorderInput {
    pub column: ColumnBounds,
    pub policy: ParagraphBorderPolicy,
}

/// Resource ceilings for border planning. Every counter is checked before the
/// corresponding output is appended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorderPlanLimits {
    pub max_work: usize,
    pub max_fragments: usize,
    pub max_primitives: usize,
}

impl Default for BorderPlanLimits {
    fn default() -> Self {
        Self {
            max_work: 4_000_000,
            max_fragments: 1_000_000,
            max_primitives: 1_000_000,
        }
    }
}

/// Integer page-local point in 1/64 pt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutPoint {
    pub x: i64,
    pub y: i64,
}

/// Rectangle in page-local 1/64 pt coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// Geometry emitted by the planner. A dot is a round filled mark, while a line
/// uses a butt cap; this keeps dotted strokes circular in both existing backends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderPrimitive {
    Fill {
        rect: LayoutRect,
        color: Color,
    },
    Stroke {
        start: LayoutPoint,
        end: LayoutPoint,
        width: i64,
        color: Color,
    },
    Dot {
        center: LayoutPoint,
        diameter: i64,
        color: Color,
    },
}

/// Page-local primitives in paint order: all paragraph fills first, then all
/// border strokes and dots. `page_index` addresses the source `Pagination`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BorderPagePlan {
    pub page_index: usize,
    pub primitives: Vec<BorderPrimitive>,
}

/// Complete validated plan. No partial plan is returned on failure.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParagraphBorderPlan {
    pub pages: Vec<BorderPagePlan>,
}

/// Invalid policy or geometry, exceeded work/output limits, or checked
/// arithmetic/allocation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BorderPlanError {
    InvalidInput(&'static str),
    InvalidPolicy(&'static str),
    LimitExceeded(&'static str),
    ArithmeticOverflow,
    AllocationFailed,
}

impl fmt::Display for BorderPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid paragraph border input: {message}"),
            Self::InvalidPolicy(message) => write!(f, "invalid paragraph border policy: {message}"),
            Self::LimitExceeded(name) => write!(f, "paragraph border {name} limit exceeded"),
            Self::ArithmeticOverflow => f.write_str("paragraph border arithmetic overflow"),
            Self::AllocationFailed => f.write_str("paragraph border allocation failed"),
        }
    }
}
impl Error for BorderPlanError {}

#[derive(Clone, Copy)]
struct LineGeometry {
    paragraph_index: usize,
    line_index: usize,
    page_index: usize,
    top: i64,
    bottom: i64,
}

#[derive(Clone, Copy)]
struct ParagraphFragment {
    paragraph_index: usize,
    page_index: usize,
    top: i64,
    bottom: i64,
}

/// Build page-local shading and border geometry for a complete pagination.
/// Paragraphs are grouped only when their source indices are consecutive on
/// that page, their border policies match (shading is ignored), and their
/// horizontal column bounds match. A multi-page group becomes one closed frame
/// per page. Empty paragraphs and paragraphs without any border interrupt a
/// group.
pub fn plan_paragraph_borders(
    layouts: &[ParagraphLayout],
    pagination: &Pagination,
    inputs: &[ParagraphBorderInput],
    limits: BorderPlanLimits,
) -> Result<ParagraphBorderPlan, BorderPlanError> {
    if layouts.len() != inputs.len() {
        return Err(BorderPlanError::InvalidInput(
            "border inputs must match paragraph count",
        ));
    }
    if limits.max_work == 0 || limits.max_fragments == 0 || limits.max_primitives == 0 {
        return Err(BorderPlanError::InvalidInput("budgets must be positive"));
    }
    if layouts.len() > limits.max_work
        || pagination.pages.len() > limits.max_work
        || pagination.placements.len() > limits.max_work
    {
        return Err(BorderPlanError::LimitExceeded("work"));
    }
    for input in inputs {
        ColumnBounds::new(input.column.x, input.column.width)?;
        validate_policy(&input.policy)?;
    }
    let mut work = 0usize;
    let mut lines_by_paragraph: Vec<Vec<LineGeometry>> = Vec::new();
    reserve_exact(&mut lines_by_paragraph, layouts.len())?;
    let mut expected_lines = 0usize;
    for layout in layouts {
        work = add_work(work, 1, limits.max_work)?;
        if layout.width < 0 || layout.height < 0 {
            return Err(BorderPlanError::InvalidInput(
                "paragraph dimensions cannot be negative",
            ));
        }
        let mut prior_source = None;
        for line in &layout.lines {
            work = add_work(work, 1, limits.max_work)?;
            validate_layout_line(line, prior_source)?;
            prior_source = Some(line.source.clone());
            expected_lines = expected_lines
                .checked_add(1)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
        }
        lines_by_paragraph.push(Vec::new());
    }

    if expected_lines != pagination.placements.len() {
        return Err(BorderPlanError::InvalidInput(
            "pagination must conserve every layout line exactly once",
        ));
    }
    validate_page_ranges(pagination)?;
    let mut seen: Vec<Vec<bool>> = Vec::new();
    reserve_exact(&mut seen, layouts.len())?;
    for layout in layouts {
        let mut row = Vec::new();
        reserve_exact(&mut row, layout.lines.len())?;
        row.resize(layout.lines.len(), false);
        seen.push(row);
    }

    let mut previous_source: Option<(usize, usize)> = None;
    for (page_index, page) in pagination.pages.iter().enumerate() {
        for placement_index in page.placements.clone() {
            work = add_work(work, 1, limits.max_work)?;
            let placement =
                pagination
                    .placements
                    .get(placement_index)
                    .ok_or(BorderPlanError::InvalidInput(
                        "page placement range is outside pagination",
                    ))?;
            validate_placement(placement, page_index, layouts, &mut seen)?;
            let key = (placement.paragraph_index, placement.line_index);
            if previous_source.is_some_and(|previous| previous >= key) {
                return Err(BorderPlanError::InvalidInput(
                    "placements must preserve strict paragraph and line source order",
                ));
            }
            previous_source = Some(key);
            let line = &layouts[placement.paragraph_index].lines[placement.line_index];
            let bottom = placement
                .top
                .checked_add(line.height)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            let _baseline = placement
                .baseline
                .checked_add(line.height)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            lines_by_paragraph[placement.paragraph_index].push(LineGeometry {
                paragraph_index: placement.paragraph_index,
                line_index: placement.line_index,
                page_index,
                top: placement.top,
                bottom,
            });
        }
    }
    if seen.iter().any(|row| row.iter().any(|seen| !seen)) {
        return Err(BorderPlanError::InvalidInput(
            "pagination is missing a layout line",
        ));
    }
    let fragments = paragraph_fragments(&lines_by_paragraph, limits, &mut work)?;
    let mut plan = ParagraphBorderPlan::default();
    let mut primitive_count = 0usize;
    reserve_exact(&mut plan.pages, pagination.pages.len())?;
    work = add_work(work, pagination.pages.len(), limits.max_work)?;
    for page_index in 0..pagination.pages.len() {
        plan.pages.push(BorderPagePlan {
            page_index,
            primitives: Vec::new(),
        });
    }

    let mut grouped = vec![false; fragments.len()];
    // Per-paragraph shading remains independent even inside a shared frame.
    for fragment in &fragments {
        work = add_work(work, 1, limits.max_work)?;
        let input = &inputs[fragment.paragraph_index];
        if let Some(color) = input.policy.shading {
            push_primitive(
                &mut plan.pages[fragment.page_index].primitives,
                BorderPrimitive::Fill {
                    rect: content_rect(fragment, input.column)?,
                    color,
                },
                limits,
                &mut primitive_count,
                &mut work,
            )?;
        }
    }

    for index in 0..fragments.len() {
        work = add_work(work, 1, limits.max_work)?;
        if grouped[index] {
            continue;
        }
        let first = fragments[index];
        let first_input = &inputs[first.paragraph_index];
        if !first_input.policy.has_frame() {
            continue;
        }
        let mut end = index + 1;
        while end < fragments.len() {
            work = add_work(work, 1, limits.max_work)?;
            let previous = fragments[end - 1];
            let next = fragments[end];
            if next.page_index != first.page_index
                || next.paragraph_index != previous.paragraph_index + 1
                || !can_share_frame(first, previous, next, first_input, inputs)
            {
                break;
            }
            end += 1;
        }
        for item in &mut grouped[index..end] {
            *item = true;
        }
        emit_group(
            &mut plan.pages[first.page_index].primitives,
            &fragments[index..end],
            inputs,
            limits,
            &mut primitive_count,
            &mut work,
        )?;
    }
    Ok(plan)
}

fn validate_policy(policy: &ParagraphBorderPolicy) -> Result<(), BorderPlanError> {
    for line in [
        policy.top,
        policy.bottom,
        policy.left,
        policy.right,
        policy.between,
    ]
    .into_iter()
    .flatten()
    {
        BorderLine::new(line.width, line.color, line.style, line.spacing)?;
    }
    Ok(())
}

fn validate_layout_line(
    line: &LayoutLine,
    previous_source: Option<core::ops::Range<usize>>,
) -> Result<(), BorderPlanError> {
    if line.height <= 0 {
        return Err(BorderPlanError::InvalidInput(
            "line height must be positive",
        ));
    }
    if line.source.start > line.source.end
        || previous_source
            .is_some_and(|range| line.source == range || line.source.start < range.end)
        || line.width < 0
        || line.x.checked_add(line.width).is_none()
        || line.baseline.checked_add(line.height).is_none()
    {
        return Err(BorderPlanError::InvalidInput(
            "layout line geometry or source order is invalid",
        ));
    }
    Ok(())
}

fn validate_page_ranges(pagination: &Pagination) -> Result<(), BorderPlanError> {
    let mut cursor = 0usize;
    for page in &pagination.pages {
        if page.placements.start != cursor
            || page.placements.start > page.placements.end
            || page.placements.end > pagination.placements.len()
        {
            return Err(BorderPlanError::InvalidInput(
                "page placement ranges must be ordered, contiguous, and in bounds",
            ));
        }
        cursor = page.placements.end;
    }
    if cursor != pagination.placements.len() {
        return Err(BorderPlanError::InvalidInput(
            "page ranges must cover all placements",
        ));
    }
    Ok(())
}

fn validate_placement(
    placement: &PlacedLine,
    page_index: usize,
    layouts: &[ParagraphLayout],
    seen: &mut [Vec<bool>],
) -> Result<(), BorderPlanError> {
    if placement.page_index != page_index {
        return Err(BorderPlanError::InvalidInput(
            "placement page index does not match its page range",
        ));
    }
    let row = seen
        .get_mut(placement.paragraph_index)
        .ok_or(BorderPlanError::InvalidInput(
            "placement paragraph index is out of range",
        ))?;
    let occupied = row
        .get_mut(placement.line_index)
        .ok_or(BorderPlanError::InvalidInput(
            "placement line index is out of range",
        ))?;
    if *occupied {
        return Err(BorderPlanError::InvalidInput(
            "pagination contains a duplicate line placement",
        ));
    }
    *occupied = true;
    let line = &layouts[placement.paragraph_index].lines[placement.line_index];
    if placement.top.checked_add(line.height).is_none()
        || placement.baseline.checked_add(line.height).is_none()
    {
        return Err(BorderPlanError::ArithmeticOverflow);
    }
    Ok(())
}

fn paragraph_fragments(
    lines_by_paragraph: &[Vec<LineGeometry>],
    limits: BorderPlanLimits,
    work: &mut usize,
) -> Result<Vec<ParagraphFragment>, BorderPlanError> {
    let mut fragments = Vec::new();
    for (paragraph_index, lines) in lines_by_paragraph.iter().enumerate() {
        let mut current: Option<ParagraphFragment> = None;
        let mut previous_line_index = None;
        for line in lines {
            *work = add_work(*work, 1, limits.max_work)?;
            if line.paragraph_index != paragraph_index
                || previous_line_index.is_some_and(|previous| line.line_index <= previous)
            {
                return Err(BorderPlanError::InvalidInput(
                    "paragraph line placements must preserve source order",
                ));
            }
            previous_line_index = Some(line.line_index);
            match current {
                Some(mut fragment) if fragment.page_index == line.page_index => {
                    if line.top != fragment.bottom {
                        return Err(BorderPlanError::InvalidInput(
                            "paragraph line boxes must be contiguous and ordered on a page",
                        ));
                    }
                    fragment.bottom = line.bottom;
                    current = Some(fragment);
                }
                Some(fragment) => {
                    push_fragment(&mut fragments, fragment, limits)?;
                    current = Some(ParagraphFragment {
                        paragraph_index,
                        page_index: line.page_index,
                        top: line.top,
                        bottom: line.bottom,
                    });
                }
                None => {
                    current = Some(ParagraphFragment {
                        paragraph_index,
                        page_index: line.page_index,
                        top: line.top,
                        bottom: line.bottom,
                    });
                }
            }
        }
        if let Some(fragment) = current {
            push_fragment(&mut fragments, fragment, limits)?;
        }
    }
    Ok(fragments)
}

fn push_fragment(
    fragments: &mut Vec<ParagraphFragment>,
    fragment: ParagraphFragment,
    limits: BorderPlanLimits,
) -> Result<(), BorderPlanError> {
    if fragment.top > fragment.bottom {
        return Err(BorderPlanError::InvalidInput("fragment height is negative"));
    }
    if fragments.len() >= limits.max_fragments {
        return Err(BorderPlanError::LimitExceeded("fragments"));
    }
    reserve_exact(fragments, 1)?;
    fragments.push(fragment);
    Ok(())
}

fn can_share_frame(
    first: ParagraphFragment,
    previous: ParagraphFragment,
    next: ParagraphFragment,
    first_input: &ParagraphBorderInput,
    inputs: &[ParagraphBorderInput],
) -> bool {
    let next_input = &inputs[next.paragraph_index];
    first.page_index == next.page_index
        && previous.paragraph_index + 1 == next.paragraph_index
        && next_input.policy.has_frame()
        && first_input.column == next_input.column
        && first_input.policy.same_frame(&next_input.policy)
}

fn content_rect(
    fragment: &ParagraphFragment,
    column: ColumnBounds,
) -> Result<LayoutRect, BorderPlanError> {
    let height = fragment
        .bottom
        .checked_sub(fragment.top)
        .ok_or(BorderPlanError::ArithmeticOverflow)?;
    if column.width <= 0 || height <= 0 {
        return Err(BorderPlanError::InvalidInput(
            "shading rectangles must have positive dimensions",
        ));
    }
    Ok(LayoutRect {
        x: column.x,
        y: fragment.top,
        width: column.width,
        height,
    })
}

fn emit_group(
    out: &mut Vec<BorderPrimitive>,
    fragments: &[ParagraphFragment],
    inputs: &[ParagraphBorderInput],
    limits: BorderPlanLimits,
    primitive_count: &mut usize,
    work: &mut usize,
) -> Result<(), BorderPlanError> {
    let first = *fragments
        .first()
        .ok_or(BorderPlanError::InvalidInput("empty border group"))?;
    let last = *fragments
        .last()
        .ok_or(BorderPlanError::InvalidInput("empty border group"))?;
    let input = &inputs[first.paragraph_index];
    let policy = &input.policy;
    let column = input.column;
    let content_bottom = last.bottom;
    let frame_top = match policy.top {
        Some(border) => subtract(first.top, line_offset(border)?)?,
        None => first.top,
    };
    let frame_bottom = match policy.bottom {
        Some(border) => add(content_bottom, line_offset(border)?)?,
        None => content_bottom,
    };
    let horizontal_start = match policy.left {
        Some(line) => subtract(column.x, line_offset(line)?)?,
        None => column.x,
    };
    let column_right = column
        .x
        .checked_add(column.width)
        .ok_or(BorderPlanError::ArithmeticOverflow)?;
    let horizontal_end = match policy.right {
        Some(line) => add(column_right, line_offset(line)?)?,
        None => column_right,
    };

    if let Some(line) = policy.top {
        let center_y = subtract(first.top, line_offset(line)?)?;
        emit_horizontal(
            out,
            horizontal_start,
            horizontal_end,
            center_y,
            line,
            limits,
            primitive_count,
            work,
        )?;
    }
    if let Some(line) = policy.bottom {
        let center_y = add(content_bottom, line_offset(line)?)?;
        emit_horizontal(
            out,
            horizontal_start,
            horizontal_end,
            center_y,
            line,
            limits,
            primitive_count,
            work,
        )?;
    }
    if let Some(line) = policy.left {
        let center_x = subtract(column.x, line_offset(line)?)?;
        if frame_top >= frame_bottom {
            return Err(BorderPlanError::InvalidInput(
                "frame height must be positive",
            ));
        }
        emit_vertical(
            out,
            center_x,
            frame_top,
            frame_bottom,
            line,
            limits,
            primitive_count,
            work,
        )?;
    }
    if let Some(line) = policy.right {
        let right = column
            .x
            .checked_add(column.width)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        let center_x = add(right, line_offset(line)?)?;
        if frame_top >= frame_bottom {
            return Err(BorderPlanError::InvalidInput(
                "frame height must be positive",
            ));
        }
        emit_vertical(
            out,
            center_x,
            frame_top,
            frame_bottom,
            line,
            limits,
            primitive_count,
            work,
        )?;
    }
    if let Some(line) = policy.between {
        for pair in fragments.windows(2) {
            *work = add_work(*work, 1, limits.max_work)?;
            let previous = pair[0];
            let next = pair[1];
            if previous.paragraph_index + 1 != next.paragraph_index {
                continue;
            }
            let gap = next
                .top
                .checked_sub(previous.bottom)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            if gap < 0 {
                return Err(BorderPlanError::InvalidInput(
                    "adjacent paragraph line boxes overlap",
                ));
            }
            let offset = line_offset(line)?;
            let required_gap = line
                .spacing
                .checked_add(line.width)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            if required_gap > gap {
                return Err(BorderPlanError::InvalidInput(
                    "paragraph gap cannot contain the between border spacing",
                ));
            }
            let center_y = previous
                .bottom
                .checked_add(offset)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            emit_horizontal(
                out,
                column.x,
                column
                    .x
                    .checked_add(column.width)
                    .ok_or(BorderPlanError::ArithmeticOverflow)?,
                center_y,
                line,
                limits,
                primitive_count,
                work,
            )?;
        }
    }
    Ok(())
}

fn line_offset(line: BorderLine) -> Result<i64, BorderPlanError> {
    line.spacing
        .checked_add(line.width / 2)
        .ok_or(BorderPlanError::ArithmeticOverflow)
}

#[allow(clippy::too_many_arguments)] // Keep budget accounting explicit at the geometry boundary.
fn emit_horizontal(
    out: &mut Vec<BorderPrimitive>,
    x1: i64,
    x2: i64,
    y: i64,
    line: BorderLine,
    limits: BorderPlanLimits,
    primitive_count: &mut usize,
    work: &mut usize,
) -> Result<(), BorderPlanError> {
    if x1 >= x2 {
        return Err(BorderPlanError::InvalidInput(
            "border line length must be positive",
        ));
    }
    emit_styled_line(
        out,
        LayoutPoint { x: x1, y },
        LayoutPoint { x: x2, y },
        line,
        limits,
        primitive_count,
        work,
    )
}

#[allow(clippy::too_many_arguments)] // Keep budget accounting explicit at the geometry boundary.
fn emit_vertical(
    out: &mut Vec<BorderPrimitive>,
    x: i64,
    y1: i64,
    y2: i64,
    line: BorderLine,
    limits: BorderPlanLimits,
    primitive_count: &mut usize,
    work: &mut usize,
) -> Result<(), BorderPlanError> {
    if y1 >= y2 {
        return Err(BorderPlanError::InvalidInput(
            "border line length must be positive",
        ));
    }
    emit_styled_line(
        out,
        LayoutPoint { x, y: y1 },
        LayoutPoint { x, y: y2 },
        line,
        limits,
        primitive_count,
        work,
    )
}

fn emit_styled_line(
    out: &mut Vec<BorderPrimitive>,
    start: LayoutPoint,
    end: LayoutPoint,
    line: BorderLine,
    limits: BorderPlanLimits,
    primitive_count: &mut usize,
    work: &mut usize,
) -> Result<(), BorderPlanError> {
    match line.style {
        BorderStyle::Solid => push_primitive(
            out,
            BorderPrimitive::Stroke {
                start,
                end,
                width: line.width,
                color: line.color,
            },
            limits,
            primitive_count,
            work,
        ),
        BorderStyle::Double => {
            let inner_width = line.width / 3;
            let offset = (line.width - inner_width) / 2;
            let (first_start, first_end, second_start, second_end) =
                offset_line(start, end, offset)?;
            push_primitive(
                out,
                BorderPrimitive::Stroke {
                    start: first_start,
                    end: first_end,
                    width: inner_width,
                    color: line.color,
                },
                limits,
                primitive_count,
                work,
            )?;
            push_primitive(
                out,
                BorderPrimitive::Stroke {
                    start: second_start,
                    end: second_end,
                    width: inner_width,
                    color: line.color,
                },
                limits,
                primitive_count,
                work,
            )
        }
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let horizontal = start.y == end.y;
            let total = if horizontal {
                end.x.checked_sub(start.x)
            } else if start.x == end.x {
                end.y.checked_sub(start.y)
            } else {
                None
            }
            .ok_or(BorderPlanError::InvalidInput(
                "border strokes must be horizontal or vertical",
            ))?;
            let unit = match line.style {
                BorderStyle::Dashed => line.width.checked_mul(5),
                BorderStyle::Dotted => Some(line.width),
                _ => unreachable!(),
            }
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
            let gap = line
                .width
                .checked_mul(2)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            let period = unit
                .checked_add(gap)
                .ok_or(BorderPlanError::ArithmeticOverflow)?;
            let max_count = usize::try_from(
                total
                    .checked_sub(1)
                    .ok_or(BorderPlanError::InvalidInput(
                        "border line length must be positive",
                    ))?
                    .div_euclid(period)
                    .checked_add(1)
                    .ok_or(BorderPlanError::ArithmeticOverflow)?,
            )
            .map_err(|_| BorderPlanError::ArithmeticOverflow)?;
            if primitive_count
                .checked_add(max_count)
                .is_none_or(|next| next > limits.max_primitives)
            {
                return Err(BorderPlanError::LimitExceeded("primitives"));
            }
            out.try_reserve_exact(max_count)
                .map_err(|_| BorderPlanError::AllocationFailed)?;
            let mut distance = 0i64;
            while distance < total {
                *work = add_work(*work, 1, limits.max_work)?;
                let end_distance = distance
                    .checked_add(unit)
                    .ok_or(BorderPlanError::ArithmeticOverflow)?
                    .min(total);
                if line.style == BorderStyle::Dotted {
                    let center_distance = distance
                        .checked_add(line.width / 2)
                        .ok_or(BorderPlanError::ArithmeticOverflow)?
                        .min(total);
                    let center = if horizontal {
                        LayoutPoint {
                            x: start.x + center_distance,
                            y: start.y,
                        }
                    } else {
                        LayoutPoint {
                            x: start.x,
                            y: start.y + center_distance,
                        }
                    };
                    push_primitive(
                        out,
                        BorderPrimitive::Dot {
                            center,
                            diameter: line.width,
                            color: line.color,
                        },
                        limits,
                        primitive_count,
                        work,
                    )?;
                } else {
                    let piece_start = if horizontal {
                        LayoutPoint {
                            x: start.x + distance,
                            y: start.y,
                        }
                    } else {
                        LayoutPoint {
                            x: start.x,
                            y: start.y + distance,
                        }
                    };
                    let piece_end = if horizontal {
                        LayoutPoint {
                            x: start.x + end_distance,
                            y: start.y,
                        }
                    } else {
                        LayoutPoint {
                            x: start.x,
                            y: start.y + end_distance,
                        }
                    };
                    push_primitive(
                        out,
                        BorderPrimitive::Stroke {
                            start: piece_start,
                            end: piece_end,
                            width: line.width,
                            color: line.color,
                        },
                        limits,
                        primitive_count,
                        work,
                    )?;
                }
                distance = distance
                    .checked_add(period)
                    .ok_or(BorderPlanError::ArithmeticOverflow)?;
            }
            Ok(())
        }
    }
}

fn offset_line(
    start: LayoutPoint,
    end: LayoutPoint,
    offset: i64,
) -> Result<(LayoutPoint, LayoutPoint, LayoutPoint, LayoutPoint), BorderPlanError> {
    if start.y == end.y {
        let first_y = start
            .y
            .checked_sub(offset)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        let second_y = start
            .y
            .checked_add(offset)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        Ok((
            LayoutPoint {
                x: start.x,
                y: first_y,
            },
            LayoutPoint {
                x: end.x,
                y: first_y,
            },
            LayoutPoint {
                x: start.x,
                y: second_y,
            },
            LayoutPoint {
                x: end.x,
                y: second_y,
            },
        ))
    } else {
        let first_x = start
            .x
            .checked_sub(offset)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        let second_x = start
            .x
            .checked_add(offset)
            .ok_or(BorderPlanError::ArithmeticOverflow)?;
        Ok((
            LayoutPoint {
                x: first_x,
                y: start.y,
            },
            LayoutPoint {
                x: first_x,
                y: end.y,
            },
            LayoutPoint {
                x: second_x,
                y: start.y,
            },
            LayoutPoint {
                x: second_x,
                y: end.y,
            },
        ))
    }
}

fn add(a: i64, b: i64) -> Result<i64, BorderPlanError> {
    a.checked_add(b).ok_or(BorderPlanError::ArithmeticOverflow)
}

fn subtract(a: i64, b: i64) -> Result<i64, BorderPlanError> {
    a.checked_sub(b).ok_or(BorderPlanError::ArithmeticOverflow)
}

fn add_work(current: usize, amount: usize, maximum: usize) -> Result<usize, BorderPlanError> {
    let next = current
        .checked_add(amount)
        .ok_or(BorderPlanError::ArithmeticOverflow)?;
    if next > maximum {
        Err(BorderPlanError::LimitExceeded("work"))
    } else {
        Ok(next)
    }
}

fn push_primitive(
    output: &mut Vec<BorderPrimitive>,
    primitive: BorderPrimitive,
    limits: BorderPlanLimits,
    primitive_count: &mut usize,
    work: &mut usize,
) -> Result<(), BorderPlanError> {
    let next_count = primitive_count
        .checked_add(1)
        .ok_or(BorderPlanError::ArithmeticOverflow)?;
    if next_count > limits.max_primitives {
        return Err(BorderPlanError::LimitExceeded("primitives"));
    }
    *work = add_work(*work, 1, limits.max_work)?;
    reserve_exact(output, 1)?;
    output.push(primitive);
    *primitive_count = next_count;
    Ok(())
}

fn reserve_exact<T>(values: &mut Vec<T>, additional: usize) -> Result<(), BorderPlanError> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| BorderPlanError::AllocationFailed)
}
