// SPDX-License-Identifier: AGPL-3.0-or-later
//! Integration against a DOCX made by Python's ZIP/XML libraries, not our writer.

use atelier_fmt_ooxml::{
    DocxError, DocxLimits, Package, PartUri, TargetMode, import_docx, import_docx_with_limits,
};
use atelier_font::embedded::EMBEDDED_FONTS;
use atelier_font::manager::{FontManager, FontRequest, FontSource, FontStyle};
use atelier_text_layout::{LayoutOptions, layout_paragraph};
use atelier_text_model::rich::Color;
use atelier_unicode::segmentation::grapheme_boundaries;

const DOCX: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");
const TITLE: &str = "Référence de titre — Français et English mix: cœurs, accents, ligatures.";
const BODY: &str = "This second English paragraph is deliberately long so a layout engine must wrap it across multiple visual lines. It includes punctuation, varied word lengths, and enough ordinary text to make the fixture useful without depending on any renderer or platform-specific font metrics.";

#[test]
fn independently_packaged_docx_resolves_styles_and_embedded_font_into_lines() {
    let mut imported = import_docx(DOCX).expect("independent OOXML fixture imports");
    imported.document.validate().unwrap();
    let ids = imported.paragraph_ids().to_vec();
    assert_eq!(ids.len(), 2);
    assert_eq!(imported.document.paragraph(ids[0]).unwrap().text, TITLE);
    assert_eq!(imported.document.paragraph(ids[1]).unwrap().text, BODY);
    assert_eq!(imported.settings.default_tab_stop_twips, Some(720));
    assert_eq!(
        imported
            .settings
            .compatibility
            .get("http://schemas.microsoft.com/office/word#compatibilityMode"),
        Some(&"15".to_owned())
    );
    assert!(
        imported
            .latent_styles
            .iter()
            .any(|style| style.name.is_some())
    );
    assert!(
        imported
            .font_records
            .iter()
            .any(|font| font.name == "Carlito")
    );

    let (title, title_defaults) = imported.resolved_paragraph(ids[0]).unwrap();
    let (body, body_defaults) = imported.resolved_paragraph(ids[1]).unwrap();
    assert_eq!(title_defaults.character.font.as_deref(), Some("Carlito"));
    assert_eq!(title_defaults.character.size_half_points, Some(32));
    assert_eq!(
        title.attrs_at(0).bold,
        Some(false),
        "direct false overrides Strong"
    );
    assert_eq!(
        title.attrs_at(0).color,
        Some(Color {
            r: 0x44,
            g: 0x72,
            b: 0xc4
        })
    );
    assert_eq!(body_defaults.character.size_half_points, Some(22));
    assert_eq!(body_defaults.character.font.as_deref(), Some("Carlito"));

    let mut fonts = FontManager::new();
    imported.register_fonts(&mut fonts).unwrap();
    let face = fonts
        .resolve(&FontRequest::new(["Carlito"], 400, FontStyle::Normal))
        .unwrap();
    assert_eq!(face.source, FontSource::Document);

    let title_layout = layout_paragraph(
        &title,
        &title_defaults,
        &fonts,
        LayoutOptions::new(400 * 64),
    )
    .unwrap();
    let body_layout =
        layout_paragraph(&body, &body_defaults, &fonts, LayoutOptions::new(200 * 64)).unwrap();
    assert!(body_layout.lines.len() > 2);
    for (paragraph, layout, size, available) in [
        (&title, &title_layout, 16 * 64, 400 * 64),
        (&body, &body_layout, 11 * 64, 200 * 64),
    ] {
        let boundaries = grapheme_boundaries(&paragraph.text);
        assert_eq!(layout.lines.first().unwrap().source.start, 0);
        assert_eq!(
            layout.lines.last().unwrap().source.end,
            paragraph.text.len()
        );
        assert!(layout.lines.last().unwrap().final_line);
        for line in &layout.lines {
            assert!(line.width <= available);
            assert!(line.height > 0 && line.baseline > 0);
            assert!(boundaries.is_boundary(line.source.start).unwrap());
            assert!(boundaries.is_boundary(line.source.end).unwrap());
            for glyph in line.fragments.iter().flat_map(|fragment| &fragment.glyphs) {
                assert_ne!(glyph.glyph_id, 0);
                assert_eq!(glyph.family, "Carlito");
                assert_eq!(glyph.size, size);
                assert_eq!(glyph.font_id, face.id);
                assert!(
                    glyph.source.start >= line.source.start && glyph.source.end <= line.source.end
                );
            }
        }
        for pair in layout.lines.windows(2) {
            assert_eq!(pair[0].source.end, pair[1].source.start);
            assert!(pair[1].baseline > pair[0].baseline);
        }
    }
}

