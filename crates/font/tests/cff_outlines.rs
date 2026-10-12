// SPDX-License-Identifier: AGPL-3.0-or-later
//! CFF / Type 2 outline tests on synthetic fonts assembled by a small builder.
//! No real CFF font is embedded in the repository.

use atelier_font::cff::{CffFont, MAX_OPERATIONS};
use atelier_font::{FontFace, OutlineError, OutlineLimits};
use atelier_geom::{Path, Point};

// ---------------------------------------------------------------- charstrings

#[derive(Clone, Copy)]
enum T {
    N(i32),
    O(u8),
    E(u8),
    R(&'static [u8]),
}
use T::*;

const HSTEM: u8 = 1;
const VSTEM: u8 = 3;
const VMOVETO: u8 = 4;
const RLINETO: u8 = 5;
const HLINETO: u8 = 6;
const VLINETO: u8 = 7;
const RRCURVETO: u8 = 8;
const CALLSUBR: u8 = 10;
const RETURN: u8 = 11;
const ENDCHAR: u8 = 14;
const HSTEMHM: u8 = 18;
const HINTMASK: u8 = 19;
const CNTRMASK: u8 = 20;
const RMOVETO: u8 = 21;
const HMOVETO: u8 = 22;
const VSTEMHM: u8 = 23;
const RCURVELINE: u8 = 24;
const RLINECURVE: u8 = 25;
const VVCURVETO: u8 = 26;
const HHCURVETO: u8 = 27;
const CALLGSUBR: u8 = 29;
const VHCURVETO: u8 = 30;
const HVCURVETO: u8 = 31;

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
        _ => {
            let mut b = vec![28];
            b.extend_from_slice(&(v as i16).to_be_bytes());
            b
        }
    }
}

fn cs(parts: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        match *p {
            N(v) => out.extend(num(v)),
            O(op) => out.push(op),
            E(op) => out.extend([12, op]),
            R(b) => out.extend_from_slice(b),
        }
    }
    out
}

fn ns(v: &[i32]) -> Vec<T> {
    v.iter().map(|&x| N(x)).collect()
}

/// `args... op endchar` after `0 0 rmoveto`.
fn drawn(args: &[i32], op: T) -> Vec<u8> {
    let mut v = vec![N(0), N(0), O(RMOVETO)];
    v.extend(ns(args));
    v.push(op);
    v.push(O(ENDCHAR));
    cs(&v)
}
fn drawn_escape(args: &[i32], esc: u8) -> Vec<u8> {
    drawn(args, E(esc))
}

// ---------------------------------------------------------------- expected paths

struct Pb(Path);
fn p(x: f64, y: f64) -> Point {
    Point::try_new(x, y).unwrap()
}
impl Pb {
    fn new() -> Self {
        Pb(Path::new())
    }
    fn m(mut self, x: f64, y: f64) -> Self {
        self.0.move_to(p(x, y));
        self
    }
    fn l(mut self, x: f64, y: f64) -> Self {
        self.0.line_to(p(x, y)).unwrap();
        self
    }
    fn c(mut self, a: (f64, f64), b: (f64, f64), e: (f64, f64)) -> Self {
        self.0
            .cubic_to(p(a.0, a.1), p(b.0, b.1), p(e.0, e.1))
            .unwrap();
        self
    }
    fn z(mut self) -> Self {
        self.0.close().unwrap();
        self
    }
    fn done(self) -> Path {
        self.0
    }
}

// ---------------------------------------------------------------- CFF builder

fn index_with(items: &[Vec<u8>], off_size: usize) -> Vec<u8> {
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    out.push(off_size as u8);
    let mut off = 1usize;
    let put = |out: &mut Vec<u8>, v: usize| {
        out.extend_from_slice(&(v as u32).to_be_bytes()[4 - off_size..]);
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
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = items.iter().map(Vec::len).sum::<usize>() + 1;
    let off_size = if total < 256 {
        1
    } else if total < 65536 {
        2
    } else {
        4
    };
    index_with(items, off_size)
}

fn d29(v: i32) -> Vec<u8> {
    let mut b = vec![29];
    b.extend_from_slice(&v.to_be_bytes());
    b
}

/// DICT real operand (operator 30) from decimal text such as `-0.5` or `1E-3`.
fn real(text: &str) -> Vec<u8> {
    let mut nibbles = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        nibbles.push(match c {
            '0'..='9' => c as u8 - b'0',
            '.' => 0xa,
            '-' => 0xe,
            'E' if chars.peek() == Some(&'-') => {
                chars.next();
                0xc
            }
            'E' => 0xb,
            _ => panic!("bad real text"),
        });
    }
    nibbles.push(0xf);
    if nibbles.len() % 2 == 1 {
        nibbles.push(0xf);
    }
    let mut out = vec![30];
    out.extend(nibbles.chunks(2).map(|n| (n[0] << 4) | n[1]));
    out
}
/// FontMatrix DICT entry from six decimal texts.
fn matrix(v: [&str; 6]) -> Vec<u8> {
    let mut d: Vec<u8> = v.iter().flat_map(|t| real(t)).collect();
    d.extend([12, 7]);
    d
}

#[derive(Clone, Default)]
struct Fd {
    lsubrs: Vec<Vec<u8>>,
    default_w: i32,
    nominal_w: i32,
    font_matrix: Option<Vec<u8>>,
}

#[derive(Clone, Default)]
struct Spec {
    glyphs: Vec<Vec<u8>>,
    gsubrs: Vec<Vec<u8>>,
    fds: Vec<Fd>,
    /// Raw FDSelect bytes; presence makes the font CID-keyed.
    fd_select: Option<Vec<u8>>,
    /// Raw charset bytes (None: format 0 with SIDs 1..n).
    charset: Option<Vec<u8>>,
    /// Predefined charset number (0 ISOAdobe, 1 Expert, 2 ExpertSubset) instead of a table.
    charset_pre: Option<i32>,
    strings: Vec<Vec<u8>>,
    charstring_type: Option<i32>,
    font_matrix: Option<Vec<u8>>,
}

fn private_dict(fd: &Fd) -> (Vec<u8>, Vec<u8>) {
    let has_subrs = !fd.lsubrs.is_empty();
    let dict_len = 12 + if has_subrs { 6 } else { 0 };
    let mut d = d29(fd.default_w);
    d.push(20);
    d.extend(d29(fd.nominal_w));
    d.push(21);
    if has_subrs {
        d.extend(d29(dict_len as i32));
        d.push(19);
    }
    assert_eq!(d.len(), dict_len);
    let subrs = if has_subrs {
        index(&fd.lsubrs)
    } else {
        Vec::new()
    };
    (d, subrs)
}

fn build(spec: &Spec) -> Vec<u8> {
    let header = vec![1, 0, 4, 4];
    let name = index(&[b"Test".to_vec()]);
    let strings = index(&spec.strings);
    let gsubr = index(&spec.gsubrs);
    let n = spec.glyphs.len();
    let charset = spec.charset.clone().unwrap_or_else(|| {
        if spec.charset_pre.is_some() {
            return Vec::new();
        }
        let mut c = vec![0];
        for i in 1..n {
            c.extend((i as u16).to_be_bytes());
        }
        c
    });
    let charstrings = index(&spec.glyphs);
    let top_len = |cid: bool| {
        // Fixed-size dict: operands use the 5-byte form, so the length does not depend on values.
        let mut d = Vec::new();
        if let Some(t) = spec.charstring_type {
            d.extend(d29(t));
            d.extend([12, 6]);
        }
        if let Some(matrix) = &spec.font_matrix {
            d.extend(matrix);
        }
        if cid {
            for _ in 0..3 {
                d.extend(d29(0));
            }
            d.extend([12, 30]);
            d.extend(d29(0));
            d.extend([12, 36]);
            d.extend(d29(0));
            d.extend([12, 37]);
        } else {
            d.extend(d29(0));
            d.extend(d29(0));
            d.push(18);
        }
        d.extend(d29(0));
        d.push(15);
        d.extend(d29(0));
        d.push(17);
        d.len()
    };
    let cid = spec.fd_select.is_some();
    let top_idx_len = index(&[vec![0; top_len(cid)]]).len();
    let mut pos = header.len() + name.len() + top_idx_len + strings.len() + gsubr.len();
    let charset_off = pos;
    pos += charset.len();
    let cs_off = pos;
    pos += charstrings.len();
    let mut tail = Vec::new();
    let mut top = Vec::new();
    if let Some(t) = spec.charstring_type {
        top.extend(d29(t));
        top.extend([12, 6]);
    }
    if let Some(matrix) = &spec.font_matrix {
        top.extend(matrix);
    }
    if cid {
        let sel = spec.fd_select.clone().unwrap();
        let fdselect_off = pos;
        pos += sel.len();
        tail.extend(&sel);
        let fd_dicts: Vec<Vec<u8>> = spec
            .fds
            .iter()
            .map(|fd| {
                let mut d = fd.font_matrix.clone().unwrap_or_default();
                let (pd, _) = private_dict(fd);
                d.extend(d29(pd.len() as i32));
                d.extend(d29(0));
                d.push(18);
                d
            })
            .collect();
        let fdarray_len = index(&fd_dicts).len();
        let fdarray_off = pos;
        pos += fdarray_len;
        let mut dicts = Vec::new();
        let mut privates: Vec<u8> = Vec::new();
        for fd in &spec.fds {
            let (pd, subrs) = private_dict(fd);
            let mut d = fd.font_matrix.clone().unwrap_or_default();
            d.extend(d29(pd.len() as i32));
            d.extend(d29((pos + privates.len()) as i32));
            d.push(18);
            dicts.push(d);
            privates.extend(&pd);
            privates.extend(&subrs);
        }
        tail.extend(index(&dicts));
        tail.extend(privates);
        for _ in 0..3 {
            top.extend(d29(0));
        }
        top.extend([12, 30]);
        top.extend(d29(fdarray_off as i32));
        top.extend([12, 36]);
        top.extend(d29(fdselect_off as i32));
        top.extend([12, 37]);
    } else {
        let fd = spec.fds.first().cloned().unwrap_or_default();
        let (pd, subrs) = private_dict(&fd);
        top.extend(d29(pd.len() as i32));
        top.extend(d29(pos as i32));
        top.push(18);
        tail.extend(pd);
        tail.extend(subrs);
    }
    top.extend(d29(spec.charset_pre.unwrap_or(charset_off as i32)));
    top.push(15);
    top.extend(d29(cs_off as i32));
    top.push(17);
    assert_eq!(top.len(), top_len(cid));
    let mut out = header;
    out.extend(name);
    out.extend(index(&[top]));
    out.extend(strings);
    out.extend(gsubr);
    out.extend(charset);
    out.extend(charstrings);
    out.extend(tail);
    out
}

fn simple(glyphs: Vec<Vec<u8>>) -> Spec {
    Spec {
        glyphs,
        fds: vec![Fd {
            default_w: 250,
            nominal_w: 600,
            ..Fd::default()
        }],
        ..Spec::default()
    }
}

