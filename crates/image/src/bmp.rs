// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{DecodeLimits, Image, ImageError};

const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;

pub fn decode_bmp(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError> {
    if data.len() > limits.max_input_bytes {
        return Err(ImageError::LimitExceeded);
    }
    let r = Reader::new(data);
    if r.u16(0)? != 0x4d42 {
        return Err(ImageError::InvalidData("BMP signature"));
    }
    let declared_size = r.u32(2)? as usize;
    let pixel_offset = r.u32(10)? as usize;
    if declared_size != 0 && (declared_size > data.len() || declared_size < 14) {
        return Err(ImageError::Truncated);
    }
    let file_end = if declared_size == 0 {
        data.len()
    } else {
        declared_size
    };
    let hs = r.u32(14)? as usize;
    if ![40, 52, 56, 108, 124].contains(&hs) {
        return Err(ImageError::Unsupported("BMP DIB header"));
    }
    r.range(14, hs)?;
    let wi = r.i32(18)?;
    let hi = r.i32(22)?;
    if wi <= 0 || hi == 0 || hi == i32::MIN {
        return Err(ImageError::InvalidData("BMP dimensions"));
    }
    let w = wi as u32;
    let h = hi.unsigned_abs();
    if r.u16(26)? != 1 {
        return Err(ImageError::InvalidData("BMP planes"));
    }
    let bpp = r.u16(28)?;
    if ![1, 4, 8, 16, 24, 32].contains(&bpp) {
        return Err(ImageError::Unsupported("BMP bit depth"));
    }
    let compression = r.u32(30)?;
    let top_down = hi < 0;
    match compression {
        BI_RGB => (),
        BI_BITFIELDS if bpp == 16 || bpp == 32 => (),
        BI_RLE8 if bpp == 8 && !top_down => (),
        BI_RLE4 if bpp == 4 && !top_down => (),
        BI_RLE8 | BI_RLE4 => return Err(ImageError::InvalidData("BMP RLE depth or orientation")),
        _ => return Err(ImageError::Unsupported("BMP compression")),
    }
    let out_len = pixel_len(w, h, limits)?;
    if pixel_offset < 14 + hs || pixel_offset > file_end {
        return Err(ImageError::InvalidData("BMP pixel offset"));
    }
    let colors_used = r.u32(46)? as usize;
    let mut palette_start = 14 + hs;
    let mut masks = None;
    if compression == BI_BITFIELDS {
        let rgb = if hs == 40 { palette_start } else { 14 + 40 };
        let red = r.u32(rgb)?;
        let green = r.u32(rgb + 4)?;
        let blue = r.u32(rgb + 8)?;
        let alpha = if hs >= 56 { r.u32(14 + 52)? } else { 0 };
        if hs == 40 {
            palette_start += 12;
        }
        masks = Some([red, green, blue, alpha]);
    }
    if let Some(ms) = masks {
        validate_masks(ms, bpp)?;
    }
    let pal_count = if bpp <= 8 {
        if colors_used == 0 {
            1usize << bpp
        } else {
            colors_used
        }
    } else {
        colors_used
    };
    if pal_count > (1usize << bpp.min(8)) && bpp <= 8 {
        return Err(ImageError::InvalidData("BMP palette size"));
    }
    let pal_bytes = pal_count.checked_mul(4).ok_or(ImageError::LimitExceeded)?;
    let pal_end = palette_start
        .checked_add(pal_bytes)
        .ok_or(ImageError::LimitExceeded)?;
    if pal_end > pixel_offset {
        return Err(ImageError::Truncated);
    }
    let mut palette = Vec::new();
    palette
        .try_reserve_exact(pal_count)
        .map_err(|_| ImageError::LimitExceeded)?;
    for i in 0..pal_count {
        let o = palette_start + i * 4;
        palette.push([r.byte(o + 2)?, r.byte(o + 1)?, r.byte(o)?, 255]);
    }
    let mut pixels = allocate(out_len)?;
    if compression == BI_RLE4 || compression == BI_RLE8 {
        if let Some(background) = palette.first() {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(background);
            }
        }
        let image_size = r.u32(34)? as usize;
        let end = if image_size == 0 {
            file_end
        } else {
            let end = pixel_offset
                .checked_add(image_size)
                .ok_or(ImageError::LimitExceeded)?;
            if end > file_end {
                return Err(ImageError::Truncated);
            }
            end
        };
        decode_rle(
            &data[pixel_offset..end],
            w,
            h,
            compression == BI_RLE4,
            &palette,
            &mut pixels,
        )?;
    } else {
        let row_bits = u64::from(w)
            .checked_mul(u64::from(bpp))
            .ok_or(ImageError::LimitExceeded)?;
        let stride =
            usize::try_from(row_bits.div_ceil(32) * 4).map_err(|_| ImageError::LimitExceeded)?;
        let bytes = stride
            .checked_mul(h as usize)
            .ok_or(ImageError::LimitExceeded)?;
        let end = pixel_offset
            .checked_add(bytes)
            .ok_or(ImageError::LimitExceeded)?;
        if end > file_end {
            return Err(ImageError::Truncated);
        }
        for sy in 0..h {
            let y = if top_down { sy } else { h - 1 - sy };
            let row = pixel_offset + sy as usize * stride;
            for x in 0..w {
                let index = x as usize;
                let rgba = match bpp {
                    1 | 4 | 8 => {
                        let ix = match bpp {
                            1 => (data[row + index / 8] >> (7 - index % 8)) & 1,
                            4 => {
                                let b = data[row + index / 2];
                                if index % 2 == 0 { b >> 4 } else { b & 15 }
                            }
                            _ => data[row + index],
                        } as usize;
                        *palette
                            .get(ix)
                            .ok_or(ImageError::InvalidData("BMP palette index"))?
                    }
                    24 => {
                        let o = row + index * 3;
                        [data[o + 2], data[o + 1], data[o], 255]
                    }
                    16 | 32 if masks.is_some() => {
                        let raw = if bpp == 16 {
                            u32::from(u16::from_le_bytes([
                                data[row + index * 2],
                                data[row + index * 2 + 1],
                            ]))
                        } else {
                            u32::from_le_bytes([
                                data[row + index * 4],
                                data[row + index * 4 + 1],
                                data[row + index * 4 + 2],
                                data[row + index * 4 + 3],
                            ])
                        };
                        let m = masks.unwrap();
                        [
                            component(raw, m[0]),
                            component(raw, m[1]),
                            component(raw, m[2]),
                            if m[3] == 0 { 255 } else { component(raw, m[3]) },
                        ]
                    }
                    16 => {
                        let v = u32::from(u16::from_le_bytes([
                            data[row + index * 2],
                            data[row + index * 2 + 1],
                        ]));
                        [
                            scale(v & 0x7c00, 0x7c00),
                            scale(v & 0x03e0, 0x03e0),
                            scale(v & 0x001f, 0x001f),
                            255,
                        ]
                    }
                    32 => {
                        let o = row + index * 4;
                        [data[o + 2], data[o + 1], data[o], 255]
                    }
                    _ => return Err(ImageError::Unsupported("BMP pixel layout")),
                };
                put(&mut pixels, w, x, y, rgba);
            }
        }
    }
    Ok(Image {
        width: w,
        height: h,
        pixels,
    })
}

