// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic `.doc` builder for the tests: writes the FIB, piece table,
//! formatted disk pages, style sheet, font table and summary information on
//! top of the CFB test builder of `atelier-cfb`.
#![allow(dead_code)]

#[path = "../../../cfb/tests/common/mod.rs"]
pub mod cfb;

// ----- sprm helpers --------------------------------------------------------

pub fn sprm_u8(op: u16, v: u8) -> Vec<u8> {
    let mut o = op.to_le_bytes().to_vec();
    o.push(v);
    o
}
pub fn sprm_u16(op: u16, v: u16) -> Vec<u8> {
    let mut o = op.to_le_bytes().to_vec();
    o.extend_from_slice(&v.to_le_bytes());
    o
}
pub fn sprm_i16(op: u16, v: i16) -> Vec<u8> {
    sprm_u16(op, v as u16)
}
pub fn sprm_u32(op: u16, v: u32) -> Vec<u8> {
    let mut o = op.to_le_bytes().to_vec();
    o.extend_from_slice(&v.to_le_bytes());
    o
}
/// Variable-length sprm with a one byte count.
pub fn sprm_var(op: u16, data: &[u8]) -> Vec<u8> {
    let mut o = op.to_le_bytes().to_vec();
    o.push(data.len() as u8);
    o.extend_from_slice(data);
    o
}
pub fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

// ----- specification -------------------------------------------------------

#[derive(Clone)]
pub struct PieceSpec {
    pub text: String,
    pub unicode: bool,
    /// Story: 0 main, 1 footnotes, 2 headers, 3 comments, 4 endnotes, 5 text boxes, 6 header text boxes.
    pub story: u8,
    /// grpprl stored in the CLX as a `Prc` and referenced by a `Prm1`.
    pub prc: Option<Vec<u8>>,
    /// Raw `Prm` (overrides `prc`).
    pub raw_prm: Option<u16>,
}

impl PieceSpec {
    pub fn ansi(text: &str) -> Self {
        Self {
            text: text.into(),
            unicode: false,
            story: 0,
            prc: None,
            raw_prm: None,
        }
    }
    pub fn uni(text: &str) -> Self {
        Self {
            unicode: true,
            ..Self::ansi(text)
        }
    }
    pub fn units(&self) -> usize {
        if self.unicode {
            self.text.encode_utf16().count()
        } else {
            self.text.chars().count()
        }
    }
}

#[derive(Clone)]
pub struct CharRunSpec {
    pub cp_start: usize,
    pub cp_end: usize,
    pub grpprl: Vec<u8>,
}

#[derive(Clone)]
pub struct ParaRunSpec {
    pub cp_start: usize,
    pub cp_end: usize,
    pub istd: u16,
    pub grpprl: Vec<u8>,
}

#[derive(Clone)]
pub struct StdSpec {
    pub sti: u16,
    pub sgc: u8,
    pub base: u16,
    pub next: u16,
    pub name: String,
    pub papx: Vec<u8>,
    pub chpx: Vec<u8>,
}

impl StdSpec {
    pub fn para(sti: u16, name: &str, base: u16, papx: Vec<u8>, chpx: Vec<u8>) -> Self {
        Self {
            sti,
            sgc: 1,
            base,
            next: 0x0FFF,
            name: name.into(),
            papx,
            chpx,
        }
    }
    pub fn chr(sti: u16, name: &str, base: u16, chpx: Vec<u8>) -> Self {
        Self {
            sti,
            sgc: 2,
            base,
            next: 0x0FFF,
            name: name.into(),
            papx: Vec::new(),
            chpx,
        }
    }
}

#[derive(Clone)]
pub struct FontSpec {
    pub name: String,
    pub alt: Option<String>,
    pub charset: u8,
    pub ffid: u8,
}

impl FontSpec {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            alt: None,
            charset: 0,
            ffid: 0x24,
        }
    }
}

#[derive(Clone)]
pub struct DocSpec {
    pub n_fib: u16,
    pub n_fib_new: Option<u16>,
    pub flags: u16,
    /// Number of `(fc, lcb)` pairs written (93 = Word 97).
    pub pairs: usize,
    pub use_table1: bool,
    /// Also write a decoy table stream with the other name.
    pub decoy_table: bool,
    pub pieces: Vec<PieceSpec>,
    /// Order in which pieces are laid out in the file (indices into `pieces`).
    pub file_order: Option<Vec<usize>>,
    pub chpx: Vec<CharRunSpec>,
    pub papx: Vec<ParaRunSpec>,
    pub styles: Option<Vec<StdSpec>>,
    pub fonts: Vec<FontSpec>,
    pub default_ftc: [u16; 3],
    /// Section ends (cp after the section mark).
    pub section_ends: Vec<u32>,
    pub summary: Option<Vec<u8>>,
    pub data: Option<Vec<u8>>,
    /// Use the `[0][cb2]` long form for every PAPX entry.
    pub long_papx: bool,
}

