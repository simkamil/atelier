// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-243-M1: paragraph shading and borders (`w:shd`, `w:pBdr`) read, kept
//! when unchanged, written in schema order when changed, and read back equal.
mod support;

use atelier_fmt_ooxml::{DocxWriteOptions, ImportedDocx, Package, import_docx, write_docx};
use atelier_model::Transaction;
use atelier_text_model::rich::{
    Color, ParaBorder, ParaBorderStyle, ParaBorders, ParaProps, ParaShading, RichOp,
};
use support::order::order_problems;
use support::*;

fn doc_with(inner: &str) -> ImportedDocx {
    let mut d = doc(body(inner));
    d.styles = Some(STYLES.into());
    d.settings = Some(SETTINGS.into());
    d.import()
}

fn written(imp: &ImportedDocx) -> Vec<u8> {
    let w = write_docx(imp, &DocxWriteOptions::default()).unwrap();
    let package = Package::read(&w.bytes).unwrap();
    package
        .parts()
        .values()
        .find(|p| p.uri.as_str().ends_with("document.xml") || p.uri.as_str().contains("main"))
        .unwrap()
        .bytes
        .clone()
}

fn props_of(imp: &ImportedDocx, index: usize) -> ParaProps {
    let id = imp.paragraph_ids()[index];
    imp.document.paragraph(id).unwrap().props.clone()
}

const SOURCE: &str = r#"<w:p><w:pPr><w:pBdr><w:top w:val="single" w:sz="12" w:space="3" w:color="FF0000"/><w:bottom w:val="wave" w:sz="4" w:space="1" w:color="auto"/><w:between w:val="dashed" w:sz="6" w:space="2" w:color="0000FF"/><w:left w:val="nil"/></w:pBdr><w:shd w:val="pct25" w:color="00FF00" w:fill="FFFF00"/><w:jc w:val="center"/></w:pPr><w:r><w:t>boxed</w:t></w:r></w:p><w:p><w:r><w:t>plain</w:t></w:r></w:p>"#;

fn red() -> Color {
    Color { r: 255, g: 0, b: 0 }
}

#[test]
fn borders_and_shading_are_read_from_the_paragraph_properties() {
    let imp = doc_with(SOURCE);
    let p = props_of(&imp, 0);
    let top = p.borders.top.unwrap();
    assert_eq!(
        (top.style, top.eighths, top.space, top.color),
        (ParaBorderStyle::Single, 12, 3, Some(red()))
    );
    // A style Atelier cannot draw reads as a single line; it is kept on write.
    let bottom = p.borders.bottom.unwrap();
    assert_eq!(
        (bottom.style, bottom.color),
        (ParaBorderStyle::Single, None)
    );
    assert_eq!(p.borders.between.unwrap().style, ParaBorderStyle::Dashed);
    assert_eq!(p.borders.left.unwrap().style, ParaBorderStyle::None);
    assert!(p.borders.right.is_none());
    let shd = p.shading.unwrap();
    assert_eq!(
        shd.fill,
        Some(Color {
            r: 255,
            g: 255,
            b: 0
        })
    );
    assert_eq!(shd.pattern.as_deref(), Some("pct25"));
    assert_eq!(shd.pattern_color, Some(Color { r: 0, g: 255, b: 0 }));
    assert!(props_of(&imp, 1).borders.is_empty() && props_of(&imp, 1).shading.is_none());
}

#[test]
fn unchanged_borders_keep_their_source_form_on_write() {
    let imp = doc_with(SOURCE);
    let text = String::from_utf8(written(&imp)).unwrap();
    assert!(text.contains(r#"w:val="wave""#), "{text}");
    assert!(text.contains(r#"w:val="pct25""#), "{text}");
}

#[test]
fn changed_borders_and_shading_are_written_in_order_and_read_back() {
    let mut imp = doc_with(SOURCE);
    let id = imp.paragraph_ids()[1];
    let line = |style, eighths, space, color| {
        Some(ParaBorder {
            style,
            eighths,
            space,
            color,
        })
    };
    let set = ParaProps {
        borders: ParaBorders {
            top: line(ParaBorderStyle::Double, 18, 4, Some(red())),
            right: line(ParaBorderStyle::Dotted, 4, 0, None),
            left: line(ParaBorderStyle::None, 0, 0, None),
            ..ParaBorders::default()
        },
        shading: Some(ParaShading {
            fill: Some(Color { r: 1, g: 2, b: 3 }),
            ..ParaShading::default()
        }),
        alignment: Some(atelier_text_model::rich::Alignment::Right),
        ..ParaProps::default()
    };
    imp.document
        .apply(
            &Transaction::new(vec![RichOp::SetParaProps {
                para: id,
                set: set.clone(),
                clear: ParaProps::default(),
            }])
            .unwrap(),
        )
        .unwrap();
    let bytes = written(&imp);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(order_problems(&parse(&bytes)).is_empty(), "{text}");
    let pbdr = text
        .find("<w:pBdr>")
        .and_then(|a| text[a..].find("</w:pBdr>").map(|b| &text[a..a + b]));
    let pbdr = pbdr.unwrap_or_else(|| panic!("{text}"));
    let order: Vec<_> = ["w:top", "w:left", "w:right"]
        .iter()
        .map(|n| pbdr.rfind(n).unwrap_or_else(|| panic!("{n} in {pbdr}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{pbdr}");
    assert!(
        text.contains(r#"w:fill="010203""#) && text.contains(r#"w:val="clear""#),
        "{text}"
    );

    // Round trip: a second import reads the same values.
    let again = import_docx(&{
        let w = write_docx(&imp, &DocxWriteOptions::default()).unwrap();
        w.bytes
    })
    .unwrap();
    let back = props_of(&again, 1);
    assert_eq!(back.borders, set.borders);
    assert_eq!(back.shading, set.shading);
    // The first paragraph kept its source borders.
    assert_eq!(
        props_of(&again, 0).borders.bottom.unwrap().style,
        ParaBorderStyle::Single
    );
}
