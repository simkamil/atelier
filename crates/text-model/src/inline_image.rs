// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline picture model (US-270).
//!
//! A picture is stored with its original encoded bytes, never re-encoded. In
//! the plain-text editor it occupies exactly one private-use scalar of the
//! text (see [`image_char`]), so cursor movement, deletion and the existing
//! undo/redo history of [`crate::PlainTextDocument`] apply to it unchanged. In
//! the rich model it is an `AnchoredObject` of kind [`IMAGE_OBJECT_KIND`] whose
//! data is [`InlineImage::to_object_data`].

use std::sync::Arc;

/// First scalar of the supplementary private-use area B (U+100000..=U+10FFFD).
pub const IMAGE_CHAR_BASE: u32 = 0x10_0000;
/// Number of scalars reserved for pictures.
pub const IMAGE_CHAR_COUNT: u32 = 4096;
/// Last scalar of the private-use area B.
const LAST_PRIVATE: u32 = 0x10_fffd;
/// `AnchoredObject::kind` of a picture in the rich model.
pub const IMAGE_OBJECT_KIND: &str = "atelier:image";
/// The substitute written where a picture cannot be stored (plain text).
pub const OBJECT_REPLACEMENT: char = '\u{fffc}';

/// The scalar standing for picture slot `slot` when the text uses codes from `base`.
pub fn image_char(base: u32, slot: u32) -> Option<char> {
    if slot >= IMAGE_CHAR_COUNT || base < IMAGE_CHAR_BASE {
        return None;
    }
    let code = base.checked_add(slot)?;
    char::from_u32(code).filter(|_| code <= LAST_PRIVATE)
}

/// True for a scalar of the picture range.
pub fn is_image_char(c: char) -> bool {
    (IMAGE_CHAR_BASE..=LAST_PRIVATE).contains(&(c as u32))
}

/// Encoded picture formats the product stores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
}

/// Why bytes were not recognised as a supported picture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SniffError {
    /// A known but unsupported format (name for the message).
    Unsupported(&'static str),
    /// No known signature.
    Unknown,
}

impl ImageFormat {
    /// Recognises the format from its signature only (no decoding).
    pub fn sniff(bytes: &[u8]) -> Result<Self, SniffError> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Ok(Self::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Ok(Self::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Ok(Self::Gif)
        } else if bytes.starts_with(b"BM") {
            Ok(Self::Bmp)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Err(SniffError::Unsupported("WebP"))
        } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
            Err(SniffError::Unsupported("TIFF"))
        } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
            Err(SniffError::Unsupported("HEIF/AVIF"))
        } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
            Err(SniffError::Unsupported("SVG"))
        } else {
            Err(SniffError::Unknown)
        }
    }
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
        }
    }
    fn code(self) -> u8 {
        match self {
            Self::Png => 1,
            Self::Jpeg => 2,
            Self::Gif => 3,
            Self::Bmp => 4,
        }
    }
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Png,
            2 => Self::Jpeg,
            3 => Self::Gif,
            4 => Self::Bmp,
            _ => return None,
        })
    }
}

/// Failure building or decoding an [`InlineImage`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageModelError {
    EmptyDimensions,
    InvalidOrientation,
    InvalidObjectData,
}

/// A picture: original bytes, format, intrinsic size and EXIF orientation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineImage {
    pub format: ImageFormat,
    pub bytes: Arc<[u8]>,
    /// Stored pixel size, before orientation.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..=8 (1 = as stored).
    pub orientation: u8,
    /// Alternative text (at most [`MAX_ALT_BYTES`] bytes, no control characters).
    pub alt: String,
}

/// Longest alternative text kept with a picture.
pub const MAX_ALT_BYTES: usize = 512;

const MAGIC: &[u8; 4] = b"ATIM";

impl InlineImage {
    pub fn new(
        format: ImageFormat,
        bytes: Arc<[u8]>,
        width: u32,
        height: u32,
        orientation: u8,
    ) -> Result<Self, ImageModelError> {
        if width == 0 || height == 0 {
            return Err(ImageModelError::EmptyDimensions);
        }
        if !(1..=8).contains(&orientation) {
            return Err(ImageModelError::InvalidOrientation);
        }
        Ok(Self {
            format,
            bytes,
            width,
            height,
            orientation,
            alt: String::new(),
        })
    }