impl DocSpec {
    pub fn new(pieces: Vec<PieceSpec>) -> Self {
        Self {
            n_fib: 0x00C1,
            n_fib_new: None,
            flags: 0,
            pairs: 93,
            use_table1: false,
            decoy_table: false,
            pieces,
            file_order: None,
            chpx: Vec::new(),
            papx: Vec::new(),
            styles: None,
            fonts: Vec::new(),
            default_ftc: [0, 0, 0],
            section_ends: Vec::new(),
            summary: None,
            data: None,
            long_papx: false,
        }
    }
    pub fn ansi(text: &str) -> Self {
        Self::new(vec![PieceSpec::ansi(text)])
    }
}

fn compressed_byte(c: char) -> u8 {
    match c {
        '\u{20AC}' => 0x80,
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85,
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2022}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        c if (c as u32) < 0x100 && !(0x80..0xA0).contains(&(c as u32)) => c as u8,
        c => panic!("not encodable as a compressed piece: {c:?}"),
    }
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn align(v: &mut Vec<u8>, to: usize) {
    while v.len() % to != 0 {
        v.push(0);
    }
}
fn utf16z(s: &str) -> Vec<u8> {
    let mut o: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    o.extend_from_slice(&[0, 0]);
    o
}

struct Entry {
    start: u32,
    end: u32,
    data: Option<Vec<u8>>,
}

fn build_pages(mut entries: Vec<Entry>, papx: bool) -> Vec<(u32, u32, Vec<u8>)> {
    entries.sort_by_key(|e| e.start);
    let mut filled: Vec<Entry> = Vec::new();
    for e in entries {
        if let Some(last) = filled.last() {
            assert!(last.end <= e.start, "overlapping runs in the test spec");
            if last.end < e.start {
                filled.push(Entry {
                    start: last.end,
                    end: e.start,
                    data: None,
                });
            }
        }
        filled.push(e);
    }
    let bx = if papx { 13 } else { 1 };
    let mut pages = Vec::new();
    let mut current: Vec<Entry> = Vec::new();
    let size_of = |e: &Entry| e.data.as_ref().map_or(0, |d| (d.len() + 1) & !1);
    let fits = |cur: &[Entry]| {
        let n = cur.len();
        4 * (n + 1) + n * bx + cur.iter().map(size_of).sum::<usize>() <= 510
    };
    let flush = |cur: &mut Vec<Entry>, pages: &mut Vec<(u32, u32, Vec<u8>)>| {
        if cur.is_empty() {
            return;
        }
        let n = cur.len();
        let mut page = vec![0u8; 512];
        for (i, e) in cur.iter().enumerate() {
            page[4 * i..4 * i + 4].copy_from_slice(&e.start.to_le_bytes());
        }
        let last_end = cur[n - 1].end;
        page[4 * n..4 * n + 4].copy_from_slice(&last_end.to_le_bytes());
        let mut top = 510usize;
        for (i, e) in cur.iter().enumerate() {
            if let Some(d) = &e.data {
                let sz = (d.len() + 1) & !1;
                top -= sz;
                page[top..top + d.len()].copy_from_slice(d);
                page[4 * (n + 1) + i * bx] = (top / 2) as u8;
            }
        }
        page[511] = n as u8;
        pages.push((cur[0].start, last_end, page));
        cur.clear();
    };
    for e in filled {
        current.push(e);
        if !fits(&current) {
            let e = current.pop().unwrap();
            flush(&mut current, &mut pages);
            current.push(e);
            assert!(fits(&current), "single entry too large for a page");
        }
    }
    flush(&mut current, &mut pages);
    pages
}

/// Offset of the first text byte in `WordDocument` (after the FIB).
pub const TEXT_START: usize = 2048;

pub struct Built {
    pub word: Vec<u8>,
    pub table: Vec<u8>,
}

impl DocSpec {
    fn ccp(&self, story: u8) -> u32 {
        self.pieces
            .iter()
            .filter(|p| p.story == story)
            .map(|p| p.units() as u32)
            .sum()
    }

