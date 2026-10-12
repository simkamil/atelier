// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, deterministic software raster primitives in pixel coordinates.
//!
//! Paths use an 8 × 8 sample grid. Clips retain the same sample bits, so
//! intersecting a clip with a shape never multiplies unrelated coverages.
//! Images interpolate premultiplied colors and are returned as straight RGBA8.

#![forbid(unsafe_code)]

mod mask;
mod stroke;

use atelier_geom::{AffineTransform, GeometryError, Path, Point, Rect};
use atelier_image::{DecodeLimits, Image, ImageError, encode_png_rgba};
pub use stroke::{LineCap, LineJoin, StrokeStyle};

/// Fill rule used by both painting and path clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// Per-surface memory and per-operation work budgets. Flattening additionally
/// uses the fixed limits in `atelier-geom` before the edge budget is checked.
#[derive(Clone, Copy, Debug)]
pub struct RasterLimits {
    pub max_pixels: u64,
    pub max_edges: usize,
    pub max_work: u64,
    pub max_clip_depth: usize,
    pub max_mask_bytes: usize,
}

impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            max_pixels: 16_777_216,
            max_edges: 65_536,
            max_work: 250_000_000,
            max_clip_depth: 16,
            max_mask_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterError {
    InvalidInput,
    LimitExceeded,
    ClipStackUnderflow,
    Geometry(GeometryError),
    Image(ImageError),
}

impl core::fmt::Display for RasterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "raster error: {self:?}")
    }
}
impl std::error::Error for RasterError {}
impl From<GeometryError> for RasterError {
    fn from(value: GeometryError) -> Self {
        Self::Geometry(value)
    }
}
impl From<ImageError> for RasterError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

/// Raster surface. All fallible drawing preparations finish before pixels or
/// the clip stack are changed. Colors are sRGB bytes, without gamma conversion.
pub struct Rasterizer {
    image: Image,
    limits: RasterLimits,
    clip: Option<Vec<u64>>,
    saved_clips: Vec<Option<Vec<u64>>>,
}

impl Rasterizer {
    pub fn new(width: u32, height: u32, limits: RasterLimits) -> Result<Self, RasterError> {
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 {
            return Err(RasterError::InvalidInput);
        }
        if pixels > limits.max_pixels {
            return Err(RasterError::LimitExceeded);
        }
        let bytes = usize::try_from(pixels)
            .ok()
            .and_then(|n| n.checked_mul(4))
            .ok_or(RasterError::LimitExceeded)?;
        Ok(Self {
            image: Image {
                width,
                height,
                pixels: vec![0; bytes],
            },
            limits,
            clip: None,
            saved_clips: Vec::new(),
        })
    }

    pub fn image(&self) -> &Image {
        &self.image
    }
    pub fn into_image(self) -> Image {
        self.image
    }