/// Decode glyph 1 of a one-glyph-after-notdef font.
fn outline_of(code: Vec<u8>) -> Result<Path, OutlineError> {
    outline_in(simple(vec![cs(&[O(ENDCHAR)]), code]), 1)
}
fn outline_in(spec: Spec, gid: u16) -> Result<Path, OutlineError> {
    let bytes = build(&spec);
    CffFont::parse(&bytes)?.outline(gid, OutlineLimits::default())
}
fn glyph_in(spec: Spec, gid: u16) -> (Path, f64) {
    let bytes = build(&spec);
    let g = CffFont::parse(&bytes)
        .unwrap()
        .glyph(gid, OutlineLimits::default())
        .unwrap();
    (g.path, g.width)
}

// ---------------------------------------------------------------- drawing operators

#[test]
fn moves_and_lines_close_contours_implicitly() {
    let tri = cs(&[
        N(100),
        N(200),
        O(RMOVETO),
        N(100),
        N(0),
        N(0),
        N(100),
        O(RLINETO),
        O(ENDCHAR),
    ]);
    assert_eq!(
        outline_of(tri).unwrap(),
        Pb::new()
            .m(100., 200.)
            .l(200., 200.)
            .l(200., 300.)
            .z()
            .done()
    );

    let hv = cs(&[
        N(10),
        O(HMOVETO),
        N(50),
        N(30),
        N(20),
        O(HLINETO),
        N(5),
        O(VMOVETO),
        N(7),
        N(8),
        O(VLINETO),
        O(ENDCHAR),
    ]);
    assert_eq!(
        outline_of(hv).unwrap(),
        Pb::new()
            .m(10., 0.)
            .l(60., 0.)
            .l(60., 30.)
            .l(80., 30.)
            .z()
            .m(80., 35.)
            .l(80., 42.)
            .l(88., 42.)
            .z()
            .done()
    );
    // vlineto starts vertical, hlineto horizontal.
    let v = drawn(&[3, 4, 5], O(VLINETO));
    assert_eq!(
        outline_of(v).unwrap(),
        Pb::new().m(0., 0.).l(0., 3.).l(4., 3.).l(4., 8.).z().done()
    );
}

#[test]
fn contour_without_drawing_is_dropped_and_empty_glyph_is_empty() {
    let g = cs(&[N(5), N(5), O(RMOVETO), N(1), N(1), O(RMOVETO), O(ENDCHAR)]);
    assert!(outline_of(g).unwrap().is_empty());
    assert!(outline_of(cs(&[O(ENDCHAR)])).unwrap().is_empty());
}