    /// The picture with alternative text `alt`: control characters become
    /// spaces and the text is cut at a character boundary to [`MAX_ALT_BYTES`].
    pub fn with_alt(mut self, alt: &str) -> Self {
        let mut clean = String::new();
        for c in alt.chars().map(|c| if c.is_control() { ' ' } else { c }) {
            if clean.len() + c.len_utf8() > MAX_ALT_BYTES {
                break;
            }
            clean.push(c);
        }
        self.alt = clean.trim().to_owned();
        self
    }

    /// Pixel size once the orientation is applied (width and height swap for 5..=8).
    pub fn oriented_size(&self) -> (u32, u32) {
        oriented_size(self.width, self.height, self.orientation)
    }

    /// Payload of an `AnchoredObject` of kind [`IMAGE_OBJECT_KIND`].
    pub fn to_object_data(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.alt.len() + self.bytes.len());
        out.extend_from_slice(MAGIC);
        out.push(self.format.code());
        out.push(self.orientation);
        out.extend_from_slice(&(self.alt.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(self.alt.as_bytes());
        out.extend_from_slice(&self.bytes);
        out
    }

    /// Inverse of [`Self::to_object_data`]; never panics on foreign data.
    pub fn from_object_data(data: &[u8]) -> Result<Self, ImageModelError> {
        if data.len() < 16 || &data[..4] != MAGIC {
            return Err(ImageModelError::InvalidObjectData);
        }
        let alt_len = usize::from(u16::from_le_bytes([data[6], data[7]]));
        let rest = &data[16..];
        if alt_len > MAX_ALT_BYTES || alt_len > rest.len() {
            return Err(ImageModelError::InvalidObjectData);
        }
        let alt = core::str::from_utf8(&rest[..alt_len])
            .map_err(|_| ImageModelError::InvalidObjectData)?;
        let format = ImageFormat::from_code(data[4]).ok_or(ImageModelError::InvalidObjectData)?;
        let width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        Ok(Self::new(format, Arc::from(&rest[alt_len..]), width, height, data[5])?.with_alt(alt))
    }
}

/// Size after applying an EXIF orientation (values outside 5..=8 keep it unchanged).
pub fn oriented_size(width: u32, height: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

/// Maps a pixel `(x, y)` of the oriented image to the stored pixel it shows.
/// `width`/`height` are the stored dimensions (non-zero); `(x, y)` must lie
/// inside the oriented size. Orientations follow EXIF: 2 mirror, 3 rotate 180,
/// 4 flip, 5 transpose, 6 rotate 90 clockwise, 7 transverse, 8 rotate 90
/// counter-clockwise.
pub fn oriented_source(width: u32, height: u32, orientation: u8, x: u32, y: u32) -> (u32, u32) {
    let (w, h) = (width.saturating_sub(1), height.saturating_sub(1));
    match orientation {
        2 => (w - x, y),
        3 => (w - x, h - y),
        4 => (x, h - y),
        5 => (y, x),
        6 => (y, h - x),
        7 => (w - y, h - x),
        8 => (w - y, x),
        _ => (x, y),
    }
}

/// Points per CSS pixel: pictures without a resolution are laid out at 96 dpi.
pub const POINTS_PER_PIXEL: f64 = 0.75;

/// Natural size in points of a `width` x `height` pixel picture once its EXIF
/// `orientation` is applied.
pub fn natural_size_points(width: u32, height: u32, orientation: u8) -> (f64, f64) {
    let (w, h) = oriented_size(width, height, orientation);
    (
        f64::from(w) * POINTS_PER_PIXEL,
        f64::from(h) * POINTS_PER_PIXEL,
    )
}

/// Shrinks `(width, height)` proportionally to fit `max_width` x `max_height`.
/// Returns the input unchanged when it already fits; never enlarges. Invalid
/// input (non-finite or non-positive) yields `None`.
pub fn fit_within(width: f64, height: f64, max_width: f64, max_height: f64) -> Option<(f64, f64)> {
    if ![width, height, max_width, max_height]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        return None;
    }
    let scale = (max_width / width).min(max_height / height).min(1.0);
    Some((width * scale, height * scale))
}
