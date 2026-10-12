// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded CFF (version 1) parser and Type 2 charstring interpreter.
//!
//! Outlines are returned as cubic [`Path`]s in font units with Y pointing up,
//! with the same error type and budgets as the TrueType decoder. Hint
//! operators are counted so that `hintmask`/`cntrmask` bytes can be skipped,
//! but hinting is never applied. CFF2 and Type 1 charstrings are rejected with
//! [`OutlineError::Unsupported`].
//!
//! Errors reuse [`OutlineError`], whose `Display` text is format neutral.
//! Supported beyond the basic operators: `FontMatrix` (Top DICT and FD DICT),
//! the standard strings and the predefined charsets, and the deprecated
//! four-argument `endchar` (`seac`) accent composition.

use crate::cff_tables::{
    EXPERT_CHARSET, EXPERT_SUBSET_CHARSET, STANDARD_ENCODING, STANDARD_STRINGS,
};
use crate::outline::{OutlineError, OutlineLimits};
use atelier_geom::{Path, Point};

/// Maximum Type 2 argument stack depth.
pub const MAX_STACK: usize = 48;
/// Maximum subroutine call depth.
pub const MAX_SUBR_DEPTH: usize = 10;
/// Maximum number of numbers and operators executed for one glyph.
pub const MAX_OPERATIONS: usize = 1_000_000;
const MAX_STEMS: usize = 96;
const MAX_FONT_DICTS: usize = 256;
const MAX_DICT_ENTRIES: usize = 512;
const MAX_REAL_CHARS: usize = 64;
const PUBLIC_DEPTH: usize = 64;
const STANDARD_COUNT: usize = 391;
/// Maximum nesting of `seac` compositions: the glyph itself plus one component level.
pub const MAX_SEAC_DEPTH: usize = 2;
/// Value pushed by the Type 2 `random` operator (the specification asks for a number in ]0, 1]).
pub const RANDOM_VALUE: f64 = 0.5;
const DEFAULT_MATRIX: [f64; 6] = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];
const DEFAULT_UNITS_PER_EM: u16 = 1000;
/// Relative tolerance when matching a scaled FontMatrix entry to an integer.
const MATRIX_SNAP: f64 = 1.0e-5;
const TRANSIENT_SLOTS: usize = 32;

type Res<T> = Result<T, OutlineError>;

#[derive(Clone, Copy)]
struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn at(b: &'a [u8], p: usize) -> Self {
        Self { b, p }
    }
    fn u8(&mut self) -> Res<u8> {
        let v = *self.b.get(self.p).ok_or(OutlineError::Truncated)?;
        self.p += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Res<u16> {
        let a = self.u8()?;
        let b = self.u8()?;
        Ok(u16::from_be_bytes([a, b]))
    }
    fn i16(&mut self) -> Res<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> Res<i32> {
        let a = self.u16()?;
        let b = self.u16()?;
        Ok(((u32::from(a) << 16) | u32::from(b)) as i32)
    }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let e = self.p.checked_add(n).ok_or(OutlineError::Truncated)?;
        let s = self.b.get(self.p..e).ok_or(OutlineError::Truncated)?;
        self.p = e;
        Ok(s)
    }
}

/// A CFF INDEX. Offsets are validated lazily, entry by entry.
#[derive(Clone, Copy, Debug)]
struct Index<'a> {
    count: usize,
    off_size: usize,
    offsets: &'a [u8],
    data: &'a [u8],
}
impl<'a> Index<'a> {
    fn empty() -> Self {
        Self {
            count: 0,
            off_size: 1,
            offsets: &[],
            data: &[],
        }
    }
    fn parse(buf: &'a [u8], pos: usize) -> Res<(Self, usize)> {
        let mut r = Rd::at(buf, pos);
        let count = usize::from(r.u16()?);
        if count == 0 {
            return Ok((Self::empty(), r.p));
        }
        let off_size = usize::from(r.u8()?);
        if !(1..=4).contains(&off_size) {
            return Err(OutlineError::InvalidData("INDEX offSize"));
        }
        let offsets = r.take((count + 1) * off_size)?;
        let mut index = Self {
            count,
            off_size,
            offsets,
            data: &[],
        };
        let first = index.offset(0)?;
        let last = index.offset(count)?;
        if first != 1 || last < first {
            return Err(OutlineError::InvalidData("INDEX offsets"));
        }
        index.data = r.take(last - 1)?;
        Ok((index, r.p))
    }
    fn offset(&self, i: usize) -> Res<usize> {
        let start = i
            .checked_mul(self.off_size)
            .ok_or(OutlineError::InvalidData("INDEX offset"))?;
        let s = self
            .offsets
            .get(start..start + self.off_size)
            .ok_or(OutlineError::InvalidData("INDEX offset"))?;
        Ok(s.iter().fold(0usize, |a, &b| (a << 8) | usize::from(b)))
    }
    fn get(&self, i: usize) -> Res<&'a [u8]> {
        if i >= self.count {
            return Err(OutlineError::InvalidData("INDEX entry"));
        }
        let a = self.offset(i)?;
        let b = self.offset(i + 1)?;
        if a < 1 || b < a {
            return Err(OutlineError::InvalidData("INDEX offsets"));
        }
        self.data
            .get(a - 1..b - 1)
            .ok_or(OutlineError::InvalidData("INDEX offsets"))
    }
}

fn to_index(v: f64) -> Res<usize> {
    if v.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&v) {
        Ok(v as usize)
    } else {
        Err(OutlineError::InvalidData("DICT offset operand"))
    }
}

struct Dict {
    entries: Vec<(u16, Vec<f64>)>,
}
impl Dict {
    fn get(&self, op: u16) -> Option<&[f64]> {
        self.entries
            .iter()
            .rev()
            .find(|(o, _)| *o == op)
            .map(|(_, v)| v.as_slice())
    }
    fn number(&self, op: u16) -> Res<Option<f64>> {
        match self.get(op) {
            None => Ok(None),
            Some([v]) => Ok(Some(*v)),
            Some(_) => Err(OutlineError::InvalidData("DICT operand count")),
        }
    }
}

