// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: properties of a file written by LibreOffice (an independent tool).
//!
//! `properties.docx` comes from `properties.fodt` (see the corpus README).
//! The test reads it, edits fields, saves, and writes the result to
//! `$CARGO_TARGET_TMPDIR/us328-properties-modified.docx`;
//! `tools/check-docx-properties-libreoffice.py` then makes LibreOffice read
//! that file back and compares the metadata it sees.
use atelier_fmt_ooxml::{
    DocumentStatistics, Package, PartUri, PropertiesSnapshot, PropertyChanges, PropertyDate,
    SaveContext, W3cDateTime,
};

const SOURCE: &[u8] = include_bytes!("../../../tests/docx-reference/libreoffice/properties.docx");

pub const NEW_TITLE: &str =
    "Titre modifi\u{e9} \u{2014} \u{ab} \u{e9}t\u{e9} \u{bb} & <b>\"x\"</b>";
pub const NEW_COMPANY: &str = "Soci\u{e9}t\u{e9} \u{152}uvre & Fils";

fn core(p: &Package) -> String {
    String::from_utf8(
        p.part(&PartUri::parse("docProps/core.xml").unwrap())
            .unwrap()
            .bytes
            .clone(),
    )
    .unwrap()
}

#[test]
fn reads_libreoffice_properties_with_accents() {
    let s: PropertiesSnapshot = Package::read(SOURCE).unwrap().properties_snapshot();
    assert_eq!(s.editable.title, "Rapport & bilan : \u{e9}t\u{e9} <2026>");
    assert_eq!(
        s.editable.subject,
        "Sujet \u{ab} sp\u{e9}cial \u{bb} \u{2014} \u{153}uvre"
    );
    assert_eq!(s.editable.author, "\u{c9}lodie Dupont");
    assert_eq!(s.editable.keywords, "caf\u{e9} na\u{ef}f R&D");
    assert_eq!(
        s.editable.comments,
        "Premi\u{e8}re ligne avec accents : \u{e0}\u{e9}\u{ee}\u{f5}\u{fc}, \u{e7}, \u{153}.\nDeuxi\u{e8}me ligne : \"guillemets\" & <balises>."
    );
    assert_eq!(s.editable.language, "fr-FR");
    assert_eq!(
        s.read_only.created,
        Some(PropertyDate::Valid(
            W3cDateTime::parse("2026-03-04T05:06:07Z").unwrap()
        ))
    );
    assert_eq!(s.read_only.revision.as_deref(), Some("0"));
    assert_eq!(s.editable.company, ""); // `<Template></Template>`-style empty elements read as absent
    assert!(
        s.read_only
            .application
            .as_deref()
            .unwrap()
            .starts_with("LibreOffice/")
    );
    assert!(s.notices.is_empty(), "{:?}", s.notices);
}

#[test]
fn unmodified_libreoffice_file_keeps_its_property_parts_byte_for_byte() {
    let original = Package::read(SOURCE).unwrap();
    let mut p = original.clone();
    let _ = p.core_properties_mut();
    let _ = p.app_properties_mut();
    let out = Package::read(&p.write().unwrap()).unwrap();
    assert_eq!(core(&out), core(&original));
    let app = PartUri::parse("docProps/app.xml").unwrap();
    assert_eq!(
        out.part(&app).unwrap().bytes,
        original.part(&app).unwrap().bytes
    );
}

#[test]
fn edits_and_saves_for_libreoffice_to_read_back() {
    let original = Package::read(SOURCE).unwrap();
    let mut p = original.clone();
    p.apply_property_changes(&PropertyChanges {
        title: Some(NEW_TITLE.into()),
        company: Some(NEW_COMPANY.into()),
        ..Default::default()
    })
    .unwrap();
    // Exactly one core element changes before the save update.
    let old_title = "<dc:title>Rapport &amp; bilan : \u{e9}t\u{e9} &lt;2026&gt;</dc:title>";
    let new_title = format!(
        "<dc:title>{}</dc:title>",
        NEW_TITLE
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    );
    let mid = Package::read(&p.write().unwrap()).unwrap();
    assert_eq!(core(&mid), core(&original).replace(old_title, &new_title));

    let r = p.update_on_save(&SaveContext {
        now: W3cDateTime::utc(2026, 10, 13, 12, 0, 0).unwrap(),
        user: Some("Zo\u{eb} Martin".into()),
        application: Some("Atelier".into()),
        app_version: Some("0.1".into()),
        statistics: Some(DocumentStatistics {
            words: Some(9),
            ..Default::default()
        }),
    });
    assert_eq!(r.revision, "1"); // "0" + 1
    let bytes = p.write().unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(dir.join("us328-properties-modified.docx"), &bytes).unwrap();
    let s = Package::read(&bytes).unwrap().properties_snapshot();
    assert_eq!(s.editable.title, NEW_TITLE);
    assert_eq!(
        s.read_only.last_modified_by.as_deref(),
        Some("Zo\u{eb} Martin")
    );
    assert_eq!(s.editable.author, "\u{c9}lodie Dupont"); // creator kept
}

/// The same document without its property parts: they are created on save.
#[test]
fn creates_property_parts_for_libreoffice_to_read_back() {
    let source = Package::read(SOURCE).unwrap();
    let mut p = Package::new();
    for part in source.parts().values() {
        if !part.uri.as_str().starts_with("docProps/") {
            p.insert_part(
                part.uri.clone(),
                part.content_type.clone(),
                part.bytes.clone(),
            )
            .unwrap();
        }
    }
    let mut sources: Vec<Option<PartUri>> = vec![None];
    sources.extend(source.parts().keys().cloned().map(Some));
    for src in sources {
        for r in source.relationships(src.as_ref()) {
            if !r.target.starts_with("docProps/") {
                p.add_relationship(
                    src.clone(),
                    r.id.clone(),
                    r.rel_type.clone(),
                    r.target.clone(),
                    r.mode,
                )
                .unwrap();
            }
        }
    }
    let empty = Package::read(&p.write().unwrap()).unwrap();
    assert_eq!(empty.properties_snapshot().editable.title, "");
    p.apply_property_changes(&PropertyChanges {
        title: Some(NEW_TITLE.into()),
        subject: Some("Cr\u{e9}\u{e9} \u{e0} l'enregistrement".into()),
        keywords: Some("un; deux".into()),
        comments: Some("Ligne 1\nLigne 2 & \u{e9}".into()),
        ..Default::default()
    })
    .unwrap();
    p.update_on_save(&SaveContext {
        now: W3cDateTime::utc(2026, 10, 13, 12, 0, 0).unwrap(),
        user: Some("Zo\u{eb} Martin".into()),
        application: Some("Atelier".into()),
        app_version: Some("0.1".into()),
        statistics: None,
    });
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(
        dir.join("us328-properties-created.docx"),
        p.write().unwrap(),
    )
    .unwrap();
}