    /// Clears the whole surface, independently of the clip stack.
    pub fn clear(&mut self, rgba: [u8; 4]) {
        for pixel in self.image.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&rgba);
        }
    }

    pub fn encode_png(&self, limits: DecodeLimits) -> Result<Vec<u8>, RasterError> {
        Ok(encode_png_rgba(&self.image, limits)?)
    }

    fn reserve_mask(&self) -> Result<(), RasterError> {
        let masks = 1
            + usize::from(self.clip.is_some())
            + self.saved_clips.iter().filter(|c| c.is_some()).count();
        let bytes = self
            .image
            .pixels
            .len()
            .checked_div(4)
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_mul(masks))
            .ok_or(RasterError::LimitExceeded)?;
        if bytes > self.limits.max_mask_bytes {
            return Err(RasterError::LimitExceeded);
        }
        Ok(())
    }

    fn path_mask(&self, path: &Path, rule: FillRule) -> Result<Vec<u64>, RasterError> {
        self.reserve_mask()?;
        let flat = path.flatten(1.0 / 16.0)?;
        mask::rasterize(
            &flat,
            self.image.width,
            self.image.height,
            rule,
            self.limits,
        )
    }

    pub fn fill_path(
        &mut self,
        path: &Path,
        rule: FillRule,
        color: [u8; 4],
    ) -> Result<(), RasterError> {
        let mask = self.path_mask(path, rule)?;
        self.paint_mask(&mask, color);
        Ok(())
    }

    pub fn stroke_path(
        &mut self,
        path: &Path,
        style: StrokeStyle,
        color: [u8; 4],
    ) -> Result<(), RasterError> {
        self.reserve_mask()?;
        let polygons = stroke::expand(path, style, self.limits.max_edges)?;
        let mask = mask::rasterize(
            &polygons,
            self.image.width,
            self.image.height,
            FillRule::NonZero,
            self.limits,
        )?;
        self.paint_mask(&mask, color);
        Ok(())
    }

    fn paint_mask(&mut self, mask: &[u64], color: [u8; 4]) {
        for (index, pixel) in self.image.pixels.chunks_exact_mut(4).enumerate() {
            let bits = mask[index] & self.clip.as_ref().map_or(u64::MAX, |c| c[index]);
            let coverage = f64::from(bits.count_ones()) / 64.0;
            let alpha = f64::from(color[3]) * coverage;
            blend(
                pixel,
                [
                    f64::from(color[0]) * alpha / 255.0,
                    f64::from(color[1]) * alpha / 255.0,
                    f64::from(color[2]) * alpha / 255.0,
                    alpha,
                ],
            );
        }
    }

    pub fn push_clip_path(&mut self, path: &Path, rule: FillRule) -> Result<(), RasterError> {
        if self.saved_clips.len() >= self.limits.max_clip_depth {
            return Err(RasterError::LimitExceeded);
        }
        let mut next = self.path_mask(path, rule)?;
        if let Some(current) = &self.clip {
            for (new, old) in next.iter_mut().zip(current) {
                *new &= *old;
            }
        }
        self.saved_clips.push(self.clip.take());
        self.clip = Some(next);
        Ok(())
    }

    pub fn push_clip_rect(&mut self, rect: Rect) -> Result<(), RasterError> {
        let mut path = Path::new();
        path.move_to(rect.min());
        path.line_to(Point::try_new(rect.right(), rect.top())?)?;
        path.line_to(rect.max())?;
        path.line_to(Point::try_new(rect.left(), rect.bottom())?)?;
        path.close()?;
        self.push_clip_path(&path, FillRule::NonZero)
    }

    pub fn pop_clip(&mut self) -> Result<(), RasterError> {
        self.clip = self
            .saved_clips
            .pop()
            .ok_or(RasterError::ClipStackUnderflow)?;
        Ok(())
    }

    /// Maps the source image rectangle [0,width] × [0,height] to the surface.
    /// Pixel centers are at half integers. Outside the rectangle is transparent;
    /// bilinear sampling clamps at its edges and interpolates premultiplied RGBA.
    pub fn draw_image(
        &mut self,
        source: &Image,
        transform: AffineTransform,
    ) -> Result<(), RasterError> {
        let count = u64::from(source.width) * u64::from(source.height);
        let expected = usize::try_from(count).ok().and_then(|n| n.checked_mul(4));
        if source.width == 0 || source.height == 0 || expected != Some(source.pixels.len()) {
            return Err(RasterError::InvalidInput);
        }
        if count > self.limits.max_pixels {
            return Err(RasterError::LimitExceeded);
        }
        let inverse = transform.inverse()?;
        let corners = [
            (0.0, 0.0),
            (f64::from(source.width), 0.0),
            (0.0, f64::from(source.height)),
            (f64::from(source.width), f64::from(source.height)),
        ];
        let mut points = Vec::new();
        for (x, y) in corners {
            points.push(transform.apply(Point::try_new(x, y)?)?);
        }
        mask::validate_points(&points)?;
        let (x0, y0, x1, y1) = mask::pixel_bounds(&points, self.image.width, self.image.height);
        let work = u64::from(x1 - x0) * u64::from(y1 - y0) * 64;
        if work > self.limits.max_work {
            return Err(RasterError::LimitExceeded);
        }
        // Validate all inverse corner results before painting; affine extrema
        // over the bounded destination rectangle occur at its corners.
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            inverse.apply(Point::try_new(f64::from(x), f64::from(y))?)?;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let index = y as usize * self.image.width as usize + x as usize;
                let bits = self.clip.as_ref().map_or(u64::MAX, |c| c[index]);
                let mut covered = 0_u32;
                for sy in 0..8 {
                    for sx in 0..8 {
                        if bits & (1_u64 << (sy * 8 + sx)) == 0 {
                            continue;
                        }
                        let p = Point::try_new(
                            f64::from(x) + (f64::from(sx) + 0.5) / 8.0,
                            f64::from(y) + (f64::from(sy) + 0.5) / 8.0,
                        )?;
                        let p = inverse.apply(p)?;
                        if p.x() < 0.0
                            || p.y() < 0.0
                            || p.x() >= f64::from(source.width)
                            || p.y() >= f64::from(source.height)
                        {
                            continue;
                        }
                        covered += 1;
                    }
                }
                let center =
                    inverse.apply(Point::try_new(f64::from(x) + 0.5, f64::from(y) + 0.5)?)?;
                let mut sample = bilinear(source, center.x(), center.y());
                for channel in &mut sample {
                    *channel *= f64::from(covered) / 64.0;
                }
                blend(&mut self.image.pixels[index * 4..index * 4 + 4], sample);
            }
        }
        Ok(())
    }
}

fn blend(destination: &mut [u8], source: [f64; 4]) {
    if source[3] == 0.0 {
        return;
    }
    let remaining = 1.0 - source[3] / 255.0;
    let da = f64::from(destination[3]);
    let alpha = source[3] + da * remaining;
    for channel in 0..3 {
        destination[channel] =
            ((source[channel] + f64::from(destination[channel]) * da / 255.0 * remaining) * 255.0
                / alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
    }
    destination[3] = alpha.round().clamp(0.0, 255.0) as u8;
}

fn bilinear(source: &Image, x: f64, y: f64) -> [f64; 4] {
    let x = (x - 0.5).clamp(0.0, f64::from(source.width - 1));
    let y = (y - 0.5).clamp(0.0, f64::from(source.height - 1));
    let ix = x.floor() as u32;
    let iy = y.floor() as u32;
    let fx = x - f64::from(ix);
    let fy = y - f64::from(iy);
    let mut result = [0.0; 4];
    for (px, wx) in [(ix, 1.0 - fx), ((ix + 1).min(source.width - 1), fx)] {
        for (py, wy) in [(iy, 1.0 - fy), ((iy + 1).min(source.height - 1), fy)] {
            let start = (py as usize * source.width as usize + px as usize) * 4;
            let pixel = &source.pixels[start..start + 4];
            let a = f64::from(pixel[3]);
            for c in 0..3 {
                result[c] += f64::from(pixel[c]) * a / 255.0 * wx * wy;
            }
            result[3] += a * wx * wy;
        }
    }
    result
}
