// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-330: CFF subsetting on synthetic fonts built by the shared CFF builder
//! and on the checked-in synthetic OpenType/CFF oracle.

mod common;

use atelier_font::cff::CffFont;
use atelier_font::subset::{
    ProgramKind, SubsetError, SubsetFont, subset_cff, subset_face, subset_font,
};
use atelier_font::{FontFace, OutlineLimits};
use common::cff_builder::{Fd, Spec, build, index};

const RMOVETO: u8 = 21;
const RLINETO: u8 = 5;
const RRCURVETO: u8 = 8;
const CALLSUBR: u8 = 10;
const RETURN: u8 = 11;
const ENDCHAR: u8 = 14;
const CALLGSUBR: u8 = 29;
const HSTEM: u8 = 1;
const HINTMASK: u8 = 19;

enum T {
    N(i32),
    O(u8),
    E(u8),
    R(&'static [u8]),
}
use T::*;

fn num(v: i32) -> Vec<u8> {
    match v {
        -107..=107 => vec![(v + 139) as u8],
        108..=1131 => {
            let w = v - 108;
            vec![(w / 256 + 247) as u8, (w % 256) as u8]
        }
        -1131..=-108 => {
            let w = -v - 108;
            vec![(w / 256 + 251) as u8, (w % 256) as u8]
        }
        _ => vec![28, (v >> 8) as u8, v as u8],
    }
}
fn cs(parts: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        match p {
            N(v) => out.extend(num(*v)),
            O(op) => out.push(*op),
            E(op) => out.extend([12, *op]),
            R(b) => out.extend_from_slice(b),
        }
    }
    out
}
fn line_sub(dx: i32, dy: i32) -> Vec<u8> {
    cs(&[N(dx), N(dy), O(RLINETO), O(RETURN)])
}
fn simple_fd() -> Fd {
    Fd {
        default_w: 250,
        nominal_w: 600,
        ..Fd::default()
    }
}
fn width_glyph(w: i32, x: i32) -> Vec<u8> {
    cs(&[
        N(w),
        N(x),
        N(5),
        O(RMOVETO),
        N(40),
        N(60),
        O(RLINETO),
        O(ENDCHAR),
    ])
}

fn same_glyphs(orig: &[u8], sub: &SubsetFont) {
    let a = CffFont::parse(orig).unwrap();
    let b = CffFont::parse(&sub.data).unwrap();
    assert_eq!(b.glyph_count(), sub.glyphs.len());
    for g in &sub.glyphs {
        let x = a.glyph(g.old_gid, OutlineLimits::default()).unwrap();
        let y = b.glyph(g.new_gid, OutlineLimits::default()).unwrap();
        assert_eq!(x, y, "glyph {}", g.old_gid);
        assert_eq!(g.advance, x.width);
    }
}

/// Name-keyed font with subroutines (local 3, global 2, nested calls) and custom glyph names.
fn named_spec() -> Spec {
    let mut glyphs = vec![cs(&[O(ENDCHAR)])];
    for i in 1..=9 {
        glyphs.push(cs(&[
            N(i * 3),
            N(0),
            O(RMOVETO),
            N(i % 3 - 107),
            O(CALLSUBR),
            N(i % 2 - 107),
            O(CALLGSUBR),
            O(ENDCHAR),
        ]));
    }
    let mut charset = vec![0u8];
    for i in 1..=9u16 {
        let sid = if i <= 3 { 390 + i } else { 33 + i };
        charset.extend(sid.to_be_bytes());
    }
    Spec {
        glyphs,
        gsubrs: vec![line_sub(5, 5), line_sub(7, 1)],
        fds: vec![Fd {
            lsubrs: vec![line_sub(1, 2), line_sub(3, 4), line_sub(9, 9)],
            ..simple_fd()
        }],
        charset: Some(charset),
        strings: vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()],
        ..Spec::default()
    }
}

