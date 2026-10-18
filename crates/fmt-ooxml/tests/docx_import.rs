// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_fmt_ooxml::{
    DocxLimits, Package, PartUri, TargetMode, import_docx, import_docx_with_limits,
};
use atelier_font::manager::{FontManager, FontRequest, FontStyle};
use atelier_text_model::rich::BreakKind;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAIN_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

fn package(document: &[u8], extra: impl IntoIterator<Item = (String, String, Vec<u8>)>) -> Vec<u8> {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        document.to_vec(),
    )
    .unwrap();
    p.add_relationship(
        None,
        "rDoc",
        MAIN_REL,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    for (uri, kind, bytes) in extra {
        p.insert_part(PartUri::parse(&uri).unwrap(), kind, bytes)
            .unwrap();
    }
    p.write().unwrap()
}

#[test]
fn independent_reference_imports_styles_settings_font_and_preserves_unknown_bytes() {
    let bytes = include_bytes!("../../../tests/docx-reference/basic.docx");
    let mut imported = import_docx(bytes).unwrap();
    assert_eq!(imported.paragraph_ids().len(), 2);
    let first = imported
        .document
        .paragraph(imported.paragraph_ids()[0])
        .unwrap();
    assert_eq!(
        first.text,
        "Référence de titre — Français et English mix: cœurs, accents, ligatures."
    );
    assert_eq!(first.props.style.as_deref(), Some("Heading1"));
    let (resolved, _properties) = imported
        .resolved_paragraph(imported.paragraph_ids()[0])
        .unwrap();
    assert_eq!(
        imported
            .theme
            .fonts
            .get(&atelier_text_model::rich::ThemeFont::Minor)
            .map(String::as_str),
        Some("Carlito")
    );
    assert_eq!(
        imported
            .theme
            .colors
            .get(&atelier_text_model::rich::ThemeColor::Accent1),
        Some(&atelier_text_model::rich::Color {
            r: 0x44,
            g: 0x72,
            b: 0xc4
        })
    );
    assert_eq!(_properties.character.size_half_points, Some(32));
    assert_eq!(_properties.character.font.as_deref(), Some("Carlito"));
    assert_eq!(
        _properties.character.color,
        Some(atelier_text_model::rich::Color {
            r: 0x44,
            g: 0x72,
            b: 0xc4
        })
    );
    assert!(resolved.runs.iter().any(|r| r.attrs.bold == Some(false)));
    assert_eq!(imported.settings.default_tab_stop_twips, Some(720));
    assert!(!imported.settings.compatibility.is_empty());
    assert_eq!(
        imported
            .latent_styles
            .iter()
            .filter(|s| s.name.is_some())
            .count(),
        1
    );
    assert_eq!(imported.settings.latent_style_count, Some(276));
    assert_eq!(imported.font_records[0].name, "Carlito");
    assert_eq!(
        imported.font_bytes[0].as_slice(),
        include_bytes!("../../../data/fonts/carlito/Carlito-Regular.ttf")
    );
    let opaque = imported
        .package
        .part(&PartUri::parse("vendor/preserved-opaque.bin").unwrap())
        .unwrap();
    assert_eq!(opaque.bytes, b"US301-opaque-fixture\0\xff\x10preserve-me\n");
    let mut manager = FontManager::empty();
    imported.register_fonts(&mut manager).unwrap();
    assert_eq!(
        manager
            .resolve(&FontRequest::new(["Carlito"], 400, FontStyle::Normal))
            .unwrap()
            .family,
        "Carlito"
    );
}