#[test]
fn curve_operators_match_the_specification() {
    let cases: Vec<(&str, Vec<u8>, Path)> = vec![
        (
            "rrcurveto",
            drawn(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], O(RRCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((1., 2.), (4., 6.), (9., 12.))
                .c((16., 20.), (25., 30.), (36., 42.))
                .z()
                .done(),
        ),
        (
            "hhcurveto dy1",
            drawn(&[5, 10, 20, 30, 40], O(HHCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((10., 5.), (30., 35.), (70., 35.))
                .z()
                .done(),
        ),
        (
            "hhcurveto two",
            drawn(&[10, 20, 30, 40, 50, 60, 70, 80], O(HHCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((10., 0.), (30., 30.), (70., 30.))
                .c((120., 30.), (180., 100.), (260., 100.))
                .z()
                .done(),
        ),
        (
            "vvcurveto dx1",
            drawn(&[5, 10, 20, 30, 40], O(VVCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((5., 10.), (25., 40.), (25., 80.))
                .z()
                .done(),
        ),
        (
            "vvcurveto",
            drawn(&[10, 20, 30, 40], O(VVCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((0., 10.), (20., 40.), (20., 80.))
                .z()
                .done(),
        ),
        (
            "hvcurveto 4",
            drawn(&[10, 20, 30, 40], O(HVCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((10., 0.), (30., 30.), (30., 70.))
                .z()
                .done(),
        ),
        (
            "hvcurveto 5",
            drawn(&[10, 20, 30, 40, 50], O(HVCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((10., 0.), (30., 30.), (80., 70.))
                .z()
                .done(),
        ),
        (
            "hvcurveto 9",
            drawn(&[10, 20, 30, 40, 1, 2, 3, 4, 5], O(HVCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((10., 0.), (30., 30.), (30., 70.))
                .c((30., 71.), (32., 74.), (36., 79.))
                .z()
                .done(),
        ),
        (
            "vhcurveto 4",
            drawn(&[10, 20, 30, 40], O(VHCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((0., 10.), (20., 40.), (60., 40.))
                .z()
                .done(),
        ),
        (
            "vhcurveto 5",
            drawn(&[10, 20, 30, 40, 50], O(VHCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((0., 10.), (20., 40.), (60., 90.))
                .z()
                .done(),
        ),
        (
            "vhcurveto 8",
            drawn(&[10, 20, 30, 40, 1, 2, 3, 4], O(VHCURVETO)),
            Pb::new()
                .m(0., 0.)
                .c((0., 10.), (20., 40.), (60., 40.))
                .c((61., 40.), (63., 43.), (63., 47.))
                .z()
                .done(),
        ),
        (
            "rcurveline",
            drawn(&[1, 2, 3, 4, 5, 6, 7, 8], O(RCURVELINE)),
            Pb::new()
                .m(0., 0.)
                .c((1., 2.), (4., 6.), (9., 12.))
                .l(16., 20.)
                .z()
                .done(),
        ),
        (
            "rlinecurve",
            drawn(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], O(RLINECURVE)),
            Pb::new()
                .m(0., 0.)
                .l(1., 2.)
                .l(4., 6.)
                .c((9., 12.), (16., 20.), (25., 30.))
                .z()
                .done(),
        ),
        (
            "flex",
            drawn_escape(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 50], 35),
            Pb::new()
                .m(0., 0.)
                .c((1., 2.), (4., 6.), (9., 12.))
                .c((16., 20.), (25., 30.), (36., 42.))
                .z()
                .done(),
        ),
        (
            "hflex",
            drawn_escape(&[10, 20, 5, 30, 40, 50, 60], 34),
            Pb::new()
                .m(0., 0.)
                .c((10., 0.), (30., 5.), (60., 5.))
                .c((100., 5.), (150., 0.), (210., 0.))
                .z()
                .done(),
        ),
        (
            "hflex1",
            drawn_escape(&[10, 4, 20, 6, 30, 40, 50, 8, 60], 36),
            Pb::new()
                .m(0., 0.)
                .c((10., 4.), (30., 10.), (60., 10.))
                .c((100., 10.), (150., 18.), (210., 0.))
                .z()
                .done(),
        ),
        (
            "flex1 horizontal",
            drawn_escape(&[10, 2, 10, 2, 10, 2, 10, 2, 10, 2, 20], 37),
            Pb::new()
                .m(0., 0.)
                .c((10., 2.), (20., 4.), (30., 6.))
                .c((40., 8.), (50., 10.), (70., 0.))
                .z()
                .done(),
        ),
        (
            "flex1 vertical",
            drawn_escape(&[2, 10, 2, 10, 2, 10, 2, 10, 2, 10, 20], 37),
            Pb::new()
                .m(0., 0.)
                .c((2., 10.), (4., 20.), (6., 30.))
                .c((8., 40.), (10., 50.), (0., 70.))
                .z()
                .done(),
        ),
    ];
    for (name, code, expected) in cases {
        assert_eq!(outline_of(code).unwrap(), expected, "{name}");
    }
}

#[test]
fn drawing_operators_validate_their_argument_counts() {
    for (name, code) in [
        ("rlineto odd", drawn(&[1, 2, 3], O(RLINETO))),
        ("rlineto none", drawn(&[], O(RLINETO))),
        ("hlineto none", drawn(&[], O(HLINETO))),
        ("rrcurveto 5", drawn(&[1, 2, 3, 4, 5], O(RRCURVETO))),
        ("hhcurveto 3", drawn(&[1, 2, 3], O(HHCURVETO))),
        ("hvcurveto 6", drawn(&[1, 2, 3, 4, 5, 6], O(HVCURVETO))),
        ("rcurveline 6", drawn(&[1, 2, 3, 4, 5, 6], O(RCURVELINE))),
        ("rlinecurve 7", drawn(&[1, 2, 3, 4, 5, 6, 7], O(RLINECURVE))),
        ("flex 12", drawn_escape(&[1; 12], 35)),
        ("hflex 6", drawn_escape(&[1; 6], 34)),
        ("hflex1 8", drawn_escape(&[1; 8], 36)),
        ("flex1 10", drawn_escape(&[1; 10], 37)),
        ("rmoveto 1", cs(&[N(1), O(RMOVETO), O(ENDCHAR)])),
        (
            "rmoveto 4",
            cs(&[N(1), N(2), N(3), N(4), O(RMOVETO), O(ENDCHAR)]),
        ),
        ("hmoveto 0", cs(&[O(HMOVETO), O(ENDCHAR)])),
        (
            "line before moveto",
            cs(&[N(1), N(1), O(RLINETO), O(ENDCHAR)]),
        ),
    ] {
        assert!(
            matches!(outline_of(code), Err(OutlineError::InvalidData(_))),
            "{name}"
        );
    }
}

// ---------------------------------------------------------------- widths and hints

#[test]
fn optional_width_is_read_from_the_first_stack_clearing_operator() {
    let wide = |code: Vec<u8>| glyph_in(simple(vec![cs(&[O(ENDCHAR)]), code]), 1).1;
    assert_eq!(wide(cs(&[N(1), N(1), O(RMOVETO), O(ENDCHAR)])), 250.0);
    assert_eq!(
        wide(cs(&[N(50), N(1), N(1), O(RMOVETO), O(ENDCHAR)])),
        650.0
    );
    assert_eq!(wide(cs(&[N(-70), N(1), O(HMOVETO), O(ENDCHAR)])), 530.0);
    assert_eq!(wide(cs(&[N(7), N(1), O(VMOVETO), O(ENDCHAR)])), 607.0);
    assert_eq!(
        wide(cs(&[N(70), N(10), N(20), O(HSTEM), O(ENDCHAR)])),
        670.0
    );
    assert_eq!(wide(cs(&[N(10), N(20), O(HSTEM), O(ENDCHAR)])), 250.0);
    assert_eq!(wide(cs(&[N(30), O(ENDCHAR)])), 630.0);
    assert_eq!(wide(cs(&[O(ENDCHAR)])), 250.0);
    // Width before an implicit-vstem hintmask.
    assert_eq!(
        wide(cs(&[N(5), N(1), N(2), O(HINTMASK), R(&[0]), O(ENDCHAR)])),
        605.0
    );
    // Fractional width via 16.16.
    assert_eq!(wide(cs(&[R(&[255, 0, 0, 0x80, 0]), O(ENDCHAR)])), 600.5);
    // Only the first operator may carry a width.
    assert!(matches!(
        outline_of(cs(&[
            N(1),
            N(2),
            O(HSTEM),
            N(9),
            N(1),
            N(2),
            O(HSTEM),
            O(ENDCHAR)
        ])),
        Err(OutlineError::InvalidData(_))
    ));
    assert!(matches!(
        outline_of(cs(&[N(1), N(1), O(RMOVETO), N(3), O(ENDCHAR)])),
        Err(OutlineError::InvalidData(_))
    ));
}

#[test]
fn hint_masks_skip_the_right_number_of_bytes() {
    // 0x0e would be endchar and 0xff a 16.16 prefix if the mask were interpreted.
    let tail = [N(0), N(0), O(RMOVETO), N(4), N(4), O(RLINETO), O(ENDCHAR)];
    let expected = Pb::new().m(0., 0.).l(4., 4.).z().done();
    let mk = |head: &[T], mask: &'static [u8]| {
        let mut v = head.to_vec();
        v.push(R(mask));
        v.extend(tail);
        cs(&v)
    };
    // hstemhm (2 stems) + hintmask with 2 implicit vstem args (1 stem) = 3 stems = 1 byte.
    let head = [
        N(10),
        N(20),
        N(30),
        N(40),
        O(HSTEMHM),
        N(5),
        N(6),
        O(HINTMASK),
    ];
    assert_eq!(outline_of(mk(&head, &[0x0e])).unwrap(), expected);
    // 8 stems = 1 byte, 9 stems = 2 bytes.
    let mut eight = ns(&[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]);
    eight.push(O(VSTEMHM));
    eight.push(O(HINTMASK));
    assert_eq!(outline_of(mk(&eight, &[0xff])).unwrap(), expected);
    let mut nine = ns(&[1; 18]);
    nine.push(O(VSTEM));
    nine.push(O(CNTRMASK));
    assert_eq!(outline_of(mk(&nine, &[0x0e, 0x0e])).unwrap(), expected);
    // A second mask reuses the stem count; with a width and no stems before any mask.
    let mut twice = ns(&[1, 1, 1, 1]);
    twice.extend([O(HSTEM), O(HINTMASK), R(&[0x0e]), O(CNTRMASK), R(&[0x0e])]);
    let mut v = twice.clone();
    v.extend(tail);
    assert_eq!(outline_of(cs(&v)).unwrap(), expected);
    // Missing mask bytes: truncated.
    assert!(matches!(
        outline_of(cs(&[N(1), N(1), O(HSTEM), O(HINTMASK)])),
        Err(OutlineError::Truncated)
    ));
    // Hint operators left without a following mask do not draw anything.
    // More than 96 stems is rejected.
    let mut many = Vec::new();
    for _ in 0..5 {
        many.extend(ns(&[1; 48]));
        many.push(O(HSTEM));
    }
    many.push(O(ENDCHAR));
    assert!(matches!(
        outline_of(cs(&many)),
        Err(OutlineError::LimitExceeded("stem hints"))
    ));
    // Odd stem arguments after the width has been consumed.
    assert!(matches!(
        outline_of(cs(&[
            N(1),
            N(2),
            O(HSTEM),
            N(1),
            N(2),
            N(3),
            O(VSTEM),
            O(ENDCHAR)
        ])),
        Err(OutlineError::InvalidData(_))
    ));
}

// ---------------------------------------------------------------- numbers and arithmetic

fn eval_x(parts: &[T]) -> f64 {
    // Leaves exactly one value on the stack, moves there and draws one line.
    let mut v = parts.to_vec();
    v.extend([N(1), O(RMOVETO), N(1), N(1), O(RLINETO), O(ENDCHAR)]);
    let path = outline_of(cs(&v)).unwrap();
    let (x, _) = first_point(&path);
    x
}
fn first_point(path: &Path) -> (f64, f64) {
    let flat = path.flatten(1.0).unwrap();
    (flat[0].points[0].x(), flat[0].points[0].y())
}

#[test]
fn arithmetic_and_stack_operators() {
    assert_eq!(eval_x(&[N(3), N(4), E(10)]), 7.0);
    assert_eq!(eval_x(&[N(10), N(3), E(11)]), 7.0);
    assert_eq!(eval_x(&[N(6), N(7), E(24)]), 42.0);
    assert_eq!(eval_x(&[N(84), N(2), E(12)]), 42.0);
    assert_eq!(eval_x(&[N(5), E(14)]), -5.0);
    assert_eq!(eval_x(&[N(-9), E(9)]), 9.0);
    assert_eq!(eval_x(&[N(49), E(26)]), 7.0);
    assert_eq!(eval_x(&[N(5), E(27), E(10)]), 10.0);
    assert_eq!(eval_x(&[N(1), N(2), E(28), E(11)]), 1.0);
    assert_eq!(eval_x(&[N(1), N(2), E(18)]), 1.0);
    assert_eq!(
        eval_x(&[
            N(10),
            N(20),
            N(30),
            N(2),
            E(29),
            N(0),
            E(20),
            E(18),
            E(18),
            E(18),
            N(0),
            E(21)
        ]),
        10.0
    );
    assert_eq!(eval_x(&[N(1), N(0), E(3)]), 0.0);
    assert_eq!(eval_x(&[N(1), N(0), E(4)]), 1.0);
    assert_eq!(eval_x(&[N(0), E(5)]), 1.0);
    assert_eq!(eval_x(&[N(5), N(5), E(15)]), 1.0);
    assert_eq!(eval_x(&[N(5), N(6), E(15)]), 0.0);
    assert_eq!(eval_x(&[N(10), N(20), N(1), N(2), E(22)]), 10.0);
    assert_eq!(eval_x(&[N(10), N(20), N(3), N(2), E(22)]), 20.0);
    assert_eq!(eval_x(&[N(9), N(0), E(20), N(0), E(21)]), 9.0);
    // roll: 1 2 3 3 1 roll = 3 1 2 ; -1 roll = 2 3 1.
    assert_eq!(
        eval_x(&[N(1), N(2), N(3), N(3), N(1), E(30), E(18), E(18)]),
        3.0
    );
    assert_eq!(
        eval_x(&[N(1), N(2), N(3), N(3), N(-1), E(30), E(18), E(18)]),
        2.0
    );
    // dotsection is a no-op that clears the stack.
    assert_eq!(eval_x(&[N(5), E(0), N(8)]), 8.0);
    // Index with a negative operand copies the top element.
    assert_eq!(eval_x(&[N(4), N(5), N(-3), E(29), E(18), E(18)]), 4.0);
    // 16.16 fixed numbers and 16-bit integers.
    assert_eq!(eval_x(&[R(&[255, 0, 1, 0x80, 0])]), 1.5);
    assert_eq!(eval_x(&[R(&[255, 0xff, 0xfe, 0x80, 0])]), -1.5);
    assert_eq!(eval_x(&[N(30000)]), 30000.0);
    assert_eq!(eval_x(&[N(-1000), N(0), E(10)]), -1000.0);
    assert_eq!(eval_x(&[N(1131)]), 1131.0);
    assert_eq!(eval_x(&[N(-1131)]), -1131.0);
}

#[test]
fn arithmetic_failures_are_typed() {
    assert!(matches!(
        outline_of(cs(&[N(1), N(0), E(12), N(1), O(RMOVETO), O(ENDCHAR)])),
        Err(OutlineError::Geometry)
    ));
    assert!(matches!(
        outline_of(cs(&[N(-4), E(26), N(1), O(RMOVETO), O(ENDCHAR)])),
        Err(OutlineError::Geometry)
    ));
    assert!(matches!(
        outline_of(cs(&[N(1), E(10), O(ENDCHAR)])),
        Err(OutlineError::InvalidData(_))
    ));
    assert!(matches!(
        outline_of(cs(&[N(1), N(40), E(21), O(ENDCHAR)])),
        Err(OutlineError::InvalidData(_))
    ));
    assert!(matches!(
        outline_of(cs(&[N(1), N(5), N(1), E(30), O(ENDCHAR)])),
        Err(OutlineError::InvalidData(_))
    ));
    // `random` is deterministic: the fixed value 0.5, in ]0, 1].
    assert_eq!(eval_x(&[E(23), N(2), E(24)]), 1.0);
    assert!(matches!(
        outline_of(cs(&[E(1), O(ENDCHAR)])),
        Err(OutlineError::InvalidData(_))
    ));
    // Overflow through repeated multiplication: finite check on push.
    let mut v = vec![N(30000)];
    for _ in 0..40 {
        v.extend([E(27), E(24)]);
        v.push(E(27));
        v.push(E(24));
    }
    v.extend([N(1), O(RMOVETO), O(ENDCHAR)]);
    assert!(outline_of(cs(&v)).is_err());
}

// ---------------------------------------------------------------- subroutines

fn ret() -> Vec<u8> {
    cs(&[O(RETURN)])
}
fn line_sub(dx: i32, dy: i32) -> Vec<u8> {
    cs(&[N(dx), N(dy), O(RLINETO), O(RETURN)])
}
fn call(local: bool, subr: i32, bias: i32) -> [T; 2] {
    [N(subr - bias), O(if local { CALLSUBR } else { CALLGSUBR })]
}

#[test]
fn local_and_global_subroutines_use_the_three_bias_ranges() {
    for (count, bias) in [
        (3usize, 107),
        (1239, 107),
        (1240, 1131),
        (33899, 1131),
        (33900, 32768),
    ] {
        for local in [true, false] {
            let mut subrs = vec![ret(); count];
            let last = count - 1;
            subrs[0] = line_sub(5, 0);
            subrs[last] = line_sub(0, 9);
            let mut parts = vec![N(0), N(0), O(RMOVETO)];
            parts.extend(call(local, 0, bias));
            parts.extend(call(local, last as i32, bias));
            parts.push(O(ENDCHAR));
            let mut spec = simple(vec![cs(&[O(ENDCHAR)]), cs(&parts)]);
            if local {
                spec.fds[0].lsubrs = subrs;
            } else {
                spec.gsubrs = subrs;
            }
            assert_eq!(
                outline_in(spec, 1).unwrap(),
                Pb::new().m(0., 0.).l(5., 0.).l(5., 9.).z().done(),
                "count {count} local {local}"
            );
        }
    }
}

#[test]
fn subroutines_nest_up_to_ten_levels_and_endchar_ends_the_glyph() {
    let chain = |levels: usize| {
        let mut subrs = Vec::new();
        for k in 0..levels {
            if k + 1 == levels {
                subrs.push(line_sub(1, 1));
            } else {
                subrs.push(cs(&[N(k as i32 + 1 - 107), O(CALLSUBR), O(RETURN)]));
            }
        }
        let mut spec = simple(vec![
            cs(&[O(ENDCHAR)]),
            cs(&[N(0), N(0), O(RMOVETO), N(-107), O(CALLSUBR), O(ENDCHAR)]),
        ]);
        spec.fds[0].lsubrs = subrs;
        spec
    };
    assert_eq!(
        outline_in(chain(10), 1).unwrap(),
        Pb::new().m(0., 0.).l(1., 1.).z().done()
    );
    assert!(matches!(
        outline_in(chain(11), 1),
        Err(OutlineError::LimitExceeded("subroutine depth"))
    ));
    // A smaller caller depth is honoured, a larger one is capped at 10.
    let bytes = build(&chain(10));
    let font = CffFont::parse(&bytes).unwrap();
    let shallow = OutlineLimits {
        max_depth: 3,
        ..OutlineLimits::default()
    };
    assert!(matches!(
        font.outline(1, shallow),
        Err(OutlineError::LimitExceeded("subroutine depth"))
    ));
    let bytes = build(&chain(11));
    let font = CffFont::parse(&bytes).unwrap();
    let deep = OutlineLimits {
        max_depth: 64,
        ..OutlineLimits::default()
    };
    assert!(matches!(
        font.outline(1, deep),
        Err(OutlineError::LimitExceeded("subroutine depth"))
    ));
    let too_deep = OutlineLimits {
        max_depth: 65,
        ..OutlineLimits::default()
    };
    assert!(matches!(
        font.outline(1, too_deep),
        Err(OutlineError::LimitExceeded("public depth cap"))
    ));

    // endchar inside a subroutine stops everything after the call.
    let mut spec = simple(vec![
        cs(&[O(ENDCHAR)]),
        cs(&[
            N(0),
            N(0),
            O(RMOVETO),
            N(-107),
            O(CALLSUBR),
            N(9),
            N(9),
            O(RLINETO),
            O(ENDCHAR),
        ]),
    ]);
    spec.fds[0].lsubrs = vec![cs(&[N(2), N(2), O(RLINETO), O(ENDCHAR)])];
    assert_eq!(
        outline_in(spec, 1).unwrap(),
        Pb::new().m(0., 0.).l(2., 2.).z().done()
    );
}

#[test]
fn subroutine_errors_and_budgets() {
    // Self-recursion hits the depth bound, not the stack.
    let mut spec = simple(vec![
        cs(&[O(ENDCHAR)]),
        cs(&[N(-107), O(CALLSUBR), O(ENDCHAR)]),
    ]);
    spec.fds[0].lsubrs = vec![cs(&[N(-107), O(CALLSUBR), O(RETURN)])];
    assert!(matches!(
        outline_in(spec.clone(), 1),
        Err(OutlineError::LimitExceeded("subroutine depth"))
    ));
    // Out of range subroutine, missing local subrs, empty stack.
    assert!(matches!(
        outline_in(
            simple(vec![
                cs(&[O(ENDCHAR)]),
                cs(&[N(0), O(CALLSUBR), O(ENDCHAR)])
            ]),
            1
        ),
        Err(OutlineError::InvalidData(_))
    ));
    assert!(matches!(
        outline_in(
            simple(vec![cs(&[O(ENDCHAR)]), cs(&[O(CALLGSUBR), O(ENDCHAR)])]),
            1
        ),
        Err(OutlineError::InvalidData(_))
    ));
    // Reaching the end of a subroutine without an explicit return is malformed.
    for local in [true, false] {
        let mut spec = simple(vec![
            cs(&[O(ENDCHAR)]),
            cs(&[
                N(0),
                N(0),
                O(RMOVETO),
                N(-107),
                O(if local { CALLSUBR } else { CALLGSUBR }),
                O(ENDCHAR),
            ]),
        ]);
        let missing_return = cs(&[N(1), N(0), O(RLINETO)]);
        if local {
            spec.fds[0].lsubrs = vec![missing_return];
        } else {
            spec.gsubrs = vec![missing_return];
        }
        assert!(matches!(
            outline_in(spec, 1),
            Err(OutlineError::InvalidData("subroutine without return"))
        ));
    }
    // Total call budget (max_components) and the operation budget.
    let mut fan = simple(vec![
        cs(&[O(ENDCHAR)]),
        cs(&[N(-107), O(CALLSUBR), O(ENDCHAR)]),
    ]);
    // Subr k calls subr k+1 five times; subr 9 is a no-op leaf. Fan-out 5^9.
    let mut subrs = Vec::new();
    for k in 0..9 {
        let mut b = Vec::new();
        for _ in 0..5 {
            b.extend([N(k + 1 - 107), O(CALLSUBR)]);
        }
        b.push(O(RETURN));
        subrs.push(cs(&b));
    }
    subrs.push(ret());
    fan.fds[0].lsubrs = subrs;
    let bytes = build(&fan);
    let font = CffFont::parse(&bytes).unwrap();
    assert!(matches!(
        font.outline(1, OutlineLimits::default()),
        Err(OutlineError::LimitExceeded("components"))
    ));
    let unlimited = OutlineLimits {
        max_components: usize::MAX,
        ..OutlineLimits::default()
    };
    assert!(matches!(
        font.outline(1, unlimited),
        Err(OutlineError::LimitExceeded("operations"))
    ));
    assert_eq!(MAX_OPERATIONS, 1_000_000);
}

fn pair(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

/// Glyph 1 draws (10,20) -> (40,60); glyph width is the default width 250.
fn matrix_spec(top: Option<Vec<u8>>) -> Spec {
    let mut spec = simple(vec![
        cs(&[O(ENDCHAR)]),
        cs(&[
            N(10),
            N(20),
            O(RMOVETO),
            N(30),
            N(40),
            O(RLINETO),
            O(ENDCHAR),
        ]),
    ]);
    spec.font_matrix = top;
    spec
}

#[test]
fn top_dict_font_matrix_is_applied_in_caller_units() {
    let line = |a: (f64, f64), b: (f64, f64)| Pb::new().m(a.0, a.1).l(b.0, b.1).z().done();
    // Absent, explicit default and reals: unchanged, whatever the caller's em size.
    let default_matrix = matrix(["0.001", "0", "0", "0.001", "0", "0"]);
    for top in [None, Some(default_matrix.clone())] {
        let bytes = build(&matrix_spec(top));
        let font = CffFont::parse(&bytes).unwrap();
        for upem in [16u16, 1000, 2048, 16384] {
            let g = font
                .glyph_with_units(1, OutlineLimits::default(), upem)
                .unwrap();
            assert_eq!(g.path, line(pair(10., 20.), pair(40., 60.)));
            assert_eq!(g.width, 250.0);
        }
    }
    // Uniform 0.002: a 500-unit em drawn at 1000 units doubles everything.
    let bytes = build(&matrix_spec(Some(matrix([
        "0.002", "0", "0", "0.002", "0", "0",
    ]))));
    let font = CffFont::parse(&bytes).unwrap();
    let g = font.glyph(1, OutlineLimits::default()).unwrap();
    assert_eq!(g.path, line(pair(20., 40.), pair(80., 120.)));
    assert_eq!(g.width, 500.0);
    // Same font for a 2000 units-per-em caller: matrix * 2000 = 4.
    let g = font
        .glyph_with_units(1, OutlineLimits::default(), 2000)
        .unwrap();
    assert_eq!(g.path, line(pair(40., 80.), pair(160., 240.)));
    assert_eq!(g.width, 1000.0);
    // Skew and translation: x' = 2x + y + 5, y' = 2y - 1.
    let bytes = build(&matrix_spec(Some(matrix([
        "0.002", "0", "0.001", "0.002", "0.005", "-0.001",
    ]))));
    let g = CffFont::parse(&bytes)
        .unwrap()
        .glyph(1, OutlineLimits::default())
        .unwrap();
    assert_eq!(g.path, line(pair(45., 39.), pair(145., 119.)));
    assert_eq!(g.width, 500.0);
    // Curves go through the same transform: control points of an rrcurveto.
    let mut curve = matrix_spec(Some(matrix(["0.002", "0", "0", "0.002", "0", "0"])));
    curve.glyphs[1] = drawn(&[1, 2, 3, 4, 5, 6], O(RRCURVETO));
    let bytes = build(&curve);
    let path = CffFont::parse(&bytes)
        .unwrap()
        .outline(1, OutlineLimits::default())
        .unwrap();
    assert_eq!(
        path,
        Pb::new()
            .m(0., 0.)
            .c((2., 4.), (8., 12.), (18., 24.))
            .z()
            .done()
    );
}

#[test]
fn rounded_reciprocal_matrices_match_their_em_without_distortion() {
    // Real fonts store 1/1200 as 0.000833333; for a 1200-unit em this is the identity.
    let bytes = build(&matrix_spec(Some(matrix([
        "0.000833333",
        "0",
        "0",
        "0.000833333",
        "0",
        "0",
    ]))));
    let font = CffFont::parse(&bytes).unwrap();
    let exact = Pb::new().m(10., 20.).l(40., 60.).z().done();
    let g = font
        .glyph_with_units(1, OutlineLimits::default(), 1200)
        .unwrap();
    assert_eq!(g.path, exact);
    assert_eq!(g.width, 250.0);
    // For another em the same matrix is a real scale (1000 / 1200).
    let g = font.glyph(1, OutlineLimits::default()).unwrap();
    assert_ne!(g.path, exact);
    let (x, y) = first_point(&g.path);
    assert!((x - 10.0 * 0.833333).abs() < 1e-9 && (y - 20.0 * 0.833333).abs() < 1e-9);
}

#[test]
fn font_matrix_reaches_faces_through_units_per_em() {
    let top = Some(matrix(["0.002", "0", "0", "0.002", "0", "0"]));
    let cff = build(&matrix_spec(top));
    for (upem, a, b) in [(1000u16, 20., 80.), (2000, 40., 160.)] {
        let font = otto_with_upem(2, upem, vec![(*b"CFF ", cff.clone())]);
        let face = FontFace::parse(&font).unwrap();
        assert_eq!(
            face.outline(1).unwrap(),
            Pb::new().m(a, 2. * a).l(b, 1.5 * b).z().done(),
            "upem {upem}"
        );
    }
    // Default matrix: identical contours whatever the head.unitsPerEm.
    let plain = build(&matrix_spec(None));
    let direct = CffFont::parse(&plain)
        .unwrap()
        .outline(1, OutlineLimits::default())
        .unwrap();
    for upem in [1000u16, 2048] {
        let font = otto_with_upem(2, upem, vec![(*b"CFF ", plain.clone())]);
        assert_eq!(FontFace::parse(&font).unwrap().outline(1).unwrap(), direct);
    }
}

#[test]
fn invalid_font_matrices_are_typed_errors() {
    let bad_value = |m: Vec<u8>| {
        let bytes = build(&matrix_spec(Some(m)));
        let r = CffFont::parse(&bytes);
        matches!(r, Err(OutlineError::InvalidData("degenerate FontMatrix")))
    };
    assert!(bad_value(matrix(["0", "0", "0", "0", "0", "0"])));
    assert!(bad_value(matrix(["0.001", "0", "0.002", "0", "0", "0"])));
    assert!(bad_value(matrix([
        "0.001", "0.001", "0.001", "0.001", "0", "0"
    ])));
    // Finite entries whose determinant overflows.
    assert!(bad_value(matrix(["1E300", "0", "0", "1E300", "0", "0"])));
    // Non-finite real in the DICT itself.
    let bytes = build(&matrix_spec(Some(matrix([
        "1E999", "0", "0", "0.001", "0", "0",
    ]))));
    let r = CffFont::parse(&bytes);
    assert!(matches!(
        r,
        Err(OutlineError::InvalidData("DICT real number"))
    ));
    // Wrong operand counts: five and seven numbers.
    for n in [5usize, 7] {
        let mut d: Vec<u8> = (0..n).flat_map(|_| real("0.001")).collect();
        d.extend([12, 7]);
        assert!(matches!(
            CffFont::parse(&build(&matrix_spec(Some(d)))),
            Err(OutlineError::InvalidData("FontMatrix operand count"))
        ));
    }
    // Scaling by a huge em can overflow the coordinates: typed, not a panic.
    let big = build(&matrix_spec(Some(matrix([
        "1E150", "0", "0", "1E150", "0", "0",
    ]))));
    let r = CffFont::parse(&big).map(|f| f.outline(1, OutlineLimits::default()));
    assert!(matches!(r, Ok(Err(OutlineError::Geometry))), "{r:?}");
    // Degenerate FD matrix in a CID font.
    let mut cid = cid_spec(vec![0, 0, 0, 0, 0]);
    cid.fds[1].font_matrix = Some(matrix(["0", "0", "0", "0", "0", "0"]));
    assert!(matches!(
        CffFont::parse(&build(&cid)),
        Err(OutlineError::InvalidData("degenerate FontMatrix"))
    ));
    cid.fds[1].font_matrix = Some([d29(1), vec![12, 7]].concat());
    assert!(matches!(
        CffFont::parse(&build(&cid)),
        Err(OutlineError::InvalidData("FontMatrix operand count"))
    ));
}

#[test]
fn fd_font_matrices_are_composed_with_the_top_dict_matrix() {
    // Glyphs 1..3 select FD 0 (line 1,0 from 0,0), FD 1 (line 0,2), FD 1.
    let sel = vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4];
    let line = |a: (f64, f64), b: (f64, f64)| Pb::new().m(a.0, a.1).l(b.0, b.1).z().done();
    // Top 0.002; FD 0: [0.25 0 0 0.5 500 0]; FD 1 has no matrix. FD 0 effective:
    // [0.0005 0 0 0.001 1 0] -> x' = 0.5x + 1000, y' = y. FD 1: top alone, x2.
    let mut cid = cid_spec(sel.clone());
    cid.font_matrix = Some(matrix(["0.002", "0", "0", "0.002", "0", "0"]));
    cid.fds[0].font_matrix = Some(matrix(["0.25", "0", "0", "0.5", "500", "0"]));
    let bytes = build(&cid);
    let font = CffFont::parse(&bytes).unwrap();
    let g1 = font.glyph(1, OutlineLimits::default()).unwrap();
    assert_eq!(g1.path, line(pair(1000., 0.), pair(1000.5, 0.)));
    assert_eq!(g1.width, 111.0 * 0.5);
    let g2 = font.glyph(2, OutlineLimits::default()).unwrap();
    assert_eq!(g2.path, line(pair(0., 0.), pair(0., 4.)));
    assert_eq!(g2.width, 444.0);
    // FD matrix alone (no Top DICT matrix) is used as is: 0.004 -> x4.
    let mut cid = cid_spec(sel.clone());
    cid.fds[1].font_matrix = Some(matrix(["0.004", "0", "0", "0.004", "0", "0"]));
    let font_bytes = build(&cid);
    let font = CffFont::parse(&font_bytes).unwrap();
    assert_eq!(
        font.outline(2, OutlineLimits::default()).unwrap(),
        line(pair(0., 0.), pair(0., 8.))
    );
    // FD 0 without matrix keeps the default: unchanged.
    assert_eq!(
        font.outline(1, OutlineLimits::default()).unwrap(),
        line(pair(0., 0.), pair(1., 0.))
    );
    // Composition order: FD first, then Top. Top = 90 degree rotation x0.001,
    // FD 0 = [0.5 0 0 1 0 0]: x' = -y, y' = 0.5x.
    let mut cid = cid_spec(sel.clone());
    cid.font_matrix = Some(matrix(["0", "0.001", "-0.001", "0", "0", "0"]));
    cid.fds[0].font_matrix = Some(matrix(["0.5", "0", "0", "1", "0", "0"]));
    cid.fds[1].font_matrix = Some(matrix(["1", "0", "0", "1", "0", "0"]));
    let bytes = build(&cid);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(
        font.outline(1, OutlineLimits::default()).unwrap(),
        line(pair(0., 0.), pair(0., 0.5))
    );
    // FD 1 identity composed with the rotation: (0,2) -> (-2,0).
    assert_eq!(
        font.outline(2, OutlineLimits::default()).unwrap(),
        line(pair(0., 0.), pair(-2., 0.))
    );
    // Top and FD both default-equivalent after composition: no change at all.
    let mut cid = cid_spec(sel);
    cid.font_matrix = Some(matrix(["0.002", "0", "0", "0.002", "0", "0"]));
    cid.fds[0].font_matrix = Some(matrix(["0.5", "0", "0", "0.5", "0", "0"]));
    let bytes = build(&cid);
    assert_eq!(
        CffFont::parse(&bytes)
            .unwrap()
            .outline(1, OutlineLimits::default())
            .unwrap(),
        line(pair(0., 0.), pair(1., 0.))
    );
}

// ---------------------------------------------------------------- stack and budgets

#[test]
fn stack_points_and_contour_budgets() {
    let mut v = ns(&[1; 49]);
    v.push(O(ENDCHAR));
    assert!(matches!(
        outline_of(cs(&v)),
        Err(OutlineError::LimitExceeded("argument stack"))
    ));
    let mut ok = ns(&[1; 48]);
    ok.push(O(HSTEM));
    ok.push(O(ENDCHAR));
    assert!(outline_of(cs(&ok)).is_ok());

    let code = drawn(&[1, 1, 1, 1, 1, 1], O(RLINETO));
    let bytes = build(&simple(vec![cs(&[O(ENDCHAR)]), code]));
    let font = CffFont::parse(&bytes).unwrap();
    // 1 move + 3 lines = 4 points.
    for (points, ok) in [(3usize, false), (4, true)] {
        let r = font.outline(
            1,
            OutlineLimits {
                max_points: points,
                ..OutlineLimits::default()
            },
        );
        assert_eq!(r.is_ok(), ok, "points {points}");
        if !ok {
            assert!(matches!(r, Err(OutlineError::LimitExceeded("points"))));
        }
    }
    let two = cs(&[
        N(0),
        N(0),
        O(RMOVETO),
        N(1),
        N(1),
        O(RLINETO),
        N(5),
        N(5),
        O(RMOVETO),
        N(1),
        N(1),
        O(RLINETO),
        O(ENDCHAR),
    ]);
    let bytes = build(&simple(vec![cs(&[O(ENDCHAR)]), two]));
    let font = CffFont::parse(&bytes).unwrap();
    assert!(
        font.outline(
            1,
            OutlineLimits {
                max_contours: 2,
                ..OutlineLimits::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        font.outline(
            1,
            OutlineLimits {
                max_contours: 1,
                ..OutlineLimits::default()
            }
        ),
        Err(OutlineError::LimitExceeded("contours"))
    ));
    assert!(matches!(
        font.outline(2, OutlineLimits::default()),
        Err(OutlineError::InvalidGlyph)
    ));
}

#[test]
fn structural_charstring_errors_are_typed() {
    for (name, code, kind) in [
        ("no endchar", cs(&[N(1), N(1), O(RMOVETO)]), "invalid"),
        ("reserved 0", vec![0], "invalid"),
        ("reserved 2", vec![2], "invalid"),
        ("reserved 9", vec![9], "invalid"),
        ("reserved 13", vec![13], "invalid"),
        ("reserved 15", vec![15], "invalid"),
        ("return at top", cs(&[O(RETURN)]), "invalid"),
        (
            "seac 4 unencoded code",
            cs(&[N(1), N(2), N(3), N(4), O(ENDCHAR)]),
            "invalid",
        ),
        (
            "seac 5 unencoded code",
            cs(&[N(0), N(1), N(2), N(3), N(4), O(ENDCHAR)]),
            "invalid",
        ),
        (
            "endchar 5 args after the width",
            cs(&[
                N(1),
                N(2),
                O(HSTEM),
                N(0),
                N(1),
                N(2),
                N(65),
                N(194),
                O(ENDCHAR),
            ]),
            "invalid",
        ),
        ("endchar 2 args", cs(&[N(1), N(2), O(ENDCHAR)]), "invalid"),
        ("truncated shortint", vec![28, 0], "truncated"),
        ("truncated fixed", vec![255, 0, 0], "truncated"),
        ("truncated two-byte", vec![247], "truncated"),
        ("truncated escape", vec![12], "truncated"),
    ] {
        let r = outline_of(code);
        let ok = match kind {
            "invalid" => matches!(r, Err(OutlineError::InvalidData(_))),
            "unsupported" => matches!(r, Err(OutlineError::Unsupported(_))),
            _ => matches!(r, Err(OutlineError::Truncated)),
        };
        assert!(ok, "{name}: {r:?}");
    }
}

// ---------------------------------------------------------------- charset, strings, CID

#[test]
fn charsets_and_strings() {
    let glyph = |_: usize| cs(&[O(ENDCHAR)]);
    let mut spec = simple((0..4).map(glyph).collect());
    spec.strings = vec![b"Alpha".to_vec(), b"Beta".to_vec()];
    // Default builder charset: format 0.
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(font.glyph_count(), 4);
    assert!(!font.is_cid());
    assert_eq!(font.charset().unwrap(), vec![0, 1, 2, 3]);
    assert_eq!(font.glyph_id_in_charset(2).unwrap(), 2);
    assert_eq!(font.string(391), Some(&b"Alpha"[..]));
    assert_eq!(font.string(392), Some(&b"Beta"[..]));
    assert_eq!(font.string(393), None);
    assert_eq!(font.string(5), None);
    assert!(matches!(
        font.glyph_id_in_charset(4),
        Err(OutlineError::InvalidGlyph)
    ));
    // Format 1 and 2 ranges.
    spec.charset = Some(vec![1, 0x01, 0x90, 2]);
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(font.charset().unwrap(), vec![0, 400, 401, 402]);
    spec.charset = Some(vec![1, 0x00, 0x0a, 0, 0x00, 0x14, 1]);
    let font_bytes = build(&spec);
    assert_eq!(
        CffFont::parse(&font_bytes).unwrap().charset().unwrap(),
        vec![0, 10, 20, 21]
    );
    spec.charset = Some(vec![2, 0x00, 0x64, 0x00, 0x02]);
    let bytes = build(&spec);
    assert_eq!(
        CffFont::parse(&bytes).unwrap().charset().unwrap(),
        vec![0, 100, 101, 102]
    );
    // Range running past the glyph count, unknown format, SID overflow.
    for bad in [
        vec![2, 0x00, 0x64, 0x00, 0x03],
        vec![3, 0, 0],
        vec![1, 0xff, 0xff, 2],
    ] {
        spec.charset = Some(bad);
        let bytes = build(&spec);
        assert!(CffFont::parse(&bytes).unwrap().charset().is_err());
    }
    // Predefined ISOAdobe charset (offset 0) maps gid to SID.
    let mut iso = build(&simple((0..3).map(glyph).collect()));
    // Patch the charset operator operand in the Top DICT: find 29 .. 15 pattern for the charset.
    let at = iso
        .windows(6)
        .position(|w| w[0] == 29 && w[5] == 15)
        .unwrap();
    iso[at + 1..at + 5].copy_from_slice(&0i32.to_be_bytes());
    let font = CffFont::parse(&iso).unwrap();
    assert_eq!(font.charset().unwrap(), vec![0, 1, 2]);
    iso[at + 1..at + 5].copy_from_slice(&1i32.to_be_bytes());
    assert_eq!(
        CffFont::parse(&iso).unwrap().charset().unwrap(),
        vec![0, 1, 229]
    );
}

fn empty_glyphs(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|_| cs(&[O(ENDCHAR)])).collect()
}

#[test]
fn predefined_charsets_are_resolved_from_their_tables() {
    let with = |pre: i32, n: usize| {
        let mut spec = simple(empty_glyphs(n));
        spec.charset_pre = Some(pre);
        build(&spec)
    };
    // ISOAdobe: identity on SIDs, full length 229.
    let bytes = with(0, 229);
    let font = CffFont::parse(&bytes).unwrap();
    let cs = font.charset().unwrap();
    assert!(cs.iter().enumerate().all(|(g, &sid)| usize::from(sid) == g));
    assert_eq!(font.glyph_name(228).unwrap(), Some(&b"zcaron"[..]));
    assert!(CffFont::parse(&with(0, 230)).unwrap().charset().is_err());
    // Expert (offset 1): 166 glyphs; hand-checked head and tail of appendix C.
    let bytes = with(1, 166);
    let font = CffFont::parse(&bytes).unwrap();
    let cs = font.charset().unwrap();
    assert_eq!(cs.len(), 166);
    assert_eq!(cs[..7], [0, 1, 229, 230, 231, 232, 233]);
    assert_eq!(cs[165], 378);
    assert_eq!(font.glyph_id_in_charset(2).unwrap(), 229);
    assert_eq!(font.glyph_name(2).unwrap(), Some(&b"exclamsmall"[..]));
    assert_eq!(font.glyph_name(165).unwrap(), Some(&b"Ydieresissmall"[..]));
    assert_eq!(font.glyph_for_sid(231).unwrap(), Some(4));
    assert_eq!(font.glyph_for_sid(34).unwrap(), None);
    assert!(CffFont::parse(&with(1, 167)).unwrap().charset().is_err());
    // A shorter Expert font uses the table prefix.
    let bytes = with(1, 4);
    assert_eq!(
        CffFont::parse(&bytes).unwrap().charset().unwrap(),
        vec![0, 1, 229, 230]
    );
    // ExpertSubset (offset 2): 87 glyphs.
    let bytes = with(2, 87);
    let font = CffFont::parse(&bytes).unwrap();
    let cs = font.charset().unwrap();
    assert_eq!(cs.len(), 87);
    assert_eq!(cs[..8], [0, 1, 231, 232, 235, 236, 237, 238]);
    assert_eq!(cs[86], 346);
    assert_eq!(font.glyph_name(86).unwrap(), Some(&b"commainferior"[..]));
    assert_eq!(font.glyph_name(2).unwrap(), Some(&b"dollaroldstyle"[..]));
    assert!(CffFont::parse(&with(2, 88)).unwrap().charset().is_err());
    // Glyph 0 is always .notdef; out-of-range glyphs stay typed.
    assert_eq!(font.glyph_id_in_charset(0).unwrap(), 0);
    assert!(matches!(
        font.glyph_id_in_charset(87),
        Err(OutlineError::InvalidGlyph)
    ));
}

#[test]
fn standard_strings_and_glyph_names() {
    use atelier_font::cff::standard_string;
    assert_eq!(standard_string(0), Some(".notdef"));
    for (sid, name) in [
        (1, "space"),
        (5, "dollar"),
        (34, "A"),
        (96, "exclamdown"),
        (125, "acute"),
        (228, "zcaron"),
        (229, "exclamsmall"),
        (378, "Ydieresissmall"),
        (379, "001.000"),
        (383, "Black"),
        (390, "Semibold"),
    ] {
        assert_eq!(standard_string(sid), Some(name), "SID {sid}");
    }
    assert_eq!(standard_string(391), None);
    assert_eq!(standard_string(u16::MAX), None);
    let all: Vec<&str> = (0..391).map(|s| standard_string(s).unwrap()).collect();
    assert!(all.iter().all(|n| !n.is_empty()));
    let mut sorted = all.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 391);
    // Standard and custom SIDs resolved through a font: glyphs A, custom, custom, dollar.
    let mut spec = simple(empty_glyphs(5));
    spec.strings = vec![b"Alpha".to_vec(), b"Beta".to_vec()];
    spec.charset = Some(vec![0, 0, 34, 1, 0x87, 1, 0x88, 0, 5]);
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(font.sid_name(34), Some(&b"A"[..]));
    assert_eq!(font.sid_name(391), Some(&b"Alpha"[..]));
    assert_eq!(font.sid_name(392), Some(&b"Beta"[..]));
    assert_eq!(font.sid_name(393), None);
    assert_eq!(font.string(34), None);
    assert_eq!(font.glyph_name(0).unwrap(), Some(&b".notdef"[..]));
    assert_eq!(font.glyph_name(1).unwrap(), Some(&b"A"[..]));
    assert_eq!(font.glyph_name(2).unwrap(), Some(&b"Alpha"[..]));
    assert_eq!(font.glyph_name(3).unwrap(), Some(&b"Beta"[..]));
    assert_eq!(font.glyph_name(4).unwrap(), Some(&b"dollar"[..]));
    assert!(matches!(
        font.glyph_name(5),
        Err(OutlineError::InvalidGlyph)
    ));
    assert_eq!(font.glyph_for_sid(392).unwrap(), Some(3));
    // SID beyond the String INDEX: no name, no error.
    let mut spec = simple(empty_glyphs(2));
    spec.charset = Some(vec![0, 0x03, 0x00]);
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(font.glyph_name(1).unwrap(), None);
    // CID-keyed fonts have CIDs, not names.
    let cid = build(&cid_spec(vec![0, 0, 1, 1, 0]));
    assert_eq!(CffFont::parse(&cid).unwrap().glyph_name(1).unwrap(), None);
}

#[test]
fn empty_charstring_is_an_empty_glyph_with_the_default_width() {
    let (path, width) = glyph_in(simple(vec![cs(&[O(ENDCHAR)]), vec![]]), 1);
    assert_eq!(path, Path::new());
    assert_eq!(width, 250.0);
    // A non-empty charstring still needs its endchar.
    assert!(matches!(
        outline_of(cs(&[N(1), N(1), O(RMOVETO)])),
        Err(OutlineError::InvalidData(_))
    ));
}

// ---------------------------------------------------------------- seac

/// Font with A (gid 1, SID 34), acute (gid 2, SID 125) and the glyphs in `extra`
/// (gids 3.., SIDs 35 `B`, 36 `C`, 37 `D`, ...).
fn seac_spec(extra: Vec<Vec<u8>>) -> Spec {
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
    let mut glyphs = vec![cs(&[O(ENDCHAR)]), a, acute];
    let n = extra.len();
    glyphs.extend(extra);
    let mut spec = simple(glyphs);
    let mut charset = vec![0, 0, 34, 0, 125];
    for k in 0..n {
        charset.extend((35 + k as u16).to_be_bytes());
    }
    spec.charset = Some(charset);
    spec
}
fn seac_cs(width: Option<i32>, adx: i32, ady: i32, b: i32, a: i32) -> Vec<u8> {
    let mut v = Vec::new();
    if let Some(w) = width {
        v.push(N(w));
    }
    v.extend([N(adx), N(ady), N(b), N(a), O(ENDCHAR)]);
    cs(&v)
}

#[test]
fn endchar_with_four_arguments_composes_base_and_shifted_accent() {
    // 65 = A -> SID 34 -> gid 1; 194 = acute -> SID 125 -> gid 2.
    let spec = seac_spec(vec![seac_cs(Some(7), 40, -10, 65, 194)]);
    let base = Pb::new().m(0., 0.).l(100., 0.).l(100., 200.).z();
    // Accent origin shifted by (40, -10): rmoveto (10,300) lands on (50,290).
    let expected = base.m(50., 290.).l(70., 330.).z().done();
    let (path, width) = glyph_in(spec.clone(), 3);
    assert_eq!(path, expected);
    // The width is the composite glyph's own: nominal 600 + 7.
    assert_eq!(width, 607.0);
    // Without a width argument: default width.
    let mut no_width = spec.clone();
    no_width.glyphs[3] = seac_cs(None, 40, -10, 65, 194);
    let (path, width) = glyph_in(no_width, 3);
    assert_eq!(path, expected);
    assert_eq!(width, 250.0);
    // A width already consumed by a hint operator is not taken twice.
    let mut hinted = spec.clone();
    hinted.glyphs[3] = cs(&[
        N(7),
        N(1),
        N(2),
        O(HSTEM),
        N(40),
        N(-10),
        N(65),
        N(194),
        O(ENDCHAR),
    ]);
    let (path, width) = glyph_in(hinted, 3);
    assert_eq!(path, expected);
    assert_eq!(width, 607.0);
    // Same glyph through a matrix: the whole composition is transformed once.
    let mut scaled = spec.clone();
    scaled.font_matrix = Some(matrix(["0.002", "0", "0", "0.002", "0", "0"]));
    let (path, width) = glyph_in(scaled, 3);
    assert_eq!(
        path,
        Pb::new()
            .m(0., 0.)
            .l(200., 0.)
            .l(200., 400.)
            .z()
            .m(100., 580.)
            .l(140., 660.)
            .z()
            .done()
    );
    assert_eq!(width, 1214.0);
    // Components do not leak state: the composite still uses its own width and
    // a second decode of a component is unchanged.
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(
        font.glyph(2, OutlineLimits::default()).unwrap().path,
        Pb::new().m(10., 300.).l(30., 340.).z().done()
    );
}

#[test]
fn seac_components_use_their_own_subroutines_and_nominal_width() {
    // The accent calls local subr 0 (a 20,40 line); it also carries a width.
    let mut spec = seac_spec(vec![seac_cs(None, 5, 5, 65, 194)]);
    spec.fds[0].lsubrs = vec![line_sub(20, 40)];
    spec.glyphs[2] = cs(&[
        N(99),
        N(10),
        N(300),
        O(RMOVETO),
        N(-107),
        O(CALLSUBR),
        O(ENDCHAR),
    ]);
    let (path, width) = glyph_in(spec, 3);
    assert_eq!(
        path,
        Pb::new()
            .m(0., 0.)
            .l(100., 0.)
            .l(100., 200.)
            .z()
            .m(15., 305.)
            .l(35., 345.)
            .z()
            .done()
    );
    // The accent's own width (600 + 99) is ignored.
    assert_eq!(width, 250.0);
}

#[test]
fn seac_failures_are_typed() {
    let code = |b, a| seac_cs(None, 0, 0, b, a);
    for (name, bad) in [
        ("code outside StandardEncoding", code(3, 194)),
        ("accent unencoded", code(65, 0)),
        ("code too large", code(65, 256)),
        ("negative code", code(-1, 194)),
        ("component absent from charset", code(67, 194)),
        ("accent absent from charset", code(65, 195)),
    ] {
        let spec = seac_spec(vec![bad]);
        let bytes = build(&spec);
        let r = CffFont::parse(&bytes)
            .unwrap()
            .outline(3, OutlineLimits::default());
        assert!(
            matches!(r, Err(OutlineError::InvalidData(_))),
            "{name}: {r:?}"
        );
    }
    // Fractional code (16.16 operand).
    let frac = cs(&[N(0), N(0), R(&[255, 0, 65, 0x80, 0]), N(194), O(ENDCHAR)]);
    let bytes = build(&seac_spec(vec![frac]));
    let r = CffFont::parse(&bytes)
        .unwrap()
        .outline(3, OutlineLimits::default());
    assert!(matches!(
        r,
        Err(OutlineError::InvalidData("seac character code"))
    ));
    // Not allowed in a CID-keyed font.
    let mut cid = cid_spec(vec![0, 0, 0, 0, 0]);
    cid.glyphs[1] = seac_cs(None, 0, 0, 65, 194);
    let bytes = build(&cid);
    let r = CffFont::parse(&bytes)
        .unwrap()
        .outline(1, OutlineLimits::default());
    assert!(matches!(
        r,
        Err(OutlineError::InvalidData("seac in a CID-keyed font"))
    ));
}

#[test]
fn seac_nesting_and_budgets_are_bounded() {
    // gid 3 = B: A + acute. gid 4 = C: B + acute. gid 5 = D: C + acute.
    // gid 6 = E: E + E (self reference).
    let spec = seac_spec(vec![
        seac_cs(None, 0, 0, 65, 194),
        seac_cs(None, 0, 0, 66, 194),
        seac_cs(None, 0, 0, 67, 194),
        seac_cs(None, 0, 0, 69, 69),
    ]);
    let bytes = build(&spec);
    let font = CffFont::parse(&bytes).unwrap();
    let limits = OutlineLimits::default();
    assert!(font.outline(3, limits).is_ok());
    // One nested level is allowed.
    let nested = font.outline(4, limits).unwrap();
    assert_eq!(
        nested,
        Pb::new()
            .m(0., 0.)
            .l(100., 0.)
            .l(100., 200.)
            .z()
            .m(10., 300.)
            .l(30., 340.)
            .z()
            .m(10., 300.)
            .l(30., 340.)
            .z()
            .done()
    );
    // Two levels exceed the explicit bound; cycles end the same way.
    assert!(matches!(
        font.outline(5, limits),
        Err(OutlineError::LimitExceeded("seac depth"))
    ));
    assert!(matches!(
        font.outline(6, limits),
        Err(OutlineError::LimitExceeded("seac depth"))
    ));
    // Point and contour budgets are shared by the components: A has 3 points,
    // the accent 2.
    let at = |max_points, max_contours| {
        font.outline(
            3,
            OutlineLimits {
                max_points,
                max_contours,
                ..limits
            },
        )
    };
    assert!(at(5, 2).is_ok());
    assert!(matches!(
        at(4, 2),
        Err(OutlineError::LimitExceeded("points"))
    ));
    assert!(matches!(
        at(5, 1),
        Err(OutlineError::LimitExceeded("contours"))
    ));
}

fn cid_spec(fd_select: Vec<u8>) -> Spec {
    let main = |sub_a: i32| {
        cs(&[
            N(0),
            N(0),
            O(RMOVETO),
            N(sub_a - 107),
            O(CALLSUBR),
            O(ENDCHAR),
        ])
    };
    Spec {
        glyphs: vec![cs(&[O(ENDCHAR)]), main(0), main(0), main(0)],
        fds: vec![
            Fd {
                lsubrs: vec![line_sub(1, 0)],
                default_w: 111,
                nominal_w: 0,
                ..Fd::default()
            },
            Fd {
                lsubrs: vec![line_sub(0, 2)],
                default_w: 222,
                nominal_w: 0,
                ..Fd::default()
            },
        ],
        fd_select: Some(fd_select),
        charset: Some(vec![2, 0x00, 0x64, 0x00, 0x02]),
        ..Spec::default()
    }
}

#[test]
fn cid_keyed_fonts_select_private_dicts_by_fdselect() {
    // Format 3: glyphs 0..2 -> FD 0, glyphs 2..4 -> FD 1.
    let f3 = vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4];
    // Format 0: glyph -> FD.
    let f0 = vec![0, 0, 1, 1, 0];
    for (name, sel, expect) in [("f3", f3, [0usize, 0, 1, 1]), ("f0", f0, [0usize, 1, 1, 0])] {
        let bytes = build(&cid_spec(sel));
        let font = CffFont::parse(&bytes).unwrap();
        assert!(font.is_cid(), "{name}");
        assert_eq!(font.font_dict_count(), 2);
        assert_eq!(font.charset().unwrap(), vec![0, 100, 101, 102]);
        assert!(font.registry_ordering_supplement().is_some());
        for (gid, fd) in expect.iter().enumerate() {
            assert_eq!(
                font.font_dict_index(gid as u16).unwrap(),
                *fd,
                "{name} {gid}"
            );
        }
        for gid in 1..4u16 {
            let g = font.glyph(gid, OutlineLimits::default()).unwrap();
            let (w, line) = if expect[usize::from(gid)] == 0 {
                (111.0, (1., 0.))
            } else {
                (222.0, (0., 2.))
            };
            assert_eq!(g.width, w, "{name} {gid}");
            assert_eq!(g.path, Pb::new().m(0., 0.).l(line.0, line.1).z().done());
        }
        assert!(matches!(
            font.font_dict_index(4),
            Err(OutlineError::InvalidGlyph)
        ));
    }
}

#[test]
fn invalid_cid_structures_are_rejected() {
    for (name, sel) in [
        ("fd out of range", vec![0, 0, 1, 2, 0]),
        ("short format 0", vec![0, 0, 1]),
        ("first not zero", vec![3, 0, 1, 0, 1, 0, 0, 4]),
        ("sentinel mismatch", vec![3, 0, 1, 0, 0, 0, 0, 3]),
        ("not increasing", vec![3, 0, 2, 0, 0, 0, 0, 0, 0, 1, 0, 4]),
        ("range fd out of range", vec![3, 0, 1, 0, 0, 5, 0, 4]),
        ("zero ranges", vec![3, 0, 0, 0, 4]),
        ("unknown format", vec![9, 0, 0, 0, 0]),
        ("range past glyphs", vec![3, 0, 2, 0, 0, 0, 0, 9, 1, 0, 4]),
    ] {
        let bytes = build(&cid_spec(sel));
        assert!(CffFont::parse(&bytes).is_err(), "{name}");
    }
    // A CID font cannot use a predefined charset.
    let mut spec = cid_spec(vec![0, 0, 0, 0, 0]);
    spec.charset = Some(vec![0, 0, 1, 0, 2, 0, 3]);
    let mut bytes = build(&spec);
    let at = bytes
        .windows(6)
        .position(|w| w[0] == 29 && w[5] == 15)
        .unwrap();
    bytes[at + 1..at + 5].copy_from_slice(&0i32.to_be_bytes());
    assert!(matches!(
        CffFont::parse(&bytes),
        Err(OutlineError::InvalidData(_))
    ));
}

// ---------------------------------------------------------------- invalid tables

#[test]
fn headers_versions_and_indexes_are_validated() {
    let good = build(&simple(vec![cs(&[O(ENDCHAR)]), cs(&[O(ENDCHAR)])]));
    assert!(CffFont::parse(&good).is_ok());
    let mut v2 = good.clone();
    v2[0] = 2;
    assert!(matches!(
        CffFont::parse(&v2),
        Err(OutlineError::Unsupported(_))
    ));
    let mut v0 = good.clone();
    v0[0] = 0;
    assert!(matches!(
        CffFont::parse(&v0),
        Err(OutlineError::InvalidData(_))
    ));
    let mut hdr = good.clone();
    hdr[2] = 3;
    assert!(CffFont::parse(&hdr).is_err());
    let mut off = good.clone();
    off[3] = 0;
    assert!(CffFont::parse(&off).is_err());
    // Name INDEX with offSize 0 / 5.
    for bad in [0u8, 5] {
        let mut b = good.clone();
        b[4 + 2] = bad;
        assert!(CffFont::parse(&b).is_err(), "offSize {bad}");
    }
    // First offset not 1.
    let mut b = good.clone();
    b[4 + 3] = 2;
    assert!(CffFont::parse(&b).is_err());
    // Type 1 charstrings are not supported.
    let mut t1 = simple(vec![cs(&[O(ENDCHAR)])]);
    t1.charstring_type = Some(1);
    assert!(matches!(
        CffFont::parse(&build(&t1)),
        Err(OutlineError::Unsupported("Type 1 charstrings"))
    ));
    let mut t2 = simple(vec![cs(&[O(ENDCHAR)])]);
    t2.charstring_type = Some(2);
    assert!(CffFont::parse(&build(&t2)).is_ok());
    // No glyphs.
    assert!(CffFont::parse(&build(&simple(vec![]))).is_err());
    // Every prefix of a valid table fails or parses without panicking.
    for n in 0..good.len() {
        let _ = CffFont::parse(&good[..n]);
    }
}

#[test]
fn dict_numbers_include_integers_and_reals() {
    let spec = simple(vec![cs(&[O(ENDCHAR)]), cs(&[O(ENDCHAR)])]);
    let mut bytes = build(&spec);
    // defaultWidthX is encoded as `29 <i32> 20`; overwrite the 5-byte slot with the real
    // number "2.50000" (nibbles 2 a 5 0 0 0 f).
    let at = bytes
        .windows(6)
        .position(|w| w[0] == 29 && w[5] == 20)
        .unwrap();
    bytes[at..at + 5].copy_from_slice(&[30, 0x2a, 0x50, 0x00, 0xff]);
    let font = CffFont::parse(&bytes).unwrap();
    assert_eq!(font.glyph(1, OutlineLimits::default()).unwrap().width, 2.5);
    // Negative exponent and sign nibbles: "-1.5E-1" = -0.15.
    bytes[at..at + 5].copy_from_slice(&[30, 0xe1, 0xa5, 0xc1, 0xff]);
    let font = CffFont::parse(&bytes).unwrap();
    let w = font.glyph(1, OutlineLimits::default()).unwrap().width;
    assert!((w + 0.15).abs() < 1e-12, "{w}");
    // Reserved nibble and garbage.
    bytes[at..at + 5].copy_from_slice(&[30, 0xd1, 0xff, 0xff, 0xff]);
    assert!(CffFont::parse(&bytes).is_err());
    bytes[at..at + 5].copy_from_slice(&[30, 0xee, 0xff, 0xff, 0xff]);
    assert!(CffFont::parse(&bytes).is_err());
    bytes[at..at + 5].copy_from_slice(&[31, 0, 0, 0, 0]);
    assert!(CffFont::parse(&bytes).is_err());
}

// ---------------------------------------------------------------- sfnt integration

fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_be_bytes());
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_be_bytes());
}

/// Wrap raw tables in an `OTTO` sfnt with the mandatory metric tables.
fn otto(glyphs: u16, extra: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    otto_with_upem(glyphs, 1000, extra)
}
fn otto_with_upem(glyphs: u16, upem: u16, extra: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut head = vec![0; 54];
    put32(&mut head, 0, 0x0001_0000);
    put32(&mut head, 12, 0x5f0f_3cf5);
    put16(&mut head, 18, upem);
    let mut hhea = vec![0; 36];
    put32(&mut hhea, 0, 0x0001_0000);
    put16(&mut hhea, 4, 800);
    put16(&mut hhea, 6, (-200i16) as u16);
    put16(&mut hhea, 34, 1);
    let mut maxp = vec![0; 6];
    put32(&mut maxp, 0, 0x0000_5000);
    put16(&mut maxp, 4, glyphs);
    let mut os2 = vec![0; 78];
    put16(&mut os2, 4, 400);
    put16(&mut os2, 6, 5);
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
    let mut hmtx = vec![0x03, 0xe8, 0, 5];
    hmtx.extend(vec![0; 2 * (usize::from(glyphs) - 1)]);
    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
        (*b"name", vec![0, 0, 0, 0, 0, 6]),
        (*b"OS/2", os2),
        (*b"post", post),
        (*b"cmap", cmap),
    ];
    tables.extend(extra);
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
        off += data.len() + 3;
    }
    b
}

fn sample_spec() -> Spec {
    let mut spec = simple(vec![
        cs(&[O(ENDCHAR)]),
        cs(&[
            N(10),
            N(20),
            O(RMOVETO),
            N(30),
            N(40),
            O(RLINETO),
            O(ENDCHAR),
        ]),
        drawn(&[1, 2, 3, 4, 5, 6], O(RRCURVETO)),
    ]);
    spec.gsubrs = vec![line_sub(3, 3)];
    spec
}

#[test]
fn face_outline_dispatches_to_cff_without_the_caller_knowing() {
    let cff = build(&sample_spec());
    let font = otto(3, vec![(*b"CFF ", cff.clone())]);
    let face = FontFace::parse(&font).unwrap();
    assert!(face.table(*b"glyf").is_none());
    let direct = CffFont::parse(&cff).unwrap();
    for gid in 0..3 {
        assert_eq!(
            face.outline(gid).unwrap(),
            direct.outline(gid, OutlineLimits::default()).unwrap()
        );
    }
    assert_eq!(
        face.outline(1).unwrap(),
        Pb::new().m(10., 20.).l(40., 60.).z().done()
    );
    assert!(matches!(face.outline(3), Err(OutlineError::InvalidGlyph)));
    assert!(matches!(
        face.outline_with_limits(
            1,
            OutlineLimits {
                max_points: 1,
                ..OutlineLimits::default()
            }
        ),
        Err(OutlineError::LimitExceeded("points"))
    ));
    assert!(matches!(
        face.outline_with_limits(
            1,
            OutlineLimits {
                max_depth: 65,
                ..OutlineLimits::default()
            }
        ),
        Err(OutlineError::LimitExceeded("public depth cap"))
    ));
    // CID-keyed OpenType/CFF.
    let cid = build(&cid_spec(vec![0, 0, 1, 1, 0]));
    let font = otto(4, vec![(*b"CFF ", cid)]);
    let face = FontFace::parse(&font).unwrap();
    assert_eq!(
        face.outline(2).unwrap(),
        Pb::new().m(0., 0.).l(0., 2.).z().done()
    );
}

#[test]
fn cff2_and_missing_outline_tables_are_reported() {
    let font = otto(3, vec![(*b"CFF2", vec![2, 0, 5, 0, 0, 0])]);
    let face = FontFace::parse(&font).unwrap();
    assert!(matches!(
        face.outline(1),
        Err(OutlineError::Unsupported("CFF2 variable outlines"))
    ));
    let table_in_cff = otto(3, vec![(*b"CFF ", vec![2, 0, 5, 0])]);
    let face = FontFace::parse(&table_in_cff).unwrap();
    assert!(matches!(face.outline(1), Err(OutlineError::Unsupported(_))));
    let none = otto(3, vec![]);
    let face = FontFace::parse(&none).unwrap();
    assert!(matches!(
        face.outline(1),
        Err(OutlineError::MissingTable("loca"))
    ));
    let garbage = otto(3, vec![(*b"CFF ", vec![1, 0, 4])]);
    let face = FontFace::parse(&garbage).unwrap();
    assert!(face.outline(1).is_err());
}

// ---------------------------------------------------------------- mutation campaign

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

fn exercise(bytes: &[u8]) {
    let Ok(font) = CffFont::parse(bytes) else {
        return;
    };
    let _ = font.charset();
    for gid in 0..font.glyph_count().min(64) as u16 {
        let _ = font.glyph(gid, OutlineLimits::default());
        let _ = font.glyph_with_units(gid, OutlineLimits::default(), 2048);
        let _ = font.glyph_name(gid);
        let _ = font.font_dict_index(gid);
        let _ = font.glyph_id_in_charset(gid);
    }
    let _ = font.string(391);
    let _ = font.sid_name(34);
    let _ = font.glyph_for_sid(125);
}

fn rich_simple() -> Spec {
    let mut glyphs = vec![cs(&[O(ENDCHAR)])];
    glyphs.push(cs(&[
        N(5),
        N(1),
        N(2),
        O(HSTEMHM),
        N(3),
        N(4),
        O(HINTMASK),
        R(&[0x80]),
        N(0),
        N(0),
        O(RMOVETO),
        N(-107),
        O(CALLSUBR),
        N(-107),
        O(CALLGSUBR),
        O(ENDCHAR),
    ]));
    glyphs.push(drawn(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 50], E(35)));
    glyphs.push(drawn(&[10, 20, 30, 40, 1, 2, 3, 4, 5], O(HVCURVETO)));
    glyphs.push(drawn(&[10, 4, 20, 6, 30, 40, 50, 8, 60], E(36)));
    glyphs.push(drawn(&[1, 2, 3, 4, 5, 6, 7, 8], O(RCURVELINE)));
    glyphs.push(cs(&[
        N(3),
        N(4),
        E(10),
        N(1),
        O(RMOVETO),
        N(2),
        N(2),
        O(RLINETO),
        O(ENDCHAR),
    ]));
    let mut spec = simple(glyphs);
    spec.gsubrs = vec![line_sub(2, 2)];
    spec.fds[0].lsubrs = vec![line_sub(7, 1)];
    spec.strings = vec![b"x".to_vec()];
    spec
}

#[test]
fn seeded_mutation_and_truncation_campaign_never_panics() {
    let mut seac = seac_spec(vec![
        seac_cs(Some(3), 40, -10, 65, 194),
        seac_cs(None, 0, 0, 66, 194),
        seac_cs(None, 0, 0, 67, 66),
        seac_cs(None, 1, 1, 69, 69),
    ]);
    seac.font_matrix = Some(matrix(["0.002", "0", "0.001", "0.002", "0.005", "0"]));
    let mut cid_matrix = cid_spec(vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4]);
    cid_matrix.font_matrix = Some(matrix(["0", "0.001", "-0.001", "0", "0", "0"]));
    cid_matrix.fds[0].font_matrix = Some(matrix(["0.5", "0", "0", "1", "0", "0"]));
    let mut expert = simple(empty_glyphs(20));
    expert.charset_pre = Some(1);
    let bases = [
        build(&seac),
        build(&cid_matrix),
        build(&expert),
        build(&rich_simple()),
        build(&cid_spec(vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4])),
        build(&cid_spec(vec![0, 0, 1, 1, 0])),
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for base in &bases {
        // Every truncation, and every single-byte value at every position for small bases.
        for n in 0..=base.len() {
            exercise(&base[..n]);
        }
        for pos in 0..base.len() {
            for value in [0u8, 1, 0x7f, 0x80, 0xff, 0x0e, 0x0c, 0x1d] {
                let mut m = base.clone();
                m[pos] = value;
                exercise(&m);
            }
        }
        for _ in 0..15_000 {
            let mut m = base.clone();
            for _ in 0..=rng.below(4) {
                let pos = rng.below(m.len());
                m[pos] = rng.step() as u8;
            }
            if rng.below(8) == 0 {
                m.truncate(rng.below(m.len() + 1));
            }
            exercise(&m);
        }
    }
    // The same campaign through the sfnt wrapper and the public FontFace entry point.
    let wrapped = otto(7, vec![(*b"CFF ", bases[0].clone())]);
    let table_at = wrapped.windows(4).position(|w| w == [1, 0, 4, 4]).unwrap();
    for _ in 0..3_000 {
        let mut m = wrapped.clone();
        let pos = table_at + rng.below(bases[0].len());
        m[pos] = rng.step() as u8;
        if let Ok(face) = FontFace::parse(&m) {
            for gid in 0..7 {
                let _ = face.outline(gid);
            }
        }
    }
    for n in 0..wrapped.len() {
        if let Ok(face) = FontFace::parse(&wrapped[..n]) {
            let _ = face.outline(1);
        }
    }
}

#[test]
fn random_seac_arguments_never_loop_or_panic() {
    // Composite glyphs with arbitrary operands over a font whose accents and
    // composites reference each other; every outcome is a value or a typed error.
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut spec = seac_spec(vec![
        seac_cs(None, 0, 0, 65, 194),
        seac_cs(None, 0, 0, 66, 194),
        seac_cs(None, 0, 0, 67, 66),
        seac_cs(None, 0, 0, 69, 69),
        seac_cs(None, 0, 0, 65, 194),
    ]);
    spec.font_matrix = Some(matrix(["0.002", "0", "0", "0.002", "0", "0"]));
    let codes = [0, 3, 65, 66, 67, 68, 69, 70, 194, 195, 255, 256, -1, 1000];
    for _ in 0..5_000 {
        let pick = |rng: &mut Rng| codes[rng.below(codes.len())];
        let adx = rng.below(2000) as i32 - 1000;
        let ady = rng.below(2000) as i32 - 1000;
        let (b, a) = (pick(&mut rng), pick(&mut rng));
        let width = (rng.below(2) == 0).then(|| rng.below(100) as i32);
        spec.glyphs[7] = seac_cs(width, adx, ady, b, a);
        let bytes = build(&spec);
        let font = CffFont::parse(&bytes).unwrap();
        for gid in 0..8 {
            let _ =
                font.glyph_with_units(gid, OutlineLimits::default(), 1 + rng.below(16384) as u16);
        }
    }
}

#[test]
fn mutated_charstrings_never_loop_or_panic() {
    // Random byte strings as charstrings, with subroutines available to call.
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut spec = simple(vec![cs(&[O(ENDCHAR)]), cs(&[O(ENDCHAR)])]);
    spec.gsubrs = vec![line_sub(1, 1), cs(&[N(-107), O(CALLGSUBR), O(RETURN)])];
    spec.fds[0].lsubrs = vec![line_sub(2, 2), cs(&[N(-106), O(CALLSUBR), O(RETURN)])];
    for _ in 0..20_000 {
        let len = 1 + rng.below(40);
        let code: Vec<u8> = (0..len).map(|_| rng.step() as u8).collect();
        spec.glyphs[1] = code;
        let bytes = build(&spec);
        let font = CffFont::parse(&bytes).unwrap();
        let _ = font.glyph(1, OutlineLimits::default());
    }
}