const OP_CHARSET: u16 = 15;
const OP_CHARSTRINGS: u16 = 17;
const OP_PRIVATE: u16 = 18;
const OP_SUBRS: u16 = 19;
const OP_DEFAULT_WIDTH: u16 = 20;
const OP_NOMINAL_WIDTH: u16 = 21;
const OP_CHARSTRING_TYPE: u16 = 0x0c06;
const OP_FONT_MATRIX: u16 = 0x0c07;
const OP_ROS: u16 = 0x0c1e;
const OP_FDARRAY: u16 = 0x0c24;
const OP_FDSELECT: u16 = 0x0c25;

fn parse_real(r: &mut Rd<'_>) -> Res<f64> {
    let mut s = String::new();
    loop {
        let b = r.u8()?;
        for n in [b >> 4, b & 15] {
            match n {
                0..=9 => s.push(char::from(b'0' + n)),
                10 => s.push('.'),
                11 => s.push('E'),
                12 => s.push_str("E-"),
                14 => s.push('-'),
                15 => {
                    let v: f64 = s
                        .parse()
                        .map_err(|_| OutlineError::InvalidData("DICT real number"))?;
                    return if v.is_finite() {
                        Ok(v)
                    } else {
                        Err(OutlineError::InvalidData("DICT real number"))
                    };
                }
                _ => return Err(OutlineError::InvalidData("DICT real nibble")),
            }
            if s.len() > MAX_REAL_CHARS {
                return Err(OutlineError::LimitExceeded("DICT real length"));
            }
        }
    }
}

fn parse_dict(b: &[u8]) -> Res<Dict> {
    let mut r = Rd::at(b, 0);
    let mut operands: Vec<f64> = Vec::new();
    let mut entries = Vec::new();
    while r.p < b.len() {
        let b0 = r.u8()?;
        match b0 {
            0..=21 => {
                let op = if b0 == 12 {
                    0x0c00 | u16::from(r.u8()?)
                } else {
                    u16::from(b0)
                };
                if entries.len() >= MAX_DICT_ENTRIES {
                    return Err(OutlineError::LimitExceeded("DICT entries"));
                }
                entries.push((op, std::mem::take(&mut operands)));
            }
            28 => operands.push(f64::from(r.i16()?)),
            29 => operands.push(f64::from(r.i32()?)),
            30 => operands.push(parse_real(&mut r)?),
            32..=246 => operands.push(f64::from(i32::from(b0) - 139)),
            247..=250 => {
                let b1 = i32::from(r.u8()?);
                operands.push(f64::from((i32::from(b0) - 247) * 256 + b1 + 108));
            }
            251..=254 => {
                let b1 = i32::from(r.u8()?);
                operands.push(f64::from(-(i32::from(b0) - 251) * 256 - b1 - 108));
            }
            _ => return Err(OutlineError::InvalidData("reserved DICT byte")),
        }
        if operands.len() > MAX_STACK {
            return Err(OutlineError::LimitExceeded("DICT operands"));
        }
    }
    if !operands.is_empty() {
        return Err(OutlineError::InvalidData("DICT operands without operator"));
    }
    Ok(Dict { entries })
}

/// FontMatrix of a DICT, if present: six finite numbers forming an invertible matrix.
fn font_matrix(dict: &Dict) -> Res<Option<[f64; 6]>> {
    let Some(m) = dict.get(OP_FONT_MATRIX) else {
        return Ok(None);
    };
    let m: [f64; 6] = m
        .try_into()
        .map_err(|_| OutlineError::InvalidData("FontMatrix operand count"))?;
    check_matrix(&m)?;
    Ok(Some(m))
}

fn check_matrix(m: &[f64; 6]) -> Res<()> {
    let det = m[0] * m[3] - m[1] * m[2];
    if m.iter().any(|v| !v.is_finite()) || !det.is_finite() || det == 0.0 {
        return Err(OutlineError::InvalidData("degenerate FontMatrix"));
    }
    Ok(())
}

/// Matrix applying to a Font DICT: the Top DICT one, the FD DICT one, or the FD matrix
/// concatenated with the Top DICT matrix (FD applied first) when both are present.
fn effective_matrix(top: Option<[f64; 6]>, fd: Option<[f64; 6]>) -> Res<[f64; 6]> {
    let m = match (top, fd) {
        (None, None) => DEFAULT_MATRIX,
        (Some(m), None) | (None, Some(m)) => m,
        (Some(t), Some(f)) => [
            t[0] * f[0] + t[2] * f[1],
            t[1] * f[0] + t[3] * f[1],
            t[0] * f[2] + t[2] * f[3],
            t[1] * f[2] + t[3] * f[3],
            t[0] * f[4] + t[2] * f[5] + t[4],
            t[1] * f[4] + t[3] * f[5] + t[5],
        ],
    };
    check_matrix(&m)?;
    Ok(m)
}

/// Transform from charstring units to the caller's font units (`units_per_em`);
/// `None` for the default matrix and for a matrix that already equals
/// `1 / units_per_em`, which leave coordinates untouched.
///
/// FontMatrix reals are stored with a handful of digits (`0.000833333` for a
/// 1200-unit em), so a linear entry within [`MATRIX_SNAP`] of an integer after
/// scaling is taken to be that integer instead of distorting every outline.
fn glyph_transform(m: &[f64; 6], units_per_em: u16) -> Res<Option<[f64; 6]>> {
    if *m == DEFAULT_MATRIX {
        return Ok(None);
    }
    let u = f64::from(units_per_em);
    let mut t = m.map(|v| v * u);
    if t.iter().any(|v| !v.is_finite()) {
        return Err(OutlineError::InvalidData("degenerate FontMatrix"));
    }
    for v in &mut t[..4] {
        let r = v.round();
        if (*v - r).abs() <= MATRIX_SNAP * r.abs().max(1.0) {
            *v = r;
        }
    }
    Ok((t != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]).then_some(t))
}

