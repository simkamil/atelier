// SPDX-License-Identifier: AGPL-3.0-or-later
//! Font subsetting for PDF embedding (US-330).
//!
//! [`subset_font`] and [`subset_face`] reduce a TrueType (`glyf`) or OpenType/CFF
//! face to a set of glyphs and return a self-contained font program together
//! with the data a PDF writer needs: the glyph mapping, advances, global
//! metrics, the PostScript name and the embedding rights. [`subset_cff`]
//! does the same for a bare CFF table.
//!
//! Identifier strategy:
//!
//! * TrueType keeps the original glyph identifiers (unused glyphs become empty).
//!   Composite references stay valid without rewriting and the PDF
//!   `CIDToGIDMap` is `/Identity`.
//! * CFF renumbers the kept glyphs densely (the CharStrings INDEX has no
//!   per-glyph tables to pad). The CID to use in the content stream is reported
//!   by [`SubsetGlyph::cid`]: the new glyph index for a name-keyed font, the
//!   original CID for a CID-keyed font.
//!
//! Every error is typed; no input can make this module panic, and the output
//! depends only on the inputs.

use crate::cff::CffFont;
use crate::subset_cff;
use crate::{FontError, FontFace, OutlineError};
use std::collections::BTreeSet;

/// Maximum nesting of composite glyphs followed when closing the glyph set.
const MAX_COMPOSITE_DEPTH: usize = 32;
/// Maximum components read from a single composite glyph.
const MAX_COMPONENTS_PER_GLYPH: usize = 4096;
const MAX_NAME_RECORDS: usize = 16_384;

/// Errors returned by the subsetter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubsetError {
    /// The font data is malformed (reader error).
    Font(FontError),
    /// A CFF table is malformed (reader error).
    Cff(OutlineError),
    /// A requested glyph id is not below the glyph count.
    GlyphOutOfRange(u16),
    /// `OS/2.fsType` declares restricted license embedding: the font must not be embedded.
    /// The caller is expected to fall back (for example to a non-embedded font) and warn.
    EmbeddingRestricted,
    /// `OS/2.fsType` bit 9: only bitmaps may be embedded; outlines may not.
    BitmapOnly,
    /// `OS/2.fsType` bit 8: the font may only be embedded whole. The caller must
    /// embed the original font file unchanged (all glyph ids are then unchanged).
    SubsettingForbidden,
    /// A feature this subsetter does not handle.
    Unsupported(&'static str),
    /// Structurally inconsistent font data.
    InvalidData(&'static str),
    /// A resource limit was exceeded.
    LimitExceeded(&'static str),
}

impl core::fmt::Display for SubsetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Font(e) => write!(f, "font error: {e}"),
            Self::Cff(e) => write!(f, "CFF error: {e}"),
            Self::GlyphOutOfRange(g) => write!(f, "glyph id {g} is outside the font"),
            Self::EmbeddingRestricted => f.write_str("font license forbids embedding"),
            Self::BitmapOnly => f.write_str("font license allows bitmap embedding only"),
            Self::SubsettingForbidden => f.write_str("font license forbids subsetting"),
            Self::Unsupported(s) => write!(f, "unsupported for subsetting: {s}"),
            Self::InvalidData(s) => write!(f, "inconsistent font data: {s}"),
            Self::LimitExceeded(s) => write!(f, "subsetting limit exceeded: {s}"),
        }
    }
}
impl std::error::Error for SubsetError {}
impl From<FontError> for SubsetError {
    fn from(e: FontError) -> Self {
        Self::Font(e)
    }
}
impl From<OutlineError> for SubsetError {
    fn from(e: OutlineError) -> Self {
        Self::Cff(e)
    }
}

/// Kind of font program produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramKind {
    /// A TrueType sfnt (PDF `FontFile2`, `CIDFontType2`).
    TrueType,
    /// A bare CFF table (PDF `FontFile3` with `/Subtype /CIDFontType0C`).
    Cff,
}

