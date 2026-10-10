// SPDX-License-Identifier: AGPL-3.0-or-later
//! Safe, bounded Latin OpenType layout for ligatures and pair positioning.

use crate::FontFace;
use core::ops::Range;

const DEFAULT_VALIDATION: usize = 1_000_000;
const DEFAULT_WORK: usize = 10_000_000;
const MAX_INPUT: usize = 65_536;
const MAX_LOOKUPS: usize = 1024;

/// Resource limits for validating a layout façade and running an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutLimits {
    pub validation_units: usize,
    pub work_units: usize,
    pub input_glyphs: usize,
    pub selected_lookups: usize,
}
impl Default for LayoutLimits {
    fn default() -> Self {
        Self {
            validation_units: DEFAULT_VALIDATION,
            work_units: DEFAULT_WORK,
            input_glyphs: MAX_INPUT,
            selected_lookups: MAX_LOOKUPS,
        }
    }
}

/// Errors returned while validating or applying OpenType layout data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    Truncated,
    InvalidData(&'static str),
    InvalidFormat(&'static str),
    LimitExceeded(&'static str),
    InvalidGlyph(u16),
    InvalidClusterRange,
    Unsupported(&'static str),
}
impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated OpenType layout data"),
            Self::InvalidData(s) => write!(f, "invalid OpenType layout data: {s}"),
            Self::InvalidFormat(s) => write!(f, "unsupported OpenType layout format: {s}"),
            Self::LimitExceeded(s) => write!(f, "OpenType layout limit exceeded: {s}"),
            Self::InvalidGlyph(g) => write!(f, "glyph id {g} is outside numGlyphs"),
            Self::InvalidClusterRange => f.write_str("invalid glyph cluster range"),
            Self::Unsupported(s) => write!(f, "unsupported OpenType layout feature: {s}"),
        }
    }
}
impl std::error::Error for LayoutError {}

/// A glyph and its half-open range of Unicode scalar indices in the source text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlyphCluster {
    pub glyph_id: u16,
    pub cluster: Range<usize>,
}

/// Four signed positioning/advance deltas in font units.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlyphAdjustment {
    pub x_placement: i32,
    pub y_placement: i32,
    pub x_advance: i32,
    pub y_advance: i32,
}

#[derive(Clone)]
struct Lookup {
    kind: u16,
    flags: u16,
    subtables: Vec<usize>,
}

/// Validated view of selected Latin GSUB/GPOS lookups. It borrows the source font.
pub struct FontLayout<'a> {
    gsub: Option<&'a [u8]>,
    gpos: Option<&'a [u8]>,
    gdef: Option<&'a [u8]>,
    liga: Vec<Lookup>,
    kern: Vec<Lookup>,
    legacy: Option<&'a [u8]>,
    has_gpos_kern: bool,
    glyphs: u16,
    limits: LayoutLimits,
}
impl core::fmt::Debug for FontLayout<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FontLayout")
            .field("gsub_lookups", &self.liga.len())
            .field("gpos_lookups", &self.kern.len())
            .finish()
    }
}

impl<'a> FontFace<'a> {
    /// Build a validated layout façade using the documented default resource limits.
    pub fn layout(&self) -> Result<FontLayout<'_>, LayoutError> {
        self.layout_with_limits(LayoutLimits::default())
    }

    /// Build a validated layout façade with caller-selected resource limits.
    pub fn layout_with_limits(&self, limits: LayoutLimits) -> Result<FontLayout<'_>, LayoutError> {
        let gsub = self.table(*b"GSUB");
        let gpos = self.table(*b"GPOS");
        let gdef = self.table(*b"GDEF");
        let glyphs = self.metrics().number_of_glyphs;
        let mut budget = Budget {
            remaining: limits.validation_units,
        };
        let liga = match gsub {
            Some(t) => parse_layout(t, *b"liga", 1, glyphs, gdef, limits, &mut budget)?.0,
            None => Vec::new(),
        };
        let (kern, has_gpos_kern) = match gpos {
            Some(t) => parse_layout(t, *b"kern", 2, glyphs, gdef, limits, &mut budget)?,
            None => (Vec::new(), false),
        };
        if liga
            .iter()
            .chain(kern.iter())
            .any(|l| l.flags & 0x0008 != 0)
        {
            if let Some(table) = gdef {
                validate_gdef(table, glyphs, &mut budget)?;
            }
        }
        if liga
            .len()
            .checked_add(kern.len())
            .is_none_or(|n| n > limits.selected_lookups)
        {
            return Err(LayoutError::LimitExceeded("selected lookups"));
        }
        let legacy = if has_gpos_kern {
            None
        } else {
            self.table(*b"kern")
        };
        if let Some(t) = legacy {
            validate_kern(t, glyphs, &mut budget)?;
        }
        Ok(FontLayout {
            gsub,
            gpos,
            gdef,
            liga,
            kern,
            legacy,
            has_gpos_kern,
            glyphs,
            limits,
        })
    }
}

impl FontLayout<'_> {
    /// Apply selected GSUB `liga` lookups. Substitution preserves source ranges;
    /// ligatures merge the ranges of their components and retain skipped marks.
    pub fn apply_ligatures(
        &self,
        input: &[GlyphCluster],
    ) -> Result<Vec<GlyphCluster>, LayoutError> {
        let mut work = Budget {
            remaining: self.limits.work_units,
        };
        work.spend(input.len())?;
        validate_input(input, self.glyphs, self.limits)?;
        let Some(table) = self.gsub else {
            return Ok(input.to_vec());
        };
        let mut out = input.to_vec();
        for lookup in &self.liga {
            work.spend(out.len())?;
            apply_gsub_lookup(table, lookup, self.gdef, &mut out, &mut work, self.glyphs)?;
        }
        Ok(out)
    }

    /// Return one accumulated adjustment per input glyph. Pair positioning is
    /// applied in lookup order; a present GPOS `kern` feature suppresses `kern` v0.
    pub fn pair_adjustments(
        &self,
        input: &[GlyphCluster],
    ) -> Result<Vec<GlyphAdjustment>, LayoutError> {
        let mut work = Budget {
            remaining: self.limits.work_units,
        };
        work.spend(input.len())?;
        validate_input(input, self.glyphs, self.limits)?;
        let mut out = vec![GlyphAdjustment::default(); input.len()];
        if self.has_gpos_kern {
            let table = self
                .gpos
                .ok_or(LayoutError::InvalidData("missing GPOS table"))?;
            for lookup in &self.kern {
                work.spend(input.len())?;
                apply_gpos_lookup(table, lookup, self.gdef, input, &mut out, &mut work)?;
            }
        } else if let Some(table) = self.legacy {
            apply_legacy_kern(table, input, &mut out, &mut work)?;
        }
        Ok(out)
    }
}

#[derive(Clone, Copy)]
struct Budget {
    remaining: usize,
}
impl Budget {
    fn spend(&mut self, n: usize) -> Result<(), LayoutError> {
        self.remaining = self
            .remaining
            .checked_sub(n)
            .ok_or(LayoutError::LimitExceeded("work units"))?;
        Ok(())
    }
}

