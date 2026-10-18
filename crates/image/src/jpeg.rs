// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded baseline and progressive Huffman JPEG decoder.

use crate::{DecodeLimits, Image, ImageError};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const MAX_COEFFICIENT_BYTES: usize = 256 * 1024 * 1024;
const MAX_JPEG_SEGMENTS: usize = 4096;
const MAX_JPEG_SCANS: usize = 1024;
const MAX_JPEG_BLOCK_WORK: usize = 64 * 1024 * 1024;
const MAX_RESTART_MARKERS: usize = 1024 * 1024;

/// JPEG decode result. `orientation` is the EXIF TIFF orientation (1 when absent).
/// Pixels are kept in encoded raster order; callers may apply the orientation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedJpeg {
    pub image: Image,
    pub orientation: u8,
}

/// Decode JPEG pixels in encoded raster order. EXIF orientation is available through
/// [`decode_jpeg_with_metadata`], and is deliberately not applied here.
pub fn decode_jpeg(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError> {
    Ok(decode_jpeg_with_metadata(data, limits)?.image)
}

/// Decode JPEG pixels and return the EXIF orientation (1 through 8, default 1).
pub fn decode_jpeg_with_metadata(
    data: &[u8],
    limits: DecodeLimits,
) -> Result<DecodedJpeg, ImageError> {
    Decoder::new(data, limits)?.decode()
}

#[derive(Clone)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: u8,
    dc_table: u8,
    ac_table: u8,
    blocks_x: usize,
    blocks_y: usize,
    scan_x: usize,
    scan_y: usize,
    coefficients: Vec<i16>,
    predictor: i32,
    progression: [i8; 64],
}

#[derive(Clone)]
struct Huffman {
    counts: [u8; 16],
    values: Vec<u8>,
}

impl Huffman {
    fn parse(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.len() < 16 {
            return Err(ImageError::Truncated);
        }
        let mut counts = [0; 16];
        counts.copy_from_slice(&bytes[..16]);
        let n: usize = counts.iter().map(|&v| usize::from(v)).sum();
        if n == 0 || n > 256 || bytes.len() != 16 + n {
            return Err(ImageError::InvalidData("invalid JPEG Huffman table length"));
        }
        let mut available = 1i32;
        for &count in &counts {
            available = available * 2 - i32::from(count);
            if available < 0 {
                return Err(ImageError::InvalidData("oversubscribed JPEG Huffman table"));
            }
        }
        if available == 0 {
            return Err(ImageError::InvalidData(
                "JPEG Huffman table uses an all-ones code",
            ));
        }
        Ok(Self {
            counts,
            values: bytes[16..].to_vec(),
        })
    }

    fn symbol(&self, bits: &mut Entropy<'_>) -> Result<u8, ImageError> {
        let mut code = 0u32;
        let mut index = 0usize;
        for length in 1..=16 {
            code = (code << 1) | u32::from(bits.read_bit()?);
            let count = usize::from(self.counts[length - 1]);
            let first = canonical_first(&self.counts, length);
            if code >= first && code < first + count as u32 {
                let at = index + (code - first) as usize;
                return self
                    .values
                    .get(at)
                    .copied()
                    .ok_or(ImageError::InvalidData("bad JPEG Huffman code"));
            }
            index += count;
        }
        Err(ImageError::InvalidData("invalid JPEG Huffman code"))
    }
}

fn canonical_first(counts: &[u8; 16], len: usize) -> u32 {
    let mut code = 0u32;
    for &count in counts.iter().take(len - 1) {
        code = (code + u32::from(count)) << 1;
    }
    code
}