/// Usage permission derived from `OS/2.fsType` bits 1 to 3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmbeddingPermission {
    /// No restriction (`fsType` bits 1 to 3 clear).
    Installable,
    /// Editable embedding (bit 3).
    Editable,
    /// Preview and print embedding (bit 2): the document should be treated as read-only.
    PreviewPrint,
}

/// Embedding rights of a font.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmbeddingRights {
    pub permission: EmbeddingPermission,
    /// `false` when `fsType` bit 8 forbids subsetting (whole-font embedding only).
    pub subsetting_allowed: bool,
}

/// Interpret `OS/2.fsType` (`None`: no `OS/2` table, the font is embeddable).
///
/// Bits 1 to 3 are mutually exclusive; when several are set the least
/// restrictive wins, as the specification says. Restricted license embedding
/// (bit 1 alone) gives [`SubsetError::EmbeddingRestricted`]; bit 9 gives
/// [`SubsetError::BitmapOnly`] (this subsetter embeds outlines). Bit 8 is
/// reported through [`EmbeddingRights::subsetting_allowed`].
pub fn embedding_rights(fs_type: Option<u16>) -> Result<EmbeddingRights, SubsetError> {
    let Some(v) = fs_type else {
        return Ok(EmbeddingRights {
            permission: EmbeddingPermission::Installable,
            subsetting_allowed: true,
        });
    };
    if v & 0x0200 != 0 {
        return Err(SubsetError::BitmapOnly);
    }
    let permission = if v & 0x0008 != 0 {
        EmbeddingPermission::Editable
    } else if v & 0x0004 != 0 {
        EmbeddingPermission::PreviewPrint
    } else if v & 0x0002 != 0 {
        return Err(SubsetError::EmbeddingRestricted);
    } else {
        EmbeddingPermission::Installable
    };
    Ok(EmbeddingRights {
        permission,
        subsetting_allowed: v & 0x0100 == 0,
    })
}

/// One glyph kept in the subset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubsetGlyph {
    /// Glyph id in the source font.
    pub old_gid: u16,
    /// Glyph id in the subset font program.
    pub new_gid: u16,
    /// CID to use in the PDF content stream for this glyph.
    pub cid: u16,
    /// Horizontal advance in font units ([`SubsetMetrics::units_per_em`]).
    pub advance: f64,
}

/// Global metrics for the PDF font descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct SubsetMetrics {
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    /// `[xMin, yMin, xMax, yMax]` of the whole source font (`FontBBox`).
    pub bbox: [i16; 4],
    /// Italic angle in degrees counter-clockwise from the vertical.
    pub italic_angle: f64,
    pub cap_height: Option<i16>,
    pub x_height: Option<i16>,
    pub weight_class: Option<u16>,
    pub fixed_pitch: bool,
    pub serif: bool,
    pub italic: bool,
    pub bold: bool,
}

impl SubsetMetrics {
    /// Suggested PDF `/Flags`: Symbolic (a composite font addressed by CID), plus FixedPitch, Serif and Italic.
    pub fn descriptor_flags(&self) -> u32 {
        let mut f = 4;
        if self.fixed_pitch {
            f |= 1;
        }
        if self.serif {
            f |= 2;
        }
        if self.italic {
            f |= 64;
        }
        f
    }
    /// Rough `/StemV` estimate from the weight class (400 gives 95, 700 gives 161).
    pub fn stem_v_estimate(&self) -> u16 {
        let w = u32::from(
            self.weight_class
                .unwrap_or(if self.bold { 700 } else { 400 }),
        );
        let w = w.clamp(100, 900);
        (10 + 220 * (w - 50) / 900) as u16
    }
}

