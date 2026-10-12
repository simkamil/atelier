// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(clippy::field_reassign_with_default)]
//! ODT import tests on packages built in memory (hand-written, minimal XML).

use atelier_fmt_odf::odt::{
    ImageAnchor, ImageStatus, ImportNotice, ImportedOdt, ListLevelKind, NS_IMPORT, OdtError,
    OdtLimits, import_odt, import_odt_with_limits,
};
use atelier_text_model::rich::{
    Alignment, Color, HfVariant, LineRule, LinkTarget, Node, NodeId, Underline, VerticalAlign,
};
use atelier_zip::{Method, WriteOptions, Writer};

const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" "#,
    r#"xmlns:dc="http://purl.org/dc/elements/1.1/" "#,
    r#"xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.3""#
);

const TEXT_MIME: &str = "application/vnd.oasis.opendocument.text";

struct Pkg {
    mime: String,
    content_body: String,
    content_auto: String,
    styles: Option<String>,
    meta: Option<String>,
    manifest: Option<String>,
    extra: Vec<(String, Vec<u8>)>,
}

impl Pkg {
    fn new(body: &str) -> Self {
        Self {
            mime: TEXT_MIME.into(),
            content_body: body.into(),
            content_auto: String::new(),
            styles: None,
            meta: None,
            manifest: None,
            extra: Vec::new(),
        }
    }
    fn auto(mut self, a: &str) -> Self {
        self.content_auto = a.into();
        self
    }
    fn styles(mut self, common: &str, auto: &str, masters: &str) -> Self {
        self.styles = Some(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS}><office:styles>{common}</office:styles><office:automatic-styles>{auto}</office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"#
        ));
        self
    }
    fn raw_styles(mut self, xml: &str) -> Self {
        self.styles = Some(xml.into());
        self
    }
    fn meta(mut self, m: &str) -> Self {
        self.meta = Some(format!(
            r#"<office:document-meta {NS}><office:meta>{m}</office:meta></office:document-meta>"#
        ));
        self
    }
    fn file(mut self, name: &str, bytes: &[u8]) -> Self {
        self.extra.push((name.into(), bytes.to_vec()));
        self
    }
    fn content_xml(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS}><office:automatic-styles>{}</office:automatic-styles><office:body><office:text>{}</office:text></office:body></office:document-content>"#,
            self.content_auto, self.content_body
        )
    }
    fn build(&self) -> Vec<u8> {
        self.build_with(&self.content_xml())
    }
    fn build_with(&self, content: &str) -> Vec<u8> {
        let mut w = Writer::new(WriteOptions::default());
        w.add("mimetype", self.mime.as_bytes(), Method::Store)
            .unwrap();
        w.add("content.xml", content.as_bytes(), Method::Deflate)
            .unwrap();
        if let Some(s) = &self.styles {
            w.add("styles.xml", s.as_bytes(), Method::Deflate).unwrap();
        }
        if let Some(m) = &self.meta {
            w.add("meta.xml", m.as_bytes(), Method::Deflate).unwrap();
        }
        let manifest = self.manifest.clone().unwrap_or_else(|| {
            format!(
                r#"<manifest:manifest {NS}><manifest:file-entry manifest:full-path="/" manifest:media-type="{}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#,
                self.mime
            )
        });
        w.add(
            "META-INF/manifest.xml",
            manifest.as_bytes(),
            Method::Deflate,
        )
        .unwrap();
        for (n, b) in &self.extra {
            w.add(n, b, Method::Store).unwrap();
        }
        w.finish().unwrap()
    }
}

fn import(p: &Pkg) -> ImportedOdt {
    import_odt(&p.build()).unwrap()
}

fn body_paragraphs(doc: &ImportedOdt) -> Vec<(NodeId, atelier_text_model::rich::Paragraph)> {
    doc.paragraph_ids()
        .iter()
        .map(|id| (*id, doc.document.paragraph(*id).unwrap().clone()))
        .collect()
}

fn texts(doc: &ImportedOdt) -> Vec<String> {
    body_paragraphs(doc).into_iter().map(|p| p.1.text).collect()
}

fn opaque(list: &[atelier_text_model::rich::OpaqueAttr], name: &str) -> Option<String> {
    list.iter()
        .find(|a| a.name == name)
        .map(|a| String::from_utf8(a.value.clone()).unwrap())
}

// ---------------------------------------------------------------- package

#[test]
fn accented_text_and_package_checks() {
    let p = Pkg::new("<text:p>Éléphant à l’œuvre — «ça» 日本語 😀</text:p><text:p/>");
    let doc = import(&p);
    assert_eq!(texts(&doc), vec!["Éléphant à l’œuvre — «ça» 日本語 😀", ""]);
    assert_eq!(doc.mime_type, TEXT_MIME);
    assert!(!doc.is_template);
    // styles.xml and meta.xml are absent: imported anyway, with notices.
    assert!(doc.notices.contains(&ImportNotice::MissingPart {
        part: "styles.xml".into()
    }));
}

#[test]
fn template_is_accepted_and_other_odf_types_are_named() {
    let mut p = Pkg::new("<text:p>t</text:p>");
    p.mime = "application/vnd.oasis.opendocument.text-template".into();
    let doc = import(&p);
    assert!(doc.is_template);
    let mut p = Pkg::new("<text:p>t</text:p>");
    p.mime = "application/vnd.oasis.opendocument.spreadsheet".into();
    match import_odt(&p.build()) {
        Err(OdtError::UnsupportedDocumentType { mime_type }) => {
            assert_eq!(mime_type, "application/vnd.oasis.opendocument.spreadsheet")
        }
        other => panic!("unexpected {:?}", other.err()),
    }
    // A lying mimetype is caught by the body.
    let bytes = Pkg::new("")
        .build_with(&format!(
            r#"<office:document-content {NS}><office:body><office:presentation/></office:body></office:document-content>"#
        ));
    match import_odt(&bytes) {
        Err(OdtError::UnsupportedDocumentType { mime_type }) => {
            assert!(mime_type.ends_with("presentation"))
        }
        other => panic!("unexpected {:?}", other.err()),
    }
}

