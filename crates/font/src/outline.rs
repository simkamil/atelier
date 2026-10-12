// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded glyph outline decoding: TrueType `loca`/`glyf` here, CFF in [`crate::cff`].
//!
//! Coordinates are returned in font units with Y pointing up. Hinting programs
//! are validated and skipped; they are never interpreted.

use crate::FontFace;
use atelier_geom::{Path, Point};

const DEFAULT_DEPTH: usize = 32;
const DEFAULT_COMPONENTS: usize = 4096;
const DEFAULT_POINTS: usize = 1_000_000;
const DEFAULT_CONTOURS: usize = 100_000;
const PUBLIC_DEPTH: usize = 64;
const MAX_INSTRUCTION_BYTES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutlineLimits {
    pub max_depth: usize,
    pub max_components: usize,
    pub max_points: usize,
    pub max_contours: usize,
}

impl Default for OutlineLimits {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_DEPTH,
            max_components: DEFAULT_COMPONENTS,
            max_points: DEFAULT_POINTS,
            max_contours: DEFAULT_CONTOURS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutlineError {
    MissingTable(&'static str),
    Unsupported(&'static str),
    Truncated,
    InvalidData(&'static str),
    InvalidGlyph,
    Cycle,
    LimitExceeded(&'static str),
    Geometry,
}

impl core::fmt::Display for OutlineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingTable(t) => write!(f, "required font table is missing: {t}"),
            Self::Unsupported(s) => write!(f, "unsupported glyph outline feature: {s}"),
            Self::Truncated => f.write_str("truncated glyph outline data"),
            Self::InvalidData(s) => write!(f, "invalid glyph outline data: {s}"),
            Self::InvalidGlyph => f.write_str("glyph index is outside maxp.numGlyphs"),
            Self::Cycle => f.write_str("cyclic composite glyph reference"),
            Self::LimitExceeded(s) => write!(f, "glyph outline limit exceeded: {s}"),
            Self::Geometry => f.write_str("outline coordinates exceed geometry limits"),
        }
    }
}
impl std::error::Error for OutlineError {}

#[derive(Clone, Copy, Debug)]
struct GlyphPoint {
    x: f64,
    y: f64,
    on: bool,
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Reader<'a> {
    fn new(b: &'a [u8], p: usize) -> Self {
        Self { b, p }
    }
    fn u8(&mut self) -> Result<u8, OutlineError> {
        let v = *self.b.get(self.p).ok_or(OutlineError::Truncated)?;
        self.p += 1;
        Ok(v)
    }
    fn i8(&mut self) -> Result<i8, OutlineError> {
        Ok(self.u8()? as i8)
    }
    fn u16(&mut self) -> Result<u16, OutlineError> {
        let a = self.u8()?;
        let b = self.u8()?;
        Ok(u16::from_be_bytes([a, b]))
    }
    fn i16(&mut self) -> Result<i16, OutlineError> {
        Ok(self.u16()? as i16)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], OutlineError> {
        let e = self.p.checked_add(n).ok_or(OutlineError::Truncated)?;
        let s = self.b.get(self.p..e).ok_or(OutlineError::Truncated)?;
        self.p = e;
        Ok(s)
    }
}

struct Budget {
    lim: OutlineLimits,
    points: usize,
    contours: usize,
    components: usize,
    instructions: usize,
}

struct DecodeContext<'a> {
    offsets: &'a [usize],
    glyf: &'a [u8],
    maxp: &'a [u8],
    stack: Vec<u16>,
    budget: Budget,
}
impl Budget {
    fn add(slot: &mut usize, n: usize, max: usize, what: &'static str) -> Result<(), OutlineError> {
        let next = slot
            .checked_add(n)
            .ok_or(OutlineError::LimitExceeded(what))?;
        if next > max {
            return Err(OutlineError::LimitExceeded(what));
        }
        *slot = next;
        Ok(())
    }
}

impl<'a> FontFace<'a> {
    /// Decode a glyph outline using conservative default resource budgets.
    pub fn outline(&self, glyph_id: u16) -> Result<Path, OutlineError> {
        self.outline_with_limits(glyph_id, OutlineLimits::default())
    }

