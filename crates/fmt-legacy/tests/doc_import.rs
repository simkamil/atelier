// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-312: `.doc` import on files produced by the deterministic test builder.
//! Expected values are computed by hand from [MS-DOC].

mod common;

use atelier_fmt_legacy::doc::{
    DocError, DocStyleKind, DocVersion, ImportNotice, ImportedDoc, OPAQUE_NS, TextRegion,
    import_doc,
};
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, LineRule, LineSpacing, OpaqueAttr, Paragraph, Underline,
    VerticalAlign,
};
use common::*;

fn open(spec: &DocSpec) -> ImportedDoc {
    import_doc(&spec.build()).expect("import")
}

fn paras(doc: &ImportedDoc) -> Vec<Paragraph> {
    doc.paragraph_ids()
        .iter()
        .map(|id| doc.document.paragraph(*id).unwrap().clone())
        .collect()
}

fn texts(doc: &ImportedDoc) -> Vec<String> {
    paras(doc).into_iter().map(|p| p.text).collect()
}

fn opaque(name: &str, value: &[u8]) -> OpaqueAttr {
    OpaqueAttr {
        namespace: OPAQUE_NS.into(),
        name: name.into(),
        value: value.to_vec(),
    }
}

fn char_run(text: &str, grpprl: Vec<u8>) -> DocSpec {
    // The first paragraph is `text` + paragraph mark; the run covers `text`.
    let n = text.chars().count();
    let mut spec = DocSpec::ansi(&format!("{text}\r"));
    spec.chpx.push(CharRunSpec {
        cp_start: 0,
        cp_end: n,
        grpprl,
    });
    spec
}

fn attrs_of(doc: &ImportedDoc) -> CharAttrs {
    let p = &paras(doc)[0];
    assert_eq!(p.runs.len(), 1, "runs: {:?}", p.runs);
    p.runs[0].attrs.clone()
}

fn para_props(grpprl: Vec<u8>) -> DocSpec {
    let mut spec = DocSpec::ansi("Paragraphe\r");
    spec.papx.push(ParaRunSpec {
        cp_start: 0,
        cp_end: 11,
        istd: 0,
        grpprl,
    });
    spec
}

fn has(doc: &ImportedDoc, n: &ImportNotice) -> bool {
    doc.notices.contains(n)
}

// ----- FIB and text -----------------------------------------------------------

#[test]
fn ansi_text_paragraphs() {
    let doc = open(&DocSpec::ansi("Bonjour le monde\rDeuxieme\r"));
    assert_eq!(texts(&doc), ["Bonjour le monde", "Deuxieme"]);
    assert_eq!(doc.info.version, DocVersion::Word97);
    assert_eq!(doc.info.table_stream, "0Table");
    assert_eq!(doc.info.main_text_chars, 26);
    assert!(!doc.info.fast_saved);
    assert_eq!(
        doc.notices,
        vec![ImportNotice::SummaryInformation {
            problem: "stream absent"
        }]
    );
}

#[test]
fn compressed_high_range_follows_windows_1252() {
    // 0x93 0xE9 0x94 0x97 0x80 0x85 0x99 0xA0 -> “é” — € … ™ NBSP
    let doc = open(&DocSpec::ansi(
        "\u{201C}caf\u{E9}\u{201D} \u{2014} 5\u{20AC}\u{2026}\u{2122}\u{A0}x\r",
    ));
    assert_eq!(
        texts(&doc),
        ["\u{201C}caf\u{E9}\u{201D} \u{2014} 5\u{20AC}\u{2026}\u{2122}\u{A0}x"]
    );
    // The five undefined positions keep their Latin-1 code point.
    let built = DocSpec::ansi("12345\r").streams();
    let mut w = built.word;
    w[TEXT_START..TEXT_START + 6].copy_from_slice(&[0x81, 0x8D, 0x8F, 0x90, 0x9D, 0x0D]);
    let bytes = cfb::Builder::new(
        3,
        vec![
            cfb::stream("WordDocument", &w),
            cfb::stream("0Table", &built.table),
        ],
    )
    .build()
    .bytes;
    let doc = import_doc(&bytes).unwrap();
    assert_eq!(texts(&doc), ["\u{81}\u{8D}\u{8F}\u{90}\u{9D}"]);
}

#[test]
fn unicode_piece_with_surrogates() {
    let doc = open(&DocSpec::new(vec![PieceSpec::uni(
        "\u{65E5}\u{672C} \u{1F600} \u{E9}\r",
    )]));
    assert_eq!(texts(&doc), ["\u{65E5}\u{672C} \u{1F600} \u{E9}"]);
    // ccpText counts UTF-16 units: 2 + 1 + 2 + 1 + 1 + 1.
    assert_eq!(doc.info.main_text_chars, 8);
}

#[test]
fn several_pieces_in_any_file_order() {
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("Hello "),
        PieceSpec::uni("W\u{F6}rld\u{2014}\u{4E16}\r"),
        PieceSpec::ansi("Second\r"),
    ]);
    spec.file_order = Some(vec![2, 0, 1]);
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["Hello W\u{F6}rld\u{2014}\u{4E16}", "Second"]);
}

#[test]
fn other_stories_are_ignored_with_notices() {
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("Corps\r"),
        PieceSpec {
            story: 1,
            ..PieceSpec::ansi("Note\r")
        },
        PieceSpec {
            story: 2,
            ..PieceSpec::ansi("Entete\r\r")
        },
        PieceSpec {
            story: 3,
            ..PieceSpec::ansi("Com\r")
        },
    ]);
    spec.flags = 0;
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["Corps"]);
    assert!(has(
        &doc,
        &ImportNotice::IgnoredText {
            region: TextRegion::Footnotes,
            characters: 5
        }
    ));
    assert!(has(
        &doc,
        &ImportNotice::IgnoredText {
            region: TextRegion::HeadersFooters,
            characters: 8
        }
    ));
    assert!(has(
        &doc,
        &ImportNotice::IgnoredText {
            region: TextRegion::Comments,
            characters: 4
        }
    ));
}