fn decode_rle(
    src: &[u8],
    w: u32,
    h: u32,
    rle4: bool,
    pal: &[[u8; 4]],
    out: &mut [u8],
) -> Result<(), ImageError> {
    let mut p = 0usize;
    let (mut x, mut y) = (0u32, 0u32);
    let mut ended = false;
    while p < src.len() {
        if p + 2 > src.len() {
            return Err(ImageError::Truncated);
        }
        let n = src[p];
        let v = src[p + 1];
        p += 2;
        if n != 0 {
            let count = n as u32;
            if y >= h || x.checked_add(count).is_none_or(|e| e > w) {
                return Err(ImageError::InvalidData("BMP RLE run bounds"));
            }
            for i in 0..count {
                let ix = if rle4 {
                    if i % 2 == 0 { v >> 4 } else { v & 15 }
                } else {
                    v
                } as usize;
                let c = *pal
                    .get(ix)
                    .ok_or(ImageError::InvalidData("BMP RLE palette index"))?;
                put(out, w, x + i, h - 1 - y, c)
            }
            x += count;
            continue;
        }
        match v {
            0 => {
                x = 0;
                y += 1;
                if y > h {
                    return Err(ImageError::InvalidData("BMP RLE row"));
                }
            }
            1 => {
                ended = true;
                break;
            }
            2 => {
                if p + 2 > src.len() {
                    return Err(ImageError::Truncated);
                }
                let dx = src[p] as u32;
                let dy = src[p + 1] as u32;
                p += 2;
                x = x
                    .checked_add(dx)
                    .ok_or(ImageError::InvalidData("BMP RLE delta"))?;
                y = y
                    .checked_add(dy)
                    .ok_or(ImageError::InvalidData("BMP RLE delta"))?;
                if x > w || y >= h {
                    return Err(ImageError::InvalidData("BMP RLE delta bounds"));
                }
            }
            count => {
                let count = count as usize;
                if y >= h || x.checked_add(count as u32).is_none_or(|e| e > w) {
                    return Err(ImageError::InvalidData("BMP RLE absolute bounds"));
                }
                let bytes = if rle4 { count.div_ceil(2) } else { count };
                if p + bytes > src.len() {
                    return Err(ImageError::Truncated);
                }
                for i in 0..count {
                    let ix = if rle4 {
                        if i % 2 == 0 {
                            src[p + i / 2] >> 4
                        } else {
                            src[p + i / 2] & 15
                        }
                    } else {
                        src[p + i]
                    } as usize;
                    let c = *pal
                        .get(ix)
                        .ok_or(ImageError::InvalidData("BMP RLE palette index"))?;
                    put(out, w, x + i as u32, h - 1 - y, c)
                }
                p += bytes;
                if bytes % 2 == 1 {
                    if p >= src.len() {
                        return Err(ImageError::Truncated);
                    }
                    p += 1;
                }
                x += count as u32;
            }
        }
    }
    if !ended {
        return Err(ImageError::Truncated);
    }
    Ok(())
}
fn validate_masks(m: [u32; 4], bpp: u16) -> Result<(), ImageError> {
    let valid = if bpp == 32 {
        u32::MAX
    } else {
        (1u32 << bpp) - 1
    };
    if m[..3].contains(&0) || m.iter().any(|x| x & !valid != 0) {
        return Err(ImageError::InvalidData("BMP bitfield mask"));
    }
    for i in 0..4 {
        if m[i] != 0 && {
            let shifted = m[i] >> m[i].trailing_zeros();
            shifted & shifted.wrapping_add(1) != 0
        } {
            return Err(ImageError::InvalidData("BMP non-contiguous mask"));
        }
        for j in 0..i {
            if m[i] & m[j] != 0 {
                return Err(ImageError::InvalidData("BMP overlapping masks"));
            }
        }
    }
    Ok(())
}
fn component(v: u32, m: u32) -> u8 {
    if m == 0 {
        return 255;
    }
    let shift = m.trailing_zeros();
    let max = m >> shift;
    let n = (v & m) >> shift;
    ((u64::from(n) * 255 + u64::from(max) / 2) / u64::from(max)) as u8
}
fn scale(v: u32, m: u32) -> u8 {
    component(v, m)
}
fn put(dst: &mut [u8], w: u32, x: u32, y: u32, c: [u8; 4]) {
    let o = ((y as usize * w as usize) + x as usize) * 4;
    dst[o..o + 4].copy_from_slice(&c)
}
fn pixel_len(w: u32, h: u32, l: DecodeLimits) -> Result<usize, ImageError> {
    let n = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or(ImageError::LimitExceeded)?;
    if n == 0 || n > l.max_pixels {
        return Err(ImageError::LimitExceeded);
    }
    usize::try_from(n.checked_mul(4).ok_or(ImageError::LimitExceeded)?)
        .map_err(|_| ImageError::LimitExceeded)
}
fn allocate(n: usize) -> Result<Vec<u8>, ImageError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| ImageError::LimitExceeded)?;
    v.resize(n, 0);
    Ok(v)
}
struct Reader<'a> {
    d: &'a [u8],
}
impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d }
    }
    fn range(&self, o: usize, n: usize) -> Result<&'a [u8], ImageError> {
        let e = o.checked_add(n).ok_or(ImageError::LimitExceeded)?;
        self.d.get(o..e).ok_or(ImageError::Truncated)
    }
    fn byte(&self, o: usize) -> Result<u8, ImageError> {
        Ok(self.range(o, 1)?[0])
    }
    fn u16(&self, o: usize) -> Result<u16, ImageError> {
        Ok(u16::from_le_bytes(self.range(o, 2)?.try_into().unwrap()))
    }
    fn i32(&self, o: usize) -> Result<i32, ImageError> {
        Ok(i32::from_le_bytes(self.range(o, 4)?.try_into().unwrap()))
    }
    fn u32(&self, o: usize) -> Result<u32, ImageError> {
        Ok(u32::from_le_bytes(self.range(o, 4)?.try_into().unwrap()))
    }
}