#[test]
fn odttf_recovers_exact_source_bytes_and_original_unknown_part_survives() {
    let imported = import_docx(DOCX).unwrap();
    let original = EMBEDDED_FONTS
        .iter()
        .find(|font| {
            font.family == "Carlito"
                && font.weight == 400
                && matches!(
                    font.style,
                    atelier_font::embedded::EmbeddedFontStyle::Normal
                )
        })
        .unwrap();
    assert_eq!(imported.font_bytes.len(), 1);
    assert_eq!(imported.font_bytes[0], original.data);
    let obfuscated = &imported
        .package
        .part(&PartUri::parse("content/fonts/Carlito-Regular.odttf").unwrap())
        .unwrap()
        .bytes;
    assert_ne!(&obfuscated[..32], &original.data[..32]);
    assert_eq!(&obfuscated[32..], &original.data[32..]);
    // Fixed vector from independent Python construction using a nontrivial GUID.
    assert_eq!(
        &obfuscated[..8],
        &[0xff, 0xef, 0xdd, 0xcc, 0xbb, 0xbb, 0x98, 0x88]
    );
    let uri = PartUri::parse("vendor/preserved-opaque.bin").unwrap();
    let expected = b"US301-opaque-fixture\x00\xff\x10preserve-me\n";
    assert_eq!(imported.package.part(&uri).unwrap().bytes, expected);
    let reopened = Package::read(&imported.package.write().unwrap()).unwrap();
    assert_eq!(reopened.part(&uri).unwrap().bytes, expected);
}

#[test]
fn docx_font_budget_rejects_the_independently_generated_embedded_font() {
    let limits = DocxLimits {
        max_font_bytes: 31,
        ..DocxLimits::default()
    };
    assert!(matches!(
        import_docx_with_limits(DOCX, limits),
        Err(DocxError::Limit(_))
    ));
}

fn minimal_docx(xml: &str, relationship: Option<&str>) -> Vec<u8> {
    let mut package = Package::new();
    package
        .insert_part(
            PartUri::parse("content/main.xml").unwrap(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            xml.as_bytes().to_vec(),
        )
        .unwrap();
    if let Some(kind) = relationship {
        package
            .add_relationship(None, "main", kind, "content/main.xml", TargetMode::Internal)
            .unwrap();
    }
    package.write().unwrap()
}

#[test]
fn malformed_indents_never_panic_or_silently_become_default_properties() {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    for property in [
        r#"<w:ind w:hanging="-2147483648"/>"#,
        r#"<w:spacing w:line="not-a-number"/>"#,
        r#"<w:ind w:firstLine="2147483648"/>"#,
    ] {
        let xml = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:pPr>{property}</w:pPr><w:r><w:t>text</w:t></w:r></w:p></w:body></w:document>"#
        );
        let bytes = minimal_docx(&xml, Some(REL));
        let result = std::panic::catch_unwind(|| import_docx(&bytes));
        assert!(
            result.is_ok(),
            "untrusted numeric property panicked: {property}"
        );
        assert!(
            result.unwrap().is_err(),
            "invalid numeric property was silently ignored: {property}"
        );
    }
}

#[test]
fn main_document_requires_authentic_relationship_and_root_namespace() {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    let valid = format!(r#"<w:document xmlns:w="{W}"><w:body><w:p/></w:body></w:document>"#);
    for kind in [None, Some("urn:fake/officeDocument")] {
        assert!(
            import_docx(&minimal_docx(&valid, kind)).is_err(),
            "a MIME type or suffix is not an officeDocument relationship"
        );
    }
    let foreign = format!(
        r#"<f:document xmlns:f="urn:fake" xmlns:w="{W}"><w:body><w:p/></w:body></f:document>"#
    );
    assert!(import_docx(&minimal_docx(&foreign, Some(REL))).is_err());
}