#[test]
fn table_stream_follows_fib_flag() {
    for use_table1 in [false, true] {
        let mut spec = DocSpec::ansi("Choisi\r");
        spec.use_table1 = use_table1;
        spec.decoy_table = true;
        let doc = open(&spec);
        assert_eq!(texts(&doc), ["Choisi"]);
        assert_eq!(
            doc.info.table_stream,
            if use_table1 { "1Table" } else { "0Table" }
        );
    }
}

#[test]
fn fib_variants_and_versions() {
    for (n_fib, new, pairs, version) in [
        (0x00C1, None, 93, DocVersion::Word97),
        (0x00D9, None, 108, DocVersion::Word2000),
        (0x0101, None, 136, DocVersion::Word2002),
        (0x010C, None, 164, DocVersion::Word2003),
        (0x00C1, Some(0x0112), 183, DocVersion::Word2007),
        (0x00C1, None, 200, DocVersion::Word97),
        (0x00C1, None, 34, DocVersion::Word97),
    ] {
        let mut spec = DocSpec::ansi("v\r");
        spec.n_fib = n_fib;
        spec.n_fib_new = new;
        spec.pairs = pairs;
        let doc = open(&spec);
        assert_eq!(texts(&doc), ["v"], "pairs {pairs}");
        assert_eq!(doc.info.version, version);
    }
}

#[test]
fn fib_too_short_for_the_piece_table_is_an_error() {
    let mut spec = DocSpec::ansi("x\r");
    spec.pairs = 20;
    assert_eq!(
        import_doc(&spec.build()).err(),
        Some(DocError::InvalidStructure("empty Clx"))
    );
}

#[test]
fn encrypted_documents_get_a_distinct_error() {
    let mut spec = DocSpec::ansi("secret\r");
    spec.flags = 0x0100;
    assert_eq!(
        import_doc(&spec.build()).err(),
        Some(DocError::PasswordProtected { obfuscated: false })
    );
    spec.flags = 0x8100;
    assert_eq!(
        import_doc(&spec.build()).err(),
        Some(DocError::PasswordProtected { obfuscated: true })
    );
    // fObfuscated alone (without fEncrypted) is ignored.
    spec.flags = 0x8000;
    assert!(import_doc(&spec.build()).is_ok());
    let msg = DocError::PasswordProtected { obfuscated: false }.to_string();
    assert!(msg.contains("protégé par mot de passe"));
}

#[test]
fn old_versions_and_foreign_files_are_typed_errors() {
    let mut spec = DocSpec::ansi("old\r");
    spec.n_fib = 0x0065; // Word 6
    assert_eq!(
        import_doc(&spec.build()).err(),
        Some(DocError::UnsupportedVersion { n_fib: 0x65 })
    );
    spec.n_fib = 0x0068; // Word 95
    assert_eq!(
        import_doc(&spec.build()).err(),
        Some(DocError::UnsupportedVersion { n_fib: 0x68 })
    );
    assert!(matches!(
        import_doc(b"not a compound file at all"),
        Err(DocError::Cfb(_))
    ));
    let no_word = cfb::Builder::new(3, vec![cfb::stream("Workbook", b"xx")])
        .build()
        .bytes;
    assert_eq!(
        import_doc(&no_word).err(),
        Some(DocError::MissingStream("WordDocument"))
    );
    let mut s = DocSpec::ansi("x\r").streams();
    s.word[0] = 0;
    let bad = cfb::Builder::new(
        3,
        vec![
            cfb::stream("WordDocument", &s.word),
            cfb::stream("0Table", &s.table),
        ],
    )
    .build()
    .bytes;
    assert_eq!(import_doc(&bad).err(), Some(DocError::NotWordDocument));
    let s = DocSpec::ansi("x\r").streams();
    let no_table = cfb::Builder::new(3, vec![cfb::stream("WordDocument", &s.word)])
        .build()
        .bytes;
    assert_eq!(
        import_doc(&no_table).err(),
        Some(DocError::MissingStream("0Table"))
    );
}

#[test]
fn fast_saved_flags_are_reported() {
    let mut spec = DocSpec::ansi("x\r");
    spec.flags = 0x0004 | 0x0030;
    let doc = open(&spec);
    assert!(doc.info.fast_saved);
    assert_eq!(texts(&doc), ["x"]);
}

// ----- special characters ---------------------------------------------------------

#[test]
fn special_characters() {
    let doc = open(&DocSpec::ansi(
        "a\tb\u{0B}c\u{0C}d\u{1E}e\u{1F}f\u{A0}g\u{01}\u{08}\u{02}h\ri\u{0C}j\r",
    ));
    // 0x0C at cp 19 is a page break (no section end declared).
    let p = paras(&doc);
    assert_eq!(
        p[0].text,
        "a\tb\nc\nd\u{2011}e\u{AD}f\u{A0}ghn".replace("hn", "h")
    );
    assert_eq!(p[1].text, "i\nj");
    let kinds: Vec<_> = doc
        .breaks
        .iter()
        .map(|b| (b.kind.as_str(), b.break_type.as_deref(), b.offset))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("br", None, 3),
            ("br", Some("page"), 5),
            ("br", Some("page"), 1)
        ]
    );
    for (kind, count) in [
        ("inlinePicture", 1),
        ("drawnObject", 1),
        ("noteReference", 1),
    ] {
        assert!(
            has(&doc, &ImportNotice::ObjectsDropped { kind, count }),
            "{kind}"
        );
    }
}

#[test]
fn section_marks_end_paragraphs() {
    // "ab" 0x0C "cd" 0x0D: the first section ends after cp 3.
    let mut spec = DocSpec::ansi("ab\u{0C}cd\r");
    spec.section_ends = vec![3, 6];
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["ab", "cd"]);
    assert!(
        doc.breaks
            .iter()
            .any(|b| b.kind == "section" && b.offset == 2)
    );
}