struct Decoder<'a> {
    data: &'a [u8],
    limits: DecodeLimits,
    pos: usize,
    width: usize,
    height: usize,
    max_h: usize,
    max_v: usize,
    progressive: bool,
    components: Vec<Component>,
    quant: [Option<[u16; 64]>; 4],
    dc_tables: [Option<Huffman>; 4],
    ac_tables: [Option<Huffman>; 4],
    restart_interval: usize,
    adobe: Option<u8>,
    orientation: u8,
    saw_scan: bool,
    marker_count: usize,
    scan_count: usize,
    decoded_blocks: usize,
    restart_count: usize,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8], limits: DecodeLimits) -> Result<Self, ImageError> {
        if data.len() > limits.max_input_bytes {
            return Err(ImageError::LimitExceeded);
        }
        if data.len() < 2 {
            return Err(ImageError::Truncated);
        }
        if data[..2] != [0xff, 0xd8] {
            return Err(ImageError::InvalidData("missing JPEG SOI"));
        }
        Ok(Self {
            data,
            limits,
            pos: 2,
            width: 0,
            height: 0,
            max_h: 0,
            max_v: 0,
            progressive: false,
            components: Vec::new(),
            quant: [None; 4],
            dc_tables: std::array::from_fn(|_| None),
            ac_tables: std::array::from_fn(|_| None),
            restart_interval: 0,
            adobe: None,
            orientation: 1,
            saw_scan: false,
            marker_count: 0,
            scan_count: 0,
            decoded_blocks: 0,
            restart_count: 0,
        })
    }

    fn decode(mut self) -> Result<DecodedJpeg, ImageError> {
        loop {
            let marker = self.marker()?;
            match marker {
                0xd9 => break,
                0xd8 | 0x01 | 0xd0..=0xd7 => {
                    return Err(ImageError::InvalidData("unexpected JPEG marker"));
                }
                0xc0 | 0xc2 => self.read_frame(marker == 0xc2)?,
                0xc4 => self.read_huffman()?,
                0xdb => self.read_quant()?,
                0xdd => self.read_restart_interval()?,
                0xe1 => self.read_exif()?,
                0xee => self.read_adobe()?,
                0xda => self.read_scan()?,
                0xc1 | 0xc3 | 0xc5..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                    return Err(ImageError::Unsupported("JPEG frame coding process"));
                }
                _ => {
                    let _ = self.segment()?;
                }
            }
        }
        if !self.saw_scan || self.width == 0 || self.components.is_empty() {
            return Err(ImageError::InvalidData("incomplete JPEG image"));
        }
        if self.components.iter().any(|c| c.progression[0] < 0) {
            return Err(ImageError::InvalidData(
                "JPEG image is missing a component scan",
            ));
        }
        self.render()
    }

    fn marker(&mut self) -> Result<u8, ImageError> {
        self.marker_count = self
            .marker_count
            .checked_add(1)
            .ok_or(ImageError::LimitExceeded)?;
        if self.marker_count > MAX_JPEG_SEGMENTS {
            return Err(ImageError::LimitExceeded);
        }
        if self.pos >= self.data.len() {
            return Err(ImageError::Truncated);
        }
        if self.data[self.pos] != 0xff {
            return Err(ImageError::InvalidData("expected JPEG marker"));
        }
        while self.pos < self.data.len() && self.data[self.pos] == 0xff {
            self.pos += 1;
        }
        let marker = *self.data.get(self.pos).ok_or(ImageError::Truncated)?;
        self.pos += 1;
        if marker == 0 {
            return Err(ImageError::InvalidData("stuffed byte outside JPEG entropy"));
        }
        Ok(marker)
    }

    fn segment(&mut self) -> Result<&'a [u8], ImageError> {
        let len = be_u16(self.data, self.pos)? as usize;
        if len < 2 {
            return Err(ImageError::InvalidData("invalid JPEG segment length"));
        }
        let end = self.pos.checked_add(len).ok_or(ImageError::LimitExceeded)?;
        if end > self.data.len() {
            return Err(ImageError::Truncated);
        }
        let payload = &self.data[self.pos + 2..end];
        self.pos = end;
        Ok(payload)
    }

    fn read_frame(&mut self, progressive: bool) -> Result<(), ImageError> {
        if self.width != 0 {
            return Err(ImageError::InvalidData("multiple JPEG frames"));
        }
        let p = self.segment()?;
        if p.len() < 6 || p[0] != 8 {
            return Err(ImageError::Unsupported("JPEG sample precision"));
        }
        let h = usize::from(be_u16(p, 1)?);
        let w = usize::from(be_u16(p, 3)?);
        let n = usize::from(p[5]);
        if w == 0 || h == 0 || !(1..=4).contains(&n) || p.len() != 6 + 3 * n {
            return Err(ImageError::InvalidData("invalid JPEG frame header"));
        }
        if n == 2 {
            return Err(ImageError::Unsupported("two-component JPEG color model"));
        }
        self.validate_adobe_components(n)?;
        let pixels = (w as u64)
            .checked_mul(h as u64)
            .ok_or(ImageError::LimitExceeded)?;
        if pixels > self.limits.max_pixels {
            return Err(ImageError::LimitExceeded);
        }
        let mut comps = Vec::with_capacity(n);
        let mut ids = [false; 256];
        let mut mh = 0usize;
        let mut mv = 0usize;
        let mut sampling_sum = 0usize;
        for i in 0..n {
            let at = 6 + 3 * i;
            let id = p[at];
            if ids[usize::from(id)] {
                return Err(ImageError::InvalidData("duplicate JPEG component id"));
            }
            ids[usize::from(id)] = true;
            let hv = p[at + 1];
            let hs = usize::from(hv >> 4);
            let vs = usize::from(hv & 15);
            let q = p[at + 2];
            if !(1..=4).contains(&hs) || !(1..=4).contains(&vs) || q > 3 {
                return Err(ImageError::InvalidData("invalid JPEG sampling factors"));
            }
            mh = mh.max(hs);
            mv = mv.max(vs);
            sampling_sum = sampling_sum
                .checked_add(hs * vs)
                .ok_or(ImageError::LimitExceeded)?;
            comps.push(Component {
                id,
                h: hs,
                v: vs,
                quant: q,
                dc_table: 0,
                ac_table: 0,
                blocks_x: 0,
                blocks_y: 0,
                scan_x: 0,
                scan_y: 0,
                coefficients: Vec::new(),
                predictor: 0,
                progression: [-1; 64],
            });
        }
        if sampling_sum > 10 {
            return Err(ImageError::InvalidData(
                "JPEG sampling factors exceed frame limit",
            ));
        }
        let mcu_x = div_ceil(w, 8 * mh);
        let mcu_y = div_ceil(h, 8 * mv);
        let mut coeff_bytes = 0usize;
        for c in &mut comps {
            c.blocks_x = mcu_x.checked_mul(c.h).ok_or(ImageError::LimitExceeded)?;
            c.blocks_y = mcu_y.checked_mul(c.v).ok_or(ImageError::LimitExceeded)?;
            c.scan_x = div_ceil(w.checked_mul(c.h).ok_or(ImageError::LimitExceeded)?, 8 * mh);
            c.scan_y = div_ceil(h.checked_mul(c.v).ok_or(ImageError::LimitExceeded)?, 8 * mv);
            let count = c
                .blocks_x
                .checked_mul(c.blocks_y)
                .and_then(|x| x.checked_mul(64))
                .ok_or(ImageError::LimitExceeded)?;
            coeff_bytes = coeff_bytes
                .checked_add(count.checked_mul(2).ok_or(ImageError::LimitExceeded)?)
                .ok_or(ImageError::LimitExceeded)?;
            if coeff_bytes > MAX_COEFFICIENT_BYTES {
                return Err(ImageError::LimitExceeded);
            }
            c.coefficients
                .try_reserve_exact(count)
                .map_err(|_| ImageError::LimitExceeded)?;
            c.coefficients.resize(count, 0);
        }
        self.width = w;
        self.height = h;
        self.max_h = mh;
        self.max_v = mv;
        self.progressive = progressive;
        self.components = comps;
        Ok(())
    }

    fn read_quant(&mut self) -> Result<(), ImageError> {
        let p = self.segment()?;
        let mut at = 0;
        while at < p.len() {
            let info = p[at];
            at += 1;
            let precision = info >> 4;
            let id = usize::from(info & 15);
            if precision > 1 || id > 3 {
                return Err(ImageError::InvalidData("invalid JPEG quantization table"));
            }
            let size = 64 * (usize::from(precision) + 1);
            if at + size > p.len() {
                return Err(ImageError::Truncated);
            }
            let mut t = [0u16; 64];
            for z in 0..64 {
                t[ZIGZAG[z]] = if precision == 0 {
                    u16::from(p[at + z])
                } else {
                    be_u16(p, at + 2 * z)?
                };
            }
            if t.contains(&0) {
                return Err(ImageError::InvalidData("zero JPEG quantizer"));
            }
            self.quant[id] = Some(t);
            at += size;
        }
        Ok(())
    }

    fn read_huffman(&mut self) -> Result<(), ImageError> {
        let p = self.segment()?;
        let mut at = 0;
        while at < p.len() {
            let info = p[at];
            at += 1;
            let class = info >> 4;
            let id = usize::from(info & 15);
            if class > 1 || id > 3 || at + 16 > p.len() {
                return Err(ImageError::InvalidData("invalid JPEG Huffman selector"));
            }
            let n: usize = p[at..at + 16].iter().map(|&v| usize::from(v)).sum();
            if at + 16 + n > p.len() {
                return Err(ImageError::Truncated);
            }
            let table = Huffman::parse(&p[at..at + 16 + n])?;
            if (class == 0 && table.values.iter().any(|&symbol| symbol > 11))
                || (class == 1 && table.values.iter().any(|&symbol| symbol & 15 > 10))
            {
                return Err(ImageError::InvalidData("invalid JPEG Huffman symbol"));
            }
            at += 16 + n;
            if class == 0 {
                self.dc_tables[id] = Some(table);
            } else {
                self.ac_tables[id] = Some(table);
            }
        }
        Ok(())
    }

    fn read_restart_interval(&mut self) -> Result<(), ImageError> {
        let p = self.segment()?;
        if p.len() != 2 {
            return Err(ImageError::InvalidData("invalid JPEG DRI"));
        }
        self.restart_interval = usize::from(be_u16(p, 0)?);
        Ok(())
    }

    fn read_exif(&mut self) -> Result<(), ImageError> {
        let p = self.segment()?;
        if p.starts_with(b"Exif\0\0") {
            // Orientation is metadata: a malformed EXIF block or an orientation
            // outside 1..=8 means "as stored", never a refused image.
            self.orientation = parse_exif_orientation(&p[6..]).unwrap_or(1);
        }
        Ok(())
    }

    fn read_adobe(&mut self) -> Result<(), ImageError> {
        let p = self.segment()?;
        if p.starts_with(b"Adobe") {
            if p.len() != 12 {
                return Err(ImageError::InvalidData("invalid Adobe JPEG marker"));
            }
            let transform = p[11];
            if transform > 2 {
                return Err(ImageError::InvalidData("invalid Adobe JPEG transform"));
            }
            self.adobe = Some(transform);
            if !self.components.is_empty() {
                self.validate_adobe_components(self.components.len())?;
            }
        }
        Ok(())
    }

    fn validate_adobe_components(&self, count: usize) -> Result<(), ImageError> {
        let valid = match self.adobe {
            None => true,
            Some(0) => count == 3 || count == 4,
            Some(1) => count == 3,
            Some(2) => count == 4,
            Some(_) => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ImageError::InvalidData(
                "Adobe JPEG transform does not match component count",
            ))
        }
    }

    fn read_scan(&mut self) -> Result<(), ImageError> {
        self.scan_count = self
            .scan_count
            .checked_add(1)
            .ok_or(ImageError::LimitExceeded)?;
        if self.scan_count > MAX_JPEG_SCANS {
            return Err(ImageError::LimitExceeded);
        }
        if self.width == 0 {
            return Err(ImageError::InvalidData("JPEG scan before frame"));
        }
        self.validate_adobe_components(self.components.len())?;
        let p = self.segment()?;
        if p.len() < 4 {
            return Err(ImageError::InvalidData("invalid JPEG scan header"));
        }
        let n = usize::from(p[0]);
        if n == 0 || n > self.components.len() || p.len() != 1 + 2 * n + 3 {
            return Err(ImageError::InvalidData("invalid JPEG scan components"));
        }
        let mut scan = Vec::with_capacity(n);
        for i in 0..n {
            let id = p[1 + 2 * i];
            let selector = p[2 + 2 * i];
            let ci = self
                .components
                .iter()
                .position(|c| c.id == id)
                .ok_or(ImageError::InvalidData("unknown JPEG scan component"))?;
            if scan.contains(&ci) || selector >> 4 > 3 || selector & 15 > 3 {
                return Err(ImageError::InvalidData("invalid JPEG scan selector"));
            }
            self.components[ci].dc_table = selector >> 4;
            self.components[ci].ac_table = selector & 15;
            scan.push(ci);
        }
        let ss = p[1 + 2 * n];
        let se = p[2 + 2 * n];
        let ah = p[3 + 2 * n] >> 4;
        let al = p[3 + 2 * n] & 15;
        if (!self.progressive && (ss != 0 || se != 63 || ah != 0 || al != 0))
            || (self.progressive
                && (ss > se
                    || se > 63
                    || (ss == 0 && se != 0)
                    || (ss != 0 && n != 1)
                    || ah > 13
                    || al > 13
                    || (ah != 0 && ah != al + 1)))
        {
            return Err(ImageError::InvalidData("invalid JPEG scan parameters"));
        }
        for &ci in &scan {
            for k in usize::from(ss)..=usize::from(se) {
                let state = self.components[ci].progression[k];
                let valid = if ah == 0 {
                    state == -1
                } else {
                    state == ah as i8
                };
                if !valid {
                    return Err(ImageError::InvalidData(
                        "invalid JPEG successive approximation order",
                    ));
                }
            }
        }
        let mcu_cols = if n > 1 {
            div_ceil(self.width, 8 * self.max_h)
        } else {
            self.components[scan[0]].scan_x
        };
        let mcu_rows = if n > 1 {
            div_ceil(self.height, 8 * self.max_v)
        } else {
            self.components[scan[0]].scan_y
        };
        let total = mcu_cols
            .checked_mul(mcu_rows)
            .ok_or(ImageError::LimitExceeded)?;
        let mut ent = Entropy::new(self.data, self.pos);
        let mut eob_run = 0usize;
        let mut rst = 0u8;
        for mcu in 0..total {
            if self.restart_interval > 0 && mcu > 0 && mcu % self.restart_interval == 0 {
                self.restart_count = self
                    .restart_count
                    .checked_add(1)
                    .ok_or(ImageError::LimitExceeded)?;
                if self.restart_count > MAX_RESTART_MARKERS {
                    return Err(ImageError::LimitExceeded);
                }
                if eob_run != 0 {
                    return Err(ImageError::InvalidData(
                        "progressive JPEG EOB run crosses restart marker",
                    ));
                }
                ent.restart(0xd0 + rst)?;
                rst = (rst + 1) & 7;
                for c in &mut self.components {
                    c.predictor = 0;
                }
            }
            let mx = mcu % mcu_cols;
            let my = mcu / mcu_cols;
            if n > 1 {
                for &ci in &scan {
                    let h = self.components[ci].h;
                    let v = self.components[ci].v;
                    for by in 0..v {
                        for bx in 0..h {
                            let bi = (my * v + by) * self.components[ci].blocks_x + (mx * h + bx);
                            self.decode_block(ci, bi, &mut ent, ss, se, ah, al, &mut eob_run)?;
                        }
                    }
                }
            } else {
                let ci = scan[0];
                self.decode_block(
                    ci,
                    my * self.components[ci].blocks_x + mx,
                    &mut ent,
                    ss,
                    se,
                    ah,
                    al,
                    &mut eob_run,
                )?;
            }
        }
        if eob_run != 0 {
            return Err(ImageError::InvalidData(
                "progressive JPEG EOB run exceeds scan",
            ));
        }
        self.pos = ent.finish()?;
        for &ci in &scan {
            for k in usize::from(ss)..=usize::from(se) {
                self.components[ci].progression[k] = al as i8;
            }
        }
        self.saw_scan = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Keeps scan and block state explicit at this boundary.
    fn decode_block(
        &mut self,
        ci: usize,
        bi: usize,
        ent: &mut Entropy<'_>,
        ss: u8,
        se: u8,
        ah: u8,
        al: u8,
        eob: &mut usize,
    ) -> Result<(), ImageError> {
        self.decoded_blocks = self
            .decoded_blocks
            .checked_add(1)
            .ok_or(ImageError::LimitExceeded)?;
        if self.decoded_blocks > MAX_JPEG_BLOCK_WORK {
            return Err(ImageError::LimitExceeded);
        }
        let dc_id = usize::from(self.components[ci].dc_table);
        let ac_id = usize::from(self.components[ci].ac_table);
        let base = bi.checked_mul(64).ok_or(ImageError::LimitExceeded)?;
        if !self.progressive {
            let dc = self.dc_tables[dc_id]
                .as_ref()
                .ok_or(ImageError::InvalidData("missing JPEG DC table"))?
                .symbol(ent)?;
            if dc > 11 {
                return Err(ImageError::InvalidData("invalid JPEG DC category"));
            }
            let diff = receive(ent, dc)?;
            let c = &mut self.components[ci];
            c.predictor = c
                .predictor
                .checked_add(diff)
                .ok_or(ImageError::InvalidData("JPEG DC overflow"))?;
            set_coefficient(&mut c.coefficients, base, c.predictor)?;
            let ac = self.ac_tables[ac_id]
                .as_ref()
                .ok_or(ImageError::InvalidData("missing JPEG AC table"))?;
            let mut k = 1usize;
            while k < 64 {
                let rs = ac.symbol(ent)?;
                let run = usize::from(rs >> 4);
                let size = rs & 15;
                if size == 0 {
                    if run == 0 {
                        break;
                    }
                    if run != 15 {
                        return Err(ImageError::InvalidData("invalid JPEG AC run"));
                    }
                    if k + 16 > 64 {
                        return Err(ImageError::InvalidData("JPEG AC run exceeds block"));
                    }
                    k += 16;
                    continue;
                }
                if size > 10 {
                    return Err(ImageError::InvalidData("invalid JPEG AC category"));
                }
                k = k
                    .checked_add(run)
                    .ok_or(ImageError::InvalidData("JPEG AC run overflow"))?;
                if k >= 64 {
                    return Err(ImageError::InvalidData("JPEG AC coefficient outside block"));
                }
                let value = receive(ent, size)?;
                set_coefficient(
                    &mut self.components[ci].coefficients,
                    base + ZIGZAG[k],
                    value,
                )?;
                k += 1;
            }
            return Ok(());
        }
        if ss == 0 {
            if ah == 0 {
                let category = self.dc_tables[dc_id]
                    .as_ref()
                    .ok_or(ImageError::InvalidData("missing JPEG DC table"))?
                    .symbol(ent)?;
                if category > 11 {
                    return Err(ImageError::InvalidData("invalid JPEG DC category"));
                }
                let c = &mut self.components[ci];
                c.predictor = c
                    .predictor
                    .checked_add(receive(ent, category)?)
                    .ok_or(ImageError::InvalidData("JPEG DC overflow"))?;
                set_coefficient(
                    &mut c.coefficients,
                    base,
                    c.predictor
                        .checked_mul(1i32 << al)
                        .ok_or(ImageError::InvalidData("JPEG DC overflow"))?,
                )?;
            } else {
                let bit = i32::from(ent.read_bit()?);
                let coeff = &mut self.components[ci].coefficients[base];
                if bit != 0 && (*coeff & (1i16 << al)) == 0 {
                    let adjustment = if *coeff > 0 {
                        1i16 << al
                    } else {
                        -(1i16 << al)
                    };
                    *coeff = coeff
                        .checked_add(adjustment)
                        .ok_or(ImageError::InvalidData("JPEG DC refinement overflow"))?;
                }
            }
            return Ok(());
        }
        let ac = self.ac_tables[ac_id]
            .as_ref()
            .ok_or(ImageError::InvalidData("missing JPEG AC table"))?;
        if ah == 0 {
            if *eob > 0 {
                *eob -= 1;
                return Ok(());
            }
            let mut k = usize::from(ss);
            while k <= usize::from(se) {
                let rs = ac.symbol(ent)?;
                let run = usize::from(rs >> 4);
                let size = rs & 15;
                if size > 10 {
                    return Err(ImageError::InvalidData("invalid JPEG AC category"));
                }
                if size == 0 {
                    if run == 15 {
                        if k + 16 > usize::from(se) + 1 {
                            return Err(ImageError::InvalidData(
                                "JPEG progressive run exceeds band",
                            ));
                        }
                        k += 16;
                        continue;
                    }
                    *eob = (1usize << run) + ent.read_bits(rs >> 4)? as usize - 1;
                    break;
                }
                k = k
                    .checked_add(run)
                    .ok_or(ImageError::InvalidData("JPEG progressive run overflow"))?;
                if k > usize::from(se) {
                    return Err(ImageError::InvalidData(
                        "JPEG progressive coefficient outside band",
                    ));
                }
                let value = receive(ent, size)?
                    .checked_mul(1i32 << al)
                    .ok_or(ImageError::InvalidData("JPEG AC overflow"))?;
                set_coefficient(
                    &mut self.components[ci].coefficients,
                    base + ZIGZAG[k],
                    value,
                )?;
                k += 1;
            }
        } else {
            let p1 = 1i16 << al;
            let m1 = -p1;
            let mut k = usize::from(ss);
            if *eob == 0 {
                while k <= usize::from(se) {
                    let rs = ac.symbol(ent)?;
                    let run = usize::from(rs >> 4);
                    let size = rs & 15;
                    if size > 1 {
                        return Err(ImageError::InvalidData("invalid progressive AC refinement"));
                    }
                    let mut new_value = 0i16;
                    if size != 0 {
                        if size != 1 {
                            return Err(ImageError::InvalidData(
                                "invalid progressive AC refinement",
                            ));
                        }
                        new_value = if ent.read_bit()? != 0 { p1 } else { m1 };
                    } else if run != 15 {
                        *eob = (1usize << run) + ent.read_bits(rs >> 4)? as usize;
                        break;
                    }
                    let mut zeros = if size == 0 { 16 } else { run };
                    while k <= usize::from(se) {
                        let idx = base + ZIGZAG[k];
                        let value = self.components[ci].coefficients[idx];
                        if value != 0 {
                            if ent.read_bit()? != 0 && (value & p1) == 0 {
                                let adjustment = if value > 0 { p1 } else { m1 };
                                self.components[ci].coefficients[idx] = value
                                    .checked_add(adjustment)
                                    .ok_or(ImageError::InvalidData("JPEG refinement overflow"))?;
                            }
                        } else if zeros == 0 {
                            break;
                        } else {
                            zeros -= 1;
                        }
                        k += 1;
                    }
                    if size != 0 {
                        if k > usize::from(se) {
                            return Err(ImageError::InvalidData("JPEG AC refinement outside band"));
                        }
                        self.components[ci].coefficients[base + ZIGZAG[k]] = new_value;
                        k += 1;
                    } else if zeros > 0 {
                        return Err(ImageError::InvalidData("JPEG refinement run exceeds band"));
                    }
                }
            }
            if *eob > 0 {
                while k <= usize::from(se) {
                    let idx = base + ZIGZAG[k];
                    let value = self.components[ci].coefficients[idx];
                    if value != 0 && ent.read_bit()? != 0 && (value & p1) == 0 {
                        let adjustment = if value > 0 { p1 } else { m1 };
                        self.components[ci].coefficients[idx] = value
                            .checked_add(adjustment)
                            .ok_or(ImageError::InvalidData("JPEG refinement overflow"))?;
                    }
                    k += 1;
                }
                *eob -= 1;
            }
        }
        Ok(())
    }

    fn render(self) -> Result<DecodedJpeg, ImageError> {
        self.validate_adobe_components(self.components.len())?;
        let mut planes = Vec::with_capacity(self.components.len());
        for c in &self.components {
            let q = self.quant[usize::from(c.quant)]
                .ok_or(ImageError::InvalidData("missing JPEG quantization table"))?;
            let sample_len = c
                .blocks_x
                .checked_mul(8)
                .and_then(|x| x.checked_mul(c.blocks_y.checked_mul(8)?))
                .ok_or(ImageError::LimitExceeded)?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(sample_len)
                .map_err(|_| ImageError::LimitExceeded)?;
            samples.resize(sample_len, 0);
            for by in 0..c.blocks_y {
                for bx in 0..c.blocks_x {
                    let block = &c.coefficients
                        [(by * c.blocks_x + bx) * 64..(by * c.blocks_x + bx + 1) * 64];
                    idct(
                        block,
                        &q,
                        &mut samples[(by * 8) * (c.blocks_x * 8) + bx * 8..],
                        c.blocks_x * 8,
                    )?;
                }
            }
            planes.push(samples);
        }
        let output_len = self
            .width
            .checked_mul(self.height)
            .and_then(|x| x.checked_mul(4))
            .ok_or(ImageError::LimitExceeded)?;
        let mut out = Vec::new();
        out.try_reserve_exact(output_len)
            .map_err(|_| ImageError::LimitExceeded)?;
        out.resize(output_len, 0);
        let ids: Vec<u8> = self.components.iter().map(|c| c.id).collect();
        for y in 0..self.height {
            for x in 0..self.width {
                let mut s = [0u8; 4];
                for (i, c) in self.components.iter().enumerate() {
                    s[i] = sample_plane(
                        &planes[i],
                        c.blocks_x * 8,
                        c.blocks_y * 8,
                        x,
                        y,
                        c.h,
                        self.max_h,
                        c.v,
                        self.max_v,
                    );
                }
                let rgba = convert_color(&s, &ids, self.adobe)?;
                let at = (y * self.width + x) * 4;
                out[at..at + 4].copy_from_slice(&rgba);
            }
        }
        Ok(DecodedJpeg {
            image: Image {
                width: self.width as u32,
                height: self.height as u32,
                pixels: out,
            },
            orientation: self.orientation,
        })
    }
}

