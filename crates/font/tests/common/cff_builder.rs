// SPDX-License-Identifier: AGPL-3.0-or-later
//! Synthetic CFF 1 table builder, shared by the subsetting tests.
//!
//! Copied from `tests/cff_outlines.rs` (whose tests are unchanged).
#![allow(dead_code)]

pub fn index_with(items: &[Vec<u8>], off_size: usize) -> Vec<u8> {
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
pub fn index(items: &[Vec<u8>]) -> Vec<u8> {
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

pub fn d29(v: i32) -> Vec<u8> {
    let mut b = vec![29];
    b.extend_from_slice(&v.to_be_bytes());
    b
}

/// DICT real operand (operator 30) from decimal text such as `-0.5` or `1E-3`.
pub fn real(text: &str) -> Vec<u8> {
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
pub fn matrix(v: [&str; 6]) -> Vec<u8> {
    let mut d: Vec<u8> = v.iter().flat_map(|t| real(t)).collect();
    d.extend([12, 7]);
    d
}

#[derive(Clone, Default)]
pub struct Fd {
    pub lsubrs: Vec<Vec<u8>>,
    pub default_w: i32,
    pub nominal_w: i32,
    pub font_matrix: Option<Vec<u8>>,
}

#[derive(Clone, Default)]
pub struct Spec {
    pub glyphs: Vec<Vec<u8>>,
    pub gsubrs: Vec<Vec<u8>>,
    pub fds: Vec<Fd>,
    /// Raw FDSelect bytes; presence makes the font CID-keyed.
    pub fd_select: Option<Vec<u8>>,
    /// Raw charset bytes (None: format 0 with SIDs 1..n).
    pub charset: Option<Vec<u8>>,
    /// Predefined charset number (0 ISOAdobe, 1 Expert, 2 ExpertSubset) instead of a table.
    pub charset_pre: Option<i32>,
    pub strings: Vec<Vec<u8>>,
    pub charstring_type: Option<i32>,
    pub font_matrix: Option<Vec<u8>>,
}

pub fn private_dict(fd: &Fd) -> (Vec<u8>, Vec<u8>) {
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

pub fn build(spec: &Spec) -> Vec<u8> {
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