#[test]
fn name_keyed_subset_keeps_outlines_widths_and_names() {
    let bytes = build(&named_spec());
    let sub = subset_cff(&bytes, &[2, 5, 5]).unwrap();
    assert_eq!(sub.kind, ProgramKind::Cff);
    assert!(!sub.cid_keyed);
    assert_eq!(sub.post_script_name, "Test");
    assert_eq!(
        sub.glyphs
            .iter()
            .map(|g| (g.old_gid, g.new_gid, g.cid))
            .collect::<Vec<_>>(),
        vec![(0, 0, 0), (2, 1, 1), (5, 2, 2)]
    );
    same_glyphs(&bytes, &sub);
    let orig = CffFont::parse(&bytes).unwrap();
    let out = CffFont::parse(&sub.data).unwrap();
    assert_eq!(out.glyph_name(1).unwrap(), orig.glyph_name(2).unwrap());
    assert_eq!(out.glyph_name(1).unwrap(), Some(&b"beta"[..]));
    assert_eq!(out.glyph_name(2).unwrap(), orig.glyph_name(5).unwrap());
    // The String INDEX only keeps "beta", renumbered to the first custom SID.
    assert_eq!(out.string(391), Some(&b"beta"[..]));
    assert_eq!(out.string(392), None);
    assert!(sub.data.len() < bytes.len());
}

#[test]
fn all_glyph_subset_roundtrips_and_is_deterministic() {
    let bytes = build(&named_spec());
    let all: Vec<u16> = (0..10).collect();
    let sub = subset_cff(&bytes, &all).unwrap();
    same_glyphs(&bytes, &sub);
    assert_eq!(sub, subset_cff(&bytes, &all).unwrap());
    let none = subset_cff(&bytes, &[]).unwrap();
    assert_eq!(none.glyphs.len(), 1);
    same_glyphs(&bytes, &none);
    assert_eq!(
        subset_cff(&bytes, &[10]).unwrap_err(),
        SubsetError::GlyphOutOfRange(10)
    );
}

#[test]
fn unused_subroutines_are_pruned_and_nested_calls_survive() {
    // 300 local subroutines; glyph 1 calls subr 0, which calls subr 250; glyph 2 calls nothing.
    let mut lsubrs: Vec<Vec<u8>> = (0..300)
        .map(|i| {
            let mut v = cs(&[N(i % 100), N(1), O(RLINETO)]);
            v.extend(vec![0x8b; 40]); // padding numbers
            v.extend([11]);
            v
        })
        .collect();
    lsubrs[0] = cs(&[N(250 - 107), O(CALLSUBR), N(4), N(4), O(RLINETO), O(RETURN)]);
    lsubrs[250] = line_sub(77, 88);
    let spec = Spec {
        glyphs: vec![
            cs(&[O(ENDCHAR)]),
            cs(&[N(0), N(0), O(RMOVETO), N(-107), O(CALLSUBR), O(ENDCHAR)]),
            width_glyph(300, 1),
        ],
        fds: vec![Fd {
            lsubrs,
            ..simple_fd()
        }],
        ..Spec::default()
    };
    let bytes = build(&spec);
    let sub = subset_cff(&bytes, &[1]).unwrap();
    same_glyphs(&bytes, &sub);
    // Subroutines 0 and 250 are the only ones kept with a body; the rest are one-byte returns.
    assert!(
        sub.data.len() < bytes.len() / 4,
        "{} < {}",
        sub.data.len(),
        bytes.len()
    );
    let no_calls = subset_cff(&bytes, &[2]).unwrap();
    same_glyphs(&bytes, &no_calls);
    assert!(no_calls.data.len() < 120, "{}", no_calls.data.len());
}

#[test]
fn bias_is_preserved_when_trailing_subroutines_are_dropped() {
    // 1300 subroutines: bias 1131. Glyph 1 calls subr 5 (operand 5 - 1131).
    let lsubrs: Vec<Vec<u8>> = (0..1300).map(|i| line_sub(i % 50, 1)).collect();
    let spec = Spec {
        glyphs: vec![
            cs(&[O(ENDCHAR)]),
            cs(&[N(0), N(0), O(RMOVETO), N(5 - 1131), O(CALLSUBR), O(ENDCHAR)]),
        ],
        fds: vec![Fd {
            lsubrs,
            ..simple_fd()
        }],
        ..Spec::default()
    };
    let bytes = build(&spec);
    let sub = subset_cff(&bytes, &[1]).unwrap();
    same_glyphs(&bytes, &sub);
    assert!(sub.data.len() < bytes.len() / 2);
}