    /// Streams of the document.
    pub fn streams(&self) -> Built {
        let mut word = vec![0u8; TEXT_START];
        // Text, in file order.
        let order: Vec<usize> = self
            .file_order
            .clone()
            .unwrap_or_else(|| (0..self.pieces.len()).collect());
        let mut offsets = vec![0usize; self.pieces.len()];
        for &i in &order {
            let p = &self.pieces[i];
            offsets[i] = word.len();
            if p.unicode {
                for u in p.text.encode_utf16() {
                    put16(&mut word, u);
                }
            } else {
                word.extend(p.text.chars().map(compressed_byte));
            }
        }
        align(&mut word, 512);
        // cp -> fc runs.
        let mut cp = 0usize;
        let mut spans = Vec::new(); // (cp_start, cp_end, offset, width)
        for (i, p) in self.pieces.iter().enumerate() {
            spans.push((
                cp,
                cp + p.units(),
                offsets[i],
                if p.unicode { 2 } else { 1 },
            ));
            cp += p.units();
        }
        let fc_runs = |a: usize, b: usize| -> Vec<(u32, u32)> {
            let mut out = Vec::new();
            for &(s, e, off, w) in &spans {
                let (lo, hi) = (a.max(s), b.min(e));
                if lo < hi {
                    out.push(((off + (lo - s) * w) as u32, (off + (hi - s) * w) as u32));
                }
            }
            out
        };
        let mut chpx_entries = Vec::new();
        for r in &self.chpx {
            for (a, b) in fc_runs(r.cp_start, r.cp_end) {
                let mut d = vec![r.grpprl.len() as u8];
                d.extend_from_slice(&r.grpprl);
                chpx_entries.push(Entry {
                    start: a,
                    end: b,
                    data: Some(d),
                });
            }
        }
        let mut papx_entries = Vec::new();
        for r in &self.papx {
            for (a, b) in fc_runs(r.cp_start, r.cp_end) {
                let mut content = r.istd.to_le_bytes().to_vec();
                content.extend_from_slice(&r.grpprl);
                let d = if self.long_papx {
                    if content.len() % 2 == 1 {
                        content.push(0);
                    }
                    let mut d = vec![0u8, (content.len() / 2) as u8];
                    d.extend_from_slice(&content);
                    d
                } else {
                    if content.len() % 2 == 0 {
                        content.push(0);
                    }
                    let mut d = vec![content.len().div_ceil(2) as u8];
                    d.extend_from_slice(&content);
                    d
                };
                papx_entries.push(Entry {
                    start: a,
                    end: b,
                    data: Some(d),
                });
            }
        }
        let bte = |entries: Vec<Entry>, papx: bool, word: &mut Vec<u8>| -> Vec<u8> {
            let pages = build_pages(entries, papx);
            let mut plc = Vec::new();
            for (first, _, _) in &pages {
                put32(&mut plc, *first);
            }
            match pages.last() {
                Some((_, last, _)) => put32(&mut plc, *last),
                None => return Vec::new(),
            }
            for (_, _, page) in &pages {
                let pn = word.len() / 512;
                word.extend_from_slice(page);
                put32(&mut plc, pn as u32);
            }
            plc
        };
        let chpx_plc = bte(chpx_entries, false, &mut word);
        let papx_plc = bte(papx_entries, true, &mut word);

        // Table stream.
        let mut table = Vec::new();
        let mut pairs = vec![(0u32, 0u32); self.pairs.max(34)];
        let place = |table: &mut Vec<u8>, data: &[u8]| -> (u32, u32) {
            align(table, 4);
            let fc = table.len() as u32;
            table.extend_from_slice(data);
            (fc, data.len() as u32)
        };
        // Clx.
        let mut clx = Vec::new();
        let mut prc_index = Vec::new();
        for p in &self.pieces {
            match &p.prc {
                Some(g) => {
                    prc_index.push(Some(((clx_prc_count(&clx)) << 1 | 1) as u16));
                    clx.push(1);
                    put16(&mut clx, g.len() as u16);
                    clx.extend_from_slice(g);
                }
                None => prc_index.push(None),
            }
        }
        let n = self.pieces.len();
        let mut plcpcd = Vec::new();
        let mut cpv = 0u32;
        for p in &self.pieces {
            put32(&mut plcpcd, cpv);
            cpv += p.units() as u32;
        }
        put32(&mut plcpcd, cpv);
        for (i, p) in self.pieces.iter().enumerate() {
            put16(&mut plcpcd, 0);
            let off = offsets[i] as u32;
            put32(
                &mut plcpcd,
                if p.unicode {
                    off
                } else {
                    (off * 2) | 0x4000_0000
                },
            );
            put16(&mut plcpcd, p.raw_prm.or(prc_index[i]).unwrap_or(0));
        }
        debug_assert_eq!(plcpcd.len(), 4 * (n + 1) + 8 * n);
        clx.push(2);
        put32(&mut clx, plcpcd.len() as u32);
        clx.extend_from_slice(&plcpcd);
        pairs[33] = place(&mut table, &clx);
        if !chpx_plc.is_empty() {
            pairs[12] = place(&mut table, &chpx_plc);
        }
        if !papx_plc.is_empty() {
            pairs[13] = place(&mut table, &papx_plc);
        }
        if let Some(styles) = &self.styles {
            pairs[1] = place(&mut table, &build_stsh(styles, self.default_ftc));
        }
        if !self.fonts.is_empty() {
            pairs[15] = place(&mut table, &build_ffn(&self.fonts));
        }
        if !self.section_ends.is_empty() {
            let mut sed = Vec::new();
            put32(&mut sed, 0);
            for e in &self.section_ends {
                put32(&mut sed, *e);
            }
            for _ in &self.section_ends {
                sed.extend_from_slice(&[0u8; 12]);
            }
            pairs[6] = place(&mut table, &sed);
        }

        // FIB.
        let mut fib = Vec::new();
        put16(&mut fib, 0xA5EC);
        put16(&mut fib, self.n_fib);
        put16(&mut fib, 0);
        put16(&mut fib, 0x0409);
        put16(&mut fib, 0);
        put16(
            &mut fib,
            self.flags | if self.use_table1 { 0x0200 } else { 0 },
        );
        put16(&mut fib, 0x00BF);
        fib.resize(32, 0);
        put16(&mut fib, 14); // csw
        fib.resize(34 + 28, 0);
        put16(&mut fib, 22); // cslw
        let lw_at = fib.len();
        fib.resize(lw_at + 88, 0);
        let ccp = [
            (3, self.ccp(0)),
            (4, self.ccp(1)),
            (5, self.ccp(2)),
            (7, self.ccp(3)),
            (8, self.ccp(4)),
            (9, self.ccp(5)),
            (10, self.ccp(6)),
        ];
        for (i, v) in ccp {
            fib[lw_at + 4 * i..lw_at + 4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        put16(&mut fib, self.pairs as u16);
        for &(fc, lcb) in pairs.iter().take(self.pairs) {
            put32(&mut fib, fc);
            put32(&mut fib, lcb);
        }
        match self.n_fib_new {
            Some(v) => {
                put16(&mut fib, 2);
                put16(&mut fib, v);
                put16(&mut fib, 0);
            }
            None => put16(&mut fib, 0),
        }
        assert!(fib.len() <= TEXT_START);
        word[..fib.len()].copy_from_slice(&fib);
        Built { word, table }
    }

    /// The whole compound file.
    pub fn build(&self) -> Vec<u8> {
        let s = self.streams();
        let (name, other) = if self.use_table1 {
            ("1Table", "0Table")
        } else {
            ("0Table", "1Table")
        };
        let mut children = vec![
            cfb::stream("WordDocument", &s.word),
            cfb::stream(name, &s.table),
        ];
        if self.decoy_table {
            children.push(cfb::stream(other, &[0xEE; 64]));
        }
        if let Some(sum) = &self.summary {
            children.push(cfb::stream("\u{5}SummaryInformation", sum));
        }
        if let Some(d) = &self.data {
            children.push(cfb::stream("Data", d));
        }
        cfb::Builder::new(3, children).build().bytes
    }
}

fn clx_prc_count(clx: &[u8]) -> usize {
    // Every Prc starts with type 1; count them by walking.
    let (mut pos, mut n) = (0usize, 0usize);
    while pos < clx.len() && clx[pos] == 1 {
        let cb = u16::from_le_bytes([clx[pos + 1], clx[pos + 2]]) as usize;
        pos += 3 + cb;
        n += 1;
    }
    n
}

pub fn build_stsh(styles: &[StdSpec], ftc: [u16; 3]) -> Vec<u8> {
    let mut stshi = Vec::new();
    put16(&mut stshi, styles.len() as u16);
    put16(&mut stshi, 10);
    put16(&mut stshi, 1);
    put16(&mut stshi, 0x5B);
    put16(&mut stshi, 15);
    put16(&mut stshi, 0x0A);
    for f in ftc {
        put16(&mut stshi, f);
    }
    let mut out = Vec::new();
    put16(&mut out, stshi.len() as u16);
    out.extend_from_slice(&stshi);
    for s in styles {
        if s.name == "<empty>" {
            put16(&mut out, 0);
            continue;
        }
        let mut std = Vec::new();
        let cupx: u16 = if s.sgc == 1 { 2 } else { 1 };
        put16(&mut std, s.sti);
        put16(&mut std, u16::from(s.sgc) | s.base << 4);
        put16(&mut std, cupx | s.next << 4);
        put16(&mut std, 0);
        put16(&mut std, 0);
        put16(&mut std, s.name.encode_utf16().count() as u16);
        std.extend_from_slice(&utf16z(&s.name));
        align(&mut std, 2);
        let mut upx = Vec::new();
        if s.sgc == 1 {
            let mut p = vec![0u8, 0u8]; // istd of the style (unused by readers)
            p.extend_from_slice(&s.papx);
            upx.push(p);
        }
        upx.push(s.chpx.clone());
        for u in upx {
            put16(&mut std, u.len() as u16);
            std.extend_from_slice(&u);
            if u.len() % 2 == 1 {
                std.push(0);
            }
        }
        put16(&mut out, std.len() as u16);
        out.extend_from_slice(&std);
    }
    out
}

pub fn build_ffn(fonts: &[FontSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    put16(&mut out, fonts.len() as u16);
    put16(&mut out, 0);
    for f in fonts {
        let mut rec = vec![f.ffid];
        put16(&mut rec, 400);
        rec.push(f.charset);
        let name_chars = f.name.encode_utf16().count();
        rec.push(if f.alt.is_some() {
            name_chars as u8 + 1
        } else {
            0
        });
        rec.extend_from_slice(&[0u8; 10]);
        rec.extend_from_slice(&[0u8; 24]);
        rec.extend_from_slice(&utf16z(&f.name));
        if let Some(a) = &f.alt {
            rec.extend_from_slice(&utf16z(a));
        }
        out.push(rec.len() as u8);
        out.extend_from_slice(&rec);
    }
    out
}

// ----- summary information -------------------------------------------------

pub enum Prop {
    Str(u32, String),
    Wide(u32, String),
    Time(u32, u64),
    CodePage(u16),
}

pub fn build_summary(props: &[Prop]) -> Vec<u8> {
    let mut values: Vec<(u32, Vec<u8>)> = Vec::new();
    for p in props {
        let (id, v) = match p {
            Prop::CodePage(c) => {
                let mut v = vec![2, 0, 0, 0];
                put16(&mut v, *c);
                put16(&mut v, 0);
                (1, v)
            }
            Prop::Str(id, s) => {
                let mut v = vec![0x1E, 0, 0, 0];
                let mut b: Vec<u8> = s.chars().map(compressed_byte).collect();
                b.push(0);
                put32(&mut v, b.len() as u32);
                v.extend_from_slice(&b);
                align(&mut v, 4);
                (*id, v)
            }
            Prop::Wide(id, s) => {
                let mut v = vec![0x1F, 0, 0, 0];
                let b = utf16z(s);
                put32(&mut v, (b.len() / 2) as u32);
                v.extend_from_slice(&b);
                align(&mut v, 4);
                (*id, v)
            }
            Prop::Time(id, t) => {
                let mut v = vec![0x40, 0, 0, 0];
                v.extend_from_slice(&t.to_le_bytes());
                (*id, v)
            }
        };
        values.push((id, v));
    }
    let mut section = Vec::new();
    put32(&mut section, 0);
    put32(&mut section, values.len() as u32);
    let mut off = 8 + 8 * values.len();
    for (id, v) in &values {
        put32(&mut section, *id);
        put32(&mut section, off as u32);
        off += v.len();
    }
    for (_, v) in &values {
        section.extend_from_slice(v);
    }
    let len = section.len() as u32;
    section[..4].copy_from_slice(&len.to_le_bytes());
    let mut out = vec![0xFE, 0xFF, 0, 0];
    put32(&mut out, 0x0002_0006);
    out.extend_from_slice(&[0u8; 16]);
    put32(&mut out, 1);
    out.extend_from_slice(&[
        0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3,
        0xD9,
    ]);
    put32(&mut out, 48);
    out.extend_from_slice(&section);
    out
}

// ----- deterministic pseudo-random numbers --------------------------------

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