#[test]
fn table_cells_become_paragraphs() {
    // Two cells then the row-end mark (all 0x07), then a normal paragraph.
    let mut spec = DocSpec::ansi("one\u{07}two\u{07}\u{07}after\r");
    // Paragraph 3 (the row end mark, cp 8) carries sprmPFTtp.
    spec.papx.push(ParaRunSpec {
        cp_start: 8,
        cp_end: 9,
        istd: 0,
        grpprl: cat(&[sprm_u8(0x2416, 1), sprm_u8(0x2417, 1)]),
    });
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["one", "two", "after"]);
    assert!(has(&doc, &ImportNotice::TableCellsFlattened { cells: 2 }));
}

#[test]
fn fields_keep_results_and_drop_codes() {
    let doc = open(&DocSpec::ansi(
        "Page \u{13} PAGE \u{14}3\u{15} de \u{13}HYPERLINK \"x\" \u{14}\u{13}A\u{14}lien\u{15}\u{15}.\r",
    ));
    assert_eq!(texts(&doc), ["Page 3 de lien."]);
    assert!(has(&doc, &ImportNotice::FieldCodesDropped { fields: 3 }));
    // A field without separator is entirely code.
    let doc = open(&DocSpec::ansi("a\u{13} SEQ x \u{15}b\r"));
    assert_eq!(texts(&doc), ["ab"]);
}

#[test]
fn empty_document_keeps_one_empty_paragraph() {
    let doc = open(&DocSpec::ansi(""));
    assert_eq!(texts(&doc), [""]);
    let doc = open(&DocSpec::ansi("\r\r"));
    assert_eq!(texts(&doc), ["", ""]);
}

#[test]
fn text_without_final_mark_is_kept() {
    let doc = open(&DocSpec::ansi("abc\rdef"));
    assert_eq!(texts(&doc), ["abc", "def"]);
    assert!(has(
        &doc,
        &ImportNotice::DamagedData {
            what: "final paragraph mark missing",
            count: 1
        }
    ));
}

// ----- character formatting ---------------------------------------------------------------

#[test]
fn bold_italic_strike() {
    let doc = open(&char_run(
        "Texte",
        cat(&[sprm_u8(0x0835, 1), sprm_u8(0x0836, 1), sprm_u8(0x0837, 1)]),
    ));
    let a = attrs_of(&doc);
    assert_eq!(
        (a.bold, a.italic, a.strike),
        (Some(true), Some(true), Some(true))
    );
    let doc = open(&char_run("Texte", sprm_u8(0x0835, 0)));
    assert_eq!(attrs_of(&doc).bold, Some(false));
}

#[test]
fn underline_kinds() {
    // `Kul` values: 0x00-0x0B, 0x14, 0x17, 0x19-0x1B, 0x27, 0x2B and 0x37
    // are those written by LibreOffice (tests/doc-reference/underlines.doc).
    for (kul, expected) in [
        (0u8, Underline::None),
        (0x01, Underline::Single),
        (0x02, Underline::Single),
        (0x03, Underline::Double),
        (0x04, Underline::Dotted),
        (0x06, Underline::Thick),
        (0x07, Underline::Dashed),
        (0x09, Underline::Dashed),
        (0x0A, Underline::Dashed),
        (0x0B, Underline::Wave),
        (0x14, Underline::Dotted),
        (0x17, Underline::Dashed),
        (0x19, Underline::Dashed),
        (0x1A, Underline::Dashed),
        (0x1B, Underline::Wave),
        (0x27, Underline::Dashed),
        (0x2B, Underline::Wave),
        (0x37, Underline::Dashed),
        // Regression: these used to be mapped to other styles.
        (0x05, Underline::Single),
        (0x08, Underline::Single),
        (0x0C, Underline::Single),
        (0x7F, Underline::Single),
    ] {
        let doc = open(&char_run("U", sprm_u8(0x2A3E, kul)));
        assert_eq!(attrs_of(&doc).underline, Some(expected), "kul {kul}");
    }
    let doc = open(&char_run("U", sprm_u8(0x2A3E, 2)));
    assert!(has(
        &doc,
        &ImportNotice::ApproximatedProperty {
            name: "wordsOnlyUnderline",
            count: 1
        }
    ));
}

#[test]
fn size_position_and_colours() {
    let a = attrs_of(&open(&char_run("T", sprm_u16(0x4A43, 24))));
    assert_eq!(a.size_half_points, Some(24));
    // Out of range sizes are ignored.
    let doc = open(&char_run("T", sprm_u16(0x4A43, 1)));
    assert!(paras(&doc)[0].runs.is_empty());
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A48, 1))));
    assert_eq!(a.vertical_align, Some(VerticalAlign::Superscript));
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A48, 2))));
    assert_eq!(a.vertical_align, Some(VerticalAlign::Subscript));
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A42, 6))));
    assert_eq!(a.color, Some(Color { r: 255, g: 0, b: 0 }));
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A42, 11))));
    assert_eq!(a.color, Some(Color { r: 0, g: 128, b: 0 }));
    let a = attrs_of(&open(&char_run("T", sprm_u32(0x6870, 0x0056_3412))));
    assert_eq!(
        a.color,
        Some(Color {
            r: 0x12,
            g: 0x34,
            b: 0x56
        })
    );
    // sprmCCv overrides an earlier sprmCIco.
    let a = attrs_of(&open(&char_run(
        "T",
        cat(&[sprm_u8(0x2A42, 6), sprm_u32(0x6870, 0x0000_00FF)]),
    )));
    assert_eq!(a.color, Some(Color { r: 255, g: 0, b: 0 }));
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A0C, 7))));
    assert_eq!(
        a.highlight,
        Some(Color {
            r: 255,
            g: 255,
            b: 0
        })
    );
    let a = attrs_of(&open(&char_run("T", sprm_u8(0x2A0C, 16))));
    assert_eq!(
        a.highlight,
        Some(Color {
            r: 192,
            g: 192,
            b: 192
        })
    );
    // Automatic colour (0xFF in the high byte) cannot be expressed.
    let doc = open(&char_run("T", sprm_u32(0x6870, 0xFF00_0000)));
    assert!(has(
        &doc,
        &ImportNotice::UnsupportedProperty {
            name: "automaticColor",
            count: 1
        }
    ));
}