#[test]
fn text_controls_false_overrides_and_prefixes_keep_utf8_offsets_and_break_types() {
    let xml = format!(
        r#"<x:document xmlns:x="{W}"><x:body><x:p><x:r><x:rPr><x:b x:val="0"/></x:rPr><x:t xml:space="preserve">A e</x:t></x:r><x:r><x:t>́</x:t><x:tab/><x:br x:type="page"/><x:sym x:font="Symbol" x:char="03A9"/></x:r></x:p></x:body></x:document>"#
    );
    let bytes = package(xml.as_bytes(), []);
    let mut imported = import_docx(&bytes).unwrap();
    let id = imported.paragraph_ids()[0];
    let p = imported.document.paragraph(id).unwrap();
    // The break and the symbol are inline items of the model, not text.
    assert_eq!(p.text, "A é\t");
    assert!(p.text.is_char_boundary(p.text.len()));
    assert_eq!(p.breaks.len(), 1);
    assert_eq!(p.breaks[0].kind, BreakKind::Page);
    assert_eq!(p.breaks[0].offset, p.text.len());
    assert_eq!(p.symbols.len(), 1);
    assert_eq!(
        (
            p.symbols[0].font.as_str(),
            p.symbols[0].code,
            p.symbols[0].offset
        ),
        ("Symbol", 0x03a9, p.text.len())
    );
    assert!(p.breaks[0].id < p.symbols[0].id);
    assert_eq!(
        atelier_text_model::rich::flat::paragraph_flat(p)
            .chars()
            .filter(|c| *c == '\u{fffc}')
            .count(),
        2
    );
    // The annex tables remain, in the offsets of the model text.
    assert_eq!(imported.breaks[0].offset, p.text.len());
    assert_eq!(imported.breaks[0].break_type.as_deref(), Some("page"));
    assert_eq!(imported.symbols[0].font, "Symbol");
    assert_eq!(imported.symbols[0].character_code, "03A9");
    let (resolved, _) = imported.resolved_paragraph(id).unwrap();
    assert!(resolved.runs.iter().any(|r| r.attrs.bold == Some(false)));
}

#[test]
fn automatic_text_color_and_named_highlight_are_supported_without_loss() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:color w:val="auto"/><w:highlight w:val="yellow"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    let imported = import_docx(&package(xml.as_bytes(), [])).unwrap();
    let id = imported.paragraph_ids()[0];
    let p = imported.document.paragraph(id).unwrap();
    assert_eq!(
        p.runs[0].attrs.highlight,
        Some(atelier_text_model::rich::Color {
            r: 255,
            g: 255,
            b: 0
        })
    );
    assert!(
        p.runs[0]
            .attrs
            .opaque
            .iter()
            .any(|a| a.name == "color" && a.value == b"auto")
    );
}

