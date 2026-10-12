// SPDX-License-Identifier: AGPL-3.0-or-later
//! CFF (version 1) subsetting for [`crate::subset`].
//!
//! The kept charstrings are copied byte for byte. The glyph set is closed over
//! `seac` components, charstrings are walked to find the subroutines they call
//! (unused subroutine bodies are replaced by `return`, trailing unused ones are
//! dropped when the call bias does not change), the String INDEX is reduced to
//! the strings still referenced and every offset of the Top DICT, FD DICTs and
//! Private DICTs is recomputed.

use crate::OutlineError;
use crate::cff::{CffFont, MAX_STACK, MAX_SUBR_DEPTH};
use crate::cff_tables::STANDARD_ENCODING;
use crate::subset::SubsetError;
use std::collections::{BTreeMap, BTreeSet};

type R<T> = Result<T, SubsetError>;

const MAX_DICT_ENTRIES: usize = 512;
const MAX_FONT_DICTS: usize = 256;
/// Total number of charstring operators executed while looking for subroutine calls.
const MAX_TOTAL_OPERATIONS: usize = 32_000_000;
const STANDARD_STRINGS: u16 = 391;

fn bad(s: &'static str) -> SubsetError {
    SubsetError::Cff(OutlineError::InvalidData(s))
}

pub(crate) struct CffSubset {
    pub data: Vec<u8>,
    /// Source glyph ids, ascending (glyph 0 first); the position is the new glyph id.
    pub kept: Vec<u16>,
    /// CID to use for each kept glyph.
    pub cids: Vec<u16>,
    pub cid_keyed: bool,
    pub name: Vec<u8>,
    pub bbox: Option<[i16; 4]>,
    pub italic_angle: f64,
    pub fixed_pitch: bool,
}

// ------------------------------------------------------------------ raw structures

fn be(b: &[u8], p: usize, n: usize) -> R<usize> {
    let s = b
        .get(p..p.checked_add(n).ok_or_else(|| bad("offset"))?)
        .ok_or(SubsetError::Cff(OutlineError::Truncated))?;
    Ok(s.iter().fold(0usize, |a, &x| (a << 8) | usize::from(x)))
}

/// Read a whole INDEX; returns its entries and the position after it.
fn read_index(buf: &[u8], pos: usize) -> R<(Vec<&[u8]>, usize)> {
    let count = be(buf, pos, 2)?;
    if count == 0 {
        return Ok((Vec::new(), pos + 2));
    }
    let off_size = be(buf, pos + 2, 1)?;
    if !(1..=4).contains(&off_size) {
        return Err(bad("INDEX offSize"));
    }
    let offsets = pos + 3;
    let data = offsets + (count + 1) * off_size;
    let mut prev = be(buf, offsets, off_size)?;
    if prev != 1 {
        return Err(bad("INDEX offsets"));
    }
    let mut items = Vec::with_capacity(count);
    for i in 1..=count {
        let next = be(buf, offsets + i * off_size, off_size)?;
        if next < prev {
            return Err(bad("INDEX offsets"));
        }
        let a = data + prev - 1;
        let b = data + next - 1;
        items.push(
            buf.get(a..b)
                .ok_or(SubsetError::Cff(OutlineError::Truncated))?,
        );
        prev = next;
    }
    Ok((items, data + prev - 1))
}

fn write_index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    let total: usize = items.iter().map(Vec::len).sum::<usize>() + 1;
    let off_size = match total {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    };
    out.push(off_size as u8);
    let mut off = 1usize;
    let put = |out: &mut Vec<u8>, v: usize| {
        out.extend_from_slice(&(v as u32).to_be_bytes()[4 - off_size..])
    };
    put(&mut out, off);
    for it in items {
        off += it.len();
        put(&mut out, off);
    }
    for it in items {
        out.extend_from_slice(it);
    }
    out
}

struct Entry {
    op: u16,
    /// Byte range and value of each operand, relative to the DICT bytes.
    operands: Vec<(usize, usize, f64)>,
}

struct Dict<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

