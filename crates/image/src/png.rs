// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict, bounded decoding of static PNG images into straight RGBA8 pixels.

use crate::{DecodeLimits, Image, ImageError};
use atelier_compress::{Format, InflateError, InflateState, Status};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_PNG_FIELD: u32 = 0x7fff_ffff;
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Decode one complete PNG into top-down, straight-alpha RGBA8 pixels.
/// Color profiles and gamma metadata are deliberately not applied.
pub fn decode_png(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError> {
    if data.len() > limits.max_input_bytes {
        return Err(ImageError::LimitExceeded);
    }
    if data.len() < SIGNATURE.len() {
        return Err(ImageError::Truncated);
    }
    if &data[..8] != SIGNATURE {
        return Err(ImageError::InvalidData("bad PNG signature"));
    }

    let mut pos = 8usize;
    let mut ihdr = None;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut transparency: Option<Vec<u8>> = None;
    let mut transparent_gray = None;
    let mut transparent_rgb = None;
    let mut idat = Vec::new();
    let mut seen_idat = false;
    let mut idat_closed = false;
    let mut seen_iend = false;
    let mut seen_trns = false;

    while pos < data.len() {
        let header_end = pos
            .checked_add(8)
            .ok_or(ImageError::InvalidData("chunk offset overflow"))?;
        if header_end > data.len() {
            return Err(ImageError::Truncated);
        }
        let chunk_len_u32 = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
        if chunk_len_u32 > MAX_PNG_FIELD {
            return Err(ImageError::InvalidData("PNG chunk length exceeds 2^31-1"));
        }
        let chunk_len = chunk_len_u32 as usize;
        let kind: [u8; 4] = data[pos + 4..header_end].try_into().unwrap();
        pos = header_end;

        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err(ImageError::InvalidData("invalid chunk type"));
        }
        // The third type byte is reserved and must be uppercase.
        if kind[2].is_ascii_lowercase() {
            return Err(ImageError::InvalidData("reserved chunk bit is set"));
        }
        let end = pos
            .checked_add(chunk_len)
            .ok_or(ImageError::InvalidData("chunk length overflow"))?;
        let crc_end = end
            .checked_add(4)
            .ok_or(ImageError::InvalidData("chunk length overflow"))?;
        if crc_end > data.len() {
            return Err(ImageError::Truncated);
        }
        let payload = &data[pos..end];
        let expected_crc = u32::from_be_bytes(data[end..crc_end].try_into().unwrap());
        if chunk_crc(&kind, payload) != expected_crc {
            return Err(ImageError::InvalidData("chunk CRC mismatch"));
        }
        pos = crc_end;

        if ihdr.is_none() && kind != *b"IHDR" {
            return Err(ImageError::InvalidData("IHDR must be the first chunk"));
        }
        if seen_idat && kind != *b"IDAT" {
            idat_closed = true;
        }

        match &kind {
            b"IHDR" => {
                if ihdr.is_some() || payload.len() != 13 {
                    return Err(ImageError::InvalidData("invalid IHDR"));
                }
                let width = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                if width > MAX_PNG_FIELD || height > MAX_PNG_FIELD {
                    return Err(ImageError::InvalidData("PNG dimensions exceed 2^31-1"));
                }
                let depth = payload[8];
                let color = payload[9];
                let channels = match color {
                    0 => 1,
                    2 => 3,
                    3 => 1,
                    4 => 2,
                    6 => 4,
                    _ => return Err(ImageError::Unsupported("PNG color type")),
                };
                let valid_depth = match color {
                    0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(depth, 8 | 16),
                    3 => matches!(depth, 1 | 2 | 4 | 8),
                    _ => false,
                };
                if !valid_depth {
                    return Err(ImageError::InvalidData(
                        "illegal PNG bit depth for color type",
                    ));
                }
                if width == 0 || height == 0 {
                    return Err(ImageError::InvalidData("zero PNG dimension"));
                }
                if payload[10] != 0 || payload[11] != 0 {
                    return Err(ImageError::Unsupported("PNG compression or filter method"));
                }
                if payload[12] > 1 {
                    return Err(ImageError::Unsupported("PNG interlace method"));
                }
                let pixels = u64::from(width)
                    .checked_mul(u64::from(height))
                    .ok_or(ImageError::LimitExceeded)?;
                if pixels > limits.max_pixels {
                    return Err(ImageError::LimitExceeded);
                }
                let out_len = usize::try_from(pixels)
                    .ok()
                    .and_then(|n| n.checked_mul(4))
                    .ok_or(ImageError::LimitExceeded)?;
                let _ = out_len;
                ihdr = Some(Header {
                    width,
                    height,
                    depth,
                    color,
                    channels,
                    interlace: payload[12],
                });
            }
            b"PLTE" => {
                let h = ihdr
                    .as_ref()
                    .ok_or(ImageError::InvalidData("PLTE before IHDR"))?;
                if seen_idat || seen_trns || palette.is_some() || matches!(h.color, 0 | 4) {
                    return Err(ImageError::InvalidData(
                        "PLTE is duplicated, late, or forbidden",
                    ));
                }
                if payload.is_empty() || payload.len() > 768 || payload.len() % 3 != 0 {
                    return Err(ImageError::InvalidData("invalid PLTE length"));
                }
                let entries = payload.len() / 3;
                if h.color == 3 && entries > (1usize << h.depth) {
                    return Err(ImageError::InvalidData("palette exceeds indexed bit depth"));
                }
                palette = Some(
                    payload
                        .chunks_exact(3)
                        .map(|p| [p[0], p[1], p[2]])
                        .collect(),
                );
            }
            b"tRNS" => {
                let h = ihdr
                    .as_ref()
                    .ok_or(ImageError::InvalidData("tRNS before IHDR"))?;
                if seen_idat || seen_trns || matches!(h.color, 4 | 6) {
                    return Err(ImageError::InvalidData(
                        "tRNS is duplicated, late, or forbidden",
                    ));
                }
                seen_trns = true;
                match h.color {
                    0 => {
                        if payload.len() != 2 {
                            return Err(ImageError::InvalidData("invalid grayscale tRNS length"));
                        }
                        let value = u16::from_be_bytes(payload.try_into().unwrap());
                        if u32::from(value) > max_sample(h.depth) {
                            return Err(ImageError::InvalidData(
                                "grayscale tRNS sample exceeds bit depth",
                            ));
                        }
                        transparent_gray = Some(value);
                    }
                    2 => {
                        if payload.len() != 6 {
                            return Err(ImageError::InvalidData("invalid truecolor tRNS length"));
                        }
                        let rgb = [
                            u16::from_be_bytes(payload[0..2].try_into().unwrap()),
                            u16::from_be_bytes(payload[2..4].try_into().unwrap()),
                            u16::from_be_bytes(payload[4..6].try_into().unwrap()),
                        ];
                        if rgb.iter().any(|&v| u32::from(v) > max_sample(h.depth)) {
                            return Err(ImageError::InvalidData(
                                "truecolor tRNS sample exceeds bit depth",
                            ));
                        }
                        transparent_rgb = Some(rgb);
                    }
                    3 => {
                        let p = palette
                            .as_ref()
                            .ok_or(ImageError::InvalidData("indexed tRNS requires PLTE"))?;
                        if payload.is_empty() || payload.len() > p.len() {
                            return Err(ImageError::InvalidData("invalid palette tRNS length"));
                        }
                        transparency = Some(payload.to_vec());
                    }
                    _ => unreachable!(),
                }
            }
            b"IDAT" => {
                let h = ihdr
                    .as_ref()
                    .ok_or(ImageError::InvalidData("IDAT before IHDR"))?;
                if idat_closed || (h.color == 3 && palette.is_none()) {
                    return Err(ImageError::InvalidData(
                        "IDAT order or indexed palette is invalid",
                    ));
                }
                seen_idat = true;
                let new_len = idat
                    .len()
                    .checked_add(payload.len())
                    .ok_or(ImageError::LimitExceeded)?;
                if new_len > limits.max_input_bytes {
                    return Err(ImageError::LimitExceeded);
                }
                idat.try_reserve(payload.len())
                    .map_err(|_| ImageError::LimitExceeded)?;
                idat.extend_from_slice(payload);
            }
            b"IEND" => {
                if !payload.is_empty() || !seen_idat {
                    return Err(ImageError::InvalidData("invalid IEND"));
                }
                seen_iend = true;
                if pos != data.len() {
                    return Err(ImageError::InvalidData("data follows IEND"));
                }
                break;
            }
            _ => {
                if kind[0].is_ascii_uppercase() {
                    return Err(ImageError::Unsupported("unknown critical PNG chunk"));
                }
            }
        }
    }

    if !seen_iend {
        return Err(ImageError::Truncated);
    }
    let h = ihdr.ok_or(ImageError::Truncated)?;
    if h.color == 3 && palette.is_none() {
        return Err(ImageError::InvalidData("indexed PNG is missing PLTE"));
    }
    let passes: Vec<(usize, usize, usize, usize)> = if h.interlace == 0 {
        vec![(0, 0, 1, 1)]
    } else {
        ADAM7.to_vec()
    };
    let mut layouts = Vec::with_capacity(passes.len());
    let mut raw_len = 0usize;
    let bits_per_pixel = usize::from(h.channels)
        .checked_mul(usize::from(h.depth))
        .ok_or(ImageError::LimitExceeded)?;
    for (x0, y0, dx, dy) in passes {
        let pw = pass_extent(h.width as usize, x0, dx);
        let ph = pass_extent(h.height as usize, y0, dy);
        if pw == 0 || ph == 0 {
            layouts.push(Pass {
                x0,
                y0,
                dx,
                dy,
                width: pw,
                height: ph,
                row_bytes: 0,
                bpp: 1,
            });
            continue;
        }
        let row_bits = pw
            .checked_mul(bits_per_pixel)
            .ok_or(ImageError::LimitExceeded)?;
        let row_bytes = row_bits.checked_add(7).ok_or(ImageError::LimitExceeded)? / 8;
        let bpp = bits_per_pixel
            .checked_add(7)
            .ok_or(ImageError::LimitExceeded)?
            / 8;
        let bpp = bpp.max(1);
        let pass_bytes = row_bytes
            .checked_add(1)
            .and_then(|n| n.checked_mul(ph))
            .ok_or(ImageError::LimitExceeded)?;
        raw_len = raw_len
            .checked_add(pass_bytes)
            .ok_or(ImageError::LimitExceeded)?;
        layouts.push(Pass {
            x0,
            y0,
            dx,
            dy,
            width: pw,
            height: ph,
            row_bytes,
            bpp,
        });
    }

    let mut raw = zeroed(raw_len)?;
    inflate_exact(&idat, &mut raw)?;
    let output_len = usize::try_from(u64::from(h.width) * u64::from(h.height))
        .ok()
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageError::LimitExceeded)?;
    let mut pixels = zeroed(output_len)?;
    let transparency = Transparency {
        palette: palette.as_deref(),
        indexed: transparency.as_deref(),
        gray: transparent_gray,
        rgb: transparent_rgb,
    };
    let mut cursor = 0usize;
    for pass in &layouts {
        if pass.width == 0 || pass.height == 0 {
            continue;
        }
        for row in 0..pass.height {
            let start = cursor;
            let end = start
                .checked_add(1 + pass.row_bytes)
                .ok_or(ImageError::LimitExceeded)?;
            unfilter_row(&mut raw, start, end, row, pass.row_bytes, pass.bpp)?;
            cursor = end;
        }
        let pass_len = pass
            .height
            .checked_mul(pass.row_bytes + 1)
            .ok_or(ImageError::LimitExceeded)?;
        let start = cursor
            .checked_sub(pass_len)
            .ok_or(ImageError::InvalidData("invalid pass layout"))?;
        scatter_pass(&raw[start..cursor], *pass, h, transparency, &mut pixels)?;
    }
    if cursor != raw.len() {
        return Err(ImageError::InvalidData(
            "internal PNG scanline size mismatch",
        ));
    }
    Ok(Image {
        width: h.width,
        height: h.height,
        pixels,
    })
}

