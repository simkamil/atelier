// SPDX-License-Identifier: AGPL-3.0-or-later
//! Table import for WordprocessingML (US-303): `w:tbl` structure, merges,
//! table/row/cell properties, table styles with conditional formatting, and
//! the resolution of effective cell properties.
//!
//! The rich model stores a table as grid + rows + cells with spans and a style
//! identifier. Everything else (widths, borders, shading, margins, row rules,
//! look flags, ...) is kept in [`ImportedTables`], indexed by node id, and the
//! original `tblPr`/`trPr`/`tcPr`/`tblPrEx`/`tblGrid` elements are also kept
//! verbatim in the `opaque` attribute slot of the table, row and cell nodes.

use super::preserve::{self, Ev, Level, PreservedChild, PreservedElement, TreeBuilder};
use super::{
    DocxError, DocxLimits, ImportNotice, ParagraphSink, W_NS, W_STRICT_NS, XmlNode,
    collect_paragraphs, is_w, model_error, parse_color, parse_para_props, parse_style_char_attrs,
    theme_color_slot,
};
use atelier_text_model::rich::{
    BlockList, Cell, CharAttrs, Color, Node, NodeId, OpaqueAttr, OpaqueBlock, ParaProps, Paragraph,
    RichDocument, Row, StyleCatalog, StyleCharAttrs, Subtree, Table, Theme, ThemeColor,
};
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------
// Public data
// ---------------------------------------------------------------------------

/// A width as written by `w:w`/`w:type`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableWidth {
    Auto,
    Nil,
    /// Twentieths of a point (may be negative for indents).
    Dxa(i32),
    /// Fiftieths of a percent (5000 = 100 %).
    Pct(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableLayout {
    Fixed,
    Autofit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellVerticalAlign {
    Top,
    Center,
    Bottom,
    Both,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeightRule {
    Auto,
    AtLeast,
    Exact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowHeight {
    pub twips: u32,
    pub rule: HeightRule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VMerge {
    Restart,
    Continue,
}

/// A colour reference: automatic, explicit RGB or a theme slot with optional
/// tint/shade (`0..=255`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableColor {
    Auto,
    Rgb(Color),
    Theme {
        slot: ThemeColor,
        tint: Option<u8>,
        shade: Option<u8>,
    },
}

impl TableColor {
    /// The concrete colour; `None` for `Auto` or a theme slot the theme lacks.
    pub fn resolve(&self, theme: &Theme) -> Option<Color> {
        match *self {
            TableColor::Auto => None,
            TableColor::Rgb(c) => Some(c),
            TableColor::Theme { slot, tint, shade } => {
                let mut c = *theme.colors.get(&slot)?;
                if let Some(t) = tint {
                    // Blend towards white: t/255 of the colour remains.
                    c = mix(c, WHITE, t as u32 * 100 / 255, true);
                }
                if let Some(s) = shade {
                    c = mix(c, BLACK, s as u32 * 100 / 255, true);
                }
                Some(c)
            }
        }
    }
}

const WHITE: Color = Color {
    r: 255,
    g: 255,
    b: 255,
};
const BLACK: Color = Color { r: 0, g: 0, b: 0 };

/// `percent` of `b` over `a`; with `invert` the first colour is the one kept
/// at `percent` (tint/shade helper).
fn mix(a: Color, b: Color, percent: u32, invert: bool) -> Color {
    let (keep, other) = if invert { (a, b) } else { (b, a) };
    let p = percent.min(100);
    let m = |k: u8, o: u8| ((k as u32 * p + o as u32 * (100 - p) + 50) / 100) as u8;
    Color {
        r: m(keep.r, other.r),
        g: m(keep.g, other.g),
        b: m(keep.b, other.b),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Border {
    /// `w:val`, e.g. `single`, `double`, `nil`, `none`.
    pub style: String,
    /// Eighths of a point.
    pub size: Option<u32>,
    pub space: Option<u32>,
    pub color: Option<TableColor>,
}

impl Border {
    pub fn is_none(&self) -> bool {
        matches!(self.style.as_str(), "nil" | "none")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
    /// Top-left to bottom-right diagonal (`w:tl2br`).
    pub diagonal_down: Option<Border>,
    /// Top-right to bottom-left diagonal (`w:tr2bl`).
    pub diagonal_up: Option<Border>,
}

impl Borders {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    fn overlay(&mut self, o: &Borders) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f.clone(); } )*};
        }
        take!(
            top,
            left,
            bottom,
            right,
            inside_h,
            inside_v,
            diagonal_down,
            diagonal_up
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shading {
    /// `w:val`, e.g. `clear`, `solid`, `pct25`.
    pub pattern: String,
    pub color: Option<TableColor>,
    pub fill: Option<TableColor>,
}

impl Shading {
    /// The colour a cell is filled with, approximating patterns by blending.
    pub fn effective_fill(&self, theme: &Theme) -> Option<Color> {
        let fill = self.fill.and_then(|c| c.resolve(theme));
        let fore = match self.color {
            Some(TableColor::Auto) => Some(BLACK),
            Some(c) => c.resolve(theme),
            None => None,
        };
        match self.pattern.as_str() {
            "nil" => None,
            "clear" => fill,
            "solid" => fore.or(Some(BLACK)),
            p => {
                let percent = p
                    .strip_prefix("pct")
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(50);
                Some(mix(
                    fill.unwrap_or(WHITE),
                    fore.unwrap_or(BLACK),
                    percent,
                    false,
                ))
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CellMargins {
    pub top: Option<TableWidth>,
    pub left: Option<TableWidth>,
    pub bottom: Option<TableWidth>,
    pub right: Option<TableWidth>,
}

impl CellMargins {
    fn overlay(&mut self, o: &CellMargins) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f; } )*};
        }
        take!(top, left, bottom, right);
    }
}

/// `w:tblLook`. Without the element every flag is false (bands enabled).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableLook {
    pub first_row: bool,
    pub last_row: bool,
    pub first_column: bool,
    pub last_column: bool,
    pub no_horizontal_band: bool,
    pub no_vertical_band: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TableProps {
    pub style: Option<String>,
    pub width: Option<TableWidth>,
    pub align: Option<TableAlign>,
    pub indent: Option<TableWidth>,
    pub layout: Option<TableLayout>,
    pub cell_margins: CellMargins,
    pub cell_spacing: Option<TableWidth>,
    pub borders: Borders,
    pub shading: Option<Shading>,
    pub look: Option<TableLook>,
    /// `w:tblStyleRowBandSize` (table styles).
    pub row_band_size: Option<u32>,
    /// `w:tblStyleColBandSize` (table styles).
    pub column_band_size: Option<u32>,
    pub bidi_visual: Option<bool>,
    /// A floating table (`w:tblpPr`): positioning is not interpreted.
    pub floating: bool,
}

impl TableProps {
    fn overlay(&mut self, o: &TableProps) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f.clone(); } )*};
        }
        take!(
            style,
            width,
            align,
            indent,
            layout,
            cell_spacing,
            shading,
            look,
            row_band_size,
            column_band_size,
            bidi_visual
        );
        self.cell_margins.overlay(&o.cell_margins);
        self.borders.overlay(&o.borders);
        self.floating |= o.floating;
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RowProps {
    pub height: Option<RowHeight>,
    /// `w:tblHeader`: header row repeated on each page.
    pub header: Option<bool>,
    /// `w:cantSplit`.
    pub cant_split: Option<bool>,
    pub align: Option<TableAlign>,
    pub grid_before: u32,
    pub grid_after: u32,
    pub width_before: Option<TableWidth>,
    pub width_after: Option<TableWidth>,
    pub cell_spacing: Option<TableWidth>,
    pub hidden: Option<bool>,
    /// `w:tblPrEx`.
    pub table_exceptions: Option<TableProps>,
}

impl RowProps {
    fn overlay(&mut self, o: &RowProps) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f.clone(); } )*};
        }
        take!(
            height,
            header,
            cant_split,
            align,
            width_before,
            width_after,
            cell_spacing,
            hidden,
            table_exceptions
        );
        if o.grid_before != 0 {
            self.grid_before = o.grid_before;
        }
        if o.grid_after != 0 {
            self.grid_after = o.grid_after;
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CellProps {
    pub width: Option<TableWidth>,
    pub borders: Borders,
    pub shading: Option<Shading>,
    pub margins: CellMargins,
    pub vertical_align: Option<CellVerticalAlign>,
    pub no_wrap: Option<bool>,
    /// `w:textDirection` value (e.g. `btLr`).
    pub text_direction: Option<String>,
    /// `w:tcFitText`.
    pub fit_text: Option<bool>,
    pub hide_mark: Option<bool>,
}

/// Conditional formatting zones of a table style.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConditionalKind {
    WholeTable,
    Band1Vert,
    Band2Vert,
    Band1Horz,
    Band2Horz,
    FirstCol,
    LastCol,
    FirstRow,
    LastRow,
    NwCell,
    NeCell,
    SwCell,
    SeCell,
}

