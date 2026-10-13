// SPDX-License-Identifier: AGPL-3.0-or-later
//! Text files (US-316): opening, saving, flattening and the loss report, with
//! expected values worked out by hand.

mod common;

use atelier_text_model::plain_io::*;
use atelier_text_model::rich::*;
use atelier_text_model::{PlainTextDocument, PlainTextLimits};
use atelier_unicode::Encoding;
use common::*;

fn open(bytes: &[u8]) -> OpenedText {
    open_text(bytes, &OpenOptions::default()).unwrap()
}

fn open_as(bytes: &[u8], encoding: Encoding) -> OpenedText {
    let options = OpenOptions {
        encoding: Some(encoding),
        ..OpenOptions::default()
    };
    open_text(bytes, &options).unwrap()
}

fn save(text: &str, encoding: SaveEncoding, eol: LineEndingChoice) -> Vec<u8> {
    let options = SaveOptions {
        encoding,
        line_ending: eol,
        ..SaveOptions::default()
    };
    save_text(text, &options).unwrap().bytes
}

// ----- opening ---------------------------------------------------------------

#[test]
fn detection_and_confidence() {
    let cases: [(&[u8], Encoding, Confidence, bool); 9] = [
        (b"", Encoding::Utf8, Confidence::Indifferent, false),
        (b"a\nb", Encoding::Utf8, Confidence::Indifferent, false),
        (b"\xEF\xBB\xBFhi", Encoding::Utf8, Confidence::Certain, true),
        ("café".as_bytes(), Encoding::Utf8, Confidence::Likely, false),
        (
            b"caf\xE9",
            Encoding::Windows1252,
            Confidence::Uncertain,
            false,
        ),
        (
            b"\xFF\xFEh\0\xE9\0",
            Encoding::Utf16Le,
            Confidence::Certain,
            true,
        ),
        (
            b"\xFE\xFF\0h\0\xE9",
            Encoding::Utf16Be,
            Confidence::Certain,
            true,
        ),
        (b"h\0i\0\n\0", Encoding::Utf16Le, Confidence::Likely, false),
        (b"\0h\0i\0\n", Encoding::Utf16Be, Confidence::Likely, false),
    ];
    for (bytes, encoding, confidence, bom) in cases {
        let opened = open(bytes);
        assert_eq!(
            (opened.encoding, opened.confidence, opened.bom),
            (encoding, confidence, bom),
            "{bytes:?}"
        );
    }
    assert_eq!(open(b"\xEF\xBB\xBFhi").text, "hi");
    assert_eq!(open(b"\xFF\xFEh\0\xE9\0").text, "hé");
    assert_eq!(open(b"\xFE\xFF\0h\0\xE9").text, "hé");
    assert_eq!(open(b"h\0i\0\n\0").text, "hi\n");
    assert_eq!(open(b"caf\xE9").text, "café");
    // Manual choice overrides detection.
    let forced = open_as("café".as_bytes(), Encoding::Windows1252);
    assert_eq!(forced.text, "cafÃ©");
    assert_eq!(forced.confidence, Confidence::Chosen);
    assert!(open(b"caf\xE9").suggest_manual_choice());
    assert!(!open("café".as_bytes()).suggest_manual_choice());
}

#[test]
fn utf16_pattern_needs_enough_evidence() {
    // One unit: too short to call it UTF-16.
    assert_eq!(open(b"a\0").encoding, Encoding::Utf8);
    // Nulls on both sides: not UTF-16.
    assert_eq!(open(b"\0\0\0\0\0\0").confidence, Confidence::Indifferent);
    // Non-Latin UTF-16 without BOM has no null bytes: not recognised.
    let chinese = [0x2D, 0x4E, 0x87, 0x65]; // U+4E2D U+6587 little-endian
    assert_eq!(open(&chinese).encoding, Encoding::Windows1252);
}