fn parse_real(b: &[u8], mut p: usize) -> R<(f64, usize)> {
    let mut s = String::new();
    loop {
        let byte = *b.get(p).ok_or(SubsetError::Cff(OutlineError::Truncated))?;
        p += 1;
        for n in [byte >> 4, byte & 15] {
            match n {
                0..=9 => s.push(char::from(b'0' + n)),
                10 => s.push('.'),
                11 => s.push('E'),
                12 => s.push_str("E-"),
                14 => s.push('-'),
                15 => {
                    let v: f64 = s.parse().map_err(|_| bad("DICT real number"))?;
                    return if v.is_finite() {
                        Ok((v, p))
                    } else {
                        Err(bad("DICT real number"))
                    };
                }
                _ => return Err(bad("DICT real nibble")),
            }
            if s.len() > 64 {
                return Err(SubsetError::LimitExceeded("DICT real length"));
            }
        }
    }
}

fn parse_dict(b: &[u8]) -> R<Dict<'_>> {
    let mut entries = Vec::new();
    let mut operands: Vec<(usize, usize, f64)> = Vec::new();
    let mut p = 0;
    let byte = |p: usize| {
        b.get(p)
            .copied()
            .ok_or(SubsetError::Cff(OutlineError::Truncated))
    };
    while p < b.len() {
        let b0 = byte(p)?;
        let start = p;
        match b0 {
            0..=21 => {
                p += 1;
                let op = if b0 == 12 {
                    p += 1;
                    0x0c00 | u16::from(byte(p - 1)?)
                } else {
                    u16::from(b0)
                };
                if entries.len() >= MAX_DICT_ENTRIES {
                    return Err(SubsetError::LimitExceeded("DICT entries"));
                }
                entries.push(Entry {
                    op,
                    operands: std::mem::take(&mut operands),
                });
                continue;
            }
            28 => {
                let v = i16::from_be_bytes([byte(p + 1)?, byte(p + 2)?]);
                operands.push((start, start + 3, f64::from(v)));
                p += 3;
            }
            29 => {
                let v =
                    i32::from_be_bytes([byte(p + 1)?, byte(p + 2)?, byte(p + 3)?, byte(p + 4)?]);
                operands.push((start, start + 5, f64::from(v)));
                p += 5;
            }
            30 => {
                let (v, e) = parse_real(b, p + 1)?;
                operands.push((start, e, v));
                p = e;
            }
            32..=246 => {
                operands.push((start, start + 1, f64::from(i32::from(b0) - 139)));
                p += 1;
            }
            247..=250 => {
                let v = (i32::from(b0) - 247) * 256 + i32::from(byte(p + 1)?) + 108;
                operands.push((start, start + 2, f64::from(v)));
                p += 2;
            }
            251..=254 => {
                let v = -(i32::from(b0) - 251) * 256 - i32::from(byte(p + 1)?) - 108;
                operands.push((start, start + 2, f64::from(v)));
                p += 2;
            }
            _ => return Err(bad("reserved DICT byte")),
        }
        if operands.len() > MAX_STACK {
            return Err(SubsetError::LimitExceeded("DICT operands"));
        }
    }
    if !operands.is_empty() {
        return Err(bad("DICT operands without operator"));
    }
    Ok(Dict { bytes: b, entries })
}

impl Dict<'_> {
    fn get(&self, op: u16) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| e.op == op)
    }
    fn nums(&self, op: u16) -> Option<Vec<f64>> {
        self.get(op)
            .map(|e| e.operands.iter().map(|o| o.2).collect())
    }
    fn int(&self, op: u16) -> R<Option<usize>> {
        match self.nums(op).as_deref() {
            None => Ok(None),
            Some([v]) if v.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(v) => {
                Ok(Some(*v as usize))
            }
            Some(_) => Err(bad("DICT offset operand")),
        }
    }
    /// Copy an entry verbatim, remapping the first `sid_count` operands through `sids`.
    fn emit(&self, e: &Entry, out: &mut Vec<u8>, sids: &BTreeMap<u16, u16>) {
        let sid_count = sid_operands(e.op);
        for (i, &(a, b, v)) in e.operands.iter().enumerate() {
            match (i < sid_count, sids.get(&(v as u16))) {
                (true, Some(&new)) if v >= f64::from(STANDARD_STRINGS) => {
                    out.extend(enc_int(i32::from(new)));
                }
                _ => out.extend_from_slice(&self.bytes[a..b]),
            }
        }
        put_op(out, e.op);
    }
}