#[test]
fn encrypted_package_is_reported() {
    let mut p = Pkg::new("<text:p>secret</text:p>");
    p.manifest = Some(format!(
        r#"<manifest:manifest {NS}><manifest:file-entry manifest:full-path="/" manifest:media-type="{TEXT_MIME}"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"><manifest:encryption-data manifest:checksum-type="SHA1/1K" manifest:checksum="AA=="><manifest:algorithm manifest:algorithm-name="http://www.w3.org/2001/04/xmlenc#aes256-cbc" manifest:initialisation-vector="AA=="/><manifest:key-derivation manifest:key-derivation-name="PBKDF2" manifest:salt="AA==" manifest:iteration-count="100000"/></manifest:encryption-data></manifest:file-entry></manifest:manifest>"#
    ));
    assert_eq!(
        import_odt(&p.build()).err(),
        Some(OdtError::PasswordProtected)
    );
}

#[test]
fn not_odf_inputs_give_typed_errors() {
    assert!(matches!(import_odt(b""), Err(OdtError::Zip(_))));
    assert!(matches!(import_odt(b"hello"), Err(OdtError::Zip(_))));
    let mut w = Writer::new(WriteOptions::default());
    w.add("word/document.xml", b"<x/>", Method::Store).unwrap();
    assert!(matches!(
        import_odt(&w.finish().unwrap()),
        Err(OdtError::NotOdf(_))
    ));
    let mut w = Writer::new(WriteOptions::default());
    w.add("mimetype", b"application/zip", Method::Store)
        .unwrap();
    assert!(matches!(
        import_odt(&w.finish().unwrap()),
        Err(OdtError::NotOdf(_))
    ));
    let mut w = Writer::new(WriteOptions::default());
    w.add("mimetype", TEXT_MIME.as_bytes(), Method::Store)
        .unwrap();
    // A package without content.xml (style-only template) opens as an empty document.
    let doc = import_odt(&w.finish().unwrap()).unwrap();
    assert!(doc.notices.contains(&ImportNotice::MissingPart {
        part: "content.xml".into()
    }));
    assert!(doc.paragraph_ids().is_empty());
    assert_eq!(doc.document.section_ids().len(), 1);
}

#[test]
fn malformed_content_is_a_typed_error_but_bad_styles_and_meta_are_not() {
    let p = Pkg::new("<text:p>ok</text:p>");
    let bad = p.build_with("<office:document-content><oops></office:document-content>");
    assert!(matches!(import_odt(&bad), Err(OdtError::Xml { .. })));
    let p = Pkg::new("<text:p>still here</text:p>")
        .raw_styles("<office:document-styles><broken>")
        .meta("<dc:title>unterminated");
    let doc = import(&p);
    assert_eq!(texts(&doc), vec!["still here"]);
    let unreadable: Vec<_> = doc
        .notices
        .iter()
        .filter_map(|n| match n {
            ImportNotice::UnreadablePart { part, .. } => Some(part.as_str()),
            _ => None,
        })
        .collect();
    assert!(unreadable.contains(&"styles.xml") && unreadable.contains(&"meta.xml"));
}

// ------------------------------------------------------------------- text

#[test]
fn whitespace_follows_odf_rules() {
    let p = Pkg::new(
        "<text:p>   leading   and\n\t internal   </text:p>\
         <text:p>a<text:s/>b<text:s text:c=\"3\"/>c<text:tab/>d<text:line-break/>e</text:p>\
         <text:p>x<text:soft-page-break/>y</text:p>\
         <text:p><text:s text:c=\"2\"/>lead</text:p>",
    );
    let doc = import(&p);
    assert_eq!(
        texts(&doc),
        vec!["leading and internal ", "a b   c\td\ne", "xy", "  lead"]
    );
}

#[test]
fn headings_use_heading_styles_and_standard_maps_to_normal() {
    let p = Pkg::new(
        r#"<text:h text:style-name="Heading_20_1" text:outline-level="1">Titre</text:h>
           <text:h text:outline-level="2">Sous</text:h>
           <text:p text:style-name="Standard">Corps</text:p>"#,
    )
    .styles(
        r#"<style:default-style style:family="paragraph"><style:text-properties fo:font-size="12pt"/></style:default-style>
           <style:style style:name="Standard" style:family="paragraph" style:class="text"/>
           <style:style style:name="Heading" style:family="paragraph" style:next-style-name="Text_20_body"><style:text-properties fo:font-size="14pt" fo:font-weight="bold"/></style:style>
           <style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Heading" style:next-style-name="Text_20_body"><style:text-properties fo:font-size="130%"/></style:style>
           <style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:parent-style-name="Heading"/>
           <style:style style:name="Text_20_body" style:display-name="Text body" style:family="paragraph"/>"#,
        "",
        "",
    );
    let mut doc = import(&p);
    let ids = doc.paragraph_ids().to_vec();
    let styles: Vec<_> = ids
        .iter()
        .map(|id| doc.document.paragraph(*id).unwrap().props.style.clone())
        .collect();
    assert_eq!(
        styles,
        vec![
            Some("Heading1".into()),
            Some("Heading2".into()),
            Some("Normal".into())
        ]
    );
    assert_eq!(doc.headings, vec![(ids[0], 1), (ids[1], 2)]);
    assert_eq!(doc.style_names.get("Heading1").unwrap(), "Heading 1");
    assert_eq!(doc.next_styles.get("Heading1").unwrap(), "BodyText");
    // 130 % of the 14 pt parent: 18.2 pt -> 36 half-points; bold inherited.
    let text = doc.document.paragraph(ids[0]).unwrap().clone();
    assert!(text.runs.is_empty());
    let (_, resolved) = doc.resolved_paragraph(ids[0]).unwrap();
    assert_eq!(resolved.character.size_half_points, Some(36));
    assert_eq!(resolved.character.bold, Some(true));
    let (_, resolved) = doc.resolved_paragraph(ids[1]).unwrap();
    assert_eq!(resolved.character.size_half_points, Some(28));
    let (_, resolved) = doc.resolved_paragraph(ids[2]).unwrap();
    assert_eq!(resolved.character.size_half_points, Some(24));
    assert_eq!(resolved.character.bold, None);
}

