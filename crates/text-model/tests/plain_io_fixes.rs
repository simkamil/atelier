// SPDX-License-Identifier: AGPL-3.0-or-later
//! Review fixes of US-316: UTF-16 without BOM that looks like ASCII, UTF-32
//! BOMs, tables with vertical merges, and fixed-value checks of the BOM bytes
//! and of the replacement mark.

mod common;

use atelier_text_model::plain_io::*;
use atelier_text_model::rich::*;
use atelier_unicode::{Encoding, encode};
use common::*;

fn open(bytes: &[u8]) -> OpenedText {
    open_text(bytes, &OpenOptions::default()).unwrap()
}

// ----- 1. ASCII-looking UTF-16 ------------------------------------------------------

#[test]
fn cyrillic_utf16_without_bom_is_never_indifferent() {
    let text = "Привет\nмир это тест";
    for (little, expected) in [(true, Encoding::Utf16Le), (false, Encoding::Utf16Be)] {
        let bytes = encode(text, expected).unwrap();
        assert!(
            bytes.is_ascii(),
            "the case under test: every byte is below 0x80"
        );
        let opened = open(&bytes);
        assert_eq!(opened.confidence, Confidence::Uncertain, "little={little}");
        assert!(opened.suggest_manual_choice());
        // The candidate is the right UTF-16, but never certain.
        assert_eq!(opened.encoding, expected);
        assert_eq!(opened.text, text);
        let forced = open_text(
            &bytes,
            &OpenOptions {
                encoding: Some(expected),
                ..OpenOptions::default()
            },
        )
        .unwrap();
        assert_eq!(forced.text, text);
    }
}

#[test]
fn control_heavy_ascii_without_a_pattern_is_uncertain_utf8() {
    let opened = open(b"\x01\x02\x03\x04 abc");
    assert_eq!(opened.confidence, Confidence::Uncertain);
    assert_eq!(opened.encoding, Encoding::Utf8);
    assert!(opened.suggest_manual_choice());
}

#[test]
fn ordinary_ascii_stays_indifferent() {
    let code = "fn main() {\n\tlet x = 1;\r\n\tif x > 0 {\n\t\tprintln!(\"ok\");\n\t}\n}\n";
    for text in [
        "Hello, world.\nSecond line.",
        code,
        "page one\x0Cpage two, with one form feed in a longer text",
        "\x0C",
        "",
        "a\tb\tc\td\te",
    ] {
        let opened = open(text.as_bytes());
        assert_eq!(opened.confidence, Confidence::Indifferent, "{text:?}");
        assert!(!opened.suggest_manual_choice());
    }
}

// ----- 2. UTF-32 -----------------------------------------------------------------

#[test]
fn utf32_boms_are_never_read_in_silence() {
    let le = open(b"\xFF\xFE\x00\x00A\x00\x00\x00");
    let be = open(b"\x00\x00\xFE\xFF\x00\x00\x00A");
    for opened in [&le, &be] {
        assert!(opened.utf32_suspected);
        assert_eq!(opened.confidence, Confidence::Uncertain);
        assert!(opened.suggest_manual_choice());
    }
    // A UTF-16 LE BOM followed by a NUL character is the same bytes: the
    // ambiguity is resolved by asking.
    let nul = open(b"\xFF\xFE\x00\x00");
    assert!(nul.utf32_suspected && nul.confidence == Confidence::Uncertain);
    // A normal UTF-16 file keeps its certain BOM.
    for bytes in [&b"\xFF\xFEA\x00"[..], b"\xFE\xFF\x00A"] {
        let opened = open(bytes);
        assert!(!opened.utf32_suspected);
        assert_eq!(opened.confidence, Confidence::Certain);
    }
    assert!(
        !detect(b"\xFF\xFE\x00\x00")
            .confidence
            .eq(&Confidence::Certain)
    );
    // Imposing an encoding is an explicit choice and raises no suspicion.
    let chosen = open_text(
        b"\xFF\xFE\x00\x00A\x00",
        &OpenOptions {
            encoding: Some(Encoding::Utf16Le),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        (chosen.confidence, chosen.utf32_suspected),
        (Confidence::Chosen, false)
    );
}

// ----- 3. tables with merges -------------------------------------------------------

fn fill(doc: &mut RichDocument, table: NodeId) {
    let grid = cells(doc, table);
    for (r, row) in grid.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let Some(Node::Cell(cell)) = doc.node(*cell) else {
                panic!("cell")
            };
            let para = cell.blocks.get(0).unwrap();
            run_checked(
                doc,
                RichOp::InsertText {
                    para,
                    offset: 0,
                    text: format!("r{r}c{c}"),
                },
            );
        }
    }
}

fn table_lines(text: &str) -> Vec<&str> {
    text.split('\n').skip(1).collect()
}