#[test]
fn invalid_sequences_are_replaced_and_counted() {
    let utf8 = open_as(b"a\xF0\x28\x8C\x28z", Encoding::Utf8);
    assert_eq!(utf8.text, "a\u{FFFD}(\u{FFFD}(z");
    assert_eq!(utf8.invalid_sequences, 2);
    assert!(utf8.suggest_manual_choice() && !utf8.round_trip_is_exact());
    // A genuine U+FFFD is valid UTF-8 and is not counted.
    assert_eq!(open("\u{FFFD}".as_bytes()).invalid_sequences, 0);
    // Odd length: the last byte is one invalid sequence.
    let odd = open_as(&[0x61, 0x00, 0x62], Encoding::Utf16Le);
    assert_eq!((odd.text.as_str(), odd.invalid_sequences), ("a\u{FFFD}", 1));
    // Isolated surrogate, then a letter.
    let lone = open_as(&[0x00, 0xD8, 0x41, 0x00], Encoding::Utf16Le);
    assert_eq!(
        (lone.text.as_str(), lone.invalid_sequences),
        ("\u{FFFD}A", 1)
    );
    // A valid surrogate pair (U+1F600).
    let pair = open_as(&[0x3D, 0xD8, 0x00, 0xDE], Encoding::Utf16Le);
    assert_eq!((pair.text.as_str(), pair.invalid_sequences), ("😀", 0));
}

#[test]
fn windows_1252_c1_range() {
    let bytes: Vec<u8> = (0x80..=0x9F).collect();
    let opened = open_as(&bytes, Encoding::Windows1252);
    let expected = "€\u{FFFD}‚ƒ„…†‡ˆ‰Š‹Œ\u{FFFD}Ž\u{FFFD}\u{FFFD}‘’“”•–—˜™š›œ\u{FFFD}žŸ";
    assert_eq!(opened.text, expected);
    // 0x81, 0x8D, 0x8F, 0x90, 0x9D.
    assert_eq!(opened.invalid_sequences, 5);
    // Saving: the five replacement characters cannot be written back.
    let options = SaveOptions {
        encoding: SaveEncoding::Windows1252,
        unmappable: Unmappable::Replace,
        ..SaveOptions::default()
    };
    let saved = save_text(&opened.text, &options).unwrap();
    assert_eq!(saved.replaced, 5);
    let mut expected_bytes = bytes.clone();
    for undefined in [0x81, 0x8D, 0x8F, 0x90, 0x9D] {
        expected_bytes[undefined - 0x80] = b'?';
    }
    assert_eq!(saved.bytes, expected_bytes);
    // Every defined byte round-trips.
    let defined = save_text("€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ", &options).unwrap();
    assert_eq!(defined.replaced, 0);
}

#[test]
fn line_endings_and_line_model() {
    let crlf = open(b"a\r\nb\r\nc");
    assert_eq!(crlf.text, "a\nb\nc");
    assert_eq!(
        crlf.line_endings,
        LineEndings {
            lf: 0,
            crlf: 2,
            cr: 0
        }
    );
    assert_eq!(crlf.line_endings.style(), LineEndingStyle::CrLf);
    let cr = open(b"a\rb");
    assert_eq!(
        (cr.text.as_str(), cr.line_endings.style()),
        ("a\nb", LineEndingStyle::Cr)
    );
    let lf = open(b"a\nb\n");
    assert_eq!(lf.line_endings.style(), LineEndingStyle::Lf);
    let mixed = open(b"a\r\nb\nc\rd");
    assert_eq!(mixed.text, "a\nb\nc\nd");
    assert_eq!(
        mixed.line_endings,
        LineEndings {
            lf: 1,
            crlf: 1,
            cr: 1
        }
    );
    assert_eq!(mixed.line_endings.style(), LineEndingStyle::Mixed);
    assert_eq!(mixed.line_endings.dominant(), Eol::CrLf);
    assert!(!mixed.round_trip_is_exact());
    // "\n\r" is an LF then a CR, not a CRLF.
    assert_eq!(
        open(b"a\n\rb").line_endings,
        LineEndings {
            lf: 1,
            crlf: 0,
            cr: 1
        }
    );
    // No break: LF by default.
    assert_eq!(open(b"abc").line_endings.style(), LineEndingStyle::None);
    assert_eq!(open(b"abc").format().eol, Eol::Lf);

    // Empty file, one character, with and without the final break.
    assert_eq!(open(b"").lines().collect::<Vec<_>>(), [""]);
    assert_eq!(open(b"x").lines().collect::<Vec<_>>(), ["x"]);
    assert_eq!(open(b"a").lines().collect::<Vec<_>>(), ["a"]);
    assert_eq!(open(b"a\n").lines().collect::<Vec<_>>(), ["a", ""]);
    assert_eq!(
        open(b"a\r\n\r\n").lines().collect::<Vec<_>>(),
        ["a", "", ""]
    );
}