fn put_op(out: &mut Vec<u8>, op: u16) {
    if op >= 0x0c00 {
        out.extend([12, (op & 0xff) as u8]);
    } else {
        out.push(op as u8);
    }
}

/// Shortest DICT integer encoding.
fn enc_int(v: i32) -> Vec<u8> {
    match v {
        -107..=107 => vec![(v + 139) as u8],
        108..=1131 => vec![((v - 108) / 256 + 247) as u8, ((v - 108) % 256) as u8],
        -1131..=-108 => vec![((-v - 108) / 256 + 251) as u8, ((-v - 108) % 256) as u8],
        -32768..=32767 => vec![28, (v >> 8) as u8, v as u8],
        _ => fixed5(v as u32),
    }
}
/// Five byte DICT integer (size independent of the value, for offsets).
fn fixed5(v: u32) -> Vec<u8> {
    let mut b = vec![29];
    b.extend_from_slice(&v.to_be_bytes());
    b
}

/// Number of leading operands of a DICT operator that are string ids.
fn sid_operands(op: u16) -> usize {
    match op {
        0..=4 | 0x0c00 | 0x0c15 | 0x0c16 | 0x0c26 => 1,
        0x0c1e => 2,
        _ => 0,
    }
}

const OP_CHARSET: u16 = 15;
const OP_ENCODING: u16 = 16;
const OP_CHARSTRINGS: u16 = 17;
const OP_PRIVATE: u16 = 18;
const OP_SUBRS: u16 = 19;
const OP_ROS: u16 = 0x0c1e;
const OP_FDARRAY: u16 = 0x0c24;
const OP_FDSELECT: u16 = 0x0c25;

/// A Font DICT (or the Top DICT of a name-keyed font) with its Private DICT.
struct Fd<'a> {
    dict: Dict<'a>,
    private: Option<Dict<'a>>,
    subrs: Vec<&'a [u8]>,
}

impl<'a> Fd<'a> {
    fn load(cff: &'a [u8], dict: Dict<'a>) -> R<Self> {
        let mut fd = Fd {
            dict,
            private: None,
            subrs: Vec::new(),
        };
        match fd.dict.nums(OP_PRIVATE).as_deref() {
            None => {}
            Some(&[size, off])
                if size.fract() == 0.0 && off.fract() == 0.0 && size >= 0.0 && off >= 0.0 =>
            {
                let (size, off) = (size as usize, off as usize);
                let end = off
                    .checked_add(size)
                    .ok_or_else(|| bad("Private DICT range"))?;
                let bytes = cff.get(off..end).ok_or_else(|| bad("Private DICT range"))?;
                let private = parse_dict(bytes)?;
                if let Some(rel) = private.int(OP_SUBRS)? {
                    let pos = off.checked_add(rel).ok_or_else(|| bad("Subrs offset"))?;
                    fd.subrs = read_index(cff, pos)?.0;
                }
                fd.private = Some(private);
            }
            Some(_) => return Err(bad("Private operand count")),
        }
        Ok(fd)
    }
}

// ------------------------------------------------------------------ charstring walker

fn bias(count: usize) -> i64 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

struct Walker<'a, 'b> {
    gsubrs: &'b [&'a [u8]],
    lsubrs: &'b [&'a [u8]],
    gused: &'b mut [bool],
    lused: &'b mut [bool],
    budget: &'b mut usize,
    stack: Vec<f64>,
    transient: [f64; 32],
    stems: usize,
    /// `bchar` and `achar` of a `seac` `endchar`.
    seac: Option<(f64, f64)>,
}

