// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-306: preservation of what the DOCX import does not interpret.
//!
//! The completeness proof has two independent halves. `inventory` lists every
//! element and attribute of the source with its own XML walk and requires each
//! to be found in the preserved skeleton or in an opaque copy. `rebuild`
//! reconstructs a whole part from the model plus the captures (a preview of
//! the US-307 writer) and compares it, normalized, with the source.
use atelier_fmt_ooxml::{
    DocxError, DocxLimits, ImportNotice, ImportedDocx, Package, PartUri, Preserved, PreservedChild,
    PreservedElement, PreservedPart, TargetMode, import_docx, import_docx_with_limits,
    interpreted_elements,
};
use atelier_text_model::rich::{Node, NodeId};
use atelier_xml::{Event, Reader};
use std::collections::BTreeSet;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MAIN_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const DOC_CT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

// ---- Packages -------------------------------------------------------------

struct Doc {
    document: String,
    styles: Option<String>,
    settings: Option<String>,
    extra: Vec<(&'static str, &'static str, Vec<u8>, &'static str)>,
}
fn doc(document: String) -> Doc {
    Doc {
        document,
        styles: None,
        settings: None,
        extra: Vec::new(),
    }
}
impl Doc {
    fn bytes(&self) -> Vec<u8> {
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
    fn import(&self) -> ImportedDocx {
        import_docx(&self.bytes()).unwrap()
    }
}
fn body(inner: &str) -> String {
    format!(
        r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:mc="{MC}" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:x="urn:unused" mc:Ignorable="w14 x"><w:body>{inner}</w:body></w:document>"#
    )
}

// ---- Normalized XML -------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
enum C {
    E(String, Vec<(String, String)>, Vec<C>),
    T(String),
}
fn expanded(ns: &Option<String>, local: &str) -> String {
    format!("{{{}}}{local}", ns.as_deref().unwrap_or(""))
}
fn parse(bytes: &[u8]) -> C {
    let mut reader = Reader::new(bytes);
    let mut stack: Vec<C> = Vec::new();
    let mut root = None;
    let close = |stack: &mut Vec<C>, root: &mut Option<C>| {
        let node = stack.pop().unwrap();
        match stack.last_mut() {
            Some(C::E(_, _, kids)) => kids.push(node),
            _ => *root = Some(node),
        }
    };
    while let Some(event) = reader.next_event().unwrap() {
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                let mut attrs: Vec<_> = attributes
                    .iter()
                    .map(|a| (expanded(&a.name.namespace, &a.name.local), a.value.clone()))
                    .collect();
                attrs.sort();
                stack.push(C::E(
                    expanded(&name.namespace, &name.local),
                    attrs,
                    Vec::new(),
                ));
                if empty {
                    close(&mut stack, &mut root);
                }
            }
            Event::EndElement { .. } => close(&mut stack, &mut root),
            Event::Text(t) if !t.trim().is_empty() => {
                if let Some(C::E(_, _, kids)) = stack.last_mut() {
                    if let Some(C::T(prev)) = kids.last_mut() {
                        prev.push_str(&t);
                    } else {
                        kids.push(C::T(t));
                    }
                }
            }
            _ => {}
        }
    }
    root.unwrap()
}
/// Element and attribute keys of a tree, by path.
fn items(node: &C, path: &str, out: &mut Vec<String>) {
    if let C::E(name, attrs, kids) = node {
        let here = format!("{path}/{name}");
        out.push(here.clone());
        for (a, _) in attrs {
            out.push(format!("{here}@{a}"));
        }
        for k in kids {
            items(k, &here, out);
        }
    }
}

// ---- Accounting: what the import kept -------------------------------------