#[test]
fn standard_theme_color_names_resolve_in_runs_and_styles() {
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let aliases = [
        ("text1", "dk1", "#110000"),
        ("background1", "lt1", "#002200"),
        ("text2", "dk2", "#000033"),
        ("background2", "lt2", "#440044"),
        ("hyperlink", "hlink", "#005555"),
        ("followedHyperlink", "folHlink", "#666600"),
    ];
    let document = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:color w:themeColor="text1"/></w:rPr><w:t>a</w:t></w:r><w:r><w:rPr><w:color w:themeColor="background1"/></w:rPr><w:t>b</w:t></w:r><w:r><w:rPr><w:color w:themeColor="text2"/></w:rPr><w:t>c</w:t></w:r><w:r><w:rPr><w:color w:themeColor="background2"/></w:rPr><w:t>d</w:t></w:r><w:r><w:rPr><w:color w:themeColor="hyperlink"/></w:rPr><w:t>e</w:t></w:r><w:r><w:rPr><w:color w:themeColor="followedHyperlink"/></w:rPr><w:t>f</w:t></w:r></w:p><w:p><w:r><w:t>default</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style1"/></w:pPr><w:r><w:t>style1</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style2"/></w:pPr><w:r><w:t>style2</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style3"/></w:pPr><w:r><w:t>style3</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style4"/></w:pPr><w:r><w:t>style4</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style5"/></w:pPr><w:r><w:t>style5</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Style6"/></w:pPr><w:r><w:t>style6</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Inherited"/></w:pPr><w:r><w:t>inherited</w:t></w:r></w:p></w:body></w:document>"#
    );
    let styles = format!(
        r#"<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:color w:themeColor="text1"/></w:rPr></w:rPrDefault></w:docDefaults>{}<w:style w:type="paragraph" w:styleId="Inherited"><w:basedOn w:val="Style1"/></w:style></w:styles>"#,
        aliases
            .iter()
            .enumerate()
            .map(|(i, (alias, _, _))| format!(r#"<w:style w:type="paragraph" w:styleId="Style{}"><w:rPr><w:color w:themeColor="{}"/></w:rPr></w:style>"#, i + 1, alias))
            .collect::<String>()
    );
    let colors = aliases
        .iter()
        .map(|(_, slot, color)| {
            let hex = color.trim_start_matches('#');
            format!(r#"<a:{slot}><a:srgbClr val="{hex}"/></a:{slot}>"#)
        })
        .collect::<String>();
    let theme = format!(
        r#"<a:theme xmlns:a="{A}"><a:themeElements><a:clrScheme name="Synthetic">{colors}</a:clrScheme></a:themeElements></a:theme>"#
    );

    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        document.into_bytes(),
    )
    .unwrap();
    p.insert_part(
        PartUri::parse("word/styles.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        styles.into_bytes(),
    )
    .unwrap();
    p.insert_part(
        PartUri::parse("word/theme/theme1.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.theme+xml",
        theme.clone().into_bytes(),
    )
    .unwrap();
    p.add_relationship(
        None,
        "rDoc",
        MAIN_REL,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/document.xml").unwrap()),
        "rStyles",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
        "styles.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/document.xml").unwrap()),
        "rTheme",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
        "theme/theme1.xml",
        TargetMode::Internal,
    )
    .unwrap();

    let mut imported = import_docx(&p.write().unwrap()).unwrap();
    let expected = [
        (0, 0, 0),
        (0, 1, 1),
        (0, 2, 2),
        (0, 3, 3),
        (0, 4, 4),
        (0, 5, 5),
        (1, 0, 0),
        (2, 0, 0),
        (3, 0, 1),
        (4, 0, 2),
        (5, 0, 3),
        (6, 0, 4),
        (7, 0, 5),
        (8, 0, 0),
    ];
    for (paragraph_index, run_index, alias_index) in expected {
        let id = imported.paragraph_ids()[paragraph_index];
        let (resolved, _) = imported.resolved_paragraph(id).unwrap();
        let color = resolved.runs[run_index].attrs.color.unwrap();
        let hex = aliases[alias_index].2.trim_start_matches('#');
        assert_eq!(
            color,
            atelier_text_model::rich::Color {
                r: u8::from_str_radix(&hex[0..2], 16).unwrap(),
                g: u8::from_str_radix(&hex[2..4], 16).unwrap(),
                b: u8::from_str_radix(&hex[4..6], 16).unwrap(),
            },
            "paragraph {paragraph_index}, alias {}",
            aliases[alias_index].0
        );
    }

    // Existing compact names still resolve to the same six distinct slots.
    let legacy = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p>{}</w:p></w:body></w:document>"#,
        ["tx1", "bg1", "tx2", "bg2", "hlink", "folHlink"]
            .iter()
            .map(|name| format!(
                r#"<w:r><w:rPr><w:color w:themeColor="{name}"/></w:rPr><w:t>x</w:t></w:r>"#
            ))
            .collect::<String>()
    );
    let mut legacy_package = Package::new();
    legacy_package
        .insert_part(
            PartUri::parse("word/document.xml").unwrap(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            legacy.into_bytes(),
        )
        .unwrap();
    legacy_package
        .insert_part(
            PartUri::parse("word/theme/theme1.xml").unwrap(),
            "application/vnd.openxmlformats-officedocument.theme+xml",
            theme.into_bytes(),
        )
        .unwrap();
    legacy_package
        .add_relationship(
            None,
            "rDoc",
            MAIN_REL,
            "word/document.xml",
            TargetMode::Internal,
        )
        .unwrap();
    legacy_package
        .add_relationship(
            Some(PartUri::parse("word/document.xml").unwrap()),
            "rTheme",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
            "theme/theme1.xml",
            TargetMode::Internal,
        )
        .unwrap();
    let mut imported = import_docx(&legacy_package.write().unwrap()).unwrap();
    let id = imported.paragraph_ids()[0];
    let (resolved, _) = imported.resolved_paragraph(id).unwrap();
    assert_eq!(resolved.runs.len(), 6);
    assert_eq!(
        resolved.runs[0].attrs.color,
        Some(atelier_text_model::rich::Color {
            r: 0x11,
            g: 0,
            b: 0
        })
    );
    assert_eq!(
        resolved.runs[5].attrs.color,
        Some(atelier_text_model::rich::Color {
            r: 0x66,
            g: 0x66,
            b: 0
        })
    );

    let unknown = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:color w:themeColor="mystery"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    assert!(matches!(
        import_docx(&package(unknown.as_bytes(), [])),
        Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
    ));
}

#[test]
fn default_numbering_character_and_table_styles_do_not_replace_default_paragraph() {
    let document = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>sample</w:t></w:r></w:p></w:body></w:document>"#
    );
    let with_styles = |other_type: &str| {
        let styles = format!(
            r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:rPr><w:color w:val="112233"/></w:rPr></w:style><w:style w:type="{other_type}" w:styleId="Other" w:default="1"/></w:styles>"#
        );
        let bytes = package(
            document.as_bytes(),
            [(
                "word/styles.xml".into(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml".into(),
                styles.into_bytes(),
            )],
        );
        let mut p = Package::read(&bytes).unwrap();
        p.add_relationship(
            Some(PartUri::parse("word/document.xml").unwrap()),
            "rStyles",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
            "styles.xml",
            TargetMode::Internal,
        )
        .unwrap();
        p.write().unwrap()
    };
    for other_type in ["numbering", "character", "table"] {
        let mut imported = import_docx(&with_styles(other_type)).unwrap();
        let id = imported.paragraph_ids()[0];
        assert_eq!(
            imported
                .document
                .paragraph(id)
                .unwrap()
                .props
                .style
                .as_deref(),
            Some("Normal")
        );
        let (paragraph, _) = imported.resolved_paragraph(id).unwrap();
        assert_eq!(
            paragraph.runs[0].attrs.color,
            Some(atelier_text_model::rich::Color {
                r: 0x11,
                g: 0x22,
                b: 0x33
            })
        );
    }
    assert!(matches!(
        import_docx(&with_styles("paragraph")),
        Err(atelier_fmt_ooxml::DocxError::InvalidStructure {
            detail: "more than one default paragraph style",
            ..
        })
    ));
}

#[test]
fn malformed_symbols_and_false_namespaces_are_rejected_contextually() {
    let bad = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:sym w:char="not-hex"/></w:r></w:p></w:body></w:document>"#
    );
    assert!(matches!(
        import_docx(&package(bad.as_bytes(), [])),
        Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
    ));
    let fake = b"<f:document xmlns:f=\"urn:fake\"><f:body/></f:document>";
    assert!(matches!(
        import_docx(&package(fake, [])),
        Err(atelier_fmt_ooxml::DocxError::InvalidStructure { .. })
    ));
}