#[test]
fn spans_and_automatic_styles_become_direct_formatting() {
    let p = Pkg::new(
        r#"<text:p text:style-name="P1">plain <text:span text:style-name="T1">bold</text:span> <text:span text:style-name="Strong_20_Emphasis">strong<text:span text:style-name="T2"> both</text:span></text:span></text:p>"#,
    )
    .auto(
        r#"<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard"><style:text-properties fo:color="&#35;336699"/></style:style>
           <style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style>
           <style:style style:name="T2" style:family="text"><style:text-properties style:text-underline-style="solid" style:text-underline-type="double"/></style:style>"#,
    )
    .styles(
        r#"<style:style style:name="Standard" style:family="paragraph"/>
           <style:style style:name="Strong_20_Emphasis" style:display-name="Strong Emphasis" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#,
        "",
        "",
    );
    let mut doc = import(&p);
    let id = doc.paragraph_ids()[0];
    let para = doc.document.paragraph(id).unwrap().clone();
    assert_eq!(para.text, "plain bold strong both");
    assert_eq!(para.props.style.as_deref(), Some("Normal"));
    // Paragraph-level automatic text properties apply to the whole text.
    let blue = Color {
        r: 0x33,
        g: 0x66,
        b: 0x99,
    };
    assert_eq!(para.runs.len(), 4);
    assert_eq!(para.runs[0].attrs.color, Some(blue));
    assert_eq!(para.runs[0].attrs.bold, None);
    assert_eq!((para.runs[1].start, para.runs[1].end), (6, 10));
    assert_eq!(para.runs[1].attrs.bold, Some(true));
    assert_eq!(para.runs[1].attrs.italic, Some(true));
    assert_eq!(para.runs[3].attrs.underline, Some(Underline::Double));
    // The named character style is a style reference, not direct bold.
    let styled: Vec<_> = doc
        .run_styles
        .iter()
        .map(|r| (r.start, r.end, r.style_id.clone()))
        .collect();
    assert_eq!(styled, vec![(11, 22, Some("Strong".into()))]);
    let (resolved, _) = doc.resolved_paragraph(id).unwrap();
    let strong = resolved
        .runs
        .iter()
        .find(|r| r.start <= 12 && r.end > 12)
        .unwrap();
    assert_eq!(strong.attrs.bold, Some(true));
    assert_eq!(strong.attrs.color, Some(blue));
}

#[test]
fn character_properties_are_converted() {
    let p = Pkg::new(
        r#"<text:p><text:span text:style-name="A">a</text:span><text:span text:style-name="B">b</text:span><text:span text:style-name="C">c</text:span><text:span text:style-name="D">d</text:span></text:p>"#,
    )
    .auto(
        r#"<style:style style:name="A" style:family="text"><style:text-properties style:font-name="Liberation Serif" fo:font-size="14pt" fo:font-weight="700" style:text-line-through-style="solid" style:text-position="super 58%" fo:background-color="&#35;ffff00" fo:letter-spacing="0.05cm" fo:font-variant="small-caps" fo:language="fr" fo:country="FR" text:display="none"/></style:style>
           <style:style style:name="B" style:family="text"><style:text-properties fo:font-family="'Courier New'" fo:font-style="oblique" style:text-underline-style="dotted" style:text-position="-33% 58%" fo:text-transform="uppercase" fo:color="&#35;ff0000"/></style:style>
           <style:style style:name="C" style:family="text"><style:text-properties style:text-underline-style="wave" style:text-position="0% 100%" fo:font-weight="normal"/></style:style>
           <style:style style:name="D" style:family="text"><style:text-properties style:text-underline-style="solid" style:text-underline-width="bold" fo:background-color="transparent"/></style:style>"#,
    )
    .styles(
        "",
        "",
        "",
    );
    // font-face-decls live in content.xml
    let content = p.content_xml().replace(
        "<office:automatic-styles>",
        r#"<office:font-face-decls><style:font-face style:name="Liberation Serif" svg:font-family="'Liberation Serif'"/></office:font-face-decls><office:automatic-styles>"#,
    );
    let doc = import_odt(&p.build_with(&content)).unwrap();
    let para = doc
        .document
        .paragraph(doc.paragraph_ids()[0])
        .unwrap()
        .clone();
    let a = &para.runs[0].attrs;
    assert_eq!(a.font.as_deref(), Some("Liberation Serif"));
    assert_eq!(a.size_half_points, Some(28));
    assert_eq!(a.bold, Some(true));
    assert_eq!(a.strike, Some(true));
    assert_eq!(a.vertical_align, Some(VerticalAlign::Superscript));
    assert_eq!(
        a.highlight,
        Some(Color {
            r: 255,
            g: 255,
            b: 0
        })
    );
    assert_eq!(
        opaque(&a.opaque, "letter-spacing").as_deref(),
        Some("0.05cm")
    );
    assert_eq!(
        opaque(&a.opaque, "font-variant").as_deref(),
        Some("small-caps")
    );
    assert_eq!(opaque(&a.opaque, "language").as_deref(), Some("fr"));
    assert_eq!(opaque(&a.opaque, "display").as_deref(), Some("none"));
    let b = &para.runs[1].attrs;
    assert_eq!(b.font.as_deref(), Some("Courier New"));
    assert_eq!(b.italic, Some(true));
    assert_eq!(b.underline, Some(Underline::Dotted));
    assert_eq!(b.vertical_align, Some(VerticalAlign::Subscript));
    assert_eq!(
        opaque(&b.opaque, "text-transform").as_deref(),
        Some("uppercase")
    );
    let c = &para.runs[2].attrs;
    assert_eq!(c.underline, Some(Underline::Wave));
    assert_eq!(c.vertical_align, Some(VerticalAlign::Baseline));
    assert_eq!(c.bold, Some(false));
    let d = &para.runs[3].attrs;
    assert_eq!(d.underline, Some(Underline::Thick));
    // A transparent background cannot reset a cascade: kept opaque with a notice.
    assert_eq!(
        opaque(&d.opaque, "background-color").as_deref(),
        Some("transparent")
    );
    assert!(doc.notices.iter().any(|n| matches!(n,
        ImportNotice::UnsupportedProperty { property, .. } if property == "fo:letter-spacing")));
}

