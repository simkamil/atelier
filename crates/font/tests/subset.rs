// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-330: TrueType subsetting, embedding rights and robustness.

use atelier_font::FontFace;
use atelier_font::embedded::EMBEDDED_FONTS;
use atelier_font::subset::{
    EmbeddingPermission, ProgramKind, SubsetError, embedding_rights, subset_face, subset_font,
};
use atelier_geom::Path;

const ASCII: std::ops::RangeInclusive<u32> = 32..=126;
const ACCENTED: &str = "ÀÁÂÃÄÅÇÈÉÊËÌÍÎÏÑÒÓÔÕÖÙÚÛÜÝàáâãäåçèéêëìíîïñòóôõöùúûüýÿ";

fn gids(face: &FontFace<'_>, text: impl IntoIterator<Item = char>) -> Vec<u16> {
    text.into_iter().map(|c| face.glyph_index(c)).collect()
}

fn ascii(face: &FontFace<'_>) -> Vec<u16> {
    gids(face, ASCII.filter_map(char::from_u32))
}

/// `(tag, offset, length, checksum)` of every table of a single-face sfnt.
fn directory(b: &[u8]) -> Vec<([u8; 4], usize, usize, u32)> {
    let n = usize::from(u16::from_be_bytes([b[4], b[5]]));
    (0..n)
        .map(|i| {
            let r = &b[12 + 16 * i..28 + 16 * i];
            let u = |p: usize| u32::from_be_bytes(r[p..p + 4].try_into().unwrap());
            (
                r[0..4].try_into().unwrap(),
                u(8) as usize,
                u(12) as usize,
                u(4),
            )
        })
        .collect()
}

fn sum(b: &[u8]) -> u32 {
    let mut s = 0u32;
    for c in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s = s.wrapping_add(u32::from_be_bytes(w));
    }
    s
}

fn check_structure(b: &[u8]) {
    let dir = directory(b);
    let n = dir.len();
    let mut es = 0;
    while 1usize << (es + 1) <= n {
        es += 1;
    }
    let sr = (1usize << es) * 16;
    assert_eq!(u16::from_be_bytes([b[6], b[7]]) as usize, sr);
    assert_eq!(u16::from_be_bytes([b[8], b[9]]) as usize, es);
    assert_eq!(u16::from_be_bytes([b[10], b[11]]) as usize, n * 16 - sr);
    assert!(
        dir.windows(2).all(|w| w[0].0 < w[1].0),
        "directory sorted by tag"
    );
    assert_eq!(b.len() % 4, 0);
    let mut prev_end = 12 + 16 * n;
    for (tag, off, len, cs) in &dir {
        assert_eq!(off % 4, 0, "{tag:?} aligned");
        assert!(*off >= prev_end);
        prev_end = off + len.div_ceil(4) * 4;
        let mut t = b[*off..off + len].to_vec();
        if tag == b"head" {
            t[8..12].fill(0);
        }
        assert_eq!(sum(&t), *cs, "checksum of {tag:?}");
        assert!(
            b[off + len..prev_end].iter().all(|&x| x == 0),
            "padding of {tag:?}"
        );
    }
    assert_eq!(prev_end, b.len());
    assert_eq!(sum(b), 0xb1b0_afba, "checkSumAdjustment");
}