    /// Decode a glyph outline with cumulative limits shared by nested components.
    pub fn outline_with_limits(
        &self,
        glyph_id: u16,
        limits: OutlineLimits,
    ) -> Result<Path, OutlineError> {
        if limits.max_depth > PUBLIC_DEPTH {
            return Err(OutlineError::LimitExceeded("public depth cap"));
        }
        let glyph_count = usize::from(self.metrics().number_of_glyphs);
        if usize::from(glyph_id) >= glyph_count {
            return Err(OutlineError::InvalidGlyph);
        }
        if self.table(*b"glyf").is_none() {
            if self.table(*b"CFF2").is_some() {
                return Err(OutlineError::Unsupported("CFF2 variable outlines"));
            }
            if let Some(cff) = self.table(*b"CFF ") {
                return crate::cff::CffFont::parse(cff)?.outline_with_units(
                    glyph_id,
                    limits,
                    self.metrics().units_per_em,
                );
            }
        }
        let loca = self
            .table(*b"loca")
            .ok_or(OutlineError::MissingTable("loca"))?;
        let glyf = self
            .table(*b"glyf")
            .ok_or(OutlineError::MissingTable("glyf"))?;
        let maxp = self
            .table(*b"maxp")
            .ok_or(OutlineError::MissingTable("maxp"))?;
        if maxp.len() < 32 || be32(maxp, 0)? != 0x0001_0000 {
            return Err(OutlineError::Unsupported(
                "TrueType outlines require maxp version 1.0",
            ));
        }
        let entry = if self.metrics().index_to_loc_format == 0 {
            2
        } else {
            4
        };
        let need = (glyph_count + 1)
            .checked_mul(entry)
            .ok_or(OutlineError::InvalidData("loca length"))?;
        if loca.len() < need {
            return Err(OutlineError::Truncated);
        }
        let mut offsets = Vec::with_capacity(glyph_count + 1);
        for i in 0..=glyph_count {
            let off = if entry == 2 {
                usize::from(be16(loca, i * 2)?)
                    .checked_mul(2)
                    .ok_or(OutlineError::InvalidData("loca offset"))?
            } else {
                usize::try_from(be32(loca, i * 4)?)
                    .map_err(|_| OutlineError::InvalidData("loca offset"))?
            };
            if off > glyf.len() || offsets.last().is_some_and(|prev| *prev > off) {
                return Err(OutlineError::InvalidData("loca offsets"));
            }
            offsets.push(off);
        }
        let mut context = DecodeContext {
            offsets: &offsets,
            glyf,
            maxp,
            stack: Vec::with_capacity(limits.max_depth.min(PUBLIC_DEPTH)),
            budget: Budget {
                lim: limits,
                points: 0,
                contours: 0,
                components: 0,
                instructions: 0,
            },
        };
        let decoded = self.decode_glyph(glyph_id, 0, &mut context)?;
        points_to_path(&decoded)
    }

    fn decode_glyph(
        &self,
        gid: u16,
        depth: usize,
        context: &mut DecodeContext<'_>,
    ) -> Result<Vec<Vec<GlyphPoint>>, OutlineError> {
        if depth > context.budget.lim.max_depth {
            return Err(OutlineError::LimitExceeded("composite depth"));
        }
        if usize::from(gid) + 1 >= context.offsets.len() {
            return Err(OutlineError::InvalidData("component glyph index"));
        }
        if context.stack.contains(&gid) {
            return Err(OutlineError::Cycle);
        }
        let i = usize::from(gid);
        let start = context.offsets[i];
        let end = context.offsets[i + 1];
        if start == end {
            return Ok(Vec::new());
        }
        let data = context
            .glyf
            .get(start..end)
            .ok_or(OutlineError::Truncated)?;
        if data.len() < 10 {
            return Err(OutlineError::Truncated);
        }
        let n = be16(data, 0)? as i16;
        context.stack.push(gid);
        let result = if n >= 0 {
            self.simple(data, n as usize, context)
        } else {
            self.composite(data, depth, context)
        };
        context.stack.pop();
        result
    }