/// A subset font program and the information a PDF writer needs.
#[derive(Clone, Debug, PartialEq)]
pub struct SubsetFont {
    pub kind: ProgramKind,
    /// The font program (sfnt bytes for TrueType, CFF table bytes for CFF).
    pub data: Vec<u8>,
    /// PostScript name, restricted to characters valid in a PDF name.
    pub post_script_name: String,
    /// Kept glyphs, ascending by `old_gid`. Glyph 0 (`.notdef`) is always first.
    pub glyphs: Vec<SubsetGlyph>,
    pub metrics: SubsetMetrics,
    pub rights: EmbeddingRights,
    /// CFF only: whether the produced CFF is CID-keyed (so CIDs are the original charset CIDs).
    pub cid_keyed: bool,
}

impl SubsetFont {
    /// The kept glyph for a source glyph id.
    pub fn glyph(&self, old_gid: u16) -> Option<&SubsetGlyph> {
        self.glyphs
            .binary_search_by_key(&old_gid, |g| g.old_gid)
            .ok()
            .map(|i| &self.glyphs[i])
    }
    /// Six uppercase letters derived from the font program (PDF 9.6.4 subset tag).
    pub fn pdf_subset_tag(&self) -> [u8; 6] {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &self.data {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut tag = [0u8; 6];
        for t in &mut tag {
            *t = b'A' + (h % 26) as u8;
            h /= 26;
        }
        tag
    }
}

/// Subset face `face_index` of an sfnt font or TrueType collection.
pub fn subset_font(
    bytes: &[u8],
    face_index: usize,
    glyphs: &[u16],
) -> Result<SubsetFont, SubsetError> {
    let face = FontFace::parse_collection(bytes, face_index)?;
    subset_face(&face, glyphs)
}

/// Subset a parsed face, TrueType outlines (`glyf`) or CFF (`CFF `).
///
/// `glyphs` may be in any order with duplicates; glyph 0 is always kept and an
/// empty slice yields a font with `.notdef` only.
pub fn subset_face(face: &FontFace<'_>, glyphs: &[u16]) -> Result<SubsetFont, SubsetError> {
    let rights = embedding_rights(face.metrics().fs_type)?;
    if !rights.subsetting_allowed {
        return Err(SubsetError::SubsettingForbidden);
    }
    let count = face.metrics().number_of_glyphs;
    let wanted = wanted_set(glyphs, count)?;
    if face.table(*b"CFF ").is_some() {
        subset_otf_cff(face, &wanted, rights)
    } else if face.table(*b"CFF2").is_some() {
        Err(SubsetError::Unsupported("CFF2 variable outlines"))
    } else if face.table(*b"glyf").is_some() && face.table(*b"loca").is_some() {
        subset_truetype(face, &wanted, rights)
    } else {
        Err(SubsetError::Font(FontError::MissingTable("glyf")))
    }
}

/// Subset a bare CFF table (no `OS/2`: always embeddable). Advances come from the charstrings (1000 units per em).
pub fn subset_cff(cff: &[u8], glyphs: &[u16]) -> Result<SubsetFont, SubsetError> {
    let font = CffFont::parse(cff)?;
    let count =
        u16::try_from(font.glyph_count()).map_err(|_| SubsetError::LimitExceeded("glyph count"))?;
    let wanted = wanted_set(glyphs, count)?;
    let out = subset_cff::subset(cff, &wanted)?;
    let rights = embedding_rights(None)?;
    let mut glyphs_out = Vec::with_capacity(out.kept.len());
    for (new, &old) in out.kept.iter().enumerate() {
        let w = font.glyph(old, Default::default())?.width;
        glyphs_out.push(SubsetGlyph {
            old_gid: old,
            new_gid: new as u16,
            cid: out.cids[new],
            advance: w,
        });
    }
    let bbox = out.bbox.unwrap_or([0, 0, 1000, 1000]);
    Ok(SubsetFont {
        kind: ProgramKind::Cff,
        data: out.data,
        post_script_name: sanitize_name(&out.name),
        glyphs: glyphs_out,
        metrics: SubsetMetrics {
            units_per_em: 1000,
            ascent: bbox[3],
            descent: bbox[1],
            line_gap: 0,
            bbox,
            italic_angle: out.italic_angle,
            cap_height: None,
            x_height: None,
            weight_class: None,
            fixed_pitch: out.fixed_pitch,
            serif: false,
            italic: out.italic_angle != 0.0,
            bold: false,
        },
        rights,
        cid_keyed: out.cid_keyed,
    })
}

fn wanted_set(glyphs: &[u16], count: u16) -> Result<BTreeSet<u16>, SubsetError> {
    let mut set = BTreeSet::new();
    set.insert(0);
    for &g in glyphs {
        if g >= count {
            return Err(SubsetError::GlyphOutOfRange(g));
        }
        set.insert(g);
    }
    Ok(set)
}

fn sanitize_name(raw: &[u8]) -> String {
    let s: String = raw
        .iter()
        .filter(|&&b| (33..=126).contains(&b) && !b"[](){}<>/%".contains(&b))
        .take(63)
        .map(|&b| char::from(b))
        .collect();
    if s.is_empty() { "Subset".into() } else { s }
}

fn face_ps_name(face: &FontFace<'_>) -> String {
    let mut best: Option<(u8, &str)> = None;
    for r in face.names() {
        if r.name_id != 6 {
            continue;
        }
        let rank = match (r.platform_id, r.language_id) {
            (3, 0x0409) => 0,
            (3, _) => 1,
            (1, 0) => 2,
            _ => 3,
        };
        if best.is_none_or(|(b, _)| rank < b) {
            best = Some((rank, r.value.as_str()));
        }
    }
    sanitize_name(best.map_or(&[][..], |(_, v)| v.as_bytes()))
}

fn rd16(b: &[u8], p: usize) -> Option<u16> {
    crate::u16_at(b, p)
}
fn rdi16(b: &[u8], p: usize) -> Option<i16> {
    crate::i16_at(b, p)
}
fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// Global metrics shared by the TrueType and OpenType/CFF paths.
fn face_metrics(face: &FontFace<'_>) -> Result<SubsetMetrics, SubsetError> {
    let m = face.metrics();
    let head = face
        .table(*b"head")
        .ok_or(FontError::MissingTable("head"))?;
    let post = face
        .table(*b"post")
        .ok_or(FontError::MissingTable("post"))?;
    let os2 = face
        .table(*b"OS/2")
        .ok_or(FontError::MissingTable("OS/2"))?;
    let g = |p| rdi16(head, p).ok_or(FontError::Truncated);
    let bbox = [g(36)?, g(38)?, g(40)?, g(42)?];
    let mac_style = rd16(head, 44).ok_or(FontError::Truncated)?;
    let angle = crate::u32_at(post, 4).ok_or(FontError::Truncated)? as i32;
    let fixed = crate::u32_at(post, 12).ok_or(FontError::Truncated)? != 0;
    let version = rd16(os2, 0).unwrap_or(0);
    let family = rdi16(os2, 30).unwrap_or(0);
    let class = family >> 8;
    let (x_height, cap_height) = if version >= 2 {
        (rdi16(os2, 86), rdi16(os2, 88))
    } else {
        (None, None)
    };
    let weight = m.weight_class;
    let italic_angle = f64::from(angle) / 65536.0;
    Ok(SubsetMetrics {
        units_per_em: m.units_per_em,
        ascent: m.ascender,
        descent: m.descender,
        line_gap: m.line_gap,
        bbox,
        italic_angle,
        cap_height: cap_height.filter(|&v| v > 0),
        x_height: x_height.filter(|&v| v > 0),
        weight_class: weight,
        fixed_pitch: fixed,
        serif: matches!(class, 1..=5 | 7),
        italic: mac_style & 2 != 0 || italic_angle != 0.0,
        bold: mac_style & 1 != 0 || weight.is_some_and(|w| w >= 700),
    })
}

// ------------------------------------------------------------------ OpenType/CFF

fn subset_otf_cff(
    face: &FontFace<'_>,
    wanted: &BTreeSet<u16>,
    rights: EmbeddingRights,
) -> Result<SubsetFont, SubsetError> {
    let table = face
        .table(*b"CFF ")
        .ok_or(FontError::MissingTable("CFF "))?;
    let out = subset_cff::subset(table, wanted)?;
    let mut glyphs = Vec::with_capacity(out.kept.len());
    for (new, &old) in out.kept.iter().enumerate() {
        glyphs.push(SubsetGlyph {
            old_gid: old,
            new_gid: new as u16,
            cid: out.cids[new],
            advance: f64::from(face.horizontal_metric(old)?.advance_width),
        });
    }
    Ok(SubsetFont {
        kind: ProgramKind::Cff,
        data: out.data,
        post_script_name: face_ps_name(face),
        glyphs,
        metrics: face_metrics(face)?,
        rights,
        cid_keyed: out.cid_keyed,
    })
}

// ------------------------------------------------------------------ TrueType

struct Loca<'a> {
    bytes: &'a [u8],
    long: bool,
}
impl Loca<'_> {
    fn offset(&self, i: usize) -> Result<usize, SubsetError> {
        let v = if self.long {
            crate::u32_at(self.bytes, i * 4).map(|v| v as usize)
        } else {
            rd16(self.bytes, i * 2).map(|v| usize::from(v) * 2)
        };
        v.ok_or(SubsetError::InvalidData("loca length"))
    }
}