fn check_subset(font: &[u8], wanted: &[u16]) -> atelier_font::subset::SubsetFont {
    let orig = FontFace::parse(font).unwrap();
    let sub = subset_font(font, 0, wanted).unwrap();
    assert_eq!(sub.kind, ProgramKind::TrueType);
    check_structure(&sub.data);
    let face = FontFace::parse(&sub.data).unwrap();
    assert!(sub.glyphs.windows(2).all(|w| w[0].old_gid < w[1].old_gid));
    assert_eq!(sub.glyphs[0].old_gid, 0);
    for g in wanted {
        assert!(sub.glyph(*g).is_some(), "requested glyph {g} kept");
    }
    for g in &sub.glyphs {
        assert_eq!((g.new_gid, g.cid), (g.old_gid, g.old_gid));
        assert_eq!(
            orig.outline(g.old_gid).unwrap(),
            face.outline(g.new_gid).unwrap()
        );
        let (a, b) = (
            orig.horizontal_metric(g.old_gid).unwrap(),
            face.horizontal_metric(g.new_gid).unwrap(),
        );
        assert_eq!(a, b);
        assert_eq!(g.advance, f64::from(a.advance_width));
    }
    let kept: std::collections::BTreeSet<u16> = sub.glyphs.iter().map(|g| g.old_gid).collect();
    for gid in 0..face.metrics().number_of_glyphs {
        if !kept.contains(&gid) {
            assert_eq!(
                face.outline(gid).unwrap(),
                Path::new(),
                "dropped glyph {gid} is empty"
            );
        }
    }
    assert_eq!(face.metrics().units_per_em, orig.metrics().units_per_em);
    assert_eq!(face.metrics().fs_type, orig.metrics().fs_type);
    for t in [*b"cvt ", *b"fpgm", *b"prep"] {
        assert_eq!(face.table(t), orig.table(t));
    }
    // Same global bounding box as the source.
    assert_eq!(
        face.table(*b"head").unwrap()[36..44],
        orig.table(*b"head").unwrap()[36..44]
    );
    sub
}

#[test]
fn embedded_fonts_subset_to_identical_outlines_and_metrics() {
    for font in EMBEDDED_FONTS {
        let face = FontFace::parse(font.data).unwrap();
        let wanted = ascii(&face);
        let sub = check_subset(font.data, &wanted);
        assert!(
            sub.data.len() * 3 < font.data.len(),
            "{} ASCII subset is small",
            font.family
        );
        let mut acc = gids(&face, ACCENTED.chars());
        acc.extend(&wanted);
        let sub = check_subset(font.data, &acc);
        assert!(sub.data.len() * 2 < font.data.len());
    }
}

#[test]
fn composite_components_are_added_to_the_subset() {
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let accented = gids(&face, ACCENTED.chars());
    let sub = check_subset(font, &accented);
    assert!(
        sub.glyphs.len() > accented.len() / 2 + 1,
        "components of accented composites are kept ({} glyphs)",
        sub.glyphs.len()
    );
    assert!(
        sub.glyph(face.glyph_index('e')).is_some() || sub.glyph(face.glyph_index('E')).is_some()
    );
}

#[test]
fn single_glyph_empty_set_and_duplicates() {
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let a = face.glyph_index('A');
    let one = check_subset(font, &[a]);
    assert_eq!(one.glyphs.len(), 2);
    assert!(one.data.len() < 6000, "{}", one.data.len());
    let none = check_subset(font, &[]);
    assert_eq!(none.glyphs.len(), 1);
    let dup = check_subset(font, &[a, a, 0, a]);
    assert_eq!(dup, one);
}

#[test]
fn all_glyphs_subset_is_equivalent() {
    for idx in [0, 4, 8] {
        let font = EMBEDDED_FONTS[idx].data;
        let n = FontFace::parse(font).unwrap().metrics().number_of_glyphs;
        let all: Vec<u16> = (0..n).collect();
        let sub = check_subset(font, &all);
        assert_eq!(sub.glyphs.len(), usize::from(n));
        assert!(
            sub.data.len() < font.len(),
            "dropped tables make it smaller"
        );
    }
}

