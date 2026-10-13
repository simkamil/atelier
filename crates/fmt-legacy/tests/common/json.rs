// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tiny JSON reader of the reference tests (copy of the one of `reference.rs`).
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
    pub fn num(&self) -> i64 {
        match self {
            Json::Num(n) => *n as i64,
            other => panic!("not a number: {other:?}"),
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            other => panic!("not an array: {other:?}"),
        }
    }
    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(o) => o,
            other => panic!("not an object: {other:?}"),
        }
    }
}

pub struct Parser<'a> {
    pub s: &'a [u8],
    pub i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    pub fn value(&mut self) -> Json {
        self.ws();
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b'}' {
                        self.i += 1;
                        return Json::Obj(out);
                    }
                    if self.s[self.i] == b',' {
                        self.i += 1;
                        continue;
                    }
                    let k = self.string();
                    self.ws();
                    assert_eq!(self.s[self.i], b':');
                    self.i += 1;
                    out.push((k, self.value()));
                }
            }
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b']' {
                        self.i += 1;
                        return Json::Arr(out);
                    }
                    if self.s[self.i] == b',' {
                        self.i += 1;
                        continue;
                    }
                    out.push(self.value());
                }
            }
            b'"' => Json::Str(self.string()),
            b't' => {
                self.i += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Json::Bool(false)
            }
            b'n' => {
                self.i += 4;
                Json::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit() || b"+-.eE".contains(&self.s[self.i]))
                {
                    self.i += 1;
                }
                Json::Num(
                    std::str::from_utf8(&self.s[start..self.i])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }
    pub fn string(&mut self) -> String {
        assert_eq!(self.s[self.i], b'"');
        self.i += 1;
        let mut units: Vec<u16> = Vec::new();
        let mut buf = String::new();
        let flush = |units: &mut Vec<u16>, buf: &mut String| {
            buf.extend(char::decode_utf16(units.drain(..)).map(|r| r.unwrap()));
        };
        loop {
            let c = self.s[self.i];
            match c {
                b'"' => {
                    self.i += 1;
                    flush(&mut units, &mut buf);
                    return buf;
                }
                b'\\' => {
                    let e = self.s[self.i + 1];
                    self.i += 2;
                    match e {
                        b'u' => {
                            let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                            units.push(u16::from_str_radix(h, 16).unwrap());
                            self.i += 4;
                        }
                        other => {
                            flush(&mut units, &mut buf);
                            buf.push(match other {
                                b'n' => '\n',
                                b't' => '\t',
                                b'r' => '\r',
                                b'b' => '\u{8}',
                                b'f' => '\u{c}',
                                c => c as char,
                            });
                        }
                    }
                }
                _ => {
                    flush(&mut units, &mut buf);
                    let len = match c {
                        0..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    buf.push_str(std::str::from_utf8(&self.s[self.i..self.i + len]).unwrap());
                    self.i += len;
                }
            }
        }
    }
}

