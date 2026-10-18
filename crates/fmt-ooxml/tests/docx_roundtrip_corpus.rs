// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-306: open then save without modification over every DOCX of the
//! repository (and synthetic documents with unknown content at each level):
//! no element, no attribute lost, element order kept, part by part, in
//! normalized XML form. The report lists every document; any loss fails.
mod support;

use atelier_fmt_ooxml::{DocxWriteOptions, ImportedDocx, Package, import_docx, write_docx};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use support::{C, MC, R, W, doc, first_diff, parse, round_trip, undeclared_ignorable};

const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15: &str = "http://schemas.microsoft.com/office/word/2012/wordml";

// ---- Corpus discovery -------------------------------------------------------

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["docx", "dotx", "docm", "dotm"].contains(&e.to_ascii_lowercase().as_str())
        }) {
            out.push(p);
        }
    }
}

/// Every Word file under the repository `tests/` directories. Encrypted
/// files (CFB containers, no ZIP signature) are reported but cannot be opened
/// without a password.
fn repository_documents() -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../../tests");
    let mut files = Vec::new();
    walk(&root, &mut files);
    walk(&manifest.join("tests"), &mut files);
    let mut docs = Vec::new();
    let mut skipped = Vec::new();
    for f in files {
        let name = f
            .strip_prefix(&root)
            .map_or_else(
                |_| f.display().to_string(),
                |p| format!("tests/{}", p.display()),
            )
            .replace('\\', "/");
        let bytes = std::fs::read(&f).unwrap_or_else(|e| panic!("{name}: {e}"));
        if bytes.starts_with(b"PK") {
            docs.push((name, bytes));
        } else {
            skipped.push(name);
        }
    }
    (docs, skipped)
}

// ---- Normalized inventory ------------------------------------------------------

fn inventory(c: &C, path: &str, out: &mut BTreeMap<String, i64>, sign: i64) {
    match c {
        C::E(name, attrs, kids) => {
            let here = format!("{path}/{name}");
            *out.entry(here.clone()).or_insert(0) += sign;
            for (k, v) in attrs {
                *out.entry(format!("{here}@{k}={v}")).or_insert(0) += sign;
            }
            for k in kids {
                inventory(k, &here, out, sign);
            }
        }
        C::T(t) => *out.entry(format!("{path}#text={t}")).or_insert(0) += sign,
    }
}

/// Elements and attributes (with their path) present in the source and absent
/// from the output (`lost`) or the other way round (`added`).
fn balance(a: &C, b: &C) -> (Vec<String>, Vec<String>) {
    let mut m = BTreeMap::new();
    inventory(a, "", &mut m, 1);
    inventory(b, "", &mut m, -1);
    let pick = |positive: bool| {
        m.iter()
            .filter(|(_, n)| (**n > 0) == positive && **n != 0)
            .map(|(k, _)| k.clone())
            .collect()
    };
    (pick(true), pick(false))
}

struct Report {
    name: String,
    parts: usize,
    items: usize,
    losses: Vec<String>,
}

fn count(c: &C) -> usize {
    match c {
        C::E(_, a, k) => 1 + a.len() + k.iter().map(count).sum::<usize>(),
        C::T(_) => 0,
    }
}

