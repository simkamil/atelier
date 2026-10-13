// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small, strict PDF reader for tests. It shares no code with the writer:
//! it checks the classic xref table byte for byte, resolves every reference,
//! decodes Flate streams and reads content streams and `ToUnicode` maps.

use atelier_compress::{Format, InflateState, Status};
use std::collections::BTreeMap;

pub type Dict = BTreeMap<String, Obj>;

#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(String),
    Arr(Vec<Obj>),
    Dict(Dict),
    Stream(Dict, Vec<u8>),
    Ref(u32),
}

impl Obj {
    pub fn dict(&self) -> &Dict {
        match self {
            Obj::Dict(d) | Obj::Stream(d, _) => d,
            o => panic!("not a dictionary: {o:?}"),
        }
    }
    pub fn arr(&self) -> &[Obj] {
        match self {
            Obj::Arr(a) => a,
            o => panic!("not an array: {o:?}"),
        }
    }
    pub fn int(&self) -> i64 {
        match self {
            Obj::Int(i) => *i,
            o => panic!("not an integer: {o:?}"),
        }
    }
    pub fn num(&self) -> f64 {
        match self {
            Obj::Int(i) => *i as f64,
            Obj::Real(r) => *r,
            o => panic!("not a number: {o:?}"),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Obj::Name(n) => n,
            o => panic!("not a name: {o:?}"),
        }
    }
    pub fn str_bytes(&self) -> &[u8] {
        match self {
            Obj::Str(s) => s,
            o => panic!("not a string: {o:?}"),
        }
    }
    pub fn reference(&self) -> u32 {
        match self {
            Obj::Ref(r) => *r,
            o => panic!("not a reference: {o:?}"),
        }
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
fn is_delim(b: u8) -> bool {
    b"()<>[]{}/%".contains(&b)
}

pub struct Lexer<'a> {
    pub d: &'a [u8],
    pub pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn skip_ws(&mut self) {
        while self.pos < self.d.len() {
            if is_ws(self.d[self.pos]) {
                self.pos += 1;
            } else if self.d[self.pos] == b'%' {
                while self.pos < self.d.len() && !matches!(self.d[self.pos], 10 | 13) {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }
    fn keyword(&mut self) -> String {
        let s = self.pos;
        while self.pos < self.d.len() && !is_ws(self.d[self.pos]) && !is_delim(self.d[self.pos]) {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.d[s..self.pos]).into_owned()
    }
    /// Parses a direct object. In content streams (`content`), a bare keyword
    /// is returned as `Err(keyword)`.
    pub fn object(&mut self, content: bool) -> Result<Obj, String> {
        self.skip_ws();
        let c = *self.d.get(self.pos).ok_or("unexpected end")?;
        match c {
            b'/' => {
                self.pos += 1;
                let raw = self.keyword();
                let b = raw.as_bytes();
                let mut out = Vec::new();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b'#' {
                        let h = std::str::from_utf8(b.get(i + 1..i + 3).ok_or("bad #")?)
                            .map_err(|e| e.to_string())?;
                        out.push(u8::from_str_radix(h, 16).map_err(|e| e.to_string())?);
                        i += 3;
                    } else {
                        out.push(b[i]);
                        i += 1;
                    }
                }
                Ok(Obj::Name(String::from_utf8_lossy(&out).into_owned()))
            }
            b'(' => {
                self.pos += 1;
                let mut depth = 1;
                let mut s = Vec::new();
                while depth > 0 {
                    let b = *self.d.get(self.pos).ok_or("unterminated string")?;
                    self.pos += 1;
                    match b {
                        b'\\' => {
                            let e = *self.d.get(self.pos).ok_or("bad escape")?;
                            self.pos += 1;
                            s.push(match e {
                                b'n' => 10,
                                b'r' => 13,
                                b't' => 9,
                                b'b' => 8,
                                b'f' => 12,
                                b'0'..=b'7' => {
                                    let mut v = u32::from(e - b'0');
                                    for _ in 0..2 {
                                        match self.d.get(self.pos) {
                                            Some(&o @ b'0'..=b'7') => {
                                                v = v * 8 + u32::from(o - b'0');
                                                self.pos += 1;
                                            }
                                            _ => break,
                                        }
                                    }
                                    v as u8
                                }
                                o => o,
                            });
                        }
                        b'(' => {
                            depth += 1;
                            s.push(b);
                        }
                        b')' => {
                            depth -= 1;
                            if depth > 0 {
                                s.push(b);
                            }
                        }
                        _ => s.push(b),
                    }
                }
                Ok(Obj::Str(s))
            }
            b'<' if self.d.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                let mut dict = Dict::new();
                loop {
                    self.skip_ws();
                    if self.d.get(self.pos) == Some(&b'>')
                        && self.d.get(self.pos + 1) == Some(&b'>')
                    {
                        self.pos += 2;
                        return Ok(Obj::Dict(dict));
                    }
                    let k = self.object(false)?;
                    let Obj::Name(k) = k else {
                        return Err(format!("dictionary key is not a name: {k:?}"));
                    };
                    let v = self.object(false)?;
                    if dict.insert(k.clone(), v).is_some() {
                        return Err(format!("duplicate key {k}"));
                    }
                }
            }
            b'<' => {
                self.pos += 1;
                let mut hex = Vec::new();
                loop {
                    let b = *self.d.get(self.pos).ok_or("unterminated hex string")?;
                    self.pos += 1;
                    if b == b'>' {
                        break;
                    }
                    if !is_ws(b) {
                        hex.push(b);
                    }
                }
                if hex.len() % 2 == 1 {
                    hex.push(b'0');
                }
                let mut out = Vec::new();
                for c in hex.chunks(2) {
                    let s = std::str::from_utf8(c).map_err(|e| e.to_string())?;
                    out.push(u8::from_str_radix(s, 16).map_err(|e| e.to_string())?);
                }
                Ok(Obj::Str(out))
            }
            b'[' => {
                self.pos += 1;
                let mut v = Vec::new();
                loop {
                    self.skip_ws();
                    if self.d.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        return Ok(Obj::Arr(v));
                    }
                    v.push(self.object(content)?);
                }
            }
            _ => {
                let save = self.pos;
                let kw = self.keyword();
                if kw.is_empty() {
                    return Err(format!("unexpected byte {c:#x} at {save}"));
                }
                match kw.as_str() {
                    "true" => return Ok(Obj::Bool(true)),
                    "false" => return Ok(Obj::Bool(false)),
                    "null" => return Ok(Obj::Null),
                    _ => {}
                }
                let numeric = kw
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+'))
                    && kw.bytes().any(|b| b.is_ascii_digit());
                if !numeric {
                    return if content {
                        Err(kw)
                    } else {
                        Err(format!("unexpected keyword {kw}"))
                    };
                }
                if kw.contains('.') {
                    return kw.parse::<f64>().map(Obj::Real).map_err(|e| e.to_string());
                }
                let v: i64 = kw.parse().map_err(|_| format!("bad integer {kw}"))?;
                if !content && v >= 0 {
                    // `N G R` lookahead.
                    let mut l = Lexer {
                        d: self.d,
                        pos: self.pos,
                    };
                    l.skip_ws();
                    let g = l.keyword();
                    if !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()) {
                        l.skip_ws();
                        if l.d.get(l.pos) == Some(&b'R')
                            && l.d.get(l.pos + 1).is_none_or(|&b| is_ws(b) || is_delim(b))
                        {
                            self.pos = l.pos + 1;
                            return Ok(Obj::Ref(v as u32));
                        }
                    }
                }
                Ok(Obj::Int(v))
            }
        }
    }
}