fn validate_input(
    input: &[GlyphCluster],
    glyphs: u16,
    limits: LayoutLimits,
) -> Result<(), LayoutError> {
    if input.len() > limits.input_glyphs {
        return Err(LayoutError::LimitExceeded("input glyphs"));
    }
    for item in input {
        if item.glyph_id >= glyphs {
            return Err(LayoutError::InvalidGlyph(item.glyph_id));
        }
        if item.cluster.start >= item.cluster.end {
            return Err(LayoutError::InvalidClusterRange);
        }
    }
    Ok(())
}
fn u16at(b: &[u8], p: usize) -> Result<u16, LayoutError> {
    Ok(u16::from_be_bytes(
        b.get(
            p..p.checked_add(2)
                .ok_or(LayoutError::InvalidData("offset overflow"))?,
        )
        .ok_or(LayoutError::Truncated)?
        .try_into()
        .map_err(|_| LayoutError::Truncated)?,
    ))
}
fn i16at(b: &[u8], p: usize) -> Result<i16, LayoutError> {
    Ok(u16at(b, p)? as i16)
}
fn u32at(b: &[u8], p: usize) -> Result<u32, LayoutError> {
    Ok(u32::from_be_bytes(
        b.get(
            p..p.checked_add(4)
                .ok_or(LayoutError::InvalidData("offset overflow"))?,
        )
        .ok_or(LayoutError::Truncated)?
        .try_into()
        .map_err(|_| LayoutError::Truncated)?,
    ))
}
fn rel(base: usize, off: u16, limit: usize) -> Result<usize, LayoutError> {
    if off == 0 {
        return Err(LayoutError::InvalidData("zero required offset"));
    }
    let p = base
        .checked_add(usize::from(off))
        .ok_or(LayoutError::InvalidData("offset overflow"))?;
    if p >= limit {
        return Err(LayoutError::Truncated);
    }
    Ok(p)
}
fn rel32(base: usize, off: u32, limit: usize) -> Result<usize, LayoutError> {
    if off == 0 {
        return Err(LayoutError::InvalidData("zero required offset"));
    }
    let p = base
        .checked_add(usize::try_from(off).map_err(|_| LayoutError::InvalidData("offset overflow"))?)
        .ok_or(LayoutError::InvalidData("offset overflow"))?;
    if p >= limit {
        return Err(LayoutError::Truncated);
    }
    Ok(p)
}

fn parse_layout(
    b: &[u8],
    wanted: [u8; 4],
    kind: u16,
    glyphs: u16,
    gdef: Option<&[u8]>,
    limits: LayoutLimits,
    budget: &mut Budget,
) -> Result<(Vec<Lookup>, bool), LayoutError> {
    if b.len() < 10 {
        return Err(LayoutError::Truncated);
    }
    if u32at(b, 0)? != 0x0001_0000 {
        return Err(LayoutError::InvalidFormat("layout table version"));
    }
    let script_list = rel(0, u16at(b, 4)?, b.len())?;
    let feature_list = rel(0, u16at(b, 6)?, b.len())?;
    let lookup_list = rel(0, u16at(b, 8)?, b.len())?;
    let script_count = usize::from(u16at(b, script_list)?);
    let script_end = script_list
        .checked_add(
            2 + script_count
                .checked_mul(6)
                .ok_or(LayoutError::InvalidData("script count overflow"))?,
        )
        .ok_or(LayoutError::InvalidData("script range overflow"))?;
    if script_end > b.len() {
        return Err(LayoutError::Truncated);
    }
    budget.spend(script_count)?;
    let mut latn = None;
    let mut dflt = None;
    for i in 0..script_count {
        let p = script_list + 2 + i * 6;
        let tag: [u8; 4] = b
            .get(p..p + 4)
            .ok_or(LayoutError::Truncated)?
            .try_into()
            .map_err(|_| LayoutError::Truncated)?;
        let off = u16at(b, p + 4)?;
        if i > 0 {
            let prev = &b[script_list + 2 + (i - 1) * 6..script_list + 6 + (i - 1) * 6];
            if prev >= &b[p..p + 4] {
                return Err(LayoutError::InvalidData(
                    "script records order or duplicates",
                ));
            }
        }
        if tag == *b"latn" {
            latn = Some(rel(script_list, off, b.len())?);
        }
        if tag == *b"DFLT" {
            dflt = Some(rel(script_list, off, b.len())?);
        }
    }
    let script = latn.or(dflt);
    let feature_count = usize::from(u16at(b, feature_list)?);
    let feature_end = feature_list
        .checked_add(
            2 + feature_count
                .checked_mul(6)
                .ok_or(LayoutError::InvalidData("feature count overflow"))?,
        )
        .ok_or(LayoutError::InvalidData("feature range overflow"))?;
    if feature_end > b.len() {
        return Err(LayoutError::Truncated);
    }
    budget.spend(feature_count)?;
    let lookup_count = usize::from(u16at(b, lookup_list)?);
    budget.spend(lookup_count)?;
    let lookup_offsets_end = lookup_list
        .checked_add(
            2 + lookup_count
                .checked_mul(2)
                .ok_or(LayoutError::InvalidData("lookup count overflow"))?,
        )
        .ok_or(LayoutError::InvalidData("lookup range overflow"))?;
    if lookup_offsets_end > b.len() {
        return Err(LayoutError::Truncated);
    }
    let mut selected = vec![false; lookup_count];
    let mut feature_present = false;
    if let Some(script) = script {
        let default_off = u16at(b, script)?;
        let record_count = usize::from(u16at(b, script + 2)?);
        let records = script + 4;
        if records
            .checked_add(
                record_count
                    .checked_mul(6)
                    .ok_or(LayoutError::InvalidData("language count overflow"))?,
            )
            .filter(|&e| e <= b.len())
            .is_none()
        {
            return Err(LayoutError::Truncated);
        }
        let lang = if default_off != 0 {
            Some(rel(script, default_off, b.len())?)
        } else {
            None
        };
        if let Some(lang) = lang {
            if u16at(b, lang)? != 0 {
                return Err(LayoutError::InvalidData("LangSys lookupOrder reserved"));
            }
            let required = u16at(b, lang + 2)?;
            let n = usize::from(u16at(b, lang + 4)?);
            budget.spend(n)?;
            if lang
                .checked_add(6 + n * 2)
                .filter(|&e| e <= b.len())
                .is_none()
            {
                return Err(LayoutError::Truncated);
            }
            if required != 0xffff {
                if usize::from(required) >= feature_count {
                    return Err(LayoutError::InvalidData("required feature index"));
                }
                if feature_tag(b, feature_list, usize::from(required))? == wanted {
                    feature_present = true;
                    mark_feature(
                        b,
                        feature_list,
                        usize::from(required),
                        lookup_count,
                        &mut selected,
                        budget,
                    )?;
                }
            }
            for i in 0..n {
                let fi = usize::from(u16at(b, lang + 6 + i * 2)?);
                if fi >= feature_count {
                    return Err(LayoutError::InvalidData("feature index"));
                }
                if feature_tag(b, feature_list, fi)? == wanted {
                    feature_present = true;
                    mark_feature(b, feature_list, fi, lookup_count, &mut selected, budget)?;
                }
            }
        }
    }
    let picked_count = selected.iter().filter(|&&yes| yes).count();
    if picked_count > limits.selected_lookups {
        return Err(LayoutError::LimitExceeded("selected lookups"));
    }
    let mut picked = Vec::with_capacity(picked_count);
    for (i, yes) in selected.into_iter().enumerate() {
        if yes {
            picked.push(i);
        }
    }
    let mut result = Vec::with_capacity(picked.len());
    for i in picked {
        budget.spend(1)?;
        let off = u16at(b, lookup_list + 2 + i * 2)?;
        let p = rel(lookup_list, off, b.len())?;
        let lookup_type = u16at(b, p)?;
        let flags = u16at(b, p + 2)?;
        if flags & !0x0008 != 0 {
            return Err(LayoutError::Unsupported(
                "lookup flags other than IgnoreMarks",
            ));
        }
        if flags & 0x0008 != 0 {
            let Some(gdef) = gdef else {
                return Err(LayoutError::InvalidData("IgnoreMarks requires GDEF"));
            };
            if gdef_header(gdef)?.0 == 0 {
                return Err(LayoutError::InvalidData(
                    "IgnoreMarks requires GlyphClassDef",
                ));
            }
        }
        let count = usize::from(u16at(b, p + 4)?);
        let end = p
            .checked_add(6 + count * 2)
            .ok_or(LayoutError::InvalidData("subtable count overflow"))?;
        if end > b.len() {
            return Err(LayoutError::Truncated);
        }
        budget.spend(count)?;
        let mut subtables = Vec::with_capacity(count);
        let mut actual = if (kind == 1 && lookup_type == 7) || (kind == 2 && lookup_type == 9) {
            0
        } else {
            lookup_type
        };
        for j in 0..count {
            let mut sub = rel(p, u16at(b, p + 6 + j * 2)?, b.len())?;
            let sub_kind = if (kind == 1 && lookup_type == 7) || (kind == 2 && lookup_type == 9) {
                if u16at(b, sub)? != 1 {
                    return Err(LayoutError::InvalidFormat("extension format"));
                }
                let nested = u16at(b, sub + 2)?;
                if nested == lookup_type {
                    return Err(LayoutError::InvalidData("recursive extension"));
                }
                let target = rel32(sub, u32at(b, sub + 4)?, b.len())?;
                let expected = if kind == 1 {
                    if nested == 1 || nested == 4 {
                        nested
                    } else {
                        return Err(LayoutError::Unsupported("GSUB extension target"));
                    }
                } else if nested == 2 {
                    nested
                } else {
                    return Err(LayoutError::Unsupported("GPOS extension target"));
                };
                sub = target;
                expected
            } else {
                lookup_type
            };
            if actual == 0 {
                actual = sub_kind;
            }
            if sub_kind != actual {
                return Err(LayoutError::InvalidData("lookup subtable type mismatch"));
            }
            validate_subtable(b, sub, sub_kind, glyphs, budget)?;
            subtables.push(sub);
        }
        if (kind == 1 && actual != 1 && actual != 4) || (kind == 2 && actual != 2) {
            return Err(LayoutError::Unsupported("selected lookup type"));
        }
        result.push(Lookup {
            kind: actual,
            flags,
            subtables,
        });
    }
    Ok((result, feature_present))
}
fn feature_tag(b: &[u8], base: usize, i: usize) -> Result<[u8; 4], LayoutError> {
    let p = base + 2 + i * 6;
    b.get(p..p + 4)
        .ok_or(LayoutError::Truncated)?
        .try_into()
        .map_err(|_| LayoutError::Truncated)
}
fn mark_feature(
    b: &[u8],
    base: usize,
    i: usize,
    count: usize,
    out: &mut [bool],
    budget: &mut Budget,
) -> Result<(), LayoutError> {
    let p = base + 2 + i * 6;
    let off = u16at(b, p + 4)?;
    let f = rel(base, off, b.len())?;
    let n = usize::from(u16at(b, f + 2)?);
    budget.spend(n)?;
    if u16at(b, f)? != 0 {
        return Err(LayoutError::Unsupported("feature parameters"));
    }
    if f.checked_add(4 + n * 2).filter(|&e| e <= b.len()).is_none() {
        return Err(LayoutError::Truncated);
    }
    for j in 0..n {
        let x = usize::from(u16at(b, f + 4 + j * 2)?);
        if x >= count {
            return Err(LayoutError::InvalidData("lookup index"));
        }
        out[x] = true;
    }
    Ok(())
}
fn validate_subtable(
    b: &[u8],
    p: usize,
    kind: u16,
    glyphs: u16,
    budget: &mut Budget,
) -> Result<(), LayoutError> {
    match (kind, u16at(b, p)?) {
        (1, 1) | (1, 2) => validate_single(b, p, glyphs, budget),
        (4, 1) => validate_ligature(b, p, glyphs, budget),
        (2, 1) | (2, 2) => validate_pair(b, p, glyphs, budget),
        _ => Err(LayoutError::Unsupported(
            "selected lookup subtable type/format",
        )),
    }
}