fn glyph_bytes<'a>(glyf: &'a [u8], loca: &Loca<'_>, gid: usize) -> Result<&'a [u8], SubsetError> {
    let a = loca.offset(gid)?;
    let b = loca.offset(gid + 1)?;
    if a > b || b > glyf.len() {
        return Err(SubsetError::InvalidData("loca offsets"));
    }
    Ok(&glyf[a..b])
}

/// Component glyph ids of a composite glyph (empty for a simple or empty glyph).
fn components(data: &[u8]) -> Result<Vec<u16>, SubsetError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let contours = rdi16(data, 0).ok_or(SubsetError::InvalidData("glyph header"))?;
    if data.len() < 10 {
        return Err(SubsetError::InvalidData("glyph header"));
    }
    let mut out = Vec::new();
    if contours >= 0 {
        return Ok(out);
    }
    let mut p = 10;
    loop {
        let flags = rd16(data, p).ok_or(SubsetError::InvalidData("composite glyph"))?;
        let gid = rd16(data, p + 2).ok_or(SubsetError::InvalidData("composite glyph"))?;
        if out.len() >= MAX_COMPONENTS_PER_GLYPH {
            return Err(SubsetError::LimitExceeded("composite components"));
        }
        out.push(gid);
        p += 4 + if flags & 1 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            p += 2;
        } else if flags & 0x0040 != 0 {
            p += 4;
        } else if flags & 0x0080 != 0 {
            p += 8;
        }
        if flags & 0x0020 == 0 {
            break;
        }
    }
    if p > data.len() {
        return Err(SubsetError::InvalidData("composite glyph"));
    }
    Ok(out)
}