#[test]
fn output_is_deterministic_and_describes_the_font() {
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let w = ascii(&face);
    let mut shuffled = w.clone();
    shuffled.reverse();
    let a = subset_font(font, 0, &w).unwrap();
    let b = subset_font(font, 0, &shuffled).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.pdf_subset_tag(), b.pdf_subset_tag());
    assert!(a.pdf_subset_tag().iter().all(u8::is_ascii_uppercase));
    assert_eq!(a.post_script_name, "Carlito-Regular");
    assert_eq!(a.metrics.units_per_em, 2048);
    assert!(a.metrics.ascent > 0 && a.metrics.descent < 0);
    assert!(a.metrics.cap_height.is_some() && a.metrics.x_height.is_some());
    assert_eq!(a.metrics.italic_angle, 0.0);
    assert_eq!(a.metrics.descriptor_flags() & 4, 4);
    let bold = EMBEDDED_FONTS
        .iter()
        .find(|f| f.family == "Liberation Mono" && f.weight == 700)
        .unwrap();
    let bf = FontFace::parse(bold.data).unwrap();
    let m = subset_font(bold.data, 0, &ascii(&bf)).unwrap();
    assert!(m.metrics.fixed_pitch && m.metrics.bold);
    assert_eq!(m.metrics.descriptor_flags() & 1, 1);
    assert_eq!(m.post_script_name, "LiberationMono-Bold");
    assert!(m.metrics.stem_v_estimate() > a.metrics.stem_v_estimate());
    let italic = EMBEDDED_FONTS
        .iter()
        .find(|f| {
            f.family == "Caladea" && f.style == atelier_font::embedded::EmbeddedFontStyle::Italic
        })
        .unwrap();
    let it = subset_font(italic.data, 0, &[1]).unwrap();
    assert!(it.metrics.italic && it.metrics.italic_angle < 0.0);
}

#[test]
fn glyph_out_of_range_is_a_typed_error() {
    let font = EMBEDDED_FONTS[0].data;
    let n = FontFace::parse(font).unwrap().metrics().number_of_glyphs;
    assert_eq!(
        subset_font(font, 0, &[n]).unwrap_err(),
        SubsetError::GlyphOutOfRange(n)
    );
    assert_eq!(
        subset_font(font, 0, &[1, u16::MAX]).unwrap_err(),
        SubsetError::GlyphOutOfRange(u16::MAX)
    );
    assert!(matches!(
        subset_font(font, 3, &[1]),
        Err(SubsetError::Font(_))
    ));
    assert!(matches!(
        subset_font(b"nonsense", 0, &[1]),
        Err(SubsetError::Font(_))
    ));
}

// ------------------------------------------------------------------ embedding rights

/// Copy of `font` whose `OS/2.fsType` is `fs_type`.
fn with_fs_type(font: &[u8], fs_type: u16) -> Vec<u8> {
    let mut v = font.to_vec();
    let (_, off, _, _) = directory(&v).into_iter().find(|t| &t.0 == b"OS/2").unwrap();
    v[off + 8..off + 10].copy_from_slice(&fs_type.to_be_bytes());
    v
}

#[test]
fn every_fs_type_pattern_follows_the_specification() {
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let w = ascii(&face);
    for fs in 0..=0x03ffu16 {
        let patched = with_fs_type(font, fs);
        let result = subset_font(&patched, 0, &w);
        let rights = embedding_rights(Some(fs));
        let usage = fs & 0xe;
        if fs & 0x200 != 0 {
            assert_eq!(rights.unwrap_err(), SubsetError::BitmapOnly, "{fs:#x}");
            assert_eq!(result.unwrap_err(), SubsetError::BitmapOnly);
        } else if usage == 2 {
            assert_eq!(
                rights.unwrap_err(),
                SubsetError::EmbeddingRestricted,
                "{fs:#x}"
            );
            assert_eq!(result.unwrap_err(), SubsetError::EmbeddingRestricted);
        } else {
            let r = rights.unwrap();
            let expect = if usage & 8 != 0 {
                EmbeddingPermission::Editable
            } else if usage & 4 != 0 {
                EmbeddingPermission::PreviewPrint
            } else {
                EmbeddingPermission::Installable
            };
            assert_eq!(r.permission, expect, "{fs:#x}");
            assert_eq!(r.subsetting_allowed, fs & 0x100 == 0);
            if fs & 0x100 != 0 {
                assert_eq!(result.unwrap_err(), SubsetError::SubsettingForbidden);
            } else {
                assert_eq!(result.unwrap().rights, r);
            }
        }
    }
    // Reserved bits do not matter.
    assert!(embedding_rights(Some(0xfc01)).is_ok());
}

#[test]
fn missing_os2_means_installable() {
    let r = embedding_rights(None).unwrap();
    assert_eq!(r.permission, EmbeddingPermission::Installable);
    assert!(r.subsetting_allowed);
}

