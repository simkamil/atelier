// SPDX-License-Identifier: AGPL-3.0-or-later
//! Raster backend for `atelier-render` display lists (US-082).
//!
//! The list is first compiled into device-space operations (matrices composed,
//! clips intersected, glyph outlines and images resolved and checked). The page
//! is then painted tile by tile: each 64 x 64 tile owns a small [`Rasterizer`]
//! surface, so the cost of an operation follows its size and not the page size.
//! Tiles are shifted by whole pixels, which keeps the 8 x 8 sample grid of the
//! rasterizer aligned. Everything is single-threaded and uses only IEEE
//! arithmetic, except the rotation of images (see [`render_image`]).

use crate::{FillRule, LineCap, LineJoin, RasterError, RasterLimits, Rasterizer, StrokeStyle};
use atelier_geom::{AffineTransform, GeometryError, Path, Point, Rect};
use atelier_image::{
    DecodeLimits, Image, ImageError, decode_bmp, decode_gif, decode_jpeg, decode_png,
    encode_png_rgba,
};
use atelier_render::{
    Command, DisplayList, DisplayPath, DisplayTransform, FillRule as ListFillRule,
    LineCap as ListCap, LineJoin as ListJoin, PathElement, limits as list_limits,
};
pub use atelier_render::{SvgImage, SvgResourceProvider as ResourceProvider};
use std::collections::HashMap;

/// Edge of a square tile, in pixels.
const TILE: u32 = 64;
/// Page units per inch: a display-list unit is a point.
const UNITS_PER_INCH: f64 = 72.0;
/// Relative determinant below which an image transform is considered degenerate.
const DEGENERATE: f64 = 1.0e-9;

/// Failures of the raster backend. No partial image is ever returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// The display list is invalid or unbalanced, or a supplied outline is invalid.
    InvalidList,
    /// Resolution, region or limits are unusable.
    InvalidOptions,
    /// The provider has no outline or image for a referenced identifier.
    MissingResource,
    /// An image MIME type is not `image/png`, `image/jpeg`, `image/gif` or `image/bmp`.
    InvalidMime,
    /// A documented budget was exceeded; checked before the matching allocation.
    LimitExceeded,
    /// An image could not be decoded.
    Image(ImageError),
    /// The rasterizer refused an operation (for example coordinates beyond
    /// 1e9 pixels, or an edge/work budget).
    Raster(RasterError),
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "raster backend error: {self:?}")
    }
}
impl std::error::Error for RenderError {}
impl From<RasterError> for RenderError {
    fn from(value: RasterError) -> Self {
        Self::Raster(value)
    }
}
impl From<GeometryError> for RenderError {
    fn from(value: GeometryError) -> Self {
        Self::Raster(RasterError::Geometry(value))
    }
}
impl From<ImageError> for RenderError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

/// Output resolution. The page size comes from the display list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolution {
    /// Pixels per page unit.
    Scale(f64),
    /// Dots per inch, with 72 page units per inch.
    Dpi(f64),
}

impl Resolution {
    /// Pixels per page unit.
    pub fn scale(self) -> Result<f64, RenderError> {
        let scale = match self {
            Self::Scale(s) => s,
            Self::Dpi(d) => d / UNITS_PER_INCH,
        };
        if scale.is_finite() && scale > 0.0 {
            Ok(scale)
        } else {
            Err(RenderError::InvalidOptions)
        }
    }
}

