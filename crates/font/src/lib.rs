// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded readers for the core OpenType tables used by the text engine.
//!
//! The parser borrows all table data from the input. It validates the complete
//! directory and every supported cmap subtable before exposing a face.

#![forbid(unsafe_code)]

pub mod embedded;
pub mod layout;
pub mod outline;
pub use layout::{FontLayout, GlyphAdjustment, GlyphCluster, LayoutError, LayoutLimits};
pub use outline::{OutlineError, OutlineLimits};

use std::collections::BTreeMap;

const MAX_TABLES: usize = 4096;
const MAX_NAME_RECORDS: usize = 16_384;
const MAX_CMAP_RECORDS: usize = 1024;
const MAX_CMAP_GROUPS: usize = 1_000_000;
const MAX_FONT_BYTES: usize = 256 * 1024 * 1024;
const MAX_DECODED_NAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_CMAP_VALIDATION_WORK: usize = 1_000_000;

#[derive(Debug)]
struct ValidationBudget {
    remaining: usize,
}
impl ValidationBudget {
    fn spend(&mut self, amount: usize) -> Result<(), FontError> {
        self.remaining = self
            .remaining
            .checked_sub(amount)
            .ok_or(FontError::LimitExceeded("cmap validation work"))?;
        Ok(())
    }
}

/// Errors returned for malformed or unsupported font data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontError {
    Unsupported(&'static str),
    InvalidSignature,
    InvalidVersion(&'static str),
    Truncated,
    InvalidRange(&'static str),
    InvalidData(&'static str),
    LimitExceeded(&'static str),
    FaceIndexOutOfRange,
    MissingTable(&'static str),
}

impl core::fmt::Display for FontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported font feature: {s}"),
            Self::InvalidSignature => f.write_str("invalid sfnt/TTC signature"),
            Self::InvalidVersion(s) => write!(f, "invalid {s} version"),
            Self::Truncated => f.write_str("truncated font data"),
            Self::InvalidRange(s) => write!(f, "invalid font range: {s}"),
            Self::InvalidData(s) => write!(f, "inconsistent font data: {s}"),
            Self::LimitExceeded(s) => write!(f, "font resource limit exceeded: {s}"),
            Self::FaceIndexOutOfRange => f.write_str("font face index is out of range"),
            Self::MissingTable(s) => write!(f, "required font table is missing: {s}"),
        }
    }
}
impl std::error::Error for FontError {}

/// Horizontal advance and left side bearing for a glyph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HorizontalMetric {
    pub advance_width: u16,
    pub left_side_bearing: i16,
}

/// Font-wide metrics exposed by the core tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub index_to_loc_format: i16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub number_of_glyphs: u16,
    pub number_of_h_metrics: u16,
    pub weight_class: Option<u16>,
    pub width_class: Option<u16>,
    pub fs_type: Option<u16>,
    pub post_format: u32,
}

/// One decoded OpenType name record. Invalid UTF-16 units are replaced with U+FFFD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameRecord {
    pub platform_id: u16,
    pub encoding_id: u16,
    pub language_id: u16,
    pub name_id: u16,
    pub value: String,
}

/// Outcome of resolving a base character and variation selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariationGlyph {
    /// The selector has no variation mapping; use the base cmap mapping.
    Default,
    /// An explicit glyph mapping is present.
    NonDefault(u16),
    /// No format 14 mapping exists for this selector/base pair.
    Unsupported,
}

#[derive(Clone, Copy, Debug)]
struct Table<'a> {
    bytes: &'a [u8],
}

#[derive(Clone, Debug)]
enum CmapSubtable<'a> {
    Format4 {
        bytes: &'a [u8],
        offset: usize,
        length: usize,
    },
    Format6 {
        bytes: &'a [u8],
        offset: usize,
        length: usize,
    },
    Format12 {
        bytes: &'a [u8],
        offset: usize,
        length: usize,
    },
    Format14 {
        bytes: &'a [u8],
        offset: usize,
        length: usize,
    },
}

/// A parsed font face. All source data remains borrowed for the face lifetime.
#[derive(Debug)]
pub struct FontFace<'a> {
    bytes: &'a [u8],
    tables: BTreeMap<[u8; 4], Table<'a>>,
    cmap: Vec<CmapSubtable<'a>>,
    variation: Option<CmapSubtable<'a>>,
    names: Vec<NameRecord>,
    metrics: FontMetrics,
}