struct Closure<'a> {
    glyf: &'a [u8],
    loca: Loca<'a>,
    count: usize,
    state: Vec<u8>,
}
impl Closure<'_> {
    fn visit(&mut self, gid: usize, depth: usize) -> Result<(), SubsetError> {
        match self.state[gid] {
            2 => return Ok(()),
            1 => return Err(SubsetError::InvalidData("cyclic composite glyph")),
            _ => {}
        }
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(SubsetError::LimitExceeded("composite depth"));
        }
        self.state[gid] = 1;
        let comps = components(glyph_bytes(self.glyf, &self.loca, gid)?)?;
        for c in comps {
            let c = usize::from(c);
            if c >= self.count {
                return Err(SubsetError::InvalidData("component glyph id"));
            }
            self.visit(c, depth + 1)?;
        }
        self.state[gid] = 2;
        Ok(())
    }
}

fn subset_truetype(
    face: &FontFace<'_>,
    wanted: &BTreeSet<u16>,
    rights: EmbeddingRights,
) -> Result<SubsetFont, SubsetError> {
    let m = face.metrics();
    let count = usize::from(m.number_of_glyphs);
    let glyf = face
        .table(*b"glyf")
        .ok_or(FontError::MissingTable("glyf"))?;
    let loca_bytes = face
        .table(*b"loca")
        .ok_or(FontError::MissingTable("loca"))?;
    let long = m.index_to_loc_format == 1;
    if loca_bytes.len() < (count + 1) * if long { 4 } else { 2 } {
        return Err(SubsetError::InvalidData("loca length"));
    }
    let maxp = face
        .table(*b"maxp")
        .ok_or(FontError::MissingTable("maxp"))?;
    if maxp.len() < 32 || crate::u32_at(maxp, 0) != Some(0x0001_0000) {
        return Err(SubsetError::InvalidData("maxp version for glyf outlines"));
    }
    let mut closure = Closure {
        glyf,
        loca: Loca {
            bytes: loca_bytes,
            long,
        },
        count,
        state: vec![0; count],
    };
    for &g in wanted {
        closure.visit(usize::from(g), 0)?;
    }
    let keep: Vec<bool> = closure.state.iter().map(|&s| s == 2).collect();
    let new_count = keep.iter().rposition(|&k| k).map_or(1, |i| i + 1);

    // glyf, loca and hmtx; every kept glyph is copied verbatim at a 4-aligned offset.
    let mut new_glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::with_capacity(new_count + 1);
    let mut hmtx: Vec<u8> = Vec::with_capacity(new_count * 4);
    let mut glyphs = Vec::new();
    for (gid, &kept) in keep.iter().enumerate().take(new_count) {
        offsets.push(new_glyf.len());
        if kept {
            new_glyf.extend_from_slice(glyph_bytes(glyf, &closure.loca, gid)?);
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
            let hm = face.horizontal_metric(gid as u16)?;
            put16(&mut hmtx, hm.advance_width);
            hmtx.extend_from_slice(&hm.left_side_bearing.to_be_bytes());
            glyphs.push(SubsetGlyph {
                old_gid: gid as u16,
                new_gid: gid as u16,
                cid: gid as u16,
                advance: f64::from(hm.advance_width),
            });
        } else {
            put32(&mut hmtx, 0);
        }
    }
    offsets.push(new_glyf.len());
    let short = new_glyf.len() / 2 <= 0xffff;
    let mut loca = Vec::with_capacity(offsets.len() * 4);
    for &o in &offsets {
        if short {
            put16(&mut loca, (o / 2) as u16);
        } else {
            put32(&mut loca, o as u32);
        }
    }

    let src = |t: &[u8; 4]| face.table(*t);
    let mut head = src(b"head").ok_or(FontError::MissingTable("head"))?[..54].to_vec();
    head[8..12].fill(0);
    head[50..52].copy_from_slice(&u16::from(!short).to_be_bytes());
    let mut hhea = src(b"hhea").ok_or(FontError::MissingTable("hhea"))?[..36].to_vec();
    hhea[34..36].copy_from_slice(&(new_count as u16).to_be_bytes());
    let mut new_maxp = maxp[..32].to_vec();
    new_maxp[4..6].copy_from_slice(&(new_count as u16).to_be_bytes());
    let os2 = src(b"OS/2").ok_or(FontError::MissingTable("OS/2"))?;
    let mut post = src(b"post").ok_or(FontError::MissingTable("post"))?[..32].to_vec();
    post[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"maxp", new_maxp),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"glyf", new_glyf),
        (*b"OS/2", os2.to_vec()),
        (*b"post", post),
        (*b"cmap", empty_cmap()),
        (
            *b"name",
            trimmed_name(src(b"name").ok_or(FontError::MissingTable("name"))?)?,
        ),
    ];
    for t in [b"cvt ", b"fpgm", b"prep"] {
        if let Some(b) = src(t) {
            tables.push((*t, b.to_vec()));
        }
    }
    Ok(SubsetFont {
        kind: ProgramKind::TrueType,
        data: build_sfnt(0x0001_0000, tables)?,
        post_script_name: face_ps_name(face),
        glyphs,
        metrics: face_metrics(face)?,
        rights,
        cid_keyed: false,
    })
}

