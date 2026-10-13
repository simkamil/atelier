// SPDX-License-Identifier: AGPL-3.0-or-later
//! Font registry: one entry per font program, with the union of the glyphs
//! used on every page; embedding (subset or whole) and `ToUnicode` at the end.

use crate::out::{Out, name};
use crate::{NoEmbedReason, PdfError, PdfFont};
use atelier_font::FontFace;
use atelier_font::cff::CffFont;
use atelier_font::subset::{ProgramKind, SubsetError, embedding_rights, subset_font};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Longest text one glyph may stand for (a ligature is a handful of characters).
pub(crate) const MAX_GLYPH_TEXT: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Subset,
    Whole,
    Paths,
}

pub(crate) struct FontEntry {
    /// First `font_id` seen, used to fetch the program again at the end.
    pub font_id: u64,
    pub mode: Mode,
    /// CFF outlines (codes in the content stream are patched once CIDs are final).
    pub cff: bool,
    pub num_glyphs: u16,
    /// Advance of every glyph in 1/1000 em.
    pub widths: Vec<u16>,
    pub space_gid: Option<u16>,
    pub gids: BTreeSet<u16>,
    pub text: BTreeMap<u16, String>,
    pub text_reported: BTreeSet<u16>,
    /// Type0 font object (0 for `Mode::Paths`).
    pub obj: u32,
}

pub(crate) struct Created {
    pub entry: FontEntry,
    pub notice: Option<(Mode, Option<NoEmbedReason>)>,
}

fn scale(v: f64, upem: u16) -> f64 {
    v * 1000.0 / f64::from(upem.max(1))
}

pub(crate) fn create(font_id: u64, font: &PdfFont<'_>) -> Result<Created, PdfError> {
    let face = FontFace::parse_collection(font.data, font.face_index).map_err(PdfError::Font)?;
    let metrics = face.metrics();
    let cff = face.table(*b"CFF ").is_some();
    if face.table(*b"CFF2").is_some() {
        return Err(PdfError::UnsupportedFont("CFF2 outlines"));
    }
    if !cff && !(face.table(*b"glyf").is_some() && face.table(*b"loca").is_some()) {
        return Err(PdfError::UnsupportedFont("no TrueType or CFF outlines"));
    }
    let mut mode = Mode::Subset;
    let mut reason = None;
    match embedding_rights(metrics.fs_type) {
        Ok(r) => {
            if !r.subsetting_allowed {
                mode = Mode::Whole;
                if font.data.starts_with(b"ttcf") {
                    mode = Mode::Paths;
                    reason = Some(NoEmbedReason::CollectionNotEmbeddableWhole);
                }
            }
        }
        Err(SubsetError::BitmapOnly) => {
            mode = Mode::Paths;
            reason = Some(NoEmbedReason::BitmapOnly);
        }
        Err(SubsetError::EmbeddingRestricted) => {
            mode = Mode::Paths;
            reason = Some(NoEmbedReason::Restricted);
        }
        Err(e) => return Err(PdfError::Subset(e)),
    }
    let (widths, space_gid) = if mode == Mode::Paths {
        (Vec::new(), None)
    } else {
        let mut w = Vec::with_capacity(usize::from(metrics.number_of_glyphs));
        for gid in 0..metrics.number_of_glyphs {
            let adv = face.horizontal_metric(gid).map_or(0, |m| m.advance_width);
            let v = scale(f64::from(adv), metrics.units_per_em).round();
            w.push(v.clamp(0.0, 65535.0) as u16);
        }
        let space = face.glyph_index(' ');
        (
            w,
            (space != 0 && space < metrics.number_of_glyphs).then_some(space),
        )
    };
    Ok(Created {
        entry: FontEntry {
            font_id,
            mode,
            cff,
            num_glyphs: metrics.number_of_glyphs,
            widths,
            space_gid,
            gids: BTreeSet::new(),
            text: BTreeMap::new(),
            text_reported: BTreeSet::new(),
            obj: 0,
        },
        notice: (mode != Mode::Subset).then_some((mode, reason)),
    })
}

/// Everything the end of the document needs to know about an embedded font.
pub(crate) struct Prepared {
    pub desc: Desc,
    pub program: Vec<u8>,
    pub program_kind: ProgramKind,
    /// `/Subtype /OpenType` (whole CFF) rather than `CIDFontType0C`.
    pub opentype: bool,
    /// Glyph id -> CID, for every glyph used.
    pub cid_of: BTreeMap<u16, u16>,
}

pub(crate) struct Desc {
    pub name: String,
    pub bbox: [i32; 4],
    pub italic_angle: f64,
    pub ascent: i32,
    pub descent: i32,
    pub cap_height: i32,
    pub flags: u32,
    pub stem_v: u32,
}

fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(60)
        .collect();
    if t.is_empty() { "Font".to_owned() } else { t }
}