/// Opens, writes without modification (parts reused, then every part
/// regenerated) and compares every part of the package with the source.
fn check(name: &str, bytes: &[u8]) -> Report {
    let mut rep = Report {
        name: name.to_owned(),
        parts: 0,
        items: 0,
        losses: Vec::new(),
    };
    let imp: ImportedDocx = match import_docx(bytes) {
        Ok(i) => i,
        Err(e) => {
            rep.losses.push(format!("import failed: {e:?}"));
            return rep;
        }
    };
    for regenerate in [false, true] {
        let tag = if regenerate { "regenerated" } else { "reused" };
        let options = DocxWriteOptions {
            regenerate_all: regenerate,
            ..DocxWriteOptions::default()
        };
        let written = match write_docx(&imp, &options) {
            Ok(w) => w,
            Err(e) => {
                rep.losses.push(format!("[{tag}] write failed: {e:?}"));
                continue;
            }
        };
        let out = match Package::read(&written.bytes) {
            Ok(p) => p,
            Err(e) => {
                rep.losses.push(format!("[{tag}] output unreadable: {e:?}"));
                continue;
            }
        };
        for (uri, part) in imp.package.parts() {
            let Some(o) = out.part(uri) else {
                rep.losses.push(format!("[{tag}] part {uri} missing"));
                continue;
            };
            let is_xml = uri.as_str().ends_with(".xml") || uri.as_str().ends_with(".rels");
            if is_xml && !uri.as_str().ends_with(".rels") {
                for p in undeclared_ignorable(&o.bytes) {
                    rep.losses.push(format!(
                        "[{tag}] {uri}: mc:Ignorable prefix not declared: {p}"
                    ));
                }
            }
            if part.bytes == o.bytes && !(regenerate && is_xml) {
                continue;
            }
            if !is_xml {
                rep.losses
                    .push(format!("[{tag}] {uri}: binary part differs"));
                continue;
            }
            let (a, b) = match (
                std::panic::catch_unwind(|| parse(&part.bytes)),
                std::panic::catch_unwind(|| parse(&o.bytes)),
            ) {
                (Ok(a), Ok(b)) => (a, b),
                _ => {
                    rep.losses
                        .push(format!("[{tag}] {uri}: not parsable as XML"));
                    continue;
                }
            };
            if regenerate {
                // Every XML part of the package, identical bytes included.
                rep.parts += 1;
                rep.items += count(&a);
            }
            if let Some(d) = first_diff(&a, &b, "") {
                let (lost, added) = balance(&a, &b);
                rep.losses.push(format!(
                    "[{tag}] {uri}: {d}; lost {} {:?}; added {} {:?}",
                    lost.len(),
                    &lost[..lost.len().min(5)],
                    added.len(),
                    &added[..added.len().min(5)]
                ));
            }
        }
        if out.parts().len() != imp.package.parts().len() {
            rep.losses.push(format!("[{tag}] number of parts differs"));
        }
    }
    rep.losses.extend(determinism_problems(bytes));
    rep
}

// ---- Determinism (US-307 criterion 1) ---------------------------------------------

fn regenerate_all() -> DocxWriteOptions {
    DocxWriteOptions {
        regenerate_all: true,
        ..DocxWriteOptions::default()
    }
}

/// The package with its ZIP members in reverse order, its relationships in
/// reverse order and a useless `Default` entry: the same content built another way.
fn shuffled(bytes: &[u8]) -> Vec<u8> {
    use atelier_zip::{Archive, Limits, Method, WriteOptions, Writer};
    let archive = Archive::open(bytes, Limits::default()).expect("zip");
    let mut members: Vec<(String, Vec<u8>)> = archive
        .entries()
        .iter()
        .filter(|e| !e.name().ends_with('/'))
        .map(|e| (e.name().to_owned(), e.read_all().expect("member")))
        .collect();
    members.reverse();
    let mut zip = Writer::new(WriteOptions::default());
    for (name, mut data) in members {
        if name.ends_with(".rels") {
            data = reverse_relationships(&data);
        } else if name == "[Content_Types].xml" {
            let text = String::from_utf8_lossy(&data).into_owned();
            if let Some(at) = text.find("<Default ").or_else(|| text.find("<Override ")) {
                let extra = r#"<Default Extension="zzz" ContentType="application/x-zzz"/>"#;
                data = format!("{}{extra}{}", &text[..at], &text[at..]).into_bytes();
            }
        }
        zip.add(&name, &data, Method::Deflate).expect("add");
    }
    zip.finish().expect("finish")
}

fn reverse_relationships(xml: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(xml) else {
        return xml.to_vec();
    };
    let (Some(start), Some(end)) = (text.find("<Relationship "), text.rfind("</Relationships>"))
    else {
        return xml.to_vec();
    };
    let body = &text[start..end];
    let items: Vec<&str> = body.split_inclusive("/>").collect();
    if items.iter().any(|i| !i.starts_with("<Relationship ")) || items.concat() != body {
        return xml.to_vec();
    }
    let reversed: String = items.iter().rev().copied().collect();
    format!("{}{reversed}{}", &text[..start], &text[end..]).into_bytes()
}