#[test]
fn control_characters_are_kept_and_counted() {
    // TAB is ordinary; NUL, form feed (0x0C) and DEL are kept and counted.
    let opened = open_as(b"a\tb\0c\x0Cd\x7F", Encoding::Utf8);
    assert_eq!(opened.text, "a\tb\0c\x0Cd\x7F");
    assert_eq!(opened.control_chars, 3);
}

#[test]
fn size_limits_are_checked_before_decoding() {
    let options = OpenOptions {
        max_bytes: 4,
        ..OpenOptions::default()
    };
    assert_eq!(
        open_text(b"12345", &options),
        Err(TextIoError::TooLarge {
            limit: 4,
            actual: 5
        })
    );
    assert!(open_text(b"1234", &options).is_ok());
    let save_options = SaveOptions {
        max_bytes: 3,
        ..SaveOptions::default()
    };
    assert_eq!(
        save_text("abcd", &save_options),
        Err(TextIoError::TooLarge {
            limit: 3,
            actual: 4
        })
    );
    let opened = open(b"a\nb\nc");
    assert_eq!(
        opened.to_rich_document(2).unwrap_err(),
        TextIoError::TooManyLines {
            limit: 2,
            actual: 3
        }
    );
    let tiny = PlainTextLimits {
        max_text_bytes: 2,
        max_history_bytes: 1024,
    };
    assert!(matches!(
        opened.to_plain_document(tiny),
        Err(TextIoError::Plain(_))
    ));
}

// ----- saving ------------------------------------------------------------------

#[test]
fn saved_bytes_per_encoding() {
    use LineEndingChoice::Lf;
    use SaveEncoding::*;
    assert_eq!(save("aé", Utf8, Lf), [0x61, 0xC3, 0xA9]);
    assert_eq!(
        save("aé", Utf8Bom, Lf),
        [0xEF, 0xBB, 0xBF, 0x61, 0xC3, 0xA9]
    );
    assert_eq!(
        save("aé", Utf16Le, Lf),
        [0xFF, 0xFE, 0x61, 0x00, 0xE9, 0x00]
    );
    assert_eq!(
        save("aé", Utf16Be, Lf),
        [0xFE, 0xFF, 0x00, 0x61, 0x00, 0xE9]
    );
    assert_eq!(save("aé", Windows1252, Lf), [0x61, 0xE9]);
    // U+1F600 is the surrogate pair D83D DE00.
    assert_eq!(
        save("😀", Utf16Le, Lf),
        [0xFF, 0xFE, 0x3D, 0xD8, 0x00, 0xDE]
    );
    assert_eq!(
        save("😀", Utf16Be, Lf),
        [0xFE, 0xFF, 0xD8, 0x3D, 0xDE, 0x00]
    );
    assert_eq!(save("", Utf8, Lf), []);
    assert_eq!(save("", Utf16Le, Lf), [0xFF, 0xFE]);
}

#[test]
fn saved_line_endings() {
    use LineEndingChoice::*;
    use SaveEncoding::*;
    assert_eq!(save("x\ny", Utf8, CrLf), b"x\r\ny");
    assert_eq!(save("x\ny", Utf8, Lf), b"x\ny");
    // Any input style is converted; a blank line stays a blank line.
    assert_eq!(save("x\r\n\r\ny\rz", Utf8, Lf), b"x\n\ny\nz");
    assert_eq!(save("x\r\n\r\ny\rz", Utf8, CrLf), b"x\r\n\r\ny\r\nz");
    assert_eq!(
        save("x\ny", Utf16Le, CrLf),
        [0xFF, 0xFE, 0x78, 0, 0x0D, 0, 0x0A, 0, 0x79, 0]
    );
    assert_eq!(save("a\n", Utf8, CrLf), b"a\r\n");
    // As opened: the dominant style, including a lone CR.
    let origin = open(b"a\rb\r").format();
    assert_eq!(origin.eol, Eol::Cr);
    assert_eq!(
        save_text("p\nq", &SaveOptions::as_opened(origin))
            .unwrap()
            .bytes,
        b"p\rq"
    );
}