#[test]
fn fonts_resolve_through_the_font_table() {
    let mut spec = char_run(
        "Police",
        cat(&[
            sprm_u16(0x4A4F, 1),
            sprm_u16(0x4A50, 2),
            sprm_u16(0x4A51, 1),
        ]),
    );
    spec.fonts = vec![
        FontSpec::new("Times New Roman"),
        FontSpec {
            alt: Some("Arial Alt".into()),
            charset: 0xEE,
            ffid: 0x26,
            ..FontSpec::new("Arial")
        },
        FontSpec::new("MS Mincho"),
    ];
    let doc = open(&spec);
    assert_eq!(attrs_of(&doc).font.as_deref(), Some("Arial"));
    assert_eq!(doc.fonts.len(), 3);
    assert_eq!(doc.fonts[1].alternate_name.as_deref(), Some("Arial Alt"));
    assert_eq!(doc.fonts[1].charset, 0xEE);
    assert_eq!(doc.fonts[1].family, 2);
    assert_eq!(doc.fonts[1].pitch, 2);
    assert!(doc.fonts[1].true_type);
    // Only the East Asian slot -> used as a fallback; bad index -> notice.
    let mut spec = char_run("Police", sprm_u16(0x4A50, 2));
    spec.fonts = vec![FontSpec::new("A"), FontSpec::new("B"), FontSpec::new("C")];
    assert_eq!(attrs_of(&open(&spec)).font.as_deref(), Some("C"));
    let mut spec = char_run("Police", sprm_u16(0x4A4F, 9));
    spec.fonts = vec![FontSpec::new("A")];
    let doc = open(&spec);
    assert!(has(
        &doc,
        &ImportNotice::UnsupportedProperty {
            name: "unknownFontIndex",
            count: 1
        }
    ));
}

#[test]
fn unsupported_character_properties_are_opaque_and_noticed() {
    let a = attrs_of(&open(&char_run(
        "T",
        cat(&[
            sprm_u8(0x083A, 1),
            sprm_u8(0x083B, 0),
            sprm_u8(0x083C, 1),
            sprm_i16(0x8840, 20),
            sprm_u8(0x2A53, 1),
        ]),
    )));
    assert_eq!(a.strike, Some(true));
    assert_eq!(
        a.opaque,
        vec![
            opaque("caps", &[0]),
            opaque("doubleStrike", &[1]),
            opaque("hidden", &[1]),
            opaque("smallCaps", &[1]),
            opaque("spacing", &20i16.to_le_bytes()),
        ]
    );
    let doc = open(&char_run(
        "T",
        cat(&[sprm_u8(0x083A, 1), sprm_u8(0x083C, 1)]),
    ));
    for name in ["smallCaps", "hiddenText"] {
        assert!(has(
            &doc,
            &ImportNotice::UnsupportedProperty { name, count: 1 }
        ));
    }
}

#[test]
fn unknown_sprms_are_skipped_without_desynchronising() {
    // Unknown char sprms of every operand size class, then bold.
    let grpprl = cat(&[
        sprm_u8(0x0800 + 0x7F, 1),                 // spra 0
        sprm_u8(0x2A00 + 0x7F, 1),                 // spra 1
        sprm_u16(0x4A00 + 0x7F, 7),                // spra 2
        sprm_u32(0x6A00 + 0x7F, 7),                // spra 3
        sprm_u16(0x8800 + 0x7F, 7),                // spra 4
        sprm_u16(0xA800 + 0x7F, 7),                // spra 5
        sprm_var(0xC800 + 0x7F, &[1, 2, 3, 4, 5]), // spra 6
        {
            let mut v = 0xE87Fu16.to_le_bytes().to_vec(); // spra 7: 3 bytes
            v.extend_from_slice(&[1, 2, 3]);
            v
        },
        {
            // sprmTDefTable-style: u16 cb then cb - 1 bytes.
            let mut v = 0xD608u16.to_le_bytes().to_vec();
            v.extend_from_slice(&5u16.to_le_bytes());
            v.extend_from_slice(&[9, 9, 9, 9]);
            v
        },
        sprm_u8(0x0835, 1),
    ]);
    let doc = open(&char_run("T", grpprl));
    assert_eq!(attrs_of(&doc).bold, Some(true));
    assert!(
        doc.notices
            .contains(&ImportNotice::UnknownSprms { count: 8 })
    );
}

#[test]
fn truncated_property_lists_do_not_panic() {
    // The last sprm is cut: the first one is kept and a notice is emitted.
    let mut g = sprm_u8(0x0835, 1);
    g.extend_from_slice(&0x4A43u16.to_le_bytes());
    g.push(24); // size operand needs two bytes
    let doc = open(&char_run("T", g));
    assert_eq!(attrs_of(&doc).bold, Some(true));
    assert!(has(
        &doc,
        &ImportNotice::DamagedData {
            what: "truncated property list",
            count: 1
        }
    ));
}