/// A `cmap` with one format 4 subtable mapping nothing (only the 0xFFFF terminator).
///
/// A PDF composite font never reads it; the crate's own reader (and many tools) require a `cmap`.
fn empty_cmap() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [0u16, 1, 3, 1] {
        put16(&mut v, x);
    }
    put32(&mut v, 12);
    for x in [4u16, 24, 0, 2, 2, 0, 0, 0xffff, 0, 0xffff, 1, 0] {
        put16(&mut v, x);
    }
    v
}

/// `name` table with the records 0 to 6 of the source (copyright, family, version, PostScript name...).
fn trimmed_name(name: &[u8]) -> Result<Vec<u8>, SubsetError> {
    let bad = SubsetError::InvalidData("name table");
    let format = rd16(name, 0).ok_or(bad.clone())?;
    let count = usize::from(rd16(name, 2).ok_or(bad.clone())?);
    let storage = usize::from(rd16(name, 4).ok_or(bad.clone())?);
    if count > MAX_NAME_RECORDS {
        return Err(SubsetError::LimitExceeded("name records"));
    }
    let mut records: Vec<[u16; 6]> = Vec::new();
    let mut strings: Vec<u8> = Vec::new();
    for i in 0..count {
        let p = 6 + i * 12;
        let f = |k: usize| rd16(name, p + k * 2).ok_or(bad.clone());
        let (platform, enc, lang, id, len, off) = (f(0)?, f(1)?, f(2)?, f(3)?, f(4)?, f(5)?);
        if id > 6 || (format == 1 && lang >= 0x8000) {
            continue;
        }
        let start = storage + usize::from(off);
        let s = name
            .get(start..start + usize::from(len))
            .ok_or(bad.clone())?;
        if strings.len() + s.len() > 0xffff {
            break;
        }
        records.push([platform, enc, lang, id, len, strings.len() as u16]);
        strings.extend_from_slice(s);
    }
    let mut out = Vec::new();
    put16(&mut out, 0);
    put16(&mut out, records.len() as u16);
    put16(&mut out, (6 + 12 * records.len()) as u16);
    for r in &records {
        for &x in r {
            put16(&mut out, x);
        }
    }
    out.extend_from_slice(&strings);
    Ok(out)
}