#[test]
fn paragraph_properties_are_converted() {
    let p = Pkg::new(
        r#"<text:p text:style-name="P1">a</text:p><text:p text:style-name="P2">b</text:p><text:p text:style-name="P3">c</text:p><text:p text:style-name="P4">d</text:p>"#,
    )
    .auto(
        r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="end" fo:margin-left="2.54cm" fo:margin-right="1in" fo:text-indent="-0.5cm" fo:margin-top="12pt" fo:margin-bottom="6pt" fo:line-height="150%" fo:keep-with-next="always" fo:break-before="page" fo:orphans="2" fo:widows="2" fo:keep-together="always" fo:background-color="&#35;eeeeee" fo:border="0.5pt solid #000000"><style:tab-stops><style:tab-stop style:position="2cm"/><style:tab-stop style:position="1in" style:type="right" style:leader-text="."/></style:tab-stops></style:paragraph-properties></style:style>
           <style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="start" fo:line-height="0.6cm"/></style:style>
           <style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="justify" style:line-height-at-least="1cm" style:line-spacing="0.2cm"/></style:style>
           <style:style style:name="P4" style:family="paragraph"><style:paragraph-properties fo:text-align="center" style:writing-mode="rl-tb"/></style:style>"#,
    );
    let doc = import(&p);
    let props: Vec<_> = body_paragraphs(&doc)
        .into_iter()
        .map(|p| p.1.props)
        .collect();
    let p1 = &props[0];
    assert_eq!(p1.alignment, Some(Alignment::Right));
    assert_eq!(p1.indent_left, Some(1440));
    assert_eq!(p1.indent_right, Some(1440));
    assert_eq!(p1.indent_first_line, Some(-283));
    assert_eq!(p1.space_before, Some(240));
    assert_eq!(p1.space_after, Some(120));
    let ls = p1.line_spacing.unwrap();
    assert_eq!((ls.rule, ls.value), (LineRule::Auto, 360));
    assert_eq!(
        opaque(&p1.opaque, "keep-with-next").as_deref(),
        Some("always")
    );
    assert_eq!(opaque(&p1.opaque, "break-before").as_deref(), Some("page"));
    assert_eq!(opaque(&p1.opaque, "orphans").as_deref(), Some("2"));
    assert_eq!(opaque(&p1.opaque, "widows").as_deref(), Some("2"));
    assert_eq!(
        opaque(&p1.opaque, "keep-together").as_deref(),
        Some("always")
    );
    assert_eq!(
        opaque(&p1.opaque, "tab-stops").as_deref(),
        Some("1134:left:;1440:right:.")
    );
    assert_eq!(
        opaque(&p1.opaque, "border").as_deref(),
        Some("0.5pt solid #000000")
    );
    assert_eq!(props[1].alignment, Some(Alignment::Left));
    let ls = props[1].line_spacing.unwrap();
    assert_eq!((ls.rule, ls.value), (LineRule::Exact, 340));
    assert_eq!(props[2].alignment, Some(Alignment::Justify));
    let ls = props[2].line_spacing.unwrap();
    assert_eq!((ls.rule, ls.value), (LineRule::AtLeast, 567));
    assert_eq!(
        opaque(&props[2].opaque, "line-spacing").as_deref(),
        Some("0.2cm")
    );
    assert_eq!(props[3].alignment, Some(Alignment::Center));
}

#[test]
fn style_inheritance_defaults_unknown_parents_and_cycles() {
    let p = Pkg::new(
        r#"<text:p text:style-name="Child">x</text:p><text:p text:style-name="Missing">y</text:p><text:p text:style-name="CycA">z</text:p>"#,
    )
    .styles(
        r#"<style:default-style style:family="paragraph"><style:paragraph-properties fo:margin-bottom="10pt"/><style:text-properties fo:font-size="11pt" style:font-name="Base"/></style:default-style>
           <style:style style:name="Parent" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-style="italic"/></style:style>
           <style:style style:name="Child" style:family="paragraph" style:parent-style-name="Parent"><style:text-properties fo:font-weight="bold" fo:font-size="200%"/></style:style>
           <style:style style:name="Orphan" style:family="paragraph" style:parent-style-name="Nowhere"/>
           <style:style style:name="CycA" style:family="paragraph" style:parent-style-name="CycB"/>
           <style:style style:name="CycB" style:family="paragraph" style:parent-style-name="CycA"/>"#,
        "",
        "",
    );
    let mut doc = import(&p);
    let ids = doc.paragraph_ids().to_vec();
    let (_, r) = doc.resolved_paragraph(ids[0]).unwrap();
    assert_eq!(r.paragraph.alignment, Some(Alignment::Center));
    assert_eq!(r.paragraph.space_after, Some(200));
    assert_eq!(r.character.italic, Some(true));
    assert_eq!(r.character.bold, Some(true));
    assert_eq!(r.character.size_half_points, Some(44));
    assert_eq!(r.character.font.as_deref(), Some("Base"));
    // Unknown paragraph style: no style reference, notice, text kept.
    assert_eq!(doc.document.paragraph(ids[1]).unwrap().props.style, None);
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::UnknownStyle { name, .. } if name == "Missing"))
    );
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::UnknownStyle { name, .. } if name == "Nowhere"))
    );
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::StyleCycle { .. }))
    );
    doc.resolved_paragraph(ids[2]).unwrap();
}

// ------------------------------------------------------------------ lists

#[test]
fn lists_keep_levels_ids_and_definitions() {
    let p = Pkg::new(
        r#"<text:list text:style-name="L1" xml:id="list1">
             <text:list-header><text:p>Head</text:p></text:list-header>
             <text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>one.a</text:p></text:list-item></text:list></text:list-item>
             <text:list-item text:start-value="5"><text:p>five</text:p><text:p>five bis</text:p></text:list-item>
           </text:list>
           <text:p>between</text:p>
           <text:list text:style-name="L1" text:continue-numbering="true"><text:list-item><text:p>cont</text:p></text:list-item></text:list>
           <text:list text:style-name="L2"><text:list-item><text:p>bullet</text:p></text:list-item></text:list>"#,
    )
    .auto(
        r#"<text:list-style style:name="L1"><text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"><style:list-level-properties text:space-before="0.5cm" text:min-label-width="0.5cm"/></text:list-level-style-number><text:list-level-style-number text:level="2" style:num-format="a" style:num-suffix=")" text:start-value="3"/></text:list-style>
           <text:list-style style:name="L2"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>"#,
    );
    let doc = import(&p);
    let paras = body_paragraphs(&doc);
    assert_eq!(
        paras.iter().map(|p| p.1.text.as_str()).collect::<Vec<_>>(),
        vec![
            "Head", "one", "one.a", "five", "five bis", "between", "cont", "bullet"
        ]
    );
    let refs: Vec<_> = paras
        .iter()
        .map(|p| p.1.props.list.map(|l| (l.list_id, l.level)))
        .collect();
    let first = refs[0].unwrap().0;
    assert_eq!(refs[0], Some((first, 0)));
    assert_eq!(refs[1], Some((first, 0)));
    assert_eq!(refs[2], Some((first, 1)));
    assert_eq!(refs[3], Some((first, 0)));
    assert_eq!(refs[5], None);
    assert_eq!(
        refs[6].unwrap().0,
        first,
        "continue-numbering reuses the list"
    );
    let bullet = refs[7].unwrap().0;
    assert_ne!(bullet, first);
    let def = doc.lists.iter().find(|l| l.list_id == first).unwrap();
    assert_eq!(def.style_name.as_deref(), Some("L1"));
    assert_eq!(def.levels.len(), 2);
    assert_eq!(def.levels[0].min_label_width, Some(283));
    assert_eq!(
        def.levels[1].kind,
        ListLevelKind::Number {
            format: "a".into(),
            prefix: String::new(),
            suffix: ")".into(),
            start: 3
        }
    );
    let bdef = doc.lists.iter().find(|l| l.list_id == bullet).unwrap();
    assert_eq!(bdef.levels[0].kind, ListLevelKind::Bullet("•".into()));
    assert!(
        doc.list_items
            .iter()
            .any(|i| i.header && i.paragraph == paras[0].0)
    );
    assert!(
        doc.list_items
            .iter()
            .any(|i| i.start_value == Some(5) && i.paragraph == paras[3].0)
    );
    assert!(
        doc.list_items
            .iter()
            .any(|i| i.continuation && i.paragraph == paras[4].0)
    );
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::ListNumbering { style } if style == "L1"))
    );
}

