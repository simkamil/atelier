// SPDX-License-Identifier: AGPL-3.0-or-later
//! Small, dependency-free 2D geometry primitives.
//!
//! Coordinates are finite `f64` values whose absolute magnitude is at most
//! [`MAX_COORDINATE`]. This limit applies to coordinate values; derived
//! dimensions such as rectangle width and height may reach
//! `2 * MAX_COORDINATE`. Operations that would produce a non-finite value
//! return [`GeometryError::NumericalOverflow`].

#![forbid(unsafe_code)]

use core::fmt;

/// Largest accepted absolute coordinate, in document units.
pub const MAX_COORDINATE: f64 = 1.0e150;
const MAX_FLATTENED_SEGMENTS: usize = 1_048_576;
const MAX_SUBDIVISION_DEPTH: u8 = 32;

/// Errors produced by geometry construction and operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    /// A coordinate, tolerance, or matrix component is non-finite or outside
    /// the supported finite coordinate range.
    InvalidValue,
    /// Curve flattening requires more than the fixed segment budget.
    SubdivisionLimitExceeded,
    /// An affine matrix has no inverse.
    SingularTransform,
    /// An intermediate calculation exceeded the supported finite range.
    NumericalOverflow,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue => formatter.write_str("invalid or out-of-range geometry value"),
            Self::SubdivisionLimitExceeded => {
                formatter.write_str("curve flattening subdivision limit exceeded")
            }
            Self::SingularTransform => formatter.write_str("affine transform is singular"),
            Self::NumericalOverflow => {
                formatter.write_str("geometry calculation exceeded the supported finite range")
            }
        }
    }
}

impl std::error::Error for GeometryError {}

fn valid_coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_COORDINATE
}

fn checked(value: f64) -> Result<f64, GeometryError> {
    if valid_coordinate(value) {
        Ok(value)
    } else {
        Err(GeometryError::NumericalOverflow)
    }
}

/// A two-dimensional point. Use [`Point::try_new`] to validate coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    pub fn try_new(x: f64, y: f64) -> Result<Self, GeometryError> {
        if valid_coordinate(x) && valid_coordinate(y) {
            Ok(Self { x, y })
        } else {
            Err(GeometryError::InvalidValue)
        }
    }

    pub fn x(self) -> f64 {
        self.x
    }
    pub fn y(self) -> f64 {
        self.y
    }

    pub fn distance(self, other: Self) -> Result<f64, GeometryError> {
        let dx = checked(self.x - other.x)?;
        let dy = checked(self.y - other.y)?;
        checked(dx.hypot(dy))
    }
}

/// An axis-aligned rectangle with normalized minimum and maximum corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    min: Point,
    max: Point,
}

impl Rect {
    /// Creates a normalized rectangle from any pair of opposite corners.
    pub fn new(a: Point, b: Point) -> Self {
        Self {
            min: Point {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
            },
            max: Point {
                x: a.x.max(b.x),
                y: a.y.max(b.y),
            },
        }
    }

    pub fn from_xywh(x: f64, y: f64, width: f64, height: f64) -> Result<Self, GeometryError> {
        if !valid_coordinate(x)
            || !valid_coordinate(y)
            || !valid_coordinate(width)
            || !valid_coordinate(height)
        {
            return Err(GeometryError::InvalidValue);
        }
        let x2 = checked(x + width)?;
        let y2 = checked(y + height)?;
        Ok(Self::new(Point { x, y }, Point { x: x2, y: y2 }))
    }

    pub fn min(self) -> Point {
        self.min
    }
    pub fn max(self) -> Point {
        self.max
    }
    pub fn left(self) -> f64 {
        self.min.x
    }
    pub fn top(self) -> f64 {
        self.min.y
    }
    pub fn right(self) -> f64 {
        self.max.x
    }
    pub fn bottom(self) -> f64 {
        self.max.y
    }
    /// Returns the width, which may be as large as `2 * MAX_COORDINATE`.
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }
    /// Returns the height, which may be as large as `2 * MAX_COORDINATE`.
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn contains(self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min: Point {
                x: self.min.x.min(other.min.x),
                y: self.min.y.min(other.min.y),
            },
            max: Point {
                x: self.max.x.max(other.max.x),
                y: self.max.y.max(other.max.y),
            },
        }
    }

    /// Returns the inclusive intersection; touching edges produce a zero-size rectangle.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let min = Point {
            x: self.min.x.max(other.min.x),
            y: self.min.y.max(other.min.y),
        };
        let max = Point {
            x: self.max.x.min(other.max.x),
            y: self.max.y.min(other.max.y),
        };
        (min.x <= max.x && min.y <= max.y).then_some(Self { min, max })
    }
}