#[test]
fn hint_masks_are_walked_with_the_right_length() {
    // 2 hstems then a hintmask with one mask byte, then a global subroutine call.
    let spec = Spec {
        glyphs: vec![
            cs(&[O(ENDCHAR)]),
            cs(&[
                N(0),
                N(10),
                N(20),
                N(10),
                O(HSTEM),
                O(HINTMASK),
                R(&[0xc0]),
                N(0),
                N(0),
                O(RMOVETO),
                N(-107),
                O(CALLGSUBR),
                O(ENDCHAR),
            ]),
        ],
        gsubrs: vec![line_sub(11, 12)],
        fds: vec![simple_fd()],
        ..Spec::default()
    };
    let bytes = build(&spec);
    let sub = subset_cff(&bytes, &[1]).unwrap();
    same_glyphs(&bytes, &sub);
}

// ------------------------------------------------------------------ seac

fn seac_spec() -> Spec {
    let a = cs(&[
        N(0),
        N(0),
        O(RMOVETO),
        N(100),
        N(0),
        N(0),
        N(200),
        O(RLINETO),
        O(ENDCHAR),
    ]);
    let acute = cs(&[
        N(10),
        N(300),
        O(RMOVETO),
        N(20),
        N(40),
        O(RLINETO),
        O(ENDCHAR),
    ]);
    // StandardEncoding: 65 = A (SID 34), 194 = acute (SID 125).
    let composite = cs(&[N(650), N(15), N(250), N(65), N(194), O(ENDCHAR)]);
    let unrelated = width_glyph(400, 3);
    Spec {
        glyphs: vec![cs(&[O(ENDCHAR)]), a, acute, composite, unrelated],
        fds: vec![simple_fd()],
        charset: Some(vec![0, 0, 34, 0, 125, 0, 35, 0, 36]),
        ..Spec::default()
    }
}

#[test]
fn seac_components_are_kept() {
    let bytes = build(&seac_spec());
    let sub = subset_cff(&bytes, &[3]).unwrap();
    let kept: Vec<u16> = sub.glyphs.iter().map(|g| g.old_gid).collect();
    assert_eq!(kept, vec![0, 1, 2, 3]);
    same_glyphs(&bytes, &sub);
    let out = CffFont::parse(&sub.data).unwrap();
    assert_eq!(out.glyph_for_sid(34).unwrap(), Some(1));
    assert_eq!(out.glyph_for_sid(125).unwrap(), Some(2));
    // Without the composite the components are not dragged in.
    let plain = subset_cff(&bytes, &[4]).unwrap();
    assert_eq!(plain.glyphs.len(), 2);
    same_glyphs(&bytes, &plain);
}

#[test]
fn seac_with_a_missing_component_is_a_typed_error() {
    let mut spec = seac_spec();
    spec.charset = Some(vec![0, 0, 34, 0, 36, 0, 35, 0, 37]); // no acute in the charset
    let bytes = build(&spec);
    assert!(matches!(subset_cff(&bytes, &[3]), Err(SubsetError::Cff(_))));
}

// ------------------------------------------------------------------ CID

fn cid_spec(fd_select: Vec<u8>) -> Spec {
    let main = |sub: i32| {
        cs(&[
            N(0),
            N(0),
            O(RMOVETO),
            N(sub - 107),
            O(CALLSUBR),
            O(ENDCHAR),
        ])
    };
    Spec {
        glyphs: vec![
            cs(&[O(ENDCHAR)]),
            main(0),
            main(0),
            main(0),
            width_glyph(5, 2),
        ],
        fds: vec![
            Fd {
                lsubrs: vec![line_sub(1, 0), line_sub(8, 8)],
                default_w: 111,
                ..Fd::default()
            },
            Fd {
                lsubrs: vec![line_sub(0, 2)],
                default_w: 222,
                ..Fd::default()
            },
            Fd {
                default_w: 333,
                ..Fd::default()
            },
        ],
        fd_select: Some(fd_select),
        // Format 2 charset: glyphs 1..=4 are CIDs 100..=103.
        charset: Some(vec![2, 0x00, 0x64, 0x00, 0x03]),
        ..Spec::default()
    }
}