struct Entropy<'a> {
    data: &'a [u8],
    pos: usize,
    byte: u8,
    left: u8,
}
impl<'a> Entropy<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            byte: 0,
            left: 0,
        }
    }
    fn next_byte(&mut self) -> Result<u8, ImageError> {
        let b = *self.data.get(self.pos).ok_or(ImageError::Truncated)?;
        self.pos += 1;
        if b != 0xff {
            return Ok(b);
        }
        let next = *self.data.get(self.pos).ok_or(ImageError::Truncated)?;
        self.pos += 1;
        if next == 0 {
            return Ok(0xff);
        }
        if next == 0xff {
            return Err(ImageError::InvalidData(
                "unexpected marker fill in entropy data",
            ));
        }
        Err(ImageError::InvalidData(
            "unexpected marker inside JPEG scan",
        ))
    }
    fn read_bit(&mut self) -> Result<u8, ImageError> {
        if self.left == 0 {
            self.byte = self.next_byte()?;
            self.left = 8;
        }
        self.left -= 1;
        Ok((self.byte >> self.left) & 1)
    }
    fn read_bits(&mut self, n: u8) -> Result<u32, ImageError> {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | u32::from(self.read_bit()?);
        }
        Ok(v)
    }
    fn restart(&mut self, expected: u8) -> Result<(), ImageError> {
        self.left = 0;
        if self.pos >= self.data.len() || self.data[self.pos] != 0xff {
            return Err(ImageError::InvalidData("missing JPEG restart marker"));
        }
        while self.pos < self.data.len() && self.data[self.pos] == 0xff {
            self.pos += 1;
        }
        let marker = *self.data.get(self.pos).ok_or(ImageError::Truncated)?;
        self.pos += 1;
        if marker != expected {
            return Err(ImageError::InvalidData("unexpected JPEG restart marker"));
        }
        Ok(())
    }
    fn finish(mut self) -> Result<usize, ImageError> {
        self.left = 0;
        if self.pos >= self.data.len() || self.data[self.pos] != 0xff {
            return Err(ImageError::InvalidData("missing marker after JPEG scan"));
        }
        Ok(self.pos)
    }
}

