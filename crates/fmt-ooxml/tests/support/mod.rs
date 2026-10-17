// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared by the writer tests and the sample example (US-307): documents
//! built in memory, normalized XML comparison, round trip checks.
#![allow(dead_code)]

use atelier_fmt_ooxml::{
    DocxWriteOptions, ImportedDocx, Package, PartUri, PreservedChild, PreservedElement, TargetMode,
    import_docx, write_docx,
};
use atelier_xml::{Event, Reader};
use std::collections::BTreeSet;

pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MAIN_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
pub const DOC_CT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

// ---- Packages ---------------------------------------------------------------

pub struct Doc {
    pub document: String,
    pub styles: Option<String>,
    pub settings: Option<String>,
    pub extra: Vec<(&'static str, &'static str, Vec<u8>, &'static str)>,
}

pub fn doc(document: String) -> Doc {
    Doc {
        document,
        styles: None,
        settings: None,
        extra: Vec::new(),
    }
}

impl Doc {
    pub fn bytes(&self) -> Vec<u8> {
        let main = PartUri::parse("word/document.xml").unwrap();
        let mut p = Package::new();
        p.insert_part(main.clone(), DOC_CT, self.document.as_bytes().to_vec())
            .unwrap();
        p.add_relationship(
            None,
            "rDoc",
            MAIN_REL,
            "word/document.xml",
            TargetMode::Internal,
        )
        .unwrap();
        let mut count = 0;
        let mut add = |p: &mut Package, uri: &str, ct: &str, bytes: &[u8], rel: &str| {
            count += 1;
            p.insert_part(PartUri::parse(uri).unwrap(), ct, bytes.to_vec())
                .unwrap();
            p.add_relationship(
                Some(main.clone()),
                format!("rel{count}"),
                format!("{REL}{rel}"),
                uri.strip_prefix("word/")
                    .map_or_else(|| format!("../{uri}"), str::to_owned),
                TargetMode::Internal,
            )
            .unwrap();
        };
        if let Some(s) = &self.styles {
            add(
                &mut p,
                "word/styles.xml",
                "application/xml",
                s.as_bytes(),
                "styles",
            );
        }
        if let Some(s) = &self.settings {
            add(
                &mut p,
                "word/settings.xml",
                "application/xml",
                s.as_bytes(),
                "settings",
            );
        }
        for (uri, ct, bytes, rel) in &self.extra {
            add(&mut p, uri, ct, bytes, rel);
        }
        p.write().unwrap()
    }
    pub fn import(&self) -> ImportedDocx {
        import_docx(&self.bytes()).unwrap()
    }
}

pub fn body(inner: &str) -> String {
    format!(
        r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:mc="{MC}" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:x="urn:unused" mc:Ignorable="w14 x"><w:body>{inner}</w:body></w:document>"#
    )
}

// ---- Normalized XML -----------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C {
    E(String, Vec<(String, String)>, Vec<C>),
    T(String),
}

fn expanded(ns: &Option<String>, local: &str) -> String {
    format!("{{{}}}{local}", ns.as_deref().unwrap_or(""))
}