/// A rectangle of the full-resolution page image, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Budgets checked before the matching allocation or work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderLimits {
    /// Longest edge of the full page image (default 2^24).
    pub max_dimension: u32,
    /// Pixels of the returned image (default 16,777,216).
    pub max_pixels: u64,
    /// Compiled drawing operations, glyphs included (default 1,000,000).
    pub max_ops: usize,
    /// Operation-to-tile assignments (default 16,777,216).
    pub max_tile_entries: usize,
    /// Estimated tile work, in abstract units (default 2,000,000,000).
    pub max_work: u64,
    /// Flattened edges of one operation inside one tile (default 65,536).
    pub max_edges: usize,
    /// Encoded bytes of one image (default 8 MiB).
    pub max_image_bytes: usize,
    /// Pixels of one decoded image (default 16,777,216).
    pub max_image_pixels: u64,
    /// Decoded bytes of all distinct images (default 128 MiB).
    pub max_decoded_bytes: usize,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_dimension: 1 << 24,
            max_pixels: 16_777_216,
            max_ops: 1_000_000,
            max_tile_entries: 16_777_216,
            max_work: 2_000_000_000,
            max_edges: 65_536,
            max_image_bytes: 8 * 1024 * 1024,
            max_image_pixels: 16_777_216,
            max_decoded_bytes: 128 * 1024 * 1024,
        }
    }
}

/// Rendering parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub resolution: Resolution,
    /// Straight RGBA8 background; `[255; 4]` (white) by default, `[0; 4]` for transparent.
    pub background: [u8; 4],
    /// Sub-area of the full page image to return; the whole page when `None`.
    pub region: Option<PixelRegion>,
    pub limits: RenderLimits,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            resolution: Resolution::Dpi(96.0),
            background: [255; 4],
            region: None,
            limits: RenderLimits::default(),
        }
    }
}

/// Pixel size of the full page image: each edge is `ceil(size * scale)`, at least 1.
pub fn page_pixel_size(
    list: &DisplayList,
    resolution: Resolution,
    limits: &RenderLimits,
) -> Result<(u32, u32), RenderError> {
    let scale = resolution.scale()?;
    let bounds = list.page.bounds;
    Ok((
        extent(bounds.width(), scale, limits.max_dimension)?,
        extent(bounds.height(), scale, limits.max_dimension)?,
    ))
}

fn extent(units: f64, scale: f64, max: u32) -> Result<u32, RenderError> {
    let pixels = (units * scale).ceil();
    if !pixels.is_finite() || pixels > f64::from(max) {
        return Err(RenderError::LimitExceeded);
    }
    Ok((pixels as u32).max(1))
}

// ---------------------------------------------------------------- matrices

/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug)]
struct Mat {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Mat {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };
    fn from_list(t: DisplayTransform) -> Self {
        Self {
            a: t.a,
            b: t.b,
            c: t.c,
            d: t.d,
            e: t.e,
            f: t.f,
        }
    }
    fn translation(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }
    /// Applies `self`, then `next` (same convention as `DisplayTransform`).
    fn then(self, next: Self) -> Self {
        Self {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }
    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
    fn determinant(self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }
    /// Determinant of the linear part after normalizing by its largest entry,
    /// or 0 when the part is null.
    fn normalized_determinant(self) -> f64 {
        let scale = self
            .a
            .abs()
            .max(self.b.abs())
            .max(self.c.abs())
            .max(self.d.abs());
        if scale == 0.0 || !scale.is_finite() {
            return 0.0;
        }
        (self.a / scale) * (self.d / scale) - (self.b / scale) * (self.c / scale)
    }
}

/// Builds a geometry transform from a general matrix. The public geometry API
/// only offers translation, scale and rotation, so a matrix with a rotation or
/// shear part is decomposed as `R(phi) * diag(s1, s2) * R(theta)` (2 x 2 SVD).
fn affine(m: Mat) -> Result<AffineTransform, RenderError> {
    if m.b == 0.0 && m.c == 0.0 {
        return Ok(AffineTransform::scale(m.a, m.d)?.then(AffineTransform::translation(m.e, m.f)?)?);
    }
    let e = (m.a + m.d) / 2.0;
    let f = (m.a - m.d) / 2.0;
    let g = (m.b + m.c) / 2.0;
    let h = (m.b - m.c) / 2.0;
    let q = e.hypot(h);
    let r = f.hypot(g);
    let a1 = g.atan2(f);
    let a2 = h.atan2(e);
    Ok(AffineTransform::rotation((a2 - a1) / 2.0)?
        .then(AffineTransform::scale(q + r, q - r)?)?
        .then(AffineTransform::rotation((a2 + a1) / 2.0)?)?
        .then(AffineTransform::translation(m.e, m.f)?)?)
}

// ------------------------------------------------------------- compilation

#[derive(Clone, Copy)]
enum Kind<'a> {
    Fill {
        path: &'a DisplayPath,
        rule: FillRule,
        color: [u8; 4],
    },
    Stroke {
        path: &'a DisplayPath,
        style: StrokeStyle,
        color: [u8; 4],
    },
    Image {
        index: usize,
    },
}