#[test]
fn unrepresentable_characters_in_both_modes() {
    let text = "a→b\nc😀é";
    let options = SaveOptions {
        encoding: SaveEncoding::Windows1252,
        ..SaveOptions::default()
    };
    let Err(TextIoError::Unrepresentable {
        encoding,
        characters,
        total,
    }) = save_text(text, &options)
    else {
        panic!("should fail");
    };
    assert_eq!(encoding, Encoding::Windows1252);
    assert_eq!(total, 2);
    assert_eq!(
        characters,
        [
            Unrepresentable {
                character: '→',
                line: 1,
                column: 2
            },
            Unrepresentable {
                character: '😀',
                line: 2,
                column: 2
            },
        ]
    );
    // The first listed character is the one the encoder reports (byte 1).
    assert_eq!(
        atelier_unicode::encode(text, Encoding::Windows1252)
            .unwrap_err()
            .utf8_position,
        1
    );
    // Positions do not depend on the break style of the input.
    let crlf = text.replace('\n', "\r\n");
    let Err(TextIoError::Unrepresentable {
        characters: again, ..
    }) = save_text(&crlf, &options)
    else {
        panic!("should fail");
    };
    assert_eq!(again, characters);

    let replacing = SaveOptions {
        unmappable: Unmappable::Replace,
        ..options
    };
    let saved = save_text(text, &replacing).unwrap();
    assert_eq!(saved.bytes, [0x61, 0x3F, 0x62, 0x0A, 0x63, 0x3F, 0xE9]);
    assert_eq!(saved.replaced, 2);
    // Unicode encodings never lose anything.
    for encoding in [
        SaveEncoding::Utf8,
        SaveEncoding::Utf16Le,
        SaveEncoding::Utf16Be,
    ] {
        let options = SaveOptions {
            encoding,
            ..SaveOptions::default()
        };
        assert_eq!(save_text(text, &options).unwrap().replaced, 0);
    }
}

#[test]
fn the_error_list_is_capped_but_the_total_is_exact() {
    let text = "→".repeat(MAX_LISTED_CHARACTERS + 5);
    let options = SaveOptions {
        encoding: SaveEncoding::Windows1252,
        ..SaveOptions::default()
    };
    let Err(TextIoError::Unrepresentable {
        characters, total, ..
    }) = save_text(&text, &options)
    else {
        panic!("should fail");
    };
    assert_eq!(
        (characters.len(), total),
        (MAX_LISTED_CHARACTERS, MAX_LISTED_CHARACTERS + 5)
    );
}

// ----- round trips ---------------------------------------------------------------

#[test]
fn open_then_save_as_opened_is_exact() {
    let samples: [&[u8]; 13] = [
        b"",
        b"x",
        b"a\nb",
        b"a\nb\n",
        b"a\r\nb\r\n\r\n",
        b"a\rb\r",
        b"\xEF\xBB\xBFcaf\xC3\xA9\r\n",
        b"caf\xE9 \x80 \x9F\n",
        b"\xFF\xFEh\0\xE9\0\r\0\n\0",
        b"\xFE\xFF\0h\0\xE9\0\n",
        b"h\0\xE9\0\n\0",
        b"\0h\0\xE9\0\n",
        b"\xEF\xBB\xBF\xEF\xBB\xBFx",
    ];
    for bytes in samples {
        let opened = open(bytes);
        assert!(opened.round_trip_is_exact(), "{bytes:?}");
        let saved = save_text(&opened.text, &SaveOptions::as_opened(opened.format())).unwrap();
        assert_eq!(saved.bytes, bytes, "{bytes:?}");
    }
}

#[test]
fn plain_document_target() {
    let opened = open(b"\xEF\xBB\xBFa\r\nb\r\n");
    let doc = opened
        .to_plain_document(PlainTextLimits::default())
        .unwrap();
    assert_eq!(doc.text(), "a\nb\n");
    // The BOM is recorded the way the editor does.
    assert_eq!(doc.export_utf8().unwrap(), b"\xEF\xBB\xBFa\nb\n");
    let saved = save_plain_document(&doc, &SaveOptions::as_opened(opened.format())).unwrap();
    assert_eq!(saved.bytes, b"\xEF\xBB\xBFa\r\nb\r\n");
    // A second BOM is text, and stays text.
    let double = open(b"\xEF\xBB\xBF\xEF\xBB\xBFx");
    let doc = double
        .to_plain_document(PlainTextLimits::default())
        .unwrap();
    assert_eq!(doc.text(), "\u{FEFF}x");
    // An edited document saves its current text, with whatever breaks it holds.
    let mut doc = PlainTextDocument::from_utf8(b"p\r\nq", PlainTextLimits::default()).unwrap();
    doc.insert_text("é", 0).unwrap();
    let out = save_plain_document(&doc, &SaveOptions::default()).unwrap();
    assert_eq!(out.bytes, "ép\nq".as_bytes());
}