impl<'a> FontFace<'a> {
    /// Parse face zero of an sfnt font, or face zero of a TTC collection.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FontError> {
        Self::parse_collection(bytes, 0)
    }

    /// Parse one face from an sfnt font or TrueType collection.
    pub fn parse_collection(bytes: &'a [u8], face_index: usize) -> Result<Self, FontError> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(FontError::LimitExceeded("font bytes"));
        }
        let signature = u32_at(bytes, 0).ok_or(FontError::Truncated)?;
        let directory = if signature == tag(b"ttcf") {
            let version = u32_at(bytes, 4).ok_or(FontError::Truncated)?;
            if version != 0x0001_0000 && version != 0x0002_0000 {
                return Err(FontError::InvalidVersion("TTC"));
            }
            let count = usize::try_from(u32_at(bytes, 8).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::LimitExceeded("face count"))?;
            if count == 0 || count > 4096 {
                return Err(FontError::LimitExceeded("face count"));
            }
            let offsets_end = 12usize
                .checked_add(
                    count
                        .checked_mul(4)
                        .ok_or(FontError::InvalidRange("TTC face offsets"))?,
                )
                .ok_or(FontError::InvalidRange("TTC face offsets"))?;
            if offsets_end > bytes.len() {
                return Err(FontError::Truncated);
            }
            let header_end = if version == 0x0002_0000 {
                let dsig = offsets_end
                    .checked_add(12)
                    .ok_or(FontError::InvalidRange("TTC DSIG"))?;
                if dsig > bytes.len() {
                    return Err(FontError::Truncated);
                }
                let length =
                    usize::try_from(u32_at(bytes, offsets_end + 4).ok_or(FontError::Truncated)?)
                        .map_err(|_| FontError::InvalidRange("TTC DSIG length"))?;
                let offset =
                    usize::try_from(u32_at(bytes, offsets_end + 8).ok_or(FontError::Truncated)?)
                        .map_err(|_| FontError::InvalidRange("TTC DSIG offset"))?;
                let signature = u32_at(bytes, offsets_end).ok_or(FontError::Truncated)?;
                if length == 0 {
                    if offset != 0 || signature != 0 {
                        return Err(FontError::InvalidRange("TTC DSIG envelope"));
                    }
                } else if signature != 0x4453_4947
                    || offset
                        .checked_add(length)
                        .filter(|&end| end <= bytes.len())
                        .is_none()
                {
                    return Err(FontError::InvalidRange("TTC DSIG envelope"));
                }
                dsig
            } else {
                offsets_end
            };
            for i in 0..count {
                let off = usize::try_from(u32_at(bytes, 12 + i * 4).ok_or(FontError::Truncated)?)
                    .map_err(|_| FontError::InvalidRange("TTC face offset"))?;
                if off < header_end
                    || off
                        .checked_add(12)
                        .filter(|&end| end <= bytes.len())
                        .is_none()
                    || !matches!(
                        u32_at(bytes, off),
                        Some(0x0001_0000 | 0x4f54_544f | 0x7472_7565 | 0x7479_7031)
                    )
                {
                    return Err(FontError::InvalidRange("TTC face offset"));
                }
            }
            if face_index >= count {
                return Err(FontError::FaceIndexOutOfRange);
            }
            let entry = 12usize
                .checked_add(
                    face_index
                        .checked_mul(4)
                        .ok_or(FontError::InvalidRange("TTC face offsets"))?,
                )
                .ok_or(FontError::InvalidRange("TTC face offsets"))?;
            usize::try_from(u32_at(bytes, entry).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("TTC face offset"))?
        } else {
            if face_index != 0 {
                return Err(FontError::FaceIndexOutOfRange);
            }
            if !matches!(
                signature,
                0x0001_0000 | 0x4f54_544f | 0x7472_7565 | 0x7479_7031
            ) {
                return Err(FontError::InvalidSignature);
            }
            0
        };
        let sfnt_signature = u32_at(bytes, directory).ok_or(FontError::Truncated)?;
        if !matches!(
            sfnt_signature,
            0x0001_0000 | 0x4f54_544f | 0x7472_7565 | 0x7479_7031
        ) {
            return Err(FontError::InvalidSignature);
        }
        let count = usize::from(u16_at(bytes, directory + 4).ok_or(FontError::Truncated)?);
        if count == 0 || count > MAX_TABLES {
            return Err(FontError::LimitExceeded("table count"));
        }
        let records = directory
            .checked_add(12)
            .ok_or(FontError::InvalidRange("table directory"))?;
        let dir_end = records
            .checked_add(
                count
                    .checked_mul(16)
                    .ok_or(FontError::InvalidRange("table directory"))?,
            )
            .ok_or(FontError::InvalidRange("table directory"))?;
        if dir_end > bytes.len() {
            return Err(FontError::Truncated);
        }
        let mut tables = BTreeMap::new();
        for i in 0..count {
            let p = records + i * 16;
            let name: [u8; 4] = bytes
                .get(p..p + 4)
                .ok_or(FontError::Truncated)?
                .try_into()
                .map_err(|_| FontError::Truncated)?;
            let offset = usize::try_from(u32_at(bytes, p + 8).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("table offset"))?;
            let length = usize::try_from(u32_at(bytes, p + 12).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("table length"))?;
            let end = offset
                .checked_add(length)
                .ok_or(FontError::InvalidRange("table range"))?;
            let table = bytes
                .get(offset..end)
                .ok_or(FontError::InvalidRange("table range"))?;
            if tables.insert(name, Table { bytes: table }).is_some() {
                return Err(FontError::InvalidData("duplicate table tag"));
            }
        }
        for t in [
            b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cmap",
        ] {
            if !tables.contains_key(t.as_slice()) {
                return Err(FontError::MissingTable(
                    std::str::from_utf8(t).unwrap_or("table"),
                ));
            }
        }
        let head = tables[b"head".as_slice()].bytes;
        let hhea = tables[b"hhea".as_slice()].bytes;
        let maxp = tables[b"maxp".as_slice()].bytes;
        let os2 = tables[b"OS/2".as_slice()].bytes;
        let post = tables[b"post".as_slice()].bytes;
        if head.len() < 54 || hhea.len() < 36 || maxp.len() < 6 || os2.len() < 2 || post.len() < 32
        {
            return Err(FontError::Truncated);
        }
        if u32_at(head, 0) != Some(0x0001_0000) {
            return Err(FontError::InvalidVersion("head"));
        }
        if u32_at(head, 12) != Some(0x5f0f_3cf5) {
            return Err(FontError::InvalidData("head magic number"));
        }
        if u32_at(hhea, 0) != Some(0x0001_0000) {
            return Err(FontError::InvalidVersion("hhea"));
        }
        if !matches!(u32_at(maxp, 0), Some(0x0000_5000 | 0x0001_0000)) {
            return Err(FontError::InvalidVersion("maxp"));
        }
        if u32_at(maxp, 0) == Some(0x0001_0000) && maxp.len() < 32 {
            return Err(FontError::Truncated);
        }
        let units_per_em = u16_at(head, 18).ok_or(FontError::Truncated)?;
        if !(16..=16384).contains(&units_per_em) {
            return Err(FontError::InvalidData("unitsPerEm outside 16..=16384"));
        }
        let glyphs = u16_at(maxp, 4).ok_or(FontError::Truncated)?;
        validate_post(post, glyphs)?;
        let hmetrics = u16_at(hhea, 34).ok_or(FontError::Truncated)?;
        if glyphs == 0 || hmetrics == 0 || hmetrics > glyphs {
            return Err(FontError::InvalidData("glyph or horizontal metric count"));
        }
        let hmtx = tables[b"hmtx".as_slice()].bytes;
        let required_hmtx = usize::from(hmetrics)
            .checked_mul(4)
            .and_then(|n| n.checked_add(usize::from(glyphs - hmetrics) * 2))
            .ok_or(FontError::InvalidRange("hmtx length"))?;
        if hmtx.len() < required_hmtx {
            return Err(FontError::InvalidRange("hmtx length"));
        }
        let os2_version = u16_at(os2, 0).ok_or(FontError::Truncated)?;
        let min_os2 = match os2_version {
            0 => 78,
            1 => 86,
            2..=4 => 96,
            5 => 100,
            _ => return Err(FontError::InvalidVersion("OS/2")),
        };
        if os2.len() < min_os2 {
            return Err(FontError::Truncated);
        }
        let metrics = FontMetrics {
            units_per_em,
            index_to_loc_format: i16_at(head, 50).ok_or(FontError::Truncated)?,
            ascender: i16_at(hhea, 4).ok_or(FontError::Truncated)?,
            descender: i16_at(hhea, 6).ok_or(FontError::Truncated)?,
            line_gap: i16_at(hhea, 8).ok_or(FontError::Truncated)?,
            number_of_glyphs: glyphs,
            number_of_h_metrics: hmetrics,
            weight_class: u16_at(os2, 4),
            width_class: u16_at(os2, 6),
            fs_type: u16_at(os2, 8),
            post_format: u32_at(post, 0).ok_or(FontError::Truncated)?,
        };
        if !matches!(metrics.index_to_loc_format, 0 | 1) {
            return Err(FontError::InvalidData("indexToLocFormat"));
        }
        let names = parse_names(tables[b"name".as_slice()].bytes)?;
        let (cmap, variation) = parse_cmap(tables[b"cmap".as_slice()].bytes, glyphs)?;
        Ok(Self {
            bytes,
            tables,
            cmap,
            variation,
            names,
            metrics,
        })
    }

    /// Returns validated raw bytes for a table, if present.
    pub fn table(&self, tag: [u8; 4]) -> Option<&'a [u8]> {
        self.tables.get(&tag).map(|t| t.bytes)
    }
    /// Returns the complete source buffer used to create this face.
    pub fn source(&self) -> &'a [u8] {
        self.bytes
    }
    /// Font metrics from head, hhea, maxp, OS/2 and post.
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }
    /// Decoded name records. Invalid UTF-16 data is represented with U+FFFD.
    pub fn names(&self) -> &[NameRecord] {
        &self.names
    }

    /// Resolve a Unicode scalar using format 12 preferentially, then 4, then 6.
    /// Missing mappings and invalid scalar values map to glyph zero (.notdef).
    pub fn glyph_index(&self, character: char) -> u16 {
        let cp = character as u32;
        for format in [12, 4, 6] {
            for subtable in &self.cmap {
                let found = match subtable {
                    CmapSubtable::Format12 {
                        bytes,
                        offset,
                        length,
                    } if format == 12 => lookup12(bytes, *offset, *length, cp),
                    CmapSubtable::Format4 {
                        bytes,
                        offset,
                        length,
                    } if format == 4 && cp <= 0xffff => lookup4(bytes, *offset, *length, cp as u16),
                    CmapSubtable::Format6 {
                        bytes,
                        offset,
                        length,
                    } if format == 6 && cp <= 0xffff => lookup6(bytes, *offset, *length, cp as u16),
                    _ => None,
                };
                if let Some(gid) = found {
                    return if gid < self.metrics.number_of_glyphs {
                        gid
                    } else {
                        0
                    };
                }
            }
        }
        0
    }

    /// Resolve a base character and variation selector through cmap format 14.
    pub fn variation_glyph(&self, base: char, selector: char) -> VariationGlyph {
        let Some(CmapSubtable::Format14 {
            bytes,
            offset,
            length,
        }) = &self.variation
        else {
            return VariationGlyph::Unsupported;
        };
        let end = offset.saturating_add(*length);
        let Some(sel_count) = u32_at(bytes, offset + 6).and_then(|n| usize::try_from(n).ok())
        else {
            return VariationGlyph::Unsupported;
        };
        for i in 0..sel_count {
            let p = offset + 10 + i * 11;
            if p + 11 > end {
                return VariationGlyph::Unsupported;
            }
            let Some(sel) = u24_at(bytes, p) else {
                return VariationGlyph::Unsupported;
            };
            if sel != selector as u32 {
                continue;
            }
            let default_off = u32_at(bytes, p + 3).unwrap_or(0) as usize;
            let nondefault_off = u32_at(bytes, p + 7).unwrap_or(0) as usize;
            if nondefault_off != 0 {
                let q = offset.saturating_add(nondefault_off);
                let Some(count) = u32_at(bytes, q).and_then(|n| usize::try_from(n).ok()) else {
                    return VariationGlyph::Unsupported;
                };
                for j in 0..count {
                    let r = q.saturating_add(4 + j * 5);
                    if r + 5 > end {
                        return VariationGlyph::Unsupported;
                    }
                    if u24_at(bytes, r) == Some(base as u32) {
                        let gid = u16_at(bytes, r + 3).unwrap_or(0);
                        return if gid < self.metrics.number_of_glyphs {
                            VariationGlyph::NonDefault(gid)
                        } else {
                            VariationGlyph::Unsupported
                        };
                    }
                }
            }
            if default_off != 0 {
                let q = offset.saturating_add(default_off);
                let Some(count) = u32_at(bytes, q).and_then(|n| usize::try_from(n).ok()) else {
                    return VariationGlyph::Unsupported;
                };
                for j in 0..count {
                    let r = q.saturating_add(4 + j * 4);
                    if r + 4 > end {
                        return VariationGlyph::Unsupported;
                    }
                    let start = u24_at(bytes, r).unwrap_or(u32::MAX);
                    let last = start.saturating_add(u32::from(*bytes.get(r + 3).unwrap_or(&0)));
                    if (start..=last).contains(&(base as u32)) {
                        return VariationGlyph::Default;
                    }
                }
            }
            return VariationGlyph::Unsupported;
        }
        VariationGlyph::Unsupported
    }

    /// Return the horizontal metric, including repeated advances after the last long metric.
    pub fn horizontal_metric(&self, glyph_id: u16) -> Result<HorizontalMetric, FontError> {
        if glyph_id >= self.metrics.number_of_glyphs {
            return Err(FontError::InvalidData("glyph id outside numGlyphs"));
        }
        let b = self.tables[b"hmtx".as_slice()].bytes;
        let n = self.metrics.number_of_h_metrics;
        if glyph_id < n {
            let p = usize::from(glyph_id) * 4;
            Ok(HorizontalMetric {
                advance_width: u16_at(b, p).ok_or(FontError::Truncated)?,
                left_side_bearing: i16_at(b, p + 2).ok_or(FontError::Truncated)?,
            })
        } else {
            let advance = u16_at(b, usize::from(n - 1) * 4).ok_or(FontError::Truncated)?;
            let p = usize::from(n) * 4 + usize::from(glyph_id - n) * 2;
            Ok(HorizontalMetric {
                advance_width: advance,
                left_side_bearing: i16_at(b, p).ok_or(FontError::Truncated)?,
            })
        }
    }
}