/// One device-space drawing operation. `matrix` maps the path (or the image
/// rectangle `[0,w] x [0,h]`) to pixels of the full-resolution page image.
#[derive(Clone, Copy)]
struct Op<'a> {
    kind: Kind<'a>,
    matrix: Mat,
    /// Conservative pixel bounds `[x0, y0, x1, y1]`.
    bounds: [f64; 4],
    /// Effective clip rectangle in pixels, if any.
    clip: Option<[f64; 4]>,
    cost: u64,
}

#[derive(Clone, Copy)]
struct Glyph<'a> {
    path: &'a DisplayPath,
    local: Option<[f64; 4]>,
}

struct Compiler<'a, P: ?Sized> {
    provider: &'a P,
    limits: RenderLimits,
    ops: Vec<Op<'a>>,
    images: Vec<Image>,
    image_index: HashMap<u64, usize>,
    decoded_bytes: usize,
    glyphs: HashMap<(u64, u32), Glyph<'a>>,
    glyph_elements: usize,
}

fn color_bytes(color: atelier_render::Color) -> [u8; 4] {
    let byte = |v: f32| (f64::from(v) * 255.0).round().clamp(0.0, 255.0) as u8;
    [
        byte(color.red),
        byte(color.green),
        byte(color.blue),
        byte(color.alpha),
    ]
}

fn element_points(element: &PathElement, mut visit: impl FnMut(Point)) {
    match *element {
        PathElement::MoveTo(p) | PathElement::LineTo(p) => visit(p),
        PathElement::QuadraticTo { control, to } => {
            visit(control);
            visit(to);
        }
        PathElement::CubicTo {
            control1,
            control2,
            to,
        } => {
            visit(control1);
            visit(control2);
            visit(to);
        }
        PathElement::Close => {}
    }
}

/// Bounds of all control points; `None` when the path has no point.
fn local_bounds(path: &DisplayPath) -> Option<[f64; 4]> {
    let mut bounds: Option<[f64; 4]> = None;
    for element in &path.elements {
        element_points(element, |p| {
            let b = bounds.get_or_insert([p.x(), p.y(), p.x(), p.y()]);
            b[0] = b[0].min(p.x());
            b[1] = b[1].min(p.y());
            b[2] = b[2].max(p.x());
            b[3] = b[3].max(p.y());
        });
    }
    bounds
}

/// Converts to a geometry path, mapping each point with `m` then shifting by
/// `(-ox, -oy)`. Also enforces the move/segment rules of `atelier-geom`.
fn to_path(path: &DisplayPath, m: Mat, ox: f64, oy: f64) -> Result<Path, RenderError> {
    let mut out = Path::new();
    let map = |p: Point| -> Result<Point, RenderError> {
        let (x, y) = m.apply(p.x(), p.y());
        Ok(Point::try_new(x - ox, y - oy)?)
    };
    for element in &path.elements {
        match *element {
            PathElement::MoveTo(p) => out.move_to(map(p)?),
            PathElement::LineTo(p) => out.line_to(map(p)?)?,
            PathElement::QuadraticTo { control, to } => {
                out.quadratic_to(map(control)?, map(to)?)?;
            }
            PathElement::CubicTo {
                control1,
                control2,
                to,
            } => out.cubic_to(map(control1)?, map(control2)?, map(to)?)?,
            PathElement::Close => out.close()?,
        }
    }
    Ok(out)
}

