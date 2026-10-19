// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-250-M1: the display name, the quick-gallery flag, the default style and
//! the `uiPriority` of the styles are read from `word/styles.xml` and written
//! back; a document of the default profile keeps them through a save.

use atelier_fmt_ooxml::{
    DocxWriteOptions, NewDocument, Package, PartUri, import_docx, write_new_docx,
};
use atelier_text_model::rich::{StyleKind, default_rich_document, is_builtin_style_name};

fn styles_xml(bytes: &[u8]) -> String {
    let package = Package::read(bytes).unwrap();
    let part = package
        .part(&PartUri::parse("word/styles.xml").unwrap())
        .unwrap();
    String::from_utf8(part.bytes.clone()).unwrap()
}

#[test]
fn the_default_profile_keeps_names_flags_and_priorities_through_a_docx() {
    let profile = default_rich_document();
    let written = write_new_docx(
        &NewDocument::from_default(&profile),
        &DocxWriteOptions::default(),
    )
    .unwrap();
    let xml = styles_xml(&written.bytes);
    assert!(xml.contains(r#"<w:name w:val="heading 1"/>"#), "{xml}");
    assert!(xml.contains(r#"w:default="1""#));
    assert!(xml.contains(r#"<w:uiPriority w:val="9"/>"#));

    let imported = import_docx(&written.bytes).unwrap();
    let catalog = imported.styles.catalog();
    let heading = catalog.style("atelier.heading.1").unwrap();
    assert_eq!(heading.name.as_deref(), Some("heading 1"));
    assert!(heading.quick && !heading.is_default);
    assert_eq!(heading.ui_priority, Some(9));
    assert_eq!(heading.heading_level(), 1);
    assert!(heading.is_builtin());
    let normal = catalog.style("atelier.normal").unwrap();
    assert!(normal.is_default && normal.quick);
    assert_eq!(normal.name.as_deref(), Some("Normal"));
    // The heading 4 is not in the quick gallery, the table style is no paragraph style.
    let four = catalog.style("atelier.heading.4").unwrap();
    assert!(!four.quick);
    assert_eq!(four.heading_level(), 4);
    assert_eq!(
        catalog.default_style(StyleKind::Paragraph).unwrap().id,
        "atelier.normal"
    );
    // The flags the profile declares are the ones read back, style by style.
    for style in profile.styles.catalog().styles() {
        let back = catalog.style(&style.id).unwrap();
        assert_eq!(
            (back.quick, back.is_default, back.ui_priority),
            (style.quick, style.is_default, style.ui_priority),
            "{}",
            style.id
        );
    }
}

#[test]
fn a_style_without_qformat_is_not_quick_and_a_foreign_name_is_kept() {
    let profile = default_rich_document();
    let written = write_new_docx(
        &NewDocument::from_default(&profile),
        &DocxWriteOptions::default(),
    )
    .unwrap();
    // A style of another producer: Word's own name, a custom one, no qFormat.
    let mut package = Package::read(&written.bytes).unwrap();
    let uri = PartUri::parse("word/styles.xml").unwrap();
    let xml = styles_xml(&written.bytes).replace(
        "</w:styles>",
        r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="atelier.normal"/><w:uiPriority w:val="9"/><w:qFormat/></w:style><w:style w:type="paragraph" w:customStyle="1" w:styleId="Mine"><w:name w:val="Mon style"/><w:basedOn w:val="atelier.normal"/></w:style></w:styles>"#,
    );
    assert!(package.replace_part_bytes(&uri, xml.into_bytes()));
    let imported = import_docx(&package.write().unwrap()).unwrap();
    let catalog = imported.styles.catalog();
    let word = catalog.style("Heading1").unwrap();
    assert!(word.quick && word.is_builtin());
    assert_eq!(word.heading_level(), 1);
    let mine = catalog.style("Mine").unwrap();
    assert_eq!(mine.name.as_deref(), Some("Mon style"));
    assert!(!mine.quick && !mine.is_builtin() && mine.heading_level() == 0);
    assert!(is_builtin_style_name("Heading 3") && !is_builtin_style_name("Mon style"));
}