fn tag(b: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*b)
}
fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn i16_at(b: &[u8], p: usize) -> Option<i16> {
    Some(i16::from_be_bytes(
        b.get(p..p.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(p..p.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u24_at(b: &[u8], p: usize) -> Option<u32> {
    Some(
        (u32::from(*b.get(p)?) << 16)
            | (u32::from(*b.get(p + 1)?) << 8)
            | u32::from(*b.get(p + 2)?),
    )
}

fn validate_post(b: &[u8], glyphs: u16) -> Result<(), FontError> {
    let format = u32_at(b, 0).ok_or(FontError::Truncated)?;
    if b.len() < 32 {
        return Err(FontError::Truncated);
    }
    match format {
        0x0001_0000 | 0x0003_0000 => Ok(()),
        0x0002_0000 => {
            if b.len() < 34 {
                return Err(FontError::Truncated);
            }
            let count = usize::from(u16_at(b, 32).ok_or(FontError::Truncated)?);
            if count != usize::from(glyphs) {
                return Err(FontError::InvalidData(
                    "post glyph count disagrees with maxp",
                ));
            }
            let names_end = 34usize
                .checked_add(
                    count
                        .checked_mul(2)
                        .ok_or(FontError::InvalidRange("post glyph names"))?,
                )
                .ok_or(FontError::InvalidRange("post glyph names"))?;
            if names_end > b.len() {
                return Err(FontError::Truncated);
            }
            let mut custom_count = 0usize;
            for i in 0..count {
                let index = usize::from(u16_at(b, 34 + i * 2).ok_or(FontError::Truncated)?);
                if index > 32767 {
                    return Err(FontError::InvalidData("post glyph name index"));
                }
                if index > 257 {
                    custom_count = custom_count.max(index - 257);
                }
            }
            let mut p = names_end;
            for _ in 0..custom_count {
                let len = usize::from(*b.get(p).ok_or(FontError::Truncated)?);
                p = p
                    .checked_add(1 + len)
                    .filter(|&end| end <= b.len())
                    .ok_or(FontError::InvalidRange("post custom name"))?;
            }
            Ok(())
        }
        0x0002_5000 => {
            if b.len() < 34 {
                return Err(FontError::Truncated);
            }
            let count = usize::from(u16_at(b, 32).ok_or(FontError::Truncated)?);
            if count != usize::from(glyphs) {
                return Err(FontError::InvalidData(
                    "post glyph count disagrees with maxp",
                ));
            }
            let end = 34usize
                .checked_add(count)
                .ok_or(FontError::InvalidRange("post glyph names"))?;
            let offsets = b.get(34..end).ok_or(FontError::Truncated)?;
            for (i, &delta) in offsets.iter().enumerate() {
                let index = i as i32 + i32::from(delta as i8);
                if !(0..=257).contains(&index) {
                    return Err(FontError::InvalidData("post glyph name index"));
                }
            }
            Ok(())
        }
        _ => Err(FontError::InvalidVersion("post")),
    }
}

fn parse_names(b: &[u8]) -> Result<Vec<NameRecord>, FontError> {
    if b.len() < 6 {
        return Err(FontError::Truncated);
    }
    let format = u16_at(b, 0).ok_or(FontError::Truncated)?;
    if format > 1 {
        return Err(FontError::InvalidVersion("name"));
    }
    let count = usize::from(u16_at(b, 2).ok_or(FontError::Truncated)?);
    if count > MAX_NAME_RECORDS {
        return Err(FontError::LimitExceeded("name records"));
    }
    let string_offset = usize::from(u16_at(b, 4).ok_or(FontError::Truncated)?);
    let name_records_end = 6usize
        .checked_add(
            count
                .checked_mul(12)
                .ok_or(FontError::InvalidRange("name records"))?,
        )
        .ok_or(FontError::InvalidRange("name records"))?;
    if name_records_end > b.len() || string_offset < name_records_end || string_offset > b.len() {
        return Err(FontError::InvalidRange("name strings"));
    }
    let records_end = if format == 1 {
        let n = usize::from(u16_at(b, name_records_end).ok_or(FontError::Truncated)?);
        let end = name_records_end
            .checked_add(2)
            .and_then(|v| v.checked_add(n.checked_mul(4)?))
            .ok_or(FontError::InvalidRange("name language records"))?;
        if end > string_offset {
            return Err(FontError::InvalidRange("name language records"));
        }
        for i in 0..n {
            let p = name_records_end + 2 + i * 4;
            let len = usize::from(u16_at(b, p).ok_or(FontError::Truncated)?);
            let off = usize::from(u16_at(b, p + 2).ok_or(FontError::Truncated)?);
            if len % 2 != 0 {
                return Err(FontError::InvalidData("odd UTF-16 language tag length"));
            }
            let start = string_offset
                .checked_add(off)
                .ok_or(FontError::InvalidRange("name language tag"))?;
            let stop = start
                .checked_add(len)
                .ok_or(FontError::InvalidRange("name language tag"))?;
            if b.get(start..stop).is_none() {
                return Err(FontError::InvalidRange("name language tag"));
            }
        }
        end
    } else {
        name_records_end
    };
    if records_end > string_offset {
        return Err(FontError::InvalidRange("name strings"));
    }
    let mut out = Vec::with_capacity(count);
    let mut decoded_bytes = 0usize;
    for i in 0..count {
        let p = 6 + i * 12;
        let platform_id = u16_at(b, p).ok_or(FontError::Truncated)?;
        let encoding_id = u16_at(b, p + 2).ok_or(FontError::Truncated)?;
        let language_id = u16_at(b, p + 4).ok_or(FontError::Truncated)?;
        let name_id = u16_at(b, p + 6).ok_or(FontError::Truncated)?;
        let len = usize::from(u16_at(b, p + 8).ok_or(FontError::Truncated)?);
        let off = usize::from(u16_at(b, p + 10).ok_or(FontError::Truncated)?);
        let start = string_offset
            .checked_add(off)
            .ok_or(FontError::InvalidRange("name string"))?;
        let end = start
            .checked_add(len)
            .ok_or(FontError::InvalidRange("name string"))?;
        let raw = b
            .get(start..end)
            .ok_or(FontError::InvalidRange("name string"))?;
        let utf16 = platform_id == 0 || (platform_id == 3 && !matches!(encoding_id, 3..=5));
        if utf16 && len % 2 != 0 {
            return Err(FontError::InvalidData("odd UTF-16 name length"));
        }
        decoded_bytes = decoded_bytes
            .checked_add(raw.len())
            .filter(|&n| n <= MAX_DECODED_NAME_BYTES)
            .ok_or(FontError::LimitExceeded("decoded name bytes"))?;
        let value = if utf16 {
            let units = raw
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]));
            String::from_utf16_lossy(&units.collect::<Vec<_>>())
        } else {
            raw.iter().map(|&v| char::from(v)).collect()
        };
        out.push(NameRecord {
            platform_id,
            encoding_id,
            language_id,
            name_id,
            value,
        });
    }
    Ok(out)
}

fn parse_cmap(
    b: &[u8],
    glyphs: u16,
) -> Result<(Vec<CmapSubtable<'_>>, Option<CmapSubtable<'_>>), FontError> {
    if b.len() < 4 {
        return Err(FontError::Truncated);
    }
    if u16_at(b, 0) != Some(0) {
        return Err(FontError::InvalidVersion("cmap"));
    }
    let count = usize::from(u16_at(b, 2).ok_or(FontError::Truncated)?);
    if count == 0 || count > MAX_CMAP_RECORDS {
        return Err(FontError::LimitExceeded("cmap records"));
    }
    if 4usize
        .checked_add(
            count
                .checked_mul(8)
                .ok_or(FontError::InvalidRange("cmap records"))?,
        )
        .filter(|&e| e <= b.len())
        .is_none()
    {
        return Err(FontError::Truncated);
    }
    let mut chosen: BTreeMap<u16, (u8, CmapSubtable<'_>)> = BTreeMap::new();
    let mut variation = None;
    let mut validated = BTreeMap::<usize, (u16, CmapSubtable<'_>)>::new();
    let mut budget = ValidationBudget {
        remaining: MAX_CMAP_VALIDATION_WORK,
    };
    for i in 0..count {
        let p = 4 + i * 8;
        let platform = u16_at(b, p).ok_or(FontError::Truncated)?;
        let enc = u16_at(b, p + 2).ok_or(FontError::Truncated)?;
        let off = usize::try_from(u32_at(b, p + 4).ok_or(FontError::Truncated)?)
            .map_err(|_| FontError::InvalidRange("cmap offset"))?;
        let fmt = u16_at(b, off).ok_or(FontError::InvalidRange("cmap subtable offset"))?;
        if fmt == 14 && (platform != 0 || enc != 5) {
            continue;
        }
        if let Some((cached_fmt, sub)) = validated.get(&off) {
            if *cached_fmt == 14 {
                if variation.is_none() {
                    variation = Some(sub.clone());
                }
            } else {
                let rank = cmap_rank(platform, enc);
                if rank > 0 && chosen.get(cached_fmt).is_none_or(|(old, _)| rank > *old) {
                    chosen.insert(*cached_fmt, (rank, sub.clone()));
                }
            }
            continue;
        }
        let length = match fmt {
            4 | 6 => usize::from(u16_at(b, off + 2).ok_or(FontError::Truncated)?),
            12 => usize::try_from(u32_at(b, off + 4).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("cmap length"))?,
            14 => usize::try_from(u32_at(b, off + 2).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("cmap length"))?,
            _ => continue,
        };
        let end = off
            .checked_add(length)
            .ok_or(FontError::InvalidRange("cmap subtable"))?;
        if end > b.len() {
            return Err(FontError::InvalidRange("cmap subtable"));
        }
        let sub = match fmt {
            4 => {
                validate4(b, off, length, glyphs, &mut budget)?;
                CmapSubtable::Format4 {
                    bytes: b,
                    offset: off,
                    length,
                }
            }
            6 => {
                validate6(b, off, length, glyphs, &mut budget)?;
                CmapSubtable::Format6 {
                    bytes: b,
                    offset: off,
                    length,
                }
            }
            12 => {
                validate12(b, off, length, glyphs, &mut budget)?;
                CmapSubtable::Format12 {
                    bytes: b,
                    offset: off,
                    length,
                }
            }
            14 => {
                validate14(b, off, length, glyphs, &mut budget)?;
                if variation.is_none() {
                    variation = Some(CmapSubtable::Format14 {
                        bytes: b,
                        offset: off,
                        length,
                    });
                }
                validated.insert(
                    off,
                    (
                        14,
                        CmapSubtable::Format14 {
                            bytes: b,
                            offset: off,
                            length,
                        },
                    ),
                );
                continue;
            }
            _ => continue,
        };
        validated.insert(off, (fmt, sub.clone()));
        let rank = cmap_rank(platform, enc);
        if rank > 0 && chosen.get(&fmt).is_none_or(|(old, _)| rank > *old) {
            chosen.insert(fmt, (rank, sub));
        }
    }
    if chosen.is_empty() {
        return Err(FontError::Unsupported("no supported Unicode cmap subtable"));
    }
    let selected = [12, 4, 6]
        .into_iter()
        .find_map(|fmt| chosen.remove(&fmt).map(|(_, s)| s))
        .ok_or(FontError::Unsupported("no supported Unicode cmap subtable"))?;
    Ok((vec![selected], variation))
}

fn cmap_rank(platform: u16, enc: u16) -> u8 {
    if platform == 3 && enc == 10 {
        4
    } else if platform == 0 {
        3
    } else if platform == 3 && enc == 1 {
        2
    } else if platform == 3 && enc == 0 {
        1
    } else {
        0
    }
}

fn validate4(
    b: &[u8],
    o: usize,
    l: usize,
    glyphs: u16,
    budget: &mut ValidationBudget,
) -> Result<(), FontError> {
    if l < 16 {
        return Err(FontError::Truncated);
    }
    let seg_count_x2 = u16_at(b, o + 6).ok_or(FontError::Truncated)?;
    if seg_count_x2 % 2 != 0 {
        return Err(FontError::InvalidData("cmap format 4 segment count"));
    }
    let n = usize::from(seg_count_x2 / 2);
    if n == 0 || n > 8192 {
        return Err(FontError::InvalidData("cmap format 4 segment count"));
    }
    let need = 16usize
        .checked_add(n.checked_mul(8).ok_or(FontError::InvalidRange("cmap 4"))?)
        .ok_or(FontError::InvalidRange("cmap 4"))?;
    if need > l {
        return Err(FontError::Truncated);
    }
    let endp = o + 14;
    let startp = endp + n * 2 + 2;
    let deltap = startp + n * 2;
    let rangep = deltap + n * 2;
    let mut prev = 0;
    budget.spend(n)?;
    for i in 0..n {
        let e = u16_at(b, endp + i * 2).ok_or(FontError::Truncated)?;
        let s = u16_at(b, startp + i * 2).ok_or(FontError::Truncated)?;
        let ro = u16_at(b, rangep + i * 2).ok_or(FontError::Truncated)?;
        if s > e || (i > 0 && s <= prev) {
            return Err(FontError::InvalidData("cmap format 4 segments"));
        }
        prev = e;
        if ro != 0 && s <= e {
            let first = rangep + i * 2 + usize::from(ro);
            let last = first + usize::from(e - s) * 2;
            if last + 2 > o + l {
                return Err(FontError::InvalidRange("cmap format 4 glyph array"));
            }
            for q in (first..=last).step_by(2) {
                budget.spend(1)?;
                let gid = u16_at(b, q).ok_or(FontError::Truncated)?;
                if gid != 0 {
                    let delta = i16_at(b, deltap + i * 2).ok_or(FontError::Truncated)?;
                    let mapped = gid.wrapping_add(delta as u16);
                    if mapped >= glyphs {
                        return Err(FontError::InvalidData("cmap glyph index"));
                    }
                }
            }
        }
    }
    if u16_at(b, endp + (n - 1) * 2) != Some(u16::MAX) {
        return Err(FontError::InvalidData("cmap format 4 missing sentinel"));
    }
    Ok(())
}
fn validate6(
    b: &[u8],
    o: usize,
    l: usize,
    glyphs: u16,
    budget: &mut ValidationBudget,
) -> Result<(), FontError> {
    if l < 10 {
        return Err(FontError::Truncated);
    }
    let first = usize::from(u16_at(b, o + 6).ok_or(FontError::Truncated)?);
    let n = usize::from(u16_at(b, o + 8).ok_or(FontError::Truncated)?);
    if 10 + n * 2 > l || first + n > 65536 {
        return Err(FontError::InvalidRange("cmap format 6"));
    }
    budget.spend(n)?;
    for i in 0..n {
        let g = u16_at(b, o + 10 + i * 2).ok_or(FontError::Truncated)?;
        if g >= glyphs {
            return Err(FontError::InvalidData("cmap glyph index"));
        }
    }
    Ok(())
}
fn validate12(
    b: &[u8],
    o: usize,
    l: usize,
    glyphs: u16,
    budget: &mut ValidationBudget,
) -> Result<(), FontError> {
    if l < 16 {
        return Err(FontError::Truncated);
    }
    if u16_at(b, o + 2) != Some(0) {
        return Err(FontError::InvalidData("cmap format 12 reserved"));
    }
    let n = usize::try_from(u32_at(b, o + 12).ok_or(FontError::Truncated)?)
        .map_err(|_| FontError::LimitExceeded("cmap groups"))?;
    if n > MAX_CMAP_GROUPS
        || 16usize
            .checked_add(
                n.checked_mul(12)
                    .ok_or(FontError::InvalidRange("cmap 12"))?,
            )
            .filter(|&v| v <= l)
            .is_none()
    {
        return Err(FontError::InvalidRange("cmap format 12 groups"));
    }
    budget.spend(n)?;
    let mut prev = 0;
    for i in 0..n {
        let p = o + 16 + i * 12;
        let s = u32_at(b, p).ok_or(FontError::Truncated)?;
        let e = u32_at(b, p + 4).ok_or(FontError::Truncated)?;
        let g = u32_at(b, p + 8).ok_or(FontError::Truncated)?;
        if s > e
            || e > 0x10ffff
            || (i > 0 && s <= prev)
            || g.checked_add(e - s).is_none_or(|v| v >= u32::from(glyphs))
        {
            return Err(FontError::InvalidData("cmap format 12 group"));
        }
        prev = e;
    }
    Ok(())
}
fn validate14(
    b: &[u8],
    o: usize,
    l: usize,
    glyphs: u16,
    budget: &mut ValidationBudget,
) -> Result<(), FontError> {
    if l < 10 {
        return Err(FontError::Truncated);
    }
    let n = usize::try_from(u32_at(b, o + 6).ok_or(FontError::Truncated)?)
        .map_err(|_| FontError::LimitExceeded("variation selectors"))?;
    if n > MAX_CMAP_GROUPS
        || 10usize
            .checked_add(
                n.checked_mul(11)
                    .ok_or(FontError::InvalidRange("cmap 14"))?,
            )
            .filter(|&v| v <= l)
            .is_none()
    {
        return Err(FontError::InvalidRange("cmap format 14 selectors"));
    }
    let records_end = o
        .checked_add(10 + n * 11)
        .ok_or(FontError::InvalidRange("cmap 14 selectors"))?;
    let mut previous_selector = None;
    let mut seen_ranges = BTreeMap::new();
    let mut seen_mappings = BTreeMap::new();
    budget.spend(n)?;
    for i in 0..n {
        let p = o + 10 + i * 11;
        let selector = u24_at(b, p).ok_or(FontError::Truncated)?;
        if !is_variation_selector(selector)
            || previous_selector.is_some_and(|prev| selector <= prev)
        {
            return Err(FontError::InvalidData(
                "cmap format 14 selector order or value",
            ));
        }
        previous_selector = Some(selector);
        for (q, is_default) in [(p + 3, true), (p + 7, false)] {
            let rel = usize::try_from(u32_at(b, q).ok_or(FontError::Truncated)?)
                .map_err(|_| FontError::InvalidRange("cmap 14 offset"))?;
            if rel == 0 {
                continue;
            }
            let start = o
                .checked_add(rel)
                .ok_or(FontError::InvalidRange("cmap 14 offset"))?;
            if start < records_end {
                return Err(FontError::InvalidRange("cmap 14 offset into header"));
            }
            let cache = if is_default {
                &mut seen_ranges
            } else {
                &mut seen_mappings
            };
            if cache.contains_key(&start) {
                continue;
            }
            let count =
                usize::try_from(u32_at(b, start).ok_or(FontError::InvalidRange("cmap 14 data"))?)
                    .map_err(|_| FontError::LimitExceeded("cmap 14 mappings"))?;
            let item = if is_default { 4 } else { 5 };
            if count > MAX_CMAP_GROUPS
                || start
                    .checked_add(4 + count * item)
                    .filter(|&e| e <= o + l)
                    .is_none()
            {
                return Err(FontError::InvalidRange("cmap 14 mappings"));
            }
            budget.spend(count)?;
            let mut previous = None;
            for j in 0..count {
                let r = start + 4 + j * item;
                let cp = u24_at(b, r).ok_or(FontError::Truncated)?;
                if !is_unicode_scalar(cp) || previous.is_some_and(|prev| cp <= prev) {
                    return Err(FontError::InvalidData(
                        "cmap format 14 mapping order or codepoint",
                    ));
                }
                if is_default {
                    let last = cp
                        .checked_add(u32::from(*b.get(r + 3).ok_or(FontError::Truncated)?))
                        .filter(|&v| is_unicode_scalar(v))
                        .ok_or(FontError::InvalidData("cmap format 14 range end"))?;
                    previous = Some(last);
                } else {
                    if u16_at(b, r + 3).ok_or(FontError::Truncated)? >= glyphs {
                        return Err(FontError::InvalidData("cmap glyph index"));
                    }
                    previous = Some(cp);
                }
            }
            cache.insert(start, count);
        }
    }
    Ok(())
}

fn is_variation_selector(cp: u32) -> bool {
    (0xfe00..=0xfe0f).contains(&cp)
        || (0xe0100..=0xe01ef).contains(&cp)
        || (0x180b..=0x180d).contains(&cp)
        || cp == 0x180f
}

fn is_unicode_scalar(cp: u32) -> bool {
    cp <= 0x10ffff && !(0xd800..=0xdfff).contains(&cp)
}
fn lookup4(b: &[u8], o: usize, l: usize, cp: u16) -> Option<u16> {
    let n = usize::from(u16_at(b, o + 6)?) / 2;
    let ep = o + 14;
    let sp = ep + n * 2 + 2;
    let dp = sp + n * 2;
    let rp = dp + n * 2;
    for i in 0..n {
        let e = u16_at(b, ep + i * 2)?;
        let s = u16_at(b, sp + i * 2)?;
        if (s..=e).contains(&cp) {
            let ro = u16_at(b, rp + i * 2)?;
            let gid = if ro == 0 {
                cp.wrapping_add(i16_at(b, dp + i * 2)? as u16)
            } else {
                let q = rp + i * 2 + usize::from(ro) + usize::from(cp - s) * 2;
                let g = u16_at(b, q)?;
                if g == 0 {
                    0
                } else {
                    g.wrapping_add(i16_at(b, dp + i * 2)? as u16)
                }
            };
            return Some(gid);
        }
    }
    let _ = l;
    None
}
fn lookup6(b: &[u8], o: usize, _l: usize, cp: u16) -> Option<u16> {
    let first = u16_at(b, o + 6)?;
    let n = u16_at(b, o + 8)?;
    if cp < first || u32::from(cp) >= u32::from(first) + u32::from(n) {
        return None;
    }
    u16_at(b, o + 10 + usize::from(cp - first) * 2)
}
fn lookup12(b: &[u8], o: usize, _l: usize, cp: u32) -> Option<u16> {
    let n = usize::try_from(u32_at(b, o + 12)?).ok()?;
    let mut lo = 0;
    let mut hi = n;
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        let p = o + 16 + m * 12;
        let s = u32_at(b, p)?;
        let e = u32_at(b, p + 4)?;
        if cp < s {
            hi = m
        } else if cp > e {
            lo = m + 1
        } else {
            return u16::try_from(u32_at(b, p + 8)?.checked_add(cp - s)?).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(b: &mut [u8], p: usize, v: u16) {
        b[p..p + 2].copy_from_slice(&v.to_be_bytes());
    }
    fn put32(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_be_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut head = vec![0; 54];
        put32(&mut head, 0, 0x0001_0000);
        put32(&mut head, 12, 0x5f0f_3cf5);
        put16(&mut head, 18, 1000);
        let mut hhea = vec![0; 36];
        put32(&mut hhea, 0, 0x0001_0000);
        put16(&mut hhea, 4, 800);
        put16(&mut hhea, 6, (-200i16) as u16);
        put16(&mut hhea, 34, 1);
        let mut maxp = vec![0; 6];
        put32(&mut maxp, 0, 0x0000_5000);
        put16(&mut maxp, 4, 2);
        let mut os2 = vec![0; 78];
        put16(&mut os2, 0, 0);
        put16(&mut os2, 4, 400);
        put16(&mut os2, 6, 5);
        let mut cmap = vec![0; 28];
        put16(&mut cmap, 0, 0);
        put16(&mut cmap, 2, 1);
        put16(&mut cmap, 4, 3);
        put16(&mut cmap, 6, 10);
        put32(&mut cmap, 8, 12);
        put16(&mut cmap, 12, 12);
        put32(&mut cmap, 16, 28);
        put32(&mut cmap, 20, 0);
        put32(&mut cmap, 24, 1); // one group follows
        cmap.extend_from_slice(&0x41u32.to_be_bytes());
        cmap.extend_from_slice(&0x41u32.to_be_bytes());
        cmap.extend_from_slice(&1u32.to_be_bytes());
        let tables: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", vec![0x03, 0xe8, 0, 5, 0, 7]),
            (*b"maxp", maxp),
            (*b"name", vec![0, 0, 0, 0, 0, 6]),
            (*b"OS/2", os2),
            (*b"post", {
                let mut v = vec![0; 32];
                put32(&mut v, 0, 0x0003_0000);
                v
            }),
            (*b"cmap", cmap),
        ];
        let base = 12 + tables.len() * 16;
        let total = base + tables.iter().map(|(_, v)| v.len() + 3).sum::<usize>();
        let mut b = vec![0; total];
        put32(&mut b, 0, 0x0001_0000);
        put16(&mut b, 4, tables.len() as u16);
        let mut off = base;
        for (i, (tag, data)) in tables.iter().enumerate() {
            let p = 12 + i * 16;
            b[p..p + 4].copy_from_slice(tag);
            put32(&mut b, p + 8, off as u32);
            put32(&mut b, p + 12, data.len() as u32);
            b[off..off + data.len()].copy_from_slice(data);
            off += data.len() + 3;
        }
        b
    }

    fn replace_table(mut font: Vec<u8>, tag: [u8; 4], data: &[u8]) -> Vec<u8> {
        let count = usize::from(u16_at(&font, 4).unwrap());
        let record = (0..count)
            .map(|i| 12 + i * 16)
            .find(|&p| font[p..p + 4] == tag)
            .unwrap();
        let offset = font.len();
        font.extend_from_slice(data);
        put32(&mut font, record + 8, offset as u32);
        put32(&mut font, record + 12, data.len() as u32);
        font
    }

    fn conflicting_cmap() -> Vec<u8> {
        let mut cmap = vec![0; 20 + 28 + 32];
        put16(&mut cmap, 0, 0);
        put16(&mut cmap, 2, 2);
        put16(&mut cmap, 4, 3);
        put16(&mut cmap, 6, 10);
        put32(&mut cmap, 8, 20);
        put16(&mut cmap, 12, 0);
        put16(&mut cmap, 14, 3);
        put32(&mut cmap, 16, 48);
        put16(&mut cmap, 20, 12);
        put16(&mut cmap, 22, 0);
        put32(&mut cmap, 24, 28);
        put32(&mut cmap, 28, 0);
        put32(&mut cmap, 32, 1);
        put32(&mut cmap, 36, 0x42);
        put32(&mut cmap, 40, 0x42);
        put32(&mut cmap, 44, 1);
        let o = 48;
        put16(&mut cmap, o, 4);
        put16(&mut cmap, o + 2, 32);
        put16(&mut cmap, o + 6, 4);
        put16(&mut cmap, o + 14, 0x41);
        put16(&mut cmap, o + 16, 0xffff);
        put16(&mut cmap, o + 20, 0x41);
        put16(&mut cmap, o + 22, 0xffff);
        put16(&mut cmap, o + 24, (-64i16) as u16);
        put16(&mut cmap, o + 26, 1);
        cmap
    }

    #[test]
    fn rejects_short_and_invalid_inputs_without_panicking() {
        for n in 0..64 {
            let b = vec![0; n];
            assert!(FontFace::parse(&b).is_err());
        }
        let b = b"not a font";
        assert_eq!(FontFace::parse(b).unwrap_err(), FontError::InvalidSignature);
    }
    #[test]
    fn rejects_overflowing_table_ranges() {
        let mut b = vec![0; 28];
        b[..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        b[4..6].copy_from_slice(&1u16.to_be_bytes());
        b[12..16].copy_from_slice(b"head");
        b[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        b[24..28].copy_from_slice(&2u32.to_be_bytes());
        assert!(matches!(
            FontFace::parse(&b),
            Err(FontError::InvalidRange("table range"))
        ));
    }

    #[test]
    fn reads_cmap_metrics_and_repeated_horizontal_advance() {
        let bytes = fixture();
        let face = FontFace::parse(&bytes).unwrap();
        assert_eq!(face.glyph_index('A'), 1);
        assert_eq!(face.glyph_index('B'), 0);
        assert_eq!(face.metrics().units_per_em, 1000);
        assert_eq!(
            face.horizontal_metric(1).unwrap(),
            HorizontalMetric {
                advance_width: 1000,
                left_side_bearing: 7
            }
        );
        assert!(face.horizontal_metric(2).is_err());
        assert_eq!(face.table(*b"head").unwrap().len(), 54);
    }

    #[test]
    fn selects_and_bounds_ttc_faces() {
        let sfnt = fixture();
        let offset = 20usize;
        let mut ttc = vec![0; offset + sfnt.len()];
        put32(&mut ttc, 0, tag(b"ttcf"));
        put32(&mut ttc, 4, 0x0001_0000);
        put32(&mut ttc, 8, 1);
        put32(&mut ttc, 12, offset as u32);
        ttc[offset..].copy_from_slice(&sfnt);
        let table_count = usize::from(u16_at(&ttc, offset + 4).unwrap());
        for i in 0..table_count {
            let p = offset + 12 + i * 16;
            let absolute = u32_at(&ttc, p + 8).unwrap() + offset as u32;
            put32(&mut ttc, p + 8, absolute);
        }
        let face = FontFace::parse_collection(&ttc, 0).unwrap();
        assert_eq!(face.glyph_index('A'), 1);
        assert_eq!(
            FontFace::parse_collection(&ttc, 1).unwrap_err(),
            FontError::FaceIndexOutOfRange
        );
    }

    #[test]
    fn resolves_bmp_cmap_formats_four_and_six() {
        // Format 4: two segments (A and the required U+FFFF sentinel).
        let mut f4 = vec![0; 32];
        put16(&mut f4, 0, 4);
        put16(&mut f4, 2, 32);
        put16(&mut f4, 6, 4); // segCountX2
        put16(&mut f4, 14, 0x41);
        put16(&mut f4, 16, 0xffff);
        put16(&mut f4, 20, 0x41);
        put16(&mut f4, 22, 0xffff);
        put16(&mut f4, 24, (-64i16) as u16);
        put16(&mut f4, 26, 1);
        assert!(
            validate4(
                &f4,
                0,
                f4.len(),
                2,
                &mut ValidationBudget {
                    remaining: MAX_CMAP_VALIDATION_WORK
                }
            )
            .is_ok()
        );
        assert_eq!(lookup4(&f4, 0, f4.len(), 0x41), Some(1));

        // Format 6: one glyph in a trimmed BMP array.
        let mut f6 = vec![0; 12];
        put16(&mut f6, 0, 6);
        put16(&mut f6, 2, 12);
        put16(&mut f6, 6, 0x41);
        put16(&mut f6, 8, 1);
        put16(&mut f6, 10, 1);
        assert!(
            validate6(
                &f6,
                0,
                f6.len(),
                2,
                &mut ValidationBudget {
                    remaining: MAX_CMAP_VALIDATION_WORK
                }
            )
            .is_ok()
        );
        assert_eq!(lookup6(&f6, 0, f6.len(), 0x41), Some(1));
        assert_eq!(lookup6(&f6, 0, f6.len(), 0x42), None);
    }

    #[test]
    fn preferred_cmap_is_used_exclusively() {
        let font = replace_table(fixture(), *b"cmap", &conflicting_cmap());
        let face = FontFace::parse(&font).unwrap();
        assert_eq!(face.glyph_index('A'), 0); // format 12 misses; format 4 must not be consulted
        assert_eq!(face.glyph_index('B'), 1);
    }

    #[test]
    fn validates_name_utf16_and_format_one_language_records() {
        let odd_name = [0, 0, 0, 1, 0, 18, 0, 3, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, b'x'];
        assert!(matches!(
            parse_names(&odd_name),
            Err(FontError::InvalidData("odd UTF-16 name length"))
        ));
        let odd_tag = [0, 1, 0, 0, 0, 12, 0, 1, 0, 3, 0, 0, b'e', b'n', b' '];
        assert!(matches!(
            parse_names(&odd_tag),
            Err(FontError::InvalidData("odd UTF-16 language tag length"))
        ));
        let legacy_odd = [0, 0, 0, 1, 0, 18, 0, 3, 0, 3, 0, 0, 0, 1, 0, 1, 0, 0, b'x'];
        assert_eq!(parse_names(&legacy_odd).unwrap()[0].value, "x");
        let valid_tag = [0, 1, 0, 0, 0, 12, 0, 1, 0, 4, 0, 0, 0, b'e', 0, b'n'];
        assert!(parse_names(&valid_tag).is_ok());
    }

    #[test]
    fn bounds_alias_amplification_in_format14() {
        let selectors = 11usize;
        let mappings = 100_000usize;
        let f14_len = 10 + selectors * 11 + 4 + mappings * 5;
        let f14_off = 20usize;
        let f12_off = f14_off + f14_len;
        let mut cmap = vec![0; f12_off + 28];
        put16(&mut cmap, 0, 0);
        put16(&mut cmap, 2, 2);
        put16(&mut cmap, 4, 0);
        put16(&mut cmap, 6, 5);
        put32(&mut cmap, 8, f14_off as u32);
        put16(&mut cmap, 12, 3);
        put16(&mut cmap, 14, 10);
        put32(&mut cmap, 16, f12_off as u32);
        put16(&mut cmap, f14_off, 14);
        put32(&mut cmap, f14_off + 2, f14_len as u32);
        put32(&mut cmap, f14_off + 6, selectors as u32);
        let mapping_offset = 10 + selectors * 11;
        for i in 0..selectors {
            let p = f14_off + 10 + i * 11;
            let selector = 0xfe00 + i as u32;
            cmap[p..p + 3].copy_from_slice(&selector.to_be_bytes()[1..]);
            put32(&mut cmap, p + 7, mapping_offset as u32);
        }
        let records = f14_off + mapping_offset;
        put32(&mut cmap, records, mappings as u32);
        for i in 0..mappings {
            let p = records + 4 + i * 5;
            let cp = 0x10000 + i as u32;
            cmap[p..p + 3].copy_from_slice(&cp.to_be_bytes()[1..]);
            put16(&mut cmap, p + 3, 1);
        }
        put16(&mut cmap, f12_off, 12);
        put16(&mut cmap, f12_off + 2, 0);
        put32(&mut cmap, f12_off + 4, 28);
        put32(&mut cmap, f12_off + 8, 0);
        put32(&mut cmap, f12_off + 12, 1);
        put32(&mut cmap, f12_off + 16, 0x41);
        put32(&mut cmap, f12_off + 20, 0x41);
        put32(&mut cmap, f12_off + 24, 1);
        parse_cmap(&cmap, 2).expect("aliased mappings are validated once under the global budget");
    }
}