#[derive(Clone, Copy, Debug)]
struct Private<'a> {
    default_width: f64,
    nominal_width: f64,
    subrs: Index<'a>,
    /// Effective FontMatrix (Top DICT and FD DICT combined).
    matrix: [f64; 6],
}
impl<'a> Private<'a> {
    fn none() -> Self {
        Self {
            default_width: 0.0,
            nominal_width: 0.0,
            subrs: Index::empty(),
            matrix: DEFAULT_MATRIX,
        }
    }
    fn parse(cff: &'a [u8], size: usize, offset: usize) -> Res<Self> {
        let end = offset
            .checked_add(size)
            .ok_or(OutlineError::InvalidData("Private DICT range"))?;
        let data = cff
            .get(offset..end)
            .ok_or(OutlineError::InvalidData("Private DICT range"))?;
        let d = parse_dict(data)?;
        let subrs = match d.number(OP_SUBRS)? {
            Some(v) => {
                let pos = offset
                    .checked_add(to_index(v)?)
                    .ok_or(OutlineError::InvalidData("Subrs offset"))?;
                Index::parse(cff, pos)?.0
            }
            None => Index::empty(),
        };
        Ok(Self {
            default_width: d.number(OP_DEFAULT_WIDTH)?.unwrap_or(0.0),
            nominal_width: d.number(OP_NOMINAL_WIDTH)?.unwrap_or(0.0),
            subrs,
            matrix: DEFAULT_MATRIX,
        })
    }
    fn from_dict(cff: &'a [u8], d: &Dict) -> Res<Self> {
        match d.get(OP_PRIVATE) {
            None => Ok(Self::none()),
            Some([size, offset]) => Self::parse(cff, to_index(*size)?, to_index(*offset)?),
            Some(_) => Err(OutlineError::InvalidData("Private operand count")),
        }
    }
}

#[derive(Clone, Debug)]
enum Kind<'a> {
    Simple(Private<'a>),
    Cid {
        fonts: Vec<Private<'a>>,
        fd_select: &'a [u8],
        ordering: [f64; 3],
    },
}

/// A decoded glyph: outline plus advance width from the charstring.
#[derive(Clone, Debug, PartialEq)]
pub struct CffGlyph {
    pub path: Path,
    /// Advance width in font units (`nominalWidthX` + first operand, or `defaultWidthX`).
    pub width: f64,
}

/// A validated CFF version 1 table.
#[derive(Clone, Debug)]
pub struct CffFont<'a> {
    data: &'a [u8],
    glyph_subrs: Index<'a>,
    strings: Index<'a>,
    charstrings: Index<'a>,
    charset_offset: usize,
    kind: Kind<'a>,
}

impl<'a> CffFont<'a> {
    /// Parse the header, INDEXes, Top DICT, Private DICTs and CID structures.
    pub fn parse(data: &'a [u8]) -> Res<Self> {
        let mut r = Rd::at(data, 0);
        let major = r.u8()?;
        let _minor = r.u8()?;
        let hdr_size = usize::from(r.u8()?);
        let off_size = r.u8()?;
        match major {
            1 => {}
            2 => return Err(OutlineError::Unsupported("CFF2 variable outlines")),
            _ => return Err(OutlineError::InvalidData("CFF major version")),
        }
        if hdr_size < 4 || !(1..=4).contains(&off_size) {
            return Err(OutlineError::InvalidData("CFF header"));
        }
        let (_names, pos) = Index::parse(data, hdr_size)?;
        let (tops, pos) = Index::parse(data, pos)?;
        let (strings, pos) = Index::parse(data, pos)?;
        let (glyph_subrs, _) = Index::parse(data, pos)?;
        if tops.count == 0 {
            return Err(OutlineError::InvalidData("empty Top DICT INDEX"));
        }
        let top = parse_dict(tops.get(0)?)?;
        let top_matrix = font_matrix(&top)?;
        if let Some(t) = top.number(OP_CHARSTRING_TYPE)?
            && t != 2.0
        {
            return Err(OutlineError::Unsupported("Type 1 charstrings"));
        }
        let cs_offset = top
            .number(OP_CHARSTRINGS)?
            .ok_or(OutlineError::InvalidData("missing CharStrings"))?;
        let (charstrings, _) = Index::parse(data, to_index(cs_offset)?)?;
        if charstrings.count == 0 {
            return Err(OutlineError::InvalidData("no glyphs"));
        }
        let charset_offset = match top.number(OP_CHARSET)? {
            Some(v) => to_index(v)?,
            None => 0,
        };
        let kind = if let Some(ros) = top.get(OP_ROS) {
            if ros.len() != 3 {
                return Err(OutlineError::InvalidData("ROS operand count"));
            }
            let fd_array = top
                .number(OP_FDARRAY)?
                .ok_or(OutlineError::InvalidData("CID font without FDArray"))?;
            let fd_select = top
                .number(OP_FDSELECT)?
                .ok_or(OutlineError::InvalidData("CID font without FDSelect"))?;
            if charset_offset <= 2 {
                return Err(OutlineError::InvalidData(
                    "CID font with predefined charset",
                ));
            }
            let (fds, _) = Index::parse(data, to_index(fd_array)?)?;
            if fds.count == 0 || fds.count > MAX_FONT_DICTS {
                return Err(OutlineError::InvalidData("FDArray size"));
            }
            let mut fonts = Vec::with_capacity(fds.count);
            for i in 0..fds.count {
                let d = parse_dict(fds.get(i)?)?;
                let mut private = Private::from_dict(data, &d)?;
                private.matrix = effective_matrix(top_matrix, font_matrix(&d)?)?;
                fonts.push(private);
            }
            let fd_select =
                parse_fd_select(data, to_index(fd_select)?, charstrings.count, fonts.len())?;
            Kind::Cid {
                fonts,
                fd_select,
                ordering: [ros[0], ros[1], ros[2]],
            }
        } else {
            let mut private = Private::from_dict(data, &top)?;
            private.matrix = effective_matrix(top_matrix, None)?;
            Kind::Simple(private)
        };
        Ok(Self {
            data,
            glyph_subrs,
            strings,
            charstrings,
            charset_offset,
            kind,
        })
    }