impl<'a, P: ResourceProvider + ?Sized> Compiler<'a, P> {
    fn glyph(&mut self, font_id: u64, glyph_id: u32) -> Result<Glyph<'a>, RenderError> {
        if let Some(glyph) = self.glyphs.get(&(font_id, glyph_id)) {
            return Ok(*glyph);
        }
        if self.glyphs.len() >= list_limits::MAX_GLYPHS {
            return Err(RenderError::LimitExceeded);
        }
        let path = self
            .provider
            .glyph_outline(font_id, glyph_id)
            .ok_or(RenderError::MissingResource)?;
        let total = self
            .glyph_elements
            .checked_add(path.elements.len())
            .filter(|n| *n <= list_limits::MAX_PATH_ELEMENTS)
            .ok_or(RenderError::LimitExceeded)?;
        // Validates the move/segment rules of the supplied outline.
        to_path(path, Mat::IDENTITY, 0.0, 0.0).map_err(|_| RenderError::InvalidList)?;
        self.glyph_elements = total;
        let glyph = Glyph {
            path,
            local: local_bounds(path),
        };
        self.glyphs
            .try_reserve(1)
            .map_err(|_| RenderError::LimitExceeded)?;
        self.glyphs.insert((font_id, glyph_id), glyph);
        Ok(glyph)
    }

    fn image(&mut self, image_id: u64) -> Result<usize, RenderError> {
        if let Some(index) = self.image_index.get(&image_id) {
            return Ok(*index);
        }
        let source = self
            .provider
            .image(image_id)
            .ok_or(RenderError::MissingResource)?;
        if source.bytes.len() > self.limits.max_image_bytes {
            return Err(RenderError::LimitExceeded);
        }
        let decode = match source.mime_type {
            "image/png" => decode_png,
            "image/jpeg" => decode_jpeg,
            "image/gif" => decode_gif,
            "image/bmp" => decode_bmp,
            _ => return Err(RenderError::InvalidMime),
        };
        let decoded = decode(
            source.bytes,
            DecodeLimits {
                max_pixels: self.limits.max_image_pixels,
                max_input_bytes: self.limits.max_image_bytes,
            },
        )?;
        let expected = u64::from(decoded.width)
            .checked_mul(u64::from(decoded.height))
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_mul(4));
        if decoded.width == 0 || decoded.height == 0 || expected != Some(decoded.pixels.len()) {
            return Err(RenderError::Image(ImageError::InvalidData(
                "inconsistent decoded image",
            )));
        }
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(decoded.pixels.len())
            .filter(|n| *n <= self.limits.max_decoded_bytes)
            .ok_or(RenderError::LimitExceeded)?;
        self.images
            .try_reserve(1)
            .map_err(|_| RenderError::LimitExceeded)?;
        self.images.push(decoded);
        self.image_index.insert(image_id, self.images.len() - 1);
        Ok(self.images.len() - 1)
    }

    /// Records an operation whose local bounding box is `local`.
    fn push(
        &mut self,
        kind: Kind<'a>,
        matrix: Mat,
        local: [f64; 4],
        margin: f64,
        clip: Option<[f64; 4]>,
        cost: u64,
    ) -> Result<(), RenderError> {
        if !matrix.is_finite() || !margin.is_finite() {
            return Err(RenderError::InvalidList);
        }
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (x, y) in [
            (local[0], local[1]),
            (local[2], local[1]),
            (local[0], local[3]),
            (local[2], local[3]),
        ] {
            let (px, py) = matrix.apply(x, y);
            bounds[0] = bounds[0].min(px);
            bounds[1] = bounds[1].min(py);
            bounds[2] = bounds[2].max(px);
            bounds[3] = bounds[3].max(py);
        }
        // One extra pixel covers anti-aliasing and rounding of the sample grid.
        let margin = margin + 1.0;
        bounds = [
            bounds[0] - margin,
            bounds[1] - margin,
            bounds[2] + margin,
            bounds[3] + margin,
        ];
        if !bounds.iter().all(|v| v.is_finite()) {
            return Err(RenderError::InvalidList);
        }
        if self.ops.len() >= self.limits.max_ops {
            return Err(RenderError::LimitExceeded);
        }
        self.ops
            .try_reserve(1)
            .map_err(|_| RenderError::LimitExceeded)?;
        self.ops.push(Op {
            kind,
            matrix,
            bounds,
            clip,
            cost,
        });
        Ok(())
    }
}

