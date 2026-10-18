// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures of the editor session (US-270): validation, storage, display bitmap.
//!
//! Original bytes are kept untouched. Insertion fully decodes the picture once
//! (so a corrupt file is refused before it enters the document) but keeps only
//! the original bytes, size and EXIF orientation. The display bitmap is decoded
//! again on demand, oriented, and reduced to at most [`MAX_BITMAP_SIDE`] pixels.

use atelier_image::{
    DecodeLimits, Image, ImageError, decode_bmp, decode_gif, decode_jpeg_with_metadata, decode_png,
};
use atelier_text_model::inline_image::{
    IMAGE_CHAR_BASE, ImageFormat, InlineImage, OBJECT_REPLACEMENT, SniffError, image_char,
    is_image_char, oriented_size, oriented_source,
};
use std::sync::Arc;

/// Largest accepted encoded picture.
pub(crate) const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Longest alternative text of a picture, in bytes.
pub(crate) const MAX_ALT: usize = atelier_text_model::inline_image::MAX_ALT_BYTES;
/// Total encoded bytes kept by one document.
pub(crate) const MAX_STORE_BYTES: usize = 32 * 1024 * 1024;
/// Distinct pictures per document.
pub(crate) const MAX_IMAGES: usize = 512;
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
    /// The document already holds the maximum number of pictures.
    StoreFull,
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

/// Pictures of one document; the text refers to them by private-use scalars.
#[derive(Clone, Debug)]
pub(crate) struct ImageStore {
    base: u32,
    items: Vec<InlineImage>,
    bytes: usize,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self {
            base: IMAGE_CHAR_BASE,
            items: Vec::new(),
            bytes: 0,
        }
    }
}

impl ImageStore {
    /// Drops every picture for a newly loaded `text`, moving the scalar range
    /// past any private-use scalar the text already contains.
    pub fn reset(&mut self, text: &str) {
        let highest = text
            .chars()
            .filter(|c| is_image_char(*c))
            .map(|c| c as u32)
            .max();
        self.base = highest.map_or(IMAGE_CHAR_BASE, |code| code + 1);
        self.items.clear();
        self.bytes = 0;
    }

    /// Adds `image` (or reuses an identical stored one) and returns its scalar.
    pub fn add(&mut self, image: InlineImage) -> Result<char, ImageInsertError> {
        if let Some(slot) = self.items.iter().position(|i| {
            i.format == image.format
                && i.orientation == image.orientation
                && i.alt == image.alt
                && i.bytes[..] == image.bytes[..]
        }) {
            return self.char_of(slot).ok_or(ImageInsertError::StoreFull);
        }
        let total = self
            .bytes
            .checked_add(image.bytes.len())
            .ok_or(ImageInsertError::StoreFull)?;
        if self.items.len() >= MAX_IMAGES || total > MAX_STORE_BYTES {
            return Err(ImageInsertError::StoreFull);
        }
        let slot = self.items.len();
        let ch = self.char_of(slot).ok_or(ImageInsertError::StoreFull)?;
        self.bytes = total;
        self.items.push(image);
        Ok(ch)
    }

    fn char_of(&self, slot: usize) -> Option<char> {
        image_char(self.base, u32::try_from(slot).ok()?)
    }

    /// The slot a scalar of the text stands for, when it is a stored picture.
    pub fn slot_of(&self, c: char) -> Option<usize> {
        let code = c as u32;
        let slot = usize::try_from(code.checked_sub(self.base)?).ok()?;
        (slot < self.items.len()).then_some(slot)
    }

    pub fn get(&self, slot: usize) -> Option<&InlineImage> {
        self.items.get(slot)
    }

    /// UTF-8 `bytes` of the document (optional BOM kept) with every picture
    /// scalar replaced by U+FFFC, for plain-text output and the dirty check.
    pub fn plain_bytes(&self, bytes: Vec<u8>) -> Vec<u8> {
        if self.items.is_empty() {
            return bytes;
        }
        let skip = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            3
        } else {
            0
        };
        match core::str::from_utf8(&bytes[skip..]) {
            Ok(text) if text.chars().any(|c| self.slot_of(c).is_some()) => {
                let mut out = bytes[..skip].to_vec();
                out.extend_from_slice(self.plain_text(text).as_bytes());
                out
            }
            _ => bytes,
        }
    }

    /// `text` with every picture scalar replaced by U+FFFC.
    pub fn plain_text(&self, text: &str) -> String {
        if self.items.is_empty() || !text.chars().any(|c| self.slot_of(c).is_some()) {
            return text.to_owned();
        }
        text.chars()
            .map(|c| {
                if self.slot_of(c).is_some() {
                    OBJECT_REPLACEMENT
                } else {
                    c
                }
            })
            .collect()
    }
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