    /// Number of glyphs in the CharStrings INDEX.
    pub fn glyph_count(&self) -> usize {
        self.charstrings.count
    }

    /// Whether the font is CID-keyed (has a ROS operator).
    pub fn is_cid(&self) -> bool {
        matches!(self.kind, Kind::Cid { .. })
    }

    /// Number of Font DICTs (1 for a name-keyed font).
    pub fn font_dict_count(&self) -> usize {
        match &self.kind {
            Kind::Simple(_) => 1,
            Kind::Cid { fonts, .. } => fonts.len(),
        }
    }

    /// Registry, Ordering SIDs and Supplement of a CID-keyed font.
    pub fn registry_ordering_supplement(&self) -> Option<[f64; 3]> {
        match &self.kind {
            Kind::Simple(_) => None,
            Kind::Cid { ordering, .. } => Some(*ordering),
        }
    }

    /// Font DICT index selected by FDSelect for a glyph (0 for name-keyed fonts).
    pub fn font_dict_index(&self, gid: u16) -> Res<usize> {
        let gid = usize::from(gid);
        if gid >= self.charstrings.count {
            return Err(OutlineError::InvalidGlyph);
        }
        match &self.kind {
            Kind::Simple(_) => Ok(0),
            Kind::Cid {
                fd_select, fonts, ..
            } => {
                let fd = fd_select_lookup(fd_select, gid)?;
                if fd >= fonts.len() {
                    return Err(OutlineError::InvalidData("FDSelect font index"));
                }
                Ok(fd)
            }
        }
    }

    /// Custom string (SID >= 391) from the String INDEX. Standard strings are
    /// not returned here; [`CffFont::sid_name`] resolves both kinds.
    pub fn string(&self, sid: u16) -> Option<&'a [u8]> {
        let i = usize::from(sid).checked_sub(STANDARD_COUNT)?;
        self.strings.get(i).ok()
    }

    /// Name for any SID: one of the 391 standard strings, or an entry of the String INDEX.
    pub fn sid_name(&self, sid: u16) -> Option<&'a [u8]> {
        match standard_string(sid) {
            Some(name) => Some(name.as_bytes()),
            None => self.string(sid),
        }
    }

    /// Glyph name of a name-keyed font (`None` for CID-keyed fonts and unresolved SIDs).
    pub fn glyph_name(&self, gid: u16) -> Res<Option<&'a [u8]>> {
        if self.is_cid() {
            if usize::from(gid) >= self.charstrings.count {
                return Err(OutlineError::InvalidGlyph);
            }
            return Ok(None);
        }
        Ok(self.sid_name(self.glyph_id_in_charset(gid)?))
    }

    /// First glyph whose charset entry equals `sid` (name-keyed fonts; glyph 0 is `.notdef`).
    pub fn glyph_for_sid(&self, sid: u16) -> Res<Option<u16>> {
        if sid == 0 {
            return Ok(Some(0));
        }
        let mut found = None;
        self.charset_walk(|g, id| {
            if id == sid {
                found = Some(g as u16);
                false
            } else {
                true
            }
        })?;
        Ok(found)
    }

    /// Decode the whole charset: SID (or CID) for each glyph, glyph 0 being `.notdef`.
    pub fn charset(&self) -> Res<Vec<u16>> {
        let mut out = Vec::with_capacity(self.charstrings.count);
        out.push(0);
        self.charset_walk(|_, id| {
            out.push(id);
            true
        })?;
        Ok(out)
    }

    /// SID (or CID) of one glyph. Predefined ISOAdobe, Expert and ExpertSubset
    /// charsets are resolved from their tables.
    pub fn glyph_id_in_charset(&self, gid: u16) -> Res<u16> {
        let gid = usize::from(gid);
        if gid >= self.charstrings.count {
            return Err(OutlineError::InvalidGlyph);
        }
        if gid == 0 {
            return Ok(0);
        }
        let mut found = None;
        self.charset_walk(|g, id| {
            if g == gid {
                found = Some(id);
                false
            } else {
                true
            }
        })?;
        found.ok_or(OutlineError::InvalidData("charset"))
    }

    fn charset_walk(&self, mut f: impl FnMut(usize, u16) -> bool) -> Res<()> {
        let n = self.charstrings.count;
        match self.charset_offset {
            0 => {
                if n > 229 {
                    return Err(OutlineError::InvalidData("ISOAdobe charset too short"));
                }
                for g in 1..n {
                    if !f(g, g as u16) {
                        break;
                    }
                }
                Ok(())
            }
            off @ (1 | 2) => {
                let table: &[u16] = if off == 1 {
                    &EXPERT_CHARSET
                } else {
                    &EXPERT_SUBSET_CHARSET
                };
                if n > table.len() {
                    return Err(OutlineError::InvalidData(
                        "predefined charset shorter than the glyph count",
                    ));
                }
                for (g, &sid) in table.iter().enumerate().take(n).skip(1) {
                    if !f(g, sid) {
                        break;
                    }
                }
                Ok(())
            }
            off => {
                let mut r = Rd::at(self.data, off);
                let format = r.u8()?;
                let mut g = 1usize;
                match format {
                    0 => {
                        while g < n {
                            if !f(g, r.u16()?) {
                                return Ok(());
                            }
                            g += 1;
                        }
                    }
                    1 | 2 => {
                        while g < n {
                            let first = usize::from(r.u16()?);
                            let left = if format == 1 {
                                usize::from(r.u8()?)
                            } else {
                                usize::from(r.u16()?)
                            };
                            if first + left > 0xffff || g + left > n - 1 {
                                return Err(OutlineError::InvalidData("charset range"));
                            }
                            for k in 0..=left {
                                if !f(g, (first + k) as u16) {
                                    return Ok(());
                                }
                                g += 1;
                            }
                        }
                    }
                    _ => return Err(OutlineError::InvalidData("charset format")),
                }
                Ok(())
            }
        }
    }

    /// Decode a glyph outline with the shared [`OutlineLimits`].
    ///
    /// `max_depth` is capped to [`MAX_SUBR_DEPTH`], `max_components` bounds the
    /// total number of subroutine calls, `max_points` and `max_contours` apply as for TrueType.
    /// A non-default FontMatrix is applied for a 1000 units-per-em caller; use
    /// [`CffFont::outline_with_units`] to target another em size.
    pub fn outline(&self, gid: u16, limits: OutlineLimits) -> Res<Path> {
        Ok(self.glyph(gid, limits)?.path)
    }

    /// Like [`CffFont::outline`], with coordinates expressed in `units_per_em` units.
    pub fn outline_with_units(
        &self,
        gid: u16,
        limits: OutlineLimits,
        units_per_em: u16,
    ) -> Res<Path> {
        Ok(self.glyph_with_units(gid, limits, units_per_em)?.path)
    }

    /// Decode a glyph outline and its width.
    pub fn glyph(&self, gid: u16, limits: OutlineLimits) -> Res<CffGlyph> {
        self.glyph_with_units(gid, limits, DEFAULT_UNITS_PER_EM)
    }

    /// Decode a glyph outline and its width in `units_per_em` units.
    ///
    /// The effective FontMatrix (Top DICT, FD DICT, or both concatenated) maps
    /// charstring units to em fractions; coordinates and width are multiplied by
    /// `matrix * units_per_em`. The default matrix leaves them untouched, whatever
    /// `units_per_em` is. The width is scaled by the matrix horizontal factor.
    pub fn glyph_with_units(
        &self,
        gid: u16,
        limits: OutlineLimits,
        units_per_em: u16,
    ) -> Res<CffGlyph> {
        if limits.max_depth > PUBLIC_DEPTH {
            return Err(OutlineError::LimitExceeded("public depth cap"));
        }
        let index = usize::from(gid);
        if index >= self.charstrings.count {
            return Err(OutlineError::InvalidGlyph);
        }
        let code = self.charstrings.get(index)?;
        let private = match &self.kind {
            Kind::Simple(p) => p,
            Kind::Cid { fonts, .. } => &fonts[self.font_dict_index(gid)?],
        };
        let tf = glyph_transform(&private.matrix, units_per_em)?;
        let mut it = Interp {
            font: self,
            global: &self.glyph_subrs,
            local: &private.subrs,
            global_bias: bias(self.glyph_subrs.count),
            local_bias: bias(private.subrs.count),
            nominal: private.nominal_width,
            limits,
            max_depth: limits.max_depth.min(MAX_SUBR_DEPTH),
            tf,
            seac_depth: 0,
            stack: Vec::with_capacity(MAX_STACK),
            transient: [0.0; TRANSIENT_SLOTS],
            x: 0.0,
            y: 0.0,
            path: Path::new(),
            pending: None,
            open: false,
            stems: 0,
            width: None,
            width_done: false,
            ops: 0,
            calls: 0,
            points: 0,
            contours: 0,
        };
        it.exec(code)?;
        let mut width = it.width.unwrap_or(private.default_width);
        if let Some(t) = tf {
            width *= t[0];
            if !width.is_finite() {
                return Err(OutlineError::Geometry);
            }
        }
        Ok(CffGlyph {
            path: it.path,
            width,
        })
    }
}