fn coverage(
    b: &[u8],
    p: usize,
    glyph: u16,
    budget: &mut Budget,
) -> Result<Option<usize>, LayoutError> {
    let fmt = u16at(b, p)?;
    match fmt {
        1 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 2).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                budget.spend(1)?;
                let mid = lo + (hi - lo) / 2;
                match u16at(b, p + 4 + mid * 2)?.cmp(&glyph) {
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                    std::cmp::Ordering::Equal => return Ok(Some(mid)),
                }
            }
            Ok(None)
        }
        2 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 6).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                budget.spend(1)?;
                let mid = lo + (hi - lo) / 2;
                let q = p + 4 + mid * 6;
                let start = u16at(b, q)?;
                let end = u16at(b, q + 2)?;
                if glyph < start {
                    hi = mid;
                } else if glyph > end {
                    lo = mid + 1;
                } else {
                    let index = usize::from(u16at(b, q + 4)?);
                    return Ok(Some(index + usize::from(glyph - start)));
                }
            }
            Ok(None)
        }
        _ => Err(LayoutError::InvalidFormat("Coverage format")),
    }
}
fn validate_coverage(
    b: &[u8],
    p: usize,
    glyphs: u16,
    budget: &mut Budget,
) -> Result<usize, LayoutError> {
    let mut count = 0;
    match u16at(b, p)? {
        1 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 2).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            budget.spend(n)?;
            let mut prev = None;
            for i in 0..n {
                let g = u16at(b, p + 4 + i * 2)?;
                if g >= glyphs {
                    return Err(LayoutError::InvalidData("Coverage glyph"));
                }
                if prev.is_some_and(|v| g <= v) {
                    return Err(LayoutError::InvalidData("Coverage order or duplicates"));
                }
                prev = Some(g)
            }
            count = n
        }
        2 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 6).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            budget.spend(n)?;
            let mut prev = None;
            for i in 0..n {
                let q = p + 4 + i * 6;
                let s = u16at(b, q)?;
                let e = u16at(b, q + 2)?;
                let ix = usize::from(u16at(b, q + 4)?);
                if s > e || e >= glyphs || prev.is_some_and(|v| s <= v) || ix != count {
                    return Err(LayoutError::InvalidData("Coverage range order/index"));
                }
                count = count
                    .checked_add(usize::from(e - s) + 1)
                    .ok_or(LayoutError::InvalidData("Coverage count overflow"))?;
                prev = Some(e)
            }
        }
        _ => return Err(LayoutError::InvalidFormat("Coverage format")),
    }
    Ok(count)
}
fn class_def(b: &[u8], p: usize, g: u16, budget: &mut Budget) -> Result<u16, LayoutError> {
    match u16at(b, p)? {
        1 => {
            let start = u16at(b, p + 2)?;
            let n = usize::from(u16at(b, p + 4)?);
            if p.checked_add(6 + n * 2).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            budget.spend(1)?;
            if g < start || usize::from(g - start) >= n {
                return Ok(0);
            }
            Ok(u16at(b, p + 6 + usize::from(g - start) * 2)?)
        }
        2 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 6).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                budget.spend(1)?;
                let mid = lo + (hi - lo) / 2;
                let q = p + 4 + mid * 6;
                let start = u16at(b, q)?;
                let end = u16at(b, q + 2)?;
                if g < start {
                    hi = mid;
                } else if g > end {
                    lo = mid + 1;
                } else {
                    return u16at(b, q + 4);
                }
            }
            Ok(0)
        }
        _ => Err(LayoutError::InvalidFormat("ClassDef format")),
    }
}