fn path_cost(elements: usize) -> u64 {
    (elements as u64 + 1) * 512 + 4096
}

const IMAGE_COST: u64 = 300_000;

fn compile<'a, P: ResourceProvider + ?Sized>(
    list: &'a DisplayList,
    provider: &'a P,
    scale: f64,
    limits: RenderLimits,
) -> Result<Compiler<'a, P>, RenderError> {
    let page = list.page.bounds;
    let device = Mat {
        a: scale,
        b: 0.0,
        c: 0.0,
        d: scale,
        e: -page.left() * scale,
        f: -page.top() * scale,
    };
    let mut compiler = Compiler {
        provider,
        limits,
        ops: Vec::new(),
        images: Vec::new(),
        image_index: HashMap::new(),
        decoded_bytes: 0,
        glyphs: HashMap::new(),
        glyph_elements: 0,
    };
    // Same independent stacks and clip semantics as the SVG backend.
    let mut transforms = vec![Mat::IDENTITY];
    let mut clips: Vec<Option<Rect>> = vec![None];
    let mut clip_empty = vec![false];
    let to_pixels = |r: Rect| {
        [
            (r.left() - page.left()) * scale,
            (r.top() - page.top()) * scale,
            (r.right() - page.left()) * scale,
            (r.bottom() - page.top()) * scale,
        ]
    };
    for command in &list.commands {
        match command {
            Command::PushClip { bounds } => {
                let parent = *clips.last().ok_or(RenderError::InvalidList)?;
                let empty = *clip_empty.last().ok_or(RenderError::InvalidList)?;
                let intersection = if empty {
                    None
                } else {
                    parent.map_or(Some(*bounds), |r| r.intersection(*bounds))
                };
                clips.push(intersection);
                clip_empty.push(empty || (parent.is_some() && intersection.is_none()));
            }
            Command::PopClip => {
                if clips.len() <= 1 {
                    return Err(RenderError::InvalidList);
                }
                clips.pop();
                clip_empty.pop();
            }
            Command::PushTransform { transform } => {
                let current = *transforms.last().ok_or(RenderError::InvalidList)?;
                let next = current.then(Mat::from_list(*transform));
                if !next.is_finite() {
                    return Err(RenderError::InvalidList);
                }
                transforms.push(next);
            }
            Command::PopTransform => {
                if transforms.len() <= 1 {
                    return Err(RenderError::InvalidList);
                }
                transforms.pop();
            }
            Command::FillPath { path, color, rule } => {
                if *clip_empty.last().ok_or(RenderError::InvalidList)? {
                    continue;
                }
                let Some(local) = local_bounds(path) else {
                    continue;
                };
                let total = transforms
                    .last()
                    .ok_or(RenderError::InvalidList)?
                    .then(device);
                let clip = clips.last().ok_or(RenderError::InvalidList)?.map(to_pixels);
                let rule = match rule {
                    ListFillRule::NonZero => FillRule::NonZero,
                    ListFillRule::EvenOdd => FillRule::EvenOdd,
                };
                compiler.push(
                    Kind::Fill {
                        path,
                        rule,
                        color: color_bytes(*color),
                    },
                    total,
                    local,
                    0.0,
                    clip,
                    path_cost(path.elements.len()),
                )?;
            }
            Command::StrokePath { path, color, style } => {
                if *clip_empty.last().ok_or(RenderError::InvalidList)? {
                    continue;
                }
                let Some(local) = local_bounds(path) else {
                    continue;
                };
                let total = transforms
                    .last()
                    .ok_or(RenderError::InvalidList)?
                    .then(device);
                let clip = clips.last().ok_or(RenderError::InvalidList)?.map(to_pixels);
                // Isotropic approximation: the stroke is expanded in pixel space.
                let width = style.width * total.determinant().abs().sqrt();
                let half = width / 2.0;
                let raster_style = StrokeStyle {
                    width,
                    cap: match style.cap {
                        ListCap::Butt => LineCap::Butt,
                        ListCap::Round => LineCap::Round,
                        ListCap::Square => LineCap::Square,
                    },
                    join: match style.join {
                        ListJoin::Miter => LineJoin::Miter,
                        ListJoin::Round => LineJoin::Round,
                        ListJoin::Bevel => LineJoin::Bevel,
                    },
                    miter_limit: style.miter_limit,
                };
                if !width.is_finite() {
                    return Err(RenderError::InvalidList);
                }
                compiler.push(
                    Kind::Stroke {
                        path,
                        style: raster_style,
                        color: color_bytes(*color),
                    },
                    total,
                    local,
                    half * style.miter_limit.max(2.0),
                    clip,
                    path_cost(path.elements.len()) * 4,
                )?;
            }
            Command::DrawGlyphRun {
                font_id,
                color,
                glyphs,
            } => {
                let hidden = *clip_empty.last().ok_or(RenderError::InvalidList)?;
                let total = transforms
                    .last()
                    .ok_or(RenderError::InvalidList)?
                    .then(device);
                let clip = clips.last().ok_or(RenderError::InvalidList)?.map(to_pixels);
                for positioned in glyphs {
                    // Resources are resolved even when hidden, like the SVG backend.
                    let glyph = compiler.glyph(*font_id, positioned.glyph_id)?;
                    let Some(local) = glyph.local else {
                        continue;
                    };
                    if hidden {
                        continue;
                    }
                    let matrix = Mat::translation(positioned.position.x(), positioned.position.y())
                        .then(total);
                    compiler.push(
                        Kind::Fill {
                            path: glyph.path,
                            rule: FillRule::NonZero,
                            color: color_bytes(*color),
                        },
                        matrix,
                        local,
                        0.0,
                        clip,
                        path_cost(glyph.path.elements.len()),
                    )?;
                }
            }
            Command::DrawImage { image_id, bounds } => {
                let index = compiler.image(*image_id)?;
                if *clip_empty.last().ok_or(RenderError::InvalidList)? {
                    continue;
                }
                let (iw, ih) = (
                    f64::from(compiler.images[index].width),
                    f64::from(compiler.images[index].height),
                );
                let total = transforms
                    .last()
                    .ok_or(RenderError::InvalidList)?
                    .then(device);
                let matrix = Mat {
                    a: bounds.width() / iw,
                    b: 0.0,
                    c: 0.0,
                    d: bounds.height() / ih,
                    e: bounds.left(),
                    f: bounds.top(),
                }
                .then(total);
                if !matrix.is_finite() {
                    return Err(RenderError::InvalidList);
                }
                if matrix.normalized_determinant().abs() < DEGENERATE {
                    // Zero-area image: nothing to paint.
                    continue;
                }
                let clip = clips.last().ok_or(RenderError::InvalidList)?.map(to_pixels);
                compiler.push(
                    Kind::Image { index },
                    matrix,
                    [0.0, 0.0, iw, ih],
                    0.0,
                    clip,
                    IMAGE_COST,
                )?;
            }
            Command::Link { .. } | Command::Structure { .. } => {}
        }
    }
    if clips.len() != 1 || transforms.len() != 1 {
        return Err(RenderError::InvalidList);
    }
    Ok(compiler)
}