#[test]
fn cid_keyed_subset_selects_font_dicts_and_keeps_cids() {
    // Format 3: glyphs 0..2 -> FD 0, 2..4 -> FD 1, 4 -> FD 2.
    let f3 = vec![3, 0, 3, 0, 0, 0, 0, 2, 1, 0, 4, 2, 0, 5];
    // Format 0.
    let f0 = vec![0, 0, 0, 0, 1, 1, 2];
    for sel in [f3, f0] {
        let bytes = build(&cid_spec(sel));
        let orig = CffFont::parse(&bytes).unwrap();
        assert!(orig.is_cid());
        for set in [
            vec![3u16],
            vec![1, 3],
            vec![2],
            vec![4],
            vec![1, 2, 3, 4],
            vec![],
        ] {
            let sub = subset_cff(&bytes, &set).unwrap();
            assert!(sub.cid_keyed);
            same_glyphs(&bytes, &sub);
            let out = CffFont::parse(&sub.data).unwrap();
            assert!(out.is_cid());
            for g in &sub.glyphs {
                let expected_cid = if g.old_gid == 0 { 0 } else { 99 + g.old_gid };
                assert_eq!(g.cid, expected_cid);
                assert_eq!(out.glyph_id_in_charset(g.new_gid).unwrap(), expected_cid);
                // The Font DICT of every glyph carries the same default width (111/222/333).
                let fd_old = orig.font_dict_index(g.old_gid).unwrap();
                let fd_new = out.font_dict_index(g.new_gid).unwrap();
                let w =
                    |f: &CffFont<'_>, gid| f.glyph(gid, OutlineLimits::default()).unwrap().width;
                assert_eq!(w(&out, g.new_gid), w(&orig, g.old_gid));
                let _ = (fd_old, fd_new);
            }
        }
        // Only the Font DICTs in use survive.
        let s1 = subset_cff(&bytes, &[3]).unwrap();
        let only_fd1 = CffFont::parse(&s1.data).unwrap();
        assert_eq!(only_fd1.font_dict_count(), 2); // glyph 0 -> FD 0, glyph 3 -> FD 1
        let s2 = subset_cff(&bytes, &[4]).unwrap();
        let only_fd2 = CffFont::parse(&s2.data).unwrap();
        assert_eq!(only_fd2.font_dict_count(), 2); // glyph 0 -> FD 0, glyph 4 -> FD 2
        let s0 = subset_cff(&bytes, &[]).unwrap();
        assert_eq!(CffFont::parse(&s0.data).unwrap().font_dict_count(), 1);
        assert_eq!(
            orig.registry_ordering_supplement(),
            only_fd2.registry_ordering_supplement()
        );
    }
}

// ------------------------------------------------------------------ OpenType/CFF

fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_be_bytes());
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_be_bytes());
}

/// A minimal OpenType/CFF font around `cff`, with `OS/2.fsType = fs_type`.
fn otto(cff: Vec<u8>, glyphs: u16, fs_type: u16) -> Vec<u8> {
    let mut head = vec![0; 54];
    put32(&mut head, 0, 0x0001_0000);
    put32(&mut head, 12, 0x5f0f_3cf5);
    put16(&mut head, 18, 1000);
    put16(&mut head, 36, (-50i16) as u16);
    put16(&mut head, 38, (-250i16) as u16);
    put16(&mut head, 40, 1100);
    put16(&mut head, 42, 900);
    let mut hhea = vec![0; 36];
    put32(&mut hhea, 0, 0x0001_0000);
    put16(&mut hhea, 4, 800);
    put16(&mut hhea, 6, (-200i16) as u16);
    put16(&mut hhea, 34, glyphs);
    let mut maxp = vec![0; 6];
    put32(&mut maxp, 0, 0x0000_5000);
    put16(&mut maxp, 4, glyphs);
    let mut os2 = vec![0; 78];
    put16(&mut os2, 4, 400);
    put16(&mut os2, 6, 5);
    put16(&mut os2, 8, fs_type);
    let mut cmap = vec![0; 28];
    put16(&mut cmap, 2, 1);
    put16(&mut cmap, 4, 3);
    put16(&mut cmap, 6, 10);
    put32(&mut cmap, 8, 12);
    put16(&mut cmap, 12, 12);
    put32(&mut cmap, 16, 28);
    put32(&mut cmap, 24, 1);
    cmap.extend_from_slice(&0x41u32.to_be_bytes());
    cmap.extend_from_slice(&0x41u32.to_be_bytes());
    cmap.extend_from_slice(&1u32.to_be_bytes());
    let mut post = vec![0; 32];
    put32(&mut post, 0, 0x0003_0000);
    put32(&mut post, 4, 0xfff6_0000); // italic angle -10
    let mut hmtx = Vec::new();
    for g in 0..glyphs {
        hmtx.extend((500 + g).to_be_bytes());
        hmtx.extend(7u16.to_be_bytes());
    }
    // name: one record, PostScript name.
    let ps = b"Synthetic-Regular";
    let mut name = vec![0, 0, 0, 1, 0, 18, 0, 1, 0, 0, 0, 0, 0, 6];
    name.extend((ps.len() as u16).to_be_bytes());
    name.extend([0, 0]);
    name.extend(ps);
    let tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"CFF ", cff),
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ];
    let base = 12 + tables.len() * 16;
    let total = base + tables.iter().map(|(_, v)| v.len() + 3).sum::<usize>();
    let mut b = vec![0; total];
    b[..4].copy_from_slice(b"OTTO");
    put16(&mut b, 4, tables.len() as u16);
    let mut off = base;
    for (i, (tag, data)) in tables.iter().enumerate() {
        let p = 12 + i * 16;
        b[p..p + 4].copy_from_slice(tag);
        put32(&mut b, p + 8, off as u32);
        put32(&mut b, p + 12, data.len() as u32);
        b[off..off + data.len()].copy_from_slice(data);
        off += data.len().div_ceil(4) * 4;
    }
    b.truncate(off.min(total));
    b
}

