// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{FillRule, RasterError, RasterLimits};
use atelier_geom::{FlattenedSubpath, Point};

// Bounding coordinates also keeps cross products and scanline arithmetic finite.
pub(crate) fn validate_points(points: &[Point]) -> Result<(), RasterError> {
    if points
        .iter()
        .any(|p| p.x().abs() > 1.0e9 || p.y().abs() > 1.0e9)
    {
        return Err(RasterError::InvalidInput);
    }
    Ok(())
}

pub(crate) fn pixel_bounds(points: &[Point], width: u32, height: u32) -> (u32, u32, u32, u32) {
    let mut x0 = f64::from(width);
    let mut y0 = f64::from(height);
    let mut x1: f64 = 0.0;
    let mut y1: f64 = 0.0;
    for p in points {
        x0 = x0.min(p.x());
        y0 = y0.min(p.y());
        x1 = x1.max(p.x());
        y1 = y1.max(p.y());
    }
    (
        x0.floor().clamp(0.0, f64::from(width)) as u32,
        y0.floor().clamp(0.0, f64::from(height)) as u32,
        x1.ceil().clamp(0.0, f64::from(width)) as u32,
        y1.ceil().clamp(0.0, f64::from(height)) as u32,
    )
}

pub(crate) fn rasterize(
    paths: &[FlattenedSubpath],
    width: u32,
    height: u32,
    rule: FillRule,
    limits: RasterLimits,
) -> Result<Vec<u64>, RasterError> {
    let points: usize = paths.iter().map(|p| p.points.len()).sum();
    if points > limits.max_edges {
        return Err(RasterError::LimitExceeded);
    }
    let mut edges = Vec::new();
    let mut top = f64::from(height);
    let mut bottom: f64 = 0.0;
    for path in paths {
        validate_points(&path.points)?;
        for i in 0..path.points.len() {
            let a = path.points[i];
            let b = path.points[(i + 1) % path.points.len()];
            if a.y() == b.y() {
                continue;
            }
            top = top.min(a.y().min(b.y()));
            bottom = bottom.max(a.y().max(b.y()));
            edges.push((a, b));
        }
    }
    let y0 = top.floor().clamp(0.0, f64::from(height)) as u32;
    let y1 = bottom.ceil().clamp(0.0, f64::from(height)) as u32;
    if edges.is_empty() {
        return Ok(vec![0; width as usize * height as usize]);
    }
    let sort_factor = 1 + usize::BITS - edges.len().max(1).leading_zeros();
    let work = u64::from(y1 - y0)
        .checked_mul(8)
        .and_then(|n| {
            n.checked_mul(edges.len() as u64 * u64::from(sort_factor) + u64::from(width) * 8)
        })
        .ok_or(RasterError::LimitExceeded)?;
    if work > limits.max_work {
        return Err(RasterError::LimitExceeded);
    }
    let mut result = vec![0_u64; width as usize * height as usize];
    let mut crossings = Vec::with_capacity(edges.len());
    for y in y0..y1 {
        for sy in 0..8 {
            let scan = f64::from(y) + (f64::from(sy) + 0.5) / 8.0;
            crossings.clear();
            for &(a, b) in &edges {
                if scan >= a.y().min(b.y()) && scan < a.y().max(b.y()) {
                    let x = (a.x() * (b.y() - scan) + b.x() * (scan - a.y())) / (b.y() - a.y());
                    crossings.push((x, if b.y() > a.y() { 1_i32 } else { -1 }));
                }
            }
            crossings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0_i32;
            let mut previous: f64 = 0.0;
            for &(x, direction) in &crossings {
                let inside = match rule {
                    FillRule::NonZero => winding != 0,
                    FillRule::EvenOdd => winding % 2 != 0,
                };
                if inside {
                    let start = (previous * 8.0 - 0.5)
                        .ceil()
                        .clamp(0.0, f64::from(width) * 8.0) as u64;
                    let end = (x * 8.0 - 0.5).ceil().clamp(0.0, f64::from(width) * 8.0) as u64;
                    for sample in start..end {
                        let index = y as usize * width as usize + (sample / 8) as usize;
                        result[index] |= 1_u64 << (sy * 8 + (sample % 8) as u32);
                    }
                }
                winding += direction;
                previous = x;
            }
        }
    }
    Ok(result)
}