pub struct Pdf {
    pub data: Vec<u8>,
    /// Offset per object number (0 for the free head).
    pub offsets: Vec<usize>,
    pub trailer: Dict,
    pub xref_pos: usize,
}

pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut st = InflateState::new(Format::Zlib, 1 << 30);
    let mut out = Vec::new();
    let mut input = data;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let pr = st.push(input, &mut buf, true).map_err(|e| e.to_string())?;
        out.extend_from_slice(&buf[..pr.produced]);
        input = &input[pr.consumed..];
        match pr.status {
            Status::Done => {
                return if input.is_empty() {
                    Ok(out)
                } else {
                    Err("trailing bytes after zlib stream".into())
                };
            }
            Status::NeedInput => return Err("truncated zlib stream".into()),
            Status::NeedOutput => {}
        }
    }
}

impl Pdf {
    /// Opens and checks the file structure. Every failure is a message.
    pub fn open(data: Vec<u8>) -> Result<Pdf, String> {
        if !data.starts_with(b"%PDF-1.7\n%") {
            return Err("bad header".into());
        }
        if data.get(10..14).is_none_or(|m| m.iter().any(|&b| b < 128)) {
            return Err("missing binary marker".into());
        }
        if !data.ends_with(b"\n%%EOF\n") {
            return Err("missing %%EOF".into());
        }
        let tail = &data[data.len().saturating_sub(64)..];
        let tail_s = String::from_utf8_lossy(tail).into_owned();
        let at = tail_s.rfind("startxref\n").ok_or("no startxref")?;
        let num: String = tail_s[at + 10..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let xref_pos: usize = num.parse().map_err(|_| "bad startxref")?;
        if !data[xref_pos..].starts_with(b"xref\n") {
            return Err("startxref does not point at xref".into());
        }
        let mut l = Lexer {
            d: &data,
            pos: xref_pos + 5,
        };
        let first = l.keyword();
        if first != "0" {
            return Err("xref must start at 0".into());
        }
        l.skip_ws();
        let count: usize = l.keyword().parse().map_err(|_| "bad xref count")?;
        if data[l.pos] != b'\n' {
            return Err("xref subsection header must end with LF".into());
        }
        l.pos += 1;
        let mut offsets = Vec::new();
        for i in 0..count {
            let e = data.get(l.pos..l.pos + 20).ok_or("truncated xref entry")?;
            let ok = e[..10].iter().all(u8::is_ascii_digit)
                && e[10] == b' '
                && e[11..16].iter().all(u8::is_ascii_digit)
                && e[16] == b' '
                && (e[17] == b'n' || e[17] == b'f')
                && &e[18..20] == b" \n";
            if !ok {
                return Err(format!("malformed xref entry {i}"));
            }
            let off: usize = std::str::from_utf8(&e[..10]).unwrap().parse().unwrap();
            if i == 0 {
                if e[17] != b'f' || &e[11..16] != b"65535" {
                    return Err("object 0 must be free with generation 65535".into());
                }
            } else {
                if e[17] != b'n' || &e[11..16] != b"00000" {
                    return Err(format!("object {i} must be in use, generation 0"));
                }
                let head = format!("{i} 0 obj\n");
                if !data[off..].starts_with(head.as_bytes()) {
                    return Err(format!("xref offset of object {i} is wrong"));
                }
            }
            offsets.push(off);
            l.pos += 20;
        }
        l.skip_ws();
        if l.keyword() != "trailer" {
            return Err("missing trailer".into());
        }
        let Obj::Dict(trailer) = l.object(false)? else {
            return Err("trailer is not a dictionary".into());
        };
        l.skip_ws();
        if l.keyword() != "startxref" {
            return Err("missing startxref after trailer".into());
        }
        let pdf = Pdf {
            data,
            offsets,
            trailer,
            xref_pos,
        };
        if pdf.trailer.get("Size") != Some(&Obj::Int(count as i64)) {
            return Err("trailer /Size does not match the xref".into());
        }
        // Parse every object and check that every reference resolves.
        for n in 1..count {
            let o = pdf.try_object(n as u32)?;
            pdf.check_refs(&o, count)?;
        }
        // Objects must tile the body exactly: header, objects, xref.
        let mut ends: Vec<usize> = pdf.offsets[1..].to_vec();
        ends.sort_unstable();
        if ends.first() != Some(&pdf.data.iter().position(|&b| b == b'\n').map_or(0, |_| 15)) {
            // header is "%PDF-1.7\n%....\n" = 9 + 6 bytes
            return Err(format!(
                "first object not right after the header: {:?}",
                ends.first()
            ));
        }
        Ok(pdf)
    }

    fn check_refs(&self, o: &Obj, count: usize) -> Result<(), String> {
        match o {
            Obj::Ref(r) => {
                if *r == 0 || *r as usize >= count {
                    return Err(format!("dangling reference {r}"));
                }
            }
            Obj::Arr(a) => {
                for x in a {
                    self.check_refs(x, count)?;
                }
            }
            Obj::Dict(d) | Obj::Stream(d, _) => {
                for x in d.values() {
                    self.check_refs(x, count)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn try_object(&self, n: u32) -> Result<Obj, String> {
        let off = *self.offsets.get(n as usize).ok_or("object out of range")?;
        let mut l = Lexer {
            d: &self.data,
            pos: off,
        };
        let num = l.keyword();
        l.skip_ws();
        let generation = l.keyword();
        l.skip_ws();
        let obj = l.keyword();
        if num != n.to_string() || generation != "0" || obj != "obj" {
            return Err(format!("bad object header for {n}"));
        }
        let o = l.object(false)?;
        l.skip_ws();
        let after = l.pos;
        let o = if let Obj::Dict(d) = o {
            if self.data[after..].starts_with(b"stream") {
                if &self.data[after + 6..after + 7] != b"\n" {
                    return Err("stream keyword must be followed by LF".into());
                }
                let start = after + 7;
                let len = match d.get("Length") {
                    Some(Obj::Int(v)) => *v as usize,
                    Some(Obj::Ref(r)) => self.try_object(*r)?.int() as usize,
                    _ => return Err("stream without /Length".into()),
                };
                let end = start + len;
                if !self.data[end..].starts_with(b"\nendstream\nendobj\n") {
                    return Err(format!(
                        "object {n}: /Length {len} does not end at endstream"
                    ));
                }
                l.pos = end + 11;
                Obj::Stream(d, self.data[start..end].to_vec())
            } else {
                Obj::Dict(d)
            }
        } else {
            o
        };
        if !matches!(o, Obj::Stream(..)) {
            l.skip_ws();
        } else {
            l.skip_ws();
        }
        if l.keyword() != "endobj" {
            return Err(format!("object {n} does not end with endobj"));
        }
        Ok(o)
    }

    pub fn object(&self, n: u32) -> Obj {
        self.try_object(n).unwrap()
    }
    pub fn resolve(&self, o: &Obj) -> Obj {
        let mut cur = o.clone();
        while let Obj::Ref(r) = cur {
            cur = self.object(r);
        }
        cur
    }
    pub fn get(&self, d: &Dict, key: &str) -> Obj {
        self.resolve(
            d.get(key)
                .unwrap_or_else(|| panic!("missing /{key} in {d:?}")),
        )
    }
    pub fn opt(&self, d: &Dict, key: &str) -> Option<Obj> {
        d.get(key).map(|o| self.resolve(o))
    }
    /// Stream bytes with `/FlateDecode` undone (other filters are left alone).
    pub fn stream(&self, o: &Obj) -> Vec<u8> {
        let Obj::Stream(d, raw) = o else {
            panic!("not a stream")
        };
        match self.opt(d, "Filter") {
            None => raw.clone(),
            Some(Obj::Name(n)) if n == "FlateDecode" => inflate(raw).expect("valid zlib"),
            Some(Obj::Name(n)) => panic!("unexpected filter {n}"),
            Some(other) => panic!("unexpected filter {other:?}"),
        }
    }
    pub fn root(&self) -> Dict {
        self.resolve(self.trailer.get("Root").unwrap())
            .dict()
            .clone()
    }
    /// Walks the page tree, checking /Parent and /Count, returns the leaves in order.
    pub fn pages(&self) -> Vec<(u32, Dict)> {
        let root_ref = self.root()["Pages"].reference();
        let mut out = Vec::new();
        let n = self.walk(root_ref, None, &mut out);
        let root = self.object(root_ref);
        assert_eq!(root.dict()["Count"].int() as usize, n);
        out
    }
    fn walk(&self, id: u32, parent: Option<u32>, out: &mut Vec<(u32, Dict)>) -> usize {
        let node = self.object(id);
        let d = node.dict();
        assert_eq!(
            d.get("Parent").map(Obj::reference),
            parent,
            "bad /Parent of {id}"
        );
        match d["Type"].name() {
            "Page" => {
                out.push((id, d.clone()));
                1
            }
            "Pages" => {
                let mut total = 0;
                let kids = d["Kids"].arr();
                assert!(!kids.is_empty() && kids.len() <= 32);
                for k in kids {
                    total += self.walk(k.reference(), Some(id), out);
                }
                assert_eq!(d["Count"].int() as usize, total, "bad /Count of {id}");
                total
            }
            t => panic!("unexpected node type {t}"),
        }
    }
}

/// One operator of a content stream with its operands.
pub type Op = (String, Vec<Obj>);

pub fn content_ops(data: &[u8]) -> Vec<Op> {
    let mut l = Lexer { d: data, pos: 0 };
    let mut ops = Vec::new();
    let mut operands = Vec::new();
    loop {
        l.skip_ws();
        if l.pos >= data.len() {
            break;
        }
        match l.object(true) {
            Ok(o) => operands.push(o),
            Err(kw) => {
                assert!(
                    kw.bytes()
                        .all(|b| b.is_ascii_alphabetic() || b == b'*' || b == b'\'' || b == b'"'),
                    "bad operator {kw:?}"
                );
                ops.push((kw, std::mem::take(&mut operands)));
            }
        }
    }
    assert!(operands.is_empty(), "dangling operands");
    ops
}

/// Parses a `ToUnicode` CMap: code -> text.
pub fn parse_tounicode(data: &[u8]) -> BTreeMap<u16, String> {
    let s = String::from_utf8_lossy(data).into_owned();
    assert!(s.contains("begincodespacerange"));
    let mut map = BTreeMap::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("beginbfchar") {
        rest = &rest[i + 11..];
        let end = rest.find("endbfchar").expect("endbfchar");
        let body = &rest[..end];
        let toks: Vec<&str> = body.split_whitespace().collect();
        assert!(toks.len() % 2 == 0);
        for pair in toks.chunks(2) {
            let code =
                u16::from_str_radix(pair[0].trim_matches(|c| c == '<' || c == '>'), 16).unwrap();
            let hex = pair[1].trim_matches(|c| c == '<' || c == '>');
            let units: Vec<u16> = (0..hex.len() / 4)
                .map(|k| u16::from_str_radix(&hex[4 * k..4 * k + 4], 16).unwrap())
                .collect();
            let text = String::from_utf16(&units).expect("valid UTF-16BE in CMap");
            assert!(map.insert(code, text).is_none(), "duplicate code in CMap");
        }
        rest = &rest[end..];
    }
    map
}

/// Text of a page the way a viewer extracts it: one string per `Tm` piece.
pub fn extract_text(pdf: &Pdf, page: &Dict) -> Vec<String> {
    let res = pdf.get(page, "Resources");
    let fonts = pdf.opt(res.dict(), "Font");
    let content = match pdf.get(page, "Contents") {
        Obj::Stream(..) => pdf.get(page, "Contents"),
        other => panic!("unexpected /Contents {other:?}"),
    };
    let ops = content_ops(&pdf.stream(&content));
    let mut maps: BTreeMap<String, BTreeMap<u16, String>> = BTreeMap::new();
    if let Some(f) = &fonts {
        for (name, fo) in f.dict() {
            let font = pdf.resolve(fo);
            let tu = pdf.get(font.dict(), "ToUnicode");
            maps.insert(name.clone(), parse_tounicode(&pdf.stream(&tu)));
        }
    }
    let mut cur_font = String::new();
    let mut pieces: Vec<String> = Vec::new();
    for (op, args) in ops {
        match op.as_str() {
            "Tf" => cur_font = args[0].name().to_owned(),
            "Tm" => pieces.push(String::new()),
            "TJ" | "Tj" => {
                let strings: Vec<&Obj> = match &args[0] {
                    Obj::Arr(a) => a.iter().collect(),
                    o => vec![o],
                };
                let map = &maps[&cur_font];
                let piece = pieces.last_mut().expect("text outside Tm");
                for s in strings {
                    if let Obj::Str(b) = s {
                        assert!(b.len() % 2 == 0);
                        for c in b.chunks(2) {
                            let code = u16::from_be_bytes([c[0], c[1]]);
                            piece.push_str(map.get(&code).map_or("\u{FFFD}", String::as_str));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pieces
}