impl Walker<'_, '_> {
    fn push(&mut self, v: f64) -> R<()> {
        if !v.is_finite() {
            return Err(bad("charstring number"));
        }
        if self.stack.len() >= MAX_STACK {
            return Err(SubsetError::LimitExceeded("charstring stack"));
        }
        self.stack.push(v);
        Ok(())
    }
    fn pop(&mut self) -> R<f64> {
        self.stack
            .pop()
            .ok_or_else(|| bad("charstring stack underflow"))
    }
    /// Returns whether `endchar` ended the glyph.
    fn run(&mut self, code: &[u8], depth: usize) -> R<bool> {
        let mut p = 0;
        let at = |p: usize| {
            code.get(p)
                .copied()
                .ok_or(SubsetError::Cff(OutlineError::Truncated))
        };
        while p < code.len() {
            *self.budget = self
                .budget
                .checked_sub(1)
                .ok_or(SubsetError::LimitExceeded("charstring work"))?;
            let b0 = at(p)?;
            p += 1;
            match b0 {
                28 => {
                    let v = i16::from_be_bytes([at(p)?, at(p + 1)?]);
                    p += 2;
                    self.push(f64::from(v))?;
                }
                32..=246 => self.push(f64::from(i32::from(b0) - 139))?,
                247..=250 => {
                    let v = (i32::from(b0) - 247) * 256 + i32::from(at(p)?) + 108;
                    p += 1;
                    self.push(f64::from(v))?;
                }
                251..=254 => {
                    let v = -(i32::from(b0) - 251) * 256 - i32::from(at(p)?) - 108;
                    p += 1;
                    self.push(f64::from(v))?;
                }
                255 => {
                    let v = i32::from_be_bytes([at(p)?, at(p + 1)?, at(p + 2)?, at(p + 3)?]);
                    p += 4;
                    self.push(f64::from(v) / 65536.0)?;
                }
                1 | 3 | 18 | 23 => {
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                }
                19 | 20 => {
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                    p += self.stems.div_ceil(8);
                }
                4..=8 | 21 | 22 | 24..=27 | 30 | 31 => self.stack.clear(),
                10 | 29 => {
                    let global = b0 == 29;
                    let n = if global {
                        self.gsubrs.len()
                    } else {
                        self.lsubrs.len()
                    };
                    let v = self.pop()?;
                    if v.fract() != 0.0 {
                        return Err(bad("subroutine number"));
                    }
                    let idx = v as i64 + bias(n);
                    if idx < 0 || idx as usize >= n {
                        return Err(bad("subroutine number"));
                    }
                    let idx = idx as usize;
                    if depth >= MAX_SUBR_DEPTH {
                        return Err(SubsetError::LimitExceeded("subroutine depth"));
                    }
                    let body = if global {
                        self.gused[idx] = true;
                        self.gsubrs[idx]
                    } else {
                        self.lused[idx] = true;
                        self.lsubrs[idx]
                    };
                    if self.run(body, depth + 1)? {
                        return Ok(true);
                    }
                }
                11 => return Ok(false),
                14 => {
                    let n = self.stack.len();
                    if n == 4 || n == 5 {
                        self.seac = Some((self.stack[n - 2], self.stack[n - 1]));
                    }
                    return Ok(true);
                }
                12 => {
                    let e = at(p)?;
                    p += 1;
                    self.escape(e)?;
                }
                _ => return Err(bad("reserved charstring operator")),
            }
        }
        Ok(false)
    }

