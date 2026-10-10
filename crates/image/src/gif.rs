// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{DecodeLimits, Image, ImageError};

pub fn decode_gif(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError> {
    if data.len() > limits.max_input_bytes {
        return Err(ImageError::LimitExceeded);
    }
    let mut r = Reader { data, pos: 0 };
    let sig = r.take(6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return Err(ImageError::InvalidData("GIF signature"));
    }
    let sw = r.u16()? as u32;
    let sh = r.u16()? as u32;
    if sw == 0 || sh == 0 {
        return Err(ImageError::InvalidData("zero GIF screen size"));
    }
    let packed = r.byte()?;
    let background = r.byte()? as usize;
    let _aspect = r.byte()?;
    let global = if packed & 0x80 != 0 {
        Some(r.palette(1usize << ((packed & 7) + 1))?)
    } else {
        None
    };
    let screen_len = pixel_len(sw, sh, limits)?;
    let mut control: Option<(bool, u8)> = None;
    loop {
        match r.byte()? {
            0x3b => return Err(ImageError::InvalidData("GIF has no image")),
            0x21 => {
                let label = r.byte()?;
                if label == 0xf9 {
                    if r.byte()? != 4 {
                        return Err(ImageError::InvalidData("GIF graphic control size"));
                    }
                    let flags = r.byte()?;
                    let _delay = r.u16()?;
                    let index = r.byte()?;
                    if r.byte()? != 0 {
                        return Err(ImageError::InvalidData("GIF graphic control terminator"));
                    }
                    control = Some((flags & 1 != 0, index));
                } else if label == 0x01 {
                    if r.byte()? != 12 {
                        return Err(ImageError::InvalidData("GIF plain-text header size"));
                    }
                    r.take(12)?;
                    r.skip_subblocks()?;
                    control = None;
                } else {
                    r.skip_subblocks()?;
                }
            }
            0x2c => {
                let left = r.u16()? as u32;
                let top = r.u16()? as u32;
                let fw = r.u16()? as u32;
                let fh = r.u16()? as u32;
                if fw == 0
                    || fh == 0
                    || left.checked_add(fw).is_none_or(|x| x > sw)
                    || top.checked_add(fh).is_none_or(|y| y > sh)
                {
                    return Err(ImageError::InvalidData("GIF image rectangle"));
                }
                let ip = r.byte()?;
                let local = if ip & 0x80 != 0 {
                    Some(r.palette(1usize << ((ip & 7) + 1))?)
                } else {
                    None
                };
                let palette = local
                    .as_deref()
                    .or(global.as_deref())
                    .ok_or(ImageError::InvalidData("GIF missing color table"))?;
                let transparent = control.filter(|(enabled, _)| *enabled).map(|(_, i)| i);
                if transparent.is_some_and(|i| i as usize >= palette.len()) {
                    return Err(ImageError::InvalidData("GIF transparent index"));
                }
                if transparent.is_none() && global.as_ref().is_some_and(|p| background >= p.len()) {
                    return Err(ImageError::InvalidData("GIF background index"));
                }
                let bg = if transparent.is_none() {
                    global
                        .as_ref()
                        .and_then(|p| p.get(background))
                        .copied()
                        .map(|c| [c[0], c[1], c[2], 255])
                } else {
                    None
                };
                let mut pixels = allocate(screen_len)?;
                if let Some(color) = bg {
                    for px in pixels.chunks_exact_mut(4) {
                        px.copy_from_slice(&color);
                    }
                }
                let minimum = r.byte()?;
                if !(2..=8).contains(&minimum) {
                    return Err(ImageError::InvalidData("GIF LZW minimum code size"));
                }
                let compressed = r.subblocks()?;
                let count = usize::try_from(u64::from(fw) * u64::from(fh))
                    .map_err(|_| ImageError::LimitExceeded)?;
                let indices = lzw(&compressed, minimum, count)?;
                let mut row_order = Vec::new();
                row_order
                    .try_reserve_exact(fh as usize)
                    .map_err(|_| ImageError::LimitExceeded)?;
                if ip & 0x40 == 0 {
                    row_order.extend(0..fh);
                } else {
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        let mut y = start;
                        while y < fh {
                            row_order.push(y);
                            y += step;
                        }
                    }
                }
                for (n, &ix) in indices.iter().enumerate() {
                    let fx = (n as u32) % fw;
                    let fr = (n as u32) / fw;
                    let y = row_order[fr as usize];
                    if transparent == Some(ix) {
                        continue;
                    }
                    let c = *palette
                        .get(ix as usize)
                        .ok_or(ImageError::InvalidData("GIF palette index"))?;
                    let o = (((top + y) as usize * sw as usize) + (left + fx) as usize) * 4;
                    pixels[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
                return Ok(Image {
                    width: sw,
                    height: sh,
                    pixels,
                });
            }
            _ => return Err(ImageError::InvalidData("GIF block introducer")),
        }
    }
}

fn lzw(src: &[u8], minimum: u8, expected: usize) -> Result<Vec<u8>, ImageError> {
    let clear = 1u16 << minimum;
    let end = clear + 1;
    let mut next = end + 1;
    let mut width = minimum + 1;
    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    for i in 0..clear {
        suffix[i as usize] = i as u8;
    }
    let mut bits = BitReader { data: src, bit: 0 };
    let mut out = Vec::new();
    out.try_reserve_exact(expected)
        .map_err(|_| ImageError::LimitExceeded)?;
    let mut old: Option<u16> = None;
    let mut stack = [0u8; 4097];
    let mut saw_clear = false;
    let mut saw_end = false;
    while let Some(code) = bits.read(width) {
        if code == clear {
            next = end + 1;
            width = minimum + 1;
            old = None;
            saw_clear = true;
            continue;
        }
        if !saw_clear {
            return Err(ImageError::InvalidData("GIF LZW missing clear code"));
        }
        if code == end {
            saw_end = true;
            break;
        }
        let mut len = 0usize;
        let first;
        if code < next {
            let mut c = code;
            while c >= clear {
                if c as usize >= next as usize || len >= 4096 {
                    return Err(ImageError::InvalidData("GIF LZW dictionary chain"));
                }
                stack[len] = suffix[c as usize];
                len += 1;
                c = prefix[c as usize];
            }
            first = suffix[c as usize];
            stack[len] = first;
            len += 1;
        } else if code == next {
            let prev = old.ok_or(ImageError::InvalidData("GIF LZW code before dictionary"))?;
            let mut c = prev;
            while c >= clear {
                if c as usize >= next as usize || len >= 4096 {
                    return Err(ImageError::InvalidData("GIF LZW dictionary chain"));
                }
                stack[len] = suffix[c as usize];
                len += 1;
                c = prefix[c as usize];
            }
            first = suffix[c as usize];
            stack[len] = first;
            len += 1;
            stack[..len].reverse();
            if out.len().checked_add(len + 1).is_none_or(|n| n > expected) {
                return Err(ImageError::InvalidData("GIF LZW output length"));
            }
            out.extend_from_slice(&stack[..len]);
            out.push(first);
            if let Some(prev) = old {
                if next < 4096 {
                    prefix[next as usize] = prev;
                    suffix[next as usize] = first;
                    next += 1;
                    if next == (1 << width) && width < 12 {
                        width += 1;
                    }
                }
            }
            old = Some(code);
            continue;
        } else {
            return Err(ImageError::InvalidData("GIF LZW code out of range"));
        }
        stack[..len].reverse();
        if out.len().checked_add(len).is_none_or(|n| n > expected) {
            return Err(ImageError::InvalidData("GIF LZW output length"));
        }
        out.extend_from_slice(&stack[..len]);
        if let Some(prev) = old {
            if next < 4096 {
                prefix[next as usize] = prev;
                suffix[next as usize] = first;
                next += 1;
                if next == (1 << width) && width < 12 {
                    width += 1;
                }
            }
        }
        old = Some(code);
    }
    if !saw_end {
        return Err(ImageError::Truncated);
    }
    if out.len() != expected {
        return Err(ImageError::InvalidData("GIF LZW pixel count"));
    }
    Ok(out)
}
struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}
impl BitReader<'_> {
    fn read(&mut self, n: u8) -> Option<u16> {
        let end = self.bit.checked_add(n as usize)?;
        if end > self.data.len().checked_mul(8)? {
            return None;
        }
        let mut v = 0u16;
        for i in 0..n as usize {
            v |= (((self.data[(self.bit + i) / 8] >> ((self.bit + i) % 8)) & 1) as u16) << i;
        }
        self.bit = end;
        Some(v)
    }
}
fn pixel_len(w: u32, h: u32, limits: DecodeLimits) -> Result<usize, ImageError> {
    let n = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or(ImageError::LimitExceeded)?;
    if n == 0 || n > limits.max_pixels {
        return Err(ImageError::LimitExceeded);
    }
    usize::try_from(n.checked_mul(4).ok_or(ImageError::LimitExceeded)?)
        .map_err(|_| ImageError::LimitExceeded)
}
fn allocate(len: usize) -> Result<Vec<u8>, ImageError> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| ImageError::LimitExceeded)?;
    v.resize(len, 0);
    Ok(v)
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ImageError> {
        let end = self.pos.checked_add(n).ok_or(ImageError::LimitExceeded)?;
        let b = self.data.get(self.pos..end).ok_or(ImageError::Truncated)?;
        self.pos = end;
        Ok(b)
    }
    fn byte(&mut self) -> Result<u8, ImageError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ImageError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn palette(&mut self, n: usize) -> Result<Vec<[u8; 3]>, ImageError> {
        let b = self.take(n.checked_mul(3).ok_or(ImageError::LimitExceeded)?)?;
        Ok(b.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
    }
    fn subblocks(&mut self) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        loop {
            let n = self.byte()? as usize;
            if n == 0 {
                break;
            }
            let b = self.take(n)?;
            out.try_reserve(n).map_err(|_| ImageError::LimitExceeded)?;
            out.extend_from_slice(b)
        }
        Ok(out)
    }
    fn skip_subblocks(&mut self) -> Result<(), ImageError> {
        loop {
            let n = self.byte()? as usize;
            if n == 0 {
                return Ok(());
            }
            self.take(n)?;
        }
    }
}