// ------------------------------------------------------------------ robustness

struct Rng(u64);
impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.step() % n as u64) as usize
    }
}

fn file_offset(whole: &[u8], part: &[u8]) -> usize {
    part.as_ptr() as usize - whole.as_ptr() as usize
}

#[test]
fn cyclic_and_out_of_range_composites_are_typed_errors() {
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let e = face.glyph_index('é');
    let glyf = face.table(*b"glyf").unwrap();
    let loca = face.table(*b"loca").unwrap();
    let long = face.metrics().index_to_loc_format == 1;
    let at = |g: u16| -> usize {
        if long {
            u32::from_be_bytes(loca[usize::from(g) * 4..][..4].try_into().unwrap()) as usize
        } else {
            usize::from(u16::from_be_bytes(
                loca[usize::from(g) * 2..][..2].try_into().unwrap(),
            )) * 2
        }
    };
    let start = file_offset(font, glyf) + at(e);
    assert!(
        i16::from_be_bytes([font[start], font[start + 1]]) < 0,
        "é is a composite"
    );
    // First component refers to the composite itself.
    let mut cyc = font.to_vec();
    cyc[start + 12..start + 14].copy_from_slice(&e.to_be_bytes());
    assert_eq!(
        subset_font(&cyc, 0, &[e]).unwrap_err(),
        SubsetError::InvalidData("cyclic composite glyph")
    );
    // Component past the glyph count.
    let mut oob = font.to_vec();
    oob[start + 12..start + 14].copy_from_slice(&u16::MAX.to_be_bytes());
    assert_eq!(
        subset_font(&oob, 0, &[e]).unwrap_err(),
        SubsetError::InvalidData("component glyph id")
    );
    // Unrequested broken composites are not visited.
    assert!(subset_font(&oob, 0, &[face.glyph_index('A')]).is_ok());
}

#[test]
fn seeded_mutation_and_truncation_campaign_never_panics() {
    let mut rng = Rng(0x05ee_d330);
    // Tiny synthetic fixtures: whole-file mutations.
    for name in ["font-0-4", "font-0-12", "font-1-6", "font-1-12"] {
        let base = std::fs::read(format!(
            "{}/tests/fixtures/{name}.font",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        for _ in 0..1500 {
            let mut f = base.clone();
            for _ in 0..1 + rng.below(4) {
                let p = rng.below(f.len());
                f[p] = rng.step() as u8;
            }
            if rng.below(4) == 0 {
                f.truncate(rng.below(f.len()));
            }
            let glyphs: Vec<u16> = (0..rng.below(5)).map(|_| rng.below(8) as u16).collect();
            let _ = subset_font(&f, 0, &glyphs);
        }
    }
    // A real font: mutate the structural tables.
    let font = EMBEDDED_FONTS[0].data;
    let face = FontFace::parse(font).unwrap();
    let w = gids(&face, "Aé,".chars());
    let mut regions = Vec::new();
    for t in [
        *b"head", *b"hhea", *b"maxp", *b"loca", *b"OS/2", *b"name", *b"hmtx", *b"post",
    ] {
        let b = face.table(t).unwrap();
        regions.push((file_offset(font, b), b.len().min(4096)));
    }
    let g = face.table(*b"glyf").unwrap();
    let ge = face.glyph_index('é');
    regions.push((file_offset(font, g), g.len()));
    for _ in 0..400 {
        let mut f = font.to_vec();
        let (s, l) = regions[rng.below(regions.len())];
        for _ in 0..1 + rng.below(3) {
            let p = s + rng.below(l);
            f[p] = rng.step() as u8;
        }
        let _ = subset_font(&f, 0, &w);
        let _ = subset_font(&f, 0, &[ge, 1, 2, 3]);
    }
    for _ in 0..40 {
        let cut = rng.below(font.len());
        let _ = subset_font(&font[..cut], 0, &w);
    }
    // Random glyph ids.
    for _ in 0..200 {
        let ids: Vec<u16> = (0..rng.below(10)).map(|_| rng.step() as u16).collect();
        let _ = subset_face(&face, &ids);
    }
}
