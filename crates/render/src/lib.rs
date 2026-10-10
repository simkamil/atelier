// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ordered, validated display lists, a bounded portable binary format, and an
//! SVG backend.
//!
//! Image and font identifiers remain opaque document-owned values. The SVG
//! backend resolves them only through caller-supplied in-memory resources; it
//! does not load files, system fonts, or network resources.

#![forbid(unsafe_code)]

use atelier_geom::{GeometryError, MAX_COORDINATE, Path as GeomPath, Point, Rect};
use core::fmt;

mod svg;
pub use svg::{SvgError, SvgImage, SvgLimits, SvgResourceProvider, render_svg};

/// Stable format signature (`ATDL`, Atelier Display List).
pub const SIGNATURE: [u8; 4] = *b"ATDL";
/// Major version written by this implementation.
pub const FORMAT_MAJOR: u16 = 1;
/// Minor version written by this implementation.
pub const FORMAT_MINOR: u16 = 0;

/// Hard upper bounds applied before allocations and while validating lists.
pub mod limits {
    /// Largest accepted encoded or decoded stream.
    pub const MAX_BYTES: usize = 64 * 1024 * 1024;
    /// Largest number of commands on one page.
    pub const MAX_COMMANDS: usize = 1_000_000;
    /// Largest cumulative number of path elements on one page.
    pub const MAX_PATH_ELEMENTS: usize = 1_000_000;
    /// Largest cumulative number of glyphs on one page.
    pub const MAX_GLYPHS: usize = 1_000_000;
    /// Largest string or opaque resource identifier payload in bytes.
    pub const MAX_RESOURCE_BYTES: usize = 4096;
    /// Largest nesting depth for either independent state stack.
    pub const MAX_STACK_DEPTH: usize = 128;
}

/// Failures produced by construction, validation, encoding, and decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayListError {
    /// A value does not meet the display-list contract.
    InvalidValue,
    /// A path, point, or transform failed `atelier-geom` validation.
    Geometry(GeometryError),
    /// A push/pop sequence is unbalanced or exceeds its allowed depth.
    InvalidState,
    /// An input or output exceeds a documented resource budget.
    LimitExceeded,
    /// Bytes are malformed, truncated, have an unknown opcode/version, or trail the stream.
    InvalidFormat,
}

impl fmt::Display for DisplayListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue => f.write_str("invalid display-list value"),
            Self::Geometry(error) => write!(f, "invalid geometry: {error}"),
            Self::InvalidState => f.write_str("unbalanced or excessive display-list state"),
            Self::LimitExceeded => f.write_str("display-list resource limit exceeded"),
            Self::InvalidFormat => f.write_str("invalid display-list binary format"),
        }
    }
}

impl std::error::Error for DisplayListError {}

impl From<GeometryError> for DisplayListError {
    fn from(value: GeometryError) -> Self {
        Self::Geometry(value)
    }
}

/// Page bounds in page units. Width and height must be positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageInfo {
    pub bounds: Rect,
}

impl PageInfo {
    pub fn new(bounds: Rect) -> Result<Self, DisplayListError> {
        if !bounds.width().is_finite()
            || !bounds.height().is_finite()
            || bounds.width() <= 0.0
            || bounds.height() <= 0.0
        {
            return Err(DisplayListError::InvalidValue);
        }
        Ok(Self { bounds })
    }
}

/// Non-premultiplied red, green, blue, alpha components in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Color {
    pub fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Result<Self, DisplayListError> {
        let color = Self {
            red,
            green,
            blue,
            alpha,
        };
        color.validate()?;
        Ok(color)
    }
    fn validate(self) -> Result<(), DisplayListError> {
        if [self.red, self.green, self.blue, self.alpha]
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        {
            Ok(())
        } else {
            Err(DisplayListError::InvalidValue)
        }
    }
}

/// A path element using the validated, portable geometry point type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathElement {
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

/// A serializable path description. Construction and decoding validate the
/// same move/segment/close rules as `atelier-geom::Path`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayPath {
    pub elements: Vec<PathElement>,
}

