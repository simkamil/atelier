// SPDX-License-Identifier: AGPL-3.0-or-later
//! Image XObjects: JPEG passed through (`DCTDecode`), everything else decoded
//! by `atelier-image` and written as Flate pixels with an optional `SMask`.

use crate::PdfError;
use crate::out::Out;
use atelier_image::{
    DecodeLimits, Image, ImageError, decode_bmp, decode_gif, decode_jpeg, decode_png,
};
use std::io::Write;

/// Size limits applied before any image allocation.
#[derive(Clone, Copy)]
pub(crate) struct ImageLimits {
    pub max_bytes: usize,
    pub max_pixels: u64,
}

struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
}

/// Structural scan of a JPEG stream. `Ok(None)`: valid but not passable as is.
fn jpeg_info(b: &[u8]) -> Result<Option<JpegInfo>, ImageError> {
    if b.len() < 4 || b[0] != 0xff || b[1] != 0xd8 {
        return Err(ImageError::InvalidData("missing JPEG start marker"));
    }
    let mut at = 2usize;
    let mut sof: Option<JpegInfo> = None;
    let mut passable = true;
    loop {
        while b.get(at) == Some(&0xff) && b.get(at + 1) == Some(&0xff) {
            at += 1;
        }
        if b.get(at) != Some(&0xff) {
            return Err(ImageError::InvalidData("missing JPEG marker"));
        }
        let m = *b.get(at + 1).ok_or(ImageError::Truncated)?;
        at += 2;
        match m {
            0x01 | 0xd0..=0xd7 => continue,
            0xd8 => return Err(ImageError::InvalidData("nested JPEG start marker")),
            0xd9 => return Err(ImageError::InvalidData("JPEG without scan")),
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([
            *b.get(at).ok_or(ImageError::Truncated)?,
            *b.get(at + 1).ok_or(ImageError::Truncated)?,
        ]));
        if len < 2 || at + len > b.len() {
            return Err(ImageError::Truncated);
        }
        let seg = &b[at + 2..at + len];
        match m {
            0xc0..=0xc2 => {
                if seg.len() < 6 {
                    return Err(ImageError::Truncated);
                }
                let height = u32::from(u16::from_be_bytes([seg[1], seg[2]]));
                let width = u32::from(u16::from_be_bytes([seg[3], seg[4]]));
                let comps = seg[5];
                if width == 0 || height == 0 || seg.len() < 6 + 3 * usize::from(comps) {
                    return Err(ImageError::InvalidData("bad JPEG frame header"));
                }
                if seg[0] != 8 || !(comps == 1 || comps == 3) {
                    passable = false;
                }
                sof = Some(JpegInfo {
                    width,
                    height,
                    components: comps,
                });
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => passable = false,
            0xda => {
                let info = sof.ok_or(ImageError::InvalidData("JPEG scan before frame"))?;
                let tail = &b[b.len().saturating_sub(1024)..];
                if !tail.windows(2).any(|w| w == [0xff, 0xd9]) {
                    return Err(ImageError::Truncated);
                }
                return Ok(passable.then_some(info));
            }
            _ => {}
        }
        at += len;
    }
}

fn map(e: ImageError) -> PdfError {
    PdfError::Image(e)
}

enum Kind {
    Jpeg,
    Png,
    Gif,
    Bmp,
}

fn sniff(mime: &str, b: &[u8]) -> Result<Kind, PdfError> {
    let (kind, ok) = if b.starts_with(b"\xff\xd8\xff") {
        (Kind::Jpeg, mime == "image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        (Kind::Png, mime == "image/png")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        (Kind::Gif, mime == "image/gif")
    } else if b.starts_with(b"BM") {
        (Kind::Bmp, mime == "image/bmp")
    } else {
        return Err(map(ImageError::InvalidData("unknown image signature")));
    };
    if ok {
        Ok(kind)
    } else {
        Err(map(ImageError::InvalidData(
            "MIME type does not match the image bytes",
        )))
    }
}

/// Writes the image and returns its object number.
pub(crate) fn write_image<W: Write>(
    out: &mut Out<W>,
    mime: &str,
    bytes: &[u8],
    limits: ImageLimits,
    compress: bool,
) -> Result<u32, PdfError> {
    if bytes.len() > limits.max_bytes {
        return Err(PdfError::LimitExceeded("image bytes"));
    }
    let kind = sniff(mime, bytes)?;
    let decode_limits = DecodeLimits {
        max_pixels: limits.max_pixels,
        max_input_bytes: limits.max_bytes,
    };
    if matches!(kind, Kind::Jpeg) {
        if let Some(info) = jpeg_info(bytes).map_err(map)? {
            if u64::from(info.width) * u64::from(info.height) > limits.max_pixels {
                return Err(PdfError::LimitExceeded("image pixels"));
            }
            let cs = if info.components == 1 {
                "DeviceGray"
            } else {
                "DeviceRGB"
            };
            let id = out.alloc()?;
            let dict = format!(
                "/Type/XObject/Subtype/Image/Width {}/Height {}/ColorSpace/{cs}\
                 /BitsPerComponent 8/Interpolate true/Filter/DCTDecode",
                info.width, info.height
            );
            write_raw_stream(out, id, dict.as_bytes(), bytes)?;
            return Ok(id);
        }
    }
    let image = match kind {
        Kind::Jpeg => decode_jpeg(bytes, decode_limits),
        Kind::Png => decode_png(bytes, decode_limits),
        Kind::Gif => decode_gif(bytes, decode_limits),
        Kind::Bmp => decode_bmp(bytes, decode_limits),
    }
    .map_err(map)?;
    write_pixels(out, &image, limits, compress)
}

/// Stream whose data is already encoded by a filter named in `dict`.
fn write_raw_stream<W: Write>(
    out: &mut Out<W>,
    id: u32,
    dict: &[u8],
    data: &[u8],
) -> Result<(), PdfError> {
    // `Out::stream` appends `/Length`; the filter is already part of `dict`.
    out.stream(id, dict, data, false)
}

fn write_pixels<W: Write>(
    out: &mut Out<W>,
    image: &Image,
    limits: ImageLimits,
    compress: bool,
) -> Result<u32, PdfError> {
    let (w, h) = (image.width, image.height);
    let count = u64::from(w) * u64::from(h);
    if w == 0 || h == 0 || count > limits.max_pixels {
        return Err(PdfError::LimitExceeded("image pixels"));
    }
    let count = usize::try_from(count).map_err(|_| PdfError::LimitExceeded("image pixels"))?;
    if image.pixels.len()
        != count
            .checked_mul(4)
            .ok_or(PdfError::LimitExceeded("image"))?
    {
        return Err(map(ImageError::InvalidData("pixel buffer size")));
    }
    let gray = image
        .pixels
        .chunks_exact(4)
        .all(|p| p[0] == p[1] && p[1] == p[2]);
    let opaque = image.pixels.chunks_exact(4).all(|p| p[3] == 255);
    let mut color = Vec::with_capacity(count * if gray { 1 } else { 3 });
    for p in image.pixels.chunks_exact(4) {
        if gray {
            color.push(p[0]);
        } else {
            color.extend_from_slice(&p[..3]);
        }
    }
    let mask_id = if opaque {
        None
    } else {
        let alpha: Vec<u8> = image.pixels.chunks_exact(4).map(|p| p[3]).collect();
        let id = out.alloc()?;
        let dict = format!(
            "/Type/XObject/Subtype/Image/Width {w}/Height {h}/ColorSpace/DeviceGray\
             /BitsPerComponent 8/Interpolate true"
        );
        out.stream(id, dict.as_bytes(), &alpha, compress)?;
        Some(id)
    };
    let id = out.alloc()?;
    let cs = if gray { "DeviceGray" } else { "DeviceRGB" };
    let mut dict = format!(
        "/Type/XObject/Subtype/Image/Width {w}/Height {h}/ColorSpace/{cs}\
         /BitsPerComponent 8/Interpolate true"
    );
    if let Some(m) = mask_id {
        dict.push_str(&format!("/SMask {m} 0 R"));
    }
    out.stream(id, dict.as_bytes(), &color, compress)?;
    Ok(id)
}