#[test]
fn opentype_cff_uses_hmtx_advances_and_head_metrics() {
    let cff = build(&named_spec());
    let font = otto(cff.clone(), 10, 0);
    let sub = subset_font(&font, 0, &[4, 6]).unwrap();
    assert_eq!(sub.kind, ProgramKind::Cff);
    same_glyphs_ignoring_width(&cff, &sub);
    assert_eq!(sub.post_script_name, "Synthetic-Regular");
    for g in &sub.glyphs {
        assert_eq!(g.advance, f64::from(500 + g.old_gid));
    }
    assert_eq!(sub.metrics.bbox, [-50, -250, 1100, 900]);
    assert_eq!((sub.metrics.ascent, sub.metrics.descent), (800, -200));
    assert_eq!(sub.metrics.italic_angle, -10.0);
    assert!(sub.metrics.italic);
    // Rights apply to OpenType/CFF too.
    for (fs, expected) in [
        (2, Some(SubsetError::EmbeddingRestricted)),
        (0x200, Some(SubsetError::BitmapOnly)),
        (0x100, Some(SubsetError::SubsettingForbidden)),
        (4, None),
        (8, None),
        (0, None),
    ] {
        let r = subset_font(&otto(cff.clone(), 10, fs), 0, &[1]);
        assert_eq!(r.err(), expected, "fsType {fs:#x}");
    }
    let face = FontFace::parse(&font).unwrap();
    assert_eq!(subset_face(&face, &[4, 6]).unwrap(), sub);
}

fn same_glyphs_ignoring_width(orig: &[u8], sub: &SubsetFont) {
    let a = CffFont::parse(orig).unwrap();
    let b = CffFont::parse(&sub.data).unwrap();
    for g in &sub.glyphs {
        assert_eq!(
            a.outline(g.old_gid, OutlineLimits::default()).unwrap(),
            b.outline(g.new_gid, OutlineLimits::default()).unwrap()
        );
    }
}

#[test]
fn checked_in_fonttools_oracle_font_subsets_with_its_nested_seac() {
    let path = format!(
        "{}/../../tests/font-cff-reference/cff-oracle.otf",
        env!("CARGO_MANIFEST_DIR")
    );
    let font = std::fs::read(path).unwrap();
    let face = FontFace::parse(&font).unwrap();
    let table = face.table(*b"CFF ").unwrap();
    let n = face.metrics().number_of_glyphs;
    let all: Vec<u16> = (0..n).collect();
    let sub = subset_font(&font, 0, &all).unwrap();
    same_glyphs_ignoring_width(table, &sub);
    for g in 1..n {
        let one = subset_font(&font, 0, &[g]).unwrap();
        same_glyphs_ignoring_width(table, &one);
        for k in &one.glyphs {
            assert_eq!(
                k.advance,
                f64::from(face.horizontal_metric(k.old_gid).unwrap().advance_width)
            );
        }
    }
}

// ------------------------------------------------------------------ names, charsets, size