/// One of the 391 standard strings of Technical Note 5176, appendix A.
pub fn standard_string(sid: u16) -> Option<&'static str> {
    STANDARD_STRINGS.get(usize::from(sid)).copied()
}

fn parse_fd_select(data: &[u8], off: usize, glyphs: usize, fonts: usize) -> Res<&[u8]> {
    let mut r = Rd::at(data, off);
    let format = r.u8()?;
    let body_start = r.p;
    match format {
        0 => {
            r.take(glyphs)?;
            let body = &data[body_start..r.p];
            if body.iter().any(|&fd| usize::from(fd) >= fonts) {
                return Err(OutlineError::InvalidData("FDSelect font index"));
            }
            Ok(&data[off..r.p])
        }
        3 => {
            let n = usize::from(r.u16()?);
            let mut prev_first: Option<usize> = None;
            for i in 0..n {
                let first = usize::from(r.u16()?);
                let fd = usize::from(r.u8()?);
                if (i == 0 && first != 0) || prev_first.is_some_and(|p| first <= p) || fd >= fonts {
                    return Err(OutlineError::InvalidData("FDSelect ranges"));
                }
                prev_first = Some(first);
            }
            let sentinel = usize::from(r.u16()?);
            if n == 0 || sentinel != glyphs {
                return Err(OutlineError::InvalidData("FDSelect sentinel"));
            }
            if prev_first.is_some_and(|p| p >= glyphs) {
                return Err(OutlineError::InvalidData("FDSelect ranges"));
            }
            Ok(&data[off..r.p])
        }
        _ => Err(OutlineError::InvalidData("FDSelect format")),
    }
}

fn fd_select_lookup(sel: &[u8], gid: usize) -> Res<usize> {
    let mut r = Rd::at(sel, 0);
    match r.u8()? {
        0 => Ok(usize::from(
            *sel.get(1 + gid).ok_or(OutlineError::Truncated)?,
        )),
        3 => {
            let n = usize::from(r.u16()?);
            let mut found = None;
            for _ in 0..n {
                let first = usize::from(r.u16()?);
                let fd = usize::from(r.u8()?);
                if first > gid {
                    break;
                }
                found = Some(fd);
            }
            found.ok_or(OutlineError::InvalidData("FDSelect ranges"))
        }
        _ => Err(OutlineError::InvalidData("FDSelect format")),
    }
}

fn bias(count: usize) -> i64 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Flow {
    Return,
    End,
}

struct Interp<'a, 'f> {
    font: &'f CffFont<'a>,
    global: &'f Index<'a>,
    local: &'f Index<'a>,
    global_bias: i64,
    local_bias: i64,
    nominal: f64,
    limits: OutlineLimits,
    max_depth: usize,
    tf: Option<[f64; 6]>,
    seac_depth: usize,
    stack: Vec<f64>,
    transient: [f64; TRANSIENT_SLOTS],
    x: f64,
    y: f64,
    path: Path,
    pending: Option<Point>,
    open: bool,
    stems: usize,
    width: Option<f64>,
    width_done: bool,
    ops: usize,
    calls: usize,
    points: usize,
    contours: usize,
}