fn i16_at(b: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

pub(crate) fn prepare(entry: &FontEntry, font: &PdfFont<'_>) -> Result<Prepared, PdfError> {
    let gids: Vec<u16> = entry.gids.iter().copied().collect();
    if entry.mode == Mode::Subset {
        let sf = subset_font(font.data, font.face_index, &gids).map_err(PdfError::Subset)?;
        let m = &sf.metrics;
        let up = m.units_per_em;
        let mut cid_of = BTreeMap::new();
        for &g in &gids {
            let sg = sf
                .glyph(g)
                .ok_or(PdfError::Internal("glyph missing from subset"))?;
            if sf.kind == ProgramKind::TrueType && sg.cid != g {
                return Err(PdfError::Internal("TrueType subset renumbered glyphs"));
            }
            cid_of.insert(g, sg.cid);
        }
        let tag: String = sf.pdf_subset_tag().iter().map(|&b| char::from(b)).collect();
        let cap = m.cap_height.map_or_else(
            || scale(f64::from(m.ascent), up) * 0.7,
            |c| scale(f64::from(c), up),
        );
        let desc = Desc {
            name: format!("{tag}+{}", sanitize(&sf.post_script_name)),
            bbox: m.bbox.map(|v| scale(f64::from(v), up).round() as i32),
            italic_angle: m.italic_angle,
            ascent: scale(f64::from(m.ascent), up).round() as i32,
            descent: scale(f64::from(m.descent), up).round() as i32,
            cap_height: cap.round() as i32,
            flags: m.descriptor_flags(),
            stem_v: u32::from(m.stem_v_estimate()),
        };
        return Ok(Prepared {
            desc,
            program: sf.data,
            program_kind: sf.kind,
            opentype: false,
            cid_of,
        });
    }
    // Whole font.
    let face = FontFace::parse_collection(font.data, font.face_index).map_err(PdfError::Font)?;
    let fm = face.metrics();
    let up = fm.units_per_em;
    let head = face.table(*b"head").unwrap_or(&[]);
    let bbox = [36, 38, 40, 42].map(|o| i32::from(i16_at(head, o).unwrap_or(0)));
    let post = face.table(*b"post").unwrap_or(&[]);
    let italic_angle = post.get(4..8).map_or(0.0, |b| {
        f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])) / 65536.0
    });
    let fixed = post.get(12..16).is_some_and(|b| b != [0, 0, 0, 0]);
    let os2 = face.table(*b"OS/2").unwrap_or(&[]);
    let cap = if os2.len() >= 90 && os2.get(0..2) != Some(&[0, 0]) && os2.get(0..2) != Some(&[0, 1])
    {
        i16_at(os2, 88).map(f64::from)
    } else {
        None
    };
    let ps = face
        .names()
        .iter()
        .find(|n| n.name_id == 6)
        .map_or_else(|| "Font".to_owned(), |n| sanitize(&n.value));
    let mut flags = 4;
    if fixed {
        flags |= 1;
    }
    if italic_angle != 0.0 {
        flags |= 64;
    }
    let weight = u32::from(fm.weight_class.unwrap_or(400)).clamp(100, 900);
    let desc = Desc {
        name: ps,
        bbox: bbox.map(|v| scale(f64::from(v), up).round() as i32),
        italic_angle,
        ascent: scale(f64::from(fm.ascender), up).round() as i32,
        descent: scale(f64::from(fm.descender), up).round() as i32,
        cap_height: cap
            .map_or_else(|| scale(f64::from(fm.ascender), up) * 0.7, |c| scale(c, up))
            .round() as i32,
        flags,
        stem_v: 10 + 220 * (weight - 50) / 900,
    };
    let mut cid_of = BTreeMap::new();
    let mut kind = ProgramKind::TrueType;
    if entry.cff {
        kind = ProgramKind::Cff;
        let table = face
            .table(*b"CFF ")
            .ok_or(PdfError::Internal("CFF table vanished"))?;
        let cff = CffFont::parse(table).map_err(|_| PdfError::UnsupportedFont("CFF table"))?;
        let charset = if cff.is_cid() {
            Some(
                cff.charset()
                    .map_err(|_| PdfError::UnsupportedFont("CFF charset"))?,
            )
        } else {
            None
        };
        for &g in &gids {
            let cid = match &charset {
                Some(cs) => *cs
                    .get(usize::from(g))
                    .ok_or(PdfError::UnsupportedFont("CFF charset"))?,
                None => g,
            };
            cid_of.insert(g, cid);
        }
    } else {
        for &g in &gids {
            cid_of.insert(g, g);
        }
    }
    Ok(Prepared {
        desc,
        program: font.data.to_vec(),
        program_kind: kind,
        opentype: entry.cff,
        cid_of,
    })
}

fn utf16_hex(out: &mut Vec<u8>, s: &str) {
    out.push(b'<');
    for u in s.encode_utf16() {
        out.extend_from_slice(format!("{u:04X}").as_bytes());
    }
    out.push(b'>');
}

