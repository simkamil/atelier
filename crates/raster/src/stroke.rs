// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{RasterError, mask::validate_points};
use atelier_geom::{FlattenedSubpath, Path, Point};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Square,
    Round,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Bevel,
    Miter,
    Round,
}
#[derive(Clone, Copy, Debug)]
pub struct StrokeStyle {
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    /// Ratio of miter distance from center to half the stroke width, at least 1.
    pub miter_limit: f64,
}
impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
        }
    }
}
type P = (f64, f64);
fn p(point: Point) -> P {
    (point.x(), point.y())
}
fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}
fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}
fn mul(a: P, s: f64) -> P {
    (a.0 * s, a.1 * s)
}
fn cross(a: P, b: P) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

struct Builder {
    paths: Vec<FlattenedSubpath>,
    points: usize,
    max: usize,
}
impl Builder {
    fn polygon(&mut self, vertices: &[P]) -> Result<(), RasterError> {
        self.points = self
            .points
            .checked_add(vertices.len())
            .ok_or(RasterError::LimitExceeded)?;
        if self.points > self.max {
            return Err(RasterError::LimitExceeded);
        }
        let mut points = vertices
            .iter()
            .map(|&(x, y)| Point::try_new(x, y))
            .collect::<Result<Vec<_>, _>>()?;
        validate_points(&points)?;
        let area: f64 = vertices
            .iter()
            .zip(vertices.iter().cycle().skip(1))
            .map(|(&a, &b)| cross(a, b))
            .sum();
        if area < 0.0 {
            points.reverse();
        }
        self.paths.push(FlattenedSubpath {
            points,
            closed: true,
        });
        Ok(())
    }
    fn circle(&mut self, center: P, r: f64) -> Result<(), RasterError> {
        let n = (core::f64::consts::PI / (1.0 - (1.0 / 32.0_f64).min(r) / r).acos())
            .ceil()
            .max(12.0) as usize;
        if n > 4096 || self.points.checked_add(n).is_none_or(|v| v > self.max) {
            return Err(RasterError::LimitExceeded);
        }
        let vertices = (0..n)
            .map(|i| {
                let a = i as f64 * core::f64::consts::TAU / n as f64;
                add(center, (r * a.cos(), r * a.sin()))
            })
            .collect::<Vec<_>>();
        self.polygon(&vertices)
    }
}

pub(crate) fn expand(
    path: &Path,
    style: StrokeStyle,
    max: usize,
) -> Result<Vec<FlattenedSubpath>, RasterError> {
    if !style.width.is_finite()
        || style.width < 0.0
        || style.width > 1.0e9
        || !style.miter_limit.is_finite()
        || !(1.0..=1.0e6).contains(&style.miter_limit)
    {
        return Err(RasterError::InvalidInput);
    }
    if style.width == 0.0 {
        return Ok(Vec::new());
    }
    let r = style.width / 2.0;
    let mut builder = Builder {
        paths: Vec::new(),
        points: 0,
        max,
    };
    let flat = path.flatten(1.0 / 16.0)?;
    let mut input_points = 0_usize;
    for subpath in flat {
        input_points = input_points
            .checked_add(subpath.points.len())
            .ok_or(RasterError::LimitExceeded)?;
        if input_points > max {
            return Err(RasterError::LimitExceeded);
        }
        validate_points(&subpath.points)?;
        let mut vertices = subpath.points.into_iter().map(p).collect::<Vec<_>>();
        vertices.dedup();
        if subpath.closed && vertices.len() > 1 && vertices.first() == vertices.last() {
            vertices.pop();
        }
        let n = vertices.len();
        if n == 0 {
            continue;
        }
        if n == 1 {
            if !subpath.closed {
                match style.cap {
                    LineCap::Round => builder.circle(vertices[0], r)?,
                    LineCap::Square => {
                        let a = vertices[0];
                        builder.polygon(&[
                            (a.0 - r, a.1 - r),
                            (a.0 + r, a.1 - r),
                            (a.0 + r, a.1 + r),
                            (a.0 - r, a.1 + r),
                        ])?;
                    }
                    LineCap::Butt => {}
                }
            }
            continue;
        }
        let count = if subpath.closed { n } else { n - 1 };
        let mut dirs = Vec::with_capacity(count);
        for i in 0..count {
            let a = vertices[i];
            let b = vertices[(i + 1) % n];
            let d = sub(b, a);
            let length = d.0.hypot(d.1);
            let u = mul(d, 1.0 / length);
            let normal = (-u.1 * r, u.0 * r);
            dirs.push((u, normal));
            let a = if !subpath.closed && i == 0 && style.cap == LineCap::Square {
                sub(a, mul(u, r))
            } else {
                a
            };
            let b = if !subpath.closed && i == count - 1 && style.cap == LineCap::Square {
                add(b, mul(u, r))
            } else {
                b
            };
            builder.polygon(&[
                add(a, normal),
                add(b, normal),
                sub(b, normal),
                sub(a, normal),
            ])?;
        }
        if !subpath.closed && style.cap == LineCap::Round {
            builder.circle(vertices[0], r)?;
            builder.circle(vertices[n - 1], r)?;
        }
        let start = if subpath.closed { 0 } else { 1 };
        let end = if subpath.closed { n } else { n - 1 };
        for (i, &center) in vertices.iter().enumerate().take(end).skip(start) {
            if style.join == LineJoin::Round {
                builder.circle(center, r)?;
                continue;
            }
            let (u, normal1) = dirs[(i + count - 1) % count];
            let (v, normal2) = dirs[i % count];
            for sign in [-1.0, 1.0] {
                let a = add(center, mul(normal1, sign));
                let b = add(center, mul(normal2, sign));
                let divisor = cross(u, v);
                if style.join == LineJoin::Miter && divisor.abs() > 1.0e-12 {
                    let m = add(a, mul(u, cross(sub(b, a), v) / divisor));
                    let distance = sub(m, center);
                    if distance.0.hypot(distance.1) <= style.miter_limit * r {
                        builder.polygon(&[center, a, m, b])?;
                        continue;
                    }
                }
                builder.polygon(&[center, a, b])?;
            }
        }
    }
    Ok(builder.paths)
}