    fn simple(
        &self,
        data: &[u8],
        contour_count: usize,
        context: &mut DecodeContext<'_>,
    ) -> Result<Vec<Vec<GlyphPoint>>, OutlineError> {
        let maxp = context.maxp;
        let budget = &mut context.budget;
        if contour_count > usize::from(be16(maxp, 8)?) {
            return Err(OutlineError::InvalidData(
                "simple contour count exceeds maxp",
            ));
        }
        Budget::add(
            &mut budget.contours,
            contour_count,
            budget.lim.max_contours,
            "contours",
        )?;
        if contour_count == 0 {
            let ilen = usize::from(be16(data, 10)?);
            Budget::add(
                &mut budget.instructions,
                ilen,
                MAX_INSTRUCTION_BYTES,
                "instruction bytes",
            )?;
            if data.len() < 12usize.checked_add(ilen).ok_or(OutlineError::Truncated)? {
                return Err(OutlineError::Truncated);
            }
            return Ok(Vec::new());
        }
        let mut r = Reader::new(data, 10);
        let mut ends = Vec::with_capacity(contour_count);
        for _ in 0..contour_count {
            ends.push(usize::from(r.u16()?));
        }
        if ends.windows(2).any(|w| w[0] >= w[1]) {
            return Err(OutlineError::InvalidData("simple contour endpoints"));
        }
        let count = ends
            .last()
            .copied()
            .ok_or(OutlineError::InvalidData("empty endpoint list"))?
            .checked_add(1)
            .ok_or(OutlineError::InvalidData("point count"))?;
        if count > usize::from(be16(maxp, 6)?) {
            return Err(OutlineError::InvalidData("simple point count exceeds maxp"));
        }
        Budget::add(&mut budget.points, count, budget.lim.max_points, "points")?;
        let ilen = usize::from(r.u16()?);
        Budget::add(
            &mut budget.instructions,
            ilen,
            MAX_INSTRUCTION_BYTES,
            "instruction bytes",
        )?;
        r.take(ilen)?;
        let mut flags = Vec::with_capacity(count);
        while flags.len() < count {
            let flag = r.u8()?;
            flags.push(flag);
            if flag & 0x08 != 0 {
                let repeat = usize::from(r.u8()?);
                if flags
                    .len()
                    .checked_add(repeat)
                    .filter(|&v| v <= count)
                    .is_none()
                {
                    return Err(OutlineError::InvalidData("flag repeat count"));
                }
                flags.extend(core::iter::repeat_n(flag, repeat));
            }
        }
        let mut xs = Vec::with_capacity(count);
        let mut x = 0i64;
        for flag in &flags {
            let d = if flag & 0x02 != 0 {
                let v = i64::from(r.u8()?);
                if flag & 0x10 != 0 { v } else { -v }
            } else if flag & 0x10 != 0 {
                0
            } else {
                i64::from(r.i16()?)
            };
            x = x
                .checked_add(d)
                .ok_or(OutlineError::InvalidData("x coordinate overflow"))?;
            xs.push(x);
        }
        let mut ys = Vec::with_capacity(count);
        let mut y = 0i64;
        for flag in &flags {
            let d = if flag & 0x04 != 0 {
                let v = i64::from(r.u8()?);
                if flag & 0x20 != 0 { v } else { -v }
            } else if flag & 0x20 != 0 {
                0
            } else {
                i64::from(r.i16()?)
            };
            y = y
                .checked_add(d)
                .ok_or(OutlineError::InvalidData("y coordinate overflow"))?;
            ys.push(y);
        }
        let pts = (0..count)
            .map(|j| GlyphPoint {
                x: xs[j] as f64,
                y: ys[j] as f64,
                on: flags[j] & 1 != 0,
            })
            .collect::<Vec<_>>();
        let mut out = Vec::with_capacity(contour_count);
        let mut first = 0;
        for last in ends {
            out.push(pts[first..=last].to_vec());
            first = last + 1;
        }
        Ok(out)
    }