fn pt(x: f64, y: f64) -> Res<Point> {
    Point::try_new(x, y).map_err(|_| OutlineError::Geometry)
}

impl<'a> Interp<'a, '_> {
    fn tick(&mut self) -> Res<()> {
        self.ops += 1;
        if self.ops > MAX_OPERATIONS {
            return Err(OutlineError::LimitExceeded("operations"));
        }
        Ok(())
    }
    fn push(&mut self, v: f64) -> Res<()> {
        if !v.is_finite() {
            return Err(OutlineError::Geometry);
        }
        if self.stack.len() >= MAX_STACK {
            return Err(OutlineError::LimitExceeded("argument stack"));
        }
        self.stack.push(v);
        Ok(())
    }
    fn pop(&mut self) -> Res<f64> {
        self.stack
            .pop()
            .ok_or(OutlineError::InvalidData("argument stack underflow"))
    }
    fn pop_int(&mut self) -> Res<i64> {
        let v = self.pop()?;
        if v.abs() > 1.0e9 {
            return Err(OutlineError::InvalidData("integer operand out of range"));
        }
        Ok(v.trunc() as i64)
    }
    /// Map a point of the charstring space into the caller's font units.
    fn out(&self, x: f64, y: f64) -> Res<Point> {
        match self.tf {
            None => pt(x, y),
            Some(t) => pt(t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]),
        }
    }
    /// Run one top-level charstring (an empty one is an empty glyph).
    fn exec(&mut self, code: &'a [u8]) -> Res<()> {
        if code.is_empty() {
            return Ok(());
        }
        match self.run(code, 0)? {
            Flow::End => Ok(()),
            Flow::Return => Err(OutlineError::InvalidData("charstring without endchar")),
        }
    }
    /// Glyph index of a StandardEncoding code, through its name and the charset.
    fn seac_component(&self, code: f64) -> Res<u16> {
        if code.fract() != 0.0 || !(0.0..256.0).contains(&code) {
            return Err(OutlineError::InvalidData("seac character code"));
        }
        let sid = STANDARD_ENCODING[code as usize];
        if sid == 0 {
            return Err(OutlineError::InvalidData(
                "seac code outside StandardEncoding",
            ));
        }
        self.font
            .glyph_for_sid(sid)?
            .ok_or(OutlineError::InvalidData(
                "seac component missing from charset",
            ))
    }
    /// Deprecated `endchar` form: draw the base glyph, then the accent shifted by (adx, ady).
    fn seac(&mut self, a: [f64; 4]) -> Res<()> {
        let font = self.font;
        let Kind::Simple(private) = &font.kind else {
            return Err(OutlineError::InvalidData("seac in a CID-keyed font"));
        };
        if self.seac_depth >= MAX_SEAC_DEPTH {
            return Err(OutlineError::LimitExceeded("seac depth"));
        }
        let base = self.seac_component(a[2])?;
        let accent = self.seac_component(a[3])?;
        let saved = (
            self.stems,
            self.width,
            self.width_done,
            self.local_bias,
            self.nominal,
        );
        self.seac_depth += 1;
        self.local = &private.subrs;
        self.local_bias = bias(private.subrs.count);
        self.nominal = private.nominal_width;
        for (gid, ox, oy) in [(base, 0.0, 0.0), (accent, a[0], a[1])] {
            self.stack.clear();
            self.stems = 0;
            self.width = None;
            self.width_done = false;
            self.x = ox;
            self.y = oy;
            self.pending = None;
            let code = font.charstrings.get(usize::from(gid))?;
            self.exec(code)?;
        }
        self.seac_depth -= 1;
        (
            self.stems,
            self.width,
            self.width_done,
            self.local_bias,
            self.nominal,
        ) = saved;
        Ok(())
    }
    fn add_points(&mut self, n: usize) -> Res<()> {
        self.points += n;
        if self.points > self.limits.max_points {
            return Err(OutlineError::LimitExceeded("points"));
        }
        Ok(())
    }
    fn close_contour(&mut self) -> Res<()> {
        if self.open {
            self.path.close().map_err(|_| OutlineError::Geometry)?;
            self.open = false;
        }
        self.pending = None;
        Ok(())
    }
    fn move_by(&mut self, dx: f64, dy: f64) -> Res<()> {
        self.close_contour()?;
        self.x += dx;
        self.y += dy;
        self.add_points(1)?;
        self.pending = Some(self.out(self.x, self.y)?);
        Ok(())
    }
    fn begin_draw(&mut self) -> Res<()> {
        if let Some(p) = self.pending.take() {
            self.contours += 1;
            if self.contours > self.limits.max_contours {
                return Err(OutlineError::LimitExceeded("contours"));
            }
            self.path.move_to(p);
            self.open = true;
            Ok(())
        } else if self.open {
            Ok(())
        } else {
            Err(OutlineError::InvalidData("drawing operator before moveto"))
        }
    }
    fn line(&mut self, dx: f64, dy: f64) -> Res<()> {
        self.begin_draw()?;
        self.add_points(1)?;
        self.x += dx;
        self.y += dy;
        self.path
            .line_to(self.out(self.x, self.y)?)
            .map_err(|_| OutlineError::Geometry)
    }
    fn curve(&mut self, d: [f64; 6]) -> Res<()> {
        self.begin_draw()?;
        self.add_points(3)?;
        let (x1, y1) = (self.x + d[0], self.y + d[1]);
        let (x2, y2) = (x1 + d[2], y1 + d[3]);
        let (x3, y3) = (x2 + d[4], y2 + d[5]);
        let (c1, c2, end) = (self.out(x1, y1)?, self.out(x2, y2)?, self.out(x3, y3)?);
        self.x = x3;
        self.y = y3;
        self.path
            .cubic_to(c1, c2, end)
            .map_err(|_| OutlineError::Geometry)
    }
    /// Consume the optional leading width on the first stack-clearing operator.
    fn take_width(&mut self, has_extra: bool) {
        if !self.width_done {
            self.width_done = true;
            if has_extra && !self.stack.is_empty() {
                self.width = Some(self.nominal + self.stack.remove(0));
            }
        }
    }

    fn run(&mut self, code: &'a [u8], depth: usize) -> Res<Flow> {
        let mut r = Rd::at(code, 0);
        while r.p < code.len() {
            let b0 = r.u8()?;
            self.tick()?;
            match b0 {
                32..=246 => self.push(f64::from(i32::from(b0) - 139))?,
                247..=250 => {
                    let b1 = i32::from(r.u8()?);
                    self.push(f64::from((i32::from(b0) - 247) * 256 + b1 + 108))?;
                }
                251..=254 => {
                    let b1 = i32::from(r.u8()?);
                    self.push(f64::from(-(i32::from(b0) - 251) * 256 - b1 - 108))?;
                }
                28 => {
                    let v = r.i16()?;
                    self.push(f64::from(v))?;
                }
                255 => {
                    let v = r.i32()?;
                    self.push(f64::from(v) / 65536.0)?;
                }
                1 | 3 | 18 | 23 => {
                    self.take_width(self.stack.len() % 2 == 1);
                    self.count_stems()?;
                }
                19 | 20 => {
                    self.take_width(self.stack.len() % 2 == 1);
                    self.count_stems()?;
                    r.take(self.stems.div_ceil(8))?;
                }
                10 | 29 => {
                    let (index, bias) = if b0 == 10 {
                        (self.local, self.local_bias)
                    } else {
                        (self.global, self.global_bias)
                    };
                    let n = self.pop_int()? + bias;
                    let n = usize::try_from(n)
                        .map_err(|_| OutlineError::InvalidData("subroutine number"))?;
                    let sub = index.get(n)?;
                    if depth + 1 > self.max_depth {
                        return Err(OutlineError::LimitExceeded("subroutine depth"));
                    }
                    self.calls += 1;
                    if self.calls > self.limits.max_components {
                        return Err(OutlineError::LimitExceeded("components"));
                    }
                    if self.run(sub, depth + 1)? == Flow::End {
                        return Ok(Flow::End);
                    }
                }
                11 => return Ok(Flow::Return),
                14 => {
                    match self.stack.len() {
                        0 | 4 => self.take_width(false),
                        1 | 5 if !self.width_done => self.take_width(true),
                        _ => return Err(OutlineError::InvalidData("endchar arguments")),
                    }
                    self.close_contour()?;
                    if let [adx, ady, bchar, achar] = self.stack[..] {
                        self.seac([adx, ady, bchar, achar])?;
                    }
                    return Ok(Flow::End);
                }
                12 => {
                    let b1 = r.u8()?;
                    self.escape(b1)?;
                }
                4 | 21 | 22 | 5..=8 | 24..=27 | 30 | 31 => {
                    match b0 {
                        21 => self.take_width(self.stack.len() > 2),
                        4 | 22 => self.take_width(self.stack.len() > 1),
                        _ => {}
                    }
                    let args = std::mem::take(&mut self.stack);
                    let res = self.path_op(b0, &args);
                    self.stack = args;
                    self.stack.clear();
                    res?;
                }
                _ => return Err(OutlineError::InvalidData("reserved Type 2 operator")),
            }
        }
        if depth == 0 {
            Ok(Flow::Return)
        } else {
            Err(OutlineError::InvalidData("subroutine without return"))
        }
    }

    fn count_stems(&mut self) -> Res<()> {
        if self.stack.len() % 2 != 0 {
            return Err(OutlineError::InvalidData("stem hint arguments"));
        }
        self.stems += self.stack.len() / 2;
        if self.stems > MAX_STEMS {
            return Err(OutlineError::LimitExceeded("stem hints"));
        }
        self.stack.clear();
        Ok(())
    }

    fn path_op(&mut self, op: u8, s: &[f64]) -> Res<()> {
        let n = s.len();
        let bad = || Err(OutlineError::InvalidData("path operator arguments"));
        match op {
            21 => {
                if s.len() != 2 {
                    return bad();
                }
                self.move_by(s[0], s[1])
            }
            22 | 4 => {
                if s.len() != 1 {
                    return bad();
                }
                if op == 22 {
                    self.move_by(s[0], 0.0)
                } else {
                    self.move_by(0.0, s[0])
                }
            }
            5 => {
                if n < 2 || n % 2 != 0 {
                    return bad();
                }
                for c in s.chunks_exact(2) {
                    self.line(c[0], c[1])?;
                }
                Ok(())
            }
            6 | 7 => {
                if n < 1 {
                    return bad();
                }
                let mut horizontal = op == 6;
                for &d in s {
                    if horizontal {
                        self.line(d, 0.0)?;
                    } else {
                        self.line(0.0, d)?;
                    }
                    horizontal = !horizontal;
                }
                Ok(())
            }
            8 => {
                if n < 6 || n % 6 != 0 {
                    return bad();
                }
                for c in s.chunks_exact(6) {
                    self.curve([c[0], c[1], c[2], c[3], c[4], c[5]])?;
                }
                Ok(())
            }
            24 => {
                if n < 8 || (n - 2) % 6 != 0 {
                    return bad();
                }
                for c in s[..n - 2].chunks_exact(6) {
                    self.curve([c[0], c[1], c[2], c[3], c[4], c[5]])?;
                }
                self.line(s[n - 2], s[n - 1])
            }
            25 => {
                if n < 8 || (n - 6) % 2 != 0 {
                    return bad();
                }
                for c in s[..n - 6].chunks_exact(2) {
                    self.line(c[0], c[1])?;
                }
                let c = &s[n - 6..];
                self.curve([c[0], c[1], c[2], c[3], c[4], c[5]])
            }
            26 | 27 => {
                if n < 4 || n % 4 > 1 {
                    return bad();
                }
                let (mut lead, rest) = if n % 4 == 1 {
                    (s[0], &s[1..])
                } else {
                    (0.0, s)
                };
                for c in rest.chunks_exact(4) {
                    if op == 27 {
                        // hhcurveto: dy1? {dxa dxb dyb dxc}
                        self.curve([c[0], lead, c[1], c[2], c[3], 0.0])?;
                    } else {
                        // vvcurveto: dx1? {dya dxb dyb dyc}
                        self.curve([lead, c[0], c[1], c[2], 0.0, c[3]])?;
                    }
                    lead = 0.0;
                }
                Ok(())
            }
            30 | 31 => {
                if n < 4 || n % 4 > 1 {
                    return bad();
                }
                let mut horizontal = op == 31;
                let mut i = 0;
                while i + 4 <= n {
                    let extra = if n - i == 5 { s[i + 4] } else { 0.0 };
                    let (a, b, c, d) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                    if horizontal {
                        self.curve([a, 0.0, b, c, extra, d])?;
                    } else {
                        self.curve([0.0, a, b, c, d, extra])?;
                    }
                    horizontal = !horizontal;
                    i += 4;
                }
                Ok(())
            }
            _ => Err(OutlineError::InvalidData("reserved Type 2 operator")),
        }
    }

    fn escape(&mut self, op: u8) -> Res<()> {
        let flex = |it: &mut Self, need: usize| -> Res<Vec<f64>> {
            if it.stack.len() != need {
                return Err(OutlineError::InvalidData("flex arguments"));
            }
            Ok(std::mem::take(&mut it.stack))
        };
        match op {
            0 => self.stack.clear(),
            3 | 4 | 15 | 10 | 11 | 12 | 24 => {
                let b = self.pop()?;
                let a = self.pop()?;
                let v = match op {
                    3 => f64::from(u8::from(a != 0.0 && b != 0.0)),
                    4 => f64::from(u8::from(a != 0.0 || b != 0.0)),
                    15 => f64::from(u8::from(a == b)),
                    10 => a + b,
                    11 => a - b,
                    12 => a / b,
                    _ => a * b,
                };
                self.push(v)?;
            }
            5 => {
                let a = self.pop()?;
                self.push(f64::from(u8::from(a == 0.0)))?;
            }
            9 => {
                let a = self.pop()?;
                self.push(a.abs())?;
            }
            14 => {
                let a = self.pop()?;
                self.push(-a)?;
            }
            26 => {
                let a = self.pop()?;
                self.push(a.sqrt())?;
            }
            18 => {
                self.pop()?;
            }
            20 => {
                let i = self.pop_int()?;
                let v = self.pop()?;
                let slot = usize::try_from(i)
                    .ok()
                    .and_then(|i| self.transient.get_mut(i))
                    .ok_or(OutlineError::InvalidData("transient array index"))?;
                *slot = v;
            }
            21 => {
                let i = self.pop_int()?;
                let v = usize::try_from(i)
                    .ok()
                    .and_then(|i| self.transient.get(i))
                    .copied()
                    .ok_or(OutlineError::InvalidData("transient array index"))?;
                self.push(v)?;
            }
            22 => {
                let v2 = self.pop()?;
                let v1 = self.pop()?;
                let s2 = self.pop()?;
                let s1 = self.pop()?;
                self.push(if v1 <= v2 { s1 } else { s2 })?;
            }
            27 => {
                let a = *self
                    .stack
                    .last()
                    .ok_or(OutlineError::InvalidData("argument stack underflow"))?;
                self.push(a)?;
            }
            28 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(b)?;
                self.push(a)?;
            }
            29 => {
                let i = self.pop_int()?.max(0) as usize;
                let len = self.stack.len();
                if i >= len {
                    return Err(OutlineError::InvalidData("index operand"));
                }
                let v = self.stack[len - 1 - i];
                self.push(v)?;
            }
            30 => {
                let j = self.pop_int()?;
                let n = self.pop_int()?;
                let len = self.stack.len();
                let n = usize::try_from(n)
                    .ok()
                    .filter(|&n| n <= len)
                    .ok_or(OutlineError::InvalidData("roll operand"))?;
                if n > 0 {
                    let shift = j.rem_euclid(n as i64) as usize;
                    self.stack[len - n..].rotate_right(shift);
                }
            }
            23 => self.push(RANDOM_VALUE)?,
            34 => {
                let a = flex(self, 7)?;
                let res = self
                    .curve([a[0], 0.0, a[1], a[2], a[3], 0.0])
                    .and_then(|()| self.curve([a[4], 0.0, a[5], -a[2], a[6], 0.0]));
                self.stack = a;
                self.stack.clear();
                res?;
            }
            35 => {
                let a = flex(self, 13)?;
                let res = self
                    .curve([a[0], a[1], a[2], a[3], a[4], a[5]])
                    .and_then(|()| self.curve([a[6], a[7], a[8], a[9], a[10], a[11]]));
                self.stack = a;
                self.stack.clear();
                res?;
            }
            36 => {
                let a = flex(self, 9)?;
                let res = self
                    .curve([a[0], a[1], a[2], a[3], a[4], 0.0])
                    .and_then(|()| {
                        self.curve([a[5], 0.0, a[6], a[7], a[8], -(a[1] + a[3] + a[7])])
                    });
                self.stack = a;
                self.stack.clear();
                res?;
            }
            37 => {
                let a = flex(self, 11)?;
                let dx = a[0] + a[2] + a[4] + a[6] + a[8];
                let dy = a[1] + a[3] + a[5] + a[7] + a[9];
                let res = self
                    .curve([a[0], a[1], a[2], a[3], a[4], a[5]])
                    .and_then(|()| {
                        if dx.abs() > dy.abs() {
                            self.curve([a[6], a[7], a[8], a[9], a[10], -dy])
                        } else {
                            self.curve([a[6], a[7], a[8], a[9], -dx, a[10]])
                        }
                    });
                self.stack = a;
                self.stack.clear();
                res?;
            }
            _ => return Err(OutlineError::InvalidData("reserved Type 2 escape operator")),
        }
        Ok(())
    }
}