#[test]
fn runs_split_at_grapheme_boundaries_and_merge() {
    // "e" + combining acute (2 scalars, 1 grapheme): the run covers only the
    // base letter, formatting extends to the whole grapheme.
    let mut spec = DocSpec::new(vec![PieceSpec::uni("e\u{301}x\r")]);
    spec.chpx.push(CharRunSpec {
        cp_start: 0,
        cp_end: 1,
        grpprl: sprm_u8(0x0835, 1),
    });
    let doc = open(&spec);
    let p = &paras(&doc)[0];
    assert_eq!(p.runs.len(), 1);
    assert_eq!((p.runs[0].start, p.runs[0].end), (0, 3));
    // Adjacent identical runs merge.
    let mut spec = DocSpec::ansi("abcd\r");
    for (a, b) in [(0, 2), (2, 4)] {
        spec.chpx.push(CharRunSpec {
            cp_start: a,
            cp_end: b,
            grpprl: sprm_u8(0x0836, 1),
        });
    }
    let p = paras(&open(&spec)).remove(0);
    assert_eq!(p.runs.len(), 1);
    assert_eq!((p.runs[0].start, p.runs[0].end), (0, 4));
}

#[test]
fn piece_property_modifiers_apply() {
    // Prm1: bold on the whole second piece; a simple Prm0 is reported only.
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("plain "),
        PieceSpec {
            prc: Some(cat(&[sprm_u8(0x0835, 1), sprm_u8(0x2403, 2)])),
            ..PieceSpec::ansi("gras\r")
        },
        PieceSpec {
            raw_prm: Some(0x0302),
            ..PieceSpec::ansi("x\r")
        },
    ]);
    spec.chpx.push(CharRunSpec {
        cp_start: 0,
        cp_end: 6,
        grpprl: sprm_u8(0x0836, 1),
    });
    let doc = open(&spec);
    let p = &paras(&doc)[0];
    assert_eq!(p.text, "plain gras");
    assert_eq!(p.runs.len(), 2);
    assert_eq!(p.runs[0].attrs.italic, Some(true));
    assert_eq!((p.runs[1].start, p.runs[1].end), (6, 10));
    assert_eq!(p.runs[1].attrs.bold, Some(true));
    // The paragraph sprm of the Prc (right alignment) applies to the paragraph
    // whose mark lies in that piece.
    assert_eq!(p.props.alignment, Some(Alignment::Right));
    assert!(has(&doc, &ImportNotice::SimplePrmIgnored { count: 1 }));
}

// ----- paragraph formatting ---------------------------------------------------------------------

#[test]
fn alignment_variants() {
    for (op, v, expected) in [
        (0x2403u16, 0u8, Alignment::Left),
        (0x2403, 1, Alignment::Center),
        (0x2403, 2, Alignment::Right),
        (0x2403, 3, Alignment::Justify),
        (0x2461, 1, Alignment::Center),
        (0x2461, 3, Alignment::Justify),
        (0x2461, 4, Alignment::Justify),
    ] {
        let doc = open(&para_props(sprm_u8(op, v)));
        assert_eq!(
            paras(&doc)[0].props.alignment,
            Some(expected),
            "{op:#x} {v}"
        );
    }
}

#[test]
fn indents_and_spacing() {
    let doc = open(&para_props(cat(&[
        sprm_i16(0x840F, 720),
        sprm_i16(0x840E, 360),
        sprm_i16(0x8411, -360),
        sprm_u16(0xA413, 240),
        sprm_u16(0xA414, 120),
    ])));
    let p = &paras(&doc)[0].props;
    assert_eq!(p.indent_left, Some(720));
    assert_eq!(p.indent_right, Some(360));
    assert_eq!(p.indent_first_line, Some(-360));
    assert_eq!((p.space_before, p.space_after), (Some(240), Some(120)));
    // Word 2000 variants win when they come later.
    let doc = open(&para_props(cat(&[
        sprm_i16(0x840F, 1),
        sprm_i16(0x845E, 1440),
        sprm_i16(0x845D, 100),
        sprm_i16(0x8460, 567),
    ])));
    let p = &paras(&doc)[0].props;
    assert_eq!(
        (p.indent_left, p.indent_right, p.indent_first_line),
        (Some(1440), Some(100), Some(567))
    );
}

#[test]
fn line_spacing_rules() {
    let lspd = |line: i16, mult: u16| {
        let mut v = 0x6412u16.to_le_bytes().to_vec();
        v.extend_from_slice(&line.to_le_bytes());
        v.extend_from_slice(&mult.to_le_bytes());
        v
    };
    for (line, mult, expected) in [
        (
            360,
            1,
            Some(LineSpacing {
                rule: LineRule::Auto,
                value: 360,
            }),
        ),
        (
            240,
            1,
            Some(LineSpacing {
                rule: LineRule::Auto,
                value: 240,
            }),
        ),
        (
            300,
            0,
            Some(LineSpacing {
                rule: LineRule::AtLeast,
                value: 300,
            }),
        ),
        (
            -280,
            0,
            Some(LineSpacing {
                rule: LineRule::Exact,
                value: 280,
            }),
        ),
        (0, 0, None),
    ] {
        let doc = open(&para_props(lspd(line, mult)));
        assert_eq!(paras(&doc)[0].props.line_spacing, expected, "{line} {mult}");
    }
}

#[test]
fn flow_properties_tabs_and_outline_are_opaque() {
    let doc = open(&para_props(cat(&[
        sprm_u8(0x2405, 1),
        sprm_u8(0x2406, 1),
        sprm_u8(0x2407, 1),
        sprm_u8(0x2431, 0),
        sprm_u8(0x2640, 2),
        sprm_var(0xC60D, &[0, 1, 0xD0, 0x02, 0x01]),
    ])));
    let p = &paras(&doc)[0].props;
    assert_eq!(
        p.opaque,
        vec![
            opaque("keepLines", &[1]),
            opaque("keepNext", &[1]),
            opaque("outlineLevel", &[2]),
            opaque("pageBreakBefore", &[1]),
            opaque("tabs", &[5, 0, 1, 0xD0, 0x02, 0x01]),
            opaque("widowControl", &[0]),
        ]
    );
    for name in [
        "keepLinesTogether",
        "keepWithNext",
        "pageBreakBefore",
        "widowOrphanControl",
        "outlineLevel",
        "tabStops",
    ] {
        assert!(
            has(&doc, &ImportNotice::UnsupportedProperty { name, count: 1 }),
            "{name}"
        );
    }
    // Body text outline level (9) is not stored.
    let doc = open(&para_props(sprm_u8(0x2640, 9)));
    assert!(paras(&doc)[0].props.opaque.is_empty());
}

