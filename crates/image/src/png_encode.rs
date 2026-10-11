// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic PNG encoding of 8-bit gray and RGBA pixels with adaptive filters.

use crate::{DecodeLimits, Image, ImageError};
use atelier_compress::{CompressError, Level, compress_zlib, crc32};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_PNG_FIELD: u32 = 0x7fff_ffff;
const IDAT_CHUNK: usize = 1 << 16;

/// Pixel layout of the caller's buffer; both are 8 bits per sample, top-down.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PngColor {
    /// One byte per pixel, written as PNG color type 0.
    Gray8,
    /// Four bytes per pixel with straight alpha, written as PNG color type 6.
    Rgba8,
}

impl PngColor {
    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::Rgba8 => 4,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Gray8 => 0,
            Self::Rgba8 => 6,
        }
    }
}

/// Encode an RGBA8 image. Equivalent to `encode_png` with `PngColor::Rgba8`.
pub fn encode_png_rgba(image: &Image, limits: DecodeLimits) -> Result<Vec<u8>, ImageError> {
    encode_png(
        image.width,
        image.height,
        PngColor::Rgba8,
        &image.pixels,
        limits,
    )
}

/// Encode top-down pixels as a non-interlaced PNG. The size, pixel and output
/// budgets of `DecodeLimits` are enforced so the result is decodable with the
/// same limits. The same input always yields the same bytes.
pub fn encode_png(
    width: u32,
    height: u32,
    color: PngColor,
    pixels: &[u8],
    limits: DecodeLimits,
) -> Result<Vec<u8>, ImageError> {
    encode_with(width, height, color, pixels, limits, None)
}

fn encode_with(
    width: u32,
    height: u32,
    color: PngColor,
    pixels: &[u8],
    limits: DecodeLimits,
    forced: Option<u8>,
) -> Result<Vec<u8>, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::InvalidData("zero PNG dimension"));
    }
    if width > MAX_PNG_FIELD || height > MAX_PNG_FIELD {
        return Err(ImageError::InvalidData("PNG dimensions exceed 2^31-1"));
    }
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(ImageError::LimitExceeded)?;
    if count > limits.max_pixels {
        return Err(ImageError::LimitExceeded);
    }
    let bpp = color.bytes_per_pixel();
    let stride = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(bpp))
        .ok_or(ImageError::LimitExceeded)?;
    let total = usize::try_from(height)
        .ok()
        .and_then(|h| h.checked_mul(stride))
        .ok_or(ImageError::LimitExceeded)?;
    if pixels.len() != total {
        return Err(ImageError::InvalidData("pixel buffer has wrong length"));
    }
    if total > limits.max_input_bytes {
        return Err(ImageError::LimitExceeded);
    }

    let raw = filter_image(pixels, stride, bpp, forced);
    let compressed = deflate(&raw)?;

    let mut out = Vec::with_capacity(compressed.len() + 64);
    out.extend_from_slice(SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, color.code(), 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    for part in compressed.chunks(IDAT_CHUNK) {
        write_chunk(&mut out, b"IDAT", part);
    }
    write_chunk(&mut out, b"IEND", &[]);
    if out.len() > limits.max_input_bytes {
        return Err(ImageError::LimitExceeded);
    }
    Ok(out)
}