fn validate_single(
    b: &[u8],
    p: usize,
    glyphs: u16,
    budget: &mut Budget,
) -> Result<(), LayoutError> {
    let fmt = u16at(b, p)?;
    let cov = rel(p, u16at(b, p + 2)?, b.len())?;
    let n = validate_coverage(b, cov, glyphs, budget)?;
    match fmt {
        1 => {
            let delta = i16at(b, p + 4)? as u16;
            budget.spend(n)?;
            match u16at(b, cov)? {
                1 => {
                    for i in 0..n {
                        if u16at(b, cov + 4 + i * 2)?.wrapping_add(delta) >= glyphs {
                            return Err(LayoutError::InvalidData("substitute glyph"));
                        }
                    }
                }
                2 => {
                    let ranges = usize::from(u16at(b, cov + 2)?);
                    let mut index = 0usize;
                    for r in 0..ranges {
                        let q = cov + 4 + r * 6;
                        let start = u16at(b, q)?;
                        let end = u16at(b, q + 2)?;
                        for gid in start..=end {
                            if gid.wrapping_add(delta) >= glyphs {
                                return Err(LayoutError::InvalidData("substitute glyph"));
                            }
                            index += 1;
                        }
                    }
                    if index != n {
                        return Err(LayoutError::InvalidData("Coverage count"));
                    }
                }
                _ => return Err(LayoutError::InvalidFormat("Coverage format")),
            }
        }
        2 => {
            let count = usize::from(u16at(b, p + 4)?);
            if count != n
                || p.checked_add(6 + count * 2)
                    .filter(|&e| e <= b.len())
                    .is_none()
            {
                return Err(LayoutError::InvalidData("SingleSubst glyph count"));
            }
            budget.spend(count)?;
            for i in 0..count {
                if u16at(b, p + 6 + i * 2)? >= glyphs {
                    return Err(LayoutError::InvalidData("substitute glyph"));
                }
            }
        }
        _ => return Err(LayoutError::InvalidFormat("SingleSubst format")),
    }
    Ok(())
}
fn validate_ligature(
    b: &[u8],
    p: usize,
    glyphs: u16,
    budget: &mut Budget,
) -> Result<(), LayoutError> {
    let cov = rel(p, u16at(b, p + 2)?, b.len())?;
    let n = validate_coverage(b, cov, glyphs, budget)?;
    let setn = usize::from(u16at(b, p + 4)?);
    if setn != n
        || p.checked_add(6 + setn * 2)
            .filter(|&e| e <= b.len())
            .is_none()
    {
        return Err(LayoutError::InvalidData("LigatureSet count"));
    }
    for i in 0..setn {
        let set = rel(p, u16at(b, p + 6 + i * 2)?, b.len())?;
        let c = usize::from(u16at(b, set)?);
        if set
            .checked_add(2 + c * 2)
            .filter(|&e| e <= b.len())
            .is_none()
        {
            return Err(LayoutError::Truncated);
        }
        budget.spend(c)?;
        for j in 0..c {
            let lig = rel(set, u16at(b, set + 2 + j * 2)?, b.len())?;
            let comp = usize::from(u16at(b, lig + 2)?);
            if comp < 2
                || lig
                    .checked_add(4 + (comp - 1) * 2)
                    .filter(|&e| e <= b.len())
                    .is_none()
            {
                return Err(LayoutError::InvalidData("Ligature component count"));
            }
            if u16at(b, lig)? >= glyphs {
                return Err(LayoutError::InvalidData("ligature glyph"));
            }
            budget.spend(comp)?;
            for k in 0..comp - 1 {
                if u16at(b, lig + 4 + k * 2)? >= glyphs {
                    return Err(LayoutError::InvalidData("ligature component glyph"));
                }
            }
        }
    }
    Ok(())
}
fn validate_pair(b: &[u8], p: usize, glyphs: u16, budget: &mut Budget) -> Result<(), LayoutError> {
    let fmt = u16at(b, p)?;
    let f1 = u16at(b, p + 4)?;
    let f2 = u16at(b, p + 6)?;
    validate_value_format(f1)?;
    validate_value_format(f2)?;
    let s1 = value_size(f1)?;
    let s2 = value_size(f2)?;
    let cov = rel(p, u16at(b, p + 2)?, b.len())?;
    let n = validate_coverage(b, cov, glyphs, budget)?;
    match fmt {
        1 => {
            let sets = usize::from(u16at(b, p + 8)?);
            if sets != n
                || p.checked_add(10 + sets * 2)
                    .filter(|&e| e <= b.len())
                    .is_none()
            {
                return Err(LayoutError::InvalidData("PairSet count"));
            }
            for i in 0..sets {
                let q = rel(p, u16at(b, p + 10 + i * 2)?, b.len())?;
                let c = usize::from(u16at(b, q)?);
                let stride = 2 + s1 + s2;
                if q.checked_add(2 + c * stride)
                    .filter(|&e| e <= b.len())
                    .is_none()
                {
                    return Err(LayoutError::Truncated);
                }
                budget.spend(c)?;
                let mut prev = None;
                for j in 0..c {
                    let at = q + 2 + j * stride;
                    let second = u16at(b, at)?;
                    if second >= glyphs || prev.is_some_and(|v| second <= v) {
                        return Err(LayoutError::InvalidData("PairSet order or glyph"));
                    }
                    prev = Some(second);
                    parse_value(b, at + 2, f1)?;
                    parse_value(b, at + 2 + s1, f2)?;
                }
            }
        }
        2 => {
            let c1 = rel(p, u16at(b, p + 8)?, b.len())?;
            let c2 = rel(p, u16at(b, p + 10)?, b.len())?;
            let n1 = usize::from(u16at(b, p + 12)?);
            let n2 = usize::from(u16at(b, p + 14)?);
            let matrix = n1
                .checked_mul(n2)
                .ok_or(LayoutError::InvalidData("ClassDef matrix dimensions"))?;
            let bytes = matrix
                .checked_mul(s1 + s2)
                .and_then(|x| p.checked_add(16 + x))
                .ok_or(LayoutError::InvalidData("ClassDef matrix dimensions"))?;
            if n1 == 0 || n2 == 0 || bytes > b.len() {
                return Err(LayoutError::InvalidData("ClassDef matrix dimensions"));
            }
            validate_class_def_all(b, c1, glyphs, n1 as u16, budget)?;
            validate_class_def_all(b, c2, glyphs, n2 as u16, budget)?;
            budget.spend(matrix)?;
            for i in 0..matrix {
                let at = p + 16 + i * (s1 + s2);
                parse_value(b, at, f1)?;
                parse_value(b, at + s1, f2)?;
            }
        }
        _ => return Err(LayoutError::InvalidFormat("PairPos format")),
    }
    Ok(())
}
fn validate_class_def_all(
    b: &[u8],
    p: usize,
    glyphs: u16,
    class_count: u16,
    budget: &mut Budget,
) -> Result<(), LayoutError> {
    match u16at(b, p)? {
        1 => {
            let n = usize::from(u16at(b, p + 4)?);
            let start = usize::from(u16at(b, p + 2)?);
            if start
                .checked_add(n)
                .is_none_or(|end| end > usize::from(glyphs))
                || p.checked_add(6 + n * 2).filter(|&e| e <= b.len()).is_none()
            {
                return Err(LayoutError::Truncated);
            }
            budget.spend(n)?;
            for i in 0..n {
                if u16at(b, p + 6 + i * 2)? >= class_count {
                    return Err(LayoutError::InvalidData("ClassDef value outside matrix"));
                }
            }
        }
        2 => {
            let n = usize::from(u16at(b, p + 2)?);
            if p.checked_add(4 + n * 6).filter(|&e| e <= b.len()).is_none() {
                return Err(LayoutError::Truncated);
            }
            budget.spend(n)?;
            let mut prev = None;
            for i in 0..n {
                let q = p + 4 + i * 6;
                let s = u16at(b, q)?;
                let e = u16at(b, q + 2)?;
                if s > e || e >= glyphs || prev.is_some_and(|v| s <= v) {
                    return Err(LayoutError::InvalidData("ClassDef range order"));
                }
                if u16at(b, q + 4)? >= class_count {
                    return Err(LayoutError::InvalidData("ClassDef value outside matrix"));
                }
                prev = Some(e)
            }
        }
        _ => return Err(LayoutError::InvalidFormat("ClassDef format")),
    }
    Ok(())
}
fn validate_value_format(f: u16) -> Result<(), LayoutError> {
    if f & !0x00ff != 0 {
        return Err(LayoutError::InvalidData("ValueFormat reserved bits"));
    }
    Ok(())
}
fn value_size(f: u16) -> Result<usize, LayoutError> {
    Ok(usize::try_from(f.count_ones()).unwrap_or(0) * 2)
}
fn parse_value(b: &[u8], p: usize, f: u16) -> Result<GlyphAdjustment, LayoutError> {
    let mut at = p;
    let mut vals = [0i16; 4];
    for (bit, slot) in vals.iter_mut().enumerate() {
        if f & (1 << bit) != 0 {
            let v = u16at(b, at)?;
            *slot = v as i16;
            at += 2;
        }
    }
    for bit in 4..8 {
        if f & (1 << bit) != 0 {
            if u16at(b, at)? != 0 {
                return Err(LayoutError::Unsupported(
                    "active Device or VariationIndex value",
                ));
            }
            at += 2;
        }
    }
    Ok(GlyphAdjustment {
        x_placement: i32::from(vals[0]),
        y_placement: i32::from(vals[1]),
        x_advance: i32::from(vals[2]),
        y_advance: i32::from(vals[3]),
    })
}

