// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-204: breaks, symbols and character styles are fields of the model, fed
//! by the import and written back by the writer; the page geometry of the
//! section comes from the `w:sectPr`.
mod support;

use atelier_fmt_ooxml::{
    DocxWriteOptions, ImportedDocx, WriteNotice, import_docx, source_text, write_docx,
};
use atelier_model::Transaction;
use atelier_text_model::rich::flat::{FlatEdit, FlatIndex, LayoutContext, PageSetup};
use atelier_text_model::rich::{
    BreakKind, CharAttrs, Inline, InlineBreak, InlineSymbol, Node, Paragraph, RichOp,
};
use support::*;

fn apply(imp: &mut ImportedDocx, ops: Vec<RichOp>) {
    imp.document
        .apply(&Transaction::new(ops).unwrap())
        .expect("valid edit");
}

fn paragraph(imp: &ImportedDocx, index: usize) -> Paragraph {
    imp.document
        .paragraph(imp.paragraph_ids()[index])
        .unwrap()
        .clone()
}

fn written(imp: &ImportedDocx, regenerate: bool) -> (ImportedDocx, String, Vec<WriteNotice>) {
    let options = DocxWriteOptions {
        regenerate_all: regenerate,
        ..DocxWriteOptions::default()
    };
    let out = write_docx(imp, &options).unwrap();
    let again = import_docx(&out.bytes).unwrap();
    let package = atelier_fmt_ooxml::Package::read(&out.bytes).unwrap();
    let main = package
        .parts()
        .values()
        .find(|p| p.uri.as_str().ends_with("document.xml"))
        .unwrap();
    let xml = String::from_utf8(main.bytes.clone()).unwrap();
    (again, xml, out.notices)
}

const RICH: &str = concat!(
    r#"<w:p><w:r><w:rPr><w:rStyle w:val="Emph"/></w:rPr><w:t>ab</w:t><w:br/><w:t>cd</w:t></w:r>"#,
    r#"<w:r><w:br w:type="page"/><w:cr/><w:sym w:font="Symbol" w:char="F0B7"/><w:br w:type="column"/><w:t>e</w:t></w:r></w:p>"#
);

#[test]
fn the_import_feeds_breaks_symbols_and_character_styles() {
    let imp = doc(body(RICH)).import();
    let p = paragraph(&imp, 0);
    assert_eq!(p.text, "abcde");
    let breaks: Vec<_> = p.breaks.iter().map(|b| (b.offset, b.kind)).collect();
    assert_eq!(
        breaks,
        [
            (2, BreakKind::Line),
            (4, BreakKind::Page),
            (4, BreakKind::Line),
            (4, BreakKind::Column)
        ]
    );
    assert_eq!(p.symbols.len(), 1);
    assert_eq!((p.symbols[0].offset, p.symbols[0].code), (4, 0xf0b7));
    assert_eq!(p.symbols[0].font, "Symbol");
    // Items at one offset follow the document order (ids).
    let mut ids: Vec<_> = p.breaks.iter().map(|b| b.id).collect();
    ids.push(p.symbols[0].id);
    assert!(ids[1] < ids[2] && ids[2] < p.symbols[0].id && p.symbols[0].id < ids[3]);
    // The character style is a run attribute; it costs no byte.
    assert_eq!(p.runs.len(), 1);
    assert_eq!((p.runs[0].start, p.runs[0].end), (0, 4));
    assert_eq!(p.runs[0].attrs.char_style.as_deref(), Some("Emph"));
    // Flat text: 5 text bytes and 5 items of 3 bytes.
    assert_eq!(imp.document.len(), 5 + 5 * 3);
    assert_eq!(
        imp.document.text(0, imp.document.len()).unwrap(),
        "ab\u{fffc}cd\u{fffc}\u{fffc}\u{fffc}\u{fffc}e"
    );
    // The text of the source, as the skeleton counts it.
    assert_eq!(source_text(&p), "ab\ncd\n\n\u{f0b7}\ne");
    // Annex tables, in the offsets of the model.
    assert_eq!(imp.breaks.len(), 4);
    assert_eq!(imp.symbols[0].offset, 4);
    assert_eq!(imp.run_styles[0].style_id.as_deref(), Some("Emph"));
    imp.document.validate().unwrap();
}