    fn escape(&mut self, e: u8) -> R<()> {
        match e {
            34..=37 => self.stack.clear(),
            3 | 4 | 15 => {
                let (b, a) = (self.pop()?, self.pop()?);
                let r = match e {
                    3 => a != 0.0 && b != 0.0,
                    4 => a != 0.0 || b != 0.0,
                    _ => a == b,
                };
                self.push(f64::from(u8::from(r)))?;
            }
            5 => {
                let a = self.pop()?;
                self.push(f64::from(u8::from(a == 0.0)))?;
            }
            9 | 14 | 26 => {
                let a = self.pop()?;
                if e == 26 && a < 0.0 {
                    return Err(bad("sqrt of a negative number"));
                }
                self.push(match e {
                    9 => a.abs(),
                    14 => -a,
                    _ => a.sqrt(),
                })?;
            }
            10 | 11 | 12 | 24 => {
                let (b, a) = (self.pop()?, self.pop()?);
                if e == 12 && b == 0.0 {
                    return Err(bad("division by zero"));
                }
                self.push(match e {
                    10 => a + b,
                    11 => a - b,
                    12 => a / b,
                    _ => a * b,
                })?;
            }
            18 => {
                self.pop()?;
            }
            20 => {
                let i = self.pop()?;
                let v = self.pop()?;
                if i.fract() != 0.0 || !(0.0..32.0).contains(&i) {
                    return Err(bad("transient array index"));
                }
                self.transient[i as usize] = v;
            }
            21 => {
                let i = self.pop()?;
                if i.fract() != 0.0 || !(0.0..32.0).contains(&i) {
                    return Err(bad("transient array index"));
                }
                self.push(self.transient[i as usize])?;
            }
            22 => {
                let (v2, v1, s2, s1) = (self.pop()?, self.pop()?, self.pop()?, self.pop()?);
                self.push(if v1 <= v2 { s1 } else { s2 })?;
            }
            23 => self.push(crate::cff::RANDOM_VALUE)?,
            27 => {
                let a = self.pop()?;
                self.push(a)?;
                self.push(a)?;
            }
            28 => {
                let (b, a) = (self.pop()?, self.pop()?);
                self.push(b)?;
                self.push(a)?;
            }
            29 => {
                let i = self.pop()?;
                if i.fract() != 0.0 || self.stack.is_empty() {
                    return Err(bad("index operand"));
                }
                let i = (i.max(0.0) as usize).min(self.stack.len() - 1);
                let v = self.stack[self.stack.len() - 1 - i];
                self.push(v)?;
            }
            30 => {
                let (j, n) = (self.pop()?, self.pop()?);
                if j.fract() != 0.0 || n.fract() != 0.0 || n < 0.0 || n as usize > self.stack.len()
                {
                    return Err(bad("roll operands"));
                }
                let n = n as usize;
                if n > 0 {
                    let len = self.stack.len();
                    let j = (j as i64).rem_euclid(n as i64) as usize;
                    self.stack[len - n..].rotate_right(j);
                }
            }
            _ => return Err(bad("reserved charstring escape")),
        }
        Ok(())
    }
}

/// Smallest subroutine count that keeps the call bias of `total` and covers `max_used`.
fn trimmed_count(total: usize, used: &[bool]) -> usize {
    let Some(last) = used.iter().rposition(|&u| u) else {
        return 0;
    };
    let floor = match bias(total) {
        107 => 0,
        1131 => 1240,
        _ => 33900,
    };
    (last + 1).max(floor).min(total)
}

fn pruned_subrs(subrs: &[&[u8]], used: &[bool]) -> Vec<Vec<u8>> {
    (0..trimmed_count(subrs.len(), used))
        .map(|i| if used[i] { subrs[i].to_vec() } else { vec![11] })
        .collect()
}

// ------------------------------------------------------------------ subsetting

fn charset_bytes(values: &[u16]) -> Vec<u8> {
    let f0 = 1 + 2 * values.len();
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    for &v in values {
        match ranges.last_mut() {
            Some((first, left)) if u32::from(*first) + u32::from(*left) + 1 == u32::from(v) => {
                *left += 1;
            }
            _ => ranges.push((v, 0)),
        }
    }
    if 1 + 4 * ranges.len() < f0 {
        let mut out = vec![2];
        for (first, left) in ranges {
            out.extend(first.to_be_bytes());
            out.extend(left.to_be_bytes());
        }
        out
    } else {
        let mut out = vec![0];
        for v in values {
            out.extend(v.to_be_bytes());
        }
        out
    }
}

fn private_block(fd: &Fd<'_>, used: &[bool], sids: &BTreeMap<u16, u16>) -> (Vec<u8>, Vec<u8>) {
    let mut d = Vec::new();
    if let Some(p) = &fd.private {
        for e in p.entries.iter().filter(|e| e.op != OP_SUBRS) {
            p.emit(e, &mut d, sids);
        }
    }
    let subrs = pruned_subrs(&fd.subrs, used);
    if subrs.is_empty() {
        return (d, Vec::new());
    }
    let off = d.len() + 6;
    d.extend(fixed5(off as u32));
    d.push(OP_SUBRS as u8);
    (d, write_index(&subrs))
}