/// Normalized tree: attributes sorted, namespace-resolved names, text nodes
/// kept in order among the children (a mixed element keeps each segment),
/// whitespace-only text dropped between elements but kept where an element has
/// no element child (so a `w:t` of spaces counts), comments and instructions
/// skipped.
pub fn parse(bytes: &[u8]) -> C {
    let mut reader = Reader::new(bytes);
    let mut stack: Vec<(C, String)> = Vec::new();
    let mut root = None;
    fn flush(top: &mut (C, String)) {
        let text = std::mem::take(&mut top.1);
        if let (C::E(_, _, kids), false) = (&mut top.0, text.is_empty()) {
            kids.push(C::T(text));
        }
    }
    fn close(stack: &mut Vec<(C, String)>, root: &mut Option<C>) {
        let mut top = stack.pop().unwrap();
        flush(&mut top);
        let mut node = top.0;
        if let C::E(_, _, kids) = &mut node {
            if kids.iter().any(|k| matches!(k, C::E(..))) {
                kids.retain(|k| !matches!(k, C::T(t) if t.trim().is_empty()));
            }
        }
        match stack.last_mut() {
            Some((C::E(_, _, kids), _)) => kids.push(node),
            _ => *root = Some(node),
        }
    }
    while let Some(event) = reader.next_event().unwrap() {
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                if let Some(top) = stack.last_mut() {
                    flush(top);
                }
                let mut attrs: Vec<_> = attributes
                    .iter()
                    .map(|a| (expanded(&a.name.namespace, &a.name.local), a.value.clone()))
                    .collect();
                attrs.sort();
                stack.push((
                    C::E(expanded(&name.namespace, &name.local), attrs, Vec::new()),
                    String::new(),
                ));
                if empty {
                    close(&mut stack, &mut root);
                }
            }
            Event::EndElement { .. } => close(&mut stack, &mut root),
            Event::Text(t) => {
                if let Some((_, text)) = stack.last_mut() {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    root.unwrap()
}

/// Every prefix listed in an `Ignorable` attribute of the part must be
/// declared (`xmlns:prefix`) on the element that carries it or an ancestor.
/// Returns the undeclared ones as `prefix (at element)`.
pub fn undeclared_ignorable(xml: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(xml);
    let b = text.as_bytes();
    let mut problems = Vec::new();
    let mut scopes: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |n| n + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i += rest.find("]]>").map_or(rest.len(), |n| n + 3);
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
            continue;
        }
        if rest.starts_with("</") {
            scopes.pop();
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
            continue;
        }
        // Start tag: scan to the closing `>` outside quotes.
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            match (quote, b[j]) {
                (0, b'"' | b'\'') => quote = b[j],
                (q, c) if q != 0 && c == q => quote = 0,
                (0, b'>') => break,
                _ => {}
            }
            j += 1;
        }
        let tag = &text[i + 1..j.min(b.len())];
        let empty = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let mut declared = Vec::new();
        let mut ignorable = Vec::new();
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let element = &tag[..name_end];
        let mut k = name_end;
        let tb = tag.as_bytes();
        while k < tb.len() {
            while k < tb.len() && tb[k].is_ascii_whitespace() {
                k += 1;
            }
            let start = k;
            while k < tb.len() && tb[k] != b'=' && !tb[k].is_ascii_whitespace() {
                k += 1;
            }
            let key = &tag[start..k];
            while k < tb.len() && (tb[k] == b'=' || tb[k].is_ascii_whitespace()) {
                k += 1;
            }
            if k >= tb.len() {
                break;
            }
            let q = tb[k];
            let v0 = k + 1;
            k = v0;
            while k < tb.len() && tb[k] != q {
                k += 1;
            }
            let value = &tag[v0..k.min(tb.len())];
            k += 1;
            if let Some(p) = key.strip_prefix("xmlns:") {
                declared.push(p.to_owned());
            } else if key.rsplit(':').next() == Some("Ignorable") && key.contains(':') {
                ignorable.extend(value.split_whitespace().map(str::to_owned));
            }
        }
        scopes.push(declared);
        for p in ignorable {
            if !scopes.iter().any(|s| s.contains(&p)) {
                problems.push(format!("{p} (at {element})"));
            }
        }
        if empty {
            scopes.pop();
        }
        i = j + 1;
    }
    problems
}

