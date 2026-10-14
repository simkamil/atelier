// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-307: variants (.dotx, .docm), macros, properties update, determinism.
mod support;

use atelier_fmt_ooxml::{
    DocxVariant, DocxWriteOptions, NewDocument, Package, PartUri, SaveContext, TargetMode,
    W3cDateTime, WriteNotice, import_docx, write_docx, write_new_docx,
};
use atelier_text_model::rich::default_rich_document;
use support::*;

const DOCM: &str = "application/vnd.ms-word.document.macroEnabled.main+xml";
const VBA_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

fn docm_bytes() -> Vec<u8> {
    let mut d = rich();
    d.extra.push((
        "word/vbaProject.bin",
        "application/vnd.ms-office.vbaProject",
        vec![0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3],
        "vbaProject",
    ));
    let bytes = d.bytes();
    // Make it a macro-enabled document with the Microsoft relationship type.
    let mut p = Package::read(&bytes).unwrap();
    let main = PartUri::parse("word/document.xml").unwrap();
    let doc = p.part(&main).unwrap().bytes.clone();
    let mut q = Package::new();
    for (uri, part) in p.parts() {
        let ct = if *uri == main {
            DOCM
        } else {
            &part.content_type
        };
        let data = if *uri == main {
            doc.clone()
        } else {
            part.bytes.clone()
        };
        q.insert_part(uri.clone(), ct, data).unwrap();
    }
    for source in [None, Some(main.clone())] {
        for r in p.relationships(source.as_ref()) {
            let ty = if r.target.ends_with("vbaProject.bin") {
                VBA_REL
            } else {
                &r.rel_type
            };
            q.add_relationship(source.clone(), &r.id, ty, &r.target, TargetMode::Internal)
                .unwrap();
        }
    }
    p = q;
    p.write().unwrap()
}

#[test]
fn docm_keeps_its_macros_and_dotx_changes_only_the_content_type() {
    let bytes = docm_bytes();
    let imp = import_docx(&bytes).unwrap();
    let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
    let out = Package::read(&w.bytes).unwrap();
    let vba = PartUri::parse("word/vbaProject.bin").unwrap();
    assert_eq!(
        out.part(&vba).unwrap().bytes,
        vec![0xD0, 0xCF, 0x11, 0xE0, 1, 2, 3]
    );
    let main = PartUri::parse("word/document.xml").unwrap();
    assert_eq!(out.part(&main).unwrap().content_type, DOCM);
    assert!(
        out.relationships(Some(&main))
            .iter()
            .any(|r| r.rel_type == VBA_REL)
    );
    assert!(w.notices.is_empty(), "{:?}", w.notices);

    // docm -> docx: macro parts and relationships go, with a notice.
    let opts = DocxWriteOptions {
        variant: Some(DocxVariant::Document),
        ..DocxWriteOptions::default()
    };
    let w = write_docx(&imp, &opts).unwrap();
    let out = Package::read(&w.bytes).unwrap();
    assert!(out.part(&vba).is_none());
    assert!(
        !out.relationships(Some(&main))
            .iter()
            .any(|r| r.rel_type == VBA_REL)
    );
    assert_eq!(
        out.part(&main).unwrap().content_type,
        DocxVariant::Document.content_type()
    );
    assert!(
        w.notices
            .iter()
            .any(|n| matches!(n, WriteNotice::MacrosRemoved { .. }))
    );
    import_docx(&w.bytes).unwrap();

    // docx -> dotx -> reimport.
    let opts = DocxWriteOptions {
        variant: Some(DocxVariant::Template),
        ..DocxWriteOptions::default()
    };
    let w = write_docx(&rich().import(), &opts).unwrap();
    let out = Package::read(&w.bytes).unwrap();
    assert_eq!(
        out.part(&main).unwrap().content_type,
        DocxVariant::Template.content_type()
    );
    assert_eq!(DocxVariant::Template.extension(), "dotx");
    import_docx(&w.bytes).unwrap();
}

#[test]
fn new_documents_in_every_variant_have_the_minimal_complete_package() {
    let d = default_rich_document();
    for variant in [
        DocxVariant::Document,
        DocxVariant::Template,
        DocxVariant::MacroDocument,
        DocxVariant::MacroTemplate,
    ] {
        let save = SaveContext {
            now: W3cDateTime::utc(2026, 10, 14, 8, 0, 0).unwrap(),
            user: Some("Zoé".into()),
            application: Some("Atelier".into()),
            app_version: Some("1".into()),
            statistics: None,
        };
        let opts = DocxWriteOptions {
            variant: Some(variant),
            save: Some(save),
            ..DocxWriteOptions::default()
        };
        let a = write_new_docx(&NewDocument::from_default(&d), &opts).unwrap();
        let b = write_new_docx(&NewDocument::from_default(&d), &opts).unwrap();
        assert_eq!(a.bytes, b.bytes, "deterministic");
        let p = Package::read(&a.bytes).unwrap();
        let names: Vec<_> = p.parts().keys().map(ToString::to_string).collect();
        for want in [
            "word/document.xml",
            "word/styles.xml",
            "word/settings.xml",
            "word/fontTable.xml",
            "docProps/core.xml",
            "docProps/app.xml",
        ] {
            assert!(
                names.iter().any(|n| n.trim_start_matches('/') == want),
                "{want} in {names:?}"
            );
        }
        let main = PartUri::parse("word/document.xml").unwrap();
        assert_eq!(p.part(&main).unwrap().content_type, variant.content_type());
        assert_eq!(p.core_properties().creator.as_deref(), Some("Zoé"));
        import_docx(&a.bytes).unwrap();
    }
}

#[test]
fn hyperlink_relationships_are_written_and_untouched_documents_reuse_parts() {
    let built = scratch_for_links();
    let w = write_new_docx(
        &NewDocument::from_default(&built),
        &DocxWriteOptions::default(),
    )
    .unwrap();
    let p = Package::read(&w.bytes).unwrap();
    let main = PartUri::parse("word/document.xml").unwrap();
    assert!(
        p.relationships(Some(&main))
            .iter()
            .any(|r| r.mode == TargetMode::External && r.target.contains("example.org"))
    );
    // An imported document, written without change, reuses every part.
    let imp = rich().import();
    let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
    assert!(w.regenerated_parts.is_empty());
    assert_eq!(w.reused_parts.len(), 3);
}

fn scratch_for_links() -> atelier_text_model::rich::DefaultRichDocument {
    scratch::scratch_document(0).doc
}