#[test]
fn huge_papx_comes_from_the_data_stream() {
    let mut data = vec![0xAA; 6];
    let off = data.len() as u32;
    let g = cat(&[sprm_u8(0x2403, 2), sprm_u16(0xA413, 99)]);
    data.extend_from_slice(&(g.len() as u16).to_le_bytes());
    data.extend_from_slice(&g);
    let mut spec = para_props(sprm_u32(0x6646, off));
    spec.data = Some(data);
    let doc = open(&spec);
    let p = &paras(&doc)[0].props;
    assert_eq!(p.alignment, Some(Alignment::Right));
    assert_eq!(p.space_before, Some(99));
    // A bad offset is reported and the paragraph is still imported.
    let mut spec = para_props(sprm_u32(0x6646, 9999));
    spec.data = Some(vec![0; 16]);
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["Paragraphe"]);
    assert!(has(
        &doc,
        &ImportNotice::DamagedData {
            what: "huge paragraph properties unreadable",
            count: 1
        }
    ));
}

#[test]
fn long_form_papx_entries() {
    let mut spec = para_props(cat(&[sprm_u8(0x2403, 1), sprm_u16(0xA414, 10)]));
    spec.long_papx = true;
    let p = paras(&open(&spec)).remove(0);
    assert_eq!(p.props.alignment, Some(Alignment::Center));
    assert_eq!(p.props.space_after, Some(10));
}

#[test]
fn table_paragraph_sprms_are_counted_not_applied() {
    let doc = open(&para_props(cat(&[
        sprm_u8(0x2416, 1),
        sprm_var(0xD609, &[1, 2, 3]),
        sprm_u8(0x2403, 1),
    ])));
    assert_eq!(paras(&doc)[0].props.alignment, Some(Alignment::Center));
}

// ----- styles ---------------------------------------------------------------------------------------

fn styled_spec() -> DocSpec {
    let mut spec = DocSpec::ansi("Titre\rCorps gras\rFin\r");
    spec.fonts = vec![FontSpec::new("Times New Roman"), FontSpec::new("Arial")];
    spec.styles = Some(vec![
        // istd 0
        StdSpec::para(
            0,
            "Normal",
            0x0FFF,
            sprm_u16(0xA414, 100),
            cat(&[sprm_u16(0x4A43, 22), sprm_u16(0x4A4F, 0)]),
        ),
        // istd 1: Heading 1 based on Normal
        StdSpec::para(
            1,
            "Titre 1",
            0,
            cat(&[sprm_u8(0x2403, 1), sprm_u16(0xA413, 240)]),
            cat(&[
                sprm_u8(0x0835, 1),
                sprm_u16(0x4A43, 32),
                sprm_u16(0x4A4F, 1),
            ]),
        ),
        // istd 2: custom style based on Heading 1, bold toggled off relative to it.
        StdSpec::para(
            0x0FFE,
            "Mon style",
            1,
            sprm_i16(0x840F, 500),
            sprm_u8(0x0835, 0x81),
        ),
        // istd 3: character style "Strong"
        StdSpec::chr(87, "Strong", 0x0FFF, sprm_u8(0x0835, 1)),
        // istd 4: empty slot
        StdSpec::para(0, "<empty>", 0x0FFF, vec![], vec![]),
        // istd 5: table style (ignored)
        StdSpec {
            sgc: 3,
            ..StdSpec::para(105, "Table Normal", 0x0FFF, vec![], vec![])
        },
    ]);
    spec.papx = vec![
        ParaRunSpec {
            cp_start: 0,
            cp_end: 6,
            istd: 1,
            grpprl: vec![],
        },
        ParaRunSpec {
            cp_start: 6,
            cp_end: 17,
            istd: 2,
            grpprl: vec![],
        },
    ];
    spec.chpx = vec![CharRunSpec {
        cp_start: 12,
        cp_end: 16,
        grpprl: sprm_u16(0x4A30, 3),
    }];
    spec
}

#[test]
fn style_sheet_is_converted_to_the_engine() {
    let mut doc = open(&styled_spec());
    let eng = doc.styles.catalog();
    let h1 = eng.style("Heading1").expect("Heading1");
    assert_eq!(h1.based_on.as_deref(), Some("Normal"));
    assert_eq!(h1.paragraph.alignment, Some(Alignment::Center));
    assert_eq!(h1.paragraph.space_before, Some(240));
    assert_eq!(h1.character.values.bold, Some(true));
    assert_eq!(h1.character.values.size_half_points, Some(32));
    assert_eq!(h1.character.values.font.as_deref(), Some("Arial"));
    // Bold "opposite of style" on top of a bold base -> false.
    let custom = eng
        .style("Monstyle")
        .expect("custom id is the sanitised name");
    assert_eq!(custom.based_on.as_deref(), Some("Heading1"));
    assert_eq!(custom.character.values.bold, Some(false));
    assert_eq!(custom.paragraph.indent_left, Some(500));
    let strong = eng.style("Strong").unwrap();
    assert_eq!(strong.character.values.bold, Some(true));
    assert!(eng.style("TableNormal").is_none());
    assert_eq!(
        eng.defaults().character.values.font.as_deref(),
        Some("Times New Roman")
    );
    assert_eq!(eng.defaults().character.values.size_half_points, Some(20));
    assert!(has(
        &doc,
        &ImportNotice::UnsupportedStyleKind {
            istd: 5,
            kind: DocStyleKind::Table
        }
    ));
    let names: Vec<_> = doc
        .style_records
        .iter()
        .map(|r| (r.istd, r.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            (0, "Normal"),
            (1, "Titre 1"),
            (2, "Mon style"),
            (3, "Strong"),
            (5, "Table Normal")
        ]
    );
    // Paragraph styles and effective properties.
    let ids = doc.paragraph_ids().to_vec();
    let styles: Vec<_> = paras(&doc).iter().map(|p| p.props.style.clone()).collect();
    assert_eq!(
        styles,
        vec![
            Some("Heading1".to_string()),
            Some("Monstyle".to_string()),
            Some("Normal".to_string())
        ]
    );
    let (p, def) = doc.resolved_paragraph(ids[0]).unwrap();
    assert_eq!(def.paragraph.alignment, Some(Alignment::Center));
    assert_eq!(def.paragraph.space_after, Some(100));
    assert_eq!(def.character.size_half_points, Some(32));
    assert_eq!(p.props.alignment, Some(Alignment::Center));
    // The custom style turns bold off; the character style turns it on again.
    let (p, _) = doc.resolved_paragraph(ids[1]).unwrap();
    assert_eq!(p.text, "Corps gras");
    let bold_at = |off: usize| {
        p.runs
            .iter()
            .find(|r| r.start <= off && off < r.end)
            .and_then(|r| r.attrs.bold)
    };
    assert_eq!(bold_at(0), Some(false));
    assert_eq!(bold_at(6), Some(true));
    assert_eq!(doc.run_styles.len(), 1);
    assert_eq!(doc.run_styles[0].style_id.as_deref(), Some("Strong"));
    assert_eq!((doc.run_styles[0].start, doc.run_styles[0].end), (6, 10));
}