/// A 2D affine transform using column vectors:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl AffineTransform {
    pub const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn translation(x: f64, y: f64) -> Result<Self, GeometryError> {
        if !valid_coordinate(x) || !valid_coordinate(y) {
            return Err(GeometryError::InvalidValue);
        }
        Ok(Self {
            e: x,
            f: y,
            ..Self::identity()
        })
    }

    pub fn scale(x: f64, y: f64) -> Result<Self, GeometryError> {
        if !valid_coordinate(x) || !valid_coordinate(y) {
            return Err(GeometryError::InvalidValue);
        }
        Ok(Self {
            a: x,
            d: y,
            ..Self::identity()
        })
    }

    pub fn rotation(radians: f64) -> Result<Self, GeometryError> {
        if !radians.is_finite() {
            return Err(GeometryError::InvalidValue);
        }
        let (sin, cos) = radians.sin_cos();
        Ok(Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        })
    }

    /// Returns the transform formed by applying `self`, then `next`.
    pub fn then(self, next: Self) -> Result<Self, GeometryError> {
        let result = Self {
            a: checked(next.a * self.a + next.c * self.b)?,
            b: checked(next.b * self.a + next.d * self.b)?,
            c: checked(next.a * self.c + next.c * self.d)?,
            d: checked(next.b * self.c + next.d * self.d)?,
            e: checked(next.a * self.e + next.c * self.f + next.e)?,
            f: checked(next.b * self.e + next.d * self.f + next.f)?,
        };
        Ok(result)
    }

    pub fn apply(self, point: Point) -> Result<Point, GeometryError> {
        Point::try_new(
            checked(self.a * point.x + self.c * point.y + self.e)?,
            checked(self.b * point.x + self.d * point.y + self.f)?,
        )
        .map_err(|_| GeometryError::NumericalOverflow)
    }

    pub fn inverse(self) -> Result<Self, GeometryError> {
        // Normalize before taking the determinant so uniformly tiny or large
        // linear parts do not underflow/overflow and masquerade as singular.
        let scale = self
            .a
            .abs()
            .max(self.b.abs())
            .max(self.c.abs())
            .max(self.d.abs());
        if scale == 0.0 {
            return Err(GeometryError::SingularTransform);
        }
        let (a_scaled, b_scaled, c_scaled, d_scaled) = (
            self.a / scale,
            self.b / scale,
            self.c / scale,
            self.d / scale,
        );
        let determinant = a_scaled * d_scaled - b_scaled * c_scaled;
        if !determinant.is_finite() {
            return Err(GeometryError::NumericalOverflow);
        }
        if determinant == 0.0 {
            return Err(GeometryError::SingularTransform);
        }
        let a = checked(d_scaled / determinant / scale)?;
        let b = checked(-b_scaled / determinant / scale)?;
        let c = checked(-c_scaled / determinant / scale)?;
        let d = checked(a_scaled / determinant / scale)?;
        let e = checked(-(a * self.e + c * self.f))?;
        let f = checked(-(b * self.e + d * self.f))?;
        Ok(Self { a, b, c, d, e, f })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Line {
        from: Point,
        to: Point,
    },
    Quadratic {
        from: Point,
        control: Point,
        to: Point,
    },
    Cubic {
        from: Point,
        control1: Point,
        control2: Point,
        to: Point,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Subpath {
    start: Point,
    segments: Vec<Segment>,
    closed: bool,
}

/// A path made of independent move-to-delimited subpaths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    subpaths: Vec<Subpath>,
}

/// One flattened subpath. Its first and last points remain explicit; closure
/// is represented by `closed` and does not duplicate the first point.
#[derive(Clone, Debug, PartialEq)]
pub struct FlattenedSubpath {
    pub points: Vec<Point>,
    pub closed: bool,
}

/// One validated path command, exposed for inspection without exposing the
/// path's mutable storage or segment invariants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadraticTo {
        control: Point,
        to: Point,
    },
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    Close,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, point: Point) {
        self.subpaths.push(Subpath {
            start: point,
            segments: Vec::new(),
            closed: false,
        });
    }

    fn active_mut(&mut self) -> Result<&mut Subpath, GeometryError> {
        self.subpaths.last_mut().ok_or(GeometryError::InvalidValue)
    }

    fn current(subpath: &Subpath) -> Point {
        match subpath.segments.last() {
            Some(Segment::Line { to, .. })
            | Some(Segment::Quadratic { to, .. })
            | Some(Segment::Cubic { to, .. }) => *to,
            None => subpath.start,
        }
    }

    fn active_for_segment(&mut self) -> Result<&mut Subpath, GeometryError> {
        if self
            .subpaths
            .last()
            .ok_or(GeometryError::InvalidValue)?
            .closed
        {
            let start = self
                .subpaths
                .last()
                .expect("active subpath was checked")
                .start;
            self.subpaths.push(Subpath {
                start,
                segments: Vec::new(),
                closed: false,
            });
        }
        self.active_mut()
    }

    pub fn line_to(&mut self, to: Point) -> Result<(), GeometryError> {
        let subpath = self.active_for_segment()?;
        let from = Self::current(subpath);
        subpath.segments.push(Segment::Line { from, to });
        Ok(())
    }

    pub fn quadratic_to(&mut self, control: Point, to: Point) -> Result<(), GeometryError> {
        let subpath = self.active_for_segment()?;
        let from = Self::current(subpath);
        subpath
            .segments
            .push(Segment::Quadratic { from, control, to });
        Ok(())
    }

    pub fn cubic_to(
        &mut self,
        control1: Point,
        control2: Point,
        to: Point,
    ) -> Result<(), GeometryError> {
        let subpath = self.active_for_segment()?;
        let from = Self::current(subpath);
        subpath.segments.push(Segment::Cubic {
            from,
            control1,
            control2,
            to,
        });
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), GeometryError> {
        self.active_mut()?.closed = true;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.subpaths.is_empty()
    }

    /// Visits the exact commands in this path in order, using constant extra
    /// memory. The callback cannot mutate the path's internal representation.
    pub fn visit_commands(&self, mut visitor: impl FnMut(PathCommand)) {
        for subpath in &self.subpaths {
            visitor(PathCommand::MoveTo(subpath.start));
            for segment in &subpath.segments {
                match *segment {
                    Segment::Line { to, .. } => visitor(PathCommand::LineTo(to)),
                    Segment::Quadratic { control, to, .. } => {
                        visitor(PathCommand::QuadraticTo { control, to })
                    }
                    Segment::Cubic {
                        control1,
                        control2,
                        to,
                        ..
                    } => visitor(PathCommand::CubicTo {
                        control1,
                        control2,
                        to,
                    }),
                }
            }
            if subpath.closed {
                visitor(PathCommand::Close);
            }
        }
    }

    /// Returns tight bounds, including every move point and curve extremum.
    pub fn bounds(&self) -> Result<Option<Rect>, GeometryError> {
        let mut bounds: Option<Rect> = None;
        for subpath in &self.subpaths {
            include_point(&mut bounds, subpath.start);
            for segment in &subpath.segments {
                match *segment {
                    Segment::Line { from, to } => {
                        include_point(&mut bounds, from);
                        include_point(&mut bounds, to);
                    }
                    Segment::Quadratic { from, control, to } => {
                        include_point(&mut bounds, from);
                        include_point(&mut bounds, to);
                        for axis in 0..2 {
                            let (p0, p1, p2) = coordinates(from, control, to, axis);
                            let denominator = p0 - 2.0 * p1 + p2;
                            if denominator != 0.0 {
                                let t = (p0 - p1) / denominator;
                                if t > 0.0 && t < 1.0 {
                                    include_point(
                                        &mut bounds,
                                        eval_quadratic(from, control, to, t)?,
                                    );
                                }
                            }
                        }
                    }
                    Segment::Cubic {
                        from,
                        control1,
                        control2,
                        to,
                    } => {
                        include_point(&mut bounds, from);
                        include_point(&mut bounds, to);
                        for axis in 0..2 {
                            let (p0, p1, p2, p3) =
                                coordinates_cubic(from, control1, control2, to, axis);
                            for t in cubic_derivative_roots(p0, p1, p2, p3) {
                                if t > 0.0 && t < 1.0 {
                                    include_point(
                                        &mut bounds,
                                        eval_cubic(from, control1, control2, to, t)?,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(bounds)
    }

    /// Flattens curves while preserving subpath boundaries and endpoints.
    /// A segment is accepted only when every Bezier control point is within
    /// `tolerance` of the finite chord segment, which bounds the curve's
    /// distance to that segment by the convex-hull property. Subdivision is
    /// capped at depth 32 and 1,048,576 output segments.
    pub fn flatten(&self, tolerance: f64) -> Result<Vec<FlattenedSubpath>, GeometryError> {
        if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > MAX_COORDINATE {
            return Err(GeometryError::InvalidValue);
        }
        let mut remaining = MAX_FLATTENED_SEGMENTS;
        let mut output = Vec::with_capacity(self.subpaths.len());
        for subpath in &self.subpaths {
            let mut points = vec![subpath.start];
            {
                let mut context = FlattenContext {
                    tolerance,
                    remaining: &mut remaining,
                    points: &mut points,
                };
                for segment in &subpath.segments {
                    match *segment {
                        Segment::Line { to, .. } => {
                            push_limited(context.points, to, context.remaining)?
                        }
                        Segment::Quadratic { from, control, to } => {
                            flatten_quadratic(from, control, to, 0, &mut context)?
                        }
                        Segment::Cubic {
                            from,
                            control1,
                            control2,
                            to,
                        } => flatten_cubic(from, control1, control2, to, 0, &mut context)?,
                    }
                }
            }
            output.push(FlattenedSubpath {
                points,
                closed: subpath.closed,
            });
        }
        Ok(output)
    }
}

fn include_point(bounds: &mut Option<Rect>, point: Point) {
    *bounds = Some(match *bounds {
        Some(rect) => rect.union(Rect::new(point, point)),
        None => Rect::new(point, point),
    });
}

fn coordinates(a: Point, b: Point, c: Point, axis: usize) -> (f64, f64, f64) {
    if axis == 0 {
        (a.x, b.x, c.x)
    } else {
        (a.y, b.y, c.y)
    }
}

fn coordinates_cubic(a: Point, b: Point, c: Point, d: Point, axis: usize) -> (f64, f64, f64, f64) {
    if axis == 0 {
        (a.x, b.x, c.x, d.x)
    } else {
        (a.y, b.y, c.y, d.y)
    }
}

fn lerp(a: Point, b: Point, t: f64) -> Result<Point, GeometryError> {
    Point::try_new(a.x * (1.0 - t) + b.x * t, a.y * (1.0 - t) + b.y * t)
        .map_err(|_| GeometryError::NumericalOverflow)
}

fn eval_quadratic(a: Point, b: Point, c: Point, t: f64) -> Result<Point, GeometryError> {
    lerp(lerp(a, b, t)?, lerp(b, c, t)?, t)
}

fn eval_cubic(a: Point, b: Point, c: Point, d: Point, t: f64) -> Result<Point, GeometryError> {
    let ab = lerp(a, b, t)?;
    let bc = lerp(b, c, t)?;
    let cd = lerp(c, d, t)?;
    lerp(lerp(ab, bc, t)?, lerp(bc, cd, t)?, t)
}

fn cubic_derivative_roots(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // Divide by the largest input before forming coefficients to avoid
    // overflow in the discriminant while preserving the polynomial roots.
    let scale = p0.abs().max(p1.abs()).max(p2.abs()).max(p3.abs());
    if scale == 0.0 {
        return Vec::new();
    }
    let (p0, p1, p2, p3) = (p0 / scale, p1 / scale, p2 / scale, p3 / scale);
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    if a == 0.0 {
        return if b == 0.0 { Vec::new() } else { vec![-c / b] };
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt();
    let q = -0.5 * (b + if b >= 0.0 { root } else { -root });
    if q == 0.0 {
        vec![-b / (2.0 * a)]
    } else if discriminant == 0.0 {
        vec![q / a]
    } else {
        vec![q / a, c / q]
    }
}

fn point_segment_distance(point: Point, a: Point, b: Point) -> Result<f64, GeometryError> {
    let dx = checked(b.x - a.x)?;
    let dy = checked(b.y - a.y)?;
    let px = checked(point.x - a.x)?;
    let py = checked(point.y - a.y)?;
    let length = dx.hypot(dy);
    if length == 0.0 {
        return Ok(px.hypot(py));
    }
    let ux = dx / length;
    let uy = dy / length;
    let projection = px * ux + py * uy;
    let along = projection.clamp(0.0, length);
    let nearest = Point::try_new(a.x + ux * along, a.y + uy * along)
        .map_err(|_| GeometryError::NumericalOverflow)?;
    point.distance(nearest)
}

fn flat_enough(points: &[Point], tolerance: f64) -> Result<bool, GeometryError> {
    let first = points[0];
    let last = points[points.len() - 1];
    for point in &points[1..points.len() - 1] {
        if point_segment_distance(*point, first, last)? > tolerance {
            return Ok(false);
        }
    }
    Ok(true)
}

fn push_limited(
    points: &mut Vec<Point>,
    point: Point,
    remaining: &mut usize,
) -> Result<(), GeometryError> {
    if *remaining == 0 {
        return Err(GeometryError::SubdivisionLimitExceeded);
    }
    *remaining -= 1;
    points.push(point);
    Ok(())
}

struct FlattenContext<'a> {
    tolerance: f64,
    remaining: &'a mut usize,
    points: &'a mut Vec<Point>,
}

fn flatten_quadratic(
    a: Point,
    b: Point,
    c: Point,
    depth: u8,
    context: &mut FlattenContext<'_>,
) -> Result<(), GeometryError> {
    if flat_enough(&[a, b, c], context.tolerance)? {
        return push_limited(context.points, c, context.remaining);
    }
    if depth >= MAX_SUBDIVISION_DEPTH {
        return Err(GeometryError::SubdivisionLimitExceeded);
    }
    let ab = lerp(a, b, 0.5)?;
    let bc = lerp(b, c, 0.5)?;
    let mid = lerp(ab, bc, 0.5)?;
    flatten_quadratic(a, ab, mid, depth + 1, context)?;
    flatten_quadratic(mid, bc, c, depth + 1, context)
}

fn flatten_cubic(
    a: Point,
    b: Point,
    c: Point,
    d: Point,
    depth: u8,
    context: &mut FlattenContext<'_>,
) -> Result<(), GeometryError> {
    if flat_enough(&[a, b, c, d], context.tolerance)? {
        return push_limited(context.points, d, context.remaining);
    }
    if depth >= MAX_SUBDIVISION_DEPTH {
        return Err(GeometryError::SubdivisionLimitExceeded);
    }
    let ab = lerp(a, b, 0.5)?;
    let bc = lerp(b, c, 0.5)?;
    let cd = lerp(c, d, 0.5)?;
    let abc = lerp(ab, bc, 0.5)?;
    let bcd = lerp(bc, cd, 0.5)?;
    let mid = lerp(abc, bcd, 0.5)?;
    flatten_cubic(a, ab, abc, mid, depth + 1, context)?;
    flatten_cubic(mid, bcd, cd, d, depth + 1, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::try_new(x, y).unwrap()
    }

    #[test]
    fn rectangles_normalize_and_handle_intersection() {
        let a = Rect::new(p(5.0, 8.0), p(1.0, 2.0));
        assert_eq!(
            (a.left(), a.top(), a.right(), a.bottom()),
            (1.0, 2.0, 5.0, 8.0)
        );
        assert!(a.contains(p(1.0, 2.0)));
        assert_eq!(
            a.intersection(Rect::new(p(4.0, 6.0), p(10.0, 12.0)))
                .unwrap(),
            Rect::new(p(4.0, 6.0), p(5.0, 8.0))
        );
        assert!(
            a.intersection(Rect::new(p(6.0, 9.0), p(7.0, 10.0)))
                .is_none()
        );
        assert_eq!(
            a.union(Rect::new(p(-1.0, 3.0), p(3.0, 10.0))),
            Rect::new(p(-1.0, 2.0), p(5.0, 10.0))
        );

        let opposite_corners = Rect::new(
            p(-MAX_COORDINATE, -MAX_COORDINATE),
            p(MAX_COORDINATE, MAX_COORDINATE),
        );
        assert_eq!(opposite_corners.width(), 2.0 * MAX_COORDINATE);
        assert_eq!(opposite_corners.height(), 2.0 * MAX_COORDINATE);
    }

    #[test]
    fn affine_composition_order_and_inverse_are_explicit() {
        let point = p(2.0, 3.0);
        let translate = AffineTransform::translation(5.0, -1.0).unwrap();
        let scale = AffineTransform::scale(2.0, 3.0).unwrap();
        let translated_then_scaled = translate.then(scale).unwrap();
        let scaled_then_translated = scale.then(translate).unwrap();
        assert_eq!(translated_then_scaled.apply(point).unwrap(), p(14.0, 6.0));
        assert_eq!(scaled_then_translated.apply(point).unwrap(), p(9.0, 8.0));
        assert_eq!(
            translated_then_scaled
                .inverse()
                .unwrap()
                .apply(p(14.0, 6.0))
                .unwrap(),
            point
        );
        assert_eq!(
            AffineTransform::scale(0.0, 1.0).unwrap().inverse(),
            Err(GeometryError::SingularTransform)
        );
        assert_eq!(
            AffineTransform::scale(1e-200, 1e-200).unwrap().inverse(),
            Err(GeometryError::NumericalOverflow)
        );
    }

    #[test]
    fn quadratic_and_cubic_bounds_include_internal_extrema() {
        let mut quadratic = Path::new();
        quadratic.move_to(p(0.0, 0.0));
        quadratic.quadratic_to(p(2.0, 4.0), p(4.0, 0.0)).unwrap();
        let bounds = quadratic.bounds().unwrap().unwrap();
        assert_eq!(bounds.top(), 0.0);
        assert!((bounds.bottom() - 2.0).abs() < 1e-12);

        let mut cubic = Path::new();
        cubic.move_to(p(0.0, 0.0));
        cubic
            .cubic_to(p(0.0, 3.0), p(3.0, 3.0), p(3.0, 0.0))
            .unwrap();
        let bounds = cubic.bounds().unwrap().unwrap();
        assert!((bounds.right() - 3.0).abs() < 1e-12);
        assert!((bounds.bottom() - 2.25).abs() < 1e-12);

        let mut symmetric_cubic = Path::new();
        symmetric_cubic.move_to(p(0.0, 0.0));
        symmetric_cubic
            .cubic_to(p(3.0, 0.0), p(-3.0, 0.0), p(0.0, 0.0))
            .unwrap();
        let bounds = symmetric_cubic.bounds().unwrap().unwrap();
        let expected_extremum = 3.0_f64.sqrt() / 2.0;
        assert!((bounds.left() + expected_extremum).abs() <= 4.0 * f64::EPSILON);
        assert!((bounds.right() - expected_extremum).abs() <= 4.0 * f64::EPSILON);

        let mut line_cubic = Path::new();
        line_cubic.move_to(p(0.0, 2.0));
        line_cubic
            .cubic_to(p(1.0, 2.0), p(2.0, 2.0), p(3.0, 2.0))
            .unwrap();
        assert_eq!(
            line_cubic.bounds().unwrap().unwrap(),
            Rect::new(p(0.0, 2.0), p(3.0, 2.0))
        );

        // x(t) = (t - 1/2)^3 has a double derivative root at t = 1/2,
        // but no internal maximum or minimum.
        let mut double_root_cubic = Path::new();
        double_root_cubic.move_to(p(-0.125, 0.0));
        double_root_cubic
            .cubic_to(p(0.125, 0.0), p(-0.125, 0.0), p(0.125, 0.0))
            .unwrap();
        assert_eq!(
            double_root_cubic.bounds().unwrap().unwrap(),
            Rect::new(p(-0.125, 0.0), p(0.125, 0.0))
        );
    }

    #[test]
    fn empty_and_move_only_paths_have_defined_bounds_and_flattening() {
        let empty = Path::new();
        assert!(empty.is_empty());
        assert_eq!(empty.bounds().unwrap(), None);
        assert_eq!(empty.flatten(0.25).unwrap(), Vec::new());
        assert_eq!(Path::new().close(), Err(GeometryError::InvalidValue));

        let point = p(3.0, -2.0);
        let mut move_only = Path::new();
        move_only.move_to(point);
        assert!(!move_only.is_empty());
        assert_eq!(move_only.bounds().unwrap(), Some(Rect::new(point, point)));
        assert_eq!(
            move_only.flatten(0.25).unwrap(),
            vec![FlattenedSubpath {
                points: vec![point],
                closed: false,
            }]
        );
        move_only.close().unwrap();
        assert_eq!(
            move_only.flatten(0.25).unwrap(),
            vec![FlattenedSubpath {
                points: vec![point],
                closed: true,
            }]
        );
    }

    #[test]
    fn visit_commands_exposes_exact_curves_without_mutable_storage() {
        let a = Point::try_new(1.0, 2.0).unwrap();
        let b = Point::try_new(3.0, 4.0).unwrap();
        let c = Point::try_new(5.0, 6.0).unwrap();
        let d = Point::try_new(7.0, 8.0).unwrap();
        let mut path = Path::new();
        path.move_to(a);
        path.line_to(b).unwrap();
        path.quadratic_to(c, d).unwrap();
        path.cubic_to(a, b, c).unwrap();
        path.close().unwrap();

        let mut commands = Vec::new();
        path.visit_commands(|command| commands.push(command));
        assert_eq!(
            commands,
            vec![
                PathCommand::MoveTo(a),
                PathCommand::LineTo(b),
                PathCommand::QuadraticTo { control: c, to: d },
                PathCommand::CubicTo {
                    control1: a,
                    control2: b,
                    to: c
                },
                PathCommand::Close,
            ]
        );
    }

    #[test]
    fn flatten_keeps_subpaths_and_catches_collinear_excursion() {
        let mut path = Path::new();
        path.move_to(p(0.0, 0.0));
        path.cubic_to(p(10.0, 0.0), p(-10.0, 0.0), p(1.0, 0.0))
            .unwrap();
        path.close().unwrap();
        path.move_to(p(20.0, 4.0));
        path.line_to(p(21.0, 4.0)).unwrap();
        let flat = path.flatten(0.05).unwrap();
        assert_eq!(flat.len(), 2);
        assert!(flat[0].closed);
        assert!(!flat[1].closed);
        assert_eq!(flat[0].points.first(), Some(&p(0.0, 0.0)));
        assert_eq!(flat[0].points.last(), Some(&p(1.0, 0.0)));
        assert!(flat[0].points.iter().any(|point| point.x > 1.0));
        let mut non_collinear = Path::new();
        non_collinear.move_to(p(0.0, 0.0));
        non_collinear
            .cubic_to(p(1.0, 10.0), p(2.0, -10.0), p(3.0, 1.0))
            .unwrap();
        assert_eq!(
            non_collinear.flatten(f64::MIN_POSITIVE),
            Err(GeometryError::SubdivisionLimitExceeded)
        );
        let bounds = path.bounds().unwrap().unwrap();
        for step in 0..=2000 {
            let t = step as f64 / 2000.0;
            let u = 1.0 - t;
            // Bernstein evaluation is independent of the De Casteljau
            // subdivision used by flatten().
            let sample = p(
                3.0 * u * u * t * 10.0 + 3.0 * u * t * t * -10.0 + t * t * t,
                0.0,
            );
            assert!(bounds.contains(sample));
            let distance = flat[0]
                .points
                .windows(2)
                .map(|pair| test_segment_distance(sample, pair[0], pair[1]))
                .fold(f64::INFINITY, f64::min);
            assert!(
                distance <= 0.05 + 1e-10,
                "sample at {t} missed by {distance}"
            );
        }
    }

    fn test_segment_distance(point: Point, a: Point, b: Point) -> f64 {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length_squared = dx * dx + dy * dy;
        if length_squared == 0.0 {
            return (point.x - a.x).hypot(point.y - a.y);
        }
        let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length_squared).clamp(0.0, 1.0);
        (point.x - (a.x + t * dx)).hypot(point.y - (a.y + t * dy))
    }

    fn assert_flat_curve(path: &Path, tolerance: f64, sample: impl Fn(f64) -> Point) {
        let flattened = path.flatten(tolerance).unwrap();
        let bounds = path.bounds().unwrap().unwrap();
        for step in 0..=1000 {
            let point = sample(step as f64 / 1000.0);
            assert!(bounds.contains(point), "bounds missed {point:?}");
            let distance = flattened[0]
                .points
                .windows(2)
                .map(|pair| test_segment_distance(point, pair[0], pair[1]))
                .fold(f64::INFINITY, f64::min);
            assert!(distance <= tolerance + 1e-10, "curve missed by {distance}");
        }
    }

    #[test]
    fn close_then_line_quadratic_or_cubic_starts_a_new_open_subpath() {
        let start = p(0.0, 0.0);
        let prior_end = p(2.0, 0.0);
        let cases = [p(4.0, 0.0), p(4.0, 2.0), p(4.0, -2.0)];

        let mut line = Path::new();
        line.move_to(start);
        line.line_to(prior_end).unwrap();
        line.close().unwrap();
        line.close().unwrap();
        line.line_to(cases[0]).unwrap();

        let mut quadratic = Path::new();
        quadratic.move_to(start);
        quadratic.line_to(prior_end).unwrap();
        quadratic.close().unwrap();
        quadratic.close().unwrap();
        quadratic.quadratic_to(p(2.0, 3.0), cases[1]).unwrap();

        let mut cubic = Path::new();
        cubic.move_to(start);
        cubic.line_to(prior_end).unwrap();
        cubic.close().unwrap();
        cubic.close().unwrap();
        cubic.cubic_to(p(1.0, 3.0), p(3.0, -3.0), cases[2]).unwrap();

        for (path, expected_end) in [
            (&line, cases[0]),
            (&quadratic, cases[1]),
            (&cubic, cases[2]),
        ] {
            let flattened = path.flatten(0.05).unwrap();
            assert_eq!(flattened.len(), 2);
            assert!(flattened[0].closed);
            assert_eq!(flattened[0].points.first(), Some(&start));
            assert_eq!(flattened[0].points.last(), Some(&prior_end));
            assert!(!flattened[1].closed);
            assert_eq!(flattened[1].points.first(), Some(&start));
            assert_eq!(flattened[1].points.last(), Some(&expected_end));
        }
    }

    #[test]
    fn flatten_samples_non_collinear_curves_and_coincident_cubic_endpoints() {
        let tolerance = 0.05;
        let mut quadratic = Path::new();
        quadratic.move_to(p(0.0, 0.0));
        quadratic.quadratic_to(p(2.0, 6.0), p(6.0, 1.0)).unwrap();
        assert_flat_curve(&quadratic, tolerance, |t| {
            let u = 1.0 - t;
            p(2.0 * u * t * 2.0 + t * t * 6.0, 2.0 * u * t * 6.0 + t * t)
        });

        let mut cubic = Path::new();
        cubic.move_to(p(0.0, 0.0));
        cubic
            .cubic_to(p(1.0, 8.0), p(7.0, 8.0), p(8.0, 1.0))
            .unwrap();
        assert_flat_curve(&cubic, tolerance, |t| {
            let u = 1.0 - t;
            p(
                3.0 * u * u * t + 3.0 * u * t * t * 7.0 + t * t * t * 8.0,
                3.0 * u * u * t * 8.0 + 3.0 * u * t * t * 8.0 + t * t * t,
            )
        });

        let start = p(4.0, 2.0);
        let control1 = p(10.0, 14.0);
        let control2 = p(-2.0, 14.0);
        let mut loop_curve = Path::new();
        loop_curve.move_to(start);
        loop_curve.cubic_to(control1, control2, start).unwrap();
        assert_flat_curve(&loop_curve, tolerance, |t| {
            let u = 1.0 - t;
            p(
                u * u * u * start.x
                    + 3.0 * u * u * t * control1.x
                    + 3.0 * u * t * t * control2.x
                    + t * t * t * start.x,
                u * u * u * start.y
                    + 3.0 * u * u * t * control1.y
                    + 3.0 * u * t * t * control2.y
                    + t * t * t * start.y,
            )
        });
    }

    #[test]
    fn flatten_rejects_invalid_tolerance_and_path_without_current_subpath() {
        assert_eq!(Path::new().flatten(0.0), Err(GeometryError::InvalidValue));
        assert_eq!(
            Path::new().flatten(f64::NAN),
            Err(GeometryError::InvalidValue)
        );
        assert_eq!(
            Path::new().line_to(p(1.0, 1.0)),
            Err(GeometryError::InvalidValue)
        );
    }

    #[test]
    fn point_and_rect_reject_numeric_overflow() {
        assert_eq!(
            Point::try_new(f64::INFINITY, 0.0),
            Err(GeometryError::InvalidValue)
        );
        assert_eq!(
            Point::try_new(MAX_COORDINATE * 2.0, 0.0),
            Err(GeometryError::InvalidValue)
        );
        assert_eq!(
            Rect::from_xywh(MAX_COORDINATE, 0.0, MAX_COORDINATE, 1.0),
            Err(GeometryError::NumericalOverflow)
        );
    }
}