#[test]
fn malformed_rgb_and_paragraph_budget_fail_without_publishing_a_partial_result() {
    let malformed = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:color w:val="1é234"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    assert!(matches!(
        import_docx(&package(malformed.as_bytes(), [])),
        Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
    ));
    let valid = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p><w:p/></w:body></w:document>"#
    );
    let limits = DocxLimits {
        max_paragraphs: 1,
        ..DocxLimits::default()
    };
    assert!(matches!(
        import_docx_with_limits(&package(valid.as_bytes(), []), limits),
        Err(atelier_fmt_ooxml::DocxError::Limit("paragraph count"))
    ));
}

#[test]
fn malformed_on_off_values_are_not_silently_treated_as_true() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:b w:val="sometimes"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    assert!(matches!(
        import_docx(&package(xml.as_bytes(), [])),
        Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
    ));
}

#[test]
fn grapheme_splits_keep_the_first_scalar_style_and_later_text_style() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>a</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>́b</w:t></w:r></w:p><w:p><w:r><w:t>a</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>́b</w:t></w:r></w:p></w:body></w:document>"#
    );
    let imported = import_docx(&package(xml.as_bytes(), [])).unwrap();
    let first_id = imported.paragraph_ids()[0];
    let first = imported.document.paragraph(first_id).unwrap();
    assert_eq!(first.text, "áb");
    assert_eq!(first.runs.len(), 2);
    assert_eq!((first.runs[0].start, first.runs[0].end), (0, 3));
    assert_eq!(first.runs[0].attrs.bold, Some(true));
    assert_eq!(first.runs[0].attrs.italic, None);
    assert_eq!((first.runs[1].start, first.runs[1].end), (3, 4));
    assert_eq!(first.runs[1].attrs.italic, Some(true));
    let second_id = imported.paragraph_ids()[1];
    let second = imported.document.paragraph(second_id).unwrap();
    assert_eq!(second.runs.len(), 1);
    assert_eq!((second.runs[0].start, second.runs[0].end), (3, 4));
    assert_eq!(second.runs[0].attrs.italic, Some(true));
    for id in imported.paragraph_ids() {
        imported.document.paragraph(*id).unwrap();
    }
    imported.document.validate().unwrap();
}