#[test]
fn rich_document_target_one_paragraph_per_line() {
    for (bytes, lines) in [
        (&b""[..], vec![""]),
        (b"a", vec!["a"]),
        (b"a\nb", vec!["a", "b"]),
        (b"a\r\nb\r\n", vec!["a", "b", ""]),
        (b"\n\n", vec!["", "", ""]),
    ] {
        let opened = open(bytes);
        let doc = opened.to_rich_document(1000).unwrap();
        doc.validate().unwrap();
        let texts: Vec<String> = doc
            .paragraph_ids()
            .iter()
            .map(|id| doc.paragraph(*id).unwrap().text.clone())
            .collect();
        assert_eq!(texts, lines, "{bytes:?}");
        assert!(doc.paragraph_ids().iter().all(|id| {
            let p = doc.paragraph(*id).unwrap();
            p.props.is_empty() && p.runs.is_empty()
        }));
        let saved = save_rich_document(&doc, &SaveOptions::as_opened(opened.format())).unwrap();
        assert_eq!(saved.bytes, bytes, "{bytes:?}");
    }
    // The built document can be edited through the normal operations.
    let mut doc = open(b"ab\ncd").to_rich_document(10).unwrap();
    let ids = doc.paragraph_ids();
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: ids[1],
            offset: 0,
            text: "X".into(),
        },
    );
    assert_eq!(rich_to_text(&doc), "ab\nXcd");
}

// ----- rich export and loss report ------------------------------------------------------

fn cell_paragraph(doc: &RichDocument, cell: NodeId) -> NodeId {
    match doc.node(cell) {
        Some(Node::Cell(c)) => c.blocks.get(0).unwrap(),
        _ => panic!("not a cell"),
    }
}

