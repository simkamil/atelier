// SPDX-License-Identifier: AGPL-3.0-or-later
//! Adapter from typed rich-layout paragraph border geometry to display-list
//! commands. All unit conversion happens here: layout values are 1/64 pt and
//! display-list coordinates are typographic points.

use crate::{
    Color, Command, DisplayListError, DisplayPath, FillRule, LineCap, LineJoin, PathElement,
    StrokeStyle, limits,
};
use atelier_geom::Point;
use atelier_text_layout::borders::{
    BorderPagePlan, BorderPrimitive, LayoutPoint, LayoutRect, ParagraphBorderPlan,
};

/// Convert one page of a validated paragraph border plan into render commands.
/// The returned vector is complete on success and empty on any error; callers
/// can append it to their text commands after this function succeeds.
pub fn paragraph_border_commands(
    plan: &ParagraphBorderPlan,
    page_index: usize,
) -> Result<Vec<Command>, DisplayListError> {
    if plan
        .pages
        .iter()
        .enumerate()
        .any(|(index, page)| page.page_index != index)
    {
        return Err(DisplayListError::InvalidValue);
    }
    let page = plan
        .pages
        .iter()
        .find(|page| page.page_index == page_index)
        .ok_or(DisplayListError::InvalidValue)?;
    validate_page_plan(page)?;
    let path_elements = page.primitives.iter().try_fold(0usize, |total, primitive| {
        total.checked_add(match primitive {
            BorderPrimitive::Fill { .. } => 5,
            BorderPrimitive::Stroke { .. } => 2,
            BorderPrimitive::Dot { .. } => 6,
        })
    });
    if page.primitives.len() > limits::MAX_COMMANDS
        || path_elements.is_none_or(|count| count > limits::MAX_PATH_ELEMENTS)
    {
        return Err(DisplayListError::LimitExceeded);
    }

    let mut commands = Vec::new();
    commands
        .try_reserve_exact(page.primitives.len())
        .map_err(|_| DisplayListError::LimitExceeded)?;
    for primitive in &page.primitives {
        commands.push(match *primitive {
            BorderPrimitive::Fill { rect, color } => Command::FillPath {
                path: rectangle_path(rect)?,
                color: render_color(color),
                rule: FillRule::NonZero,
            },
            BorderPrimitive::Stroke {
                start,
                end,
                width,
                color,
            } => {
                if width <= 0 {
                    return Err(DisplayListError::InvalidValue);
                }
                Command::StrokePath {
                    path: DisplayPath::new(vec![
                        PathElement::MoveTo(point(start)?),
                        PathElement::LineTo(point(end)?),
                    ])?,
                    color: render_color(color),
                    style: StrokeStyle::new(units(width), LineCap::Butt, LineJoin::Miter, 4.0)?,
                }
            }
            BorderPrimitive::Dot {
                center,
                diameter,
                color,
            } => {
                if diameter <= 0 {
                    return Err(DisplayListError::InvalidValue);
                }
                Command::FillPath {
                    path: circle_path(center, diameter)?,
                    color: render_color(color),
                    rule: FillRule::NonZero,
                }
            }
        });
    }
    Ok(commands)
}

fn validate_page_plan(page: &BorderPagePlan) -> Result<(), DisplayListError> {
    if page.primitives.len() > limits::MAX_COMMANDS {
        return Err(DisplayListError::LimitExceeded);
    }
    for primitive in &page.primitives {
        match *primitive {
            BorderPrimitive::Fill { rect, .. } => {
                if rect.width <= 0 || rect.height <= 0 {
                    return Err(DisplayListError::InvalidValue);
                }
                rect.x
                    .checked_add(rect.width)
                    .and_then(|_| rect.y.checked_add(rect.height))
                    .ok_or(DisplayListError::InvalidValue)?;
            }
            BorderPrimitive::Stroke {
                start, end, width, ..
            } => {
                if width <= 0 || (start.x == end.x && start.y == end.y) {
                    return Err(DisplayListError::InvalidValue);
                }
                let _ = point(start)?;
                let _ = point(end)?;
            }
            BorderPrimitive::Dot {
                center, diameter, ..
            } => {
                if diameter <= 0 {
                    return Err(DisplayListError::InvalidValue);
                }
                let radius = diameter / 2;
                center
                    .x
                    .checked_add(radius)
                    .and_then(|_| center.x.checked_sub(radius))
                    .and_then(|_| center.y.checked_add(radius))
                    .and_then(|_| center.y.checked_sub(radius))
                    .ok_or(DisplayListError::InvalidValue)?;
                let _ = point(center)?;
            }
        }
    }
    Ok(())
}

fn rectangle_path(rect: LayoutRect) -> Result<DisplayPath, DisplayListError> {
    if rect.width <= 0 || rect.height <= 0 {
        return Err(DisplayListError::InvalidValue);
    }
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or(DisplayListError::InvalidValue)?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or(DisplayListError::InvalidValue)?;
    DisplayPath::new(vec![
        PathElement::MoveTo(point(LayoutPoint {
            x: rect.x,
            y: rect.y,
        })?),
        PathElement::LineTo(point(LayoutPoint {
            x: right,
            y: rect.y,
        })?),
        PathElement::LineTo(point(LayoutPoint {
            x: right,
            y: bottom,
        })?),
        PathElement::LineTo(point(LayoutPoint {
            x: rect.x,
            y: bottom,
        })?),
        PathElement::Close,
    ])
}

fn circle_path(center: LayoutPoint, diameter: i64) -> Result<DisplayPath, DisplayListError> {
    let radius = diameter as f64 / 128.0;
    if radius <= 0.0 || !radius.is_finite() {
        return Err(DisplayListError::InvalidValue);
    }
    let cx = center.x as f64 / 64.0;
    let cy = center.y as f64 / 64.0;
    const KAPPA: f64 = 0.552_284_749_830_793_6;
    let c = radius * KAPPA;
    let top = Point::try_new(cx, cy - radius)?;
    let path = DisplayPath::new(vec![
        PathElement::MoveTo(top),
        PathElement::CubicTo {
            control1: Point::try_new(cx + c, cy - radius)?,
            control2: Point::try_new(cx + radius, cy - c)?,
            to: Point::try_new(cx + radius, cy)?,
        },
        PathElement::CubicTo {
            control1: Point::try_new(cx + radius, cy + c)?,
            control2: Point::try_new(cx + c, cy + radius)?,
            to: Point::try_new(cx, cy + radius)?,
        },
        PathElement::CubicTo {
            control1: Point::try_new(cx - c, cy + radius)?,
            control2: Point::try_new(cx - radius, cy + c)?,
            to: Point::try_new(cx - radius, cy)?,
        },
        PathElement::CubicTo {
            control1: Point::try_new(cx - radius, cy - c)?,
            control2: Point::try_new(cx - c, cy - radius)?,
            to: top,
        },
        PathElement::Close,
    ])?;
    Ok(path)
}

fn point(point: LayoutPoint) -> Result<Point, DisplayListError> {
    Ok(Point::try_new(units(point.x), units(point.y))?)
}

fn units(value: i64) -> f64 {
    value as f64 / 64.0
}

fn render_color(color: atelier_text_model::rich::Color) -> Color {
    Color {
        red: f32::from(color.r) / 255.0,
        green: f32::from(color.g) / 255.0,
        blue: f32::from(color.b) / 255.0,
        alpha: 1.0,
    }
}