impl DisplayPath {
    pub fn new(elements: Vec<PathElement>) -> Result<Self, DisplayListError> {
        let value = Self { elements };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), DisplayListError> {
        let mut path = GeomPath::new();
        for element in &self.elements {
            match *element {
                PathElement::MoveTo(p) => path.move_to(p),
                PathElement::LineTo(p) => path.line_to(p)?,
                PathElement::QuadraticTo { control, to } => path.quadratic_to(control, to)?,
                PathElement::CubicTo {
                    control1,
                    control2,
                    to,
                } => path.cubic_to(control1, control2, to)?,
                PathElement::Close => path.close()?,
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
}

impl StrokeStyle {
    pub fn new(
        width: f64,
        cap: LineCap,
        join: LineJoin,
        miter_limit: f64,
    ) -> Result<Self, DisplayListError> {
        if !width.is_finite()
            || width <= 0.0
            || width > MAX_COORDINATE
            || !miter_limit.is_finite()
            || miter_limit < 1.0
            || miter_limit > MAX_COORDINATE
        {
            return Err(DisplayListError::InvalidValue);
        }
        Ok(Self {
            width,
            cap,
            join,
            miter_limit,
        })
    }
    fn validate(self) -> Result<(), DisplayListError> {
        Self::new(self.width, self.cap, self.join, self.miter_limit).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub position: Point,
    pub advance: Point,
}

/// Serializable affine matrix in column-vector order:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayTransform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl DisplayTransform {
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
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Result<Self, DisplayListError> {
        let t = Self { a, b, c, d, e, f };
        t.validate()?;
        Ok(t)
    }
    fn validate(self) -> Result<(), DisplayListError> {
        if [self.a, self.b, self.c, self.d, self.e, self.f]
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
        {
            Ok(())
        } else {
            Err(DisplayListError::InvalidValue)
        }
    }
    /// Returns the matrix produced by applying `self`, then `next`.
    fn then(self, next: Self) -> Result<Self, DisplayListError> {
        let v = Self {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    FillPath {
        path: DisplayPath,
        color: Color,
        rule: FillRule,
    },
    StrokePath {
        path: DisplayPath,
        color: Color,
        style: StrokeStyle,
    },
    DrawGlyphRun {
        font_id: u64,
        color: Color,
        glyphs: Vec<PositionedGlyph>,
    },
    DrawImage {
        image_id: u64,
        bounds: Rect,
    },
    PushClip {
        bounds: Rect,
    },
    PopClip,
    PushTransform {
        transform: DisplayTransform,
    },
    PopTransform,
    /// Link rectangle already expressed in transformed page coordinates.
    Link {
        bounds: Rect,
        target: String,
    },
    /// Semantic marker; does not change graphics state.
    Structure {
        role: String,
        node_id: u64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    pub page: PageInfo,
    pub commands: Vec<Command>,
}

impl DisplayList {
    pub fn new(page: PageInfo, commands: Vec<Command>) -> Result<Self, DisplayListError> {
        let list = Self { page, commands };
        list.validate()?;
        Ok(list)
    }
    pub fn validate(&self) -> Result<(), DisplayListError> {
        PageInfo::new(self.page.bounds)?;
        if self.commands.len() > limits::MAX_COMMANDS {
            return Err(DisplayListError::LimitExceeded);
        }
        let mut clip_depth = 0usize;
        let mut transform_stack = vec![DisplayTransform::identity()];
        let mut clip_stack: Vec<Option<Rect>> = vec![None]; // None is unbounded; empty intersection is tracked separately below.
        let mut clip_empty = vec![false];
        let mut path_total = 0usize;
        let mut glyph_total = 0usize;
        for command in &self.commands {
            match &command {
                Command::FillPath { path, color, .. } => {
                    path.validate()?;
                    color.validate()?;
                    path_total = path_total
                        .checked_add(path.elements.len())
                        .ok_or(DisplayListError::LimitExceeded)?;
                }
                Command::StrokePath { path, color, style } => {
                    path.validate()?;
                    color.validate()?;
                    style.validate()?;
                    path_total = path_total
                        .checked_add(path.elements.len())
                        .ok_or(DisplayListError::LimitExceeded)?;
                }
                Command::DrawGlyphRun { color, glyphs, .. } => {
                    color.validate()?;
                    glyph_total = glyph_total
                        .checked_add(glyphs.len())
                        .ok_or(DisplayListError::LimitExceeded)?;
                    for g in glyphs {
                        validate_point(g.position)?;
                        validate_point(g.advance)?;
                    }
                }
                Command::DrawImage { bounds, .. } | Command::Link { bounds, .. } => {
                    validate_rect(*bounds, false)?
                }
                Command::PushClip { bounds } => {
                    validate_rect(*bounds, false)?;
                    clip_depth += 1;
                    if clip_depth > limits::MAX_STACK_DEPTH {
                        return Err(DisplayListError::LimitExceeded);
                    }
                    let parent_empty = *clip_empty.last().ok_or(DisplayListError::InvalidState)?;
                    let parent = *clip_stack.last().ok_or(DisplayListError::InvalidState)?;
                    let intersection = if parent_empty {
                        None
                    } else {
                        parent.map_or(Some(*bounds), |r| r.intersection(*bounds))
                    };
                    clip_stack.push(intersection);
                    clip_empty.push(parent_empty || (parent.is_some() && intersection.is_none()));
                }
                Command::PopClip => {
                    if clip_depth == 0 {
                        return Err(DisplayListError::InvalidState);
                    }
                    clip_depth -= 1;
                    clip_stack.pop();
                    clip_empty.pop();
                }
                Command::PushTransform { transform } => {
                    transform.validate()?;
                    let composite = transform_stack
                        .last()
                        .ok_or(DisplayListError::InvalidState)?
                        .then(*transform)?;
                    transform_stack.push(composite);
                    if transform_stack.len() - 1 > limits::MAX_STACK_DEPTH {
                        return Err(DisplayListError::LimitExceeded);
                    }
                }
                Command::PopTransform => {
                    if transform_stack.len() == 1 {
                        return Err(DisplayListError::InvalidState);
                    }
                    transform_stack.pop();
                }
                Command::Structure { role, .. } => validate_string(role)?,
            }
            if path_total > limits::MAX_PATH_ELEMENTS || glyph_total > limits::MAX_GLYPHS {
                return Err(DisplayListError::LimitExceeded);
            }
            if let Command::Link { target, .. } = command {
                validate_string(target)?;
            }
        }
        if clip_depth != 0 || transform_stack.len() != 1 {
            return Err(DisplayListError::InvalidState);
        }
        Ok(())
    }
    /// Encodes this validated list into deterministic little-endian bytes.
    pub fn encode(&self) -> Result<Vec<u8>, DisplayListError> {
        self.encode_with_limit(limits::MAX_BYTES)
    }
    fn encode_with_limit(&self, max_bytes: usize) -> Result<Vec<u8>, DisplayListError> {
        let expected_len = self.encoded_len()?;
        if expected_len > max_bytes || expected_len > limits::MAX_BYTES {
            return Err(DisplayListError::LimitExceeded);
        }
        self.validate()?;
        let mut out = Vec::with_capacity(expected_len);
        out.extend_from_slice(&SIGNATURE);
        put_u16(&mut out, FORMAT_MAJOR);
        put_u16(&mut out, FORMAT_MINOR);
        put_rect(&mut out, self.page.bounds)?;
        put_u32_len(&mut out, self.commands.len())?;
        for cmd in &self.commands {
            encode_command(&mut out, cmd)?;
        }
        debug_assert_eq!(out.len(), expected_len);
        Ok(out)
    }
    fn encoded_len(&self) -> Result<usize, DisplayListError> {
        let mut total = 44usize;
        for command in &self.commands {
            let size = match command {
                Command::FillPath { path, .. } => 18usize.checked_add(path_encoded_len(path)?),
                Command::StrokePath { path, .. } => 35usize.checked_add(path_encoded_len(path)?),
                Command::DrawGlyphRun { glyphs, .. } => 29usize.checked_add(
                    glyphs
                        .len()
                        .checked_mul(36)
                        .ok_or(DisplayListError::LimitExceeded)?,
                ),
                Command::DrawImage { .. } => Some(41),
                Command::PushClip { .. } => Some(33),
                Command::PopClip | Command::PopTransform => Some(1),
                Command::PushTransform { .. } => Some(49),
                Command::Link { target, .. } => 37usize.checked_add(target.len()),
                Command::Structure { role, .. } => 13usize.checked_add(role.len()),
            }
            .ok_or(DisplayListError::LimitExceeded)?;
            total = total
                .checked_add(size)
                .ok_or(DisplayListError::LimitExceeded)?;
        }
        Ok(total)
    }
    /// Strictly decodes a complete stream. No trailing bytes or partial list is accepted.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayListError> {
        if bytes.len() > limits::MAX_BYTES {
            return Err(DisplayListError::LimitExceeded);
        }
        let mut r = Reader {
            bytes,
            at: 0,
            path_elements: 0,
            glyphs: 0,
        };
        if r.take(4)? != SIGNATURE {
            return Err(DisplayListError::InvalidFormat);
        }
        if r.u16()? != FORMAT_MAJOR || r.u16()? != FORMAT_MINOR {
            return Err(DisplayListError::InvalidFormat);
        }
        let bounds = r.rect()?;
        let page = PageInfo::new(bounds)?;
        let count = r.count(limits::MAX_COMMANDS)?;
        if count > r.remaining() {
            return Err(DisplayListError::InvalidFormat);
        }
        let mut commands = Vec::new();
        let mut clip_depth = 0usize;
        let mut transform_depth = 0usize;
        for _ in 0..count {
            let command = decode_command(&mut r)?;
            match command {
                Command::PushClip { .. } => {
                    clip_depth += 1;
                    if clip_depth > limits::MAX_STACK_DEPTH {
                        return Err(DisplayListError::LimitExceeded);
                    }
                }
                Command::PopClip => {
                    if clip_depth == 0 {
                        return Err(DisplayListError::InvalidState);
                    }
                    clip_depth -= 1;
                }
                Command::PushTransform { .. } => {
                    transform_depth += 1;
                    if transform_depth > limits::MAX_STACK_DEPTH {
                        return Err(DisplayListError::LimitExceeded);
                    }
                }
                Command::PopTransform => {
                    if transform_depth == 0 {
                        return Err(DisplayListError::InvalidState);
                    }
                    transform_depth -= 1;
                }
                _ => {}
            }
            commands.push(command);
        }
        if clip_depth != 0 || transform_depth != 0 {
            return Err(DisplayListError::InvalidState);
        }
        if r.at != bytes.len() {
            return Err(DisplayListError::InvalidFormat);
        }
        Self::new(page, commands)
    }
}

fn path_encoded_len(path: &DisplayPath) -> Result<usize, DisplayListError> {
    path.elements.iter().try_fold(4usize, |total, element| {
        let size = match element {
            PathElement::MoveTo(_) | PathElement::LineTo(_) => 17,
            PathElement::QuadraticTo { .. } => 33,
            PathElement::CubicTo { .. } => 49,
            PathElement::Close => 1,
        };
        total
            .checked_add(size)
            .ok_or(DisplayListError::LimitExceeded)
    })
}

fn validate_point(p: Point) -> Result<(), DisplayListError> {
    Point::try_new(p.x(), p.y()).map(|_| ()).map_err(Into::into)
}
fn validate_rect(r: Rect, positive: bool) -> Result<(), DisplayListError> {
    validate_point(r.min())?;
    validate_point(r.max())?;
    if !r.width().is_finite()
        || !r.height().is_finite()
        || (positive && (r.width() <= 0.0 || r.height() <= 0.0))
    {
        return Err(DisplayListError::InvalidValue);
    }
    Ok(())
}
fn validate_string(s: &str) -> Result<(), DisplayListError> {
    if s.len() > limits::MAX_RESOURCE_BYTES {
        Err(DisplayListError::LimitExceeded)
    } else {
        Ok(())
    }
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f64(out: &mut Vec<u8>, v: f64) -> Result<(), DisplayListError> {
    if !v.is_finite() {
        return Err(DisplayListError::InvalidValue);
    }
    out.extend_from_slice(&v.to_le_bytes());
    Ok(())
}
fn put_f32(out: &mut Vec<u8>, v: f32) -> Result<(), DisplayListError> {
    if !v.is_finite() {
        return Err(DisplayListError::InvalidValue);
    }
    out.extend_from_slice(&v.to_le_bytes());
    Ok(())
}
fn put_u32_len(out: &mut Vec<u8>, n: usize) -> Result<(), DisplayListError> {
    put_u32(
        out,
        u32::try_from(n).map_err(|_| DisplayListError::LimitExceeded)?,
    );
    Ok(())
}
fn put_point(out: &mut Vec<u8>, p: Point) -> Result<(), DisplayListError> {
    validate_point(p)?;
    put_f64(out, p.x())?;
    put_f64(out, p.y())
}
fn put_rect(out: &mut Vec<u8>, r: Rect) -> Result<(), DisplayListError> {
    validate_rect(r, false)?;
    put_point(out, r.min())?;
    put_point(out, r.max())
}
fn put_color(out: &mut Vec<u8>, c: Color) -> Result<(), DisplayListError> {
    c.validate()?;
    for v in [c.red, c.green, c.blue, c.alpha] {
        put_f32(out, v)?;
    }
    Ok(())
}
fn put_str(out: &mut Vec<u8>, s: &str) -> Result<(), DisplayListError> {
    validate_string(s)?;
    put_u32_len(out, s.len())?;
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
fn put_transform(out: &mut Vec<u8>, t: DisplayTransform) -> Result<(), DisplayListError> {
    t.validate()?;
    for v in [t.a, t.b, t.c, t.d, t.e, t.f] {
        put_f64(out, v)?;
    }
    Ok(())
}

fn encode_command(out: &mut Vec<u8>, c: &Command) -> Result<(), DisplayListError> {
    match c {
        Command::FillPath { path, color, rule } => {
            out.push(1);
            out.push(match rule {
                FillRule::NonZero => 0,
                FillRule::EvenOdd => 1,
            });
            put_color(out, *color)?;
            put_path(out, path)?;
        }
        Command::StrokePath { path, color, style } => {
            out.push(2);
            put_color(out, *color)?;
            put_f64(out, style.width)?;
            out.push(style.cap as u8);
            out.push(style.join as u8);
            put_f64(out, style.miter_limit)?;
            put_path(out, path)?;
        }
        Command::DrawGlyphRun {
            font_id,
            color,
            glyphs,
        } => {
            out.push(3);
            put_u64(out, *font_id);
            put_color(out, *color)?;
            put_u32_len(out, glyphs.len())?;
            for g in glyphs {
                put_u32(out, g.glyph_id);
                put_point(out, g.position)?;
                put_point(out, g.advance)?;
            }
        }
        Command::DrawImage { image_id, bounds } => {
            out.push(4);
            put_u64(out, *image_id);
            put_rect(out, *bounds)?;
        }
        Command::PushClip { bounds } => {
            out.push(5);
            put_rect(out, *bounds)?;
        }
        Command::PopClip => out.push(6),
        Command::PushTransform { transform } => {
            out.push(7);
            put_transform(out, *transform)?;
        }
        Command::PopTransform => out.push(8),
        Command::Link { bounds, target } => {
            out.push(9);
            put_rect(out, *bounds)?;
            put_str(out, target)?;
        }
        Command::Structure { role, node_id } => {
            out.push(10);
            put_str(out, role)?;
            put_u64(out, *node_id);
        }
    }
    Ok(())
}

fn put_path(out: &mut Vec<u8>, path: &DisplayPath) -> Result<(), DisplayListError> {
    put_u32_len(out, path.elements.len())?;
    for e in &path.elements {
        match *e {
            PathElement::MoveTo(p) => {
                out.push(0);
                put_point(out, p)?;
            }
            PathElement::LineTo(p) => {
                out.push(1);
                put_point(out, p)?;
            }
            PathElement::QuadraticTo { control, to } => {
                out.push(2);
                put_point(out, control)?;
                put_point(out, to)?;
            }
            PathElement::CubicTo {
                control1,
                control2,
                to,
            } => {
                out.push(3);
                put_point(out, control1)?;
                put_point(out, control2)?;
                put_point(out, to)?;
            }
            PathElement::Close => out.push(4),
        }
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    path_elements: usize,
    glyphs: usize,
}
impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DisplayListError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(DisplayListError::InvalidFormat)?;
        let s = self
            .bytes
            .get(self.at..end)
            .ok_or(DisplayListError::InvalidFormat)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DisplayListError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DisplayListError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| DisplayListError::InvalidFormat)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, DisplayListError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| DisplayListError::InvalidFormat)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, DisplayListError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| DisplayListError::InvalidFormat)?,
        ))
    }
    fn f64(&mut self) -> Result<f64, DisplayListError> {
        let v = f64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| DisplayListError::InvalidFormat)?,
        );
        if !v.is_finite() {
            Err(DisplayListError::InvalidValue)
        } else {
            Ok(v)
        }
    }
    fn f32(&mut self) -> Result<f32, DisplayListError> {
        let v = f32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| DisplayListError::InvalidFormat)?,
        );
        if !v.is_finite() {
            Err(DisplayListError::InvalidValue)
        } else {
            Ok(v)
        }
    }
    fn count(&mut self, max: usize) -> Result<usize, DisplayListError> {
        let n = usize::try_from(self.u32()?).map_err(|_| DisplayListError::LimitExceeded)?;
        if n > max {
            Err(DisplayListError::LimitExceeded)
        } else {
            Ok(n)
        }
    }
    fn point(&mut self) -> Result<Point, DisplayListError> {
        Ok(Point::try_new(self.f64()?, self.f64()?)?)
    }
    fn rect(&mut self) -> Result<Rect, DisplayListError> {
        let a = self.point()?;
        let b = self.point()?;
        Ok(Rect::new(a, b))
    }
    fn color(&mut self) -> Result<Color, DisplayListError> {
        Color::new(self.f32()?, self.f32()?, self.f32()?, self.f32()?)
    }
    fn string(&mut self) -> Result<String, DisplayListError> {
        let n = self.count(limits::MAX_RESOURCE_BYTES)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| DisplayListError::InvalidFormat)
    }
    fn transform(&mut self) -> Result<DisplayTransform, DisplayListError> {
        DisplayTransform::new(
            self.f64()?,
            self.f64()?,
            self.f64()?,
            self.f64()?,
            self.f64()?,
            self.f64()?,
        )
    }
}

