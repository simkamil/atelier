// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-313: structure of `.doc` files (tables, lists, sections, headers and
//! footers, fields, bookmarks) on files made by the deterministic test builder.
//! They cover what LibreOffice does not write (horizontal merge flags, unusual
//! number formats, section break kinds, damaged structures); results are
//! computed by hand from [MS-DOC]. The same structures written by LibreOffice
//! are checked in `reference_structure.rs`.

mod common;

use atelier_fmt_legacy::doc::{
    DocError, DocLimits, HfState, ImportNotice, ImportedDoc, LevelAlignment, LevelFollow,
    ListLevelKind, NumberFormat, SectionBreak, import_doc, import_doc_with_limits,
};
use atelier_text_model::rich::{HfKind, HfVariant, LinkTarget, Node};
use common::*;

fn open(spec: &DocSpec) -> ImportedDoc {
    let doc = import_doc(&spec.build()).expect("import");
    doc.document.validate().expect("valid model");
    doc
}

fn texts(doc: &ImportedDoc) -> Vec<String> {
    doc.paragraph_ids()
        .iter()
        .map(|id| doc.document.paragraph(*id).unwrap().text.clone())
        .collect()
}

fn pr(cp: usize, grpprl: Vec<u8>) -> ParaRunSpec {
    ParaRunSpec {
        cp_start: cp,
        cp_end: cp + 1,
        istd: 0,
        grpprl,
    }
}

fn in_table(itap: u32) -> Vec<u8> {
    cat(&[sprm_u8(0x2416, 1), sprm_u32(0x6649, itap)])
}