#[test]
fn relative_toggles_in_direct_formatting() {
    // Heading 1 is bold; direct "opposite" (0x81) -> not bold, "same" (0x80) -> inherited.
    let mut spec = styled_spec();
    spec.chpx = vec![
        CharRunSpec {
            cp_start: 0,
            cp_end: 2,
            grpprl: sprm_u8(0x0835, 0x81),
        },
        CharRunSpec {
            cp_start: 2,
            cp_end: 5,
            grpprl: sprm_u8(0x0835, 0x80),
        },
    ];
    let doc = open(&spec);
    let p = &paras(&doc)[0];
    assert_eq!(p.runs.len(), 1);
    assert_eq!(p.runs[0].attrs.bold, Some(false));
    assert_eq!((p.runs[0].start, p.runs[0].end), (0, 2));
}

#[test]
fn style_cycles_and_bad_references_do_not_loop() {
    let mut spec = DocSpec::ansi("a\rb\r");
    spec.styles = Some(vec![
        StdSpec::para(0, "Normal", 0x0FFF, vec![], vec![]),
        StdSpec::para(0x0FFE, "A", 2, vec![], sprm_u8(0x0836, 1)), // 1 -> 2
        StdSpec::para(0x0FFE, "B", 1, vec![], vec![]),             // 2 -> 1 (cycle)
        StdSpec::para(0x0FFE, "C", 3, vec![], vec![]),             // self reference
        StdSpec::para(0x0FFE, "D", 77, vec![], vec![]),            // missing base
        StdSpec::chr(0x0FFE, "E", 0, vec![]),                      // kind mismatch
    ]);
    spec.papx = vec![
        ParaRunSpec {
            cp_start: 0,
            cp_end: 2,
            istd: 1,
            grpprl: vec![],
        },
        ParaRunSpec {
            cp_start: 2,
            cp_end: 4,
            istd: 999,
            grpprl: vec![],
        },
    ];
    let mut doc = open(&spec);
    let reasons: Vec<_> = doc
        .notices
        .iter()
        .filter_map(|n| match n {
            ImportNotice::InvalidStyleReference { istd, reason } => Some((*istd, *reason)),
            _ => None,
        })
        .collect();
    assert!(reasons.contains(&(3, "base style missing, unsupported or of another kind")));
    assert!(reasons.contains(&(4, "base style missing, unsupported or of another kind")));
    assert!(reasons.contains(&(5, "base style missing, unsupported or of another kind")));
    assert!(reasons.iter().any(|(_, r)| *r == "base style cycle"));
    // Resolution terminates for every paragraph.
    for id in doc.paragraph_ids().to_vec() {
        doc.resolved_paragraph(id).unwrap();
    }
    // Invalid paragraph style index falls back to Normal.
    assert_eq!(paras(&doc)[1].props.style.as_deref(), Some("Normal"));
    assert!(has(
        &doc,
        &ImportNotice::DamagedData {
            what: "paragraph refers to a missing style",
            count: 1
        }
    ));
}

#[test]
fn custom_style_names_are_made_unique() {
    let mut spec = DocSpec::ansi("a\r");
    spec.styles = Some(vec![
        StdSpec::para(0, "Normal", 0x0FFF, vec![], vec![]),
        StdSpec::para(0x0FFE, "My Style", 0, vec![], vec![]),
        StdSpec::para(0x0FFE, "MyStyle", 0, vec![], vec![]),
        StdSpec::para(0x0FFE, "Heading1", 0, vec![], vec![]),
        StdSpec::para(0x0FFE, "", 0, vec![], vec![]),
    ]);
    let doc = open(&spec);
    let ids: Vec<_> = doc
        .style_records
        .iter()
        .map(|r| r.id.clone().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["Normal", "MyStyle", "MyStyle_2", "Heading1", "Style4"]
    );
}

// ----- document properties ------------------------------------------------------------------------------