fn deflate(raw: &[u8]) -> Result<Vec<u8>, ImageError> {
    let mut buffer = vec![0u8; raw.len() + raw.len() / 8 + 1024];
    loop {
        match compress_zlib(raw, Level::Normal, &mut buffer) {
            Ok(n) => {
                buffer.truncate(n);
                return Ok(buffer);
            }
            Err(CompressError::OutputTooSmall { needed }) if needed > buffer.len() => {
                buffer.resize(needed, 0);
            }
            Err(CompressError::OutputTooSmall { .. }) => {
                return Err(ImageError::InvalidData("zlib compression failed"));
            }
        }
    }
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Prefix each row with its filter type; `forced` disables the heuristic.
fn filter_image(pixels: &[u8], stride: usize, bpp: usize, forced: Option<u8>) -> Vec<u8> {
    let zero = vec![0u8; stride];
    let mut raw = Vec::with_capacity(pixels.len() + pixels.len() / stride);
    let mut candidate = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for (y, row) in pixels.chunks_exact(stride).enumerate() {
        let prior = if y == 0 {
            &zero[..]
        } else {
            &pixels[(y - 1) * stride..y * stride]
        };
        let kinds = match forced {
            Some(kind) => kind..=kind,
            None => 0..=4,
        };
        let mut best_kind = 0;
        let mut best_score = u64::MAX;
        for kind in kinds {
            filter_row(kind, row, prior, bpp, &mut candidate);
            // Minimum sum of absolute values, bytes read as signed.
            let score: u64 = candidate
                .iter()
                .map(|&b| u64::from((b as i8).unsigned_abs()))
                .sum();
            if score < best_score {
                best_score = score;
                best_kind = kind;
                std::mem::swap(&mut best, &mut candidate);
            }
        }
        raw.push(best_kind);
        raw.extend_from_slice(&best);
    }
    raw
}

fn filter_row(kind: u8, row: &[u8], prior: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..row.len() {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prior[i];
        let c = if i >= bpp { prior[i - bpp] } else { 0 };
        let predictor = match kind {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            _ => paeth(a, b, c),
        };
        out[i] = row[i].wrapping_sub(predictor);
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_png;

    fn sample(width: u32, height: u32, bpp: usize) -> Vec<u8> {
        (0..width as usize * height as usize * bpp)
            .map(|i| (i * 37 % 251) as u8 ^ (i / 7) as u8)
            .collect()
    }

    #[test]
    fn every_forced_filter_decodes_to_the_source_pixels() {
        for kind in 0..=4 {
            for (color, bpp) in [(PngColor::Rgba8, 4), (PngColor::Gray8, 1)] {
                let pixels = sample(7, 5, bpp);
                let png =
                    encode_with(7, 5, color, &pixels, DecodeLimits::default(), Some(kind)).unwrap();
                let image = decode_png(&png, DecodeLimits::default()).unwrap();
                let expected: Vec<u8> = if bpp == 4 {
                    pixels.clone()
                } else {
                    pixels.iter().flat_map(|&g| [g, g, g, 255]).collect()
                };
                assert_eq!(image.pixels, expected, "filter {kind} {color:?}");
            }
        }
    }

    #[test]
    fn adaptive_choice_has_the_minimum_score_and_reaches_every_type() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut seen = [false; 5];
        for _ in 0..200 {
            // Smooth random walks with a few jumps exercise all predictors.
            let (w, h, bpp) = (9usize, 6usize, 4usize);
            let mut pixels = vec![0u8; w * h * bpp];
            for i in 0..pixels.len() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let step = (state >> 40) as u8 % 7;
                let base = if i >= bpp { pixels[i - bpp] } else { 128 };
                let up = if i >= w * bpp { pixels[i - w * bpp] } else { 0 };
                pixels[i] = match (state >> 32) as u8 % 4 {
                    0 => base.wrapping_add(step),
                    1 => up.wrapping_add(step),
                    2 => ((u16::from(base) + u16::from(up)) / 2) as u8,
                    _ => (state >> 48) as u8,
                };
            }
            let raw = filter_image(&pixels, w * bpp, bpp, None);
            let zero = vec![0u8; w * bpp];
            for y in 0..h {
                let row = &pixels[y * w * bpp..(y + 1) * w * bpp];
                let prior = if y == 0 {
                    &zero[..]
                } else {
                    &pixels[(y - 1) * w * bpp..y * w * bpp]
                };
                let score = |kind: u8| {
                    let mut out = vec![0u8; row.len()];
                    filter_row(kind, row, prior, bpp, &mut out);
                    out.iter()
                        .map(|&b| u64::from((b as i8).unsigned_abs()))
                        .sum::<u64>()
                };
                let chosen = raw[y * (w * bpp + 1)];
                seen[usize::from(chosen)] = true;
                assert!((0..=4).all(|k| score(chosen) <= score(k)));
                assert!(
                    (0..chosen).all(|k| score(k) > score(chosen)),
                    "ties go to the lowest type"
                );
            }
        }
        assert_eq!(seen, [true; 5]);
    }

    #[test]
    fn paeth_picks_the_closest_neighbour_with_ordered_ties() {
        assert_eq!(paeth(10, 10, 10), 10);
        assert_eq!(paeth(0, 5, 5), 0);
        assert_eq!(paeth(5, 0, 5), 0);
        assert_eq!(paeth(100, 50, 20), 100);
    }
}