/// Same model, same bytes: twice, independent of how the source was built and of its
/// raw bytes; fixed point after a save/open cycle; nothing copied from the source.
fn determinism_problems(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let imp = match import_docx(bytes) {
        Ok(i) => i,
        Err(e) => return vec![format!("[determinism] import failed: {e:?}")],
    };
    let first = match write_docx(&imp, &regenerate_all()) {
        Ok(w) => w,
        Err(e) => return vec![format!("[determinism] write failed: {e:?}")],
    };
    if !first.reused_parts.is_empty() {
        out.push(format!(
            "[determinism] parts reused: {:?}",
            first.reused_parts
        ));
    }
    let mut seen: Vec<String> = first
        .regenerated_parts
        .iter()
        .chain(&first.opaque_parts)
        .cloned()
        .collect();
    seen.sort();
    let mut want: Vec<String> = imp
        .package
        .parts()
        .keys()
        .map(ToString::to_string)
        .collect();
    want.sort();
    if seen != want {
        out.push(format!(
            "[determinism] parts not partitioned: {seen:?} vs {want:?}"
        ));
    }
    match write_docx(&imp, &regenerate_all()) {
        Ok(second) if second.bytes == first.bytes => {}
        _ => out.push("[determinism] two writes differ".into()),
    }
    // Raw bytes of the parts the model generates are altered after the import.
    let mut tampered = import_docx(bytes).expect("import");
    let generated: Vec<String> = [
        &tampered.preserved.document,
        &tampered.preserved.styles,
        &tampered.preserved.settings,
    ]
    .into_iter()
    .flatten()
    .map(|p| p.uri.clone())
    .collect();
    for uri in &generated {
        let uri = atelier_fmt_ooxml::PartUri::parse(uri).expect("uri");
        assert!(
            tampered
                .package
                .replace_part_bytes(&uri, b"<not-even-xml".to_vec())
        );
    }
    match write_docx(&tampered, &regenerate_all()) {
        Ok(t) if t.bytes == first.bytes => {}
        _ => out.push("[determinism] output depends on the raw bytes of a generated part".into()),
    }
    // The same package built in another order.
    match import_docx(&shuffled(bytes)).map(|i| write_docx(&i, &regenerate_all())) {
        Ok(Ok(s)) if s.bytes == first.bytes => {}
        Ok(Ok(_)) => out.push("[determinism] output depends on the order of the source".into()),
        _ => out.push("[determinism] shuffled source fails".into()),
    }
    // Fixed point: open the output, save, open, save.
    let again = import_docx(&first.bytes).and_then(|i| {
        write_docx(&i, &regenerate_all())
            .map_err(|_| atelier_fmt_ooxml::DocxError::MissingPart("write"))
    });
    match again {
        Ok(c) => match import_docx(&c.bytes).map(|i| write_docx(&i, &regenerate_all())) {
            Ok(Ok(d)) if d.bytes == c.bytes => {}
            _ => out.push("[determinism] no fixed point after a second cycle".into()),
        },
        Err(e) => out.push(format!("[determinism] output does not reopen: {e:?}")),
    }
    out
}

// ---- Synthetic documents -------------------------------------------------------

fn root(inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:mc="{MC}" xmlns:w14="{W14}" xmlns:w15="{W15}" xmlns:x="urn:foreign" mc:Ignorable="w14 w15 x"><w:body>{inner}</w:body></w:document>"#
    )
}