pub(crate) fn subset(cff: &[u8], wanted: &BTreeSet<u16>) -> R<CffSubset> {
    let font = CffFont::parse(cff)?;
    let glyph_count = font.glyph_count();
    if glyph_count > 0xffff {
        return Err(SubsetError::LimitExceeded("glyph count"));
    }
    let hdr = usize::from(
        *cff.get(2)
            .ok_or(SubsetError::Cff(OutlineError::Truncated))?,
    );
    let (names, pos) = read_index(cff, hdr)?;
    let (tops, pos) = read_index(cff, pos)?;
    let (strings, pos) = read_index(cff, pos)?;
    let (gsubrs, _) = read_index(cff, pos)?;
    let top = parse_dict(tops.first().ok_or_else(|| bad("empty Top DICT INDEX"))?)?;
    let cs_off = top
        .int(OP_CHARSTRINGS)?
        .ok_or_else(|| bad("missing CharStrings"))?;
    let (charstrings, _) = read_index(cff, cs_off)?;
    if charstrings.len() != glyph_count {
        return Err(bad("CharStrings count"));
    }
    let cid_keyed = top.get(OP_ROS).is_some();
    let charset = font.charset()?;

    let mut fds: Vec<Fd<'_>> = Vec::new();
    if cid_keyed {
        let off = top
            .int(OP_FDARRAY)?
            .ok_or_else(|| bad("CID font without FDArray"))?;
        let (dicts, _) = read_index(cff, off)?;
        if dicts.is_empty() || dicts.len() > MAX_FONT_DICTS {
            return Err(bad("FDArray size"));
        }
        for d in dicts {
            fds.push(Fd::load(cff, parse_dict(d)?)?);
        }
    } else {
        // The Top DICT of a name-keyed font acts as its only Font DICT (parsed a second time).
        fds.push(Fd::load(cff, parse_dict(top.bytes)?)?);
    }

    // Close the glyph set over seac components while recording subroutine use.
    let mut keep = wanted.clone();
    keep.insert(0);
    let mut queue: Vec<u16> = keep.iter().copied().collect();
    let mut gused = vec![false; gsubrs.len()];
    let mut lused: Vec<Vec<bool>> = fds.iter().map(|f| vec![false; f.subrs.len()]).collect();
    let mut budget = MAX_TOTAL_OPERATIONS;
    let mut fd_of: BTreeMap<u16, usize> = BTreeMap::new();
    while let Some(g) = queue.pop() {
        let fd = font.font_dict_index(g)?;
        fd_of.insert(g, fd);
        let code = charstrings[usize::from(g)];
        if code.is_empty() {
            continue;
        }
        let mut w = Walker {
            gsubrs: &gsubrs,
            lsubrs: &fds[fd].subrs,
            gused: &mut gused,
            lused: &mut lused[fd],
            budget: &mut budget,
            stack: Vec::new(),
            transient: [0.0; 32],
            stems: 0,
            seac: None,
        };
        if !w.run(code, 0)? {
            return Err(bad("charstring without endchar"));
        }
        if let Some((base, accent)) = w.seac {
            if cid_keyed {
                return Err(bad("seac in a CID-keyed font"));
            }
            for code in [base, accent] {
                if code.fract() != 0.0 || !(0.0..256.0).contains(&code) {
                    return Err(bad("seac character code"));
                }
                let sid = STANDARD_ENCODING[code as usize];
                if sid == 0 {
                    return Err(bad("seac code outside StandardEncoding"));
                }
                let comp = charset
                    .iter()
                    .position(|&s| s == sid)
                    .ok_or_else(|| bad("seac component missing from charset"))?;
                let comp = comp as u16;
                if keep.insert(comp) {
                    queue.push(comp);
                }
            }
        }
    }
    let kept: Vec<u16> = keep.iter().copied().collect();
    let k = kept.len();

    // Font DICTs in use.
    let used_fds: BTreeSet<usize> = kept.iter().map(|g| fd_of[g]).collect();
    let fd_new: BTreeMap<usize, usize> = used_fds
        .iter()
        .copied()
        .enumerate()
        .map(|(n, o)| (o, n))
        .collect();

    // Strings still referenced.
    let dropped_top = |op: u16| {
        matches!(
            op,
            OP_CHARSET | OP_ENCODING | OP_CHARSTRINGS | OP_PRIVATE | OP_FDARRAY | OP_FDSELECT
        )
    };
    let mut sid_set: BTreeSet<u16> = BTreeSet::new();
    let mut note = |dict: &Dict<'_>, skip: &dyn Fn(u16) -> bool| -> R<()> {
        for e in dict.entries.iter().filter(|e| !skip(e.op)) {
            for o in e.operands.iter().take(sid_operands(e.op)) {
                let v = o.2;
                if v.fract() != 0.0 || !(0.0..65536.0).contains(&v) {
                    return Err(bad("SID operand"));
                }
                if v >= f64::from(STANDARD_STRINGS) {
                    sid_set.insert(v as u16);
                }
            }
        }
        Ok(())
    };
    note(&top, &dropped_top)?;
    for &o in &used_fds {
        if cid_keyed {
            note(&fds[o].dict, &|op| op == OP_PRIVATE)?;
        }
    }
    if !cid_keyed {
        for &g in kept.iter().skip(1) {
            let s = charset[usize::from(g)];
            if s >= STANDARD_STRINGS {
                sid_set.insert(s);
            }
        }
    }
    let mut sids: BTreeMap<u16, u16> = BTreeMap::new();
    let mut new_strings: Vec<Vec<u8>> = Vec::new();
    for &s in &sid_set {
        let text = strings
            .get(usize::from(s - STANDARD_STRINGS))
            .ok_or_else(|| bad("SID outside the String INDEX"))?;
        sids.insert(s, STANDARD_STRINGS + new_strings.len() as u16);
        new_strings.push(text.to_vec());
    }

    // Charset and FDSelect.
    let cids: Vec<u16> = if cid_keyed {
        kept.iter().map(|&g| charset[usize::from(g)]).collect()
    } else {
        (0..k as u16).collect()
    };
    let values: Vec<u16> = kept
        .iter()
        .skip(1)
        .map(|&g| {
            let s = charset[usize::from(g)];
            if cid_keyed {
                s
            } else {
                sids.get(&s).copied().unwrap_or(s)
            }
        })
        .collect();
    let charset_out = charset_bytes(&values);
    let fd_select_out = if cid_keyed {
        let sel: Vec<u8> = kept.iter().map(|g| fd_new[&fd_of[g]] as u8).collect();
        let mut ranges = Vec::new();
        for (i, &fd) in sel.iter().enumerate() {
            if i == 0 || sel[i - 1] != fd {
                ranges.push((i as u16, fd));
            }
        }
        if 5 + 3 * ranges.len() < 1 + k {
            let mut out = vec![3];
            out.extend((ranges.len() as u16).to_be_bytes());
            for (first, fd) in ranges {
                out.extend(first.to_be_bytes());
                out.push(fd);
            }
            out.extend((k as u16).to_be_bytes());
            out
        } else {
            let mut out = vec![0];
            out.extend(sel);
            out
        }
    } else {
        Vec::new()
    };

    // Private blocks.
    let mut privates: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for &o in &used_fds {
        privates.push(private_block(&fds[o], &lused[o], &sids));
    }
    let gsubr_out = write_index(&pruned_subrs(&gsubrs, &gused));
    let charstrings_out = write_index(
        &kept
            .iter()
            .map(|&g| charstrings[usize::from(g)].to_vec())
            .collect::<Vec<_>>(),
    );
    let name = names.first().map_or(b"Subset".to_vec(), |n| n.to_vec());
    let header = vec![1, 0, 4, 4];
    let name_index = write_index(std::slice::from_ref(&name));
    let string_index = write_index(&new_strings);

    // Offsets are fixed width, so every size is known before the offsets are.
    let build_top =
        |charset_off: u32, select_off: u32, array_off: u32, cs_off: u32, priv_off: u32| {
            let mut d = Vec::new();
            if cid_keyed && let Some(ros) = top.get(OP_ROS) {
                top.emit(ros, &mut d, &sids);
            }
            for e in top
                .entries
                .iter()
                .filter(|e| !dropped_top(e.op) && !(cid_keyed && e.op == OP_ROS))
            {
                top.emit(e, &mut d, &sids);
            }
            d.extend(fixed5(charset_off));
            put_op(&mut d, OP_CHARSET);
            if cid_keyed {
                d.extend(fixed5(select_off));
                put_op(&mut d, OP_FDSELECT);
                d.extend(fixed5(array_off));
                put_op(&mut d, OP_FDARRAY);
            }
            d.extend(fixed5(cs_off));
            put_op(&mut d, OP_CHARSTRINGS);
            if !cid_keyed {
                d.extend(fixed5(privates[0].0.len() as u32));
                d.extend(fixed5(priv_off));
                put_op(&mut d, OP_PRIVATE);
            }
            d
        };
    let build_fds = |priv_start: usize| -> Vec<Vec<u8>> {
        let mut pos = priv_start;
        let mut out = Vec::new();
        for (n, &o) in used_fds.iter().enumerate() {
            let fd = &fds[o];
            let mut d = Vec::new();
            for e in fd.dict.entries.iter().filter(|e| e.op != OP_PRIVATE) {
                fd.dict.emit(e, &mut d, &sids);
            }
            d.extend(fixed5(privates[n].0.len() as u32));
            d.extend(fixed5(pos as u32));
            put_op(&mut d, OP_PRIVATE);
            pos += privates[n].0.len() + privates[n].1.len();
            out.push(d);
        }
        out
    };
    let top_len = write_index(&[build_top(0, 0, 0, 0, 0)]).len();
    let mut pos = header.len() + name_index.len() + top_len + string_index.len() + gsubr_out.len();
    let charset_off = pos;
    pos += charset_out.len();
    let select_off = pos;
    pos += fd_select_out.len();
    let cs_off = pos;
    pos += charstrings_out.len();
    let array_off = pos;
    let fd_array_out = if cid_keyed {
        write_index(&build_fds(0))
    } else {
        Vec::new()
    };
    pos += fd_array_out.len();
    let priv_off = pos;

    let mut out = header;
    out.extend(name_index);
    out.extend(write_index(&[build_top(
        charset_off as u32,
        select_off as u32,
        array_off as u32,
        cs_off as u32,
        priv_off as u32,
    )]));
    out.extend(string_index);
    out.extend(gsubr_out);
    out.extend(&charset_out);
    out.extend(&fd_select_out);
    out.extend(charstrings_out);
    if cid_keyed {
        out.extend(write_index(&build_fds(priv_off)));
    }
    for (d, s) in &privates {
        out.extend(d);
        out.extend(s);
    }
    if u32::try_from(out.len()).is_err() {
        return Err(SubsetError::LimitExceeded("font size"));
    }

    let check = CffFont::parse(&out)?;
    if check.glyph_count() != k || check.is_cid() != cid_keyed {
        return Err(SubsetError::InvalidData("internal CFF layout error"));
    }

    let clamp = |v: f64| v.round().clamp(-32768.0, 32767.0) as i16;
    let bbox = top
        .nums(5)
        .filter(|v| v.len() == 4)
        .map(|v| [clamp(v[0]), clamp(v[1]), clamp(v[2]), clamp(v[3])]);
    Ok(CffSubset {
        data: out,
        kept,
        cids,
        cid_keyed,
        name,
        bbox,
        italic_angle: top
            .nums(0x0c02)
            .and_then(|v| v.first().copied())
            .unwrap_or(0.0),
        fixed_pitch: top
            .nums(0x0c01)
            .and_then(|v| v.first().copied())
            .unwrap_or(0.0)
            != 0.0,
    })
}