impl ConditionalKind {
    /// Application order: later entries override earlier ones. This is the
    /// single place to change if Word turns out to rank columns above rows.
    /// Reading of ECMA-376 §17.7.6, not yet checked against Word.
    pub const APPLY_ORDER: [ConditionalKind; 13] = [
        ConditionalKind::WholeTable,
        ConditionalKind::Band1Vert,
        ConditionalKind::Band2Vert,
        ConditionalKind::Band1Horz,
        ConditionalKind::Band2Horz,
        ConditionalKind::FirstRow,
        ConditionalKind::LastRow,
        ConditionalKind::FirstCol,
        ConditionalKind::LastCol,
        ConditionalKind::NwCell,
        ConditionalKind::NeCell,
        ConditionalKind::SwCell,
        ConditionalKind::SeCell,
    ];

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "wholeTable" => Self::WholeTable,
            "band1Vert" => Self::Band1Vert,
            "band2Vert" => Self::Band2Vert,
            "band1Horz" => Self::Band1Horz,
            "band2Horz" => Self::Band2Horz,
            "firstCol" => Self::FirstCol,
            "lastCol" => Self::LastCol,
            "firstRow" => Self::FirstRow,
            "lastRow" => Self::LastRow,
            "nwCell" => Self::NwCell,
            "neCell" => Self::NeCell,
            "swCell" => Self::SwCell,
            "seCell" => Self::SeCell,
            _ => return None,
        })
    }
}

/// One `w:tblStylePr` block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConditionalFormat {
    pub paragraph: ParaProps,
    pub character: StyleCharAttrs,
    pub table: TableProps,
    pub row: RowProps,
    pub cell: CellProps,
}

/// The table-specific part of a `w:style w:type="table"`. Its paragraph and
/// character properties live in the style engine as for any style.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TableStyleDef {
    pub id: String,
    pub based_on: Option<String>,
    pub is_default: bool,
    pub table: TableProps,
    pub row: RowProps,
    pub cell: CellProps,
    pub conditionals: BTreeMap<ConditionalKind, ConditionalFormat>,
}

/// Table-level data of an imported `w:tbl`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedTable {
    pub props: TableProps,
    /// Column widths from `w:tblGrid` (twips), normalised to `columns`.
    pub grid: Vec<Option<u32>>,
    pub columns: usize,
    pub rows: Vec<NodeId>,
    /// 0 for a table of the body, 1 for a table nested in a cell, ...
    pub depth: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedRow {
    pub props: RowProps,
    pub table: Option<NodeId>,
    pub index: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedCell {
    pub props: CellProps,
    pub table: Option<NodeId>,
    pub row: Option<NodeId>,
    pub row_index: usize,
    /// Grid column of the cell's left edge.
    pub column_index: usize,
    pub row_span: usize,
    pub col_span: usize,
    /// True for an empty cell added to keep the grid rectangular
    /// (`gridBefore`, `gridAfter`, short rows).
    pub synthetic: bool,
}

/// Table information that the rich model has no slot for, indexed by node id.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedTables {
    pub tables: BTreeMap<NodeId, ImportedTable>,
    pub rows: BTreeMap<NodeId, ImportedRow>,
    pub cells: BTreeMap<NodeId, ImportedCell>,
    /// Table styles of `styles.xml` (conditional formatting and table-level
    /// properties), by style id.
    pub styles: BTreeMap<String, TableStyleDef>,
    /// The `w:default="1"` table style, applied to tables without `tblStyle`.
    pub default_style: Option<String>,
    /// Paragraph directly inside a cell -> that cell.
    pub paragraph_cell: BTreeMap<NodeId, NodeId>,
}

/// Effective properties of a cell after table style, conditional formatting
/// and direct table/row/cell properties.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedCell {
    pub table: Option<NodeId>,
    pub row: Option<NodeId>,
    pub row_index: usize,
    pub column_index: usize,
    pub row_span: usize,
    pub col_span: usize,
    /// Conditional zones applied, in application order.
    pub conditionals: Vec<ConditionalKind>,
    pub table_props: TableProps,
    pub row_props: RowProps,
    pub width: Option<TableWidth>,
    /// Effective top/left/bottom/right and diagonal borders (inside borders
    /// are folded into the cell edges).
    pub borders: Borders,
    pub shading: Option<Shading>,
    pub fill: Option<Color>,
    pub margins: CellMargins,
    pub vertical_align: Option<CellVerticalAlign>,
    pub no_wrap: Option<bool>,
    pub text_direction: Option<String>,
    pub fit_text: Option<bool>,
    /// Paragraph/character contribution of the applicable conditional zones.
    pub paragraph: ParaProps,
    pub character: CharAttrs,
}

// ---------------------------------------------------------------------------
// Notices
// ---------------------------------------------------------------------------

const MAX_TABLE_NOTICES: usize = 512;

pub(super) struct Notes {
    part: String,
    seen: BTreeSet<(String, String)>,
    pub out: Vec<ImportNotice>,
}

impl Notes {
    fn new(part: &str) -> Self {
        Self {
            part: part.to_owned(),
            seen: BTreeSet::new(),
            out: Vec::new(),
        }
    }
    fn note(&mut self, local: &str, value: impl Into<String>) {
        let value = value.into();
        if self.out.len() >= MAX_TABLE_NOTICES
            || !self.seen.insert((local.to_owned(), value.clone()))
        {
            return;
        }
        self.out.push(ImportNotice::UnsupportedProperty {
            part: self.part.clone(),
            namespace: W_NS.to_owned(),
            local_name: local.to_owned(),
            value,
        });
    }
}

/// Element names read by this module; they are not reported as unsupported.
pub(super) fn is_handled_element(local: &str) -> bool {
    matches!(
        local,
        "tbl"
            | "tblPr"
            | "tblPrEx"
            | "tblGrid"
            | "gridCol"
            | "tr"
            | "trPr"
            | "tc"
            | "tcPr"
            | "tblStyle"
            | "tblW"
            | "tblInd"
            | "tblLayout"
            | "tblCellMar"
            | "tblCellSpacing"
            | "tblBorders"
            | "tblLook"
            | "tblStyleRowBandSize"
            | "tblStyleColBandSize"
            | "tblpPr"
            | "bidiVisual"
            | "gridBefore"
            | "gridAfter"
            | "wBefore"
            | "wAfter"
            | "trHeight"
            | "tblHeader"
            | "cantSplit"
            | "hidden"
            | "tcW"
            | "gridSpan"
            | "hMerge"
            | "vMerge"
            | "tcBorders"
            | "tcMar"
            | "vAlign"
            | "noWrap"
            | "textDirection"
            | "tcFitText"
            | "hideMark"
            | "shd"
            | "top"
            | "bottom"
            | "left"
            | "right"
            | "start"
            | "end"
            | "insideH"
            | "insideV"
            | "tl2br"
            | "tr2bl"
            | "tblStylePr"
            | "default"
    )
}

// ---------------------------------------------------------------------------
// Small parsing helpers (all lenient: invalid values are ignored)
// ---------------------------------------------------------------------------

fn kids(node: &XmlNode) -> impl Iterator<Item = &XmlNode> {
    node.children
        .iter()
        .filter(|c| matches!(c.name.namespace.as_deref(), Some(W_NS | W_STRICT_NS)))
}

fn find_w<'a>(node: &'a XmlNode, local: &str) -> Option<&'a XmlNode> {
    kids(node).find(|c| c.name.local == local)
}

fn on_off(node: &XmlNode) -> Option<bool> {
    match node.w_attr("val") {
        None => Some(true),
        Some("1" | "true" | "on") => Some(true),
        Some("0" | "false" | "off") => Some(false),
        Some(_) => None,
    }
}