/// An unknown element number `i`, using each kind of unknown content.
fn unknown(i: usize) -> String {
    match i % 5 {
        0 => format!(
            r#"<w14:unk{i} w14:a="1" w15:b="2" x:c="3"><x:deep>t&amp;{i}</x:deep></w14:unk{i}>"#
        ),
        1 => format!(r#"<w15:unk{i} w15:collapsed="1"/>"#),
        2 => format!(r#"<x:unk{i} xmlns:y="urn:local" y:k="v"><y:z>tail</y:z></x:unk{i}>"#),
        3 => format!(r#"<w:futureElement{i} w:val="x" w14:q="1"><w:child/></w:futureElement{i}>"#),
        _ => format!(
            r#"<mc:AlternateContent><mc:Choice Requires="w14"><w14:unk{i}/></mc:Choice><mc:Fallback><w:unkFb{i}/></mc:Fallback></mc:AlternateContent>"#
        ),
    }
}

/// `children` with one unknown element inserted at every position.
fn variants(children: &[&str], seed: usize) -> Vec<String> {
    (0..=children.len())
        .map(|pos| {
            let mut s = String::new();
            for (i, c) in children.iter().enumerate() {
                if i == pos {
                    s.push_str(&unknown(seed + pos));
                }
                s.push_str(c);
            }
            if pos == children.len() {
                s.push_str(&unknown(seed + pos));
            }
            s
        })
        .collect()
}

const ATTR: &str = r#" w14:paraId="00AB12CD" w15:extra="1" x:foreign="f" w:rsidR="00112233""#;

fn synthetic() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |name: String, body: String, styles: Option<String>, settings: Option<String>| {
        let mut d = doc(root(&body));
        d.styles = styles;
        d.settings = settings;
        out.push((name, d.bytes()));
    };
    let ppr = [
        r#"<w:pStyle w:val="Normal"/>"#,
        "<w:keepNext/>",
        r#"<w:jc w:val="center"/>"#,
        "<w:rPr><w:b/></w:rPr>",
    ];
    let rpr = [
        "<w:b/>",
        "<w:i/>",
        r#"<w:sz w:val="24"/>"#,
        r#"<w:lang w:val="fr-FR"/>"#,
    ];
    let run = [
        "<w:rPr><w:b/></w:rPr>",
        "<w:t>ab</w:t>",
        "<w:tab/>",
        r#"<w:t xml:space="preserve"> cd </w:t>"#,
    ];
    let para = [
        r#"<w:pPr><w:jc w:val="left"/></w:pPr>"#,
        "<w:r><w:t>x</w:t></w:r>",
        r#"<w:hyperlink r:id="rId9"><w:r><w:t>l</w:t></w:r></w:hyperlink>"#,
        "<w:r><w:t>y</w:t></w:r>",
    ];
    let tbl = [
        r#"<w:tblPr><w:tblW w:w="100" w:type="dxa"/></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol w:w="100"/></w:tblGrid>"#,
        "<w:tr><w:tc><w:p/></w:tc></w:tr>",
        "<w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>",
    ];
    let tr = [
        "<w:trPr><w:cantSplit/></w:trPr>",
        "<w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc>",
        "<w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc>",
    ];
    let tc = [
        r#"<w:tcPr><w:tcW w:w="100" w:type="dxa"/></w:tcPr>"#,
        "<w:p><w:r><w:t>a</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>b</w:t></w:r></w:p>",
    ];
    let tblpr = [
        r#"<w:tblStyle w:val="T1"/>"#,
        r#"<w:tblW w:w="0" w:type="auto"/>"#,
        r#"<w:jc w:val="left"/>"#,
    ];
    let tcpr = [
        r#"<w:tcW w:w="100" w:type="dxa"/>"#,
        r#"<w:vAlign w:val="top"/>"#,
    ];
    let trpr = ["<w:cantSplit/>", "<w:tblHeader/>"];
    let sect = [
        r#"<w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1" w:right="1" w:bottom="1" w:left="1"/>"#,
        r#"<w:cols w:space="708"/>"#,
    ];
    let body = [
        "<w:p><w:r><w:t>first</w:t></w:r></w:p>",
        r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#,
        "<w:p><w:r><w:t>last</w:t></w:r></w:p>",
    ];
    let last_sect = r#"<w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr>"#;

    for (i, v) in variants(&ppr, 0).into_iter().enumerate() {
        add(
            format!("unknown-in-pPr-{i}"),
            format!(r#"<w:p{ATTR}><w:pPr>{v}</w:pPr><w:r><w:t>t</w:t></w:r></w:p>"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&rpr, 10).into_iter().enumerate() {
        add(
            format!("unknown-in-rPr-{i}"),
            format!(r#"<w:p><w:r{ATTR}><w:rPr{ATTR}>{v}</w:rPr><w:t>t</w:t></w:r></w:p>"#),
            None,
            None,
        );
        add(
            format!("unknown-in-pPr-rPr-{i}"),
            format!(r#"<w:p><w:pPr><w:rPr>{v}</w:rPr></w:pPr><w:r><w:t>t</w:t></w:r></w:p>"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&run, 20).into_iter().enumerate() {
        add(
            format!("unknown-in-r-{i}"),
            format!(r#"<w:p><w:r{ATTR}>{v}</w:r></w:p>"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&para, 30).into_iter().enumerate() {
        add(
            format!("unknown-in-p-{i}"),
            format!(r#"<w:p{ATTR}>{v}</w:p>"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&tbl, 40).into_iter().enumerate() {
        add(
            format!("unknown-in-tbl-{i}"),
            format!(r#"<w:p/><w:tbl{ATTR}>{v}</w:tbl><w:p/>"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&tr, 50).into_iter().enumerate() {
        add(
            format!("unknown-in-tr-{i}"),
            format!(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/><w:gridCol w:w="1"/></w:tblGrid><w:tr{ATTR}>{v}</w:tr></w:tbl><w:p/>"#
            ),
            None,
            None,
        );
    }
    for (i, v) in variants(&tc, 60).into_iter().enumerate() {
        add(
            format!("unknown-in-tc-{i}"),
            format!(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc{ATTR}>{v}</w:tc></w:tr></w:tbl><w:p/>"#
            ),
            None,
            None,
        );
    }
    for (name, kids, wrap) in [
        ("tblPr", &tblpr[..], 0),
        ("tcPr", &tcpr[..], 1),
        ("trPr", &trpr[..], 2),
    ] {
        for (i, v) in variants(kids, 70 + wrap).into_iter().enumerate() {
            let (a, b, c) = match wrap {
                0 => (v.as_str(), "", ""),
                1 => ("", v.as_str(), ""),
                _ => ("", "", v.as_str()),
            };
            add(
                format!("unknown-in-{name}-{i}"),
                format!(
                    r#"<w:tbl><w:tblPr{ATTR}>{a}</w:tblPr><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:trPr>{c}</w:trPr><w:tc><w:tcPr{ATTR}>{b}</w:tcPr><w:p/></w:tc></w:tr></w:tbl><w:p/>"#
                ),
                None,
                None,
            );
        }
    }
    for (i, v) in variants(&sect, 80).into_iter().enumerate() {
        add(
            format!("unknown-in-sectPr-{i}"),
            format!(r#"<w:p><w:r><w:t>s</w:t></w:r></w:p><w:sectPr{ATTR}>{v}</w:sectPr>"#),
            None,
            None,
        );
        add(
            format!("unknown-in-pPr-sectPr-{i}"),
            format!(r#"<w:p><w:pPr><w:sectPr>{v}</w:sectPr></w:pPr></w:p><w:p/>{last_sect}"#),
            None,
            None,
        );
    }
    for (i, v) in variants(&body, 90).into_iter().enumerate() {
        add(
            format!("unknown-in-body-{i}"),
            format!("{v}{last_sect}"),
            None,
            None,
        );
    }
    // Containers and extensions.
    add(
        "extensions-and-containers".into(),
        concat!(
            r#"<w:p w14:paraId="1" w14:textId="2"><w:pPr><w14:foo/><w15:bar w15:x="1"/></w:pPr>"#,
            r#"<w:r><w:rPr><w14:ligatures w14:val="all"/><w14:numSpacing w14:val="tabular"/></w:rPr><w:t>e</w:t></w:r>"#,
            r#"<w:proofErr w:type="spellStart"/><w:bookmarkStart w:id="1" w:name="b"/><w:bookmarkEnd w:id="1"/>"#,
            r#"<w:commentRangeStart w:id="0"/><w:r><w:t>c</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="begin"/><w:instrText> PAGE </w:instrText><w:fldChar w:fldCharType="end"/></w:r>"#,
            r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r>"#,
            r#"<w:ins w:id="5" w:author="a"><w:r><w:t>i</w:t></w:r></w:ins><w:del w:id="6" w:author="a"><w:r><w:delText>d</w:delText></w:r></w:del>"#,
            r#"<w:sdt><w:sdtPr><w15:appearance w15:val="hidden"/></w:sdtPr><w:sdtContent><w:r><w:t>s</w:t></w:r></w:sdtContent></w:sdt></w:p>"#,
            r#"<mc:AlternateContent><mc:Choice Requires="w14"><w:p><w:r><w:t>C</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:r><w:t>F</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>"#,
            r#"<w:sdt><w:sdtPr/><w:sdtContent><w:p><w:r><w:t>b</w:t></w:r></w:p></w:sdtContent></w:sdt>"#,
            r#"<w:permStart w:id="9"/><w:p/><w:permEnd w:id="9"/>"#,
            r#"<w:sectPr w14:x="1"><w:pgSz w:w="1" w:h="1"/><w15:y/></w:sectPr>"#
        )
        .into(),
        None,
        None,
    );
    // Styles and settings.
    let styles = format!(
        r#"<w:styles xmlns:w="{W}" xmlns:w14="{W14}" xmlns:w15="{W15}" xmlns:mc="{MC}" xmlns:x="urn:foreign" mc:Ignorable="w14 w15"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w14:foo/><w:lang w:val="fr-FR" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><x:unk/></w:docDefaults><w:latentStyles w:count="1"><w:lsdException w:name="Normal" w15:z="1"/></w:latentStyles><w:style w:type="paragraph" w:styleId="Normal" w:default="1" w14:foo="1" x:bar="2"><w:name w:val="Normal"/><w15:unk/><w:aliases w:val="a"/><w:uiPriority w:val="0"/><w:qFormat/><w:pPr><w:keepNext/><x:u/><w:jc w:val="left"/></w:pPr><w:rPr><w:noProof/><w:b/><w14:glow/></w:rPr><w:rsid w:val="00112233"/></w:style><w:style w:type="table" w:styleId="T1"><w:name w:val="T"/><w:tblPr><w:tblBorders/></w:tblPr><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr></w:style></w:styles>"#
    );
    let settings = format!(
        r#"<w:settings xmlns:w="{W}" xmlns:w14="{W14}" xmlns:w15="{W15}" xmlns:x="urn:foreign"><w:zoom w:percent="100"/><w:defaultTabStop w:val="708"/><x:before/><w:compat><w:compatSetting w:name="a" w:uri="u" w:val="1"/></w:compat><w14:docId w14:val="1"/><w15:chartTrackingRefBased/><w:rsids><w:rsidRoot w:val="0"/></w:rsids></w:settings>"#
    );
    add(
        "styles-and-settings".into(),
        r#"<w:p><w:pPr><w:pStyle w:val="Normal"/></w:pPr><w:r><w:t>s</w:t></w:r></w:p>"#.into(),
        Some(styles),
        Some(settings),
    );
    // Header and footer references, parts with unknown content, numbering,
    // unknown content at the root of styles.xml (parts reused or regenerated).
    let mut d = doc(root(
        r#"<w:p><w:r><w:t>hf</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rel2"/><w:footerReference w:type="default" r:id="rel3"/><w14:ext/><w:pgSz w:w="1" w:h="1"/></w:sectPr>"#,
    ));
    let part = |tag: &str, inner: &str| {
        format!(
            r#"<w:{tag} xmlns:w="{W}" xmlns:w14="{W14}" xmlns:w15="{W15}" xmlns:mc="{MC}" xmlns:x="urn:foreign" mc:Ignorable="w14 x"><w:p w14:paraId="7"><w:pPr><w14:u/></w:pPr><w:r><w:t>{tag}</w:t></w:r><x:in w15:a="1"/></w:p>{inner}</w:{tag}>"#
        )
    };
    d.styles = Some(format!(
        r#"<w:styles xmlns:w="{W}" xmlns:w14="{W14}" xmlns:mc="{MC}" xmlns:x="urn:foreign" mc:Ignorable="w14 x" w14:rootAttr="1"><x:atRoot a="1"/><w14:atRoot2/><w:docDefaults/><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style><x:last/></w:styles>"#
    ));
    d.extra = vec![
        (
            "word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            part("hdr", "<x:tail/>").into_bytes(),
            "header",
        ),
        (
            "word/footer1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
            part("ftr", "<w14:tail/>").into_bytes(),
            "footer",
        ),
        (
            "word/numbering.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
            format!(
                r#"<w:numbering xmlns:w="{W}" xmlns:w14="{W14}" xmlns:mc="{MC}" xmlns:x="urn:foreign" mc:Ignorable="w14 x"><x:first/><w:abstractNum w:abstractNumId="0" w14:a="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><x:inLvl/></w:lvl><w14:inAbstract/></w:abstractNum><w:num w:numId="1" x:b="2"><w:abstractNumId w:val="0"/><x:inNum/></w:num><w14:last/></w:numbering>"#
            )
            .into_bytes(),
            "numbering",
        ),
    ];
    out.push(("headers-footers-numbering".into(), d.bytes()));
    out
}

/// Bodies and cells whose last known block is a table followed by an unknown
/// element, and a table inside a trailing `w:sdt`: Word requires a `w:p`
/// last, the writer adds it (the only difference with the source).
fn tail_cases() -> Vec<(String, Vec<u8>)> {
    let tbl = r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#;
    let sect = r#"<w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr>"#;
    let unk = unknown(0);
    let cases = [
        ("body-tbl-unknown", format!("<w:p/>{tbl}{unk}{sect}")),
        ("body-tbl-unknown-no-sectPr", format!("<w:p/>{tbl}{unk}")),
        (
            "cell-tbl-unknown",
            format!(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc>{tbl}{unk}</w:tc></w:tr></w:tbl><w:p/>{sect}"#
            ),
        ),
        (
            "body-p-sdt-tbl",
            format!("<w:p/><w:sdt><w:sdtPr/><w:sdtContent>{tbl}</w:sdtContent></w:sdt>{sect}"),
        ),
        (
            "cell-p-sdt-tbl",
            format!(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc><w:p/><w:sdt><w:sdtPr/><w:sdtContent>{tbl}</w:sdtContent></w:sdt></w:tc></w:tr></w:tbl><w:p/>{sect}"#
            ),
        ),
    ];
    cases
        .into_iter()
        .map(|(n, b)| (n.to_owned(), doc(root(&b)).bytes()))
        .collect()
}

fn last_block_is_paragraph(c: &C, out: &mut Vec<String>) {
    let C::E(name, _, kids) = c else { return };
    let local = name.rsplit('}').next().unwrap_or("");
    if local == "body" || local == "tc" {
        let last = kids.iter().rev().find_map(|k| match k {
            C::E(n, _, _) if !n.ends_with("}sectPr") => Some(n.rsplit('}').next().unwrap_or("")),
            _ => None,
        });
        if last != Some("p") {
            out.push(format!("{local} ends with {last:?}"));
        }
    }
    for k in kids {
        last_block_is_paragraph(k, out);
    }
}

// ---- Tests ---------------------------------------------------------------------

fn run(docs: &[(String, Vec<u8>)]) -> Vec<Report> {
    let mut reports = Vec::new();
    for (name, bytes) in docs {
        let mut r = check(name, bytes);
        // Stricter checks of the shared helper (re-import equal, parts identical).
        r.losses.extend(round_trip(name, bytes).differences);
        reports.push(r);
    }
    reports
}

fn print(title: &str, reports: &[Report]) -> usize {
    let mut failures = 0;
    println!("== {title}: {} documents", reports.len());
    for r in reports {
        if r.losses.is_empty() {
            println!(
                "  ok    {}: {} parts, {} elements and attributes",
                r.name, r.parts, r.items
            );
        } else {
            failures += 1;
            println!("  LOSS  {}: {} problem(s)", r.name, r.losses.len());
            for l in &r.losses {
                println!("        {l}");
            }
        }
    }
    failures
}

#[test]
fn repository_documents_round_trip_without_loss() {
    let (docs, skipped) = repository_documents();
    assert!(
        docs.len() >= 4,
        "corpus not found: {} documents",
        docs.len()
    );
    for s in &skipped {
        println!("skipped (not a ZIP package, encrypted): {s}");
    }
    let failures = print("repository corpus", &run(&docs));
    assert_eq!(failures, 0, "documents with losses");
}

#[test]
fn synthetic_documents_with_unknown_content_round_trip_without_loss() {
    let docs = synthetic();
    assert!(docs.len() >= 60, "{} synthetic documents", docs.len());
    let failures = print("synthetic corpus", &run(&docs));
    assert_eq!(failures, 0, "documents with losses");
}

fn regenerate() -> DocxWriteOptions {
    DocxWriteOptions {
        regenerate_all: true,
        ..DocxWriteOptions::default()
    }
}

#[test]
fn bodies_and_cells_end_with_a_paragraph_after_a_trailing_table() {
    for (name, bytes) in tail_cases() {
        let imp = import_docx(&bytes).unwrap();
        let written = write_docx(&imp, &regenerate()).unwrap();
        let out = Package::read(&written.bytes).unwrap();
        let uri = imp
            .package
            .parts()
            .keys()
            .find(|u| u.as_str() == "word/document.xml")
            .unwrap();
        let (src, dst) = (
            parse(&imp.package.part(uri).unwrap().bytes),
            parse(&out.part(uri).unwrap().bytes),
        );
        let mut problems = Vec::new();
        last_block_is_paragraph(&dst, &mut problems);
        assert!(problems.is_empty(), "{name}: {problems:?}");
        // Nothing lost; the only addition is the terminating paragraph.
        let (lost, added) = balance(&src, &dst);
        assert!(lost.is_empty(), "{name}: lost {lost:?}");
        assert!(
            added.iter().all(|a| a.ends_with("}p")) && added.len() == 1,
            "{name}: added {added:?}"
        );
        // Written twice: the output is stable and reads back.
        let again = write_docx(&import_docx(&written.bytes).unwrap(), &regenerate()).unwrap();
        let again = Package::read(&again.bytes).unwrap();
        let (a, b) = (
            parse(&out.part(uri).unwrap().bytes),
            parse(&again.part(uri).unwrap().bytes),
        );
        assert_eq!(first_diff(&a, &b, ""), None, "{name}: second round trip");
        println!("  ok    {name}");
    }
}

#[test]
fn ignorable_check_flags_an_undeclared_prefix() {
    let ok = format!(r#"<a xmlns:mc="{MC}" xmlns:p="u" mc:Ignorable="p"><b/></a>"#);
    assert!(undeclared_ignorable(ok.as_bytes()).is_empty());
    let inherited = format!(r#"<a xmlns:p="u"><b xmlns:mc="{MC}" mc:Ignorable="p"/></a>"#);
    assert!(undeclared_ignorable(inherited.as_bytes()).is_empty());
    let bad = format!(r#"<a xmlns:mc="{MC}" mc:Ignorable="p q" xmlns:q="u"><b/></a>"#);
    assert_eq!(undeclared_ignorable(bad.as_bytes()), ["p (at a)"]);
    let sibling = format!(r#"<a xmlns:mc="{MC}"><c xmlns:p="u"/><b mc:Ignorable="p"/></a>"#);
    assert_eq!(undeclared_ignorable(sibling.as_bytes()).len(), 1);
}

/// The determinism check has teeth: without `regenerate_all` the package layout
/// of the source (member order, relationship order, `Default` entries) shows in the
/// output, so the equality proved with it is not an accident of the fixtures.
#[test]
fn determinism_check_sees_the_layout_of_the_source_when_nothing_is_regenerated() {
    let (docs, _) = repository_documents();
    let mut differing = 0;
    for (name, bytes) in &docs {
        let shuffled = shuffled(bytes);
        assert_ne!(&shuffled, bytes, "{name}: the variant is the same file");
        let plain = |b: &[u8]| {
            import_docx(b)
                .ok()
                .and_then(|i| write_docx(&i, &DocxWriteOptions::default()).ok())
                .map(|w| w.bytes)
        };
        if plain(bytes) != plain(&shuffled) {
            differing += 1;
        }
    }
    assert!(differing > 0, "no document shows the layout of its source");
}
