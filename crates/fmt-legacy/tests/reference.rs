// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reference `.doc` files of `tests/doc-reference` (produced by LibreOffice
//! through `tools/generate-doc-reference.py`), checked against the expected
//! values recorded next to them. A tiny JSON reader keeps `expected.json` the
//! single source of truth without any dependency.

use atelier_fmt_legacy::doc::{DocumentProperties, ImportedDoc, import_doc};
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, LineRule, ParaProps, Paragraph, Underline, VerticalAlign,
};

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
    fn num(&self) -> i64 {
        match self {
            Json::Num(n) => *n as i64,
            other => panic!("not a number: {other:?}"),
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            other => panic!("not an array: {other:?}"),
        }
    }
    fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(o) => o,
            other => panic!("not an object: {other:?}"),
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn value(&mut self) -> Json {
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
    fn string(&mut self) -> String {
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

fn hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn check_para(key: &str, p: &ParaProps, exp: &Json) {
    match key {
        "alignment" => {
            let want = match exp.str() {
                "left" => Alignment::Left,
                "center" => Alignment::Center,
                "right" => Alignment::Right,
                _ => Alignment::Justify,
            };
            assert_eq!(p.alignment, Some(want));
        }
        "indent_left" => assert_eq!(p.indent_left.map(i64::from), Some(exp.num())),
        "indent_right" => assert_eq!(p.indent_right.map(i64::from), Some(exp.num())),
        "indent_first_line" => assert_eq!(p.indent_first_line.map(i64::from), Some(exp.num())),
        "space_before" => assert_eq!(p.space_before.map(i64::from), Some(exp.num())),
        "space_after" => assert_eq!(p.space_after.map(i64::from), Some(exp.num())),
        "line_spacing" => {
            let ls = p.line_spacing.expect("line spacing");
            let rule = match exp.get("rule").unwrap().str() {
                "auto" => LineRule::Auto,
                "exact" => LineRule::Exact,
                _ => LineRule::AtLeast,
            };
            assert_eq!(ls.rule, rule);
            assert_eq!(i64::from(ls.value), exp.get("value").unwrap().num());
        }
        other => panic!("unknown paragraph key {other}"),
    }
}

fn check_char(key: &str, c: &CharAttrs, exp: &Json) {
    match key {
        "bold" => assert_eq!(c.bold, Some(matches!(exp, Json::Bool(true))), "bold"),
        "italic" => assert_eq!(c.italic, Some(matches!(exp, Json::Bool(true))), "italic"),
        "strike" => assert_eq!(c.strike, Some(matches!(exp, Json::Bool(true))), "strike"),
        "underline" => assert_eq!(
            c.underline,
            Some(match exp.str() {
                "double" => Underline::Double,
                "thick" => Underline::Thick,
                "dotted" => Underline::Dotted,
                "dashed" => Underline::Dashed,
                "wave" => Underline::Wave,
                _ => Underline::Single,
            })
        ),
        "vertical_align" => assert_eq!(
            c.vertical_align,
            Some(if exp.str() == "superscript" {
                VerticalAlign::Superscript
            } else {
                VerticalAlign::Subscript
            })
        ),
        "color" => assert_eq!(c.color.map(hex).as_deref(), Some(exp.str()), "color"),
        "highlight" => assert_eq!(
            c.highlight.map(hex).as_deref(),
            Some(exp.str()),
            "highlight"
        ),
        "font" => assert_eq!(c.font.as_deref(), Some(exp.str()), "font"),
        "size_half_points" => assert_eq!(c.size_half_points.map(i64::from), Some(exp.num())),
        "id" | "char_style" | "text" => {}
        other => panic!("unknown character key {other}"),
    }
}

fn paragraphs(doc: &ImportedDoc) -> Vec<Paragraph> {
    doc.paragraph_ids()
        .iter()
        .map(|id| doc.document.paragraph(*id).unwrap().clone())
        .collect()
}

#[test]
fn libreoffice_reference_files() {
    let expected = Parser {
        s: include_bytes!("../../../tests/doc-reference/expected.json"),
        i: 0,
    }
    .value();
    let files: [(&str, &[u8]); 5] = [
        (
            "text-accents.doc",
            include_bytes!("../../../tests/doc-reference/text-accents.doc"),
        ),
        (
            "char-formatting.doc",
            include_bytes!("../../../tests/doc-reference/char-formatting.doc"),
        ),
        (
            "para-formatting.doc",
            include_bytes!("../../../tests/doc-reference/para-formatting.doc"),
        ),
        (
            "underlines.doc",
            include_bytes!("../../../tests/doc-reference/underlines.doc"),
        ),
        (
            "styles.doc",
            include_bytes!("../../../tests/doc-reference/styles.doc"),
        ),
    ];
    let listed = expected.get("files").unwrap().entries();
    assert_eq!(listed.len(), files.len());
    for (name, bytes) in files {
        let entry = expected.get("files").unwrap().get(name).expect(name);
        assert_eq!(
            bytes.len() as i64,
            entry.get("size").unwrap().num(),
            "{name}"
        );
        let mut doc = import_doc(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let fib = entry.get("fib").unwrap();
        assert_eq!(
            i64::from(doc.info.n_fib),
            fib.get("n_fib").unwrap().num(),
            "{name}"
        );
        assert_eq!(
            doc.info.main_text_chars as i64,
            fib.get("ccp_text").unwrap().num(),
            "{name}"
        );
        assert_eq!(
            doc.info.table_stream,
            fib.get("table").unwrap().str(),
            "{name}"
        );

        // Document properties.
        let props = entry.get("properties").unwrap();
        let p: &DocumentProperties = &doc.properties;
        for (key, value) in props.entries() {
            let got = match key.as_str() {
                "title" => &p.title,
                "subject" => &p.subject,
                "author" => &p.author,
                "keywords" => &p.keywords,
                "comments" => &p.comments,
                "last_author" => &p.last_author,
                "created_unix" | "modified_unix" => {
                    let ft = if key == "created_unix" {
                        p.created
                    } else {
                        p.modified
                    };
                    let secs = ft.and_then(DocumentProperties::filetime_to_unix_seconds);
                    assert_eq!(secs.map(|s| s as i64), Some(value.num()), "{name} {key}");
                    continue;
                }
                other => panic!("unknown property {other}"),
            };
            assert_eq!(got.as_deref(), Some(value.str()), "{name} {key}");
        }
        let listed = props.entries().len();
        let present = [
            &p.title,
            &p.subject,
            &p.author,
            &p.keywords,
            &p.comments,
            &p.last_author,
        ]
        .iter()
        .filter(|v| v.is_some())
        .count()
            + usize::from(p.created.is_some())
            + usize::from(p.modified.is_some());
        assert_eq!(present, listed, "{name}: properties read but not expected");

        // Text and formatting.
        let want = entry.get("paragraphs").unwrap().arr();
        let ids = doc.paragraph_ids().to_vec();
        let got_texts: Vec<_> = paragraphs(&doc).into_iter().map(|p| p.text).collect();
        let want_texts: Vec<_> = want.iter().map(|w| w.get("text").unwrap().str()).collect();
        assert_eq!(got_texts, want_texts, "{name}");
        for (i, w) in want.iter().enumerate() {
            let (resolved, defaults) = doc.resolved_paragraph(ids[i]).unwrap();
            if let Some(style) = w.get("style") {
                assert_eq!(
                    resolved.props.style.as_deref(),
                    None,
                    "resolved drops the style"
                );
                let direct = doc.document.paragraph(ids[i]).unwrap();
                assert_eq!(
                    direct.props.style.as_deref(),
                    Some(style.str()),
                    "{name} #{i}"
                );
            }
            if let Some(eff) = w.get("style_effective") {
                for (k, v) in eff.get("paragraph").unwrap().entries() {
                    check_para(k, &resolved.props, v);
                }
                for (k, v) in eff.get("character").unwrap().entries() {
                    check_char(k, &defaults.character, v);
                }
            }
            if let Some(pp) = w.get("props") {
                for (k, v) in pp.entries() {
                    check_para(k, &resolved.props, v);
                }
            }
            if let Some(runs) = w.get("runs") {
                let mut from = 0usize;
                for run in runs.arr() {
                    let text = run.get("text").unwrap().str();
                    let start = resolved.text[from..].find(text).expect(text) + from;
                    from = start + text.len();
                    let attrs = resolved
                        .runs
                        .iter()
                        .find(|r| r.start <= start && start < r.end)
                        .map(|r| r.attrs.clone())
                        .unwrap_or_default();
                    for (k, v) in run.entries() {
                        check_char(k, &attrs, v);
                    }
                    if let Some(cs) = run.get("char_style") {
                        assert!(
                            doc.run_styles.iter().any(|r| r.paragraph == ids[i]
                                && r.start <= start
                                && start < r.end
                                && r.style_id.as_deref() == Some(cs.str())),
                            "{name} #{i}: character style {}",
                            cs.str()
                        );
                    }
                }
            }
        }
    }
}
