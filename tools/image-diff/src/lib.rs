// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pixel comparison of RGBA images with a two-level tolerance, difference images and
//! reference-image assertions for tests (US-003).
//!
//! # Alpha
//!
//! Images are straight RGBA8 ([`Image`]). Colour channels are compared **premultiplied by
//! alpha** (`round(c * a / 255)`), the alpha channel as is. The colour of a nearly or fully
//! transparent pixel is invisible, so two pixels that look the same on any background must
//! compare as the same: `(255, 0, 0, 0)` and `(0, 0, 255, 0)` are identical, while a change of
//! alpha always counts. The per-channel difference of a pixel is the largest of the four
//! absolute differences (red, green, blue, alpha).

#![forbid(unsafe_code)]

mod reference;

pub use atelier_image::Image;
pub use reference::{
    Config, MatchError, OUT_DIR_ENV, Outcome, UPDATE_ENV, artifact_paths, assert_image_matches,
    assert_image_matches_with, check_image_matches, load_png, save_png,
};

/// How many pixels may differ by more than the channel tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaxDifferent {
    /// At most this many pixels.
    Count(u64),
    /// At most this percentage (0 to 100) of the pixels of the image.
    Percent(f64),
}

/// Two-level tolerance: a pixel is "different" when one of its channels differs by more than
/// `channel`; the images match when at most `max_different` pixels are different.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub channel: u8,
    pub max_different: MaxDifferent,
}

impl Tolerance {
    /// No difference accepted: the right choice for deterministic rendering.
    pub const fn exact() -> Self {
        Self {
            channel: 0,
            max_different: MaxDifferent::Count(0),
        }
    }

    /// Channel tolerance only; every pixel beyond it is a failure.
    pub const fn channel(channel: u8) -> Self {
        Self {
            channel,
            max_different: MaxDifferent::Count(0),
        }
    }

    /// Channel tolerance and a percentage of pixels allowed beyond it.
    pub const fn new(channel: u8, max_different_percent: f64) -> Self {
        Self {
            channel,
            max_different: MaxDifferent::Percent(max_different_percent),
        }
    }

    fn allows(&self, different: u64, total: u64) -> bool {
        match self.max_different {
            MaxDifferent::Count(n) => different <= n,
            MaxDifferent::Percent(p) => different as f64 * 100.0 <= p * total as f64,
        }
    }
}

/// Failures that prevent a comparison.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffError {
    /// Dimensions differ; no pixel comparison is attempted.
    DimensionMismatch {
        expected: (u32, u32),
        actual: (u32, u32),
    },
    /// The pixel buffer does not match `width * height * 4`.
    InvalidImage,
    /// The percentage is not a finite number in `0..=100`.
    InvalidTolerance,
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DimensionMismatch { expected, actual } => write!(
                f,
                "dimensions differ: expected {}x{}, got {}x{}",
                expected.0, expected.1, actual.0, actual.1
            ),
            Self::InvalidImage => f.write_str("pixel buffer does not match the dimensions"),
            Self::InvalidTolerance => f.write_str("percentage must be a number from 0 to 100"),
        }
    }
}
impl std::error::Error for DiffError {}

/// Bounding box of the different pixels, in pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Result of a comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub width: u32,
    pub height: u32,
    pub total_pixels: u64,
    /// Pixels with at least one channel beyond the channel tolerance.
    pub different_pixels: u64,
    /// Largest difference per channel (R, G, B premultiplied; A) over all pixels.
    pub max_difference: [u8; 4],
    /// Mean difference per channel over all pixels.
    pub mean_difference: [f64; 4],
    /// Bounding box of the different pixels, `None` when there are none.
    pub bounds: Option<Bounds>,
    /// The tolerance used.
    pub tolerance: Tolerance,
    /// Whether the images match within the tolerance.
    pub passed: bool,
}

impl Report {
    /// Proportion of different pixels, from 0 to 1.
    pub fn fraction_different(&self) -> f64 {
        if self.total_pixels == 0 {
            0.0
        } else {
            self.different_pixels as f64 / self.total_pixels as f64
        }
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "dimensions: {}x{}", self.width, self.height)?;
        writeln!(
            f,
            "different pixels: {} of {} ({:.4}%)",
            self.different_pixels,
            self.total_pixels,
            self.fraction_different() * 100.0
        )?;
        let m = self.max_difference;
        writeln!(
            f,
            "max difference (R G B A): {} {} {} {}",
            m[0], m[1], m[2], m[3]
        )?;
        let a = self.mean_difference;
        writeln!(
            f,
            "mean difference (R G B A): {:.4} {:.4} {:.4} {:.4}",
            a[0], a[1], a[2], a[3]
        )?;
        match self.bounds {
            Some(b) => writeln!(
                f,
                "bounding box: x={} y={} width={} height={}",
                b.x, b.y, b.width, b.height
            )?,
            None => writeln!(f, "bounding box: none")?,
        }
        let channel = self.tolerance.channel;
        match self.tolerance.max_different {
            MaxDifferent::Count(n) => writeln!(
                f,
                "tolerance: channel {channel}, at most {n} different pixels"
            )?,
            MaxDifferent::Percent(p) => writeln!(
                f,
                "tolerance: channel {channel}, at most {p}% different pixels"
            )?,
        }
        let verdict = if self.passed {
            "identical within tolerance"
        } else {
            "different"
        };
        write!(f, "result: {verdict}")
    }
}

