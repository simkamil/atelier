// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures of the editor (US-270): validation and display bitmap.
//!
//! Original bytes are kept untouched. Insertion fully decodes the picture once
//! (so a corrupt file is refused before it enters the document) but keeps only
//! the original bytes, size and EXIF orientation. The display bitmap is decoded
//! again on demand, oriented, and reduced to at most [`MAX_BITMAP_SIDE`] pixels.

use atelier_image::{
    DecodeLimits, Image, ImageError, decode_bmp, decode_gif, decode_jpeg_with_metadata, decode_png,
};
use atelier_text_model::inline_image::{
    ImageFormat, InlineImage, SniffError, oriented_size, oriented_source,
};
use std::sync::Arc;

/// Largest accepted encoded picture.
pub(crate) const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Decoded pixel budget of one picture (4096 x 4096).
pub(crate) const MAX_PIXELS: u64 = 16_777_216;
/// Longest side of a display bitmap.
pub(crate) const MAX_BITMAP_SIDE: u32 = 1600;

/// Why a picture was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImageInsertError {
    /// Not PNG, JPEG, GIF or BMP, or a variant the decoders do not support.
    Unsupported,
    /// Recognised by signature but not decodable.
    Corrupt,
    /// Encoded size or pixel count over the limits.
    TooLarge,
}

fn limits() -> DecodeLimits {
    DecodeLimits {
        max_pixels: MAX_PIXELS,
        max_input_bytes: MAX_IMAGE_BYTES,
    }
}

fn map_decode(error: ImageError) -> ImageInsertError {
    match error {
        ImageError::LimitExceeded => ImageInsertError::TooLarge,
        ImageError::Unsupported(_) => ImageInsertError::Unsupported,
        ImageError::Truncated | ImageError::InvalidData(_) => ImageInsertError::Corrupt,
    }
}

fn decode(format: ImageFormat, bytes: &[u8]) -> Result<(Image, u8), ImageInsertError> {
    let limits = limits();
    match format {
        ImageFormat::Png => decode_png(bytes, limits).map(|i| (i, 1)),
        ImageFormat::Gif => decode_gif(bytes, limits).map(|i| (i, 1)),
        ImageFormat::Bmp => decode_bmp(bytes, limits).map(|i| (i, 1)),
        ImageFormat::Jpeg => decode_jpeg_with_metadata(bytes, limits).map(|d| {
            let orientation = if (1..=8).contains(&d.orientation) {
                d.orientation
            } else {
                1
            };
            (d.image, orientation)
        }),
    }
    .map_err(map_decode)
}

/// Validates `bytes` as a PNG, JPEG, GIF or BMP and describes it. The decode
/// result is dropped: only bytes, stored size and orientation are kept.
pub(crate) fn analyze(bytes: &[u8]) -> Result<InlineImage, ImageInsertError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageInsertError::TooLarge);
    }
    let format = match ImageFormat::sniff(bytes) {
        Ok(format) => format,
        Err(SniffError::Unsupported(_) | SniffError::Unknown) => {
            return Err(ImageInsertError::Unsupported);
        }
    };
    let (image, orientation) = decode(format, bytes)?;
    InlineImage::new(
        format,
        Arc::from(bytes),
        image.width,
        image.height,
        orientation,
    )
    .map_err(|_| ImageInsertError::Corrupt)
}

/// Oriented RGBA8 pixels of a picture, straight alpha, top-down.
pub(crate) struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes `image`, applies its orientation and reduces it to at most
/// [`MAX_BITMAP_SIDE`] pixels on its longest side by area averaging.
pub(crate) fn render_bitmap(image: &InlineImage) -> Result<Bitmap, ImageInsertError> {
    let (decoded, _) = decode(image.format, &image.bytes)?;
    if decoded.width != image.width || decoded.height != image.height {
        return Err(ImageInsertError::Corrupt);
    }
    let (ow, oh) = oriented_size(decoded.width, decoded.height, image.orientation);
    let longest = ow.max(oh);
    let (tw, th) = if longest > MAX_BITMAP_SIDE {
        let scale = f64::from(MAX_BITMAP_SIDE) / f64::from(longest);
        (
            ((f64::from(ow) * scale).round() as u32).max(1),
            ((f64::from(oh) * scale).round() as u32).max(1),
        )
    } else {
        (ow, oh)
    };
    let out_len = (tw as usize)
        .checked_mul(th as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageInsertError::TooLarge)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(out_len)
        .map_err(|_| ImageInsertError::TooLarge)?;
    let sw = decoded.width;
    let sh = decoded.height;
    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let (sx, sy) = oriented_source(sw, sh, image.orientation, x, y);
        let at = (sy as usize * sw as usize + sx as usize) * 4;
        [
            decoded.pixels[at],
            decoded.pixels[at + 1],
            decoded.pixels[at + 2],
            decoded.pixels[at + 3],
        ]
    };
    for ty in 0..th {
        let y0 = (u64::from(ty) * u64::from(oh) / u64::from(th)) as u32;
        let y1 = ((u64::from(ty + 1) * u64::from(oh) / u64::from(th)) as u32).max(y0 + 1);
        for tx in 0..tw {
            let x0 = (u64::from(tx) * u64::from(ow) / u64::from(tw)) as u32;
            let x1 = ((u64::from(tx + 1) * u64::from(ow) / u64::from(tw)) as u32).max(x0 + 1);
            if x1 - x0 == 1 && y1 - y0 == 1 {
                rgba.extend_from_slice(&pixel(x0, y0));
                continue;
            }
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1.min(oh) {
                for x in x0..x1.min(ow) {
                    let p = pixel(x, y);
                    let alpha = u64::from(p[3]);
                    r += u64::from(p[0]) * alpha;
                    g += u64::from(p[1]) * alpha;
                    b += u64::from(p[2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            if a == 0 || n == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                rgba.extend_from_slice(&[
                    ((r + a / 2) / a) as u8,
                    ((g + a / 2) / a) as u8,
                    ((b + a / 2) / a) as u8,
                    ((a + n / 2) / n) as u8,
                ]);
            }
        }
    }
    Ok(Bitmap {
        width: tw,
        height: th,
        rgba,
    })
}