struct Walk<'a> {
    imp: &'a ImportedDocx,
    /// The part the copies come from: they are stored without its root declarations.
    part: &'a PreservedPart,
    interpreted: BTreeSet<&'static str>,
    out: Vec<String>,
}
impl Walk<'_> {
    fn bytes(&mut self, bytes: &[u8], path: &str) {
        items(&parse(&self.part.standalone(bytes)), path, &mut self.out);
    }
    fn element(
        &mut self,
        e: &PreservedElement,
        path: &str,
        para: Option<NodeId>,
        owner: Option<NodeId>,
    ) {
        let owner = e.node.or(owner);
        let para = if e.name.ends_with(":p") || e.name == "p" {
            e.node.or(para)
        } else {
            para
        };
        if e.shadow {
            return;
        }
        let here = format!("{path}/{{{}}}{}", e.namespace, local(&e.name));
        if e.slot {
            // The content sits in the opaque slot of the owning node.
            let slot = self.slot(owner.unwrap(), local(&e.name));
            self.bytes(&slot, path);
        } else {
            self.out.push(here.clone());
            for a in &e.attrs {
                self.out
                    .push(format!("{here}@{{{}}}{}", a.namespace, local(&a.name)));
            }
            let l = local(&e.name);
            assert!(
                self.interpreted.contains(l) || KNOWN_ROOTS.contains(&l),
                "skeleton element {} is not declared interpreted",
                e.name
            );
        }
        if e.shadow {
            return;
        }
        for c in &e.children {
            self.child(c, &here, para, owner);
        }
    }
    fn child(
        &mut self,
        c: &PreservedChild,
        path: &str,
        para: Option<NodeId>,
        owner: Option<NodeId>,
    ) {
        match c {
            PreservedChild::Element(e) => self.element(e, path, para, owner),
            PreservedChild::Opaque(b) => self.bytes(b, path),
            PreservedChild::Anchored(id) => {
                let p = self.imp.document.paragraph(para.unwrap()).unwrap();
                let a = p.anchored.iter().find(|a| a.id == *id).unwrap();
                self.bytes(&a.data, path);
            }
            PreservedChild::Block(id) => match self.imp.document.node(*id).unwrap() {
                Node::Opaque(o) => self.bytes(&o.data, path),
                Node::Paragraph(_) => {
                    let e = &self.imp.preserved.paragraphs[id];
                    self.element(e, path, Some(*id), None);
                }
                _ => {
                    let e = &self.imp.preserved.tables[id];
                    self.element(e, path, para, None);
                }
            },
        }
    }
    fn slot(&self, owner: NodeId, name: &str) -> Vec<u8> {
        let list = match self.imp.document.node(owner).unwrap() {
            Node::Table(t) => &t.opaque,
            Node::Row(r) => &r.opaque,
            Node::Cell(c) => &c.opaque,
            other => panic!("slot on {other:?}"),
        };
        list.iter().find(|o| o.name == name).unwrap().value.clone()
    }
}
const KNOWN_ROOTS: &[&str] = &[
    "document",
    "styles",
    "settings",
    "body",
    "p",
    "r",
    "tbl",
    "tr",
    "tc",
    "t",
    "tab",
    "br",
    "cr",
    "sym",
    "noBreakHyphen",
    "softHyphen",
    "pPr",
    "rPr",
    "tcPr",
    "trPr",
    "tblPr",
    "tblGrid",
    "tblPrEx",
    "docDefaults",
    "rPrDefault",
    "pPrDefault",
    "style",
    "basedOn",
    "link",
    "defaultTabStop",
    "sdt",
    "sdtContent",
    "Fallback",
    "Choice",
];
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap()
}
fn interpreted_set() -> BTreeSet<&'static str> {
    interpreted_elements().into_iter().map(|(_, n)| n).collect()
}
fn parts(imp: &ImportedDocx) -> Vec<&PreservedPart> {
    [
        &imp.preserved.document,
        &imp.preserved.styles,
        &imp.preserved.settings,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Inventory of one document: `(elements, attributes)` checked; panics with
/// the orphans if any element or attribute is neither interpreted nor kept.
fn inventory(bytes: &[u8]) -> (usize, usize) {
    let imp = import_docx(bytes).unwrap();
    let (mut elements, mut attributes) = (0, 0);
    for part in parts(&imp) {
        let uri = PartUri::parse(&part.uri).unwrap();
        let source = imp.package.part(&uri).unwrap().bytes.clone();
        let mut expected = Vec::new();
        items(&parse(&source), "", &mut expected);
        let mut walk = Walk {
            imp: &imp,
            part,
            interpreted: interpreted_set(),
            out: Vec::new(),
        };
        walk.element(&part.root, "", None, None);
        let mut found = walk.out;
        expected.sort();
        found.sort();
        let orphans: Vec<_> = diff(&expected, &found);
        let extras: Vec<_> = diff(&found, &expected);
        assert!(
            orphans.is_empty() && extras.is_empty(),
            "{}: orphans {:?} extras {:?}",
            part.uri,
            &orphans[..orphans.len().min(8)],
            &extras[..extras.len().min(8)]
        );
        attributes += expected.iter().filter(|k| k.contains('@')).count();
        elements += expected.iter().filter(|k| !k.contains('@')).count();
        assert_eq!(
            rebuild(&imp, part),
            parse(&source),
            "rebuild of {}",
            part.uri
        );
    }
    (elements, attributes)
}
/// Multiset difference `a - b` of two sorted lists.
fn diff(a: &[String], b: &[String]) -> Vec<String> {
    let mut rest = b.to_vec();
    let mut out = Vec::new();
    for x in a {
        match rest.binary_search(x) {
            Ok(i) => {
                rest.remove(i);
            }
            Err(_) => out.push(x.clone()),
        }
    }
    out
}

// ---- Reconstruction (preview of the writer) -------------------------------

fn rebuild(imp: &ImportedDocx, part: &PreservedPart) -> C {
    fn elem(
        imp: &ImportedDocx,
        part: &PreservedPart,
        e: &PreservedElement,
        para: Option<NodeId>,
        owner: Option<NodeId>,
    ) -> C {
        let owner = e.node.or(owner);
        let para = if local(&e.name) == "p" {
            e.node.or(para)
        } else {
            para
        };
        if e.slot {
            let w = Walk {
                imp,
                part,
                interpreted: BTreeSet::new(),
                out: Vec::new(),
            };
            return parse(&part.standalone(&w.slot(owner.unwrap(), local(&e.name))));
        }
        let mut attrs: Vec<_> = e
            .attrs
            .iter()
            .map(|a| {
                (
                    format!("{{{}}}{}", a.namespace, local(&a.name)),
                    a.value.clone(),
                )
            })
            .collect();
        attrs.sort();
        let mut kids = Vec::new();
        if local(&e.name) == "t" && e.namespace == W {
            let (a, b) = e.range.unwrap();
            let text = &imp.document.paragraph(para.unwrap()).unwrap().text[a..b];
            if !text.trim().is_empty() {
                kids.push(C::T(text.to_owned()));
            }
        }
        for c in &e.children {
            match c {
                PreservedChild::Element(x) if x.shadow => {}
                PreservedChild::Element(x) => kids.push(elem(imp, part, x, para, owner)),
                PreservedChild::Opaque(b) => kids.push(parse(&part.standalone(b))),
                PreservedChild::Anchored(id) => {
                    let p = imp.document.paragraph(para.unwrap()).unwrap();
                    kids.push(parse(&part.standalone(
                        &p.anchored.iter().find(|a| a.id == *id).unwrap().data,
                    )));
                }
                PreservedChild::Block(id) => match imp.document.node(*id).unwrap() {
                    Node::Opaque(o) => kids.push(parse(&part.standalone(&o.data))),
                    Node::Paragraph(_) => {
                        kids.push(elem(
                            imp,
                            part,
                            &imp.preserved.paragraphs[id],
                            Some(*id),
                            None,
                        ));
                    }
                    _ => kids.push(elem(imp, part, &imp.preserved.tables[id], para, None)),
                },
            }
        }
        C::E(
            format!("{{{}}}{}", e.namespace, local(&e.name)),
            attrs,
            kids,
        )
    }
    elem(imp, part, &part.root, None, None)
}

// ---- Documents ------------------------------------------------------------

const PARA: &str = concat!(
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
const BLOCKS: &str = concat!(
    r#"<w:sdt><w:sdtPr><w:tag w:val="t"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Blk</w:t></w:r></w:p></w:sdtContent></w:sdt>"#,
    r#"<w:customXml w:element="e"><w:p><w:r><w:t>Cx</w:t></w:r></w:p></w:customXml>"#,
    r#"<w:altChunk r:id="rId9"/>"#,
    r#"<w:del w:id="3"><w:p><w:r><w:delText>D</w:delText></w:r></w:p></w:del>"#,
    r#"<mc:AlternateContent><mc:Fallback><w:p><w:r><w:t>Alt</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent>"#,
    r#"<w:sectPr><w:pgSz w:w="1" w:h="2"/></w:sectPr>"#
);
const TABLE: &str = concat!(
    r#"<w:tbl w:rsidR="x"><w:tblPr><w:tblW w:w="100" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="100"/></w:tblGrid>"#,
    r#"<w:bookmarkStart w:id="5" w:name="tb"/>"#,
    r#"<w:tr w:rsidR="1"><w:trPr><w:cantSplit/></w:trPr><w:sdt><w:sdtPr/><w:sdtContent>"#,
    r#"<w:tc><w:tcPr><w:tcW w:w="100" w:type="dxa"/></w:tcPr><w:bookmarkStart w:id="6" w:name="cb"/>"#,
    r#"<w:p><w:r><w:t>cell</w:t></w:r></w:p><w:permStart w:id="7"/></w:tc></w:sdtContent></w:sdt></w:tr></w:tbl>"#
);
const STYLES: &str = concat!(
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">"#,
    r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="fr-FR" w:eastAsia="en-US"/></w:rPr></w:rPrDefault>"#,
    r#"<w:pPrDefault><w:pPr><w:spacing w:after="160"/><w:widowControl/></w:pPr></w:pPrDefault></w:docDefaults>"#,
    r#"<w:latentStyles w:defLockedState="0" w:count="3"><w:lsdException w:name="Normal" w:uiPriority="0"/></w:latentStyles>"#,
    r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal" w14:foo="1"><w:name w:val="Normal"/><w:uiPriority w:val="0"/><w:qFormat/>"#,
    r#"<w:pPr><w:keepNext/><w:jc w:val="left"/></w:pPr><w:rPr><w:noProof/><w:b/></w:rPr></w:style>"#,
    r#"<w:style w:type="table" w:styleId="T1"><w:name w:val="T"/><w:basedOn w:val="Normal"/><w:tblPr><w:tblBorders/></w:tblPr></w:style></w:styles>"#
);
const SETTINGS: &str = concat!(
    r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:zoom w:percent="100"/><w:defaultTabStop w:val="708"/>"#,
    r#"<w:compat><w:compatSetting w:name="a" w:uri="u" w:val="1"/></w:compat><w:rsids><w:rsidRoot w:val="0"/></w:rsids></w:settings>"#
);

fn rich() -> Doc {
    let mut d = doc(body(&format!(
        r#"<w:bookmarkStart w:id="1" w:name="top"/>{PARA}{BLOCKS}"#
    )));
    d.styles = Some(STYLES.into());
    d.settings = Some(SETTINGS.into());
    d
}

fn kinds(imp: &ImportedDocx) -> Vec<String> {
    let section = imp.document.section_ids()[0];
    imp.document
        .block_ids(Some(section))
        .unwrap()
        .iter()
        .map(|id| match imp.document.node(*id).unwrap() {
            Node::Opaque(o) => format!("opaque:{}", o.kind),
            Node::Paragraph(p) => format!("p:{}", p.text),
            Node::Table(_) => "table".into(),
            _ => "other".into(),
        })
        .collect()
}

// ---- Cases of the story ---------------------------------------------------

#[test]
fn inline_elements_are_anchored_at_their_exact_text_offsets() {
    let imp = rich().import();
    let ids = imp.paragraph_ids();
    let p = imp.document.paragraph(ids[0]).unwrap();
    // `del` text is not in the visible text; the chosen fallback branch is.
    assert_eq!(p.text, "AbcdéxlkinstFBSD");
    let got: Vec<_> = p
        .anchored
        .iter()
        .map(|a| (a.offset, a.kind.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (2, "w:fldChar"),
            (4, "w:proofErr"),
            (6, "w:drawing"),
            (6, "w:commentRangeStart"),
            (7, "w:commentRangeEnd"),
            (7, "w:commentReference"),
            (11, "w:del"),
            (13, "m:oMath"),
            (13, "mc:AlternateContent"),
        ]
    );
    let del = p.anchored.iter().find(|a| a.kind == "w:del").unwrap();
    assert!(String::from_utf8_lossy(&del.data).contains("<w:delText>gone</w:delText>"));
    let drawing = p.anchored.iter().find(|a| a.kind == "w:drawing").unwrap();
    assert!(String::from_utf8_lossy(&drawing.data).contains("urn:wp"));
    assert_eq!(imp.document.paragraph(ids[1]).unwrap().text, "Blk");
}

#[test]
fn offsets_snap_to_grapheme_boundaries() {
    // The mark sits between a base letter and its combining accent, which
    // live in different runs: it moves to the start of the cluster.
    let d = doc(body(
        r#"<w:p><w:r><w:t>ae</w:t></w:r><w:proofErr w:type="x"/><w:r><w:t>&#x301;z</w:t></w:r></w:p>"#,
    ));
    let imp = d.import();
    let p = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
    assert_eq!(p.text, "ae\u{301}z");
    assert_eq!(p.anchored.len(), 1);
    assert_eq!(p.anchored[0].offset, 1);
}

#[test]
fn unknown_properties_and_attributes_keep_order_and_values() {
    let imp = rich().import();
    let id = imp.paragraph_ids()[0];
    let skeleton = &imp.preserved.paragraphs[&id];
    assert_eq!(skeleton.node, Some(id));
    let attr = |name: &str| {
        skeleton
            .attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    };
    assert_eq!(attr("w14:paraId"), Some("1A2B3C4D"));
    assert_eq!(attr("w:rsidR"), Some("00AA11BB"));
    let PreservedChild::Element(ppr) = &skeleton.children[0] else {
        panic!("pPr first");
    };
    let shape: Vec<_> = ppr
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => format!("known:{}", e.name),
            PreservedChild::Opaque(b) => {
                format!(
                    "opaque:{}",
                    String::from_utf8_lossy(b)
                        .split(['>', ' ', '/'])
                        .next()
                        .unwrap()
                )
            }
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        shape,
        [
            "known:w:pStyle",
            "opaque:<w:keepNext",
            "known:w:jc",
            "opaque:<w:rPr",
            "opaque:<w:sectPr"
        ]
    );
    // First run: rPr keeps b, lang, sz and the w14 extension in source order.
    let PreservedChild::Element(run) = &skeleton.children[1] else {
        panic!("run second");
    };
    assert_eq!(run.attrs[0].name, "w:rsidR");
    assert_eq!(run.range, Some((0, 4)));
    let PreservedChild::Element(rpr) = &run.children[0] else {
        panic!("rPr first")
    };
    let order: Vec<_> = rpr
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => e.name.clone(),
            PreservedChild::Opaque(b) => String::from_utf8_lossy(b)
                .chars()
                .skip(1)
                .take_while(|c| *c != ' ' && *c != '/')
                .collect(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(order, ["w:b", "w:lang", "w:sz", "w14:ligatures"]);
    // The text leaves keep their ranges; the field character its anchor.
    let ranges: Vec<_> = run
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => e.range,
            _ => None,
        })
        .collect();
    assert_eq!(ranges, [None, Some((0, 2)), None, Some((2, 4))]);
    assert!(matches!(run.children[2], PreservedChild::Anchored(_)));
}

#[test]
fn block_level_unknowns_are_opaque_blocks_at_their_position() {
    let imp = rich().import();
    assert_eq!(
        kinds(&imp),
        [
            "opaque:w:bookmarkStart",
            "p:AbcdéxlkinstFBSD",
            "p:Blk",
            "p:Cx",
            "opaque:w:altChunk",
            "opaque:w:del",
            "opaque:mc:AlternateContent",
            "p:Alt",
        ]
    );
    // Visible text of wrapped blocks is imported; the wrappers are recorded.
    let root = &imp.preserved.document.as_ref().unwrap().root;
    let PreservedChild::Element(body) = &root.children[0] else {
        panic!("body")
    };
    let names: Vec<_> = body
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => format!("{}{}", e.name, if e.shadow { "*" } else { "" }),
            PreservedChild::Block(_) => "block".into(),
            PreservedChild::Opaque(b) => String::from_utf8_lossy(b)[..9].to_owned(),
            PreservedChild::Anchored(_) => "anchored".into(),
        })
        .collect();
    assert_eq!(
        names,
        [
            "block",
            "block",
            "w:sdt",
            "w:customXml",
            "block",
            "block",
            "block",
            "mc:Fallback*",
            "<w:sectPr"
        ]
    );
    let sdt = body.children.iter().find_map(|c| match c {
        PreservedChild::Element(e) if e.name == "w:sdt" => Some(e),
        _ => None,
    });
    let sdt = sdt.unwrap();
    assert!(matches!(&sdt.children[0], PreservedChild::Opaque(b) if b.starts_with(b"<w:sdtPr")));
    assert!(matches!(&sdt.children[1], PreservedChild::Element(e) if e.name == "w:sdtContent"));
}