fn check(image: &Image) -> Result<(), DiffError> {
    let expected = u64::from(image.width) * u64::from(image.height) * 4;
    if image.pixels.len() as u64 == expected {
        Ok(())
    } else {
        Err(DiffError::InvalidImage)
    }
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((u32::from(channel) * u32::from(alpha) + 127) / 255) as u8
}

/// Comparison value of a pixel: premultiplied colour, then alpha.
fn comparable(pixel: &[u8]) -> [u8; 4] {
    let a = pixel[3];
    [
        premultiply(pixel[0], a),
        premultiply(pixel[1], a),
        premultiply(pixel[2], a),
        a,
    ]
}

fn pixel_difference(a: &[u8], b: &[u8]) -> [u8; 4] {
    let (a, b) = (comparable(a), comparable(b));
    [
        a[0].abs_diff(b[0]),
        a[1].abs_diff(b[1]),
        a[2].abs_diff(b[2]),
        a[3].abs_diff(b[3]),
    ]
}

fn validate(expected: &Image, actual: &Image) -> Result<(), DiffError> {
    check(expected)?;
    check(actual)?;
    if (expected.width, expected.height) != (actual.width, actual.height) {
        return Err(DiffError::DimensionMismatch {
            expected: (expected.width, expected.height),
            actual: (actual.width, actual.height),
        });
    }
    Ok(())
}

/// Compares two images pixel by pixel. Never panics; different dimensions are an error.
pub fn compare(
    expected: &Image,
    actual: &Image,
    tolerance: &Tolerance,
) -> Result<Report, DiffError> {
    if let MaxDifferent::Percent(p) = tolerance.max_different
        && !(p.is_finite() && (0.0..=100.0).contains(&p))
    {
        return Err(DiffError::InvalidTolerance);
    }
    validate(expected, actual)?;
    let total = u64::from(expected.width) * u64::from(expected.height);
    let mut different = 0u64;
    let mut max = [0u8; 4];
    let mut sum = [0u64; 4];
    // (min_x, min_y, max_x, max_y)
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    let width = expected.width.max(1) as usize;
    for (i, (a, b)) in expected
        .pixels
        .chunks_exact(4)
        .zip(actual.pixels.chunks_exact(4))
        .enumerate()
    {
        let d = pixel_difference(a, b);
        let mut beyond = false;
        for c in 0..4 {
            max[c] = max[c].max(d[c]);
            sum[c] += u64::from(d[c]);
            beyond |= d[c] > tolerance.channel;
        }
        if beyond {
            different += 1;
            let (x, y) = ((i % width) as u32, (i / width) as u32);
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    let mean = sum.map(|s| {
        if total == 0 {
            0.0
        } else {
            s as f64 / total as f64
        }
    });
    Ok(Report {
        width: expected.width,
        height: expected.height,
        total_pixels: total,
        different_pixels: different,
        max_difference: max,
        mean_difference: mean,
        bounds: bounds.map(|(x0, y0, x1, y1)| Bounds {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        }),
        tolerance: *tolerance,
        passed: tolerance.allows(different, total),
    })
}

/// Colour of the pixels beyond the channel tolerance in a difference image.
pub const DIFF_MARK: [u8; 4] = [255, 0, 0, 255];

/// Difference image: the obtained image composited over white and faded to a quarter of its
/// intensity (opaque), with every pixel beyond the channel tolerance painted [`DIFF_MARK`].
pub fn diff_image(
    expected: &Image,
    actual: &Image,
    tolerance: &Tolerance,
) -> Result<Image, DiffError> {
    validate(expected, actual)?;
    let mut pixels = Vec::with_capacity(actual.pixels.len());
    for (a, b) in expected
        .pixels
        .chunks_exact(4)
        .zip(actual.pixels.chunks_exact(4))
    {
        let d = pixel_difference(a, b);
        if d.iter().any(|&v| v > tolerance.channel) {
            pixels.extend_from_slice(&DIFF_MARK);
        } else {
            let alpha = u32::from(b[3]);
            for &c in &b[..3] {
                let over_white = (u32::from(c) * alpha + 255 * (255 - alpha) + 127) / 255;
                pixels.push(((over_white + 3 * 255 + 2) / 4) as u8);
            }
            pixels.push(255);
        }
    }
    Ok(Image {
        width: expected.width,
        height: expected.height,
        pixels,
    })
}

/// Asserts a rendered image against a reference PNG, resolving the path relative to the calling
/// crate (`CARGO_MANIFEST_DIR` is read at the call site):
/// `assert_image_matches!(&image, "tests/reference/page.png", Tolerance::exact())`.
#[macro_export]
macro_rules! assert_image_matches {
    ($actual:expr, $relative:expr, $tolerance:expr) => {
        $crate::assert_image_matches(
            $actual,
            ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join($relative),
            &$tolerance,
        )
    };
}