#[test]
fn items_and_styles_round_trip_without_loss() {
    let imp = doc(body(RICH)).import();
    for regenerate in [false, true] {
        let (again, xml, notices) = written(&imp, regenerate);
        assert!(notices.is_empty(), "{notices:?}");
        let (a, b) = (paragraph(&imp, 0), paragraph(&again, 0));
        assert_eq!(a.text, b.text, "regenerate {regenerate}");
        assert_eq!(a.runs, b.runs);
        assert_eq!(
            a.breaks
                .iter()
                .map(|x| (x.offset, x.kind))
                .collect::<Vec<_>>(),
            b.breaks
                .iter()
                .map(|x| (x.offset, x.kind))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            a.symbols
                .iter()
                .map(|x| (x.offset, x.font.clone(), x.code))
                .collect::<Vec<_>>(),
            b.symbols
                .iter()
                .map(|x| (x.offset, x.font.clone(), x.code))
                .collect::<Vec<_>>()
        );
        assert!(xml.contains(r#"w:type="page""#) && xml.contains(r#"w:rStyle w:val="Emph""#));
    }
}

#[test]
fn items_added_to_the_model_are_written_as_elements() {
    let mut imp = doc(body(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hello world</w:t></w:r></w:p>"#,
    ))
    .import();
    let para = imp.paragraph_ids()[0];
    let (b, s) = (
        imp.document.alloc_id().unwrap(),
        imp.document.alloc_id().unwrap(),
    );
    apply(
        &mut imp,
        vec![
            RichOp::InsertInline {
                para,
                item: Inline::Break(InlineBreak {
                    id: b,
                    offset: 5,
                    kind: BreakKind::Page,
                }),
            },
            RichOp::InsertInline {
                para,
                item: Inline::Symbol(InlineSymbol {
                    id: s,
                    offset: 11,
                    font: "Wingdings".into(),
                    code: 0xf0fc,
                }),
            },
        ],
    );
    for regenerate in [false, true] {
        let (again, xml, _) = written(&imp, regenerate);
        assert!(xml.contains(r#"<w:br w:type="page"/>"#), "{xml}");
        assert!(
            xml.contains(r#"<w:sym w:font="Wingdings" w:char="F0FC"/>"#),
            "{xml}"
        );
        let q = paragraph(&again, 0);
        assert_eq!(q.text, "Hello world", "regenerate {regenerate}");
        assert_eq!(
            q.breaks
                .iter()
                .map(|x| (x.offset, x.kind))
                .collect::<Vec<_>>(),
            [(5, BreakKind::Page)]
        );
        assert_eq!(
            (
                q.symbols[0].offset,
                q.symbols[0].code,
                q.symbols[0].font.as_str()
            ),
            (11, 0xf0fc, "Wingdings")
        );
        assert_eq!(q.runs, paragraph(&imp, 0).runs);
    }
}

#[test]
fn flat_edits_around_items_are_written_and_read_back_equal() {
    let mut imp = doc(body(RICH)).import();
    let before = imp.document.len();
    let at = imp.document.len() - 1; // before the final "e"
    imp.document.replace_range(at, at, "XY").unwrap();
    imp.document.replace_range(0, 0, "\u{e9}\n").unwrap();
    assert_eq!(imp.document.len(), before + 2 + 3);
    let flat = imp.document.text(0, imp.document.len()).unwrap();
    let (again, _, notices) = written(&imp, false);
    assert!(
        !notices
            .iter()
            .any(|n| matches!(n, WriteNotice::ParagraphRebuilt { .. })),
        "{notices:?}"
    );
    assert_eq!(again.document.len(), imp.document.len());
    assert_eq!(again.document.text(0, again.document.len()).unwrap(), flat);
    // And the other way: through the rebuilt path.
    let (again, _, _) = written(&imp, true);
    assert_eq!(again.document.text(0, again.document.len()).unwrap(), flat);
    // Undo restores the model exactly.
    imp.document.undo().unwrap();
    imp.document.undo().unwrap();
    assert_eq!(imp.document.len(), before);
}

#[test]
fn a_character_style_set_through_the_model_is_written() {
    let mut imp = doc(body(r#"<w:p><w:r><w:t>Hello world</w:t></w:r></w:p>"#)).import();
    let para = imp.paragraph_ids()[0];
    apply(
        &mut imp,
        vec![RichOp::SetCharAttrs {
            para,
            start: 0,
            end: 5,
            set: CharAttrs {
                char_style: Some("Strong".into()),
                ..CharAttrs::default()
            },
            clear: CharAttrs::default(),
        }],
    );
    for regenerate in [false, true] {
        let (again, xml, _) = written(&imp, regenerate);
        assert!(xml.contains(r#"<w:rStyle w:val="Strong"/>"#), "{xml}");
        let q = paragraph(&again, 0);
        assert_eq!(q.runs.len(), 1, "regenerate {regenerate}");
        assert_eq!((q.runs[0].start, q.runs[0].end), (0, 5));
        assert_eq!(q.runs[0].attrs.char_style.as_deref(), Some("Strong"));
    }
    // Clearing it removes the element.
    apply(
        &mut imp,
        vec![RichOp::SetCharAttrs {
            para,
            start: 0,
            end: 5,
            set: CharAttrs::default(),
            clear: CharAttrs {
                char_style: Some(String::new()),
                ..CharAttrs::default()
            },
        }],
    );
    let (again, xml, _) = written(&imp, false);
    assert!(!xml.contains("rStyle"), "{xml}");
    assert!(paragraph(&again, 0).runs.is_empty());
}

#[test]
fn the_section_gets_its_page_geometry_from_the_sect_pr() {
    let landscape = concat!(
        r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/>"#,
        r#"<w:pgMar w:top="720" w:right="1000" w:bottom="900" w:left="1100" w:header="300" w:footer="400" w:gutter="50"/></w:sectPr>"#
    );
    let imp = doc(body(landscape)).import();
    let setup = imp.document.page_setup(0).unwrap();
    assert_eq!(
        setup,
        PageSetup {
            width: 16838,
            height: 11906,
            landscape: true,
            margin_top: 720,
            margin_right: 1000,
            margin_bottom: 900,
            margin_left: 1100,
            header: 300,
            footer: 400,
            gutter: 50,
        }
    );
    let (_, section) = imp.document.section_of(0).unwrap();
    assert_eq!(section.opaque.len(), 10);
    assert!(
        section
            .opaque
            .iter()
            .all(|a| a.namespace == atelier_text_model::rich::flat::WORD_NS)
    );
    // Without a sectPr: the defaults. A partial one: defaults for the rest.
    assert_eq!(
        doc(body(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#))
            .import()
            .document
            .page_setup(0)
            .unwrap(),
        PageSetup::standard()
    );
    let partial = r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>"#;
    let setup = doc(body(partial)).import().document.page_setup(0).unwrap();
    assert_eq!((setup.width, setup.height), (12240, 15840));
    assert_eq!(setup.margin_left, PageSetup::DEFAULT_MARGIN);
    // The sectPr of the first section (in a paragraph) wins over the last.
    let two = concat!(
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>b</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="16838" w:h="11906"/></w:sectPr>"#
    );
    let setup = doc(body(two)).import().document.page_setup(0).unwrap();
    assert_eq!((setup.width, setup.height), (12240, 15840));
    // The geometry survives the write.
    let imp = doc(body(landscape)).import();
    let (again, _, _) = written(&imp, true);
    assert_eq!(again.document.page_setup(0), imp.document.page_setup(0));
    // Nothing else changed in the model by feeding it.
    assert!(matches!(
        imp.document.node(imp.document.section_ids()[0]),
        Some(Node::Section(_))
    ));
    let _ = imp.document.paragraph_count();
    let _ = LayoutContext::block_kind(&imp.document, 0);
}

#[test]
fn break_attributes_survive_import_and_rewriting() {
    use atelier_text_model::rich::ClearSide;
    let xml = concat!(
        r#"<w:p><w:r><w:t>a</w:t><w:br w:type="textWrapping" w:clear="all"/><w:t>b</w:t><w:br w:clear="left"/>"#,
        r#"<w:br w:type="textWrapping"/><w:br/></w:r></w:p>"#
    );
    let imp = doc(body(xml)).import();
    let kinds = |p: &Paragraph| p.breaks.iter().map(|b| b.kind).collect::<Vec<_>>();
    let expected = vec![
        BreakKind::Wrapping {
            explicit_type: true,
            clear: Some(ClearSide::All),
        },
        BreakKind::Wrapping {
            explicit_type: false,
            clear: Some(ClearSide::Left),
        },
        BreakKind::Wrapping {
            explicit_type: true,
            clear: None,
        },
        BreakKind::Line,
    ];
    assert_eq!(kinds(&paragraph(&imp, 0)), expected);
    for regenerate in [false, true] {
        let (again, xml, _) = written(&imp, regenerate);
        assert_eq!(kinds(&paragraph(&again, 0)), expected, "{regenerate}");
        assert!(xml.contains(r#"w:clear="all""#), "{xml}");
    }
    // Added through the model, in the skeleton path and the rebuilt one.
    let mut imp = doc(body(r#"<w:p><w:r><w:t>xy</w:t></w:r></w:p>"#)).import();
    let para = imp.paragraph_ids()[0];
    let id = imp.document.alloc_id().unwrap();
    apply(
        &mut imp,
        vec![RichOp::InsertInline {
            para,
            item: Inline::Break(InlineBreak {
                id,
                offset: 1,
                kind: expected[0],
            }),
        }],
    );
    for regenerate in [false, true] {
        let (again, _, _) = written(&imp, regenerate);
        assert_eq!(kinds(&paragraph(&again, 0)), [expected[0]], "{regenerate}");
    }
}