#[derive(Clone, Copy)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    channels: u8,
    interlace: u8,
}
#[derive(Clone, Copy)]
struct Pass {
    x0: usize,
    y0: usize,
    dx: usize,
    dy: usize,
    width: usize,
    height: usize,
    row_bytes: usize,
    bpp: usize,
}

#[derive(Clone, Copy)]
struct Transparency<'a> {
    palette: Option<&'a [[u8; 3]]>,
    indexed: Option<&'a [u8]>,
    gray: Option<u16>,
    rgb: Option<[u16; 3]>,
}

fn zeroed(len: usize) -> Result<Vec<u8>, ImageError> {
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| ImageError::LimitExceeded)?;
    out.resize(len, 0);
    Ok(out)
}

fn pass_extent(total: usize, origin: usize, step: usize) -> usize {
    if total <= origin {
        0
    } else {
        1 + (total - 1 - origin) / step
    }
}

fn chunk_crc(kind: &[u8; 4], payload: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in kind.iter().chain(payload) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn inflate_exact(input: &[u8], output: &mut [u8]) -> Result<(), ImageError> {
    let mut decoder = InflateState::new(Format::Zlib, output.len() as u64);
    let first = decoder.push(input, output, true).map_err(|e| match e {
        InflateError::UnexpectedEof => ImageError::Truncated,
        InflateError::OutputLimit => ImageError::InvalidData("zlib output exceeds PNG scanlines"),
        InflateError::InvalidData(_) => ImageError::InvalidData("invalid PNG zlib stream"),
    })?;
    if first.produced != output.len() {
        return Err(ImageError::InvalidData(
            "PNG zlib stream has wrong output length",
        ));
    }
    let (status, consumed) = if first.status == Status::NeedOutput {
        // InflateState can fill the caller's exact-size buffer before it has
        // consumed the DEFLATE end marker and zlib checksum. Drain that tail
        // without permitting another output byte.
        let tail = decoder
            .push(&input[first.consumed..], &mut [], true)
            .map_err(|e| match e {
                InflateError::UnexpectedEof => ImageError::Truncated,
                InflateError::OutputLimit => {
                    ImageError::InvalidData("zlib output exceeds PNG scanlines")
                }
                InflateError::InvalidData(_) => ImageError::InvalidData("invalid PNG zlib stream"),
            })?;
        (tail.status, first.consumed + tail.consumed)
    } else {
        (first.status, first.consumed)
    };
    if status != Status::Done || consumed != input.len() {
        return Err(ImageError::InvalidData(
            "PNG zlib stream has wrong length or trailing data",
        ));
    }
    Ok(())
}

fn unfilter_row(
    raw: &mut [u8],
    start: usize,
    end: usize,
    row: usize,
    row_bytes: usize,
    bpp: usize,
) -> Result<(), ImageError> {
    let filter = *raw.get(start).ok_or(ImageError::Truncated)?;
    if end > raw.len() || end - start != row_bytes + 1 {
        return Err(ImageError::Truncated);
    }
    for i in 0..row_bytes {
        let at = start + 1 + i;
        let x = raw[at];
        let a = if i >= bpp { raw[at - bpp] } else { 0 };
        let b = if row > 0 { raw[at - row_bytes - 1] } else { 0 };
        let c = if row > 0 && i >= bpp {
            raw[at - row_bytes - 1 - bpp]
        } else {
            0
        };
        raw[at] = match filter {
            0 => x,
            1 => x.wrapping_add(a),
            2 => x.wrapping_add(b),
            3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
            4 => x.wrapping_add(paeth(a, b, c)),
            _ => return Err(ImageError::InvalidData("invalid PNG filter type")),
        };
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

fn scatter_pass(
    bytes: &[u8],
    pass: Pass,
    h: Header,
    transparency: Transparency<'_>,
    out: &mut [u8],
) -> Result<(), ImageError> {
    let mut cursor = 0usize;
    for py in 0..pass.height {
        let row = bytes
            .get(cursor + 1..cursor + 1 + pass.row_bytes)
            .ok_or(ImageError::Truncated)?;
        cursor = cursor
            .checked_add(1 + pass.row_bytes)
            .ok_or(ImageError::LimitExceeded)?;
        for px in 0..pass.width {
            let sample =
                |channel: usize| read_sample(row, px * usize::from(h.channels) + channel, h.depth);
            let (r, g, b, a) = match h.color {
                0 => {
                    let gray = sample(0)?;
                    let v = scale(gray, h.depth);
                    (
                        v,
                        v,
                        v,
                        if transparency.gray == Some(gray as u16) {
                            0
                        } else {
                            255
                        },
                    )
                }
                2 => {
                    let rv = sample(0)?;
                    let gv = sample(1)?;
                    let bv = sample(2)?;
                    let transparent = transparency.rgb == Some([rv as u16, gv as u16, bv as u16]);
                    (
                        scale(rv, h.depth),
                        scale(gv, h.depth),
                        scale(bv, h.depth),
                        if transparent { 0 } else { 255 },
                    )
                }
                3 => {
                    let index = sample(0)? as usize;
                    let color = transparency
                        .palette
                        .and_then(|p| p.get(index))
                        .ok_or(ImageError::InvalidData("palette index out of range"))?;
                    (
                        color[0],
                        color[1],
                        color[2],
                        transparency
                            .indexed
                            .and_then(|t| t.get(index))
                            .copied()
                            .unwrap_or(255),
                    )
                }
                4 => {
                    let gray = sample(0)?;
                    let alpha = sample(1)?;
                    let v = scale(gray, h.depth);
                    (v, v, v, scale(alpha, h.depth))
                }
                6 => (
                    scale(sample(0)?, h.depth),
                    scale(sample(1)?, h.depth),
                    scale(sample(2)?, h.depth),
                    scale(sample(3)?, h.depth),
                ),
                _ => return Err(ImageError::InvalidData("invalid PNG color type")),
            };
            let x = pass
                .x0
                .checked_add(px.checked_mul(pass.dx).ok_or(ImageError::LimitExceeded)?)
                .ok_or(ImageError::LimitExceeded)?;
            let y = pass
                .y0
                .checked_add(py.checked_mul(pass.dy).ok_or(ImageError::LimitExceeded)?)
                .ok_or(ImageError::LimitExceeded)?;
            let at = y
                .checked_mul(h.width as usize)
                .and_then(|n| n.checked_add(x))
                .and_then(|n| n.checked_mul(4))
                .ok_or(ImageError::LimitExceeded)?;
            let dest = out
                .get_mut(at..at + 4)
                .ok_or(ImageError::InvalidData("Adam7 coordinate out of bounds"))?;
            dest.copy_from_slice(&[r, g, b, a]);
        }
    }
    Ok(())
}

fn read_sample(row: &[u8], sample_index: usize, depth: u8) -> Result<u32, ImageError> {
    match depth {
        1 | 2 | 4 => {
            let bit = sample_index
                .checked_mul(usize::from(depth))
                .ok_or(ImageError::LimitExceeded)?;
            let byte = *row.get(bit / 8).ok_or(ImageError::Truncated)?;
            let shift = 8 - usize::from(depth) - (bit % 8);
            Ok(u32::from((byte >> shift) & ((1 << depth) - 1)))
        }
        8 => Ok(u32::from(
            *row.get(sample_index).ok_or(ImageError::Truncated)?,
        )),
        16 => {
            let at = sample_index
                .checked_mul(2)
                .ok_or(ImageError::LimitExceeded)?;
            Ok(u32::from(u16::from_be_bytes(
                row.get(at..at + 2)
                    .ok_or(ImageError::Truncated)?
                    .try_into()
                    .unwrap(),
            )))
        }
        _ => Err(ImageError::InvalidData("invalid PNG sample depth")),
    }
}

fn max_sample(depth: u8) -> u32 {
    (1u32 << depth) - 1
}
fn scale(sample: u32, depth: u8) -> u8 {
    ((sample * 255 + max_sample(depth) / 2) / max_sample(depth)) as u8
}