#[test]
fn list_nesting_is_bounded() {
    let mut body = String::new();
    for _ in 0..60 {
        body.push_str("<text:list><text:list-item>");
    }
    body.push_str("<text:p>deep</text:p>");
    for _ in 0..60 {
        body.push_str("</text:list-item></text:list>");
    }
    let doc = import(&Pkg::new(&body));
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::NestingLimit { what } if *what == "lists"))
    );
    assert!(doc.paragraph_ids().is_empty());
    let mut limits = OdtLimits::default();
    limits.max_nesting = 100;
    let bytes = Pkg::new(&body).build();
    let doc = import_odt_with_limits(&bytes, limits).unwrap();
    let para = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(
        para.props.list.unwrap().level,
        8,
        "level is clamped to the model maximum"
    );
}

// ----------------------------------------------------------------- tables

fn table_of(doc: &ImportedOdt) -> (NodeId, atelier_text_model::rich::Table) {
    let section = doc.document.section_ids()[0];
    for id in doc.document.block_ids(Some(section)).unwrap() {
        if let Some(Node::Table(t)) = doc.document.node(id) {
            return (id, t.clone());
        }
    }
    panic!("no table");
}

fn cell_text(doc: &ImportedOdt, id: NodeId) -> String {
    let Some(Node::Cell(c)) = doc.document.node(id) else {
        panic!()
    };
    c.blocks
        .to_vec()
        .iter()
        .filter_map(|b| match doc.document.node(*b) {
            Some(Node::Paragraph(p)) => Some(p.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn tables_with_merges_header_rows_widths_and_repeats() {
    let p = Pkg::new(
        r#"<table:table table:name="T" table:style-name="Tbl">
             <table:table-column table:style-name="C1" table:number-columns-repeated="2"/>
             <table:table-column table:style-name="C2"/>
             <table:table-header-rows><table:table-row>
               <table:table-cell table:number-columns-spanned="2" table:style-name="Cell"><text:p>H</text:p></table:table-cell><table:covered-table-cell/>
               <table:table-cell><text:p>H3</text:p></table:table-cell></table:table-row></table:table-header-rows>
             <table:table-row>
               <table:table-cell table:number-rows-spanned="2"><text:p>tall</text:p></table:table-cell>
               <table:table-cell><text:p>b</text:p></table:table-cell>
               <table:table-cell><text:p>c</text:p><text:p>c2</text:p></table:table-cell></table:table-row>
             <table:table-row>
               <table:covered-table-cell/>
               <table:table-cell table:number-columns-repeated="2"><text:p>r</text:p></table:table-cell></table:table-row>
           </table:table>
           <text:p>after</text:p>"#,
    )
    .auto(
        r#"<style:style style:name="C1" style:family="table-column"><style:table-column-properties style:column-width="2.54cm"/></style:style>
           <style:style style:name="C2" style:family="table-column"><style:table-column-properties style:column-width="1in"/></style:style>
           <style:style style:name="Tbl" style:family="table"><style:table-properties style:width="17cm" table:align="margins"/></style:style>
           <style:style style:name="Cell" style:family="table-cell"><style:table-cell-properties fo:background-color="&#35;dddddd" fo:padding="0.1cm"/></style:style>"#,
    );
    let doc = import(&p);
    let (_, t) = table_of(&doc);
    assert_eq!(t.columns, 3);
    assert_eq!(t.rows.len(), 3);
    assert_eq!(
        opaque(&t.opaque, "column-widths").as_deref(),
        Some("1440,1440,1440")
    );
    assert_eq!(opaque(&t.opaque, "header-rows").as_deref(), Some("1"));
    assert_eq!(opaque(&t.opaque, "width").as_deref(), Some("17cm"));
    let rows: Vec<_> = t
        .rows
        .iter()
        .map(|r| {
            let Some(Node::Row(row)) = doc.document.node(*r) else {
                panic!()
            };
            row.cells.clone()
        })
        .collect();
    assert_eq!(rows[0].len(), 2);
    assert_eq!(rows[1].len(), 3);
    assert_eq!(rows[2].len(), 2);
    let Some(Node::Cell(h)) = doc.document.node(rows[0][0]) else {
        panic!()
    };
    assert_eq!((h.col_span, h.row_span), (2, 1));
    assert_eq!(
        opaque(&h.opaque, "background-color").as_deref(),
        Some("#dddddd")
    );
    let Some(Node::Cell(tall)) = doc.document.node(rows[1][0]) else {
        panic!()
    };
    assert_eq!((tall.col_span, tall.row_span), (1, 2));
    assert_eq!(cell_text(&doc, rows[1][2]), "c|c2");
    assert_eq!(cell_text(&doc, rows[2][0]), "r");
    assert_eq!(cell_text(&doc, rows[2][1]), "r");
    // Cell paragraphs are part of the body order, followed by the paragraph after.
    assert_eq!(
        texts(&doc),
        vec!["H", "H3", "tall", "b", "c", "c2", "r", "r", "after"]
    );
}

#[test]
fn ragged_tables_empty_cells_and_nested_tables() {
    let p = Pkg::new(
        r#"<table:table>
             <table:table-row><table:table-cell/><table:table-cell><text:p>x</text:p></table:table-cell><table:table-cell/></table:table-row>
             <table:table-row><table:table-cell table:number-columns-spanned="9" table:number-rows-spanned="9"><table:table><table:table-row><table:table-cell><text:p>inner</text:p></table:table-cell></table:table-row></table:table></table:table-cell></table:table-row>
           </table:table>"#,
    );
    let doc = import(&p);
    let (_, t) = table_of(&doc);
    assert_eq!(t.columns, 3);
    let Some(Node::Row(r1)) = doc.document.node(t.rows[1]) else {
        panic!()
    };
    let Some(Node::Cell(c)) = doc.document.node(r1.cells[0]) else {
        panic!()
    };
    assert_eq!(
        (c.col_span, c.row_span),
        (3, 1),
        "spans are clamped to the grid"
    );
    let inner = c.blocks.to_vec();
    assert!(matches!(doc.document.node(inner[0]), Some(Node::Table(_))));
    assert!(
        matches!(doc.document.node(inner[1]), Some(Node::Paragraph(_))),
        "cell ends with a paragraph"
    );
    assert_eq!(texts(&doc), vec!["x", "inner"]);
}

#[test]
fn table_limits_are_checked_before_allocation() {
    let huge = r#"<table:table><table:table-row table:number-rows-repeated="1000000000"><table:table-cell table:number-columns-repeated="1000000000"/></table:table-row></table:table>"#;
    assert_eq!(
        import_odt(&Pkg::new(huge).build()).err(),
        Some(OdtError::Limit("table cells"))
    );
    let rows = r#"<table:table><table:table-row table:number-rows-repeated="4000000000000"/></table:table>"#;
    assert_eq!(
        import_odt(&Pkg::new(rows).build()).err(),
        Some(OdtError::Limit("table cells"))
    );
}

// ----------------------------------------------------------------- images

const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 0];

#[test]
fn frames_images_and_other_objects() {
    let p = Pkg::new(
        r##"<text:p>See <draw:frame draw:name="logo" text:anchor-type="as-char" svg:width="5.08cm" svg:height="2.54cm"><draw:image xlink:href="Pictures/a.png" xlink:type="simple"/><svg:title>Logo</svg:title><svg:desc>The logo</svg:desc></draw:frame> here<draw:frame text:anchor-type="paragraph" svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/missing.png"/></draw:frame><draw:frame text:anchor-type="page"><draw:image xlink:href="http://example.org/x.png"/></draw:frame><draw:frame text:anchor-type="char"><draw:text-box><text:p>boxed</text:p></draw:text-box></draw:frame><draw:frame text:anchor-type="char"><draw:object-ole xlink:href="Object 1"/><draw:image xlink:href="Pictures/a.png"/></draw:frame><draw:custom-shape/></text:p>"##,
    )
    .file("Pictures/a.png", &PNG);
    let doc = import(&p);
    let para = doc
        .document
        .paragraph(doc.paragraph_ids()[0])
        .unwrap()
        .clone();
    assert_eq!(para.text, "See  here");
    let kinds: Vec<_> = para.anchored.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "image/png",
            "odf:image-reference",
            "odf:image-reference",
            "odf:text-box",
            "odf:object",
            "image/png",
            "odf:shape"
        ]
    );
    assert_eq!(para.anchored[0].data, PNG, "image bytes are kept as is");
    assert_eq!(para.anchored[0].offset, 4);
    let img = &doc.images[0];
    assert_eq!(img.anchor, ImageAnchor::AsChar);
    assert_eq!((img.width, img.height), (Some(2880), Some(1440)));
    assert_eq!(img.title.as_deref(), Some("Logo"));
    assert_eq!(img.description.as_deref(), Some("The logo"));
    assert_eq!(img.name.as_deref(), Some("logo"));
    assert_eq!(img.media_type.as_deref(), Some("image/png"));
    assert_eq!(img.status, ImageStatus::Embedded);
    assert_eq!(doc.images[1].status, ImageStatus::Missing);
    assert_eq!(doc.images[1].anchor, ImageAnchor::Paragraph);
    assert_eq!(doc.images[2].status, ImageStatus::External);
    assert_eq!(doc.images[2].anchor, ImageAnchor::Page);
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::ObjectKept { .. }))
    );
    // The text box payload is preserved verbatim as XML.
    let tb = String::from_utf8(para.anchored[3].data.clone()).unwrap();
    assert!(tb.contains("boxed") && tb.contains("text-box"));
}