fn to_unicode(entries: &[(u16, &str)]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(
        b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
          /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
          /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
          1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        c.extend_from_slice(format!("{} beginbfchar\n", chunk.len()).as_bytes());
        for (cid, text) in chunk {
            c.extend_from_slice(format!("<{cid:04X}> ").as_bytes());
            utf16_hex(&mut c, text);
            c.push(b'\n');
        }
        c.extend_from_slice(b"endbfchar\n");
    }
    c.extend_from_slice(b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    c
}

/// Writes the font objects; `entry.obj` (the Type0 dictionary) was allocated earlier.
pub(crate) fn write<W: Write>(
    out: &mut Out<W>,
    entry: &FontEntry,
    prep: &Prepared,
    compress: bool,
) -> Result<(), PdfError> {
    let cid_font = out.alloc()?;
    let descriptor = out.alloc()?;
    let file = out.alloc()?;
    let tounicode_text: Vec<(u16, &str)> = prep
        .cid_of
        .iter()
        .filter_map(|(g, cid)| {
            entry
                .text
                .get(g)
                .filter(|t| !t.is_empty())
                .map(|t| (*cid, t.as_str()))
        })
        .collect();
    let mut tu: Vec<(u16, &str)> = tounicode_text;
    tu.sort_by_key(|(c, _)| *c);
    let tounicode = if tu.is_empty() {
        None
    } else {
        let id = out.alloc()?;
        out.stream(id, b"", &to_unicode(&tu), compress)?;
        Some(id)
    };

    let mut n = Vec::new();
    name(&mut n, &prep.desc.name);
    let base = String::from_utf8_lossy(&n).into_owned();

    // Type0
    let mut t0 = format!(
        "<</Type/Font/Subtype/Type0/BaseFont{base}/Encoding/Identity-H/DescendantFonts[{cid_font} 0 R]"
    );
    if let Some(t) = tounicode {
        t0.push_str(&format!("/ToUnicode {t} 0 R"));
    }
    t0.push_str(">>");
    out.object(entry.obj, t0.as_bytes())?;

    // CIDFont with /W
    let mut widths: Vec<(u16, u16)> = prep
        .cid_of
        .iter()
        .map(|(g, cid)| {
            (
                *cid,
                entry.widths.get(usize::from(*g)).copied().unwrap_or(0),
            )
        })
        .collect();
    widths.sort_unstable();
    widths.dedup_by_key(|w| w.0);
    let mut w = String::from("[");
    let mut i = 0;
    while i < widths.len() {
        let mut j = i + 1;
        while j < widths.len() && widths[j].0 == widths[j - 1].0.wrapping_add(1) {
            j += 1;
        }
        w.push_str(&format!("{}[", widths[i].0));
        for (k, item) in widths[i..j].iter().enumerate() {
            if k > 0 {
                w.push(' ');
            }
            w.push_str(&item.1.to_string());
        }
        w.push_str("]\n");
        i = j;
    }
    w.push(']');
    let truetype = prep.program_kind == ProgramKind::TrueType;
    let mut cf = format!(
        "<</Type/Font/Subtype/{}/BaseFont{base}\
         /CIDSystemInfo<</Registry(Adobe)/Ordering(Identity)/Supplement 0>>\
         /FontDescriptor {descriptor} 0 R/DW 1000/W{w}",
        if truetype {
            "CIDFontType2"
        } else {
            "CIDFontType0"
        }
    );
    if truetype {
        cf.push_str("/CIDToGIDMap/Identity");
    }
    cf.push_str(">>");
    out.object(cid_font, cf.as_bytes())?;

    // Descriptor
    let d = &prep.desc;
    let key = if truetype { "FontFile2" } else { "FontFile3" };
    let mut ds = Vec::new();
    {
        let mut b = crate::out::Buf::default();
        b.num(d.italic_angle, 2);
        ds.extend_from_slice(&b.v);
    }
    let desc = format!(
        "<</Type/FontDescriptor/FontName{base}/Flags {}/FontBBox[{} {} {} {}]/ItalicAngle {}\
         /Ascent {}/Descent {}/CapHeight {}/StemV {}/{key} {file} 0 R>>",
        d.flags,
        d.bbox[0],
        d.bbox[1],
        d.bbox[2],
        d.bbox[3],
        String::from_utf8_lossy(&ds),
        d.ascent,
        d.descent,
        d.cap_height,
        d.stem_v
    );
    out.object(descriptor, desc.as_bytes())?;

    // Program
    let dict = if truetype {
        format!("/Length1 {}", prep.program.len())
    } else if prep.opentype {
        "/Subtype/OpenType".to_owned()
    } else {
        "/Subtype/CIDFontType0C".to_owned()
    };
    out.stream(file, dict.as_bytes(), &prep.program, compress)
}