/// `sprmTDefTable` with one `TC80` per cell flag word.
fn def_table(centers: &[i16], grfs: &[u16]) -> Vec<u8> {
    let n = grfs.len();
    assert_eq!(centers.len(), n + 1);
    let mut data = vec![n as u8];
    for c in centers {
        data.extend_from_slice(&c.to_le_bytes());
    }
    for g in grfs {
        data.extend_from_slice(&g.to_le_bytes());
        data.extend_from_slice(&[0u8; 18]);
    }
    let mut out = 0xD608u16.to_le_bytes().to_vec();
    out.extend_from_slice(&(data.len() as u16 + 1).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn row_end(centers: &[i16], grfs: &[u16]) -> Vec<u8> {
    cat(&[in_table(1), sprm_u8(0x2417, 1), def_table(centers, grfs)])
}

/// Paragraph properties of the marks listed: `(cp, grpprl)`.
fn marks(spec: &mut DocSpec, list: Vec<(usize, Vec<u8>)>) {
    spec.papx = list.into_iter().map(|(cp, g)| pr(cp, g)).collect();
}

fn repaired(doc: &ImportedDoc, what: &str) -> usize {
    doc.notices
        .iter()
        .find_map(|n| match n {
            ImportNotice::TableRepaired { what: w, count } if *w == what => Some(*count),
            _ => None,
        })
        .unwrap_or(0)
}

fn cells_of(doc: &ImportedDoc) -> Vec<(usize, usize, usize, usize, bool)> {
    let mut v: Vec<_> = doc
        .tables
        .cells
        .values()
        .map(|c| {
            (
                c.row_index,
                c.column_index,
                c.row_span,
                c.col_span,
                c.synthetic,
            )
        })
        .collect();
    v.sort();
    v
}

// ----- tables ---------------------------------------------------------------------------

#[test]
fn horizontal_merge_flags_make_one_wide_cell() {
    // TC 0 starts a merged group (fFirstMerged = 1), TC 1 is merged into it
    // (fMerged = 2): two cell marks for three grid columns.
    // A second row with three cells gives the grid its three columns.
    let mut spec = DocSpec::ansi("ab\u{7}c\u{7}\u{7}d\u{7}e\u{7}f\u{7}\u{7}z\r");
    let c = &[0i16, 1000, 2000, 3000][..];
    marks(
        &mut spec,
        vec![
            (2, in_table(1)),
            (4, in_table(1)),
            (5, row_end(c, &[1, 2, 0])),
            (7, in_table(1)),
            (9, in_table(1)),
            (11, in_table(1)),
            (12, row_end(c, &[0, 0, 0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["ab", "c", "d", "e", "f", "z"]);
    let t = doc.tables.tables.values().next().unwrap();
    assert_eq!((t.columns, t.grid.clone()), (3, vec![Some(1000); 3]));
    assert_eq!(
        cells_of(&doc),
        vec![
            (0, 0, 1, 2, false),
            (0, 2, 1, 1, false),
            (1, 0, 1, 1, false),
            (1, 1, 1, 1, false),
            (1, 2, 1, 1, false)
        ]
    );
    // Alone, the merged row has only two grid columns.
    let mut spec = DocSpec::ansi("ab\u{7}c\u{7}\u{7}z\r");
    marks(
        &mut spec,
        vec![
            (2, in_table(1)),
            (4, in_table(1)),
            (5, row_end(c, &[1, 2, 0])),
        ],
    );
    let doc = open(&spec);
    let t = doc.tables.tables.values().next().unwrap();
    assert_eq!(
        (t.columns, t.grid.clone()),
        (2, vec![Some(2000), Some(1000)])
    );
}

#[test]
fn rows_with_different_boundaries_share_one_grid() {
    // Row 1: [0,1000,2000] (two cells); row 2: [0,2000] (one cell).
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}c\u{7}\u{7}z\r");
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, row_end(&[0, 1000, 2000], &[0, 0])),
            (6, in_table(1)),
            (7, row_end(&[0, 2000], &[0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(
        cells_of(&doc),
        vec![
            (0, 0, 1, 1, false),
            (0, 1, 1, 1, false),
            (1, 0, 1, 2, false)
        ]
    );
    assert_eq!(repaired(&doc, "short row completed"), 0);
}

#[test]
fn short_rows_get_a_synthetic_cell() {
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}c\u{7}\u{7}z\r");
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, row_end(&[0, 1000, 2000], &[0, 0])),
            (6, in_table(1)),
            (7, row_end(&[0, 1000], &[0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(
        cells_of(&doc),
        vec![
            (0, 0, 1, 1, false),
            (0, 1, 1, 1, false),
            (1, 0, 1, 1, false),
            (1, 1, 1, 1, true)
        ]
    );
    assert_eq!(repaired(&doc, "short row completed"), 1);
    // The synthetic cell holds one empty paragraph.
    let (id, _) = doc.tables.cells.iter().find(|(_, c)| c.synthetic).unwrap();
    let blocks = doc.document.block_ids(Some(*id)).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(doc.document.paragraph(blocks[0]).unwrap().text, "");
}

#[test]
fn cell_count_and_definition_may_disagree() {
    // Three cell marks, a definition of two cells.
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}c\u{7}\u{7}");
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (5, in_table(1)),
            (6, row_end(&[0, 1000, 2000], &[0, 0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(cells_of(&doc).len(), 3);
    assert_eq!(
        repaired(&doc, "cell count differs from the row definition"),
        1
    );
    // No definition at all: equal columns.
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}");
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, cat(&[in_table(1), sprm_u8(0x2417, 1)])),
        ],
    );
    let doc = open(&spec);
    let t = doc.tables.tables.values().next().unwrap();
    assert_eq!(t.grid, vec![Some(1440), Some(1440)]);
    assert_eq!(repaired(&doc, "row without cell definition"), 1);
}

#[test]
fn vertical_merges_with_and_without_a_start_cell() {
    // fVertRestart|fVertMerge = 0x60 starts, 0x20 continues.
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}x\u{7}d\u{7}\u{7}");
    let c = &[0i16, 1000, 2000][..];
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, row_end(c, &[0x60, 0])),
            (6, in_table(1)),
            (8, in_table(1)),
            (9, row_end(c, &[0x20, 0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(
        cells_of(&doc),
        vec![
            (0, 0, 2, 1, false),
            (0, 1, 1, 1, false),
            (1, 1, 1, 1, false)
        ]
    );
    // Non-empty content of the continuation moves into the start cell.
    // `x` now follows `a` in document order.
    assert_eq!(texts(&doc), ["a", "x", "b", "d"]);
    assert_eq!(
        repaired(&doc, "content of a vertically merged cell moved"),
        1
    );
    // Continuation without a start: an ordinary cell.
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}c\u{7}d\u{7}\u{7}");
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, row_end(c, &[0, 0])),
            (6, in_table(1)),
            (8, in_table(1)),
            (9, row_end(c, &[0x20, 0])),
        ],
    );
    let doc = open(&spec);
    assert!(cells_of(&doc).iter().all(|c| c.2 == 1));
    assert_eq!(repaired(&doc, "vertical merge without a start cell"), 1);
}

#[test]
fn an_unterminated_table_is_closed() {
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}");
    marks(&mut spec, vec![(1, in_table(1)), (3, in_table(1))]);
    let doc = open(&spec);
    assert_eq!(doc.tables.tables.len(), 1);
    assert_eq!(cells_of(&doc).len(), 2);
}

#[test]
fn nested_tables_use_inner_cell_and_row_marks() {
    // Outer cell: "a", an inner 1x1 table holding "b", then "c".
    let mut spec = DocSpec::ansi("a\rb\r\rc\u{7}\u{7}z\r");
    let inner_cell = cat(&[in_table(2), sprm_u8(0x244B, 1)]);
    let inner_row = cat(&[
        in_table(2),
        sprm_u8(0x244B, 1),
        sprm_u8(0x244C, 1),
        def_table(&[0, 1000], &[0]),
    ]);
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, inner_cell),
            (4, inner_row),
            (6, in_table(1)),
            (7, row_end(&[0, 3000], &[0])),
        ],
    );
    let doc = open(&spec);
    assert_eq!(texts(&doc), ["a", "b", "c", "z"]);
    let mut depths: Vec<usize> = doc.tables.tables.values().map(|t| t.depth).collect();
    depths.sort();
    assert_eq!(depths, [0, 1]);
    let (outer_cell, _) = doc
        .tables
        .cells
        .iter()
        .find(|(_, c)| doc.tables.tables[&c.table.unwrap()].depth == 0)
        .unwrap();
    let blocks = doc.document.block_ids(Some(*outer_cell)).unwrap();
    assert_eq!(blocks.len(), 3);
    assert!(matches!(doc.document.node(blocks[1]), Some(Node::Table(_))));
    // A nesting limit below the depth of the file clamps, never fails.
    let limits = DocLimits {
        max_table_depth: 1,
        ..DocLimits::default()
    };
    let doc = import_doc_with_limits(&spec.build(), limits).unwrap();
    doc.document.validate().unwrap();
}