#[test]
fn formatting_loss_lists_every_category() {
    let mut doc = RichDocument::new();
    let p0 = first_para(&doc);
    let section = first_section(&doc);
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para: p0,
            offset: 0,
            text: "Bonjour le monde".into(),
        },
    );
    // Two character runs.
    for (start, end, attrs) in [(0, 7, bold()), (8, 10, italic())] {
        run_checked(
            &mut doc,
            RichOp::SetCharAttrs {
                para: p0,
                start,
                end,
                set: attrs,
                clear: CharAttrs::default(),
            },
        );
    }
    // Paragraph property + style + list.
    run_checked(
        &mut doc,
        RichOp::SetParaProps {
            para: p0,
            set: ParaProps {
                alignment: Some(Alignment::Center),
                style: Some("atelier.heading.1".into()),
                list: Some(ListRef {
                    list_id: 1,
                    level: 0,
                }),
                ..ParaProps::default()
            },
            clear: ParaProps::default(),
        },
    );
    let (field, link, bookmark, object) = (
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
        doc.alloc_id().unwrap(),
    );
    for item in [
        Inline::Field(Field {
            id: field,
            start: 11,
            end: 16,
            instruction: "DATE".into(),
        }),
        Inline::Link(Link {
            id: link,
            start: 0,
            end: 7,
            target: LinkTarget::External("https://example.org".into()),
            tooltip: None,
        }),
        Inline::Bookmark(Bookmark {
            id: bookmark,
            name: "b".into(),
            start: 0,
            end: 7,
        }),
        Inline::Anchored(AnchoredObject {
            id: object,
            offset: 3,
            kind: "image".into(),
            data: vec![1],
        }),
    ] {
        run_checked(&mut doc, RichOp::InsertInline { para: p0, item });
    }
    // A 2x2 table; the first cell holds two paragraphs.
    let table = add_table(&mut doc, 2, 2);
    let grid = cells(&doc, table);
    for (cell, text) in [
        (grid[0][0], "A1A2"),
        (grid[0][1], "B"),
        (grid[1][0], "C"),
        (grid[1][1], "D"),
    ] {
        let para = cell_paragraph(&doc, cell);
        run_checked(
            &mut doc,
            RichOp::InsertText {
                para,
                offset: 0,
                text: text.into(),
            },
        );
    }
    let first_cell = grid[0][0];
    let new_id = doc.alloc_id().unwrap();
    run_checked(
        &mut doc,
        RichOp::SplitParagraph {
            parent: Some(first_cell),
            position: 0,
            offset: 2,
            new_id,
        },
    );
    // An opaque block, a header with text, an empty footer, a second section.
    let opaque = doc.new_opaque("w:sdt", &[1]).unwrap();
    let position = doc.block_ids(Some(section)).unwrap().len();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(section),
            position,
            subtree: opaque,
        },
    );
    let header = doc.new_header_footer("Entête").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Header,
            variant: HfVariant::Default,
            subtree: header,
        },
    );
    let footer = doc.new_header_footer("").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Footer,
            variant: HfVariant::Default,
            subtree: footer,
        },
    );
    let second = doc.new_section().unwrap();
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: None,
            position: 1,
            subtree: second,
        },
    );
    doc.validate().unwrap();

    let loss = formatting_loss(&doc);
    assert_eq!(
        loss,
        FormattingLoss {
            character_runs: 2,
            formatted_paragraphs: 1,
            styled_paragraphs: 1,
            tables: 1,
            objects: 2,
            fields: 1,
            links: 1,
            bookmarks: 1,
            headers_footers: 1,
            list_items: 1,
            section_breaks: 1,
        }
    );
    assert!(!loss.is_empty());
    assert_eq!(loss.categories().len(), 11);
    assert_eq!(loss.categories()[3], (LossCategory::Tables, 1));
    // Flattening: the table is two TAB-separated lines; the cell's two
    // paragraphs are joined by a space; the opaque block, header and objects
    // are absent; the field result and link text are kept; the second section
    // adds its empty paragraph.
    assert_eq!(rich_to_text(&doc), "Bonjour le monde\nA1 A2\tB\nC\tD\n");
    let saved = save_rich_document(&doc, &SaveOptions::default()).unwrap();
    assert_eq!(saved.bytes, "Bonjour le monde\nA1 A2\tB\nC\tD\n".as_bytes());
}

#[test]
fn a_document_without_formatting_has_an_empty_report() {
    let doc = open(b"a\nb\n\nc").to_rich_document(100).unwrap();
    let loss = formatting_loss(&doc);
    assert!(loss.is_empty());
    assert!(loss.categories().is_empty());
    assert_eq!(loss, FormattingLoss::default());
    assert!(formatting_loss(&RichDocument::new()).is_empty());
    // An empty header is not a loss.
    let mut doc = RichDocument::new();
    let section = first_section(&doc);
    let footer = doc.new_header_footer("").unwrap();
    run_checked(
        &mut doc,
        RichOp::AttachHeaderFooter {
            section,
            kind: HfKind::Footer,
            variant: HfVariant::Default,
            subtree: footer,
        },
    );
    assert!(formatting_loss(&doc).is_empty());
}

#[test]
fn table_cells_are_tab_separated_and_spans_keep_columns_aligned() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 1, 3);
    let grid = cells(&doc, table);
    for (cell, text) in grid[0].iter().zip(["a", "b", "c"]) {
        let para = cell_paragraph(&doc, *cell);
        run_checked(
            &mut doc,
            RichOp::InsertText {
                para,
                offset: 0,
                text: text.into(),
            },
        );
    }
    // Paragraph 0 is the empty one before the table.
    assert_eq!(rich_to_text(&doc), "\na\tb\tc");
}

#[test]
fn labels_and_lists_for_the_interface() {
    assert_eq!(MANUAL_ENCODINGS.len(), 7);
    assert!(MANUAL_ENCODINGS.contains(&Encoding::Windows1252));
    assert_eq!(SAVE_ENCODINGS[0], SaveEncoding::Utf8);
    assert_eq!(encoding_label(Encoding::Utf16Le), "UTF-16 LE");
    assert_eq!(LossCategory::Tables.catalog_key(), "text-loss-tables");
    let default = SaveOptions::default();
    assert_eq!(
        (default.encoding, default.line_ending),
        (SaveEncoding::Utf8, LineEndingChoice::Lf)
    );
}