#[test]
fn alternating_styled_runs_and_split_graphemes_keep_stable_offsets() {
    let mut xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>a</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>́b</w:t></w:r>"#
    );
    for index in 0..256 {
        let (tag, character) = if index % 2 == 0 {
            ("b", 'x')
        } else {
            ("i", 'y')
        };
        xml.push_str(&format!(
            r#"<w:r><w:rPr><w:{tag}/></w:rPr><w:t>{character}</w:t></w:r>"#
        ));
    }
    xml.push_str("</w:p></w:body></w:document>");
    let mut imported = import_docx(&package(xml.as_bytes(), [])).unwrap();
    let id = imported.paragraph_ids()[0];
    let paragraph = imported.document.paragraph(id).unwrap();
    assert_eq!(paragraph.runs.first().unwrap().end, 3);
    assert_eq!(paragraph.runs[0].attrs.bold, Some(true));
    assert_eq!(paragraph.runs[1].attrs.italic, Some(true));
    assert_eq!(paragraph.runs[1].start, 3);
    imported.document.validate().unwrap();
    let (resolved, _) = imported.resolved_paragraph(id).unwrap();
    assert!(
        resolved
            .runs
            .windows(2)
            .all(|pair| pair[0].end == pair[1].start)
    );
    assert_eq!(resolved.runs.len(), 258);
}

#[test]
fn hyperlink_text_is_visible_but_deleted_instruction_and_foreign_text_are_not() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}" xmlns:f="urn:foreign" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>before </w:t></w:r><w:hyperlink r:id="external"><w:r><w:t>linked</w:t></w:r></w:hyperlink><w:del><w:r><w:delText>deleted</w:delText></w:r><w:p><w:r><w:t>deleted paragraph</w:t></w:r></w:p></w:del><w:r><w:instrText>HIDDEN</w:instrText><w:t> after</w:t></w:r><f:box><w:r><w:t>foreign</w:t></w:r><w:p><w:r><w:t>foreign paragraph</w:t></w:r></w:p></f:box></w:p></w:body></w:document>"#
    );
    let imported = import_docx(&package(xml.as_bytes(), [])).unwrap();
    let p = imported
        .document
        .paragraph(imported.paragraph_ids()[0])
        .unwrap();
    assert_eq!(p.text, "before linked after");
    assert!(imported.notices.iter().any(|n| matches!(n, atelier_fmt_ooxml::ImportNotice::UnsupportedElement { local_name, .. } if local_name == "hyperlink")));
}