fn decode_command(r: &mut Reader<'_>) -> Result<Command, DisplayListError> {
    Ok(match r.u8()? {
        1 => {
            let rule = match r.u8()? {
                0 => FillRule::NonZero,
                1 => FillRule::EvenOdd,
                _ => return Err(DisplayListError::InvalidFormat),
            };
            Command::FillPath {
                color: r.color()?,
                path: read_path(r)?,
                rule,
            }
        }
        2 => {
            let color = r.color()?;
            let width = r.f64()?;
            let cap = match r.u8()? {
                0 => LineCap::Butt,
                1 => LineCap::Round,
                2 => LineCap::Square,
                _ => return Err(DisplayListError::InvalidFormat),
            };
            let join = match r.u8()? {
                0 => LineJoin::Miter,
                1 => LineJoin::Round,
                2 => LineJoin::Bevel,
                _ => return Err(DisplayListError::InvalidFormat),
            };
            let miter_limit = r.f64()?;
            Command::StrokePath {
                color,
                style: StrokeStyle::new(width, cap, join, miter_limit)?,
                path: read_path(r)?,
            }
        }
        3 => {
            let font_id = r.u64()?;
            let color = r.color()?;
            let n = r.count(limits::MAX_GLYPHS.saturating_sub(r.glyphs))?;
            const GLYPH_BYTES: usize = 4 + 4 * 8;
            if n > r.remaining() / GLYPH_BYTES {
                return Err(DisplayListError::InvalidFormat);
            }
            r.glyphs = r
                .glyphs
                .checked_add(n)
                .ok_or(DisplayListError::LimitExceeded)?;
            let mut glyphs = Vec::with_capacity(n);
            for _ in 0..n {
                glyphs.push(PositionedGlyph {
                    glyph_id: r.u32()?,
                    position: r.point()?,
                    advance: r.point()?,
                });
            }
            Command::DrawGlyphRun {
                font_id,
                color,
                glyphs,
            }
        }
        4 => Command::DrawImage {
            image_id: r.u64()?,
            bounds: r.rect()?,
        },
        5 => Command::PushClip { bounds: r.rect()? },
        6 => Command::PopClip,
        7 => Command::PushTransform {
            transform: r.transform()?,
        },
        8 => Command::PopTransform,
        9 => Command::Link {
            bounds: r.rect()?,
            target: r.string()?,
        },
        10 => Command::Structure {
            role: r.string()?,
            node_id: r.u64()?,
        },
        _ => return Err(DisplayListError::InvalidFormat),
    })
}
fn read_path(r: &mut Reader<'_>) -> Result<DisplayPath, DisplayListError> {
    let n = r.count(limits::MAX_PATH_ELEMENTS.saturating_sub(r.path_elements))?;
    if n > r.remaining() {
        return Err(DisplayListError::InvalidFormat);
    }
    r.path_elements = r
        .path_elements
        .checked_add(n)
        .ok_or(DisplayListError::LimitExceeded)?;
    let mut elements = Vec::new();
    for _ in 0..n {
        elements.push(match r.u8()? {
            0 => PathElement::MoveTo(r.point()?),
            1 => PathElement::LineTo(r.point()?),
            2 => PathElement::QuadraticTo {
                control: r.point()?,
                to: r.point()?,
            },
            3 => PathElement::CubicTo {
                control1: r.point()?,
                control2: r.point()?,
                to: r.point()?,
            },
            4 => PathElement::Close,
            _ => return Err(DisplayListError::InvalidFormat),
        });
    }
    DisplayPath::new(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: f64, y: f64) -> Point {
        Point::try_new(x, y).unwrap()
    }
    fn page() -> PageInfo {
        PageInfo::new(Rect::from_xywh(0.0, 0.0, 100.0, 200.0).unwrap()).unwrap()
    }
    fn complex() -> DisplayList {
        DisplayList::new(
            page(),
            vec![
                Command::PushClip {
                    bounds: Rect::from_xywh(1.0, 2.0, 80.0, 90.0).unwrap(),
                },
                Command::PushTransform {
                    transform: DisplayTransform::new(1.0, 0.25, 0.5, 1.0, 3.0, 4.0).unwrap(),
                },
                Command::FillPath {
                    path: DisplayPath::new(vec![
                        PathElement::MoveTo(p(1.0, 2.0)),
                        PathElement::CubicTo {
                            control1: p(2.0, 3.0),
                            control2: p(4.0, 5.0),
                            to: p(6.0, 7.0),
                        },
                        PathElement::Close,
                    ])
                    .unwrap(),
                    color: Color::new(0.1, 0.2, 0.3, 0.4).unwrap(),
                    rule: FillRule::EvenOdd,
                },
                Command::StrokePath {
                    path: DisplayPath::new(vec![
                        PathElement::MoveTo(p(0.0, 0.0)),
                        PathElement::LineTo(p(3.0, 3.0)),
                    ])
                    .unwrap(),
                    color: Color::new(1.0, 0.0, 0.0, 1.0).unwrap(),
                    style: StrokeStyle::new(2.0, LineCap::Round, LineJoin::Bevel, 4.0).unwrap(),
                },
                Command::DrawGlyphRun {
                    font_id: 42,
                    color: Color::new(0.0, 0.0, 0.0, 0.8).unwrap(),
                    glyphs: vec![PositionedGlyph {
                        glyph_id: 7,
                        position: p(10.0, 20.0),
                        advance: p(5.0, 0.0),
                    }],
                },
                Command::DrawImage {
                    image_id: 88,
                    bounds: Rect::from_xywh(5.0, 6.0, 7.0, 8.0).unwrap(),
                },
                Command::PopTransform,
                Command::Link {
                    bounds: Rect::from_xywh(1.0, 2.0, 3.0, 4.0).unwrap(),
                    target: "doc:node/17".into(),
                },
                Command::Structure {
                    role: "paragraph".into(),
                    node_id: 17,
                },
                Command::PopClip,
            ],
        )
        .unwrap()
    }
    #[test]
    fn round_trip_preserves_every_command_and_order() {
        let list = complex();
        assert_eq!(DisplayList::decode(&list.encode().unwrap()).unwrap(), list);
    }

    #[test]
    fn encoded_lengths_match_every_opcode_and_variable_operand() {
        let color = Color::new(0.1, 0.2, 0.3, 0.4).unwrap();
        let path = DisplayPath::new(vec![
            PathElement::MoveTo(p(0.0, 0.0)),
            PathElement::LineTo(p(1.0, 1.0)),
            PathElement::QuadraticTo {
                control: p(2.0, 2.0),
                to: p(3.0, 3.0),
            },
            PathElement::CubicTo {
                control1: p(4.0, 4.0),
                control2: p(5.0, 5.0),
                to: p(6.0, 6.0),
            },
            PathElement::Close,
        ])
        .unwrap();
        let bounds = Rect::from_xywh(0.0, 0.0, 10.0, 20.0).unwrap();
        let cases = vec![
            (
                Command::FillPath {
                    path: path.clone(),
                    color,
                    rule: FillRule::EvenOdd,
                },
                44 + 18 + 121,
            ),
            (
                Command::StrokePath {
                    path,
                    color,
                    style: StrokeStyle::new(2.0, LineCap::Round, LineJoin::Miter, 4.0).unwrap(),
                },
                44 + 35 + 121,
            ),
            (
                Command::DrawGlyphRun {
                    font_id: 7,
                    color,
                    glyphs: vec![PositionedGlyph {
                        glyph_id: 9,
                        position: p(1.0, 2.0),
                        advance: p(3.0, 0.0),
                    }],
                },
                44 + 29 + 36,
            ),
            (
                Command::DrawImage {
                    image_id: 1,
                    bounds,
                },
                44 + 41,
            ),
            (Command::PushClip { bounds }, 44 + 33 + 1),
            (
                Command::PushTransform {
                    transform: DisplayTransform::identity(),
                },
                44 + 49 + 1,
            ),
            (
                Command::Link {
                    bounds,
                    target: "abc".into(),
                },
                44 + 37 + 3,
            ),
            (
                Command::Structure {
                    role: "p1".into(),
                    node_id: 1,
                },
                44 + 13 + 2,
            ),
        ];
        for (command, expected) in cases {
            let commands = match &command {
                Command::PushClip { .. } => vec![command, Command::PopClip],
                Command::PushTransform { .. } => vec![command, Command::PopTransform],
                _ => vec![command],
            };
            let list = DisplayList::new(page(), commands).unwrap();
            assert_eq!(list.encoded_len().unwrap(), expected);
            assert_eq!(list.encode().unwrap().len(), expected);
        }
    }

    #[test]
    fn encoder_refuses_over_budget_before_writing() {
        let list = DisplayList::new(
            page(),
            vec![Command::Structure {
                role: "paragraph".into(),
                node_id: 3,
            }],
        )
        .unwrap();
        let exact_len = list.encoded_len().unwrap();
        assert_eq!(
            list.encode_with_limit(exact_len - 1),
            Err(DisplayListError::LimitExceeded)
        );
        assert_eq!(list.encode_with_limit(exact_len).unwrap().len(), exact_len);
    }
    #[test]
    fn known_header_and_empty_page_encoding() {
        let bytes = DisplayList::new(page(), vec![]).unwrap().encode().unwrap();
        assert_eq!(&bytes[..8], b"ATDL\x01\x00\x00\x00");
        let mut expected = Vec::from(*b"ATDL");
        expected.extend_from_slice(&1u16.to_le_bytes());
        expected.extend_from_slice(&0u16.to_le_bytes());
        for v in [0.0f64, 0.0, 100.0, 200.0] {
            expected.extend_from_slice(&v.to_le_bytes());
        }
        expected.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(bytes, expected);
    }
    #[test]
    fn rejects_unbalanced_state_and_invalid_values() {
        assert_eq!(
            DisplayList::new(page(), vec![Command::PopClip]),
            Err(DisplayListError::InvalidState)
        );
        assert!(Color::new(f32::NAN, 0.0, 0.0, 1.0).is_err());
        assert!(PageInfo::new(Rect::from_xywh(0.0, 0.0, 0.0, 1.0).unwrap()).is_err());
    }
    #[test]
    fn independent_nested_state_stacks_balance_separately() {
        let commands = vec![
            Command::PushClip {
                bounds: Rect::from_xywh(0.0, 0.0, 20.0, 20.0).unwrap(),
            },
            Command::PushClip {
                bounds: Rect::from_xywh(30.0, 30.0, 20.0, 20.0).unwrap(),
            },
            Command::PushTransform {
                transform: DisplayTransform::new(1.0, 0.0, 0.0, 1.0, 2.0, 3.0).unwrap(),
            },
            Command::PushTransform {
                transform: DisplayTransform::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0).unwrap(),
            },
            Command::PopClip,
            Command::PopTransform,
            Command::PopTransform,
            Command::PopClip,
        ];
        assert!(DisplayList::new(page(), commands).is_ok());

        // A structurally complete stream with a lone PopClip is rejected too.
        let mut bytes = Vec::from(*b"ATDL");
        bytes.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
        bytes.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
        for value in [0.0f64, 0.0, 100.0, 200.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(6);
        assert_eq!(
            DisplayList::decode(&bytes),
            Err(DisplayListError::InvalidState)
        );
    }
    #[test]
    fn rejects_unknown_version_opcode_and_trailing_data() {
        let list = complex();
        let mut bytes = list.encode().unwrap();
        bytes[4] = 2;
        assert_eq!(
            DisplayList::decode(&bytes),
            Err(DisplayListError::InvalidFormat)
        );
        let mut bytes = list.encode().unwrap();
        bytes[8 + 32 + 4] = 255;
        assert!(DisplayList::decode(&bytes).is_err());
        let mut bytes = list.encode().unwrap();
        bytes.push(0);
        assert_eq!(
            DisplayList::decode(&bytes),
            Err(DisplayListError::InvalidFormat)
        );
    }
    #[test]
    fn every_truncation_of_complex_stream_is_rejected() {
        let bytes = complex().encode().unwrap();
        for end in 0..bytes.len() {
            assert!(
                DisplayList::decode(&bytes[..end]).is_err(),
                "accepted truncation at {end}"
            );
        }
    }
    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut state = 0x1234_5678u32;
        for len in 0..2048 {
            let mut bytes = vec![0; len];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *b = state as u8;
            }
            let _ = DisplayList::decode(&bytes);
        }
    }
    #[test]
    fn cumulative_path_and_glyph_budgets_are_enforced() {
        let path = DisplayPath {
            elements: vec![PathElement::MoveTo(p(0.0, 0.0)); limits::MAX_PATH_ELEMENTS],
        };
        let list = DisplayList {
            page: page(),
            commands: vec![
                Command::FillPath {
                    path: path.clone(),
                    color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
                    rule: FillRule::NonZero,
                },
                Command::FillPath {
                    path,
                    color: Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
                    rule: FillRule::NonZero,
                },
            ],
        };
        assert_eq!(list.validate(), Err(DisplayListError::LimitExceeded));
    }
    #[test]
    fn decoder_checks_cumulative_budgets_before_allocating() {
        let color = Color::new(0.0, 0.0, 0.0, 1.0).unwrap();
        let mut bytes = DisplayList::new(
            page(),
            vec![Command::FillPath {
                path: DisplayPath::default(),
                color,
                rule: FillRule::NonZero,
            }],
        )
        .unwrap()
        .encode()
        .unwrap();
        let count_offset = 44 + 1 + 1 + 16;
        bytes[count_offset..count_offset + 4].copy_from_slice(
            &u32::try_from(limits::MAX_PATH_ELEMENTS + 1)
                .unwrap()
                .to_le_bytes(),
        );
        assert_eq!(
            DisplayList::decode(&bytes),
            Err(DisplayListError::LimitExceeded)
        );
        let glyphs = vec![
            PositionedGlyph {
                glyph_id: 1,
                position: p(0.0, 0.0),
                advance: p(1.0, 0.0)
            };
            limits::MAX_GLYPHS + 1
        ];
        let list = DisplayList {
            page: page(),
            commands: vec![Command::DrawGlyphRun {
                font_id: 1,
                color,
                glyphs,
            }],
        };
        assert_eq!(list.validate(), Err(DisplayListError::LimitExceeded));
    }

    fn binary_header(command_count: u32) -> Vec<u8> {
        let mut bytes = Vec::from(*b"ATDL");
        bytes.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
        bytes.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
        for value in [0.0f64, 0.0, 100.0, 200.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&command_count.to_le_bytes());
        bytes
    }

    #[test]
    fn short_streams_cannot_trigger_large_declared_count_reservations() {
        let command_count = binary_header(limits::MAX_COMMANDS as u32);
        assert_eq!(
            DisplayList::decode(&command_count),
            Err(DisplayListError::InvalidFormat)
        );

        let color = Color::new(0.0, 0.0, 0.0, 1.0).unwrap();
        let mut path_stream = DisplayList::new(
            page(),
            vec![Command::FillPath {
                path: DisplayPath::default(),
                color,
                rule: FillRule::NonZero,
            }],
        )
        .unwrap()
        .encode()
        .unwrap();
        let path_count_offset = 44 + 1 + 1 + 16;
        path_stream[path_count_offset..path_count_offset + 4]
            .copy_from_slice(&(limits::MAX_PATH_ELEMENTS as u32).to_le_bytes());
        assert_eq!(
            DisplayList::decode(&path_stream),
            Err(DisplayListError::InvalidFormat)
        );

        let mut glyph_stream = DisplayList::new(
            page(),
            vec![Command::DrawGlyphRun {
                font_id: 1,
                color,
                glyphs: Vec::new(),
            }],
        )
        .unwrap()
        .encode()
        .unwrap();
        let glyph_count_offset = 44 + 1 + 8 + 16;
        glyph_stream[glyph_count_offset..glyph_count_offset + 4]
            .copy_from_slice(&(limits::MAX_GLYPHS as u32).to_le_bytes());
        assert_eq!(
            DisplayList::decode(&glyph_stream),
            Err(DisplayListError::InvalidFormat)
        );
    }

    #[test]
    fn decoder_rejects_invalid_stack_operations_as_soon_as_seen() {
        let mut pop_stream = binary_header(5000);
        pop_stream.extend(std::iter::repeat_n(6, 5000));
        assert_eq!(
            DisplayList::decode(&pop_stream),
            Err(DisplayListError::InvalidState)
        );

        let mut deep_stream = binary_header((limits::MAX_STACK_DEPTH + 1) as u32);
        for _ in 0..=limits::MAX_STACK_DEPTH {
            deep_stream.extend_from_slice(&[5]);
            for value in [0.0f64, 0.0, 1.0, 1.0] {
                deep_stream.extend_from_slice(&value.to_le_bytes());
            }
        }
        assert_eq!(
            DisplayList::decode(&deep_stream),
            Err(DisplayListError::LimitExceeded)
        );
    }
}