#[test]
fn table_limits_are_typed_errors() {
    let mut spec = DocSpec::ansi("a\u{7}b\u{7}\u{7}c\u{7}\u{7}");
    let c = &[0i16, 1000, 2000][..];
    marks(
        &mut spec,
        vec![
            (1, in_table(1)),
            (3, in_table(1)),
            (4, row_end(c, &[0, 0])),
            (6, in_table(1)),
            (7, row_end(&[0, 1000], &[0])),
        ],
    );
    let bytes = spec.build();
    for (limit, limits) in [
        (
            "table rows",
            DocLimits {
                max_table_rows: 1,
                ..DocLimits::default()
            },
        ),
        (
            "table cells",
            DocLimits {
                max_table_cells: 2,
                ..DocLimits::default()
            },
        ),
    ] {
        match import_doc_with_limits(&bytes, limits) {
            Err(DocError::Limit(l)) => assert_eq!(l, limit),
            other => panic!("{limit}: {:?}", other.map(|_| ())),
        }
    }
    // Too many grid columns: the grid follows the cell order instead.
    let limits = DocLimits {
        max_table_columns: 1,
        ..DocLimits::default()
    };
    let doc = import_doc_with_limits(&bytes, limits).unwrap();
    doc.document.validate().unwrap();
    assert_eq!(repaired(&doc, "too many grid columns"), 1);
}

#[test]
fn table_properties_are_read() {
    // sprmTJc, sprmTDxaLeft, sprmTDyaRowHeight (exact), sprmTTableHeader,
    // sprmTFCantSplit, sprmTTableWidth (50ths of percent), vertical alignment.
    let mut spec = DocSpec::ansi("a\u{7}\u{7}z\r");
    let g = cat(&[
        in_table(1),
        sprm_u8(0x2417, 1),
        sprm_u16(0x548A, 2),
        sprm_i16(0x9601, 120),
        sprm_i16(0x9407, -300),
        sprm_u8(0x3404, 1),
        sprm_u8(0x3403, 1),
        cat(&[0xF614u16.to_le_bytes().to_vec(), vec![2, 0x88, 0x13]]),
        def_table(&[0, 1000], &[0x100]),
    ]);
    marks(&mut spec, vec![(1, in_table(1)), (2, g)]);
    let doc = open(&spec);
    let t = doc.tables.tables.values().next().unwrap();
    use atelier_fmt_legacy::doc::{CellVerticalAlign, HeightRule, TableAlign, TableWidth};
    assert_eq!(t.props.align, Some(TableAlign::Right));
    assert_eq!(t.props.indent, Some(TableWidth::Dxa(120)));
    assert_eq!(t.props.width, Some(TableWidth::Pct(5000)));
    let r = doc.tables.rows.values().next().unwrap();
    assert_eq!(r.props.header, Some(true));
    assert_eq!(r.props.cant_split, Some(true));
    let h = r.props.height.unwrap();
    assert_eq!((h.twips, h.rule), (300, HeightRule::Exact));
    let c = doc.tables.cells.values().next().unwrap();
    assert_eq!(c.props.vertical_align, Some(CellVerticalAlign::Bottom));
}

// ----- lists ----------------------------------------------------------------------------