#[test]
fn image_budget_is_enforced() {
    let p = Pkg::new(
        r#"<text:p><draw:frame><draw:image xlink:href="Pictures/a.png"/></draw:frame></text:p>"#,
    )
    .file("Pictures/a.png", &PNG);
    let mut limits = OdtLimits::default();
    limits.max_image_bytes = 4;
    let doc = import_odt_with_limits(&p.build(), limits).unwrap();
    assert_eq!(doc.images[0].status, ImageStatus::Missing);
    let mut limits = OdtLimits::default();
    limits.max_images = 0;
    assert_eq!(
        import_odt_with_limits(&p.build(), limits).err(),
        Some(OdtError::Limit("image count"))
    );
}

// --------------------------------------------- page layout, headers, footers

const MASTERS: &str = r#"<style:master-page style:name="Standard" style:page-layout-name="Mpm1">
    <style:header><text:p>Header <text:page-number>1</text:page-number></text:p></style:header>
    <style:header-first><text:p>First</text:p></style:header-first>
    <style:header-left><text:p>Left</text:p></style:header-left>
    <style:footer><text:p>Page <text:page-number text:select-page="current"/> of <text:page-count/></text:p></style:footer>
  </style:master-page>
  <style:master-page style:name="Landscape" style:page-layout-name="Mpm2"/>"#;

const LAYOUTS: &str = r#"<style:page-layout style:name="Mpm1"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" fo:margin-top="2cm" fo:margin-bottom="2cm" fo:margin-left="2.5cm" fo:margin-right="2.5cm"/></style:page-layout>
  <style:page-layout style:name="Mpm2"><style:page-layout-properties fo:page-width="29.7cm" fo:page-height="21cm" style:print-orientation="landscape" fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" fo:margin-right="1cm"/></style:page-layout>"#;