#[test]
fn size_and_other_supported_numeric_properties_are_validated() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:sz w:val="24"/><w:szCs w:val="40"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    let imported = import_docx(&package(xml.as_bytes(), [])).unwrap();
    let p = imported
        .document
        .paragraph(imported.paragraph_ids()[0])
        .unwrap();
    assert_eq!(p.runs[0].attrs.size_half_points, Some(24));
    assert!(
        p.runs[0]
            .attrs
            .opaque
            .iter()
            .any(|a| a.name == "szCs" && a.value == b"40")
    );
    assert!(imported.notices.iter().any(|n| matches!(n, atelier_fmt_ooxml::ImportNotice::UnsupportedProperty { local_name, .. } if local_name == "szCs")));

    for malformed in [
        r#"<w:r><w:rPr><w:sz w:val="large"/></w:rPr><w:t>x</w:t></w:r>"#,
        r#"<w:pPr><w:ind w:left="nope"/></w:pPr>"#,
        r#"<w:pPr><w:ind w:right="nope"/></w:pPr>"#,
        r#"<w:pPr><w:spacing w:before="nope"/></w:pPr>"#,
        r#"<w:pPr><w:spacing w:after="nope"/></w:pPr>"#,
        r#"<w:pPr><w:spacing w:line="nope"/></w:pPr>"#,
    ] {
        let xml = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p>{malformed}<w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
        );
        assert!(
            matches!(
                import_docx(&package(xml.as_bytes(), [])),
                Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
            ),
            "{malformed}"
        );
    }
}

#[test]
fn highlight_none_is_preserved_and_warns_that_cascade_reset_is_unsupported() {
    let mut p = Package::new();
    let document = format!(
        r#"<w:document xmlns:w="{W}"><w:body><w:p><w:pPr><w:pStyle w:val="Base"/></w:pPr><w:r><w:rPr><w:highlight w:val="none"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        document.into_bytes(),
    )
    .unwrap();
    p.insert_part(PartUri::parse("word/styles.xml").unwrap(), "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml", format!(r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="Base"><w:rPr><w:highlight w:val="yellow"/></w:rPr></w:style></w:styles>"#).into_bytes()).unwrap();
    p.add_relationship(
        None,
        "rDoc",
        MAIN_REL,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/document.xml").unwrap()),
        "rStyles",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
        "styles.xml",
        TargetMode::Internal,
    )
    .unwrap();
    let mut imported = import_docx(&p.write().unwrap()).unwrap();
    let id = imported.paragraph_ids()[0];
    let paragraph = imported.document.paragraph(id).unwrap();
    assert_eq!(
        paragraph.runs[0]
            .attrs
            .opaque
            .iter()
            .find(|a| a.name == "highlight")
            .unwrap()
            .value,
        b"none"
    );
    let (resolved, _) = imported.resolved_paragraph(id).unwrap();
    assert_eq!(
        resolved.runs[0].attrs.highlight,
        Some(atelier_text_model::rich::Color {
            r: 255,
            g: 255,
            b: 0
        })
    );
    assert!(imported.notices.iter().any(|n| matches!(n, atelier_fmt_ooxml::ImportNotice::UnsupportedProperty { local_name, value, .. } if local_name == "highlight" && value == "none")));
}

#[test]
fn embedded_font_rejects_fake_relationship_type_suffix() {
    let mut p = Package::new();
    let doc = format!(r#"<w:document xmlns:w="{W}"><w:body/></w:document>"#);
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        doc.into_bytes(),
    )
    .unwrap();
    let fonts = format!(
        r#"<w:fonts xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:font w:name="Fake"><w:embedRegular r:id="rFont" w:fontKey="00112233-4455-6677-8899-aabbccddeeff"/></w:font></w:fonts>"#
    );
    p.insert_part(
        PartUri::parse("word/fontTable.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml",
        fonts.into_bytes(),
    )
    .unwrap();
    p.insert_part(
        PartUri::parse("word/fonts/fake.odttf").unwrap(),
        "application/vnd.openxmlformats-officedocument.obfuscatedFont",
        vec![0; 64],
    )
    .unwrap();
    p.add_relationship(
        None,
        "rDoc",
        MAIN_REL,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/document.xml").unwrap()),
        "rFonts",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable",
        "fontTable.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/fontTable.xml").unwrap()),
        "rFont",
        "urn:example:font",
        "fonts/fake.odttf",
        TargetMode::Internal,
    )
    .unwrap();
    assert!(matches!(
        import_docx(&p.write().unwrap()),
        Err(atelier_fmt_ooxml::DocxError::InvalidValue { .. })
    ));
}