/// First difference between two trees, as a path and the two sides.
pub fn first_diff(a: &C, b: &C, path: &str) -> Option<String> {
    match (a, b) {
        (C::T(x), C::T(y)) if x == y => None,
        (C::E(n1, a1, k1), C::E(n2, a2, k2)) => {
            let here = format!("{path}/{n1}");
            if n1 != n2 {
                return Some(format!("{here}: name {n1} vs {n2}"));
            }
            if a1 != a2 {
                return Some(format!("{here}: attributes {a1:?} vs {a2:?}"));
            }
            if k1.len() != k2.len() {
                let short = |k: &[C]| {
                    k.iter()
                        .map(|c| match c {
                            C::E(n, _, _) => n.rsplit('}').next().unwrap_or("").to_owned(),
                            C::T(t) => format!("'{t}'"),
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                };
                return Some(format!(
                    "{here}: children [{}] vs [{}]",
                    short(k1),
                    short(k2)
                ));
            }
            k1.iter()
                .zip(k2)
                .enumerate()
                .find_map(|(i, (x, y))| first_diff(x, y, &format!("{here}[{i}]")))
        }
        _ => Some(format!("{path}: {a:?} vs {b:?}")),
    }
}

// ---- Round trip ---------------------------------------------------------------

#[derive(Debug, Default)]
pub struct RoundTrip {
    pub parts_compared: usize,
    pub parts_identical: usize,
    pub differences: Vec<String>,
}

fn skeleton_uris(imp: &ImportedDocx) -> BTreeSet<String> {
    [
        &imp.preserved.document,
        &imp.preserved.styles,
        &imp.preserved.settings,
    ]
    .into_iter()
    .flatten()
    .map(|p| p.uri.clone())
    .collect()
}

/// Imports, writes (reusing untouched parts, then regenerating every part),
/// compares each part with the source and re-imports the result.
pub fn round_trip(name: &str, bytes: &[u8]) -> RoundTrip {
    let mut rt = RoundTrip::default();
    let imp = import_docx(bytes).unwrap_or_else(|e| panic!("{name}: import: {e:?}"));
    let interpreted = skeleton_uris(&imp);
    for regenerate in [false, true] {
        let options = DocxWriteOptions {
            regenerate_all: regenerate,
            ..DocxWriteOptions::default()
        };
        let written = write_docx(&imp, &options).unwrap_or_else(|e| panic!("{name}: write: {e:?}"));
        let out = Package::read(&written.bytes).unwrap_or_else(|e| panic!("{name}: read: {e:?}"));
        let tag = if regenerate { "regenerated" } else { "reused" };
        // Same set of parts.
        let want: Vec<_> = imp
            .package
            .parts()
            .keys()
            .map(ToString::to_string)
            .collect();
        let got: Vec<_> = out.parts().keys().map(ToString::to_string).collect();
        if want != got {
            rt.differences
                .push(format!("{name} [{tag}]: parts {want:?} vs {got:?}"));
        }
        for (uri, part) in imp.package.parts() {
            let Some(o) = out.part(uri) else { continue };
            if part.content_type != o.content_type {
                rt.differences.push(format!(
                    "{name} [{tag}] {uri}: content type {} vs {}",
                    part.content_type, o.content_type
                ));
            }
            if interpreted.contains(uri.as_str()) {
                rt.parts_compared += 1;
                let (a, b) = (parse(&part.bytes), parse(&o.bytes));
                match first_diff(&a, &b, "") {
                    None => rt.parts_identical += 1,
                    Some(d) => rt.differences.push(format!("{name} [{tag}] {uri}: {d}")),
                }
                if !regenerate && part.bytes != o.bytes {
                    rt.differences
                        .push(format!("{name} [{tag}] {uri}: bytes not reused"));
                }
            } else if part.bytes != o.bytes {
                rt.differences
                    .push(format!("{name} [{tag}] {uri}: bytes differ"));
            }
        }
        // Re-import: the model is equal.
        let again = import_docx(&written.bytes)
            .unwrap_or_else(|e| panic!("{name} [{tag}]: reimport: {e:?}"));
        if again.document != imp.document {
            rt.differences
                .push(format!("{name} [{tag}]: reimported document differs"));
        }
        if again.tables != imp.tables {
            rt.differences
                .push(format!("{name} [{tag}]: reimported tables differ"));
        }
        if again.settings != imp.settings {
            rt.differences
                .push(format!("{name} [{tag}]: reimported settings differ"));
        }
        let (a, b) = (&again.preserved, &imp.preserved);
        for (label, same) in [
            ("document", a.document == b.document),
            ("styles", a.styles == b.styles),
            ("settings", a.settings == b.settings),
            ("paragraphs", a.paragraphs == b.paragraphs),
            ("tables", a.tables == b.tables),
        ] {
            if !same {
                rt.differences.push(format!(
                    "{name} [{tag}]: reimported skeleton differs ({label})"
                ));
            }
        }
        let ids = imp.paragraph_ids().to_vec();
        if again.paragraph_ids() != ids.as_slice() {
            rt.differences
                .push(format!("{name} [{tag}]: paragraph order differs"));
        }
    }
    rt
}

// ---- Documents built for the tests ------------------------------------------

pub const PARA: &str = concat!(
    r#"<w:p w14:paraId="1A2B3C4D" w:rsidR="00AA11BB">"#,
    r#"<w:pPr><w:pStyle w:val="Normal"/><w:keepNext/><w:jc w:val="center"/><w:rPr><w:b/></w:rPr>"#,
    r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr>"#,
    r#"<w:r w:rsidR="00112233"><w:rPr><w:b/><w:lang w:val="fr-FR"/><w:sz w:val="24"/><w14:ligatures w14:val="all"/></w:rPr>"#,
    r#"<w:t>Ab</w:t><w:fldChar w:fldCharType="begin"/><w:t xml:space="preserve">cd</w:t></w:r>"#,
    r#"<w:proofErr w:type="spellStart"/>"#,
    r#"<w:r><w:t>é</w:t><w:drawing><wp:inline xmlns:wp="urn:wp"><wp:extent cx="1" cy="2"/></wp:inline></w:drawing></w:r>"#,
    r#"<w:commentRangeStart w:id="0"/><w:r><w:t>x</w:t></w:r><w:commentRangeEnd w:id="0"/>"#,
    r#"<w:r><w:commentReference w:id="0"/></w:r>"#,
    r#"<w:hyperlink r:id="rId9"><w:r><w:t>lk</w:t></w:r></w:hyperlink>"#,
    r#"<w:ins w:id="1" w:author="a"><w:r><w:t>in</w:t></w:r></w:ins>"#,
    r#"<w:del w:id="2" w:author="a"><w:r><w:delText>gone</w:delText></w:r></w:del>"#,
    r#"<w:smartTag w:uri="u" w:element="e"><w:smartTagPr><w:attr w:name="n" w:val="v"/></w:smartTagPr><w:r><w:t>st</w:t></w:r></w:smartTag>"#,
    r#"<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"#,
    r#"<mc:AlternateContent><mc:Choice Requires="wps"><w:r><w:t>CH</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>FB</w:t></w:r></mc:Fallback></mc:AlternateContent>"#,
    r#"<w:sdt><w:sdtPr><w:alias w:val="a"/></w:sdtPr><w:sdtContent><w:r><w:t>SD</w:t></w:r></w:sdtContent></w:sdt>"#,
    r#"</w:p>"#
);
pub const BLOCKS: &str = concat!(
    r#"<w:sdt><w:sdtPr><w:tag w:val="t"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Blk</w:t></w:r></w:p></w:sdtContent></w:sdt>"#,
    r#"<w:customXml w:element="e"><w:p><w:r><w:t>Cx</w:t></w:r></w:p></w:customXml>"#,
    r#"<w:altChunk r:id="rId9"/>"#,
    r#"<w:del w:id="3"><w:p><w:r><w:delText>D</w:delText></w:r></w:p></w:del>"#,
    r#"<mc:AlternateContent><mc:Fallback><w:p><w:r><w:t>Alt</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>"#,
    r#"<w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#
);
pub const TABLE: &str = concat!(
    r#"<w:tbl w:rsidR="x"><w:tblPr><w:tblW w:w="100" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="100"/></w:tblGrid>"#,
    r#"<w:bookmarkStart w:id="5" w:name="tb"/>"#,
    r#"<w:tr w:rsidR="1"><w:trPr><w:cantSplit/></w:trPr><w:sdt><w:sdtPr/><w:sdtContent>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="100" w:type="dxa"/></w:tcPr><w:bookmarkStart w:id="6" w:name="cb"/>"#,
    r#"<w:p><w:r><w:t>cell</w:t></w:r></w:p><w:permStart w:id="7"/></w:tc></w:sdtContent></w:sdt></w:tr></w:tbl>"#
);
pub const STYLES: &str = concat!(
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">"#,
    r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="fr-FR" w:eastAsia="en-US"/></w:rPr></w:rPrDefault>"#,
    r#"<w:pPrDefault><w:pPr><w:spacing w:after="160"/><w:widowControl/></w:pPr></w:pPrDefault></w:docDefaults>"#,
    r#"<w:latentStyles w:defLockedState="0" w:count="3"><w:lsdException w:name="Normal" w:uiPriority="0"/></w:latentStyles>"#,
    r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal" w14:foo="1"><w:name w:val="Normal"/><w:uiPriority w:val="0"/><w:qFormat/>"#,
    r#"<w:pPr><w:keepNext/><w:jc w:val="left"/></w:pPr><w:rPr><w:noProof/><w:b/></w:rPr></w:style>"#,
    r#"<w:style w:type="table" w:styleId="T1"><w:name w:val="T"/><w:basedOn w:val="Normal"/><w:tblPr><w:tblBorders/></w:tblPr></w:style></w:styles>"#
);
pub const SETTINGS: &str = concat!(
    r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:zoom w:percent="100"/><w:defaultTabStop w:val="708"/>"#,
    r#"<w:compat><w:compatSetting w:name="a" w:uri="u" w:val="1"/></w:compat><w:rsids><w:rsidRoot w:val="0"/></w:rsids></w:settings>"#
);

pub fn rich() -> Doc {
    let mut d = doc(body(&format!(
        r#"<w:bookmarkStart w:id="1" w:name="top"/>{PARA}{BLOCKS}"#
    )));
    d.styles = Some(STYLES.into());
    d.settings = Some(SETTINGS.into());
    d
}

fn grid(n: usize) -> String {
    format!(
        "<w:tblGrid>{}</w:tblGrid>",
        (0..n)
            .map(|_| r#"<w:gridCol w:w="2000"/>"#)
            .collect::<String>()
    )
}
fn tc(text: &str) -> String {
    format!(
        r#"<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#
    )
}

/// Documents with the constructs of the other import tests: tables with
/// merges, runs with every supported property, symbols, tabs and breaks.
pub fn built_documents() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = vec![("rich".into(), rich().bytes())];
    let table = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="T1"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>{}<w:tr><w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>span</w:t></w:r></w:p></w:tc>{}</w:tr><w:tr>{}{}{}</w:tr><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>vm</w:t></w:r></w:p></w:tc>{}{}</w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{}{}</w:tr></w:tbl>"#,
        grid(3),
        tc("c"),
        tc("a"),
        tc("b"),
        tc("c"),
        tc("x"),
        tc("y"),
        tc("p"),
        tc("q")
    );
    let mut d = doc(body(&format!(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p>{table}<w:p><w:r><w:t>after</w:t></w:r></w:p>"#
    )));
    d.styles = Some(STYLES.into());
    out.push(("tables".into(), d.bytes()));
    let text = concat!(
        r#"<w:p><w:pPr><w:pStyle w:val="Normal"/><w:spacing w:before="120" w:after="240" w:line="360" w:lineRule="auto"/><w:ind w:left="720" w:hanging="360"/><w:jc w:val="both"/></w:pPr>"#,
        r#"<w:r><w:rPr><w:rFonts w:ascii="Carlito" w:hAnsi="Carlito"/><w:b/><w:i w:val="0"/><w:strike/><w:color w:val="FF0000"/><w:sz w:val="28"/><w:szCs w:val="28"/><w:highlight w:val="yellow"/><w:u w:val="wave"/><w:vertAlign w:val="superscript"/></w:rPr><w:t xml:space="preserve"> écrit à l'œuvre </w:t></w:r>"#,
        r#"<w:r><w:tab/><w:t>tab</w:t><w:br/><w:t>line</w:t><w:br w:type="page"/><w:noBreakHyphen/><w:softHyphen/><w:sym w:font="Wingdings" w:char="F04A"/></w:r>"#,
        r#"<w:r><w:rPr><w:color w:val="auto"/><w:highlight w:val="none"/></w:rPr><w:t>   spaces   </w:t></w:r></w:p>"#,
        r#"<w:p/><w:p><w:r><w:t>日本語 e&#769; &amp; &lt;tag&gt;</w:t></w:r></w:p>"#
    );
    let mut d = doc(body(text));
    d.styles = Some(STYLES.into());
    d.settings = Some(SETTINGS.into());
    out.push(("text".into(), d.bytes()));
    out
}

/// Documents of the repository corpus.
pub fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/docx-reference");
    let mut out = Vec::new();
    for rel in [
        "basic.docx",
        "libreoffice/rich.docx",
        "libreoffice/properties.docx",
    ] {
        let bytes = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        out.push((rel.to_owned(), bytes));
    }
    out
}

pub fn skeleton_child_count(e: &PreservedElement) -> usize {
    1 + e
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(x) => skeleton_child_count(x),
            _ => 1,
        })
        .sum::<usize>()
}

pub mod order;
pub mod scratch;