#[test]
fn table_structure_unknowns_and_wrappers_are_kept() {
    let mut d = doc(body(TABLE));
    d.styles = None;
    let imp = d.import();
    assert_eq!(imp.paragraph_ids().len(), 2); // the cell paragraph and the final empty one
    let table_id = *imp
        .preserved
        .tables
        .keys()
        .find(|id| matches!(imp.document.node(**id), Some(Node::Table(_))))
        .unwrap();
    let t = &imp.preserved.tables[&table_id];
    assert_eq!(t.attrs[0].name, "w:rsidR");
    let shape: Vec<_> = t
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => {
                format!("{}{}", e.name, if e.slot { "=slot" } else { "" })
            }
            PreservedChild::Opaque(_) => "opaque".into(),
            PreservedChild::Block(_) => "row".into(),
            PreservedChild::Anchored(_) => "anchored".into(),
        })
        .collect();
    assert_eq!(shape, ["w:tblPr=slot", "w:tblGrid=slot", "opaque", "row"]);
    let PreservedChild::Block(row_id) = t.children[3] else {
        panic!()
    };
    let row = &imp.preserved.tables[&row_id];
    assert_eq!(row.attrs[0].name, "w:rsidR");
    assert_eq!(row.children.len(), 2); // trPr slot, sdt wrapper
    let PreservedChild::Element(sdt) = &row.children[1] else {
        panic!()
    };
    assert_eq!(sdt.name, "w:sdt");
    let PreservedChild::Element(content) = &sdt.children[1] else {
        panic!()
    };
    let PreservedChild::Block(cell_id) = content.children[0] else {
        panic!()
    };
    let cell = &imp.preserved.tables[&cell_id];
    let blocks = match imp.document.node(cell_id) {
        Some(Node::Cell(c)) => c.blocks.len(),
        _ => 0,
    };
    // tcPr slot, bookmarkStart (opaque block), paragraph, permStart (opaque block)
    assert_eq!(cell.children.len(), 4);
    assert_eq!(blocks, 3);
    inventory(&d.bytes());
}

