// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_core_wasm::rich_preview::{PreviewError, preview_docx};
use atelier_fmt_ooxml::{Package, PartUri, TargetMode};
use atelier_render::{Command, DisplayList, PathElement};

const REFERENCE: &[u8] = include_bytes!("../../../tests/docx-reference/basic.docx");
const TITLE: &str = "Référence de titre — Français et English mix: cœurs, accents, ligatures.";

#[test]
fn reference_docx_renders_real_title_color_and_valid_page() {
    let preview = preview_docx(REFERENCE).expect("reference DOCX previews");
    assert_eq!(preview.text.split('\n').next(), Some(TITLE));
    assert_eq!(preview.pages.len(), 1);
    let page = DisplayList::decode(&preview.pages[0]).expect("ATDL validates");
    assert!((page.page.bounds.width() - 595.276).abs() < 0.01);
    assert!((page.page.bounds.height() - 841.891).abs() < 0.01);
    assert!(page.commands.iter().any(|command| matches!(command,
        Command::FillPath { path, .. } if path.elements.iter().filter(|element| matches!(element, PathElement::MoveTo(_))).count() > 1
    )), "counter contours stay grouped with their glyph");
    assert!(page.commands.iter().any(|command| matches!(command,
        Command::FillPath { color, .. } if (color.red * 255.0).round() as u8 == 0x44
            && (color.green * 255.0).round() as u8 == 0x72
            && (color.blue * 255.0).round() as u8 == 0xc4
    )));
    assert!(
        page.commands
            .iter()
            .filter(|c| matches!(c, Command::FillPath { .. }))
            .count()
            > TITLE.chars().count()
    );
    let (mut blue_height, mut body_height) = (0.0f64, 0.0f64);
    for command in &page.commands {
        let Command::FillPath { path, color, .. } = command else {
            continue;
        };
        let ys = path
            .elements
            .iter()
            .filter_map(|element| match element {
                PathElement::MoveTo(p) | PathElement::LineTo(p) => Some(p.y()),
                PathElement::QuadraticTo { control, to } => Some(control.y().min(to.y())),
                PathElement::CubicTo {
                    control1,
                    control2,
                    to,
                } => Some(control1.y().min(control2.y()).min(to.y())),
                PathElement::Close => None,
            })
            .collect::<Vec<_>>();
        if ys.is_empty() {
            continue;
        }
        let height = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - ys.iter().copied().fold(f64::INFINITY, f64::min);
        if (color.blue * 255.0).round() as u8 == 0xc4 {
            blue_height = blue_height.max(height);
        } else {
            body_height = body_height.max(height);
        }
    }
    assert!(
        blue_height > body_height,
        "title outlines use the larger resolved font size"
    );
    assert_eq!(
        preview,
        preview_docx(REFERENCE).unwrap(),
        "layout is deterministic"
    );
}

#[test]
fn inline_bold_italic_underline_and_size_reach_the_display_list() {
    let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/><w:i/><w:u w:val="single"/><w:color w:val="FF0000"/><w:sz w:val="40"/></w:rPr><w:t>Bold italic underlined</w:t></w:r></w:p></w:body></w:document>"#;
    let preview = preview_docx(&minimal_docx(xml)).expect("styled run previews");
    let page = DisplayList::decode(&preview.pages[0]).unwrap();
    assert!(
        page.commands
            .iter()
            .any(|command| matches!(command, Command::StrokePath { .. }))
    );
    assert!(page.commands.iter().any(|command| matches!(command,
        Command::FillPath { color, .. } if color.red == 1.0 && color.green == 0.0 && color.blue == 0.0
    )));
}

#[test]
fn tab_control_keeps_its_advance_without_drawing_notdef() {
    let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p></w:body></w:document>"#;
    let preview = preview_docx(&minimal_docx(xml)).expect("tab-containing DOCX previews");
    assert_eq!(preview.text, "A\tB");
    let page = DisplayList::decode(&preview.pages[0]).unwrap();
    assert_eq!(
        page.commands
            .iter()
            .filter(|command| matches!(command, Command::FillPath { .. }))
            .count(),
        2,
        "only A and B are drawn; the tab layout glyph is not a .notdef outline"
    );
}

#[test]
fn generated_long_docx_paginates_without_losing_text_order() {
    let text = "A long paragraph used to force line wrapping across multiple pages. ".repeat(4);
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#,
        (0..60)
            .map(|_| format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#))
            .collect::<String>()
    );
    let bytes = minimal_docx(&xml);
    let preview = preview_docx(&bytes).expect("long document remains within preview budgets");
    assert!(preview.pages.len() > 1);
    assert_eq!(preview.text.matches(&text).count(), 60);
    assert!(
        preview
            .pages
            .iter()
            .all(|page| DisplayList::decode(page).is_ok())
    );
    assert_eq!(preview, preview_docx(&bytes).unwrap());
}

#[test]
fn invalid_and_over_limit_docx_return_controlled_errors() {
    assert!(matches!(
        preview_docx(b"not a docx"),
        Err(PreviewError::Import(_))
    ));
    assert_eq!(
        preview_docx(&vec![0; 8 * 1024 * 1024 + 1]),
        Err(PreviewError::InputTooLarge)
    );
}

#[test]
fn paragraph_and_text_budgets_reject_oversized_models() {
    let paragraphs = (0..1025)
        .map(|_| "<w:p><w:r><w:t>x</w:t></w:r></w:p>")
        .collect::<String>();
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{paragraphs}</w:body></w:document>"#
    );
    assert!(preview_docx(&minimal_docx(&xml)).is_err());

    let oversized_text = "x".repeat(128 * 1024 + 1);
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{oversized_text}</w:t></w:r></w:p></w:body></w:document>"#
    );
    assert!(preview_docx(&minimal_docx(&xml)).is_err());
}

#[test]
fn glyph_budget_is_enforced_while_layout_is_built() {
    let oversized_text = "x".repeat(65_537);
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{oversized_text}</w:t></w:r></w:p></w:body></w:document>"#
    );
    assert!(matches!(
        preview_docx(&minimal_docx(&xml)),
        Err(PreviewError::Layout(_)) | Err(PreviewError::Limit("glyphs"))
    ));
}

fn minimal_docx(xml: &str) -> Vec<u8> {
    let mut package = Package::new();
    package
        .insert_part(
            PartUri::parse("word/document.xml").unwrap(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            xml.as_bytes().to_vec(),
        )
        .unwrap();
    package
        .add_relationship(
            None,
            "rId1",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
            "word/document.xml",
            TargetMode::Internal,
        )
        .unwrap();
    package.write().unwrap()
}