    fn composite(
        &self,
        data: &[u8],
        depth: usize,
        context: &mut DecodeContext<'_>,
    ) -> Result<Vec<Vec<GlyphPoint>>, OutlineError> {
        let maxp = context.maxp;
        const WORDS: u16 = 1;
        const XY: u16 = 2;
        const ROUND: u16 = 4;
        const SCALE: u16 = 8;
        const MORE: u16 = 0x20;
        const XY_SCALE: u16 = 0x40;
        const MATRIX: u16 = 0x80;
        const INSTRUCTIONS: u16 = 0x100;
        const SCALED: u16 = 0x800;
        const UNSCALED: u16 = 0x1000;
        if depth > usize::from(be16(maxp, 30)?) {
            return Err(OutlineError::InvalidData("composite depth exceeds maxp"));
        }
        let mut r = Reader::new(data, 10);
        let mut all: Vec<Vec<GlyphPoint>> = Vec::new();
        let mut flags;
        let mut have_instr = false;
        let mut local_components = 0usize;
        loop {
            local_components += 1;
            if local_components > usize::from(be16(maxp, 28)?) {
                return Err(OutlineError::InvalidData("component count exceeds maxp"));
            }
            Budget::add(
                &mut context.budget.components,
                1,
                context.budget.lim.max_components,
                "components",
            )?;
            flags = r.u16()?;
            let child = r.u16()?;
            let (a1, a2) = if flags & WORDS != 0 {
                let a = r.i16()?;
                let b = r.i16()?;
                if flags & XY != 0 {
                    (i64::from(a), i64::from(b))
                } else {
                    (i64::from(a as u16), i64::from(b as u16))
                }
            } else {
                let a = r.i8()?;
                let b = r.i8()?;
                if flags & XY != 0 {
                    (i64::from(a), i64::from(b))
                } else {
                    (i64::from(a as u8), i64::from(b as u8))
                }
            };
            if flags & !0x1fff != 0 {
                return Err(OutlineError::InvalidData("reserved component flags"));
            }
            if flags & SCALED != 0 && flags & UNSCALED != 0 {
                return Err(OutlineError::InvalidData(
                    "contradictory component offset flags",
                ));
            }
            if flags & XY_SCALE != 0 && flags & SCALE != 0
                || flags & MATRIX != 0 && (flags & XY_SCALE != 0 || flags & SCALE != 0)
            {
                return Err(OutlineError::InvalidData(
                    "conflicting component transforms",
                ));
            }
            if flags & XY == 0 && flags & (SCALED | UNSCALED | ROUND) != 0 {
                return Err(OutlineError::InvalidData(
                    "offset flags without XY arguments",
                ));
            }
            let mut xx = 1.0;
            let mut xy = 0.0;
            let mut yx = 0.0;
            let mut yy = 1.0;
            if flags & MATRIX != 0 {
                xx = f2dot14(r.i16()?);
                yx = f2dot14(r.i16()?);
                xy = f2dot14(r.i16()?);
                yy = f2dot14(r.i16()?);
            } else if flags & XY_SCALE != 0 {
                xx = f2dot14(r.i16()?);
                yy = xx;
            } else if flags & SCALE != 0 {
                xx = f2dot14(r.i16()?);
                yy = f2dot14(r.i16()?);
            }
            let mut child_contours = self.decode_glyph(child, depth + 1, context)?;
            for contour in &mut child_contours {
                for p in contour.iter_mut() {
                    let (x, y) = transform(p.x, p.y, xx, xy, yx, yy)?;
                    p.x = x;
                    p.y = y;
                }
            }
            let (mut dx, mut dy) = if flags & XY != 0 {
                (a1 as f64, a2 as f64)
            } else {
                let pi = usize::try_from(a1)
                    .map_err(|_| OutlineError::InvalidData("attachment index"))?;
                let ci = usize::try_from(a2)
                    .map_err(|_| OutlineError::InvalidData("attachment index"))?;
                let parent = all.iter().flat_map(|c| c.iter()).nth(pi).copied().ok_or(
                    OutlineError::Unsupported("phantom or invalid parent attachment point"),
                )?;
                let childp = child_contours
                    .iter()
                    .flat_map(|c| c.iter())
                    .nth(ci)
                    .copied()
                    .ok_or(OutlineError::Unsupported(
                        "phantom or invalid child attachment point",
                    ))?;
                (parent.x - childp.x, parent.y - childp.y)
            };
            if flags & XY != 0 && flags & SCALED != 0 {
                (dx, dy) = transform(dx, dy, xx, xy, yx, yy)?;
            }
            if flags & XY != 0 && flags & ROUND != 0 {
                dx = dx.round();
                dy = dy.round();
            }
            for contour in &mut child_contours {
                for p in contour {
                    p.x += dx;
                    p.y += dy;
                    validate_coord(p.x, p.y)?;
                }
            }
            let composite_points = all
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                .checked_add(child_contours.iter().map(Vec::len).sum())
                .ok_or(OutlineError::LimitExceeded("points"))?;
            let composite_contours = all
                .len()
                .checked_add(child_contours.len())
                .ok_or(OutlineError::LimitExceeded("contours"))?;
            if composite_points > usize::from(be16(maxp, 10)?)
                || composite_contours > usize::from(be16(maxp, 12)?)
            {
                return Err(OutlineError::InvalidData("composite outline exceeds maxp"));
            }
            all.extend(child_contours);
            have_instr |= flags & INSTRUCTIONS != 0;
            if flags & MORE == 0 {
                break;
            }
        }
        if have_instr {
            let n = usize::from(r.u16()?);
            Budget::add(
                &mut context.budget.instructions,
                n,
                MAX_INSTRUCTION_BYTES,
                "instruction bytes",
            )?;
            r.take(n)?;
        }
        Ok(all)
    }
}

fn be16(b: &[u8], p: usize) -> Result<u16, OutlineError> {
    Ok(u16::from_be_bytes(
        b.get(p..p + 2)
            .ok_or(OutlineError::Truncated)?
            .try_into()
            .map_err(|_| OutlineError::Truncated)?,
    ))
}
fn be32(b: &[u8], p: usize) -> Result<u32, OutlineError> {
    Ok(u32::from_be_bytes(
        b.get(p..p + 4)
            .ok_or(OutlineError::Truncated)?
            .try_into()
            .map_err(|_| OutlineError::Truncated)?,
    ))
}
fn f2dot14(v: i16) -> f64 {
    f64::from(v) / 16384.0
}
fn validate_coord(x: f64, y: f64) -> Result<(), OutlineError> {
    Point::try_new(x, y)
        .map(|_| ())
        .map_err(|_| OutlineError::Geometry)
}
fn transform(
    x: f64,
    y: f64,
    xx: f64,
    xy: f64,
    yx: f64,
    yy: f64,
) -> Result<(f64, f64), OutlineError> {
    let a = x * xx + y * xy;
    let b = x * yx + y * yy;
    validate_coord(a, b)?;
    Ok((a, b))
}

fn points_to_path(contours: &[Vec<GlyphPoint>]) -> Result<Path, OutlineError> {
    let mut path = Path::new();
    for contour in contours {
        if contour.is_empty() {
            return Err(OutlineError::InvalidData("empty contour"));
        }
        let first = contour[0];
        let last = *contour
            .last()
            .ok_or(OutlineError::InvalidData("empty contour"))?;
        let (start, mut i, mut processed, start_index) = if first.on {
            (point(first)?, 1usize, 1usize, Some(0usize))
        } else if last.on {
            (point(last)?, 0usize, 1usize, Some(contour.len() - 1))
        } else {
            (midpoint(last, first)?, 0usize, 0usize, None)
        };
        path.move_to(start);
        let mut current = start;
        while processed < contour.len() {
            let p = contour[i % contour.len()];
            if p.on {
                let q = point(p)?;
                if q != current {
                    path.line_to(q).map_err(|_| OutlineError::Geometry)?;
                }
                current = q;
                i += 1;
                processed += 1;
            } else {
                let next = contour[(i + 1) % contour.len()];
                let wraps_to_start = start_index == Some((i + 1) % contour.len());
                let (end, advance) = if next.on {
                    (point(next)?, if wraps_to_start { 1 } else { 2 })
                } else {
                    (midpoint(p, next)?, 1)
                };
                path.quadratic_to(point(p)?, end)
                    .map_err(|_| OutlineError::Geometry)?;
                current = end;
                i += advance;
                processed += advance;
            }
        }
        if current != start {
            path.line_to(start).map_err(|_| OutlineError::Geometry)?;
        }
        path.close().map_err(|_| OutlineError::Geometry)?;
    }
    Ok(path)
}
fn point(p: GlyphPoint) -> Result<Point, OutlineError> {
    Point::try_new(p.x, p.y).map_err(|_| OutlineError::Geometry)
}
fn midpoint(a: GlyphPoint, b: GlyphPoint) -> Result<Point, OutlineError> {
    Point::try_new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0).map_err(|_| OutlineError::Geometry)
}