fn lvl(
    start: i32,
    nfc: u8,
    flags: u8,
    follow: u8,
    papx: &[u8],
    chpx: &[u8],
    text: &[u16],
) -> Vec<u8> {
    let mut v = start.to_le_bytes().to_vec();
    v.extend_from_slice(&[nfc, flags]);
    v.extend_from_slice(&[0u8; 9]);
    v.push(follow);
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(&[chpx.len() as u8, papx.len() as u8, 0, 0]);
    v.extend_from_slice(papx);
    v.extend_from_slice(chpx);
    v.extend_from_slice(&(text.len() as u16).to_le_bytes());
    for u in text {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v
}

fn decimal() -> Vec<u8> {
    lvl(1, 0, 0, 0, &[], &[], &[0, u16::from(b'.')])
}

/// `PlfLst` (header + LSTF, the `lcb`) followed by the LVLs.
fn plf_lst(lists: &[(u32, Vec<Vec<u8>>)]) -> (Vec<u8>, u32) {
    let mut head = (lists.len() as u16).to_le_bytes().to_vec();
    let mut body = Vec::new();
    for (lsid, levels) in lists {
        head.extend_from_slice(&lsid.to_le_bytes());
        head.extend_from_slice(&lsid.to_le_bytes());
        for _ in 0..9 {
            head.extend_from_slice(&0x0FFFu16.to_le_bytes());
        }
        head.extend_from_slice(&[u8::from(levels.len() == 1), 0]);
        for l in levels {
            body.extend_from_slice(l);
        }
    }
    let lcb = head.len() as u32;
    head.extend_from_slice(&body);
    (head, lcb)
}

type LfoLvl = (u8, Option<i32>, Option<Vec<u8>>);

fn plf_lfo(lfos: &[(u32, Vec<LfoLvl>)]) -> Vec<u8> {
    let mut v = (lfos.len() as u32).to_le_bytes().to_vec();
    for (lsid, levels) in lfos {
        v.extend_from_slice(&lsid.to_le_bytes());
        v.extend_from_slice(&[0u8; 8]);
        v.extend_from_slice(&[levels.len() as u8, 0, 0, 0]);
    }
    for (_, levels) in lfos {
        v.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        for (ilvl, start, fmt) in levels {
            v.extend_from_slice(&start.unwrap_or(0).to_le_bytes());
            let bits = ilvl
                | if start.is_some() { 0x10 } else { 0 }
                | if fmt.is_some() { 0x20 } else { 0 };
            v.extend_from_slice(&[bits, 0, 0, 0]);
            if let Some(f) = fmt {
                v.extend_from_slice(f);
            }
        }
    }
    v
}

fn nine(first: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut v = first;
    while v.len() < 9 {
        v.push(decimal());
    }
    v
}

fn list_spec(text: &str, lists: &[(u32, Vec<Vec<u8>>)], lfos: &[(u32, Vec<LfoLvl>)]) -> DocSpec {
    let mut spec = DocSpec::ansi(text);
    let (lst, lcb) = plf_lst(lists);
    spec.table_extra = vec![(73, lst, Some(lcb)), (74, plf_lfo(lfos), None)];
    spec
}

fn list_para(ilfo: i16, ilvl: u8) -> Vec<u8> {
    cat(&[sprm_u16(0x460B, ilfo as u16), sprm_u8(0x260A, ilvl)])
}

#[test]
fn list_definitions_overrides_and_references() {
    let bullet = lvl(1, 23, 0, 0, &[], &[], &[0xF0B7]);
    let lower_a = lvl(
        3,
        4,
        0x02 | 0x04,
        1,
        &cat(&[sprm_i16(0x845E, 720), sprm_i16(0x8460, -360)]),
        &[],
        &[1, u16::from(b')')],
    );
    let mut spec = list_spec(
        "p1\rp2\rp3\rp4\rp5\rp6\rp7\rp8\r",
        &[(10, nine(vec![decimal(), bullet]))],
        &[
            (10, vec![]),
            (10, vec![(0, Some(7), None), (1, None, Some(lower_a))]),
            (99, vec![]),
        ],
    );
    spec.styles = Some(vec![
        StdSpec::para(0, "Normal", 0x0FFF, vec![], vec![]),
        StdSpec::para(0x0FFE, "Liste", 0, list_para(1, 1), vec![]),
    ]);
    let mut p = vec![
        (2, list_para(1, 0)),
        (5, list_para(2, 1)),
        (8, list_para(3, 0)),
        (11, list_para(9, 0)),
        (14, list_para(1, 12)),
        (17, list_para(-2, 0)),
    ];
    // p7 takes the list of its style, p8 opts out with ilfo 0.
    marks(&mut spec, vec![]);
    let mut runs: Vec<ParaRunSpec> = p.drain(..).map(|(cp, g)| pr(cp, g)).collect();
    runs.push(ParaRunSpec {
        cp_start: 20,
        cp_end: 21,
        istd: 1,
        grpprl: vec![],
    });
    runs.push(ParaRunSpec {
        cp_start: 23,
        cp_end: 24,
        istd: 1,
        grpprl: sprm_u16(0x460B, 0),
    });
    spec.papx = runs;
    let doc = open(&spec);
    let refs: Vec<_> = doc
        .paragraph_ids()
        .iter()
        .map(|id| {
            doc.document
                .paragraph(*id)
                .unwrap()
                .props
                .list
                .map(|l| (l.list_id, l.level))
        })
        .collect();
    assert_eq!(
        refs,
        [
            Some((1, 0)),
            Some((2, 1)),
            Some((3, 0)),
            None,
            Some((1, 8)),
            None,
            Some((1, 1)),
            None
        ]
    );
    assert!(
        doc.list_items
            .iter()
            .any(|i| i.from_style && i.list_id == 1)
    );
    assert!(doc.notices.contains(&ImportNotice::DamagedData {
        what: "list reference to a missing list",
        count: 2
    }));
    assert_eq!(doc.lists.len(), 3);
    let l1 = &doc.lists[0];
    assert!(l1.shared_definition && !l1.simple);
    assert_eq!(l1.levels.len(), 9);
    assert_eq!(
        l1.levels[0].kind,
        ListLevelKind::Number {
            format: NumberFormat::Decimal,
            text: "%1.".into(),
            start: 1
        }
    );
    assert_eq!(l1.levels[1].kind, ListLevelKind::Bullet("\u{F0B7}".into()));
    let l2 = &doc.lists[1];
    assert!(l2.levels[0].start_overridden && !l2.levels[0].overridden);
    assert_eq!(
        l2.levels[0].kind,
        ListLevelKind::Number {
            format: NumberFormat::Decimal,
            text: "%1.".into(),
            start: 7
        }
    );
    let o = &l2.levels[1];
    assert!(o.overridden && o.legal);
    assert_eq!(o.alignment, LevelAlignment::Right);
    assert_eq!(o.follow, LevelFollow::Space);
    assert_eq!(
        o.kind,
        ListLevelKind::Number {
            format: NumberFormat::LowerLetter,
            text: "%2)".into(),
            start: 3
        }
    );
    assert_eq!(
        (o.para_props.indent_left, o.para_props.indent_first_line),
        (Some(720), Some(-360))
    );
    assert!(doc.lists[2].levels.is_empty());
}

#[test]
fn number_formats_follow_the_nfc_table() {
    // Only 0-4, 23 and 255 are proven by LibreOffice files; the rest comes
    // from [MS-DOC].
    let nfcs = [5u8, 6, 7, 8, 22, 255, 0x40, 1, 2];
    let levels: Vec<Vec<u8>> = nfcs
        .iter()
        .map(|n| lvl(1, *n, 0, 2, &[], &[], &[0]))
        .collect();
    let spec = list_spec("a\r", &[(1, levels)], &[(1, vec![])]);
    let doc = open(&spec);
    let formats: Vec<_> = doc.lists[0]
        .levels
        .iter()
        .map(|l| match &l.kind {
            ListLevelKind::Number { format, .. } => Some(*format),
            _ => None,
        })
        .collect();
    assert_eq!(
        formats,
        [
            Some(NumberFormat::Ordinal),
            Some(NumberFormat::CardinalText),
            Some(NumberFormat::OrdinalText),
            Some(NumberFormat::Hex),
            Some(NumberFormat::DecimalZero),
            None,
            Some(NumberFormat::Other(0x40)),
            Some(NumberFormat::UpperRoman),
            Some(NumberFormat::LowerRoman),
        ]
    );
    assert_eq!(doc.lists[0].levels[5].kind, ListLevelKind::None);
    assert_eq!(doc.lists[0].levels[0].follow, LevelFollow::Nothing);
}

#[test]
fn word_6_numbering_is_noticed() {
    let mut spec = DocSpec::ansi("a\r");
    marks(&mut spec, vec![(1, sprm_var(0xC63E, &[0; 4]))]);
    let doc = open(&spec);
    assert!(
        doc.notices
            .contains(&ImportNotice::LegacyNumbering { paragraphs: 1 })
    );
}

#[test]
fn damaged_list_tables_do_not_break_the_import() {
    let mut spec = DocSpec::ansi("a\r");
    spec.table_extra = vec![(73, vec![0xFF; 40], Some(40)), (74, vec![0xFF; 20], None)];
    let doc = import_doc(&spec.build());
    assert!(doc.is_ok() || matches!(doc, Err(DocError::Limit(_))));
}

// ----- sections -------------------------------------------------------------------------

#[test]
fn sections_break_types_and_properties() {
    // bkc 0 continuous, 1 new column, 3 even page, 4 odd page (LibreOffice
    // writes 0 and the default new page).
    let mut spec = DocSpec::ansi("a\u{C}b\u{C}c\u{C}d\r");
    spec.section_ends = vec![2, 4, 6, 8];
    spec.sepx = vec![
        cat(&[sprm_u8(0x3009, 0), sprm_u8(0x300E, 1)]),
        cat(&[
            sprm_u8(0x3009, 1),
            sprm_u8(0x301D, 2),
            sprm_u16(0xB01F, 15840),
            sprm_u16(0xB020, 12240),
            sprm_u16(0xB021, 100),
            sprm_u16(0xB022, 200),
            sprm_u16(0x9023, 300),
            sprm_u16(0x9024, 400),
            sprm_u16(0xB025, 50),
            sprm_u16(0xB017, 700),
            sprm_u16(0xB018, 800),
            sprm_u16(0x500B, 2),
            sprm_u16(0x900C, 500),
            sprm_u8(0x300A, 1),
        ]),
        sprm_u8(0x3009, 3),
        cat(&[
            sprm_u8(0x3009, 4),
            sprm_u8(0x300E, 3),
            sprm_u8(0x3011, 1),
            sprm_u16(0x501C, 9),
        ]),
    ];
    let doc = open(&spec);
    assert_eq!(doc.sections.len(), 4);
    assert_eq!(doc.document.section_ids().len(), 4);
    assert_eq!(texts(&doc), ["a", "b", "c", "d"]);
    let kinds: Vec<_> = doc.sections.iter().map(|s| s.break_type).collect();
    assert_eq!(
        kinds,
        [
            SectionBreak::Continuous,
            SectionBreak::NewColumn,
            SectionBreak::EvenPage,
            SectionBreak::OddPage
        ]
    );
    assert_eq!(
        doc.sections[0].page_numbers.format,
        Some(NumberFormat::UpperRoman)
    );
    let s = &doc.sections[1];
    assert!(s.page.landscape && s.title_page);
    assert_eq!((s.page.width, s.page.height), (Some(15840), Some(12240)));
    assert_eq!(
        (
            s.page.margin_left,
            s.page.margin_right,
            s.page.margin_top,
            s.page.margin_bottom,
            s.page.gutter,
            s.page.header_distance,
            s.page.footer_distance
        ),
        (
            Some(100),
            Some(200),
            Some(300),
            Some(400),
            Some(50),
            Some(700),
            Some(800)
        )
    );
    assert_eq!((s.columns.count, s.columns.spacing), (3, Some(500)));
    let last = &doc.sections[3].page_numbers;
    assert_eq!(
        (last.format, last.restart, last.start),
        (Some(NumberFormat::UpperLetter), true, Some(9))
    );
    // Each section holds its paragraph.
    for (i, s) in doc.sections.iter().enumerate() {
        let blocks = doc.document.block_ids(Some(s.section)).unwrap();
        assert_eq!(blocks.len(), 1, "section {i}");
    }
    assert!(doc.breaks.iter().filter(|b| b.kind == "section").count() == 3);
    // Missing SEPX: defaults.
    let mut spec = DocSpec::ansi("a\u{C}b\r");
    spec.section_ends = vec![2, 4];
    let doc = open(&spec);
    assert_eq!(doc.sections.len(), 2);
    assert_eq!(doc.sections[1].break_type, SectionBreak::NewPage);
    assert_eq!(doc.sections[1].page.width, None);
}

#[test]
fn section_limit_is_a_typed_error() {
    let mut spec = DocSpec::ansi("a\u{C}b\r");
    spec.section_ends = vec![2, 4];
    let limits = DocLimits {
        max_sections: 1,
        ..DocLimits::default()
    };
    assert!(matches!(
        import_doc_with_limits(&spec.build(), limits),
        Err(DocError::Limit("section count"))
    ));
}

// ----- headers and footers --------------------------------------------------------------

fn hdd_spec() -> DocSpec {
    // Main text: two sections. Header document: 6 empty separator stories, then
    // for section 0: even header (0), odd header "Hdr" + extra mark (5), even
    // footer (0), odd footer "\r\r" (2: explicitly empty), first header
    // "First" + extra mark (7), first footer (0); section 1: six empty stories.
    let mut spec = DocSpec::new(vec![
        PieceSpec::ansi("m\u{C}n\r"),
        PieceSpec {
            story: 2,
            ..PieceSpec::ansi("Hdr\r\r\r\rFirst\r\r")
        },
    ]);
    spec.section_ends = vec![2, 4];
    spec.sepx = vec![sprm_u8(0x300A, 1)];
    let cps: [u32; 19] = [
        0, 0, 0, 0, 0, 0, 0, 0, 5, 5, 7, 14, 14, 14, 14, 14, 14, 14, 14,
    ];
    let plc: Vec<u8> = cps.iter().flat_map(|c| c.to_le_bytes()).collect();
    spec.table_extra = vec![(11, plc, None)];
    spec
}

#[test]
fn header_stories_slots_and_inheritance() {
    let doc = open(&hdd_spec());
    assert_eq!(texts(&doc), ["m", "n"]);
    assert!(!doc.facing_pages);
    // Without facing pages the even slots do not apply; first-page slots only
    // for the section with a title page.
    let s0 = &doc.sections[0].headers_footers;
    let slots: Vec<_> = s0.iter().map(|h| (h.kind, h.variant)).collect();
    assert_eq!(
        slots,
        [
            (HfKind::Header, HfVariant::Default),
            (HfKind::Footer, HfVariant::Default),
            (HfKind::Header, HfVariant::First),
            (HfKind::Footer, HfVariant::First),
        ]
    );
    let HfState::Present(h) = s0[0].state else {
        panic!("odd header")
    };
    assert_eq!(s0[1].state, HfState::Empty);
    let HfState::Present(first) = s0[2].state else {
        panic!("first header")
    };
    assert_eq!(s0[3].state, HfState::Inherited);
    let para_text = |hf| {
        let ids = &doc
            .header_paragraphs
            .iter()
            .find(|(x, _)| *x == hf)
            .unwrap()
            .1;
        ids.iter()
            .map(|id| doc.document.paragraph(*id).unwrap().text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(para_text(h), ["Hdr"]);
    assert_eq!(para_text(first), ["First"]);
    let s1 = &doc.sections[1].headers_footers;
    assert!(
        s1.iter()
            .all(|x| x.state == HfState::Inherited && x.variant == HfVariant::Default)
    );
    assert_eq!(
        doc.effective_header_footer(1, HfKind::Header, HfVariant::Default),
        Some(h)
    );
    assert_eq!(
        doc.effective_header_footer(1, HfKind::Footer, HfVariant::Default),
        None
    );
    // The model holds the attached headers; header text is not in the body.
    let Some(Node::Section(n)) = doc.document.node(doc.sections[0].section) else {
        panic!("section")
    };
    assert_eq!((n.headers.len(), n.footers.len()), (2, 0));
    assert!(
        !doc.notices
            .iter()
            .any(|n| matches!(n, ImportNotice::IgnoredText { .. }))
    );
    // Facing pages (DopBase.fFacingPages) makes the even slots apply.
    let mut spec = hdd_spec();
    spec.table_extra.push((31, vec![1, 0, 0, 0], None));
    let doc = open(&spec);
    assert!(doc.facing_pages);
    assert!(
        doc.sections[0]
            .headers_footers
            .iter()
            .any(|h| h.variant == HfVariant::Even && h.state == HfState::Inherited)
    );
}

// ----- fields ---------------------------------------------------------------------------

fn plcffld(marks: &[(u32, u8, u8)], end: u32) -> Vec<u8> {
    let mut v: Vec<u8> = marks.iter().flat_map(|m| m.0.to_le_bytes()).collect();
    v.extend_from_slice(&end.to_le_bytes());
    for m in marks {
        v.extend_from_slice(&[m.1, m.2]);
    }
    v
}

fn issue(doc: &ImportedDoc, what: &str) -> usize {
    doc.notices
        .iter()
        .find_map(|n| match n {
            ImportNotice::FieldDegraded { what: w, count } if *w == what => Some(*count),
            _ => None,
        })
        .unwrap_or(0)
}

#[test]
fn page_field_with_a_plcffld() {
    // a 0x13 " PAGE " 0x14 "1" 0x15 b: marks at cp 1, 8, 10.
    let mut spec = DocSpec::ansi("a\u{13} PAGE \u{14}1\u{15}b\r");
    spec.table_extra = vec![(
        16,
        plcffld(&[(1, 0x13, 33), (8, 0x14, 0xFF), (10, 0x15, 0x80)], 13),
        None,
    )];
    let doc = open(&spec);
    let p = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(p.text, "a1b");
    assert_eq!(p.fields.len(), 1);
    assert_eq!((p.fields[0].start, p.fields[0].end), (1, 2));
    assert_eq!(p.fields[0].instruction, "PAGE");
    assert_eq!(doc.fields[0].field_type, Some(33));
    assert_eq!(doc.fields[0].code, "PAGE");
    // A mark missing from the Plcffld is not a field mark.
    let mut spec = DocSpec::ansi("a\u{13} PAGE \u{14}1\u{15}b\r");
    spec.table_extra = vec![(16, plcffld(&[(1, 0x13, 33), (8, 0x14, 0xFF)], 13), None)];
    let doc = open(&spec);
    assert_eq!(issue(&doc, "field mark missing from Plcffld"), 1);
    assert!(
        doc.document
            .paragraph(doc.paragraph_ids()[0])
            .unwrap()
            .fields
            .is_empty()
    );
}

#[test]
fn hyperlink_codes() {
    let doc = open(&DocSpec::ansi(
        "\u{13} HYPERLINK \"http://a.b/c\" \\o \"tip\" \u{14}one\u{15} \
         \u{13}HYPERLINK \"f.doc\" \\l \"m\"\u{14}two\u{15} \
         \u{13}HYPERLINK  \\l \"m\"\u{01}\u{14}three\u{15} \u{13}HYPERLINK \u{14}four\u{15}\r",
    ));
    let p = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(p.text, "one two three four");
    let got: Vec<_> = p
        .links
        .iter()
        .map(|l| (&p.text[l.start..l.end], l.target.clone(), l.tooltip.clone()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "one",
                LinkTarget::External("http://a.b/c".into()),
                Some("tip".into())
            ),
            ("two", LinkTarget::External("f.doc#m".into()), None),
            ("three", LinkTarget::Bookmark("m".into()), None),
        ]
    );
    assert_eq!(issue(&doc, "hyperlink not kept"), 1);
}

#[test]
fn damaged_and_nested_fields_degrade() {
    // A field over two paragraphs.
    let doc = open(&DocSpec::ansi("a\u{13} TOC \u{14}x\ry\u{15}z\r"));
    assert_eq!(texts(&doc), ["ax", "yz"]);
    assert_eq!(issue(&doc, "field spans paragraphs"), 1);
    // A field containing a field: the inner PAGE is kept.
    let doc = open(&DocSpec::ansi(
        "\u{13} SEQ n \u{14}\u{13} PAGE \u{14}1\u{15}x\u{15}\r",
    ));
    let p = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    assert_eq!(p.text, "1x");
    assert_eq!(p.fields.len(), 1);
    assert_eq!(p.fields[0].instruction, "PAGE");
    assert_eq!(issue(&doc, "field containing a field"), 1);
    // Stray separator and end marks.
    let doc = open(&DocSpec::ansi("a\u{15}b\u{14}c\r"));
    assert_eq!(texts(&doc), ["abc"]);
    assert_eq!(issue(&doc, "unmatched field mark"), 2);
    // A field without separator is entirely code; a non-closed one too.
    let doc = open(&DocSpec::ansi("a\u{13} SEQ x \r"));
    assert_eq!(texts(&doc), ["a"]);
    // Nesting beyond the limit is hidden, never an error.
    let limits = DocLimits {
        max_field_depth: 2,
        ..DocLimits::default()
    };
    let bytes =
        DocSpec::ansi("\u{13}a\u{13}b\u{13}c\u{13}d\u{14}R\u{15}\u{15}\u{15}\u{15}z\r").build();
    let doc = import_doc_with_limits(&bytes, limits).unwrap();
    doc.document.validate().unwrap();
    assert_eq!(issue(&doc, "field nesting too deep"), 1);
    let limits = DocLimits {
        max_fields: 2,
        ..DocLimits::default()
    };
    assert!(matches!(
        import_doc_with_limits(&bytes, limits),
        Err(DocError::Limit("field count"))
    ));
}

// ----- bookmarks ------------------------------------------------------------------------

fn bookmark_tables(
    names: &[&str],
    ranges: &[(u32, u32)],
    end: u32,
) -> Vec<(usize, Vec<u8>, Option<u32>)> {
    let mut sttb = vec![0xFF, 0xFF];
    sttb.extend_from_slice(&(names.len() as u16).to_le_bytes());
    sttb.extend_from_slice(&[0, 0]);
    for n in names {
        let units: Vec<u16> = n.encode_utf16().collect();
        sttb.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for u in units {
            sttb.extend_from_slice(&u.to_le_bytes());
        }
    }
    let mut bkf: Vec<u8> = ranges.iter().flat_map(|r| r.0.to_le_bytes()).collect();
    bkf.extend_from_slice(&end.to_le_bytes());
    for i in 0..ranges.len() {
        bkf.extend_from_slice(&(i as u16).to_le_bytes());
        bkf.extend_from_slice(&[0, 0]);
    }
    let mut bkl: Vec<u8> = ranges.iter().flat_map(|r| r.1.to_le_bytes()).collect();
    bkl.extend_from_slice(&end.to_le_bytes());
    vec![(21, sttb, None), (22, bkf, None), (23, bkl, None)]
}

#[test]
fn bookmarks_cut_at_paragraph_ends_and_duplicates() {
    // "one\r" (cp 0-3) "two\r" (4-7) "three\r" (8-13).
    let mut spec = DocSpec::ansi("one\rtwo\rthree\r");
    spec.table_extra = bookmark_tables(
        &["A", "B", "A", "C", "D"],
        &[(1, 2), (2, 6), (4, 5), (5, 4), (8, 8)],
        14,
    );
    let doc = open(&spec);
    let p0 = doc.document.paragraph(doc.paragraph_ids()[0]).unwrap();
    let names: Vec<_> = p0
        .bookmarks
        .iter()
        .map(|b| (b.name.as_str(), b.start, b.end))
        .collect();
    assert_eq!(names, [("A", 1, 2), ("B", 2, 3)]);
    // B crossed the paragraph end; the second A is a duplicate, C ends before
    // it starts; D is a point at the start of "three".
    let b = doc.bookmarks.iter().find(|b| b.name == "B").unwrap();
    assert!(b.truncated);
    assert!(doc.notices.contains(&ImportNotice::BookmarkAdjusted {
        what: "duplicate name",
        count: 1
    }));
    assert!(doc.notices.contains(&ImportNotice::BookmarkAdjusted {
        what: "end before start",
        count: 1
    }));
    let p2 = doc.document.paragraph(doc.paragraph_ids()[2]).unwrap();
    assert_eq!(p2.bookmarks.len(), 1);
    assert_eq!((p2.bookmarks[0].start, p2.bookmarks[0].end), (0, 0));
    assert!(doc.document.bookmark_named("A").is_some());
}