#[test]
fn large_font_charset_and_index_formats() {
    // 3000 glyphs: INDEX offsets over 16 bits, sequential SIDs (charset ranges).
    let glyphs: Vec<Vec<u8>> = (0..3000)
        .map(|i| {
            if i == 0 {
                cs(&[O(ENDCHAR)])
            } else {
                width_glyph(500 + i % 100, i % 90)
            }
        })
        .collect();
    let spec = Spec {
        glyphs,
        fds: vec![simple_fd()],
        strings: (391..3000).map(|i| format!("g{i}").into_bytes()).collect(),
        charset: Some({
            let mut c = vec![2u8, 0, 1];
            c.extend(2998u16.to_be_bytes());
            c
        }),
        ..Spec::default()
    };
    let bytes = build(&spec);
    let wanted: Vec<u16> = (1..200).chain(1000..1010).chain([2999]).collect();
    let sub = subset_cff(&bytes, &wanted).unwrap();
    same_glyphs(&bytes, &sub);
    assert!(sub.data.len() < bytes.len() / 5);
    let _ = index(&[]);
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

#[test]
fn seeded_mutation_and_truncation_campaign_never_panics() {
    let bases = vec![
        build(&named_spec()),
        build(&seac_spec()),
        build(&cid_spec(vec![3, 0, 3, 0, 0, 0, 0, 2, 1, 0, 4, 2, 0, 5])),
        build(&cid_spec(vec![0, 0, 0, 1, 1, 2])),
    ];
    let mut rng = Rng(0x0330_cff0);
    for base in &bases {
        for _ in 0..4000 {
            let mut f = base.clone();
            for _ in 0..1 + rng.below(4) {
                let p = rng.below(f.len());
                f[p] = rng.step() as u8;
            }
            if rng.below(5) == 0 {
                f.truncate(rng.below(f.len()));
            }
            let glyphs: Vec<u16> = (0..rng.below(6)).map(|_| rng.below(12) as u16).collect();
            if let Ok(out) = subset_cff(&f, &glyphs) {
                // Whatever succeeds must be a valid CFF font.
                CffFont::parse(&out.data).unwrap();
            }
            let wrapped = otto(f, 10, rng.below(0x400) as u16);
            let _ = subset_font(&wrapped, 0, &glyphs);
        }
    }
}

#[test]
fn hostile_charstrings_are_bounded() {
    // Recursive subroutine, endless arithmetic and absurd operands.
    let recursive = Spec {
        glyphs: vec![cs(&[O(ENDCHAR)]), cs(&[N(-107), O(CALLSUBR), O(ENDCHAR)])],
        fds: vec![Fd {
            lsubrs: vec![cs(&[N(-107), O(CALLSUBR), O(RETURN)])],
            ..simple_fd()
        }],
        ..Spec::default()
    };
    assert!(subset_cff(&build(&recursive), &[1]).is_err());
    let underflow = Spec {
        glyphs: vec![cs(&[O(ENDCHAR)]), cs(&[O(CALLSUBR), O(ENDCHAR)])],
        fds: vec![simple_fd()],
        ..Spec::default()
    };
    assert!(subset_cff(&build(&underflow), &[1]).is_err());
    let wild = Spec {
        glyphs: vec![
            cs(&[O(ENDCHAR)]),
            cs(&[N(1), N(2), N(3), O(RRCURVETO), E(30), O(ENDCHAR)]),
        ],
        fds: vec![simple_fd()],
        ..Spec::default()
    };
    let _ = subset_cff(&build(&wild), &[1]);
}

#[test]
fn deprecated_dotsection_is_accepted_like_the_outline_reader() {
    let glyph = cs(&[
        N(0),
        N(10),
        O(HSTEM),
        E(0),
        N(5),
        N(5),
        O(RMOVETO),
        N(1),
        N(1),
        O(RLINETO),
        O(ENDCHAR),
    ]);
    let bytes = build(&Spec {
        glyphs: vec![cs(&[O(ENDCHAR)]), glyph],
        fds: vec![simple_fd()],
        ..Spec::default()
    });
    // The outline reader renders this glyph, so the subsetter must not refuse it.
    let font = CffFont::parse(&bytes).unwrap();
    font.glyph(1, OutlineLimits::default()).unwrap();
    let sub = subset_cff(&bytes, &[1]).unwrap();
    same_glyphs(&bytes, &sub);
}