fn receive(ent: &mut Entropy<'_>, n: u8) -> Result<i32, ImageError> {
    if n > 16 {
        return Err(ImageError::InvalidData("JPEG magnitude too large"));
    }
    if n == 0 {
        return Ok(0);
    }
    let v = ent.read_bits(n)? as i32;
    let threshold = 1i32 << (n - 1);
    Ok(if v < threshold {
        v - ((1i32 << n) - 1)
    } else {
        v
    })
}
fn set_coefficient(values: &mut [i16], at: usize, value: i32) -> Result<(), ImageError> {
    let v =
        i16::try_from(value).map_err(|_| ImageError::InvalidData("JPEG coefficient overflow"))?;
    *values
        .get_mut(at)
        .ok_or(ImageError::InvalidData("JPEG block out of bounds"))? = v;
    Ok(())
}
fn be_u16(b: &[u8], at: usize) -> Result<u16, ImageError> {
    let v = b.get(at..at + 2).ok_or(ImageError::Truncated)?;
    Ok(u16::from_be_bytes([v[0], v[1]]))
}
fn div_ceil(a: usize, b: usize) -> usize {
    a / b + usize::from(a % b != 0)
}

fn idct(coeff: &[i16], q: &[u16; 64], out: &mut [u8], stride: usize) -> Result<(), ImageError> {
    static BASIS: std::sync::OnceLock<[[f64; 8]; 8]> = std::sync::OnceLock::new();
    let basis = BASIS.get_or_init(|| {
        std::array::from_fn(|frequency| {
            let scale = if frequency == 0 {
                std::f64::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            std::array::from_fn(|position| {
                scale
                    * (((2 * position + 1) * frequency) as f64 * std::f64::consts::PI / 16.0).cos()
            })
        })
    });
    let mut deq = [0f64; 64];
    for i in 0..64 {
        deq[i] = f64::from(coeff[i]) * f64::from(q[i]);
    }
    let mut horizontal = [0f64; 64];
    for v in 0..8 {
        for x in 0..8 {
            for u in 0..8 {
                horizontal[v * 8 + x] += deq[v * 8 + u] * basis[u][x];
            }
        }
    }
    #[allow(clippy::needless_range_loop)] // Matrix coordinates index the IDCT basis by column.
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0;
            for v in 0..8 {
                sum += horizontal[v * 8 + x] * basis[v][y];
            }
            let p = (0.25 * sum + 128.0).round().clamp(0.0, 255.0) as u8;
            let at = y * stride + x;
            *out.get_mut(at).ok_or(ImageError::LimitExceeded)? = p;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Dimensions, sampling factors, and coordinates are independent.
fn sample_plane(
    p: &[u8],
    pw: usize,
    ph: usize,
    x: usize,
    y: usize,
    h: usize,
    mh: usize,
    v: usize,
    mv: usize,
) -> u8 {
    let fx = ((x as f64 + 0.5) * h as f64 / mh as f64) - 0.5;
    let fy = ((y as f64 + 0.5) * v as f64 / mv as f64) - 0.5;
    let x0 = fx.floor().max(0.0) as usize;
    let y0 = fy.floor().max(0.0) as usize;
    let x1 = (x0 + 1).min(pw - 1);
    let y1 = (y0 + 1).min(ph - 1);
    let dx = fx.fract().clamp(0.0, 1.0);
    let dy = fy.fract().clamp(0.0, 1.0);
    let a = f64::from(p[y0 * pw + x0]) * (1.0 - dx) + f64::from(p[y0 * pw + x1]) * dx;
    let b = f64::from(p[y1 * pw + x0]) * (1.0 - dx) + f64::from(p[y1 * pw + x1]) * dx;
    (a * (1.0 - dy) + b * dy).round().clamp(0.0, 255.0) as u8
}

fn convert_color(s: &[u8; 4], ids: &[u8], adobe: Option<u8>) -> Result<[u8; 4], ImageError> {
    let rgb = if ids.len() == 1 {
        [s[0], s[0], s[0]]
    } else if ids.len() == 3 && (adobe == Some(0) || ids == b"RGB") {
        [s[0], s[1], s[2]]
    } else if ids.len() == 3 {
        ycbcr(s[0], s[1], s[2])
    } else if ids.len() == 4 {
        if adobe == Some(2) {
            let inverted_cmy = ycbcr(s[0], s[1], s[2]);
            [
                (u16::from(inverted_cmy[0]) * u16::from(s[3]) / 255) as u8,
                (u16::from(inverted_cmy[1]) * u16::from(s[3]) / 255) as u8,
                (u16::from(inverted_cmy[2]) * u16::from(s[3]) / 255) as u8,
            ]
        } else if adobe == Some(0) {
            [
                (u16::from(s[0]) * u16::from(s[3]) / 255) as u8,
                (u16::from(s[1]) * u16::from(s[3]) / 255) as u8,
                (u16::from(s[2]) * u16::from(s[3]) / 255) as u8,
            ]
        } else {
            [
                255u16.saturating_sub(u16::from(s[0]) + u16::from(s[3])) as u8,
                255u16.saturating_sub(u16::from(s[1]) + u16::from(s[3])) as u8,
                255u16.saturating_sub(u16::from(s[2]) + u16::from(s[3])) as u8,
            ]
        }
    } else {
        return Err(ImageError::Unsupported("JPEG color components"));
    };
    Ok([rgb[0], rgb[1], rgb[2], 255])
}
fn ycbcr(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let yy = f64::from(y);
    let cb = f64::from(cb) - 128.0;
    let cr = f64::from(cr) - 128.0;
    [
        (yy + 1.402 * cr).round().clamp(0.0, 255.0) as u8,
        (yy - 0.344_136 * cb - 0.714_136 * cr)
            .round()
            .clamp(0.0, 255.0) as u8,
        (yy + 1.772 * cb).round().clamp(0.0, 255.0) as u8,
    ]
}

fn parse_exif_orientation(tiff: &[u8]) -> Result<u8, ImageError> {
    if tiff.len() < 8 {
        return Err(ImageError::InvalidData("truncated EXIF TIFF header"));
    }
    let le = match &tiff[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(ImageError::InvalidData("invalid EXIF byte order")),
    };
    let u16at = |at: usize| -> Result<u16, ImageError> {
        let end = at.checked_add(2).ok_or(ImageError::LimitExceeded)?;
        let b = tiff.get(at..end).ok_or(ImageError::Truncated)?;
        Ok(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32at = |at: usize| -> Result<u32, ImageError> {
        let end = at.checked_add(4).ok_or(ImageError::LimitExceeded)?;
        let b = tiff.get(at..end).ok_or(ImageError::Truncated)?;
        Ok(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    if u16at(2)? != 42 {
        return Err(ImageError::InvalidData("invalid EXIF TIFF magic"));
    }
    let ifd = usize::try_from(u32at(4)?).map_err(|_| ImageError::LimitExceeded)?;
    let count = usize::from(u16at(ifd)?);
    let end = ifd
        .checked_add(2)
        .and_then(|x| x.checked_add(count.checked_mul(12)?))
        .ok_or(ImageError::LimitExceeded)?;
    if end.checked_add(4).ok_or(ImageError::LimitExceeded)? > tiff.len() {
        return Err(ImageError::Truncated);
    }
    for i in 0..count {
        let at = ifd + 2 + 12 * i;
        if u16at(at)? == 0x0112 {
            if u16at(at + 2)? != 3 || u32at(at + 4)? != 1 {
                return Err(ImageError::InvalidData("invalid EXIF orientation entry"));
            }
            let value = u16at(at + 8)?;
            if !(1..=8).contains(&value) {
                return Err(ImageError::InvalidData("EXIF orientation outside 1..=8"));
            }
            return Ok(value as u8);
        }
    }
    Ok(1)
}