#[test]
fn vertical_merge_keeps_the_columns_aligned() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 3);
    fill(&mut doc, table);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 1,
            col: 0,
            rows: 2,
            cols: 1,
        },
    );
    // The merge moved r2c0 into the merged cell, which is written once, in
    // its first row; the covered slot of the last row is an empty field.
    assert_eq!(
        table_lines(&rich_to_text(&doc)),
        ["r0c0\tr0c1\tr0c2", "r1c0 r2c0\tr1c1\tr1c2", "\tr2c1\tr2c2"]
    );
}

#[test]
fn horizontal_and_combined_merges() {
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 3);
    fill(&mut doc, table);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 0,
            col: 0,
            rows: 1,
            cols: 2,
        },
    );
    assert_eq!(
        table_lines(&rich_to_text(&doc)),
        ["r0c0 r0c1\t\tr0c2", "r1c0\tr1c1\tr1c2", "r2c0\tr2c1\tr2c2"]
    );
    // Both at once: a 2x2 block in the bottom right.
    let mut doc = RichDocument::new();
    let table = add_table(&mut doc, 3, 3);
    fill(&mut doc, table);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table,
            row: 1,
            col: 1,
            rows: 2,
            cols: 2,
        },
    );
    let text = rich_to_text(&doc);
    let lines = table_lines(&text);
    assert_eq!(lines.len(), 3);
    assert!(
        lines.iter().all(|l| l.matches('\t').count() == 2),
        "{lines:?}"
    );
    assert_eq!(lines[0], "r0c0\tr0c1\tr0c2");
    assert!(lines[1].starts_with("r1c0\tr1c1"));
    assert_eq!(lines[2], "r2c0\t\t");
    assert_eq!(text.matches("r2c2").count(), 1);
}

#[test]
fn merges_in_a_nested_table() {
    let mut doc = RichDocument::new();
    let outer = add_table(&mut doc, 1, 2);
    let host = cells(&doc, outer)[0][0];
    let nested = doc.new_table(3, 3).unwrap();
    let nested_id = nested.root;
    run_checked(
        &mut doc,
        RichOp::InsertBlock {
            parent: Some(host),
            position: 1,
            subtree: nested,
        },
    );
    fill(&mut doc, nested_id);
    run_checked(
        &mut doc,
        RichOp::MergeCells {
            table: nested_id,
            row: 1,
            col: 0,
            rows: 2,
            cols: 1,
        },
    );
    let guest = cells(&doc, outer)[0][1];
    let para = match doc.node(guest) {
        Some(Node::Cell(c)) => c.blocks.get(0).unwrap(),
        _ => panic!("cell"),
    };
    run_checked(
        &mut doc,
        RichOp::InsertText {
            para,
            offset: 0,
            text: "side".into(),
        },
    );
    let text = rich_to_text(&doc);
    let lines = table_lines(&text);
    // The nested table is flattened into its cell: one outer row, one TAB,
    // every merged text once.
    assert_eq!(lines.len(), 1, "{text:?}");
    assert_eq!(lines[0].matches('\t').count(), 1);
    assert!(lines[0].ends_with("\tside"));
    assert_eq!(lines[0].matches("r1c0 r2c0").count(), 1);
    assert_eq!(lines[0].matches("r2c1").count(), 1);
}

// ----- fixed values -----------------------------------------------------------------

#[test]
fn bom_bytes_and_replacement_mark_are_exact() {
    let save = |encoding| {
        save_text(
            "",
            &SaveOptions {
                encoding,
                ..SaveOptions::default()
            },
        )
        .unwrap()
        .bytes
    };
    assert_eq!(save(SaveEncoding::Utf8), b"");
    assert_eq!(save(SaveEncoding::Utf8Bom), [0xEF, 0xBB, 0xBF]);
    assert_eq!(save(SaveEncoding::Utf16Le), [0xFF, 0xFE]);
    assert_eq!(save(SaveEncoding::Utf16Be), [0xFE, 0xFF]);
    assert_eq!(save(SaveEncoding::Windows1252), b"");
    // Reading side, with literal bytes: each BOM is recognised and removed.
    for (bytes, encoding) in [
        (&[0xEF, 0xBB, 0xBF, 0x41][..], Encoding::Utf8),
        (&[0xFF, 0xFE, 0x41, 0x00], Encoding::Utf16Le),
        (&[0xFE, 0xFF, 0x00, 0x41], Encoding::Utf16Be),
    ] {
        let opened = open(bytes);
        assert_eq!(
            (opened.encoding, opened.bom, opened.text.as_str()),
            (encoding, true, "A")
        );
    }
    let replacing = SaveOptions {
        encoding: SaveEncoding::Windows1252,
        unmappable: Unmappable::Replace,
        ..SaveOptions::default()
    };
    let saved = save_text("→", &replacing).unwrap();
    assert_eq!(saved.bytes, [0x3F]);
    assert_eq!(saved.bytes, b"?");
    assert_eq!(saved.replaced, 1);
}