// ---------------------------------------------------------------- painting

fn paint_tile<P: ?Sized>(
    compiler: &Compiler<'_, P>,
    entries: &[u32],
    origin: (f64, f64),
    size: (u32, u32),
    background: [u8; 4],
    tile_limits: RasterLimits,
) -> Result<Image, RenderError> {
    let (ox, oy) = origin;
    let mut surface = Rasterizer::new(size.0, size.1, tile_limits)?;
    surface.clear(background);
    let mut current: Option<[f64; 4]> = None;
    let mut pushed = false;
    for &index in entries {
        let op = &compiler.ops[index as usize];
        if op.clip != current {
            if pushed {
                surface.pop_clip()?;
                pushed = false;
            }
            if let Some(c) = op.clip {
                let limit_x = f64::from(size.0) + 1.0;
                let limit_y = f64::from(size.1) + 1.0;
                let clamp = |v: f64, max: f64| v.clamp(-1.0, max);
                let rect = Rect::new(
                    Point::try_new(clamp(c[0] - ox, limit_x), clamp(c[1] - oy, limit_y))?,
                    Point::try_new(clamp(c[2] - ox, limit_x), clamp(c[3] - oy, limit_y))?,
                );
                surface.push_clip_rect(rect)?;
                pushed = true;
            }
            current = op.clip;
        }
        match op.kind {
            Kind::Fill { path, rule, color } => {
                let path = to_path(path, op.matrix, ox, oy)?;
                surface.fill_path(&path, rule, color)?;
            }
            Kind::Stroke { path, style, color } => {
                let path = to_path(path, op.matrix, ox, oy)?;
                surface.stroke_path(&path, style, color)?;
            }
            Kind::Image { index } => {
                let mut matrix = op.matrix;
                matrix.e -= ox;
                matrix.f -= oy;
                surface.draw_image(&compiler.images[index], affine(matrix)?)?;
            }
        }
    }
    Ok(surface.into_image())
}