#[test]
fn master_pages_give_sections_headers_footers_and_geometry() {
    let p = Pkg::new(
        r#"<text:p>portrait</text:p><text:p text:style-name="PL">landscape</text:p><text:p>still landscape</text:p>"#,
    )
    .auto(r#"<style:style style:name="PL" style:family="paragraph" style:master-page-name="Landscape"/>"#)
    .styles("", LAYOUTS, MASTERS);
    let doc = import(&p);
    assert_eq!(doc.sections.len(), 2);
    let s0 = &doc.sections[0];
    assert_eq!(s0.master_page.as_deref(), Some("Standard"));
    assert_eq!(s0.page.width, Some(11906));
    assert_eq!(s0.page.height, Some(16838));
    assert_eq!(s0.page.landscape, Some(false));
    assert_eq!(s0.page.margin_left, Some(1417));
    assert_eq!(doc.sections[1].page.landscape, Some(true));
    assert_eq!(doc.sections[1].page.margin_top, Some(567));
    let ids = doc.document.section_ids();
    assert_eq!(ids.len(), 2);
    let Some(Node::Section(sec)) = doc.document.node(ids[0]) else {
        panic!()
    };
    assert_eq!(opaque(&sec.opaque, "page-width").as_deref(), Some("11906"));
    assert_eq!(sec.body.len(), 1);
    let variants: Vec<_> = sec.headers.iter().map(|h| h.0).collect();
    assert_eq!(
        variants,
        vec![HfVariant::Default, HfVariant::First, HfVariant::Even]
    );
    assert_eq!(sec.footers.len(), 1);
    let hf_text = |id: NodeId| -> atelier_text_model::rich::Paragraph {
        let Some(Node::HeaderFooter(h)) = doc.document.node(id) else {
            panic!()
        };
        doc.document
            .paragraph(h.blocks.get(0).unwrap())
            .unwrap()
            .clone()
    };
    let header = hf_text(sec.headers[0].1);
    assert_eq!(header.text, "Header 1");
    assert_eq!(header.fields.len(), 1);
    assert_eq!(header.fields[0].instruction, "PAGE");
    assert_eq!((header.fields[0].start, header.fields[0].end), (7, 8));
    let footer = hf_text(sec.footers[0].1);
    assert_eq!(footer.text, "Page 1 of 1");
    let instr: Vec<_> = footer
        .fields
        .iter()
        .map(|f| f.instruction.as_str())
        .collect();
    assert_eq!(instr, vec!["PAGE", "NUMPAGES"]);
    let Some(Node::Section(sec2)) = doc.document.node(ids[1]) else {
        panic!()
    };
    assert_eq!(sec2.body.len(), 2);
    assert!(sec2.headers.is_empty());
    // Header/footer paragraphs are not part of the body paragraph list.
    assert_eq!(
        texts(&doc),
        vec!["portrait", "landscape", "still landscape"]
    );
}

#[test]
fn first_paragraph_with_master_page_does_not_create_an_empty_section() {
    let p = Pkg::new(r#"<text:p text:style-name="PL">only</text:p>"#)
        .auto(r#"<style:style style:name="PL" style:family="paragraph" style:master-page-name="Landscape"/>"#)
        .styles("", LAYOUTS, MASTERS);
    let doc = import(&p);
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].master_page.as_deref(), Some("Landscape"));
}

// ----------------------------------------------------- links, bookmarks, fields

#[test]
fn links_bookmarks_and_notes() {
    let p = Pkg::new(
        r##"<text:p>go <text:a xlink:href="https://example.org/a?b=1&amp;c=2" office:title="Tip">out</text:a>, <text:a xlink:href="&#35;Target|outline">in</text:a>
           <text:bookmark text:name="Point"/><text:bookmark-start text:name="Range"/>ranged<text:bookmark-end text:name="Range"/>
           <text:bookmark-start text:name="Open"/>x<text:bookmark-end text:name="Never"/>
           <text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>note text</text:p></text:note-body></text:note></text:p>
           <text:p><text:bookmark text:name="Point"/><text:bookmark text:name="Target"/>dup</text:p>"##,
    );
    let doc = import(&p);
    let paras = body_paragraphs(&doc);
    let p0 = &paras[0].1;
    assert_eq!(p0.text, "go out, in ranged x 1");
    assert_eq!(p0.links.len(), 2);
    assert_eq!(
        p0.links[0].target,
        LinkTarget::External("https://example.org/a?b=1&c=2".into())
    );
    assert_eq!(p0.links[0].tooltip.as_deref(), Some("Tip"));
    assert_eq!(&p0.text[p0.links[0].start..p0.links[0].end], "out");
    assert_eq!(p0.links[1].target, LinkTarget::Bookmark("Target".into()));
    let names: Vec<_> = p0
        .bookmarks
        .iter()
        .map(|b| (b.name.as_str(), &p0.text[b.start..b.end]))
        .collect();
    assert!(names.contains(&("Point", "")));
    assert!(names.contains(&("Range", "ranged")));
    assert!(names.contains(&("Open", "x 1")), "{names:?}");
    // The citation is kept as a superscript call; the note body is dropped.
    assert!(!p0.text.contains("note text"));
    let call = p0.runs.last().unwrap();
    assert_eq!(call.attrs.vertical_align, Some(VerticalAlign::Superscript));
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::NoteContentIgnored { id } if id == "ftn1"))
    );
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::DuplicateBookmark { name } if name == "Point"))
    );
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::BookmarkSpansParagraphs { name } if name == "Open"))
    );
    assert_eq!(paras[1].1.bookmarks.len(), 1);
    assert!(doc.document.bookmark_named("Target").is_some());
}

#[test]
fn date_time_and_other_fields() {
    let p = Pkg::new(
        r#"<text:p>d=<text:date style:data-style-name="N1" text:date-value="2026-10-12">12/10/2026</text:date> t=<text:time>10:30</text:time> f=<text:date text:fixed="true">fixed</text:date> a=<text:author-name>Ann</text:author-name> <text:title>Doc</text:title></text:p>"#,
    );
    let doc = import(&p);
    let para = &body_paragraphs(&doc)[0].1;
    assert_eq!(para.text, "d=12/10/2026 t=10:30 f=fixed a=Ann Doc");
    let f: Vec<_> = para
        .fields
        .iter()
        .map(|f| (f.instruction.as_str(), &para.text[f.start..f.end]))
        .collect();
    assert_eq!(f, vec![("DATE", "12/10/2026"), ("TIME", "10:30")]);
    assert!(doc.notices.iter().any(
        |n| matches!(n, ImportNotice::FieldReplacedByResult { name } if name == "author-name")
    ));
}

