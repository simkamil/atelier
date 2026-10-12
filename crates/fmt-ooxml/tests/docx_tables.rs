// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-303: DOCX table import (grid, merges, properties, table styles).
use atelier_fmt_ooxml::{
    CellVerticalAlign, ConditionalKind, DocxError, DocxLimits, HeightRule, ImportNotice,
    ImportedDocx, Package, PartUri, TableAlign, TableColor, TableLayout, TableWidth, TargetMode,
    import_docx, import_docx_with_limits,
};
use atelier_text_model::rich::{Color, Node, NodeId, Table, Theme, ThemeColor};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAIN_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const STYLES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

fn docx(body: &str, styles: Option<&str>) -> Vec<u8> {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#).into_bytes(),
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
    if let Some(styles) = styles {
        p.insert_part(
            PartUri::parse("word/styles.xml").unwrap(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
            format!(r#"<w:styles xmlns:w="{W}">{styles}</w:styles>"#).into_bytes(),
        )
        .unwrap();
        p.add_relationship(
            Some(PartUri::parse("word/document.xml").unwrap()),
            "rStyles",
            STYLES_REL,
            "styles.xml",
            TargetMode::Internal,
        )
        .unwrap();
    }
    p.write().unwrap()
}

fn import(body: &str) -> ImportedDocx {
    import_docx(&docx(body, None)).unwrap()
}

fn tc(text: &str) -> String {
    format!(r#"<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#)
}

fn row(cells: &[&str]) -> String {
    let inner: String = cells.iter().map(|c| tc(c)).collect();
    format!("<w:tr>{inner}</w:tr>")
}

fn grid(n: usize) -> String {
    let cols: String = (0..n).map(|_| r#"<w:gridCol w:w="1000"/>"#).collect();
    format!("<w:tblGrid>{cols}</w:tblGrid>")
}

fn blocks(d: &ImportedDocx) -> Vec<NodeId> {
    let section = d.document.section_ids()[0];
    d.document.block_ids(Some(section)).unwrap()
}

fn table_of(d: &ImportedDocx, id: NodeId) -> &Table {
    match d.document.node(id) {
        Some(Node::Table(t)) => t,
        other => panic!("not a table: {other:?}"),
    }
}

fn first_table(d: &ImportedDocx) -> (NodeId, &Table) {
    let id = blocks(d)
        .into_iter()
        .find(|id| matches!(d.document.node(*id), Some(Node::Table(_))))
        .expect("a table");
    (id, table_of(d, id))
}

fn cell_ids(d: &ImportedDocx, row: NodeId) -> Vec<NodeId> {
    match d.document.node(row) {
        Some(Node::Row(r)) => r.cells.clone(),
        other => panic!("not a row: {other:?}"),
    }
}

fn cell_text(d: &ImportedDocx, cell: NodeId) -> String {
    let Some(Node::Cell(c)) = d.document.node(cell) else {
        panic!("not a cell");
    };
    c.blocks
        .to_vec()
        .into_iter()
        .filter_map(|b| match d.document.node(b) {
            Some(Node::Paragraph(p)) => Some(p.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text of every cell, per row, in model order.
fn texts(d: &ImportedDocx, table: &Table) -> Vec<Vec<String>> {
    table
        .rows
        .iter()
        .map(|r| {
            cell_ids(d, *r)
                .into_iter()
                .map(|c| cell_text(d, c))
                .collect()
        })
        .collect()
}

fn spans(d: &ImportedDocx, cell: NodeId) -> (usize, usize) {
    let Some(Node::Cell(c)) = d.document.node(cell) else {
        panic!("not a cell");
    };
    (c.col_span, c.row_span)
}

fn has_notice(d: &ImportedDocx, local: &str) -> bool {
    d.notices.iter().any(|n| {
        matches!(n, ImportNotice::UnsupportedProperty { local_name, .. } if local_name == local)
    })
}

#[test]
fn simple_table_has_grid_widths_text_and_keeps_surrounding_paragraphs() {
    let body = format!(
        r#"<w:p><w:r><w:t>before</w:t></w:r></w:p><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="6000"/></w:tblGrid>{}{}</w:tbl><w:p><w:r><w:t>after</w:t></w:r></w:p>"#,
        row(&["a", "b"]),
        row(&["c", "d"])
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let b = blocks(&d);
    assert_eq!(b.len(), 3);
    assert!(matches!(d.document.node(b[0]), Some(Node::Paragraph(p)) if p.text == "before"));
    assert!(matches!(d.document.node(b[2]), Some(Node::Paragraph(p)) if p.text == "after"));
    let table = table_of(&d, b[1]);
    assert_eq!(table.columns, 2);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(texts(&d, table), vec![vec!["a", "b"], vec!["c", "d"]]);
    let info = &d.tables.tables[&b[1]];
    assert_eq!(info.grid, vec![Some(3000), Some(6000)]);
    assert_eq!(info.depth, 0);
    // Every XML paragraph is still listed, in document order.
    assert_eq!(d.paragraph_ids().len(), 6);
    // Tables are handled: no "unsupported element" notice for the structure.
    assert!(!d.notices.iter().any(|n| matches!(
        n,
        ImportNotice::UnsupportedElement { local_name, .. }
            if ["tbl", "tr", "tc", "tblPr", "tblGrid", "gridCol"].contains(&local_name.as_str())
    )));
}

#[test]
fn table_first_in_body_keeps_a_trailing_empty_paragraph() {
    let d = import(&format!("<w:tbl>{}{}</w:tbl>", grid(1), row(&["x"])));
    d.document.validate().unwrap();
    let b = blocks(&d);
    assert_eq!(b.len(), 2);
    assert!(matches!(d.document.node(b[0]), Some(Node::Table(_))));
    assert!(matches!(d.document.node(b[1]), Some(Node::Paragraph(p)) if p.text.is_empty()));
}

#[test]
fn grid_span_becomes_a_column_span() {
    let merged = r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>"#;
    let d = import(&format!(
        "<w:tbl>{}{merged}{}</w:tbl>",
        grid(3),
        row(&["1", "2", "3"])
    ));
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.columns, 3);
    let first = cell_ids(&d, t.rows[0]);
    assert_eq!(first.len(), 2);
    assert_eq!(spans(&d, first[0]), (2, 1));
    assert_eq!(spans(&d, first[1]), (1, 1));
    assert_eq!(d.tables.cells[&first[0]].column_index, 0);
    assert_eq!(d.tables.cells[&first[1]].column_index, 2);
}

fn vmerge_cell(kind: &str, text: &str) -> String {
    let m = match kind {
        "restart" => r#"<w:vMerge w:val="restart"/>"#,
        "continue" => "<w:vMerge/>",
        _ => "",
    };
    format!(r#"<w:tc><w:tcPr>{m}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>"#)
}

#[test]
fn vertical_merge_becomes_a_row_span_and_drops_continuations() {
    let body = format!(
        "<w:tbl>{}<w:tr>{}{}</w:tr><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        grid(2),
        vmerge_cell("restart", "top"),
        tc("r0"),
        vmerge_cell("continue", ""),
        tc("r1"),
        vmerge_cell("continue", ""),
        tc("r2"),
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(
        texts(&d, t),
        vec![vec!["top", "r0"], vec!["r1"], vec!["r2"]]
    );
    let first = cell_ids(&d, t.rows[0]);
    assert_eq!(spans(&d, first[0]), (1, 3));
    assert_eq!(d.tables.cells[&first[0]].row_span, 3);
    assert!(!has_notice(&d, "vMerge"));
}

#[test]
fn vertical_merge_without_a_start_is_a_plain_cell_with_a_notice() {
    let body = format!(
        "<w:tbl>{}<w:tr>{}{}</w:tr></w:tbl>",
        grid(2),
        vmerge_cell("continue", "orphan"),
        tc("b"),
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(texts(&d, t), vec![vec!["orphan", "b"]]);
    assert!(has_notice(&d, "vMerge"));
}

#[test]
fn continuation_with_content_moves_it_to_the_first_cell() {
    let body = format!(
        "<w:tbl>{}<w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        grid(2),
        vmerge_cell("restart", "keep"),
        tc("a"),
        vmerge_cell("continue", "hidden"),
        tc("b"),
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(texts(&d, t), vec![vec!["keep\nhidden", "a"], vec!["b"]]);
    assert!(has_notice(&d, "vMerge"));
}

#[test]
fn grid_before_and_after_pad_with_synthetic_cells() {
    let shifted = r#"<w:tr><w:trPr><w:gridBefore w:val="1"/><w:wBefore w:w="500" w:type="dxa"/><w:gridAfter w:val="1"/><w:wAfter w:w="700" w:type="dxa"/></w:trPr><w:tc><w:p><w:r><w:t>mid</w:t></w:r></w:p></w:tc></w:tr>"#;
    let d = import(&format!(
        "<w:tbl>{}{shifted}{}</w:tbl>",
        grid(3),
        row(&["a", "b", "c"])
    ));
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.columns, 3);
    assert_eq!(texts(&d, t), vec![vec!["", "mid", ""], vec!["a", "b", "c"]]);
    let cells = cell_ids(&d, t.rows[0]);
    assert!(d.tables.cells[&cells[0]].synthetic);
    assert!(!d.tables.cells[&cells[1]].synthetic);
    assert!(d.tables.cells[&cells[2]].synthetic);
    let rp = &d.tables.rows[&t.rows[0]].props;
    assert_eq!((rp.grid_before, rp.grid_after), (1, 1));
    assert_eq!(rp.width_before, Some(TableWidth::Dxa(500)));
    assert_eq!(rp.width_after, Some(TableWidth::Dxa(700)));
}

#[test]
fn legacy_horizontal_merge_is_converted_to_a_span() {
    let legacy = r#"<w:tr><w:tc><w:tcPr><w:hMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>h</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:hMerge w:val="continue"/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>z</w:t></w:r></w:p></w:tc></w:tr>"#;
    let d = import(&format!("<w:tbl>{}{legacy}</w:tbl>", grid(3)));
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    let cells = cell_ids(&d, t.rows[0]);
    assert_eq!(cells.len(), 2);
    assert_eq!(spans(&d, cells[0]), (2, 1));
    assert!(has_notice(&d, "hMerge"));
}

#[test]
fn uneven_rows_and_inconsistent_grids_are_normalised_with_notices() {
    // Rows of 1, 3 and 2 cells; grid declares 2 columns.
    let body = format!(
        "<w:tbl>{}{}{}{}</w:tbl>",
        grid(2),
        row(&["a"]),
        row(&["b", "c", "d"]),
        row(&["e", "f"])
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let (id, t) = first_table(&d);
    assert_eq!(t.columns, 3);
    let first = cell_ids(&d, t.rows[0]);
    assert_eq!(first.len(), 2);
    assert_eq!(spans(&d, first[1]), (2, 1));
    assert!(d.tables.cells[&first[1]].synthetic);
    assert_eq!(d.tables.tables[&id].grid.len(), 3);
    assert!(has_notice(&d, "tblGrid"));
    assert!(has_notice(&d, "tr"));
}

#[test]
fn grid_wider_than_every_row_is_truncated() {
    let d = import(&format!("<w:tbl>{}{}</w:tbl>", grid(5), row(&["a", "b"])));
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.columns, 2);
    assert!(has_notice(&d, "tblGrid"));
}

#[test]
fn empty_cells_and_rows_get_an_empty_paragraph() {
    let d = import("<w:tbl><w:tr><w:tc/></w:tr><w:tr/></w:tbl>");
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.columns, 1);
    assert_eq!(t.rows.len(), 2);
    let d = import("<w:tbl/>");
    d.document.validate().unwrap();
    assert!(
        blocks(&d)
            .iter()
            .all(|b| !matches!(d.document.node(*b), Some(Node::Table(_))))
    );
    assert!(has_notice(&d, "tbl"));
}

#[test]
fn nested_tables_are_imported_and_mapped_to_their_cells() {
    let inner = format!("<w:tbl>{}{}</w:tbl>", grid(1), row(&["deep"]));
    let outer = format!(
        "<w:tbl>{}<w:tr><w:tc><w:p><w:r><w:t>lead</w:t></w:r></w:p>{inner}<w:p><w:r><w:t>tail</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>side</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        grid(2)
    );
    let d = import(&outer);
    d.document.validate().unwrap();
    let (outer_id, t) = first_table(&d);
    let cells = cell_ids(&d, t.rows[0]);
    let Some(Node::Cell(cell)) = d.document.node(cells[0]) else {
        panic!()
    };
    let kinds: Vec<_> = cell
        .blocks
        .to_vec()
        .into_iter()
        .map(|b| matches!(d.document.node(b), Some(Node::Table(_))))
        .collect();
    assert_eq!(kinds, vec![false, true, false]);
    let nested = cell.blocks.get(1).unwrap();
    let nested_table = table_of(&d, nested);
    assert_eq!(texts(&d, nested_table), vec![vec!["deep"]]);
    assert_eq!(d.tables.tables[&nested].depth, 1);
    assert_eq!(d.tables.tables[&outer_id].depth, 0);
    // The nested paragraph belongs to the inner cell, not the outer one.
    let inner_cell = cell_ids(&d, nested_table.rows[0])[0];
    let inner_para = match d.document.node(inner_cell) {
        Some(Node::Cell(c)) => c.blocks.get(0).unwrap(),
        _ => panic!(),
    };
    assert_eq!(d.tables.paragraph_cell[&inner_para], inner_cell);
    // lead, deep, tail, side and the final empty paragraph of the section.
    assert_eq!(d.paragraph_ids().len(), 5);
}

#[test]
fn nesting_beyond_the_depth_limit_flattens_to_paragraphs() {
    let l3 = format!("<w:tbl>{}{}</w:tbl>", grid(1), row(&["three"]));
    let l2 = format!(
        "<w:tbl>{}<w:tr><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p>{l3}</w:tc></w:tr></w:tbl>",
        grid(1)
    );
    let l1 = format!(
        "<w:tbl>{}<w:tr><w:tc><w:p><w:r><w:t>one</w:t></w:r></w:p>{l2}</w:tc></w:tr></w:tbl>",
        grid(1)
    );
    let limits = DocxLimits {
        max_table_depth: 2,
        ..DocxLimits::default()
    };
    let d = import_docx_with_limits(&docx(&l1, None), limits).unwrap();
    d.document.validate().unwrap();
    assert_eq!(d.tables.tables.len(), 2);
    let all: Vec<String> = d
        .paragraph_ids()
        .iter()
        .map(|p| d.document.paragraph(*p).unwrap().text.clone())
        .collect();
    assert!(all.contains(&"three".to_owned()));
    assert!(has_notice(&d, "tbl"));
}

#[test]
fn table_limits_are_typed_errors() {
    let many_rows = format!("<w:tbl>{}</w:tbl>", row(&["x"]).repeat(5));
    let limits = DocxLimits {
        max_table_rows: 4,
        ..DocxLimits::default()
    };
    assert!(matches!(
        import_docx_with_limits(&docx(&many_rows, None), limits),
        Err(DocxError::Limit("table rows"))
    ));
    let limits = DocxLimits {
        max_table_cells: 4,
        ..DocxLimits::default()
    };
    assert!(matches!(
        import_docx_with_limits(&docx(&many_rows, None), limits),
        Err(DocxError::Limit("table cells"))
    ));
}

#[test]
fn absurd_spans_and_widths_are_clamped_not_fatal() {
    let weird = r#"<w:tbl><w:tblPr><w:tblW w:w="-5" w:type="dxa"/><w:tblInd w:w="-120" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="-10"/><w:gridCol w:w="abc"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="-30" w:type="dxa"/><w:gridSpan w:val="4000000000"/></w:tcPr><w:p/></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="0"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:gridSpan w:val="-3"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#;
    let d = import(weird);
    d.document.validate().unwrap();
    let (id, t) = first_table(&d);
    assert_eq!(t.columns, DocxLimits::default().max_table_columns);
    let info = &d.tables.tables[&id];
    // Negative table width is kept as written; indents may legitimately be negative.
    assert_eq!(info.props.indent, Some(TableWidth::Dxa(-120)));
    let first = cell_ids(&d, t.rows[0])[0];
    assert_eq!(d.tables.cells[&first].props.width, None);
    assert!(has_notice(&d, "gridSpan"));
}

#[test]
fn table_row_and_cell_properties_are_read() {
    let body = r#"<w:tbl>
<w:tblPr><w:tblStyle w:val="Nope"/><w:tblW w:w="2500" w:type="pct"/><w:jc w:val="center"/><w:tblInd w:w="240" w:type="dxa"/><w:tblLayout w:type="fixed"/>
<w:tblCellMar><w:top w:w="10" w:type="dxa"/><w:left w:w="20" w:type="dxa"/><w:bottom w:w="30" w:type="dxa"/><w:right w:w="40" w:type="dxa"/></w:tblCellMar>
<w:tblCellSpacing w:w="15" w:type="dxa"/>
<w:tblBorders><w:top w:val="single" w:sz="12" w:space="0" w:color="FF0000"/><w:left w:val="double" w:sz="4" w:themeColor="accent2" w:themeTint="99"/><w:bottom w:val="nil"/><w:right w:val="dotted" w:sz="6" w:color="auto"/><w:insideH w:val="single" w:sz="2" w:color="00FF00"/><w:insideV w:val="single" w:sz="2" w:color="0000FF"/></w:tblBorders>
<w:shd w:val="clear" w:color="auto" w:fill="EEEEEE"/>
<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/>
</w:tblPr><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid>
<w:tr><w:trPr><w:trHeight w:val="400" w:hRule="exact"/><w:tblHeader/><w:cantSplit/><w:jc w:val="right"/></w:trPr>
<w:tc><w:tcPr><w:tcW w:w="1500" w:type="dxa"/><w:tcBorders><w:top w:val="single" w:sz="8" w:color="123456"/><w:tl2br w:val="single" w:sz="4" w:color="000000"/></w:tcBorders><w:shd w:val="solid" w:color="ABCDEF" w:fill="auto"/><w:tcMar><w:left w:w="99" w:type="dxa"/></w:tcMar><w:vAlign w:val="bottom"/><w:noWrap/><w:textDirection w:val="btLr"/><w:tcFitText/></w:tcPr><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>
<w:tc><w:p/></w:tc></w:tr>
<w:tr><w:trPr><w:trHeight w:val="300"/></w:trPr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"#;
    let d = import(body);
    d.document.validate().unwrap();
    let (id, t) = first_table(&d);
    // Unknown style ids are not applied to the model (they would break resolution).
    assert_eq!(t.style, None);
    assert!(has_notice(&d, "tblStyle"));
    let p = &d.tables.tables[&id].props;
    assert_eq!(p.style.as_deref(), Some("Nope"));
    assert_eq!(p.width, Some(TableWidth::Pct(2500)));
    assert_eq!(p.align, Some(TableAlign::Center));
    assert_eq!(p.indent, Some(TableWidth::Dxa(240)));
    assert_eq!(p.layout, Some(TableLayout::Fixed));
    assert_eq!(p.cell_margins.top, Some(TableWidth::Dxa(10)));
    assert_eq!(p.cell_margins.right, Some(TableWidth::Dxa(40)));
    assert_eq!(p.cell_spacing, Some(TableWidth::Dxa(15)));
    assert_eq!(p.borders.top.as_ref().unwrap().size, Some(12));
    assert_eq!(
        p.borders.top.as_ref().unwrap().color,
        Some(TableColor::Rgb(Color { r: 255, g: 0, b: 0 }))
    );
    assert_eq!(
        p.borders.left.as_ref().unwrap().color,
        Some(TableColor::Theme {
            slot: ThemeColor::Accent2,
            tint: Some(0x99),
            shade: None
        })
    );
    assert!(p.borders.bottom.as_ref().unwrap().is_none());
    assert_eq!(
        p.borders.right.as_ref().unwrap().color,
        Some(TableColor::Auto)
    );
    assert!(p.borders.inside_h.is_some() && p.borders.inside_v.is_some());
    assert_eq!(p.shading.as_ref().unwrap().pattern, "clear");
    let look = p.look.unwrap();
    assert!(look.first_row && look.first_column && look.no_vertical_band);
    assert!(!look.last_row && !look.last_column && !look.no_horizontal_band);

    let r0 = &d.tables.rows[&t.rows[0]].props;
    assert_eq!(r0.height.unwrap().twips, 400);
    assert_eq!(r0.height.unwrap().rule, HeightRule::Exact);
    assert_eq!(r0.header, Some(true));
    assert_eq!(r0.cant_split, Some(true));
    assert_eq!(r0.align, Some(TableAlign::Right));
    let r1 = &d.tables.rows[&t.rows[1]].props;
    assert_eq!(r1.height.unwrap().rule, HeightRule::AtLeast);

    let c = cell_ids(&d, t.rows[0])[0];
    let cp = &d.tables.cells[&c].props;
    assert_eq!(cp.width, Some(TableWidth::Dxa(1500)));
    assert_eq!(cp.borders.top.as_ref().unwrap().size, Some(8));
    assert!(cp.borders.diagonal_down.is_some());
    assert_eq!(cp.margins.left, Some(TableWidth::Dxa(99)));
    assert_eq!(cp.vertical_align, Some(CellVerticalAlign::Bottom));
    assert_eq!(cp.no_wrap, Some(true));
    assert_eq!(cp.text_direction.as_deref(), Some("btLr"));
    assert_eq!(cp.fit_text, Some(true));
    assert!(has_notice(&d, "textDirection"));
    assert!(has_notice(&d, "tcFitText"));
    assert!(has_notice(&d, "tcBorders"));

    // Original property elements are kept verbatim in the model's opaque slots.
    let table_node = table_of(&d, id);
    let tbl_pr = table_node
        .opaque
        .iter()
        .find(|a| a.name == "tblPr")
        .unwrap();
    let xml = String::from_utf8(tbl_pr.value.clone()).unwrap();
    assert!(xml.starts_with("<w:tblPr xmlns:w=") && xml.contains(r#"<w:tblW w:w="2500""#));
    assert!(table_node.opaque.iter().any(|a| a.name == "tblGrid"));
    let Some(Node::Row(row)) = d.document.node(t.rows[0]) else {
        panic!()
    };
    assert!(row.opaque.iter().any(|a| a.name == "trPr"));
    let Some(Node::Cell(cell)) = d.document.node(c) else {
        panic!()
    };
    assert!(cell.opaque.iter().any(|a| a.name == "tcPr"));
}

#[test]
fn row_exceptions_and_tracked_row_deletions() {
    let body = format!(
        r#"<w:tbl>{}<w:tr><w:tblPrEx><w:tblW w:w="9000" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="010203"/></w:tblBorders></w:tblPrEx><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:trPr><w:del w:id="1" w:author="x"/></w:trPr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#,
        grid(1)
    );
    let d = import(&body);
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.rows.len(), 1);
    let ex = d.tables.rows[&t.rows[0]]
        .props
        .table_exceptions
        .as_ref()
        .unwrap();
    assert_eq!(ex.width, Some(TableWidth::Dxa(9000)));
    // The exception border reaches the cell.
    let cell = cell_ids(&d, t.rows[0])[0];
    let resolved = d.tables.resolve_cell(cell, &d.theme).unwrap();
    assert_eq!(
        resolved.borders.top.as_ref().unwrap().color,
        Some(TableColor::Rgb(Color { r: 1, g: 2, b: 3 }))
    );
    assert!(has_notice(&d, "trPr"));
}

const FANCY: &str = r#"
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"/>
<w:style w:type="paragraph" w:styleId="NoBold"><w:basedOn w:val="Normal"/><w:rPr><w:b w:val="0"/></w:rPr></w:style>
<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:tblPr><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
<w:style w:type="table" w:styleId="Fancy"><w:basedOn w:val="TableNormal"/>
<w:pPr><w:spacing w:after="0"/></w:pPr><w:rPr><w:sz w:val="20"/></w:rPr>
<w:tblPr><w:tblStyleRowBandSize w:val="1"/><w:tblBorders><w:top w:val="single" w:sz="8" w:color="112233"/><w:insideH w:val="single" w:sz="4" w:color="445566"/></w:tblBorders></w:tblPr>
<w:tblStylePr w:type="band1Horz"><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="DDDDDD"/></w:tcPr></w:tblStylePr>
<w:tblStylePr w:type="firstRow"><w:rPr><w:b/><w:color w:val="FFFFFF"/></w:rPr><w:trPr><w:tblHeader/></w:trPr><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="000080"/></w:tcPr></w:tblStylePr>
<w:tblStylePr w:type="firstCol"><w:rPr><w:i/></w:rPr><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="00FF00"/></w:tcPr></w:tblStylePr>
<w:tblStylePr w:type="nwCell"><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="FF0000"/></w:tcPr></w:tblStylePr>
<w:tblStylePr w:type="lastRow"><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="AAAAAA"/></w:tcPr></w:tblStylePr>
</w:style>"#;

fn fancy_table(look: &str, extra_cell_pr: &str, first_para_pr: &str) -> String {
    let mut rows = String::new();
    for r in 0..4 {
        rows.push_str("<w:tr>");
        for c in 0..3 {
            let pr = if r == 1 && c == 1 { extra_cell_pr } else { "" };
            let ppr = if r == 0 && c == 1 { first_para_pr } else { "" };
            rows.push_str(&format!(
                r#"<w:tc><w:tcPr>{pr}</w:tcPr><w:p><w:pPr>{ppr}</w:pPr><w:r><w:t>r{r}c{c}</w:t></w:r></w:p></w:tc>"#
            ));
        }
        rows.push_str("</w:tr>");
    }
    format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="Fancy"/>{look}</w:tblPr>{}{rows}</w:tbl>"#,
        grid(3)
    )
}

const LOOK: &str = r#"<w:tblLook w:val="04E0"/>"#;

fn fill_of(d: &ImportedDocx, table: &Table, r: usize, c: usize) -> Option<Color> {
    let cell = cell_ids(d, table.rows[r])[c];
    d.tables.resolve_cell(cell, &d.theme).unwrap().fill
}

fn conds_of(d: &ImportedDocx, table: &Table, r: usize, c: usize) -> Vec<ConditionalKind> {
    let cell = cell_ids(d, table.rows[r])[c];
    d.tables.resolve_cell(cell, &d.theme).unwrap().conditionals
}

#[test]
fn conditional_formatting_follows_the_specification_order_on_overlaps() {
    use ConditionalKind::*;
    let d = import_docx(&docx(&fancy_table(LOOK, "", ""), Some(FANCY))).unwrap();
    d.document.validate().unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(t.style.as_deref(), Some("Fancy"));
    let rgb = |r, g, b| Some(Color { r, g, b });
    // Corner: first row, first column and the corner zone overlap -> corner wins.
    assert_eq!(
        conds_of(&d, t, 0, 0),
        vec![WholeTable, FirstRow, FirstCol, NwCell]
    );
    assert_eq!(fill_of(&d, t, 0, 0), rgb(0xFF, 0, 0));
    // First row (no banding on the header row).
    assert_eq!(conds_of(&d, t, 0, 1), vec![WholeTable, FirstRow]);
    assert_eq!(fill_of(&d, t, 0, 1), rgb(0, 0, 0x80));
    // First column overlaps band 1: the column zone is applied later.
    assert_eq!(conds_of(&d, t, 1, 0), vec![WholeTable, Band1Horz, FirstCol]);
    assert_eq!(fill_of(&d, t, 1, 0), rgb(0, 0xFF, 0));
    // Plain banded cell, then the next band has no formatting.
    assert_eq!(fill_of(&d, t, 1, 1), rgb(0xDD, 0xDD, 0xDD));
    assert_eq!(conds_of(&d, t, 2, 1), vec![WholeTable, Band2Horz]);
    assert_eq!(fill_of(&d, t, 2, 1), None);
    // Rows rank below columns: the first column zone is applied after the last
    // row zone and wins where they cross without a corner definition.
    assert_eq!(
        conds_of(&d, t, 3, 0),
        vec![WholeTable, LastRow, FirstCol, SwCell]
    );
    assert_eq!(fill_of(&d, t, 3, 0), rgb(0, 0xFF, 0));
    // Borders: the style's outer top / inside horizontal borders map to cell edges.
    let cell = |r, c| cell_ids(&d, t.rows[r])[c];
    let top_row = d.tables.resolve_cell(cell(0, 1), &d.theme).unwrap();
    assert_eq!(top_row.borders.top.as_ref().unwrap().size, Some(8));
    let inner = d.tables.resolve_cell(cell(1, 1), &d.theme).unwrap();
    assert_eq!(inner.borders.top.as_ref().unwrap().size, Some(4));
    assert_eq!(inner.borders.bottom.as_ref().unwrap().size, Some(4));
    assert!(
        d.tables
            .resolve_cell(cell(3, 1), &d.theme)
            .unwrap()
            .borders
            .bottom
            .is_none()
    );
    // Inherited margins from the base table style, and the header row flag.
    assert_eq!(inner.margins.left, Some(TableWidth::Dxa(108)));
    assert_eq!(top_row.row_props.header, Some(true));
    assert_eq!(inner.row_props.header, None);
}

#[test]
fn look_flags_and_band_sizes_change_the_applicable_zones() {
    use ConditionalKind::*;
    // No look: no first row/column, bands on from the first row.
    let d = import_docx(&docx(
        &fancy_table(r#"<w:tblLook w:val="0000"/>"#, "", ""),
        Some(FANCY),
    ))
    .unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(
        conds_of(&d, t, 0, 0),
        vec![WholeTable, Band1Vert, Band1Horz]
    );
    assert_eq!(
        conds_of(&d, t, 1, 1),
        vec![WholeTable, Band2Vert, Band2Horz]
    );
    // Explicit attributes take precedence over the bit mask.
    let look = r#"<w:tblLook w:val="0000" w:firstRow="1" w:noHBand="1" w:noVBand="1"/>"#;
    let d = import_docx(&docx(&fancy_table(look, "", ""), Some(FANCY))).unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(conds_of(&d, t, 0, 0), vec![WholeTable, FirstRow]);
    assert_eq!(conds_of(&d, t, 2, 2), vec![WholeTable]);

    // Row band size 2 on the style: rows 0-1 band 1, rows 2-3 band 2.
    let styles = FANCY.replace(
        r#"<w:tblStyleRowBandSize w:val="1"/>"#,
        r#"<w:tblStyleRowBandSize w:val="2"/>"#,
    );
    let look = r#"<w:tblLook w:val="0200"/>"#;
    let _ = look;
    let d = import_docx(&docx(
        &fancy_table(r#"<w:tblLook w:val="0400"/>"#, "", ""),
        Some(&styles),
    ))
    .unwrap();
    let (_, t) = first_table(&d);
    assert_eq!(conds_of(&d, t, 1, 1), vec![WholeTable, Band1Horz]);
    assert_eq!(conds_of(&d, t, 2, 1), vec![WholeTable, Band2Horz]);
    assert_eq!(conds_of(&d, t, 3, 1), vec![WholeTable, Band2Horz]);
}

#[test]
fn direct_cell_properties_override_conditional_formatting() {
    let direct =
        r#"<w:shd w:val="clear" w:color="auto" w:fill="123456"/><w:vAlign w:val="center"/>"#;
    let d = import_docx(&docx(&fancy_table(LOOK, direct, ""), Some(FANCY))).unwrap();
    let (_, t) = first_table(&d);
    // Cell (1,1) would be band 1 grey.
    assert_eq!(
        fill_of(&d, t, 1, 1),
        Some(Color {
            r: 0x12,
            g: 0x34,
            b: 0x56
        })
    );
    let cell = cell_ids(&d, t.rows[1])[1];
    let r = d.tables.resolve_cell(cell, &d.theme).unwrap();
    assert_eq!(r.vertical_align, Some(CellVerticalAlign::Center));
    // The unaffected neighbour keeps the style's grey.
    assert_eq!(
        fill_of(&d, t, 1, 2),
        Some(Color {
            r: 0xDD,
            g: 0xDD,
            b: 0xDD
        })
    );
}

#[test]
fn conditional_character_formatting_reaches_resolved_paragraphs() {
    let mut d = import_docx(&docx(&fancy_table(LOOK, "", ""), Some(FANCY))).unwrap();
    let (_, t) = first_table(&d);
    let t = t.clone();
    let para_of = |d: &ImportedDocx, r: usize, c: usize| -> NodeId {
        let cell = cell_ids(d, t.rows[r])[c];
        let Some(Node::Cell(cell)) = d.document.node(cell) else {
            panic!()
        };
        cell.blocks.get(0).unwrap()
    };
    // Header cell: bold white from firstRow, 10 pt and no spacing from the whole style.
    let p01 = para_of(&d, 0, 1);
    let (resolved, props) = d.resolved_paragraph(p01).unwrap();
    assert_eq!(props.character.bold, Some(true));
    assert_eq!(
        props.character.color,
        Some(Color {
            r: 255,
            g: 255,
            b: 255
        })
    );
    assert_eq!(props.character.size_half_points, Some(20));
    assert_eq!(props.paragraph.space_after, Some(0));
    assert_eq!(resolved.runs[0].attrs.bold, Some(true));
    // Header + first column corner: italic from firstCol too.
    let (_, corner) = d.resolved_paragraph(para_of(&d, 0, 0)).unwrap();
    assert_eq!(corner.character.italic, Some(true));
    assert_eq!(corner.character.bold, Some(true));
    // Body cell: neither.
    let (_, body) = d.resolved_paragraph(para_of(&d, 2, 2)).unwrap();
    assert_eq!(body.character.bold, None);
    assert_eq!(body.character.size_half_points, Some(20));
}

#[test]
fn paragraph_style_and_direct_formatting_outrank_conditional_formatting() {
    let styles = FANCY;
    // Header cell paragraph whose style turns bold off, and one with a direct run override.
    let body = fancy_table(LOOK, "", r#"<w:pStyle w:val="NoBold"/>"#);
    let mut d = import_docx(&docx(&body, Some(styles))).unwrap();
    let (_, t) = first_table(&d);
    let cell = cell_ids(&d, t.rows[0].to_owned())[1];
    let Some(Node::Cell(c)) = d.document.node(cell) else {
        panic!()
    };
    let p = c.blocks.get(0).unwrap();
    let (_, props) = d.resolved_paragraph(p).unwrap();
    assert_eq!(props.character.bold, Some(false));
    // Colour is not set by the paragraph style, so the conditional white still applies.
    assert_eq!(
        props.character.color,
        Some(Color {
            r: 255,
            g: 255,
            b: 255
        })
    );

    let direct = r#"<w:tbl><w:tblPr><w:tblStyle w:val="Fancy"/><w:tblLook w:val="0020"/></w:tblPr><w:tblGrid><w:gridCol w:w="1"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>x</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let mut d = import_docx(&docx(direct, Some(styles))).unwrap();
    let p = d.paragraph_ids()[0];
    let (resolved, _) = d.resolved_paragraph(p).unwrap();
    assert_eq!(resolved.runs[0].attrs.bold, Some(false));
    assert_eq!(
        resolved.runs[0].attrs.color,
        Some(Color {
            r: 255,
            g: 255,
            b: 255
        })
    );
}

#[test]
fn paragraphs_outside_tables_resolve_as_before() {
    let mut d = import_docx(&docx("<w:p><w:r><w:t>plain</w:t></w:r></w:p>", Some(FANCY))).unwrap();
    let p = d.paragraph_ids()[0];
    let (_, props) = d.resolved_paragraph(p).unwrap();
    assert_eq!(props.character.bold, None);
    assert_eq!(props.paragraph.space_after, None);
}

#[test]
fn style_cycles_and_unusable_table_styles_never_break_resolution() {
    let styles = r#"<w:style w:type="table" w:styleId="A"><w:basedOn w:val="B"/></w:style><w:style w:type="table" w:styleId="B"><w:basedOn w:val="A"/></w:style><w:style w:type="paragraph" w:styleId="P"/>"#;
    let body = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="A"/></w:tblPr>{}</w:tbl><w:tbl><w:tblPr><w:tblStyle w:val="P"/></w:tblPr>{}</w:tbl>"#,
        row(&["x"]),
        row(&["y"])
    );
    let mut d = import_docx(&docx(&body, Some(styles))).unwrap();
    for id in d.paragraph_ids().to_vec() {
        d.resolved_paragraph(id).unwrap();
    }
    for id in d.tables.cells.keys().copied().collect::<Vec<_>>() {
        d.tables.resolve_cell(id, &d.theme).unwrap();
    }
}

#[test]
fn theme_colours_tint_and_shade() {
    let mut theme = Theme::default();
    theme.colors.insert(
        ThemeColor::Accent1,
        Color {
            r: 100,
            g: 200,
            b: 0,
        },
    );
    let c = |tint, shade| TableColor::Theme {
        slot: ThemeColor::Accent1,
        tint,
        shade,
    };
    assert_eq!(
        c(None, None).resolve(&theme),
        Some(Color {
            r: 100,
            g: 200,
            b: 0
        })
    );
    assert_eq!(
        c(None, Some(0x80)).resolve(&theme),
        Some(Color {
            r: 50,
            g: 100,
            b: 0
        })
    );
    assert_eq!(
        c(Some(0x80), None).resolve(&theme),
        Some(Color {
            r: 178,
            g: 228,
            b: 128
        })
    );
    assert_eq!(TableColor::Auto.resolve(&theme), None);
    assert_eq!(
        TableColor::Theme {
            slot: ThemeColor::Accent2,
            tint: None,
            shade: None
        }
        .resolve(&theme),
        None
    );
}

// ---- Robustness: seeded mutations and truncations -----------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn corpus() -> Vec<String> {
    let nested = format!(
        "<w:tbl>{}<w:tr><w:tc><w:p><w:r><w:t>o</w:t></w:r></w:p><w:tbl>{}{}</w:tbl></w:tc></w:tr></w:tbl>",
        grid(1),
        grid(2),
        row(&["i", "j"])
    );
    let merged = format!(
        "<w:tbl>{}<w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
        grid(2),
        vmerge_cell("restart", "a"),
        tc("b"),
        vmerge_cell("continue", ""),
        tc("d")
    );
    vec![
        fancy_table(LOOK, "<w:gridSpan w:val=\"2\"/>", ""),
        nested,
        merged,
        r#"<w:tbl><w:tr><w:trPr><w:gridBefore w:val="2"/><w:gridAfter w:val="1"/></w:trPr><w:tc><w:tcPr><w:hMerge w:val="restart"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:hMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#
            .to_owned(),
    ]
}

#[test]
fn mutated_and_truncated_table_documents_never_panic() {
    let mut rng = Rng(0x5eed_u64 | 1);
    let mut accepted = 0usize;
    for body in corpus() {
        let full = docx(&body, Some(FANCY));
        // The unmodified document must import.
        import_docx(&full).unwrap();
        let alphabet: &[u8] = b"<>/\"= 0123456789-wxyz&;";
        for round in 0..300 {
            // Mutate only the document XML so the package stays readable.
            let mut xml =
                format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#)
                    .into_bytes();
            match round % 5 {
                0 => {
                    for _ in 0..1 + rng.below(4) {
                        let at = rng.below(xml.len());
                        xml[at] = alphabet[rng.below(alphabet.len())];
                    }
                }
                1 => {
                    let cut = rng.below(xml.len());
                    xml.truncate(cut);
                }
                2 => {
                    let at = rng.below(xml.len());
                    let end = (at + 1 + rng.below(40)).min(xml.len());
                    xml.drain(at..end);
                }
                3 => {
                    // Well-formed mutation: change numbers in place.
                    let values = ["0", "1", "2", "-1", "99999999999", "4294967296", "x", ""];
                    let mut text = String::from_utf8(xml).unwrap();
                    for _ in 0..1 + rng.below(3) {
                        let digits: Vec<usize> = text
                            .char_indices()
                            .filter(|(_, c)| c.is_ascii_digit())
                            .map(|(i, _)| i)
                            .collect();
                        if digits.is_empty() {
                            break;
                        }
                        let at = digits[rng.below(digits.len())];
                        text.replace_range(at..at + 1, values[rng.below(values.len())]);
                    }
                    xml = text.into_bytes();
                }
                _ => {
                    // Well-formed mutation: rename every use of one element.
                    let names = [
                        "w:tc",
                        "w:tr",
                        "w:gridSpan",
                        "w:vMerge",
                        "w:hMerge",
                        "w:tblGrid",
                        "w:gridCol",
                        "w:trPr",
                        "w:tcPr",
                        "w:tbl",
                        "w:p",
                        "w:t",
                    ];
                    let (a, b) = (names[rng.below(names.len())], names[rng.below(names.len())]);
                    let text = String::from_utf8(xml).unwrap();
                    xml = text
                        .replace(&format!("<{a} "), &format!("<{b} "))
                        .replace(&format!("<{a}>"), &format!("<{b}>"))
                        .replace(&format!("<{a}/>"), &format!("<{b}/>"))
                        .replace(&format!("</{a}>"), &format!("</{b}>"))
                        .into_bytes();
                }
            }
            let mut p = Package::new();
            p.insert_part(
                PartUri::parse("word/document.xml").unwrap(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
                xml,
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
            let bytes = p.write().unwrap();
            let result = std::panic::catch_unwind(|| import_docx(&bytes));
            let result = result.expect("import panicked");
            if let Ok(d) = result {
                accepted += 1;
                d.document.validate().expect("imported model is valid");
                for cell in d.tables.cells.keys() {
                    d.tables.resolve_cell(*cell, &d.theme).unwrap();
                }
            }
        }
        // Truncating the whole package never panics either.
        for _ in 0..50 {
            let cut = rng.below(full.len());
            let slice = &full[..cut];
            assert!(std::panic::catch_unwind(|| import_docx(slice)).is_ok());
        }
    }
    assert!(accepted > 300, "mutations were all rejected: {accepted}");
}

#[test]
fn row_band_size_set_directly_on_the_table_overrides_the_style() {
    use ConditionalKind::*;
    let body = fancy_table(
        r#"<w:tblLook w:val="0400"/><w:tblStyleRowBandSize w:val="2"/>"#,
        "",
        "",
    );
    let d = import_docx(&docx(&body, Some(FANCY))).unwrap();
    let (_, t) = first_table(&d);
    // The style says 1; the table says 2: rows 0-1 band 1, rows 2-3 band 2.
    assert_eq!(conds_of(&d, t, 1, 1), vec![WholeTable, Band1Horz]);
    assert_eq!(conds_of(&d, t, 2, 1), vec![WholeTable, Band2Horz]);
}

#[test]
fn paragraph_ids_lists_every_paragraph_including_the_final_empty_one() {
    let d = import(&format!("<w:tbl>{}{}</w:tbl>", grid(1), row(&["x"])));
    assert_eq!(d.paragraph_ids().len(), 2);
    assert_eq!(d.paragraph_ids(), d.document.paragraph_ids().as_slice());
    let d = import("<w:tbl><w:tr><w:tc/></w:tr></w:tbl>");
    assert_eq!(d.paragraph_ids(), d.document.paragraph_ids().as_slice());
    assert_eq!(d.paragraph_ids().len(), 2);
}

fn continuation_doc(cont_paragraph: &str) -> ImportedDocx {
    let cont = format!("<w:tc><w:tcPr><w:vMerge/></w:tcPr>{cont_paragraph}</w:tc>");
    let body = format!(
        "<w:tbl>{}<w:tr>{}{}</w:tr><w:tr>{cont}{}</w:tr></w:tbl>",
        grid(2),
        vmerge_cell("restart", "a"),
        tc("b"),
        tc("d")
    );
    import(&body)
}

fn anchor_block_count(d: &ImportedDocx) -> usize {
    let (_, t) = first_table(d);
    let first = cell_ids(d, t.rows[0])[0];
    let Some(Node::Cell(c)) = d.document.node(first) else {
        panic!()
    };
    c.blocks.len()
}

#[test]
fn vmerge_continuation_keeping_only_non_text_content_is_not_dropped() {
    for content in [
        "<w:r><w:drawing/></w:r>",
        "<w:r><w:pict/></w:r>",
        "<w:r><w:tab/></w:r>",
        "<w:r><w:br/></w:r>",
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
    ] {
        let d = continuation_doc(&format!("<w:p>{content}</w:p>"));
        d.document.validate().unwrap();
        assert_eq!(anchor_block_count(&d), 2, "content kept for {content}");
        assert!(has_notice(&d, "vMerge"), "{content}");
    }
    // Tab stops in paragraph properties are not content.
    let d = continuation_doc(
        r#"<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="100"/></w:tabs></w:pPr></w:p>"#,
    );
    assert_eq!(anchor_block_count(&d), 1);
}