fn parse_int(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

/// Integer twips, or a decimal number with a unit suffix (strict documents).
fn parse_twips(value: &str) -> Option<i32> {
    let v = value.trim();
    if let Some(n) = parse_int(v) {
        return Some(n.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    }
    let (number, factor) = [
        ("mm", 56.692_913),
        ("cm", 566.929_13),
        ("in", 1440.0),
        ("pt", 20.0),
        ("pc", 240.0),
        ("pi", 240.0),
    ]
    .iter()
    .find_map(|(unit, f)| v.strip_suffix(unit).map(|n| (n, *f)))?;
    let n: f64 = number.trim().parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    Some((n * factor).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

fn parse_width(node: &XmlNode, notes: &mut Notes) -> Option<TableWidth> {
    let ty = node.w_attr("type").unwrap_or("dxa");
    let raw = node.w_attr("w");
    match ty {
        "auto" => Some(TableWidth::Auto),
        "nil" => Some(TableWidth::Nil),
        "dxa" => {
            let v = parse_twips(raw?).or_else(|| {
                notes.note(&node.name.local, format!("invalid width {raw:?}"));
                None
            })?;
            Some(TableWidth::Dxa(v))
        }
        "pct" => {
            let raw = raw?;
            let value = if let Some(p) = raw.strip_suffix('%') {
                p.trim()
                    .parse::<f64>()
                    .ok()
                    .map(|f| (f * 50.0).round() as i64)
            } else {
                parse_int(raw)
            };
            match value {
                Some(v) if v >= 0 => Some(TableWidth::Pct(v.min(u32::MAX as i64) as u32)),
                _ => {
                    notes.note(&node.name.local, format!("invalid percentage {raw:?}"));
                    None
                }
            }
        }
        other => {
            notes.note(&node.name.local, format!("unknown width type {other:?}"));
            None
        }
    }
}

fn hex_byte(value: Option<&str>) -> Option<u8> {
    u8::from_str_radix(value?.trim(), 16).ok()
}

fn parse_table_color(
    hex: Option<&str>,
    theme: Option<&str>,
    tint: Option<&str>,
    shade: Option<&str>,
) -> Option<TableColor> {
    if let Some(slot) = theme.and_then(theme_color_slot) {
        return Some(TableColor::Theme {
            slot,
            tint: hex_byte(tint),
            shade: hex_byte(shade),
        });
    }
    match hex? {
        "auto" => Some(TableColor::Auto),
        other => parse_color(Some(other)).ok().flatten().map(TableColor::Rgb),
    }
}

fn parse_border(node: &XmlNode) -> Option<Border> {
    let style = node.w_attr("val")?.to_owned();
    Some(Border {
        style,
        size: node
            .w_attr("sz")
            .and_then(parse_int)
            .map(|v| v.clamp(0, 4096) as u32),
        space: node
            .w_attr("space")
            .and_then(parse_int)
            .map(|v| v.clamp(0, 4096) as u32),
        color: parse_table_color(
            node.w_attr("color"),
            node.w_attr("themeColor"),
            node.w_attr("themeTint"),
            node.w_attr("themeShade"),
        ),
    })
}

fn parse_borders(node: &XmlNode) -> Borders {
    let mut b = Borders::default();
    for c in kids(node) {
        let border = parse_border(c);
        match c.name.local.as_str() {
            "top" => b.top = border,
            "left" | "start" => b.left = border,
            "bottom" => b.bottom = border,
            "right" | "end" => b.right = border,
            "insideH" => b.inside_h = border,
            "insideV" => b.inside_v = border,
            "tl2br" => b.diagonal_down = border,
            "tr2bl" => b.diagonal_up = border,
            _ => {}
        }
    }
    b
}

fn parse_shading(node: &XmlNode) -> Option<Shading> {
    let pattern = node.w_attr("val")?.to_owned();
    Some(Shading {
        pattern,
        color: parse_table_color(
            node.w_attr("color"),
            node.w_attr("themeColor"),
            node.w_attr("themeTint"),
            node.w_attr("themeShade"),
        ),
        fill: parse_table_color(
            node.w_attr("fill"),
            node.w_attr("themeFill"),
            node.w_attr("themeFillTint"),
            node.w_attr("themeFillShade"),
        ),
    })
}

fn parse_margins(node: &XmlNode, notes: &mut Notes) -> CellMargins {
    let mut m = CellMargins::default();
    for c in kids(node) {
        let w = parse_width(c, notes);
        match c.name.local.as_str() {
            "top" => m.top = w,
            "left" | "start" => m.left = w,
            "bottom" => m.bottom = w,
            "right" | "end" => m.right = w,
            _ => {}
        }
    }
    m
}

fn parse_align(value: Option<&str>) -> Option<TableAlign> {
    match value? {
        "center" => Some(TableAlign::Center),
        "right" | "end" => Some(TableAlign::Right),
        "left" | "start" => Some(TableAlign::Left),
        _ => None,
    }
}

fn parse_look(node: &XmlNode) -> TableLook {
    let mask = node
        .w_attr("val")
        .and_then(|v| u32::from_str_radix(v.trim(), 16).ok())
        .unwrap_or(0);
    let flag = |name: &str, bit: u32| -> bool {
        match node.w_attr(name) {
            Some("1" | "true" | "on") => true,
            Some("0" | "false" | "off") => false,
            _ => mask & bit != 0,
        }
    };
    TableLook {
        first_row: flag("firstRow", 0x0020),
        last_row: flag("lastRow", 0x0040),
        first_column: flag("firstColumn", 0x0080),
        last_column: flag("lastColumn", 0x0100),
        no_horizontal_band: flag("noHBand", 0x0200),
        no_vertical_band: flag("noVBand", 0x0400),
    }
}

fn parse_tbl_pr(node: &XmlNode, notes: &mut Notes) -> TableProps {
    let mut p = TableProps::default();
    for c in kids(node) {
        match c.name.local.as_str() {
            "tblStyle" => p.style = c.w_attr("val").map(str::to_owned),
            "tblW" => p.width = parse_width(c, notes),
            "jc" => p.align = parse_align(c.w_attr("val")),
            "tblInd" => p.indent = parse_width(c, notes),
            "tblLayout" => {
                p.layout = match c.w_attr("type") {
                    Some("fixed") => Some(TableLayout::Fixed),
                    Some("autofit") => Some(TableLayout::Autofit),
                    _ => None,
                }
            }
            "tblCellMar" => p.cell_margins = parse_margins(c, notes),
            "tblCellSpacing" => p.cell_spacing = parse_width(c, notes),
            "tblBorders" => p.borders = parse_borders(c),
            "shd" => p.shading = parse_shading(c),
            "tblLook" => p.look = Some(parse_look(c)),
            "tblStyleRowBandSize" => {
                p.row_band_size = c
                    .w_attr("val")
                    .and_then(parse_int)
                    .map(|v| v.clamp(1, 1024) as u32)
            }
            "tblStyleColBandSize" => {
                p.column_band_size = c
                    .w_attr("val")
                    .and_then(parse_int)
                    .map(|v| v.clamp(1, 1024) as u32)
            }
            "bidiVisual" => p.bidi_visual = on_off(c),
            "tblpPr" => {
                p.floating = true;
                notes.note("tblpPr", "floating table positioning is not interpreted");
            }
            _ => {}
        }
    }
    p
}

fn parse_tr_pr(node: &XmlNode, notes: &mut Notes) -> RowProps {
    let mut p = RowProps::default();
    for c in kids(node) {
        match c.name.local.as_str() {
            "trHeight" => {
                let twips = c
                    .w_attr("val")
                    .and_then(parse_int)
                    .map(|v| v.clamp(0, i32::MAX as i64) as u32);
                let rule = match c.w_attr("hRule") {
                    Some("auto") => HeightRule::Auto,
                    Some("exact") => HeightRule::Exact,
                    _ => HeightRule::AtLeast,
                };
                if let Some(twips) = twips {
                    p.height = Some(RowHeight { twips, rule });
                }
            }
            "tblHeader" => p.header = on_off(c),
            "cantSplit" => p.cant_split = on_off(c),
            "jc" => p.align = parse_align(c.w_attr("val")),
            "gridBefore" => {
                p.grid_before = c
                    .w_attr("val")
                    .and_then(parse_int)
                    .unwrap_or(0)
                    .clamp(0, 1 << 20) as u32
            }
            "gridAfter" => {
                p.grid_after = c
                    .w_attr("val")
                    .and_then(parse_int)
                    .unwrap_or(0)
                    .clamp(0, 1 << 20) as u32
            }
            "wBefore" => p.width_before = parse_width(c, notes),
            "wAfter" => p.width_after = parse_width(c, notes),
            "tblCellSpacing" => p.cell_spacing = parse_width(c, notes),
            "hidden" => p.hidden = on_off(c),
            _ => {}
        }
    }
    p
}

struct TcInfo {
    props: CellProps,
    grid_span: Option<i64>,
    v_merge: Option<VMerge>,
    /// `Some(true)` for `hMerge` restart, `Some(false)` for continue.
    h_merge: Option<bool>,
}

fn parse_tc_pr(node: &XmlNode, notes: &mut Notes) -> TcInfo {
    let mut info = TcInfo {
        props: CellProps::default(),
        grid_span: None,
        v_merge: None,
        h_merge: None,
    };
    for c in kids(node) {
        match c.name.local.as_str() {
            "tcW" => info.props.width = parse_width(c, notes),
            "gridSpan" => info.grid_span = c.w_attr("val").and_then(parse_int),
            "vMerge" => {
                info.v_merge = Some(match c.w_attr("val") {
                    Some("restart") => VMerge::Restart,
                    _ => VMerge::Continue,
                })
            }
            "hMerge" => info.h_merge = Some(c.w_attr("val") == Some("restart")),
            "tcBorders" => {
                info.props.borders = parse_borders(c);
                if info.props.borders.diagonal_down.is_some()
                    || info.props.borders.diagonal_up.is_some()
                {
                    notes.note(
                        "tcBorders",
                        "diagonal borders are kept in ImportedTables only",
                    );
                }
            }
            "shd" => info.props.shading = parse_shading(c),
            "noWrap" => info.props.no_wrap = on_off(c),
            "tcMar" => info.props.margins = parse_margins(c, notes),
            "textDirection" => {
                let v = c.w_attr("val").unwrap_or("lrTb").to_owned();
                if v != "lrTb" && v != "lrTbV" {
                    notes.note(
                        "textDirection",
                        format!("text direction {v:?} is not rendered"),
                    );
                }
                info.props.text_direction = Some(v);
            }
            "tcFitText" => {
                info.props.fit_text = on_off(c);
                if info.props.fit_text == Some(true) {
                    notes.note("tcFitText", "fit text to cell width is not rendered");
                }
            }
            "vAlign" => {
                info.props.vertical_align = match c.w_attr("val") {
                    Some("top") => Some(CellVerticalAlign::Top),
                    Some("center") => Some(CellVerticalAlign::Center),
                    Some("bottom") => Some(CellVerticalAlign::Bottom),
                    Some("both") => Some(CellVerticalAlign::Both),
                    _ => None,
                }
            }
            "hideMark" => info.props.hide_mark = on_off(c),
            _ => {}
        }
    }
    info
}

// ---------------------------------------------------------------------------
// Verbatim preservation of property elements
// ---------------------------------------------------------------------------

fn escape(out: &mut Vec<u8>, text: &str, attr: bool) {
    for ch in text.chars() {
        match ch {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '"' if attr => out.extend_from_slice(b"&quot;"),
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

fn serialize(node: &XmlNode, declared: &mut Vec<(String, String)>, out: &mut Vec<u8>) {
    let mark = declared.len();
    let mut wanted: Vec<(String, String)> = Vec::new();
    let mut want = |prefix: &Option<String>, ns: &Option<String>| {
        if let Some(ns) = ns {
            let p = prefix.clone().unwrap_or_default();
            if !declared.iter().rev().any(|(dp, dn)| *dp == p && dn == ns)
                && !wanted.iter().any(|(wp, _)| *wp == p)
            {
                wanted.push((p, ns.clone()));
            }
        }
    };
    want(&node.name.prefix, &node.name.namespace);
    for a in &node.attrs {
        if a.name.prefix.is_some() {
            want(&a.name.prefix, &a.name.namespace);
        }
    }
    out.push(b'<');
    out.extend_from_slice(node.name.qualified().as_bytes());
    for (p, ns) in &wanted {
        out.extend_from_slice(if p.is_empty() {
            b" xmlns=\""
        } else {
            b" xmlns:"
        });
        if !p.is_empty() {
            out.extend_from_slice(p.as_bytes());
            out.extend_from_slice(b"=\"");
        }
        escape(out, ns, true);
        out.push(b'"');
    }
    declared.extend(wanted);
    for a in &node.attrs {
        out.push(b' ');
        out.extend_from_slice(a.name.qualified().as_bytes());
        out.extend_from_slice(b"=\"");
        escape(out, &a.value, true);
        out.push(b'"');
    }
    if node.children.is_empty() && node.text.is_empty() {
        out.extend_from_slice(b"/>");
    } else {
        out.push(b'>');
        escape(out, &node.text, false);
        for c in &node.children {
            serialize(c, declared, out);
        }
        out.extend_from_slice(b"</");
        out.extend_from_slice(node.name.qualified().as_bytes());
        out.push(b'>');
    }
    declared.truncate(mark);
}

/// The element serialized from the parsed tree (US-306 fallback for elements
/// that have no US-032 capture).
pub(super) fn dom_bytes(node: &XmlNode) -> Vec<u8> {
    let mut out = Vec::new();
    serialize(node, &mut Vec::new(), &mut out);
    out
}

fn opaque_of(node: &XmlNode) -> OpaqueAttr {
    let mut out = Vec::new();
    serialize(node, &mut Vec::new(), &mut out);
    OpaqueAttr {
        namespace: W_NS.to_owned(),
        name: node.name.local.clone(),
        value: out,
    }
}

fn sorted(mut list: Vec<OpaqueAttr>) -> Vec<OpaqueAttr> {
    list.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    list.dedup_by(|a, b| a.namespace == b.namespace && a.name == b.name);
    list
}

// ---------------------------------------------------------------------------
// Row and cell collection
// ---------------------------------------------------------------------------

/// Rows of a table or cells of a row, looking through content wrappers only.
fn collect_named<'a>(node: &'a XmlNode, target: &str, out: &mut Vec<&'a XmlNode>) {
    for child in kids(node) {
        if child.name.local == target {
            out.push(child);
        } else if matches!(
            child.name.local.as_str(),
            "sdt" | "sdtContent" | "customXml" | "ins" | "moveTo" | "smartTag"
        ) {
            collect_named(child, target, out);
        }
    }
}

fn has_content(tc: &XmlNode) -> bool {
    let mut texts = Vec::new();
    tc.descendants(W_NS, "t", &mut texts);
    tc.descendants(W_STRICT_NS, "t", &mut texts);
    if texts.iter().any(|t| !t.text.is_empty()) {
        return true;
    }
    // Anything else a run can carry: drawings, tabs, breaks, fields...
    let mut runs = Vec::new();
    tc.descendants(W_NS, "r", &mut runs);
    tc.descendants(W_STRICT_NS, "r", &mut runs);
    let in_run = runs.iter().any(|r| {
        r.children.iter().any(|c| {
            matches!(
                c.name.local.as_str(),
                "drawing"
                    | "pict"
                    | "object"
                    | "tab"
                    | "br"
                    | "cr"
                    | "sym"
                    | "fldChar"
                    | "instrText"
                    | "noBreakHyphen"
                    | "softHyphen"
                    | "footnoteReference"
                    | "endnoteReference"
            )
        })
    });
    let mut tables = Vec::new();
    tc.descendants(W_NS, "tbl", &mut tables);
    tc.descendants(W_STRICT_NS, "tbl", &mut tables);
    in_run || !tables.is_empty()
}

// ---------------------------------------------------------------------------
// Table styles
// ---------------------------------------------------------------------------

pub(super) fn parse_table_styles(
    root: &XmlNode,
    notes: &mut Notes,
) -> (BTreeMap<String, TableStyleDef>, Option<String>) {
    let mut styles = Vec::new();
    root.descendants(W_NS, "style", &mut styles);
    if styles.is_empty() {
        root.descendants(W_STRICT_NS, "style", &mut styles);
    }
    let mut defs = BTreeMap::new();
    let mut default = None;
    for n in styles {
        if n.w_attr("type") != Some("table") {
            continue;
        }
        let Some(id) = n.w_attr("styleId") else {
            continue;
        };
        let mut def = TableStyleDef {
            id: id.to_owned(),
            based_on: find_w(n, "basedOn")
                .and_then(|x| x.w_attr("val"))
                .map(str::to_owned),
            is_default: matches!(n.w_attr("default"), Some("1" | "true" | "on")),
            ..TableStyleDef::default()
        };
        if def.is_default && default.is_none() {
            default = Some(def.id.clone());
        }
        if let Some(p) = find_w(n, "tblPr") {
            def.table = parse_tbl_pr(p, notes);
        }
        if let Some(p) = find_w(n, "trPr") {
            def.row = parse_tr_pr(p, notes);
        }
        if let Some(p) = find_w(n, "tcPr") {
            def.cell = parse_tc_pr(p, notes).props;
        }
        for pr in kids(n).filter(|c| c.name.local == "tblStylePr") {
            let Some(kind) = pr.w_attr("type").and_then(ConditionalKind::parse) else {
                notes.note(
                    "tblStylePr",
                    format!("unknown conditional type {:?}", pr.w_attr("type")),
                );
                continue;
            };
            let mut cf = ConditionalFormat::default();
            if let Some(p) = find_w(pr, "pPr") {
                match parse_para_props(p) {
                    Ok(v) => cf.paragraph = v,
                    Err(_) => notes.note("tblStylePr", "invalid paragraph properties ignored"),
                }
            }
            if let Some(p) = find_w(pr, "rPr") {
                match parse_style_char_attrs(p) {
                    Ok(v) => cf.character = v,
                    Err(_) => notes.note("tblStylePr", "invalid character properties ignored"),
                }
            }
            if let Some(p) = find_w(pr, "tblPr") {
                cf.table = parse_tbl_pr(p, notes);
            }
            if let Some(p) = find_w(pr, "trPr") {
                cf.row = parse_tr_pr(p, notes);
            }
            if let Some(p) = find_w(pr, "tcPr") {
                cf.cell = parse_tc_pr(p, notes).props;
            }
            def.conditionals.insert(kind, cf);
        }
        defs.insert(def.id.clone(), def);
    }
    (defs, default)
}

// ---------------------------------------------------------------------------
// Building tables
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Merge {
    None,
    Restart,
    Continue,
}

struct Item<'a> {
    tc: Option<&'a XmlNode>,
    /// Further `hMerge` continuation cells folded into this one.
    extra: Vec<&'a XmlNode>,
    col: usize,
    span: usize,
    merge: Merge,
    props: CellProps,
    opaque: Option<OpaqueAttr>,
    /// Anchor `(row, item)` when this vertical-merge continuation was folded.
    merged_into: Option<(usize, usize)>,
    row_span: usize,
}

struct RowPlan<'a> {
    tr: &'a XmlNode,
    props: RowProps,
    opaque: Vec<OpaqueAttr>,
    items: Vec<Item<'a>>,
}

struct CellBuild {
    id: NodeId,
    blocks: Vec<NodeId>,
    has_paragraph: bool,
    tree: TreeBuilder,
}

fn key(node: &XmlNode) -> usize {
    node as *const XmlNode as usize
}

pub(super) struct TableBuilder<'a> {
    limits: &'a DocxLimits,
    catalog: &'a StyleCatalog,
    theme: &'a Theme,
    pub notes: Notes,
    pub out: ImportedTables,
    cells_total: usize,
}

impl<'a> TableBuilder<'a> {
    pub fn new(
        limits: &'a DocxLimits,
        catalog: &'a StyleCatalog,
        theme: &'a Theme,
        main_part: &str,
    ) -> Self {
        Self {
            limits,
            catalog,
            theme,
            notes: Notes::new(main_part),
            out: ImportedTables::default(),
            cells_total: 0,
        }
    }

    /// Reads the table styles of `styles.xml` (conditional formatting, ...).
    pub fn read_styles(&mut self, root: &XmlNode, styles_part: &str) {
        let mut notes = Notes::new(styles_part);
        let (defs, default) = parse_table_styles(root, &mut notes);
        self.out.styles = defs;
        self.out.default_style = default;
        self.notes.out.extend(notes.out);
    }

    /// A table style usable by the style engine: every style of the
    /// `basedOn` chain exists as a table style and its theme references
    /// resolve.
    fn style_is_usable(&self, id: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut next = Some(id);
        while let Some(current) = next {
            if !seen.insert(current.to_owned()) {
                return false;
            }
            let Some(style) = self.catalog.style(current) else {
                return false;
            };
            if style.kind != atelier_text_model::rich::StyleKind::Table {
                return false;
            }
            let c = &style.character;
            if (c.values.font.is_none()
                && c.theme_font
                    .is_some_and(|s| self.theme.fonts.get(&s).is_none_or(|f| f.is_empty())))
                || (c.values.color.is_none()
                    && c.theme_color
                        .is_some_and(|s| !self.theme.colors.contains_key(&s)))
                || (c.values.highlight.is_none()
                    && c.theme_highlight
                        .is_some_and(|s| !self.theme.colors.contains_key(&s)))
            {
                return false;
            }
            next = style.based_on.as_deref();
        }
        true
    }

    pub fn finish(self) -> (ImportedTables, Vec<ImportNotice>) {
        (self.out, self.notes.out)
    }

    /// Builds the detached branch for a body-level `w:tbl`. `None` when the
    /// table has no row at all.
    pub fn build_table(
        &mut self,
        document: &mut RichDocument,
        sink: &mut ParagraphSink<'_>,
        tbl: &XmlNode,
    ) -> Result<Option<Subtree>, DocxError> {
        let mut nodes = BTreeMap::new();
        let root = self.build(document, sink, tbl, 0, &mut nodes)?;
        Ok(root.map(|root| Subtree { root, nodes }))
    }

    fn count_cell(&mut self) -> Result<(), DocxError> {
        self.cells_total += 1;
        if self.cells_total > self.limits.max_table_cells {
            return Err(DocxError::Limit("table cells"));
        }
        Ok(())
    }

    fn build<'x>(
        &mut self,
        document: &mut RichDocument,
        sink: &mut ParagraphSink<'_>,
        tbl: &'x XmlNode,
        depth: usize,
        nodes: &mut BTreeMap<NodeId, Node>,
    ) -> Result<Option<NodeId>, DocxError> {
        let max_cols = self.limits.max_table_columns.max(1);
        let mut props = TableProps::default();
        let mut table_opaque = Vec::new();
        if let Some(p) = find_w(tbl, "tblPr") {
            props = parse_tbl_pr(p, &mut self.notes);
            table_opaque.push(opaque_of(p));
        }
        let mut grid: Vec<Option<u32>> = Vec::new();
        if let Some(g) = find_w(tbl, "tblGrid") {
            table_opaque.push(opaque_of(g));
            for c in kids(g).filter(|c| c.name.local == "gridCol") {
                if grid.len() >= max_cols {
                    self.notes
                        .note("tblGrid", "grid columns beyond the limit were dropped");
                    break;
                }
                let w = c.w_attr("w").and_then(parse_twips);
                grid.push(match w {
                    Some(v) if v >= 0 => Some(v as u32),
                    Some(_) => {
                        self.notes.note("gridCol", "negative column width ignored");
                        None
                    }
                    None => None,
                });
            }
        }

        // Rows.
        let mut row_nodes = Vec::new();
        collect_named(tbl, "tr", &mut row_nodes);
        if row_nodes.len() > self.limits.max_table_rows {
            return Err(DocxError::Limit("table rows"));
        }
        let mut plans: Vec<RowPlan<'x>> = Vec::new();
        for tr in row_nodes {
            let mut row_props = RowProps::default();
            let mut row_opaque = Vec::new();
            if let Some(ex) = find_w(tr, "tblPrEx") {
                let mut e = parse_tbl_pr(ex, &mut self.notes);
                e.style = None;
                row_props.table_exceptions = Some(e);
                row_opaque.push(opaque_of(ex));
            }
            if let Some(p) = find_w(tr, "trPr") {
                if find_w(p, "del").is_some() {
                    self.notes
                        .note("trPr", "row deleted by a tracked change was dropped");
                    continue;
                }
                let parsed = parse_tr_pr(p, &mut self.notes);
                let ex = row_props.table_exceptions.take();
                row_props = parsed;
                row_props.table_exceptions = ex;
                row_opaque.push(opaque_of(p));
            }
            let plan = self.plan_row(tr, row_props, sorted(row_opaque), max_cols)?;
            plans.push(plan);
        }
        if plans.is_empty() {
            self.notes.note("tbl", "table without rows was dropped");
            return Ok(None);
        }

        // Columns.
        let widest = plans
            .iter()
            .map(|p| {
                p.items.iter().map(|i| i.col + i.span).max().unwrap_or(0)
                    + p.props.grid_after as usize
            })
            .max()
            .unwrap_or(0);
        let mut columns = if widest == 0 {
            grid.len().max(1)
        } else {
            widest
        };
        if columns > max_cols {
            self.notes
                .note("tbl", "table wider than the column limit was clipped");
            columns = max_cols;
        }
        if !grid.is_empty() && grid.len() != columns {
            self.notes.note(
                "tblGrid",
                format!("grid has {} columns but rows use {columns}", grid.len()),
            );
        }
        grid.resize(columns, None);
        for plan in &mut plans {
            // Clip items to the grid, then pad short rows.
            plan.items.retain(|i| i.col < columns);
            for i in &mut plan.items {
                i.span = i.span.min(columns - i.col);
            }
            let end = plan.items.iter().map(|i| i.col + i.span).max().unwrap_or(0);
            if end < columns {
                if plan.props.grid_after == 0 && end > 0 {
                    self.notes.note("tr", "short row padded with an empty cell");
                }
                plan.items.push(Item {
                    tc: None,
                    extra: Vec::new(),
                    col: end,
                    span: columns - end,
                    merge: Merge::None,
                    props: CellProps::default(),
                    opaque: None,
                    merged_into: None,
                    row_span: 1,
                });
            }
        }

        // Vertical merges.
        let mut open: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
        for r in 0..plans.len() {
            for k in 0..plans[r].items.len() {
                let (col, span, merge, real) = {
                    let i = &plans[r].items[k];
                    (i.col, i.span, i.merge, i.tc.is_some())
                };
                match merge {
                    Merge::Continue if real => {
                        let anchor = open
                            .get(&(col, span))
                            .copied()
                            .filter(|(ar, ak)| plans[*ar].items[*ak].row_span + *ar == r);
                        if let Some((ar, ak)) = anchor {
                            plans[ar].items[ak].row_span += 1;
                            plans[r].items[k].merged_into = Some((ar, ak));
                        } else {
                            self.notes.note(
                                "vMerge",
                                "continuation without a matching start was kept as a plain cell",
                            );
                            plans[r].items[k].merge = Merge::Restart;
                            open.insert((col, span), (r, k));
                        }
                    }
                    Merge::Restart => {
                        open.insert((col, span), (r, k));
                    }
                    _ => {}
                }
            }
        }

        // Nodes.
        let table_id = document.alloc_id().map_err(model_error)?;
        let mut row_ids = Vec::new();
        let mut cell_builds: BTreeMap<(usize, usize), CellBuild> = BTreeMap::new();
        let mut row_cells: Vec<Vec<(usize, NodeId)>> = Vec::new();
        let mut folded: BTreeMap<usize, PreservedElement> = BTreeMap::new();
        let mut cell_elems: BTreeMap<usize, PreservedElement> = BTreeMap::new();
        for (r, plan) in plans.iter().enumerate() {
            let row_id = document.alloc_id().map_err(model_error)?;
            row_ids.push(row_id);
            let mut list = Vec::new();
            for (k, item) in plan.items.iter().enumerate() {
                if let Some((ar, ak)) = item.merged_into {
                    // Folded continuation: keep any real content.
                    if let Some(tc) = item.tc {
                        let mut local = TreeBuilder::new(sink.pres.shell(tc)?);
                        if item.tc.is_some_and(has_content) {
                            self.notes.note(
                                "vMerge",
                                "content of a merged continuation cell was moved to the first cell",
                            );
                            let (cell_id, mut blocks, mut has_p) = {
                                let b = cell_builds.get(&(ar, ak)).expect("anchor built first");
                                (b.id, Vec::new(), b.has_paragraph)
                            };
                            self.fill_blocks(
                                document,
                                sink,
                                tc,
                                depth,
                                cell_id,
                                nodes,
                                (&mut blocks, &mut has_p),
                                (&mut local, false),
                            )?;
                            let b = cell_builds.get_mut(&(ar, ak)).expect("anchor built first");
                            b.blocks.extend(blocks);
                            b.has_paragraph = has_p;
                        } else if let Some(pr) = find_w(tc, "tcPr") {
                            local.push(PreservedChild::Opaque(dom_bytes(pr)));
                        }
                        folded.insert(key(tc), local.finish());
                    }
                    continue;
                }
                self.count_cell()?;
                let cell_id = document.alloc_id().map_err(model_error)?;
                let root = match item.tc {
                    Some(tc) => sink.pres.shell(tc)?,
                    None => PreservedElement::default(),
                };
                let mut build = CellBuild {
                    id: cell_id,
                    blocks: Vec::new(),
                    has_paragraph: false,
                    tree: TreeBuilder::new(root),
                };
                if let Some(tc) = item.tc {
                    self.fill_blocks(
                        document,
                        sink,
                        tc,
                        depth,
                        cell_id,
                        nodes,
                        (&mut build.blocks, &mut build.has_paragraph),
                        (&mut build.tree, true),
                    )?;
                    for extra in &item.extra {
                        let mut local = TreeBuilder::new(sink.pres.shell(extra)?);
                        if has_content(extra) {
                            self.fill_blocks(
                                document,
                                sink,
                                extra,
                                depth,
                                cell_id,
                                nodes,
                                (&mut build.blocks, &mut build.has_paragraph),
                                (&mut local, false),
                            )?;
                        } else if let Some(pr) = find_w(extra, "tcPr") {
                            local.push(PreservedChild::Opaque(dom_bytes(pr)));
                        }
                        folded.insert(key(extra), local.finish());
                    }
                }
                list.push((k, cell_id));
                cell_builds.insert((r, k), build);
            }
            row_cells.push(list);
        }

        // Finalise cells, rows, table.
        for (r, plan) in plans.iter().enumerate() {
            let mut ids = Vec::new();
            for (k, cell_id) in &row_cells[r] {
                let item = &plan.items[*k];
                let mut build = cell_builds.remove(&(r, *k)).expect("cell built");
                if !build.has_paragraph {
                    let pid = document.alloc_id().map_err(model_error)?;
                    nodes.insert(pid, Node::Paragraph(Paragraph::default()));
                    self.out.paragraph_cell.insert(pid, *cell_id);
                    build.blocks.push(pid);
                }
                let tree = std::mem::replace(&mut build.tree, TreeBuilder::new(Default::default()));
                if let Some(tc) = item.tc {
                    let mut element = tree.finish();
                    element.node = Some(*cell_id);
                    cell_elems.insert(key(tc), element);
                }
                let blocks = BlockList::from_ids(&build.blocks).map_err(model_error)?;
                nodes.insert(
                    *cell_id,
                    Node::Cell(Cell {
                        col_span: item.span,
                        row_span: item.row_span,
                        blocks,
                        opaque: item.opaque.iter().cloned().collect(),
                    }),
                );
                let mut cell_props = item.props.clone();
                if matches!(cell_props.width, Some(TableWidth::Dxa(v)) if v < 0) {
                    self.notes.note("tcW", "negative cell width ignored");
                    cell_props.width = None;
                }
                self.out.cells.insert(
                    *cell_id,
                    ImportedCell {
                        props: cell_props,
                        table: Some(table_id),
                        row: Some(row_ids[r]),
                        row_index: r,
                        column_index: item.col,
                        row_span: item.row_span,
                        col_span: item.span,
                        synthetic: item.tc.is_none(),
                    },
                );
                ids.push(*cell_id);
            }
            nodes.insert(
                row_ids[r],
                Node::Row(Row {
                    cells: ids,
                    opaque: plan.opaque.clone(),
                }),
            );
            self.out.rows.insert(
                row_ids[r],
                ImportedRow {
                    props: plan.props.clone(),
                    table: Some(table_id),
                    index: r,
                },
            );
        }
        table_tree(
            &mut sink.pres,
            tbl,
            table_id,
            &plans,
            &row_ids,
            &mut cell_elems,
            &mut folded,
        )?;
        let mut style = None;
        if let Some(id) = props.style.clone() {
            if self.style_is_usable(&id) {
                style = Some(id);
            } else {
                self.notes.note(
                    "tblStyle",
                    format!("table style {id:?} is missing or unusable; not applied"),
                );
            }
        }
        nodes.insert(
            table_id,
            Node::Table(Table {
                columns,
                rows: row_ids.clone(),
                style,
                opaque: sorted(table_opaque),
            }),
        );
        self.out.tables.insert(
            table_id,
            ImportedTable {
                props,
                grid,
                columns,
                rows: row_ids,
                depth,
            },
        );
        Ok(Some(table_id))
    }

    fn plan_row<'x>(
        &mut self,
        tr: &'x XmlNode,
        props: RowProps,
        opaque: Vec<OpaqueAttr>,
        max_cols: usize,
    ) -> Result<RowPlan<'x>, DocxError> {
        let mut items: Vec<Item<'x>> = Vec::new();
        let mut cursor = (props.grid_before as usize).min(max_cols);
        if cursor > 0 {
            items.push(Item {
                tc: None,
                extra: Vec::new(),
                col: 0,
                span: cursor,
                merge: Merge::None,
                props: CellProps::default(),
                opaque: None,
                merged_into: None,
                row_span: 1,
            });
        }
        let mut cells = Vec::new();
        collect_named(tr, "tc", &mut cells);
        for tc in cells {
            let (info, tc_opaque) = match find_w(tc, "tcPr") {
                Some(p) => (parse_tc_pr(p, &mut self.notes), Some(opaque_of(p))),
                None => (
                    TcInfo {
                        props: CellProps::default(),
                        grid_span: None,
                        v_merge: None,
                        h_merge: None,
                    },
                    None,
                ),
            };
            let span = match info.grid_span {
                None => 1,
                Some(v) if v >= 1 => (v as u64).min(max_cols as u64) as usize,
                Some(_) => {
                    self.notes.note("gridSpan", "span below 1 treated as 1");
                    1
                }
            };
            if info.grid_span.is_some_and(|v| v as u64 > max_cols as u64) {
                self.notes
                    .note("gridSpan", "span above the column limit was clamped");
            }
            if info.h_merge == Some(false) {
                if let Some(prev) = items.iter_mut().rev().find(|i| i.tc.is_some()) {
                    prev.span += span;
                    prev.extra.push(tc);
                    cursor += span;
                    self.notes.note(
                        "hMerge",
                        "legacy horizontal merge converted to a column span",
                    );
                    continue;
                }
                self.notes.note(
                    "hMerge",
                    "continuation without a start was kept as a plain cell",
                );
            }
            self.count_cell()?;
            let merge = match info.v_merge {
                Some(VMerge::Restart) => Merge::Restart,
                Some(VMerge::Continue) => Merge::Continue,
                None => Merge::None,
            };
            items.push(Item {
                tc: Some(tc),
                extra: Vec::new(),
                col: cursor,
                span,
                merge,
                props: info.props,
                opaque: tc_opaque,
                merged_into: None,
                row_span: 1,
            });
            cursor += span;
        }
        Ok(RowPlan {
            tr,
            props,
            opaque,
            items,
        })
    }

    /// Reads the blocks of a cell container. `(blocks, has_paragraph)` collect
    /// the model blocks; `(tree, slot)` receives the source skeleton, `slot`
    /// telling whether `w:tcPr` is held by the cell's `opaque` slot.
    #[allow(clippy::too_many_arguments)]
    fn fill_blocks(
        &mut self,
        document: &mut RichDocument,
        sink: &mut ParagraphSink<'_>,
        container: &XmlNode,
        depth: usize,
        cell_id: NodeId,
        nodes: &mut BTreeMap<NodeId, Node>,
        (blocks, has_paragraph): (&mut Vec<NodeId>, &mut bool),
        (tree, slot): (&mut TreeBuilder, bool),
    ) -> Result<(), DocxError> {
        let events = preserve::events(container, Level::Cell, &sink.pres.decls);
        for event in events {
            match event {
                Ev::Known(n) if is_w(n, "tcPr") => {
                    if slot {
                        let mut marker = sink.pres.shell(n)?;
                        marker.slot = true;
                        tree.push(PreservedChild::Element(marker));
                    } else {
                        tree.push(PreservedChild::Opaque(dom_bytes(n)));
                    }
                }
                Ev::Known(p) if is_w(p, "p") => {
                    self.add_paragraph(document, sink, p, cell_id, nodes, blocks, Some(tree))?;
                    *has_paragraph = true;
                }
                Ev::Known(t) => {
                    if depth + 1 < self.limits.max_table_depth {
                        if let Some(root) = self.build(document, sink, t, depth + 1, nodes)? {
                            blocks.push(root);
                            tree.push(PreservedChild::Block(root));
                        }
                    } else {
                        self.notes.note(
                            "tbl",
                            "table nested beyond the depth limit was flattened to paragraphs",
                        );
                        tree.push(PreservedChild::Opaque(dom_bytes(t)));
                        let mut flat = Vec::new();
                        collect_paragraphs(t, &mut flat);
                        for p in flat {
                            self.add_paragraph(document, sink, p, cell_id, nodes, blocks, None)?;
                            *has_paragraph = true;
                        }
                    }
                }
                Ev::Open(n, shadow) => {
                    let mut element = sink.pres.shell(n)?;
                    element.shadow = shadow;
                    tree.open(element);
                }
                Ev::Close => tree.close(),
                Ev::Opaque(n) => {
                    let id = document.alloc_id().map_err(model_error)?;
                    nodes.insert(
                        id,
                        Node::Opaque(OpaqueBlock {
                            kind: n.name.qualified(),
                            data: preserve::xml_of(n),
                        }),
                    );
                    blocks.push(id);
                    tree.push(PreservedChild::Block(id));
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_paragraph(
        &mut self,
        document: &mut RichDocument,
        sink: &mut ParagraphSink<'_>,
        p: &XmlNode,
        cell_id: NodeId,
        nodes: &mut BTreeMap<NodeId, Node>,
        blocks: &mut Vec<NodeId>,
        tree: Option<&mut TreeBuilder>,
    ) -> Result<(), DocxError> {
        let (paragraph, meta) = sink.read(document, p)?;
        let id = document.alloc_id().map_err(model_error)?;
        nodes.insert(id, Node::Paragraph(paragraph));
        sink.record(id, meta);
        self.out.paragraph_cell.insert(id, cell_id);
        blocks.push(id);
        if let Some(tree) = tree {
            tree.push(PreservedChild::Block(id));
        }
        Ok(())
    }
}

/// Records the source skeleton of a table: `w:tbl`, its rows and cells, with
/// the wrappers, unknown children and attributes the model does not hold.
fn table_tree(
    pres: &mut preserve::Pres,
    tbl: &XmlNode,
    table_id: NodeId,
    plans: &[RowPlan<'_>],
    row_ids: &[NodeId],
    cells: &mut BTreeMap<usize, PreservedElement>,
    folded: &mut BTreeMap<usize, PreservedElement>,
) -> Result<(), DocxError> {
    let mut root = pres.shell(tbl)?;
    root.node = Some(table_id);
    let mut table = TreeBuilder::new(root);
    for event in preserve::events(tbl, Level::Table, &pres.decls) {
        match event {
            Ev::Known(n) if n.name.local == "tr" => {
                let Some(r) = plans.iter().position(|p| std::ptr::eq(p.tr, n)) else {
                    table.push(PreservedChild::Opaque(dom_bytes(n)));
                    continue;
                };
                let mut root = pres.shell(n)?;
                root.node = Some(row_ids[r]);
                let mut row = TreeBuilder::new(root);
                for ev in preserve::events(n, Level::Row, &pres.decls) {
                    match ev {
                        Ev::Known(c) if c.name.local == "tc" => {
                            if let Some(cell) = cells.remove(&key(c)) {
                                let id = cell.node.expect("cell element has an id");
                                pres.out.tables.insert(id, cell);
                                row.push(PreservedChild::Block(id));
                            } else if let Some(f) = folded.remove(&key(c)) {
                                row.push(PreservedChild::Element(f));
                            } else {
                                row.push(PreservedChild::Opaque(dom_bytes(c)));
                            }
                        }
                        Ev::Known(c) => {
                            let mut marker = pres.shell(c)?;
                            marker.slot = true;
                            row.push(PreservedChild::Element(marker));
                        }
                        Ev::Open(c, shadow) => {
                            let mut element = pres.shell(c)?;
                            element.shadow = shadow;
                            row.open(element);
                        }
                        Ev::Close => row.close(),
                        Ev::Opaque(c) => row.push(PreservedChild::Opaque(preserve::xml_of(c))),
                    }
                }
                let element = row.finish();
                pres.out.tables.insert(row_ids[r], element);
                table.push(PreservedChild::Block(row_ids[r]));
            }
            Ev::Known(n) => {
                let mut marker = pres.shell(n)?;
                marker.slot = true;
                table.push(PreservedChild::Element(marker));
            }
            Ev::Open(n, shadow) => {
                let mut element = pres.shell(n)?;
                element.shadow = shadow;
                table.open(element);
            }
            Ev::Close => table.close(),
            Ev::Opaque(n) => table.push(PreservedChild::Opaque(preserve::xml_of(n))),
        }
    }
    pres.out.tables.insert(table_id, table.finish());
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

fn style_char(s: &StyleCharAttrs, theme: &Theme) -> CharAttrs {
    let mut a = s.values.clone();
    if a.font.is_none() {
        if let Some(f) = s.theme_font.and_then(|slot| theme.fonts.get(&slot)) {
            if !f.is_empty() {
                a.font = Some(f.clone());
            }
        }
    }
    if a.color.is_none() {
        a.color = s
            .theme_color
            .and_then(|slot| theme.colors.get(&slot).copied());
    }
    if a.highlight.is_none() {
        a.highlight = s
            .theme_highlight
            .and_then(|slot| theme.colors.get(&slot).copied());
    }
    a
}

/// Paragraph and character properties set along a style's `basedOn` chain
/// (base first), ignoring toggle semantics and defaults.
pub(super) fn chain_formatting(
    catalog: &StyleCatalog,
    id: Option<&str>,
    theme: &Theme,
) -> (ParaProps, CharAttrs) {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut next = id;
    while let Some(current) = next {
        if !seen.insert(current.to_owned()) {
            break;
        }
        let Some(style) = catalog.style(current) else {
            break;
        };
        chain.push(style);
        next = style.based_on.as_deref();
    }
    let mut para = ParaProps::default();
    let mut ch = CharAttrs::default();
    for style in chain.into_iter().rev() {
        para.overlay(&style.paragraph);
        ch.overlay(&style_char(&style.character, theme));
    }
    para.style = None;
    (para, ch)
}

fn cell_edges(
    tbl: &Borders,
    tc: &Borders,
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
) -> Borders {
    let mut out = Borders {
        top: if top {
            tbl.top.clone()
        } else {
            tbl.inside_h.clone()
        },
        bottom: if bottom {
            tbl.bottom.clone()
        } else {
            tbl.inside_h.clone()
        },
        left: if left {
            tbl.left.clone()
        } else {
            tbl.inside_v.clone()
        },
        right: if right {
            tbl.right.clone()
        } else {
            tbl.inside_v.clone()
        },
        ..Borders::default()
    };
    out.overlay(&Borders {
        top: tc.top.clone(),
        left: tc.left.clone(),
        bottom: tc.bottom.clone(),
        right: tc.right.clone(),
        diagonal_down: tc.diagonal_down.clone(),
        diagonal_up: tc.diagonal_up.clone(),
        ..Borders::default()
    });
    out
}

impl ImportedTables {
    fn style_chain(&self, id: Option<&str>) -> Vec<&TableStyleDef> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = id.or(self.default_style.as_deref());
        while let Some(current) = next {
            if !seen.insert(current) {
                break;
            }
            let Some(style) = self.styles.get(current) else {
                break;
            };
            chain.push(style);
            next = style.based_on.as_deref();
        }
        chain.reverse();
        chain
    }

    /// Effective table-level properties (style chain, then direct).
    pub fn resolve_table(&self, table: NodeId) -> Option<TableProps> {
        let t = self.tables.get(&table)?;
        let mut out = TableProps::default();
        for s in self.style_chain(t.props.style.as_deref()) {
            out.overlay(&s.table);
        }
        out.overlay(&t.props);
        Some(out)
    }

    /// Effective properties of `cell` (see the module documentation for the
    /// order: style, conditional zones in [`ConditionalKind::APPLY_ORDER`],
    /// direct table, row exceptions, direct cell).
    pub fn resolve_cell(&self, cell: NodeId, theme: &Theme) -> Option<ResolvedCell> {
        let ic = self.cells.get(&cell)?;
        let table_id = ic.table?;
        let it = self.tables.get(&table_id)?;
        let ir = ic.row.and_then(|r| self.rows.get(&r));
        let chain = self.style_chain(it.props.style.as_deref());

        let rows = it.rows.len().max(1);
        let cols = it.columns.max(1);
        let (r0, r1) = (ic.row_index, ic.row_index + ic.row_span.max(1) - 1);
        let (c0, c1) = (ic.column_index, ic.column_index + ic.col_span.max(1) - 1);
        let look = it
            .props
            .look
            .or_else(|| chain.iter().rev().find_map(|s| s.table.look))
            .unwrap_or_default();
        let band = |sel: fn(&TableProps) -> Option<u32>| -> usize {
            sel(&it.props)
                .or_else(|| chain.iter().rev().find_map(|s| sel(&s.table)))
                .unwrap_or(1)
                .max(1) as usize
        };
        let row_band = band(|t| t.row_band_size);
        let col_band = band(|t| t.column_band_size);

        let single_row = r0 == 0 && r1 == rows - 1;
        let single_col = c0 == 0 && c1 == cols - 1;
        let first_row = look.first_row && r0 == 0;
        let last_row = look.last_row && r1 == rows - 1 && !(single_row && first_row);
        let first_col = look.first_column && c0 == 0;
        let last_col = look.last_column && c1 == cols - 1 && !(single_col && first_col);

        let mut applicable = vec![ConditionalKind::WholeTable];
        if !look.no_vertical_band && !first_col && !last_col {
            let idx = c0.saturating_sub(usize::from(look.first_column)) / col_band;
            applicable.push(if idx % 2 == 0 {
                ConditionalKind::Band1Vert
            } else {
                ConditionalKind::Band2Vert
            });
        }
        if !look.no_horizontal_band && !first_row && !last_row {
            let idx = r0.saturating_sub(usize::from(look.first_row)) / row_band;
            applicable.push(if idx % 2 == 0 {
                ConditionalKind::Band1Horz
            } else {
                ConditionalKind::Band2Horz
            });
        }
        if first_col {
            applicable.push(ConditionalKind::FirstCol);
        }
        if last_col {
            applicable.push(ConditionalKind::LastCol);
        }
        if first_row {
            applicable.push(ConditionalKind::FirstRow);
        }
        if last_row {
            applicable.push(ConditionalKind::LastRow);
        }
        if first_row && first_col {
            applicable.push(ConditionalKind::NwCell);
        }
        if first_row && last_col {
            applicable.push(ConditionalKind::NeCell);
        }
        if last_row && first_col {
            applicable.push(ConditionalKind::SwCell);
        }
        if last_row && last_col {
            applicable.push(ConditionalKind::SeCell);
        }
        let ordered: Vec<ConditionalKind> = ConditionalKind::APPLY_ORDER
            .iter()
            .copied()
            .filter(|k| applicable.contains(k))
            .collect();

        let (top, bottom, left, right) = (r0 == 0, r1 == rows - 1, c0 == 0, c1 == cols - 1);
        let mut res = ResolvedCell {
            table: Some(table_id),
            row: ic.row,
            row_index: ic.row_index,
            column_index: ic.column_index,
            row_span: ic.row_span,
            col_span: ic.col_span,
            ..ResolvedCell::default()
        };
        let mut row_props = RowProps::default();
        let mut table_props = TableProps::default();

        let layer = |res: &mut ResolvedCell, tbl: &TableProps, tc: &CellProps| {
            res.borders.overlay(&cell_edges(
                &tbl.borders,
                &tc.borders,
                top,
                bottom,
                left,
                right,
            ));
            if tbl.shading.is_some() {
                res.shading = tbl.shading.clone();
            }
            res.margins.overlay(&tbl.cell_margins);
            if tc.shading.is_some() {
                res.shading = tc.shading.clone();
            }
            res.margins.overlay(&tc.margins);
            macro_rules! take {
                ($($f:ident),*) => {$( if tc.$f.is_some() { res.$f = tc.$f.clone(); } )*};
            }
            take!(width, vertical_align, no_wrap, text_direction, fit_text);
        };

        for kind in &ordered {
            if *kind == ConditionalKind::WholeTable {
                for s in &chain {
                    layer(&mut res, &s.table, &s.cell);
                    row_props.overlay(&s.row);
                    table_props.overlay(&s.table);
                }
            }
            for s in &chain {
                if let Some(c) = s.conditionals.get(kind) {
                    layer(&mut res, &c.table, &c.cell);
                    row_props.overlay(&c.row);
                    res.paragraph.overlay(&c.paragraph);
                    res.character.overlay(&style_char(&c.character, theme));
                }
            }
        }
        layer(&mut res, &it.props, &CellProps::default());
        table_props.overlay(&it.props);
        if let Some(ir) = ir {
            if let Some(ex) = &ir.props.table_exceptions {
                layer(&mut res, ex, &CellProps::default());
            }
            row_props.overlay(&ir.props);
        }
        layer(&mut res, &TableProps::default(), &ic.props);
        res.conditionals = ordered;
        res.table_props = table_props;
        res.row_props = row_props;
        res.borders.inside_h = None;
        res.borders.inside_v = None;
        res.fill = res.shading.as_ref().and_then(|s| s.effective_fill(theme));
        Some(res)
    }
}