#[test]
fn tracked_changes_sections_and_indexes_are_flattened() {
    let p = Pkg::new(
        r#"<text:tracked-changes><text:changed-region text:id="c1"><text:deletion><text:p>deleted text</text:p></text:deletion></text:changed-region></text:tracked-changes>
           <text:p>kept <text:change-start text:change-id="c1"/>final<text:change-end text:change-id="c1"/></text:p>
           <text:section text:name="Sect1"><text:p>in section</text:p><text:section text:name="Inner"><text:p>deeper</text:p></text:section></text:section>
           <text:table-of-content><text:index-body><text:index-title><text:p>Contents</text:p></text:index-title><text:p>Entry</text:p></text:index-body></text:table-of-content>"#,
    );
    let doc = import(&p);
    assert_eq!(
        texts(&doc),
        vec!["kept final", "in section", "deeper", "Contents", "Entry"]
    );
    assert!(doc.notices.contains(&ImportNotice::TrackedChangesIgnored));
    assert!(
        doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::FlattenedSection { name } if name == "Sect1"))
    );
}

#[test]
fn metadata_is_read() {
    let p = Pkg::new("<text:p>x</text:p>").meta(
        r#"<meta:generator>LibreOffice/24</meta:generator><dc:title>Titre</dc:title><dc:subject>Sujet</dc:subject><dc:creator>Moi</dc:creator><meta:initial-creator>Lui</meta:initial-creator><dc:description>Desc</dc:description><meta:keyword>a</meta:keyword><meta:keyword>b</meta:keyword><meta:creation-date>2026-01-02T03:04:05</meta:creation-date><dc:date>2026-02-03T04:05:06</dc:date><dc:language>fr-FR</dc:language><meta:document-statistic meta:page-count="3" meta:word-count="42" meta:character-count="300" meta:paragraph-count="7" meta:table-count="1" meta:image-count="2"/>"#,
    );
    let m = import(&p).metadata;
    assert_eq!(m.title.as_deref(), Some("Titre"));
    assert_eq!(m.subject.as_deref(), Some("Sujet"));
    assert_eq!(m.creator.as_deref(), Some("Moi"));
    assert_eq!(m.initial_creator.as_deref(), Some("Lui"));
    assert_eq!(m.description.as_deref(), Some("Desc"));
    assert_eq!(m.keywords, vec!["a", "b"]);
    assert_eq!(m.creation_date.as_deref(), Some("2026-01-02T03:04:05"));
    assert_eq!(m.modification_date.as_deref(), Some("2026-02-03T04:05:06"));
    assert_eq!(m.language.as_deref(), Some("fr-FR"));
    assert_eq!(
        (m.page_count, m.word_count, m.character_count),
        (Some(3), Some(42), Some(300))
    );
    assert_eq!(
        (m.paragraph_count, m.table_count, m.image_count),
        (Some(7), Some(1), Some(2))
    );
}

// ------------------------------------------------------------ flat documents

#[test]
fn flat_document_with_inline_image() {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document {NS} office:mimetype="{TEXT_MIME}"><office:meta><dc:title>Flat</dc:title></office:meta><office:automatic-styles><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles><office:body><office:text><text:p>flat <text:span text:style-name="T1">bold</text:span><draw:frame text:anchor-type="as-char"><draw:image><office:binary-data>iVBORw0KGgoAAAAA</office:binary-data></draw:image></draw:frame></text:p></office:text></office:body></office:document>"#
    );
    let doc = import_odt(xml.as_bytes()).unwrap();
    let para = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(para.text, "flat bold");
    assert_eq!(para.runs[0].attrs.bold, Some(true));
    assert_eq!(para.anchored[0].kind, "image/png");
    assert_eq!(para.anchored[0].data, PNG);
    assert_eq!(doc.images[0].status, ImageStatus::InlineBinary);
    assert_eq!(doc.metadata.title.as_deref(), Some("Flat"));
}

// ------------------------------------------------------------------ limits

#[test]
fn text_and_paragraph_limits() {
    let p = Pkg::new("<text:p>abcdef</text:p><text:p>ghi</text:p>");
    let mut limits = OdtLimits::default();
    limits.max_text_bytes = 8;
    assert_eq!(
        import_odt_with_limits(&p.build(), limits).err(),
        Some(OdtError::Limit("text bytes"))
    );
    let mut limits = OdtLimits::default();
    limits.max_paragraphs = 1;
    assert_eq!(
        import_odt_with_limits(&p.build(), limits).err(),
        Some(OdtError::Limit("paragraph count"))
    );
    // text:s repetition cannot allocate more than the text budget.
    let bomb = Pkg::new(r#"<text:p><text:s text:c="4000000000000"/></text:p>"#);
    assert_eq!(
        import_odt(&bomb.build()).err(),
        Some(OdtError::Limit("text bytes"))
    );
    let mut limits = OdtLimits::default();
    limits.max_styles = 1;
    let p = Pkg::new("<text:p/>").styles(
        r#"<style:style style:name="A" style:family="paragraph"/><style:style style:name="B" style:family="paragraph"/>"#,
        "",
        "",
    );
    assert_eq!(
        import_odt_with_limits(&p.build(), limits).err(),
        Some(OdtError::Limit("style count"))
    );
}

#[test]
fn notice_budget_is_bounded() {
    let mut body = String::new();
    for i in 0..50 {
        body.push_str(&format!("<text:p><text:bogus{i}/></text:p>"));
    }
    // Unknown prefixed elements need a namespace to be well formed.
    let body = body.replace("<text:bogus", "<text:b");
    let mut limits = OdtLimits::default();
    limits.max_notices = 5;
    let doc = import_odt_with_limits(&Pkg::new(&body).build(), limits).unwrap();
    assert_eq!(doc.notices.len(), 6);
    assert_eq!(doc.notices.last(), Some(&ImportNotice::TooManyNotices));
    let _ = NS_IMPORT;
}

#[test]
fn paragraph_properties_misplaced_in_text_properties_are_honoured() {
    // Seen in real templates: fo:line-height / fo:text-align inside text-properties.
    let p = Pkg::new(r#"<text:p text:style-name="Odd">x</text:p>"#).styles(
        r#"<style:style style:name="Odd" style:family="paragraph"><style:text-properties fo:line-height="120%" fo:text-align="center" fo:font-weight="bold"/></style:style>"#,
        "",
        "",
    );
    let mut doc = import(&p);
    let (_, r) = doc.resolved_paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(r.paragraph.alignment, Some(Alignment::Center));
    assert_eq!(r.paragraph.line_spacing.unwrap().value, 288);
    assert_eq!(r.character.bold, Some(true));
    assert!(
        !doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::UnsupportedProperty { .. }))
    );
}