/// Replays one display list on a raster surface and returns straight RGBA8 pixels.
///
/// Page units map to pixels with `options.resolution`; the image is
/// `ceil(page size * scale)` pixels wide and high (see [`page_pixel_size`]), or
/// `options.region` of it. Commands follow the SVG backend: the same
/// [`ResourceProvider`] (outlines and encoded images) can serve both. Fill
/// rules, stroke styles, clip rectangles (intersected, independent of the
/// transform stack) and transforms behave identically; links and structure
/// markers are ignored.
///
/// Documented approximations: a contour under a non-similarity transform uses
/// the mean scale `sqrt(|det|)` for its width; an image whose transform
/// contains a rotation or shear is built through `AffineTransform::rotation`,
/// whose trigonometry may differ in the last bit between platforms, while all
/// other output is bit-identical everywhere. Coordinates beyond 1e9 pixels in a
/// painted shape are refused by the rasterizer (`RenderError::Raster`).
pub fn render_image<P: ResourceProvider + ?Sized>(
    list: &DisplayList,
    provider: &P,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    let limits = options.limits;
    list.validate().map_err(|_| RenderError::InvalidList)?;
    let scale = options.resolution.scale()?;
    let (full_width, full_height) = page_pixel_size(list, options.resolution, &limits)?;
    let region = match options.region {
        None => PixelRegion {
            x: 0,
            y: 0,
            width: full_width,
            height: full_height,
        },
        Some(r) => {
            let fits = |start: u32, len: u32, total: u32| {
                len > 0 && start.checked_add(len).is_some_and(|end| end <= total)
            };
            if !fits(r.x, r.width, full_width) || !fits(r.y, r.height, full_height) {
                return Err(RenderError::InvalidOptions);
            }
            r
        }
    };
    let pixels = u64::from(region.width) * u64::from(region.height);
    if pixels > limits.max_pixels {
        return Err(RenderError::LimitExceeded);
    }
    let byte_len = usize::try_from(pixels)
        .ok()
        .and_then(|n| n.checked_mul(4))
        .ok_or(RenderError::LimitExceeded)?;

    let compiler = compile(list, provider, scale, limits)?;

    // Tiles are aligned on the grid of the full page image, whatever the
    // region: a pixel is always computed in the same tile, so a region is
    // bit-identical to the matching crop of the full render.
    let tile_edge = TILE as usize;
    let first_tx = region.x as usize / tile_edge;
    let first_ty = region.y as usize / tile_edge;
    let end_x = region.x as usize + region.width as usize;
    let end_y = region.y as usize + region.height as usize;
    let tiles_x = (end_x - 1) / tile_edge - first_tx + 1;
    let tiles_y = (end_y - 1) / tile_edge - first_ty + 1;
    let tile_count = tiles_x
        .checked_mul(tiles_y)
        .ok_or(RenderError::LimitExceeded)?;
    let mut buckets: Vec<Vec<u32>> = Vec::new();
    buckets
        .try_reserve_exact(tile_count)
        .map_err(|_| RenderError::LimitExceeded)?;
    buckets.resize_with(tile_count, Vec::new);
    let mut entries = 0usize;
    let mut work = 0u64;
    let (rx, ry) = (f64::from(region.x), f64::from(region.y));
    let (rx1, ry1) = (end_x as f64, end_y as f64);
    for (index, op) in compiler.ops.iter().enumerate() {
        let mut x0 = op.bounds[0].max(rx);
        let mut y0 = op.bounds[1].max(ry);
        let mut x1 = op.bounds[2].min(rx1);
        let mut y1 = op.bounds[3].min(ry1);
        if let Some(c) = op.clip {
            x0 = x0.max(c[0]);
            y0 = y0.max(c[1]);
            x1 = x1.min(c[2]);
            y1 = y1.min(c[3]);
        }
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let first_x = x0.floor() as usize / tile_edge;
        let first_y = y0.floor() as usize / tile_edge;
        let last_x = (x1.ceil() as usize).max(1).saturating_sub(1) / tile_edge;
        let last_y = (y1.ceil() as usize).max(1).saturating_sub(1) / tile_edge;
        let count = (last_x - first_x + 1) * (last_y - first_y + 1);
        entries = entries
            .checked_add(count)
            .filter(|n| *n <= limits.max_tile_entries)
            .ok_or(RenderError::LimitExceeded)?;
        work = work
            .checked_add(op.cost.saturating_mul(count as u64))
            .filter(|n| *n <= limits.max_work)
            .ok_or(RenderError::LimitExceeded)?;
        for ty in first_y..=last_y {
            for tx in first_x..=last_x {
                let bucket = &mut buckets[(ty - first_ty) * tiles_x + (tx - first_tx)];
                bucket
                    .try_reserve(1)
                    .map_err(|_| RenderError::LimitExceeded)?;
                bucket.push(index as u32);
            }
        }
    }

    let tile_pixels = u64::from(TILE) * u64::from(TILE);
    let tile_limits = RasterLimits {
        max_pixels: limits.max_image_pixels.max(tile_pixels),
        max_edges: limits.max_edges,
        max_work: 64_000_000,
        max_clip_depth: 2,
        max_mask_bytes: 4 * 1024 * 1024,
    };
    let mut pixel_data: Vec<u8> = Vec::new();
    pixel_data
        .try_reserve_exact(byte_len)
        .map_err(|_| RenderError::LimitExceeded)?;
    pixel_data.resize(byte_len, 0);
    let stride = region.width as usize * 4;
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let left = (first_tx + tx) * tile_edge;
            let top = (first_ty + ty) * tile_edge;
            let size = (
                (full_width as usize - left).min(tile_edge) as u32,
                (full_height as usize - top).min(tile_edge) as u32,
            );
            let tile = paint_tile(
                &compiler,
                &buckets[ty * tiles_x + tx],
                (left as f64, top as f64),
                size,
                options.background,
                tile_limits,
            )?;
            // Copy the part of the tile that lies inside the region.
            let from_x = left.max(region.x as usize);
            let to_x = (left + size.0 as usize).min(end_x);
            let from_y = top.max(region.y as usize);
            let to_y = (top + size.1 as usize).min(end_y);
            for y in from_y..to_y {
                let source = ((y - top) * size.0 as usize + (from_x - left)) * 4;
                let target = (y - region.y as usize) * stride + (from_x - region.x as usize) * 4;
                let len = (to_x - from_x) * 4;
                pixel_data[target..target + len]
                    .copy_from_slice(&tile.pixels[source..source + len]);
            }
        }
    }
    Ok(Image {
        width: region.width,
        height: region.height,
        pixels: pixel_data,
    })
}

/// Like [`render_image`], encoded as a deterministic PNG.
pub fn render_png<P: ResourceProvider + ?Sized>(
    list: &DisplayList,
    provider: &P,
    options: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let image = render_image(list, provider, options)?;
    Ok(encode_png_rgba(
        &image,
        DecodeLimits {
            max_pixels: options.limits.max_pixels,
            max_input_bytes: usize::MAX,
        },
    )?)
}