fn checksum(b: &[u8]) -> u32 {
    let mut sum = 0u32;
    for c in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum = sum.wrapping_add(u32::from_be_bytes(w));
    }
    sum
}

/// Assemble an sfnt: sorted directory, correct checksums, 4-byte alignment, `head.checkSumAdjustment`.
fn build_sfnt(version: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Result<Vec<u8>, SubsetError> {
    tables.sort_by_key(|t| t.0);
    let n = tables.len();
    let mut entry_selector = 0usize;
    while 1usize << (entry_selector + 1) <= n {
        entry_selector += 1;
    }
    let search_range = (1usize << entry_selector) * 16;
    let mut out = Vec::new();
    put32(&mut out, version);
    put16(&mut out, n as u16);
    put16(&mut out, search_range as u16);
    put16(&mut out, entry_selector as u16);
    put16(&mut out, (n * 16 - search_range) as u16);
    let mut pos = 12 + 16 * n;
    let mut head_pos = None;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        put32(&mut out, checksum(data));
        put32(
            &mut out,
            u32::try_from(pos).map_err(|_| SubsetError::LimitExceeded("font size"))?,
        );
        put32(&mut out, data.len() as u32);
        if tag == b"head" {
            head_pos = Some(pos);
        }
        pos += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(h) = head_pos {
        let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&out));
        out[h + 8..h + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    Ok(out)
}