#[test]
fn styles_and_settings_keep_unknown_properties() {
    let imp = rich().import();
    let styles = imp.preserved.styles.as_ref().unwrap();
    let names: Vec<_> = styles
        .root
        .children
        .iter()
        .map(|c| match c {
            PreservedChild::Element(e) => e.name.clone(),
            PreservedChild::Opaque(b) => String::from_utf8_lossy(b)
                .chars()
                .skip(1)
                .take_while(|c| *c != ' ' && *c != '/' && *c != '>')
                .collect(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(
        names,
        ["w:docDefaults", "w:latentStyles", "w:style", "w:style"]
    );
    let PreservedChild::Element(style) = &styles.root.children[2] else {
        panic!()
    };
    assert_eq!(style.attrs.last().unwrap().name, "w14:foo");
    let PreservedChild::Element(ppr) = &style.children[3] else {
        panic!()
    };
    assert_eq!(ppr.name, "w:pPr");
    assert!(matches!(&ppr.children[0], PreservedChild::Opaque(b) if b.starts_with(b"<w:keepNext")));
    let settings = imp.preserved.settings.as_ref().unwrap();
    assert_eq!(settings.root.children.len(), 4);
    assert!(
        matches!(&settings.root.children[1], PreservedChild::Element(e) if e.name == "w:defaultTabStop")
    );
}

#[test]
fn markup_compatibility_declarations_ignorable_and_must_understand() {
    let imp = rich().import();
    let part = imp.preserved.document.as_ref().unwrap();
    let prefixes: Vec<_> = part.declarations.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(prefixes, ["w", "r", "mc", "w14", "m", "wps", "x"]);
    assert_eq!(
        part.ignorable,
        [
            "http://schemas.microsoft.com/office/word/2010/wordml",
            "urn:unused"
        ]
    );
    // The chosen branch is the fallback (wps is not understood); the whole
    // element is kept for re-emission.
    let p = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
    let alt = p
        .anchored
        .iter()
        .find(|a| a.kind == "mc:AlternateContent")
        .unwrap();
    let xml = String::from_utf8_lossy(&alt.data);
    assert!(
        xml.contains("Requires=\"wps\"")
            && xml.contains("<w:t>CH</w:t>")
            && xml.contains("<w:t>FB</w:t>")
    );
    // Ignorable namespaces raise no notice; a foreign one that is not listed does.
    assert!(!imp.notices.iter().any(|n| matches!(n, ImportNotice::UnsupportedElement { namespace, .. } if namespace == "urn:unused")));
    let m = imp.notices.iter().any(|n| matches!(n, ImportNotice::UnsupportedElement { namespace, local_name, .. } if namespace.ends_with("/math") && local_name == "oMath"));
    assert!(m, "{:?}", imp.notices);

    let xml = format!(
        r#"<w:document xmlns:w="{W}" xmlns:mc="{MC}" xmlns:z="urn:z" mc:MustUnderstand="z"><w:body><w:p><w:r><w:t>ok</w:t></w:r></w:p></w:body></w:document>"#
    );
    let imp = doc(xml).import();
    assert_eq!(
        imp.document.paragraph(imp.paragraph_ids()[0]).unwrap().text,
        "ok"
    );
    assert!(imp.notices.iter().any(|n| matches!(n, ImportNotice::UnsupportedProperty { namespace, local_name, value, .. } if namespace == "urn:z" && local_name == "MustUnderstand" && value == "z")));
}

#[test]
fn unmodeled_parts_and_relationships_are_byte_identical() {
    let blobs: Vec<(&'static str, &'static str, Vec<u8>, &'static str)> = vec![
        ("word/comments.xml", "application/xml", b"<w:comments xmlns:w='urn:w'><w:comment w:id='0'/></w:comments>".to_vec(), "comments"),
        ("word/footnotes.xml", "application/xml", b"<w:footnotes xmlns:w='urn:w'/>".to_vec(), "footnotes"),
        ("word/endnotes.xml", "application/xml", b"<w:endnotes xmlns:w='urn:w'/>".to_vec(), "endnotes"),
        ("word/glossary/document.xml", "application/xml", b"<w:glossaryDocument xmlns:w='urn:w'/>".to_vec(), "glossaryDocument"),
        ("customXml/item1.xml", "application/xml", b"<root>\xc3\xa9</root>".to_vec(), "customXml"),
        ("word/vbaProject.bin", "application/vnd.ms-office.vbaProject", (0..=255u8).collect(), "vbaProject"),
        ("word/webSettings.xml", "application/xml", b"<w:webSettings xmlns:w='urn:w'/>".to_vec(), "webSettings"),
        ("word/fontTable.xml", "application/xml", b"<w:fonts xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:font w:name='A'><w:foo/></w:font></w:fonts>".to_vec(), "fontTable"),
        ("word/theme/theme1.xml", "application/xml", b"<a:theme xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main'><a:x/></a:theme>".to_vec(), "theme"),
        ("word/media/image1.png", "image/png", vec![0x89, b'P', b'N', b'G', 0, 1, 2], "image"),
        ("docProps/thumbnail.jpeg", "image/jpeg", vec![0xff, 0xd8, 9, 9], "thumbnail"),
        ("docProps/custom.xml", "application/xml", b"<Properties/>".to_vec(), "custom-properties"),
    ];
    let mut d = rich();
    d.extra = blobs.clone();
    let source = Package::read(&d.bytes()).unwrap();
    let imp = d.import();
    for (uri, _, bytes, _) in &blobs {
        let u = PartUri::parse(uri).unwrap();
        let part = imp.package.part(&u).unwrap_or_else(|| panic!("{uri} lost"));
        assert_eq!(&part.bytes, bytes, "{uri}");
        assert_eq!(part.content_type, source.part(&u).unwrap().content_type);
    }
    let main = PartUri::parse("word/document.xml").unwrap();
    assert_eq!(
        imp.package.relationships(Some(&main)),
        source.relationships(Some(&main))
    );
    assert_eq!(imp.package.relationships(None), source.relationships(None));
    assert_eq!(imp.package.parts().len(), source.parts().len());
}

#[test]
fn every_interpreted_element_has_an_effect_and_the_code_matches_the_declaration() {
    // Declared => handled: one probe per element of the run and paragraph levels.
    let run = |props: &str, content: &str| {
        let d = doc(body(&format!(
            r#"<w:p><w:r><w:rPr>{props}</w:rPr>{content}</w:r></w:p>"#
        )));
        d.import()
    };
    for (xml, expect_run) in [
        ("<w:b/>", true),
        ("<w:i/>", true),
        ("<w:strike/>", true),
        ("<w:dstrike/>", true),
        (r#"<w:u w:val="double"/>"#, true),
        (r#"<w:vertAlign w:val="superscript"/>"#, true),
        (r#"<w:rFonts w:ascii="Mono"/>"#, true),
        (r#"<w:sz w:val="30"/>"#, true),
        (r#"<w:color w:val="FF0000"/>"#, true),
        (r#"<w:highlight w:val="yellow"/>"#, true),
        (r#"<w:szCs w:val="30"/>"#, true),
    ] {
        let imp = run(xml, "<w:t>x</w:t>");
        let p = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
        assert_eq!(!p.runs.is_empty(), expect_run, "{xml}");
    }
    let imp = run(r#"<w:rStyle w:val="S"/>"#, "<w:t>x</w:t>");
    assert_eq!(imp.run_styles[0].style_id.as_deref(), Some("S"));
    let imp = run(
        "",
        "<w:t>a</w:t><w:tab/><w:br/><w:cr/><w:noBreakHyphen/><w:softHyphen/><w:sym w:font=\"F\" w:char=\"F0A7\"/>",
    );
    let p = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
    assert_eq!(p.text, "a\t\n\n\u{2011}\u{ad}\u{f0a7}");
    for xml in [
        r#"<w:pStyle w:val="S"/>"#,
        r#"<w:jc w:val="center"/>"#,
        r#"<w:ind w:left="5"/>"#,
        r#"<w:spacing w:after="5"/>"#,
    ] {
        let imp = doc(body(&format!("<w:p><w:pPr>{xml}</w:pPr></w:p>"))).import();
        let p = imp.document.paragraph(imp.paragraph_ids()[0]).unwrap();
        assert_ne!(p.props, Default::default(), "{xml}");
    }
    // Declared set == code. Arms of the three readers, found in the source.
    let source = include_str!("../src/docx.rs");
    let arms = |function: &str, indent: usize| -> BTreeSet<String> {
        let start = source.find(&format!("fn {function}(")).unwrap();
        let end = start
            + source[start..]
                .find("\nfn ")
                .unwrap_or(source.len() - start);
        let pad = " ".repeat(indent);
        let mut out = BTreeSet::new();
        for line in source[start..end].lines() {
            if line.starts_with(&pad) && line[indent..].starts_with('"') {
                let head = line
                    .split("=>")
                    .next()
                    .unwrap()
                    .split(" if ")
                    .next()
                    .unwrap();
                out.extend(head.split('"').skip(1).step_by(2).map(str::to_owned));
            }
        }
        out
    };
    let declared = |level: &str| -> BTreeSet<String> {
        interpreted_elements()
            .into_iter()
            .filter(|(l, _)| l == level)
            .map(|(_, n)| n.to_owned())
            .collect()
    };
    let mut rpr = declared("RPr");
    rpr.remove("rStyle"); // read with the run, not by the property reader
    assert_eq!(arms("parse_char_attrs", 12), rpr);
    assert_eq!(arms("parse_para_props", 12), declared("PPr"));
    assert_eq!(arms("parse_paragraph", 16), declared("Run"));
}

// ---- Completeness proof -----------------------------------------------------

#[test]
fn inventory_over_reference_libreoffice_and_built_documents() {
    let mut documents: Vec<(String, Vec<u8>)> = vec![
        (
            "basic.docx".into(),
            include_bytes!("../../../tests/docx-reference/basic.docx").to_vec(),
        ),
        (
            "libreoffice/rich.docx".into(),
            include_bytes!("../../../tests/docx-reference/libreoffice/rich.docx").to_vec(),
        ),
        ("built:rich".into(), rich().bytes()),
        ("built:table".into(), doc(body(TABLE)).bytes()),
    ];
    let merged = concat!(
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:gridSpan w:val="1"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:hMerge/></w:tcPr><w:p/></w:tc></w:tr>"#,
        r#"<w:tr><w:trPr><w:del w:id="1"/></w:trPr><w:tc><w:p/></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t>moved</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"#
    );
    documents.push(("built:merged".into(), doc(body(merged)).bytes()));
    documents.push(("built:misc".into(), doc(body(
        r#"<w:p><w:pPr><w:rPr><w:i/></w:rPr></w:pPr><w:r><w:rPr><w:rStyle w:val="A"/><w:rFonts w:ascii="X" w:eastAsia="Y"/><w:u w:val="words"/><w:color w:val="auto" w:themeShade="1"/></w:rPr><w:br w:clear="all"/><w:sym w:font="F" w:char="41"/></w:r><w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple><w:dir w:val="rtl"><w:r><w:t>d</w:t></w:r></w:dir></w:p>"#,
    )).bytes()));
    // Invalid but seen: children inside elements the schema leaves empty.
    documents.push((
        "built:leaf-children".into(),
        doc(body(
            r#"<w:p><w:r><w:tab><w:ext w:a="1"/></w:tab><w:br><z:q xmlns:z="urn:z"/></w:br><w:cr><w:ext/></w:cr><w:noBreakHyphen><w:ext/></w:noBreakHyphen><w:sym w:font="F" w:char="41"><w:ext/></w:sym><w:t>t<w:ext w:b="2"/></w:t></w:r></w:p>"#,
        ))
        .bytes(),
    ));
    let (mut elements, mut attributes) = (0, 0);
    for (name, bytes) in &documents {
        let (e, a) = std::panic::catch_unwind(|| inventory(bytes))
            .unwrap_or_else(|_| panic!("inventory failed for {name}"));
        elements += e;
        attributes += a;
    }
    eprintln!(
        "inventory: {} documents, {elements} elements, {attributes} attributes, 0 orphans",
        documents.len()
    );
    assert!(elements > 300 && attributes > 100);
}

#[test]
fn limits_bound_the_captures() {
    let bytes = rich().bytes();
    let tight = |f: fn(&mut DocxLimits)| {
        let mut l = DocxLimits::default();
        f(&mut l);
        import_docx_with_limits(&bytes, l).err()
    };
    assert_eq!(
        tight(|l| l.max_opaque_bytes = 100),
        Some(DocxError::Limit("opaque capture bytes"))
    );
    assert_eq!(
        tight(|l| l.max_opaque_items = 3),
        Some(DocxError::Limit("opaque capture count"))
    );
    assert_eq!(
        tight(|l| l.max_preserved_nodes = 5),
        Some(DocxError::Limit("preserved nodes"))
    );
    let imp = import_docx(&bytes).unwrap();
    assert!(imp.preserved.opaque_items > 10);
    assert!(imp.preserved.opaque_bytes > 500);
    let _: &Preserved = &imp.preserved;
}

#[test]
fn mutated_and_truncated_documents_never_panic_and_keep_limits() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }
    let mut rng = Rng(0x05ee_d306);
    let seeds = [rich().document, doc(body(TABLE)).document];
    let alphabet = b"<>/\"=: &;wmcx";
    let (mut ok, mut err) = (0, 0);
    for round in 0..600 {
        let mut text = seeds[round % 2].clone().into_bytes();
        match round % 3 {
            0 => text.truncate(rng.below(text.len())),
            _ => {
                for _ in 0..1 + rng.below(4) {
                    let at = rng.below(text.len());
                    text[at] = alphabet[rng.below(alphabet.len())];
                }
            }
        }
        let mut d = doc(String::new());
        d.document = String::from_utf8_lossy(&text).into_owned();
        let limits = DocxLimits {
            max_opaque_bytes: 65536,
            ..DocxLimits::default()
        };
        match import_docx_with_limits(&d.bytes(), limits) {
            Ok(imp) => {
                ok += 1;
                assert!(imp.preserved.opaque_bytes <= 65536);
                imp.document.validate().unwrap();
            }
            Err(_) => err += 1,
        }
    }
    assert!(ok > 5 && err > 50, "ok {ok} err {err}");
}