fn gdef_header(b: &[u8]) -> Result<(u16, u32), LayoutError> {
    if b.len() < 12 {
        return Err(LayoutError::Truncated);
    }
    let version = u32at(b, 0)?;
    let header_len = match version {
        0x0001_0000 => 12,
        0x0001_0002 => 14,
        0x0001_0003 => 18,
        _ => return Err(LayoutError::InvalidFormat("GDEF version")),
    };
    if b.len() < header_len {
        return Err(LayoutError::Truncated);
    }
    Ok((u16at(b, 4)?, version))
}
fn is_mark(gdef: Option<&[u8]>, glyph: u16, budget: &mut Budget) -> Result<bool, LayoutError> {
    let Some(b) = gdef else { return Ok(false) };
    let (off, _) = gdef_header(b)?;
    if off == 0 {
        return Ok(false);
    }
    let p = rel(0, off, b.len())?;
    Ok(class_def(b, p, glyph, budget)? == 3)
}
fn validate_gdef(b: &[u8], glyphs: u16, budget: &mut Budget) -> Result<(), LayoutError> {
    let (off, _) = gdef_header(b)?;
    if off == 0 {
        return Ok(());
    }
    let p = rel(0, off, b.len())?;
    validate_class_def_all(b, p, glyphs, 5, budget)
}
fn apply_gsub_lookup(
    b: &[u8],
    l: &Lookup,
    gdef: Option<&[u8]>,
    items: &mut Vec<GlyphCluster>,
    work: &mut Budget,
    glyphs: u16,
) -> Result<(), LayoutError> {
    match l.kind {
        1 => {
            for item in items {
                for &p in &l.subtables {
                    let fmt = u16at(b, p)?;
                    let cov = rel(p, u16at(b, p + 2)?, b.len())?;
                    if let Some(ix) = coverage(b, cov, item.glyph_id, work)? {
                        item.glyph_id = if fmt == 1 {
                            item.glyph_id.wrapping_add(i16at(b, p + 4)? as u16)
                        } else {
                            u16at(b, p + 6 + ix * 2)?
                        };
                        if item.glyph_id >= glyphs {
                            return Err(LayoutError::InvalidData("substituted glyph"));
                        }
                        break;
                    }
                }
            }
        }
        4 => {
            let mut i = 0;
            while i < items.len() {
                work.spend(1)?;
                let mut found = None;
                'subtables: for &p in &l.subtables {
                    let cov = rel(p, u16at(b, p + 2)?, b.len())?;
                    let Some(ix) = coverage(b, cov, items[i].glyph_id, work)? else {
                        continue;
                    };
                    let n = usize::from(u16at(b, p + 4)?);
                    if ix >= n {
                        continue;
                    }
                    let set = rel(p, u16at(b, p + 6 + ix * 2)?, b.len())?;
                    let c = usize::from(u16at(b, set)?);
                    for j in 0..c {
                        let lig = rel(set, u16at(b, set + 2 + j * 2)?, b.len())?;
                        let nc = usize::from(u16at(b, lig + 2)?);
                        work.spend(nc)?;
                        let mut pos = i + 1;
                        let mut indices = vec![i];
                        let mut matches = true;
                        for k in 0..nc - 1 {
                            while pos < items.len()
                                && l.flags & 0x0008 != 0
                                && is_mark(gdef, items[pos].glyph_id, work)?
                            {
                                pos += 1;
                            }
                            if pos >= items.len()
                                || items[pos].glyph_id != u16at(b, lig + 4 + k * 2)?
                            {
                                matches = false;
                                break;
                            }
                            indices.push(pos);
                            pos += 1;
                        }
                        if matches {
                            found = Some((u16at(b, lig)?, indices));
                            break 'subtables;
                        }
                    }
                }
                if let Some((gid, indices)) = found {
                    let start = indices
                        .iter()
                        .map(|&x| items[x].cluster.start)
                        .min()
                        .ok_or(LayoutError::InvalidData("empty ligature"))?;
                    let end = indices
                        .iter()
                        .map(|&x| items[x].cluster.end)
                        .max()
                        .ok_or(LayoutError::InvalidData("empty ligature"))?;
                    for &x in indices.iter().skip(1).rev() {
                        work.spend(items.len() - x)?;
                        items.remove(x);
                    }
                    items[i] = GlyphCluster {
                        glyph_id: gid,
                        cluster: start..end,
                    };
                }
                i += 1;
            }
        }
        _ => return Err(LayoutError::Unsupported("GSUB lookup")),
    }
    Ok(())
}
fn add(a: &mut GlyphAdjustment, b: GlyphAdjustment) -> Result<(), LayoutError> {
    a.x_placement = a
        .x_placement
        .checked_add(b.x_placement)
        .ok_or(LayoutError::LimitExceeded("adjustment accumulation"))?;
    a.y_placement = a
        .y_placement
        .checked_add(b.y_placement)
        .ok_or(LayoutError::LimitExceeded("adjustment accumulation"))?;
    a.x_advance = a
        .x_advance
        .checked_add(b.x_advance)
        .ok_or(LayoutError::LimitExceeded("adjustment accumulation"))?;
    a.y_advance = a
        .y_advance
        .checked_add(b.y_advance)
        .ok_or(LayoutError::LimitExceeded("adjustment accumulation"))?;
    Ok(())
}
fn next_non_mark(
    items: &[GlyphCluster],
    mut i: usize,
    flags: u16,
    gdef: Option<&[u8]>,
    budget: &mut Budget,
) -> Result<Option<usize>, LayoutError> {
    while i < items.len() {
        if flags & 0x0008 == 0 || !is_mark(gdef, items[i].glyph_id, budget)? {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}
fn apply_gpos_lookup(
    b: &[u8],
    l: &Lookup,
    gdef: Option<&[u8]>,
    items: &[GlyphCluster],
    out: &mut [GlyphAdjustment],
    work: &mut Budget,
) -> Result<(), LayoutError> {
    let mut i = 0;
    while i < items.len() {
        work.spend(1)?;
        let Some(j) = next_non_mark(items, i + 1, l.flags, gdef, work)? else {
            break;
        };
        let mut matched = None;
        for &p in &l.subtables {
            let cov = rel(p, u16at(b, p + 2)?, b.len())?;
            let Some(ix) = coverage(b, cov, items[i].glyph_id, work)? else {
                continue;
            };
            let fmt = u16at(b, p)?;
            let f1 = u16at(b, p + 4)?;
            let f2 = u16at(b, p + 6)?;
            let s1 = value_size(f1)?;
            let s2 = value_size(f2)?;
            let pair = if fmt == 1 {
                let sets = usize::from(u16at(b, p + 8)?);
                if ix >= sets {
                    return Err(LayoutError::InvalidData("PairSet coverage index"));
                }
                let q = rel(p, u16at(b, p + 10 + ix * 2)?, b.len())?;
                let n = usize::from(u16at(b, q)?);
                let stride = 2 + s1 + s2;
                let target = items[j].glyph_id;
                let mut lo = 0;
                let mut hi = n;
                let mut found = None;
                while lo < hi {
                    work.spend(1)?;
                    let mid = lo + (hi - lo) / 2;
                    let at = q + 2 + mid * stride;
                    let gid = u16at(b, at)?;
                    if target < gid {
                        hi = mid;
                    } else if target > gid {
                        lo = mid + 1;
                    } else {
                        found = Some((
                            parse_value(b, at + 2, f1)?,
                            parse_value(b, at + 2 + s1, f2)?,
                            f2,
                        ));
                        break;
                    }
                }
                found
            } else {
                let c1 = rel(p, u16at(b, p + 8)?, b.len())?;
                let c2 = rel(p, u16at(b, p + 10)?, b.len())?;
                let n1 = usize::from(u16at(b, p + 12)?);
                let n2 = usize::from(u16at(b, p + 14)?);
                let a = usize::from(class_def(b, c1, items[i].glyph_id, work)?);
                let z = usize::from(class_def(b, c2, items[j].glyph_id, work)?);
                if a >= n1 || z >= n2 {
                    return Err(LayoutError::InvalidData("PairPos class outside matrix"));
                }
                let at = p + 16 + (a * n2 + z) * (s1 + s2);
                Some((parse_value(b, at, f1)?, parse_value(b, at + s1, f2)?, f2))
            };
            if pair.is_some() {
                matched = pair;
                break;
            }
        }
        if let Some((v1, v2, f2)) = matched {
            add(&mut out[i], v1)?;
            add(&mut out[j], v2)?;
            i += if f2 == 0 { 1 } else { j - i + 1 };
        } else {
            i += 1;
        }
    }
    Ok(())
}

fn validate_kern(b: &[u8], glyphs: u16, budget: &mut Budget) -> Result<(), LayoutError> {
    if b.len() < 4 {
        return Err(LayoutError::Truncated);
    }
    if u16at(b, 0)? != 0 {
        return Err(LayoutError::Unsupported("kern version"));
    }
    let n = usize::from(u16at(b, 2)?);
    budget.spend(n)?;
    let mut p = 4;
    for _ in 0..n {
        if p + 6 > b.len() {
            return Err(LayoutError::Truncated);
        }
        if u16at(b, p)? != 0 {
            return Err(LayoutError::Unsupported("kern subtable version"));
        }
        let len = usize::from(u16at(b, p + 2)?);
        let cov = u16at(b, p + 4)?;
        if len < 6 || p.checked_add(len).filter(|&e| e <= b.len()).is_none() {
            return Err(LayoutError::Truncated);
        }
        let format = (cov >> 8) & 0xff;
        if cov & 0x6 != 0 {
            return Err(LayoutError::Unsupported("kern minimum/cross-stream"));
        }
        if cov & 0x1 != 0 {
            if format != 0 {
                return Err(LayoutError::Unsupported("legacy kern format"));
            }
            if cov & 0x00f0 != 0 {
                return Err(LayoutError::InvalidData("kern coverage reserved bits"));
            }
            if len < 14 {
                return Err(LayoutError::Truncated);
            }
            let pairs = usize::from(u16at(b, p + 6)?);
            let pair_bytes = pairs
                .checked_mul(6)
                .and_then(|v| p.checked_add(14 + v))
                .ok_or(LayoutError::InvalidData("kern pair range overflow"))?;
            if pair_bytes > p + len {
                return Err(LayoutError::Truncated);
            }
            budget.spend(pairs)?;
            let power = if pairs == 0 {
                0
            } else {
                1usize << (usize::BITS - 1 - pairs.leading_zeros())
            };
            let expected_search = power * 6;
            if usize::from(u16at(b, p + 8)?) != expected_search
                || u16at(b, p + 10)? != power.trailing_zeros() as u16
                || usize::from(u16at(b, p + 12)?) != pairs * 6 - expected_search
            {
                return Err(LayoutError::InvalidData("kern format 0 search header"));
            }
            let mut prev = None;
            for i in 0..pairs {
                let q = p + 14 + i * 6;
                let l = u16at(b, q)?;
                let r = u16at(b, q + 2)?;
                if l >= glyphs || r >= glyphs {
                    return Err(LayoutError::InvalidData("kern glyph"));
                }
                let key = (l, r);
                if prev.is_some_and(|v| key <= v) {
                    return Err(LayoutError::InvalidData("kern pair order or duplicate"));
                }
                prev = Some(key)
            }
        } else if cov & 0x00f0 != 0 {
            return Err(LayoutError::InvalidData("kern coverage reserved bits"));
        } else if format != 0 {
            return Err(LayoutError::Unsupported("legacy kern format"));
        }
        p += len;
    }
    Ok(())
}
fn apply_legacy_kern(
    b: &[u8],
    items: &[GlyphCluster],
    out: &mut [GlyphAdjustment],
    work: &mut Budget,
) -> Result<(), LayoutError> {
    let n = usize::from(u16at(b, 2)?);
    work.spend(n)?;
    let mut p = 4;
    for _ in 0..n {
        let len = usize::from(u16at(b, p + 2)?);
        let cov = u16at(b, p + 4)?;
        if cov & 1 != 0 {
            let count = usize::from(u16at(b, p + 6)?);
            for i in 0..items.len().saturating_sub(1) {
                work.spend(1)?;
                let key = (items[i].glyph_id, items[i + 1].glyph_id);
                let mut lo = 0;
                let mut hi = count;
                while lo < hi {
                    let m = lo + (hi - lo) / 2;
                    let q = p + 14 + m * 6;
                    let pair = (u16at(b, q)?, u16at(b, q + 2)?);
                    if key < pair {
                        hi = m
                    } else if key > pair {
                        lo = m + 1
                    } else {
                        let value = i32::from(i16at(b, q + 4)?);
                        if cov & 0x8 != 0 {
                            out[i].x_advance = value;
                        } else {
                            add(
                                &mut out[i],
                                GlyphAdjustment {
                                    x_advance: value,
                                    ..Default::default()
                                },
                            )?;
                        }
                        break;
                    }
                }
            }
        }
        p += len
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(b: &mut [u8], p: usize, v: u16) {
        b[p..p + 2].copy_from_slice(&v.to_be_bytes());
    }
    fn b16(v: u16, out: &mut Vec<u8>) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    fn budget() -> Budget {
        Budget {
            remaining: DEFAULT_VALIDATION,
        }
    }
    fn one_lookup_layout(
        table_tag: [u8; 4],
        lookup: Vec<u8>,
        feature_indexes: &[u16],
        required: u16,
    ) -> Vec<u8> {
        let mut script = vec![
            0,
            1,
            b'l',
            b'a',
            b't',
            b'n',
            0,
            8,
            0,
            4,
            0,
            0,
            0,
            0,
            0xff,
            0xff,
            0,
            feature_indexes.len() as u8,
        ];
        for &v in feature_indexes {
            b16(v, &mut script);
        }
        let mut feature = vec![0, 1];
        feature.extend_from_slice(&table_tag);
        feature.extend_from_slice(&[0, 8, 0, 0]);
        b16(1, &mut feature);
        b16(0, &mut feature);
        let mut lookup_list = vec![0, 1, 0, 4];
        lookup_list.extend_from_slice(&lookup);
        let script_off = 10usize;
        let feature_off = script_off + script.len();
        let lookup_off = feature_off + feature.len();
        let mut out = vec![0; 10];
        put16(&mut out, 0, 1);
        put16(&mut out, 4, script_off as u16);
        put16(&mut out, 6, feature_off as u16);
        put16(&mut out, 8, lookup_off as u16);
        out.extend_from_slice(&script);
        out.extend_from_slice(&feature);
        out.extend_from_slice(&lookup_list);
        let lang = script_off + 12;
        put16(&mut out, lang + 2, required);
        out
    }
    fn single_lookup(sub: Vec<u8>, lookup_type: u16) -> Vec<u8> {
        let mut v = vec![0, lookup_type as u8, 0, 0, 0, 1, 0, 8];
        v.extend(sub);
        v
    }

    #[test]
    fn coverage_and_class_def_formats_one_and_two_validate_and_resolve() {
        let cov1 = [0, 1, 0, 2, 0, 1, 0, 3];
        assert_eq!(coverage(&cov1, 0, 3, &mut budget()).unwrap(), Some(1));
        let cov2 = [0, 2, 0, 1, 0, 3, 0, 5, 0, 0];
        assert_eq!(coverage(&cov2, 0, 5, &mut budget()).unwrap(), Some(2));
        let c1 = [0, 1, 0, 3, 0, 2, 0, 1, 0, 2];
        assert_eq!(class_def(&c1, 0, 4, &mut budget()).unwrap(), 2);
        let c2 = [0, 2, 0, 1, 0, 4, 0, 6, 0, 3];
        assert_eq!(class_def(&c2, 0, 5, &mut budget()).unwrap(), 3);
    }

    #[test]
    fn gsub_single_substitution_formats_one_and_two_work() {
        let f1 = vec![0, 1, 0, 6, 0, 2, 0, 1, 0, 1, 0, 1];
        validate_subtable(&f1, 0, 1, 6, &mut budget()).unwrap();
        let mut items = vec![GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        }];
        apply_gsub_lookup(
            &f1,
            &Lookup {
                kind: 1,
                flags: 0,
                subtables: vec![0],
            },
            None,
            &mut items,
            &mut budget(),
            6,
        )
        .unwrap();
        assert_eq!(items[0].glyph_id, 3);
        let f2 = [0, 2, 0, 8, 0, 1, 0, 3, 0, 1, 0, 1, 0, 1];
        validate_subtable(&f2, 0, 1, 6, &mut budget()).unwrap();
        let mut items = vec![GlyphCluster {
            glyph_id: 1,
            cluster: 2..3,
        }];
        apply_gsub_lookup(
            &f2,
            &Lookup {
                kind: 1,
                flags: 0,
                subtables: vec![0],
            },
            None,
            &mut items,
            &mut budget(),
            6,
        )
        .unwrap();
        assert_eq!(
            items[0],
            GlyphCluster {
                glyph_id: 3,
                cluster: 2..3
            }
        );
    }

    #[test]
    fn ignore_marks_does_not_filter_the_current_glyph() {
        let gdef = [
            0, 1, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 2, 0, 2, 0, 3,
        ];
        let sub = [0, 2, 0, 8, 0, 1, 0, 4, 0, 1, 0, 1, 0, 2];
        let mut items = vec![GlyphCluster {
            glyph_id: 2,
            cluster: 0..1,
        }];
        apply_gsub_lookup(
            &sub,
            &Lookup {
                kind: 1,
                flags: 0x0008,
                subtables: vec![0],
            },
            Some(&gdef),
            &mut items,
            &mut budget(),
            6,
        )
        .unwrap();
        assert_eq!(items[0].glyph_id, 4);
    }

    #[test]
    fn lookup_and_ligature_priority_use_first_match_in_lookup_list_order() {
        let mut alternatives = vec![0, 2, 0, 8, 0, 1, 0, 4, 0, 1, 0, 1, 0, 1];
        alternatives.extend_from_slice(&[0, 2, 0, 8, 0, 1, 0, 3, 0, 1, 0, 1, 0, 1]);
        let lookup = Lookup {
            kind: 1,
            flags: 0,
            subtables: vec![0, 14],
        };
        let mut items = vec![GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        }];
        apply_gsub_lookup(&alternatives, &lookup, None, &mut items, &mut budget(), 6).unwrap();
        assert_eq!(items[0].glyph_id, 4);

        let mut lig = vec![0, 1, 0, 26, 0, 1, 0, 8, 0, 2, 0, 6, 0, 12];
        lig.extend_from_slice(&[0, 4, 0, 2, 0, 3, 0, 5, 0, 2, 0, 3]);
        lig.extend_from_slice(&[0, 1, 0, 1, 0, 1]);
        validate_subtable(&lig, 0, 4, 6, &mut budget()).unwrap();
        let mut items = vec![
            GlyphCluster {
                glyph_id: 1,
                cluster: 0..1,
            },
            GlyphCluster {
                glyph_id: 3,
                cluster: 1..2,
            },
        ];
        apply_gsub_lookup(
            &lig,
            &Lookup {
                kind: 4,
                flags: 0,
                subtables: vec![0],
            },
            None,
            &mut items,
            &mut budget(),
            6,
        )
        .unwrap();
        assert_eq!(
            items,
            [GlyphCluster {
                glyph_id: 4,
                cluster: 0..2
            }]
        );

        let sub1 = vec![0, 2, 0, 8, 0, 1, 0, 2, 0, 1, 0, 1, 0, 1];
        let sub2 = vec![0, 2, 0, 8, 0, 1, 0, 4, 0, 1, 0, 1, 0, 2];
        let block1 = single_lookup(sub1, 1);
        let block2 = single_lookup(sub2, 1);
        let mut script = vec![0, 1, b'l', b'a', b't', b'n', 0, 8, 0, 4, 0, 0];
        script.extend_from_slice(&[0, 0, 0xff, 0xff, 0, 1, 0, 0]);
        let feature = vec![0, 1, b'l', b'i', b'g', b'a', 0, 8, 0, 0, 0, 2, 0, 0, 0, 1];
        let mut ll = vec![0, 2];
        b16(6, &mut ll);
        b16((6 + block1.len()) as u16, &mut ll);
        ll.extend_from_slice(&block1);
        ll.extend_from_slice(&block2);
        let mut table = vec![0; 10];
        put16(&mut table, 0, 1);
        put16(&mut table, 4, 10);
        put16(&mut table, 6, 30);
        put16(&mut table, 8, 46);
        table.extend_from_slice(&script);
        table.extend_from_slice(&feature);
        table.extend_from_slice(&ll);
        assert_eq!(table.len(), 96);
        assert_eq!(u16at(&table, 46).unwrap(), 2);
        assert_eq!(u16at(&table, 48).unwrap(), 6);
        assert_eq!(u16at(&table, 50).unwrap(), 28);
        let (lookups, _) = parse_layout(
            &table,
            *b"liga",
            1,
            6,
            None,
            LayoutLimits::default(),
            &mut budget(),
        )
        .unwrap();
        assert_eq!(lookups.len(), 2);
        let mut items = vec![GlyphCluster {
            glyph_id: 1,
            cluster: 0..1,
        }];
        for l in &lookups {
            apply_gsub_lookup(&table, l, None, &mut items, &mut budget(), 6).unwrap();
        }
        assert_eq!(items[0].glyph_id, 4);
    }

    #[test]
    fn gpos_pair_class_matrix_format_two_applies_signed_value() {
        let mut b = vec![0; 42];
        put16(&mut b, 0, 2);
        put16(&mut b, 2, 18);
        put16(&mut b, 4, 4);
        put16(&mut b, 8, 24);
        put16(&mut b, 10, 32);
        put16(&mut b, 12, 1);
        put16(&mut b, 14, 1);
        put16(&mut b, 16, (-50i16) as u16);
        put16(&mut b, 18, 1);
        put16(&mut b, 20, 1);
        put16(&mut b, 22, 1);
        put16(&mut b, 24, 1);
        put16(&mut b, 26, 1);
        put16(&mut b, 28, 1);
        put16(&mut b, 30, 0);
        put16(&mut b, 32, 2);
        put16(&mut b, 34, 1);
        put16(&mut b, 36, 3);
        put16(&mut b, 38, 3);
        put16(&mut b, 40, 0);
        validate_subtable(&b, 0, 2, 6, &mut budget()).unwrap();
        let items = [
            GlyphCluster {
                glyph_id: 1,
                cluster: 0..1,
            },
            GlyphCluster {
                glyph_id: 3,
                cluster: 1..2,
            },
        ];
        let mut out = [GlyphAdjustment::default(); 2];
        apply_gpos_lookup(
            &b,
            &Lookup {
                kind: 2,
                flags: 0,
                subtables: vec![0],
            },
            None,
            &items,
            &mut out,
            &mut Budget {
                remaining: DEFAULT_WORK,
            },
        )
        .unwrap();
        assert_eq!(out[0].x_advance, -50);
    }

    #[test]
    fn pair_value_format_two_adjusts_second_glyph_and_advances_cursor() {
        let mut b = vec![0; 38];
        put16(&mut b, 0, 1);
        put16(&mut b, 2, 30);
        put16(&mut b, 4, 4);
        put16(&mut b, 6, 4);
        put16(&mut b, 8, 2);
        put16(&mut b, 10, 14);
        put16(&mut b, 12, 22);
        put16(&mut b, 14, 1);
        put16(&mut b, 16, 3);
        put16(&mut b, 18, (-10i16) as u16);
        put16(&mut b, 20, (-20i16) as u16);
        put16(&mut b, 22, 1);
        put16(&mut b, 24, 1);
        put16(&mut b, 26, (-30i16) as u16);
        put16(&mut b, 28, (-40i16) as u16);
        put16(&mut b, 30, 1);
        put16(&mut b, 32, 2);
        put16(&mut b, 34, 1);
        put16(&mut b, 36, 3);
        validate_subtable(&b, 0, 2, 6, &mut budget()).unwrap();
        let input = [
            GlyphCluster {
                glyph_id: 1,
                cluster: 0..1,
            },
            GlyphCluster {
                glyph_id: 3,
                cluster: 1..2,
            },
            GlyphCluster {
                glyph_id: 1,
                cluster: 2..3,
            },
        ];
        let mut out = [GlyphAdjustment::default(); 3];
        apply_gpos_lookup(
            &b,
            &Lookup {
                kind: 2,
                flags: 0,
                subtables: vec![0],
            },
            None,
            &input,
            &mut out,
            &mut Budget {
                remaining: DEFAULT_WORK,
            },
        )
        .unwrap();
        assert_eq!(out[0].x_advance, -10);
        assert_eq!(out[1].x_advance, -20);
        assert_eq!(out[2], GlyphAdjustment::default());
    }

    #[test]
    fn required_feature_deduplicates_lookup_and_extension_recursion_is_rejected() {
        let sub = vec![0, 2, 0, 8, 0, 1, 0, 2, 0, 1, 0, 1, 0, 1];
        let lookup = single_lookup(sub, 1);
        let table = one_lookup_layout(*b"liga", lookup, &[0], 0);
        let (lookups, present) = parse_layout(
            &table,
            *b"liga",
            1,
            6,
            None,
            LayoutLimits::default(),
            &mut budget(),
        )
        .unwrap();
        assert!(present);
        assert_eq!(lookups.len(), 1);
        let extension = vec![0, 1, 0, 7, 0, 0, 0, 8, 0, 1, 0, 7, 0, 0, 0, 0];
        let wrapped = single_lookup(extension, 7);
        let table = one_lookup_layout(*b"liga", wrapped, &[0], 0xffff);
        assert!(matches!(
            parse_layout(
                &table,
                *b"liga",
                1,
                6,
                None,
                LayoutLimits::default(),
                &mut budget()
            ),
            Err(LayoutError::InvalidData("recursive extension"))
        ));
    }

    #[test]
    fn validation_limit_accepts_exact_budget_and_rejects_one_less() {
        let mut kern = vec![
            0, 0, 0, 1, 0, 0, 0, 20, 0, 1, 0, 1, 0, 6, 0, 0, 0, 0, 0, 1, 0, 3,
        ];
        kern.extend_from_slice(&(-20i16).to_be_bytes());
        validate_kern(&kern, 6, &mut Budget { remaining: 2 }).unwrap();
        assert!(matches!(
            validate_kern(&kern, 6, &mut Budget { remaining: 1 }),
            Err(LayoutError::LimitExceeded(_))
        ));
    }

    #[test]
    fn single_substitution_coverage_ranges_are_budgeted_linearly() {
        let ranges = 30_000u16;
        let mut sub = vec![0, 1, 0, 6, 0, 0, 0, 2];
        b16(ranges, &mut sub);
        for glyph in 0..ranges {
            b16(glyph, &mut sub);
            b16(glyph, &mut sub);
            b16(glyph, &mut sub);
        }
        assert!(matches!(
            validate_subtable(
                &sub,
                0,
                1,
                ranges,
                &mut Budget {
                    remaining: usize::from(ranges) * 2 - 1
                }
            ),
            Err(LayoutError::LimitExceeded(_))
        ));
        validate_subtable(
            &sub,
            0,
            1,
            ranges,
            &mut Budget {
                remaining: usize::from(ranges) * 2,
            },
        )
        .unwrap();
    }

    #[test]
    fn runtime_searches_are_logarithmic_for_long_glyph_runs() {
        let count = 1_024u16;
        let mut cov1 = vec![0, 1];
        b16(count, &mut cov1);
        let mut cov2 = vec![0, 2];
        b16(count, &mut cov2);
        let mut class1 = vec![0, 1, 0, 0];
        b16(count, &mut class1);
        let mut class2 = vec![0, 2];
        b16(count, &mut class2);
        for glyph in 0..count {
            b16(glyph, &mut cov1);
            b16(glyph, &mut cov2);
            b16(glyph, &mut cov2);
            b16(glyph, &mut cov2);
            b16(glyph % 4, &mut class1);
            b16(glyph, &mut class2);
            b16(glyph, &mut class2);
            b16(glyph % 4, &mut class2);
        }
        let mut work = Budget { remaining: 50_000 };
        for glyph in 0..count {
            assert_eq!(
                coverage(&cov1, 0, glyph, &mut work).unwrap(),
                Some(usize::from(glyph))
            );
            assert_eq!(
                coverage(&cov2, 0, glyph, &mut work).unwrap(),
                Some(usize::from(glyph))
            );
            assert_eq!(class_def(&class1, 0, glyph, &mut work).unwrap(), glyph % 4);
            assert_eq!(class_def(&class2, 0, glyph, &mut work).unwrap(), glyph % 4);
        }
    }

    #[test]
    fn gdef_version_headers_and_ignore_marks_class_definition_are_checked() {
        for (version, accepted_len) in
            [(0x0001_0000, 12usize), (0x0001_0002, 14), (0x0001_0003, 18)]
        {
            let version = version as u32;
            let mut header = vec![0; accepted_len];
            header[..4].copy_from_slice(&version.to_be_bytes());
            assert_eq!(gdef_header(&header).unwrap().1, version);
            if accepted_len > 12 {
                assert_eq!(
                    gdef_header(&header[..accepted_len - 1]),
                    Err(LayoutError::Truncated)
                );
            }
        }
        let mut lookup = single_lookup(vec![0, 2, 0, 8, 0, 1, 0, 2, 0, 1, 0, 1, 0, 1], 1);
        lookup[3] = 8;
        let table = one_lookup_layout(*b"liga", lookup, &[0], 0xffff);
        let mut gdef = vec![0; 12];
        put16(&mut gdef, 0, 1);
        assert!(matches!(
            parse_layout(
                &table,
                *b"liga",
                1,
                6,
                Some(&gdef),
                LayoutLimits::default(),
                &mut budget()
            ),
            Err(LayoutError::InvalidData(
                "IgnoreMarks requires GlyphClassDef"
            ))
        ));
    }

    #[test]
    fn legacy_kern_rejects_nonzero_subtable_version() {
        let kern = [0, 0, 0, 1, 0, 1, 0, 6, 0, 0];
        assert!(matches!(
            validate_kern(&kern, 6, &mut budget()),
            Err(LayoutError::Unsupported("kern subtable version"))
        ));
    }
}