#[test]
fn summary_information_properties() {
    let mut spec = DocSpec::ansi("x\r");
    spec.summary = Some(build_summary(&[
        Prop::CodePage(1252),
        Prop::Str(2, "R\u{E9}sum\u{E9} \u{20AC}".into()),
        Prop::Str(3, "Sujet".into()),
        Prop::Wide(4, "Zo\u{EB} \u{65E5}\u{672C}".into()),
        Prop::Str(5, "a; b".into()),
        Prop::Str(6, "note".into()),
        Prop::Str(8, "Dernier".into()),
        Prop::Str(18, "Microsoft Word 9.0".into()),
        Prop::Time(12, 132_000_000_000_000_000),
        Prop::Time(13, 132_000_000_010_000_000),
        Prop::Time(11, 0),
    ]));
    let doc = open(&spec);
    let p = &doc.properties;
    assert_eq!(p.title.as_deref(), Some("R\u{E9}sum\u{E9} \u{20AC}"));
    assert_eq!(p.subject.as_deref(), Some("Sujet"));
    assert_eq!(p.author.as_deref(), Some("Zo\u{EB} \u{65E5}\u{672C}"));
    assert_eq!(p.keywords.as_deref(), Some("a; b"));
    assert_eq!(p.comments.as_deref(), Some("note"));
    assert_eq!(p.last_author.as_deref(), Some("Dernier"));
    assert_eq!(p.application.as_deref(), Some("Microsoft Word 9.0"));
    assert_eq!(p.created, Some(132_000_000_000_000_000));
    assert_eq!(p.modified, Some(132_000_000_010_000_000));
    assert_eq!(p.last_printed, None);
    // 132_000_000_000_000_000 / 10^7 - 11_644_473_600 = 1_555_526_400 (2019-04-17).
    assert_eq!(
        atelier_fmt_legacy::doc::DocumentProperties::filetime_to_unix_seconds(p.created.unwrap()),
        Some(1_555_526_400)
    );
    assert!(doc.notices.is_empty(), "{:?}", doc.notices);
}

#[test]
fn damaged_summary_information_gives_empty_properties() {
    let mut spec = DocSpec::ansi("x\r");
    spec.summary = Some(vec![1, 2, 3]);
    let doc = open(&spec);
    assert!(doc.properties.is_empty());
    assert!(has(
        &doc,
        &ImportNotice::SummaryInformation {
            problem: "bad byte order mark"
        }
    ));
    let mut s = build_summary(&[Prop::Str(2, "t".into())]);
    s.truncate(s.len() - 6);
    spec.summary = Some(s);
    let doc = open(&spec);
    assert!(doc.properties.title.is_none());
    // UTF-8 code page.
    spec.summary = Some(build_summary(&[
        Prop::CodePage(65001),
        Prop::Str(2, "abc".into()),
    ]));
    assert_eq!(open(&spec).properties.title.as_deref(), Some("abc"));
}

// ----- more layouts ------------------------------------------------------------------------------

#[test]
fn formatting_runs_across_pieces_stored_out_of_order() {
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("abc"),
        PieceSpec::uni("d\u{E9}f"),
        PieceSpec::ansi("ghi\r"),
    ]);
    spec.file_order = Some(vec![2, 1, 0]);
    // Italic over "c", "d\u{e9}f" and "g": spans three pieces of two widths.
    spec.chpx.push(CharRunSpec {
        cp_start: 2,
        cp_end: 7,
        grpprl: sprm_u8(0x0836, 1),
    });
    let doc = open(&spec);
    let p = &paras(&doc)[0];
    assert_eq!(p.text, "abcd\u{E9}fghi");
    assert_eq!(p.runs.len(), 1);
    assert_eq!(p.runs[0].attrs.italic, Some(true));
    // "c" .. "g" = bytes 2..(3 + 3 + 1 + 1 = byte offset of "h" counting \u{E9} as two bytes).
    assert_eq!(&p.text[p.runs[0].start..p.runs[0].end], "cd\u{E9}fg");
}

#[test]
fn long_pchgtabs_operand_does_not_desynchronise() {
    // sprmPChgTabs, 255 form: cb, itbdDelMax = 1 (2 + 2 bytes), itbdAddMax = 2
    // (two 2-byte positions, two 1-byte descriptors) = 13 operand bytes.
    let mut g = 0xC615u16.to_le_bytes().to_vec();
    g.extend_from_slice(&[255, 1, 0, 0, 0, 0, 2, 10, 0, 20, 0, 1, 1]);
    g.extend_from_slice(&sprm_u8(0x2403, 2));
    let doc = open(&para_props(g));
    assert_eq!(paras(&doc)[0].props.alignment, Some(Alignment::Right));
}

#[test]
fn toggles_same_as_style_keep_the_inherited_value() {
    let mut spec = styled_spec();
    // Heading 1 (bold) with an explicit "same as style" bold: no direct value.
    spec.chpx = vec![CharRunSpec {
        cp_start: 0,
        cp_end: 5,
        grpprl: sprm_u8(0x0835, 0x80),
    }];
    let mut doc = open(&spec);
    let ids = doc.paragraph_ids().to_vec();
    assert!(paras(&doc)[0].runs.is_empty());
    let (p, _) = doc.resolved_paragraph(ids[0]).unwrap();
    assert!(
        p.runs
            .iter()
            .all(|r| r.attrs.bold.is_none() || r.attrs.bold == Some(true))
    );
}

#[test]
fn symbol_characters_use_the_private_use_area_and_their_font() {
    // "a(b": the "(" placeholder carries sprmCSymbol { ftc 1, code 0xE0 }.
    let mut op = 0x6A09u16.to_le_bytes().to_vec();
    op.extend_from_slice(&[1, 0, 0xE0, 0]);
    let mut spec = DocSpec::ansi("a(b\r");
    spec.fonts = vec![FontSpec::new("Times New Roman"), FontSpec::new("Wingdings")];
    spec.chpx.push(CharRunSpec {
        cp_start: 1,
        cp_end: 2,
        grpprl: op,
    });
    let doc = open(&spec);
    let p = &paras(&doc)[0];
    assert_eq!(p.text, "a\u{F0E0}b");
    assert_eq!(p.runs.len(), 1);
    assert_eq!(p.runs[0].attrs.font.as_deref(), Some("Wingdings"));
    assert_eq!(&p.text[p.runs[0].start..p.runs[0].end], "\u{F0E0}");
    assert!(has(
        &doc,
        &ImportNotice::ApproximatedProperty {
            name: "symbolCharacter",
            count: 1
        }
    ));
}
