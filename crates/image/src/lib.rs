// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded PNG, GIF and BMP decoding into straight RGBA8 pixels.

#![forbid(unsafe_code)]

mod bmp;
mod gif;
mod png;
mod png_encode;

pub use bmp::decode_bmp;
pub use gif::decode_gif;
pub use png::decode_png;
pub use png_encode::{PngColor, encode_png, encode_png_rgba};

/// Top-down RGBA8 pixels with straight alpha and no color-profile conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Input and decoded pixel budgets, checked before allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    pub max_pixels: u64,
    pub max_input_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_pixels: 16_777_216,
            max_input_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageError {
    Truncated,
    InvalidData(&'static str),
    Unsupported(&'static str),
    LimitExceeded,
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated image"),
            Self::InvalidData(reason) => write!(f, "invalid image: {reason}"),
            Self::Unsupported(reason) => write!(f, "unsupported image: {reason}"),
            Self::LimitExceeded => f.write_str("image decode limit exceeded"),
        }
    }
}
impl std::error::Error for ImageError {}
