// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tables ([MS-DOC] 2.6.3 table sprms, `TDefTable`, `TC80`, `Brc`, `Shd`): the
//! properties of a row (`TAP`) collected from the sprms of its end-of-row
//! paragraph, the companion structures that hold what the rich model cannot
//! carry (same conventions as `ImportedDocx::tables`) and the construction of
//! the model table with an exactly rectangular grid.

use super::assemble::{Repairs, remove_subtree};
use super::sprm::{Prl, Stats, ico_color};
use super::{DocError, DocLimits};
use atelier_text_model::rich::{
    BlockList, Cell, Color, Node, NodeId, Paragraph, RichDocument, Row, Table,
};
use std::collections::BTreeMap;

// Table sprms ([MS-DOC] 2.6.3). Observed in LibreOffice files unless noted.
const SPRM_T_JC90: u16 = 0x5400;
const SPRM_T_JC: u16 = 0x548A;
const SPRM_T_DXA_LEFT: u16 = 0x9601;
const SPRM_T_DXA_GAP_HALF: u16 = 0x9602;
const SPRM_T_CANT_SPLIT: u16 = 0x3403;
const SPRM_T_CANT_SPLIT90: u16 = 0x3466;
const SPRM_T_HEADER: u16 = 0x3404;
const SPRM_T_BORDERS80: u16 = 0xD605;
const SPRM_T_DY_ROW_HEIGHT: u16 = 0x9407;
const SPRM_T_DEF_TABLE: u16 = 0xD608;
const SPRM_T_DEF_SHD80: u16 = 0xD609;
const SPRM_T_BORDERS: u16 = 0xD613;
const SPRM_T_DEF_SHD: u16 = 0xD612;
const SPRM_T_DEF_SHD2: u16 = 0xD616;
const SPRM_T_DEF_SHD3: u16 = 0xD617;
const SPRM_T_SET_BRC80: u16 = 0xD620;
const SPRM_T_SET_BRC: u16 = 0xD62F;
const SPRM_T_TABLE_WIDTH: u16 = 0xF614;
const SPRM_T_CELL_PADDING: u16 = 0xD632;
const SPRM_T_CELL_PADDING_DEFAULT: u16 = 0xD634;
/// Shading or flow sprms written by LibreOffice that carry nothing the model
/// could use (the same shading is also in `TDefTableShd`).
const T_SILENT: [u16; 3] = [0xD670, 0xD609, 0x3465];

// ----- companion structures ---------------------------------------------------

/// Width in the units of `ftsWidth` converted to the DOCX vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableWidth {
    Auto,
    Nil,
    /// Twips.
    Dxa(i32),
    /// Fiftieths of a percent (5000 = 100 %), as stored in the file.
    Pct(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellVerticalAlign {
    Top,
    Center,
    Bottom,
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
pub enum TableColor {
    Auto,
    Rgb(Color),
}

/// A border. `style` uses the OOXML names (`single`, `double`, `dotted`,
/// `dashed`, ...), `size` is in eighths of a point, `space` in points.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Border {
    pub style: String,
    pub size: Option<u32>,
    pub space: Option<u32>,
    pub color: Option<TableColor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

impl Borders {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Shading: `pattern` is `clear`, `solid`, `pctNN` or an OOXML pattern name;
/// `color` is the foreground, `fill` the background.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shading {
    pub pattern: String,
    pub color: Option<TableColor>,
    pub fill: Option<TableColor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CellMargins {
    pub top: Option<TableWidth>,
    pub left: Option<TableWidth>,
    pub bottom: Option<TableWidth>,
    pub right: Option<TableWidth>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TableProps {
    pub width: Option<TableWidth>,
    pub align: Option<TableAlign>,
    pub indent: Option<TableWidth>,
    pub cell_margins: CellMargins,
    pub borders: Borders,
    /// Half of the space between cells (`sprmTDxaGapHalf`), twips.
    pub gap_half: Option<i32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RowProps {
    pub height: Option<RowHeight>,
    pub header: Option<bool>,
    pub cant_split: Option<bool>,
    pub align: Option<TableAlign>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CellProps {
    pub width: Option<TableWidth>,
    pub borders: Borders,
    pub shading: Option<Shading>,
    pub margins: CellMargins,
    pub vertical_align: Option<CellVerticalAlign>,
    pub no_wrap: Option<bool>,
    /// `TCGRF.textFlow` when not left-to-right/top-to-bottom (0).
    pub text_direction: Option<u8>,
    pub fit_text: Option<bool>,
    pub hide_mark: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedTable {
    pub props: TableProps,
    /// Column widths (twips) from the cell boundaries, one per grid column.
    pub grid: Vec<Option<u32>>,
    pub columns: usize,
    pub rows: Vec<NodeId>,
    /// 0 for a table of the body or of a header, 1 for a table nested in a
    /// cell, ...
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
    /// Grid column of the left edge.
    pub column_index: usize,
    pub row_span: usize,
    pub col_span: usize,
    /// Empty cell added to keep the grid rectangular.
    pub synthetic: bool,
}

/// Properties the rich model cannot carry, indexed by node id.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedDocTables {
    pub tables: BTreeMap<NodeId, ImportedTable>,
    pub rows: BTreeMap<NodeId, ImportedRow>,
    pub cells: BTreeMap<NodeId, ImportedCell>,
    /// Paragraph directly inside a cell -> that cell.
    pub paragraph_cell: BTreeMap<NodeId, NodeId>,
}

// ----- raw structures ---------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Brc {
    width: u8,
    kind: u8,
    color: TableColor,
    space: u8,
}

impl Brc {
    /// `Brc80` (4 bytes): width, type, `ico`, space/flags.
    fn from80(b: &[u8]) -> Option<Brc> {
        let b = b.get(..4)?;
        Some(Brc {
            width: b[0],
            kind: b[1],
            color: ico_color(b[2]).map_or(TableColor::Auto, TableColor::Rgb),
            space: b[3] & 0x1F,
        })
    }
    /// `Brc` (8 bytes): `COLORREF` (r, g, b, 0xFF = automatic), width, type,
    /// space/flags, unused.
    fn from8(b: &[u8]) -> Option<Brc> {
        let b = b.get(..8)?;
        Some(Brc {
            width: b[4],
            kind: b[5],
            color: if b[3] == 0xFF {
                TableColor::Auto
            } else {
                TableColor::Rgb(Color {
                    r: b[0],
                    g: b[1],
                    b: b[2],
                })
            },
            space: b[6] & 0x1F,
        })
    }
    fn to_border(self) -> Option<Border> {
        // All-zero or all-ones records mean "not set".
        if self.width == 0 && self.kind == 0 {
            return None;
        }
        if self.width == 0xFF && self.kind == 0xFF {
            return None;
        }
        Some(Border {
            style: border_style(self.kind).to_owned(),
            size: (self.width != 0xFF && self.width != 0).then_some(u32::from(self.width)),
            space: Some(u32::from(self.space)),
            color: Some(self.color),
        })
    }
}

/// `brcType` -> OOXML border name. Values 1, 3, 6, 7 are written by
/// LibreOffice (single, double, dotted, dashed); the others come from
/// [MS-DOC] only.
pub(crate) fn border_style(kind: u8) -> &'static str {
    match kind {
        0 | 255 => "nil",
        1 => "single",
        2 => "thick",
        3 => "double",
        5 => "hairline",
        6 => "dotted",
        7 => "dashed",
        8 => "dotDash",
        9 => "dotDotDash",
        10 => "triple",
        11 => "thinThickSmallGap",
        12 => "thickThinSmallGap",
        13 => "thinThickThinSmallGap",
        14 => "thinThickMediumGap",
        15 => "thickThinMediumGap",
        16 => "thinThickThinMediumGap",
        17 => "thinThickLargeGap",
        18 => "thickThinLargeGap",
        19 => "thinThickThinLargeGap",
        20 => "wave",
        21 => "doubleWave",
        22 => "dashSmallGap",
        23 => "dashDotStroked",
        24 => "threeDEmboss",
        25 => "threeDEngrave",
        26 => "outset",
        27 => "inset",
        _ => "single",
    }
}

fn pattern_name(ipat: u16) -> &'static str {
    match ipat {
        0 => "clear",
        1 => "solid",
        2 => "pct5",
        3 => "pct10",
        4 => "pct20",
        5 => "pct25",
        6 => "pct30",
        7 => "pct40",
        8 => "pct50",
        9 => "pct60",
        10 => "pct70",
        11 => "pct75",
        12 => "pct80",
        13 => "pct90",
        14 => "horzStripe",
        15 => "vertStripe",
        16 => "reverseDiagStripe",
        17 => "diagStripe",
        18 => "horzCross",
        19 => "diagCross",
        20 => "thinHorzStripe",
        21 => "thinVertStripe",
        22 => "thinReverseDiagStripe",
        23 => "thinDiagStripe",
        24 => "thinHorzCross",
        25 => "thinDiagCross",
        _ => "clear",
    }
}

fn colour4(b: &[u8]) -> TableColor {
    if b[3] == 0xFF {
        TableColor::Auto
    } else {
        TableColor::Rgb(Color {
            r: b[0],
            g: b[1],
            b: b[2],
        })
    }
}

/// `Shd` (10 bytes): foreground, background, pattern. Automatic colours are
/// kept as `Auto`; a `clear` pattern with automatic background is no shading.
fn shd10(b: &[u8]) -> Option<Shading> {
    let b = b.get(..10)?;
    let ipat = u16::from_le_bytes([b[8], b[9]]);
    shading_of(colour4(&b[0..4]), colour4(&b[4..8]), ipat)
}

/// `Shd80` (2 bytes): `icoFore` (5 bits), `icoBack` (5 bits), `ipat` (6 bits).
fn shd80(w: u16) -> Option<Shading> {
    let c = |i: u16| ico_color(i as u8).map_or(TableColor::Auto, TableColor::Rgb);
    shading_of(c(w & 0x1F), c((w >> 5) & 0x1F), w >> 10)
}

fn shading_of(fore: TableColor, back: TableColor, ipat: u16) -> Option<Shading> {
    if ipat == 0 && back == TableColor::Auto {
        return None;
    }
    Some(Shading {
        pattern: pattern_name(ipat).to_owned(),
        color: Some(fore),
        fill: Some(back),
    })
}

/// `TC80`/`TC`: `TCGRF`, preferred width, four `Brc80`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Tc {
    grf: u16,
    width: u16,
    brc: [Option<Brc>; 4],
}

impl Tc {
    fn horz_merge(&self) -> u8 {
        (self.grf & 3) as u8
    }
    fn text_flow(&self) -> u8 {
        ((self.grf >> 2) & 7) as u8
    }
    /// 0 none, 1 continuation (`fVertMerge`), 2 start (`fVertRestart`).
    fn vert_merge(&self) -> u8 {
        if self.grf & 0x40 != 0 {
            2
        } else {
            u8::from(self.grf & 0x20 != 0)
        }
    }
    fn vert_align(&self) -> u8 {
        ((self.grf >> 7) & 3) as u8
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DefTable {
    centers: Vec<i16>,
    tcs: Vec<Tc>,
}

#[derive(Clone, Debug, PartialEq)]
struct SetBrc {
    first: u8,
    lim: u8,
    sides: u8,
    brc: Brc,
}

/// Table properties gathered from the sprms of a paragraph (the end-of-row
/// paragraph holds the row's).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TapDelta {
    pub any: bool,
    def: Option<DefTable>,
    jc: Option<u8>,
    dxa_left: Option<i16>,
    gap_half: Option<i16>,
    row_height: Option<i16>,
    header: Option<bool>,
    cant_split: Option<bool>,
    borders: Option<[Option<Brc>; 6]>,
    shd: Vec<Option<Shading>>,
    set_brc: Vec<SetBrc>,
    width: Option<(u8, u16)>,
    padding_default: Vec<(u8, TableWidth)>,
    padding: Vec<(u8, u8, u8, TableWidth)>,
}

fn width_of(fts: u8, w: u16) -> TableWidth {
    match fts {
        1 => TableWidth::Auto,
        2 => TableWidth::Pct(u32::from(w)),
        3 => TableWidth::Dxa(i32::from(w)),
        _ => TableWidth::Nil,
    }
}

impl TapDelta {
    pub(crate) fn apply(&mut self, prl: &Prl<'_>, stats: &mut Stats) {
        self.any = true;
        let flag = |p: &Prl<'_>| p.byte().map(|b| b & 1 != 0);
        let op = &prl.operand;
        match prl.op {
            SPRM_T_DEF_TABLE => self.def = parse_def_table(op).or(self.def.take()),
            SPRM_T_JC | SPRM_T_JC90 => self.jc = prl.byte().or(self.jc),
            SPRM_T_DXA_LEFT => self.dxa_left = prl.i16().or(self.dxa_left),
            SPRM_T_DXA_GAP_HALF => self.gap_half = prl.i16().or(self.gap_half),
            SPRM_T_DY_ROW_HEIGHT => self.row_height = prl.i16().or(self.row_height),
            SPRM_T_HEADER => self.header = flag(prl).or(self.header),
            SPRM_T_CANT_SPLIT | SPRM_T_CANT_SPLIT90 => {
                self.cant_split = flag(prl).or(self.cant_split)
            }
            SPRM_T_BORDERS | SPRM_T_BORDERS80 => {
                let (size, parse): (usize, BrcParser) = if prl.op == SPRM_T_BORDERS {
                    (8, Brc::from8)
                } else {
                    (4, Brc::from80)
                };
                let mut b = [None; 6];
                for (i, slot) in b.iter_mut().enumerate() {
                    *slot = op.get(1 + i * size..).and_then(parse);
                }
                self.borders = Some(b);
            }
            SPRM_T_DEF_SHD | SPRM_T_DEF_SHD2 | SPRM_T_DEF_SHD3 => {
                let offset = match prl.op {
                    SPRM_T_DEF_SHD => 0,
                    SPRM_T_DEF_SHD2 => 22,
                    _ => 44,
                };
                let data = op.get(1..).unwrap_or(&[]);
                if self.shd.len() < offset {
                    self.shd.resize(offset, None);
                }
                for (i, c) in data.chunks_exact(10).take(22).enumerate() {
                    set_at(&mut self.shd, offset + i, shd10(c));
                }
            }
            SPRM_T_DEF_SHD80 => {
                if self.shd.is_empty() {
                    for (i, c) in op
                        .get(1..)
                        .unwrap_or(&[])
                        .chunks_exact(2)
                        .take(22)
                        .enumerate()
                    {
                        set_at(&mut self.shd, i, shd80(u16::from_le_bytes([c[0], c[1]])));
                    }
                }
            }
            SPRM_T_SET_BRC | SPRM_T_SET_BRC80 => {
                let brc = if prl.op == SPRM_T_SET_BRC {
                    op.get(4..).and_then(Brc::from8)
                } else {
                    op.get(4..).and_then(Brc::from80)
                };
                if let (Some(brc), Some(&first), Some(&lim), Some(&sides)) =
                    (brc, op.get(1), op.get(2), op.get(3))
                {
                    self.set_brc.push(SetBrc {
                        first,
                        lim,
                        sides,
                        brc,
                    });
                }
            }
            SPRM_T_TABLE_WIDTH => {
                if let (Some(&f), Some(w)) = (op.first(), op.get(1..3)) {
                    self.width = Some((f, u16::from_le_bytes([w[0], w[1]])));
                }
            }
            SPRM_T_CELL_PADDING_DEFAULT | SPRM_T_CELL_PADDING => {
                // cb, itcFirst, itcLim, grfbrc, ftsWidth, wWidth
                if let (Some(&first), Some(&lim), Some(&sides), Some(&fts), Some(w)) =
                    (op.get(1), op.get(2), op.get(3), op.get(4), op.get(5..7))
                {
                    let width = width_of(fts, u16::from_le_bytes([w[0], w[1]]));
                    if prl.op == SPRM_T_CELL_PADDING_DEFAULT {
                        self.padding_default.push((sides, width));
                    } else {
                        self.padding.push((first, lim, sides, width));
                    }
                }
            }
            op if T_SILENT.contains(&op) => {}
            _ => stats.unsupported("tableFormatting"),
        }
    }
}

type BrcParser = fn(&[u8]) -> Option<Brc>;

fn set_at(v: &mut Vec<Option<Shading>>, i: usize, s: Option<Shading>) {
    if v.len() <= i {
        v.resize(i + 1, None);
    }
    v[i] = s;
}

/// Maximum number of cells in a row definition ([MS-DOC] `itcMac` is 63).
const MAX_ROW_CELLS: usize = 64;

fn parse_def_table(op: &[u8]) -> Option<DefTable> {
    // operand: cb (2 bytes), columns (1 byte), centres, TC80 x columns.
    let n = usize::from(*op.get(2)?);
    if n > MAX_ROW_CELLS {
        return None;
    }
    let centers: Vec<i16> = (0..=n)
        .map(|i| {
            let o = 3 + 2 * i;
            op.get(o..o + 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
        })
        .collect::<Option<_>>()?;
    let base = 3 + 2 * (n + 1);
    let mut tcs = Vec::with_capacity(n);
    for i in 0..n {
        let o = base + 20 * i;
        let Some(b) = op.get(o..o + 20) else {
            tcs.push(Tc::default());
            continue;
        };
        tcs.push(Tc {
            grf: u16::from_le_bytes([b[0], b[1]]),
            width: u16::from_le_bytes([b[2], b[3]]),
            brc: [
                Brc::from80(&b[4..]),
                Brc::from80(&b[8..]),
                Brc::from80(&b[12..]),
                Brc::from80(&b[16..]),
            ],
        });
    }
    Some(DefTable { centers, tcs })
}

// ----- construction -------------------------------------------------------------

/// A cell already built (content in the node map) waiting for its place.
pub(crate) struct CellIn {
    pub blocks: Vec<NodeId>,
    /// Only empty paragraphs (a lone empty paragraph).
    pub empty: bool,
}

pub(crate) struct RowIn {
    pub cells: Vec<CellIn>,
    pub tap: Option<TapDelta>,
}

struct Group {
    left: i32,
    right: i32,
    tc: Tc,
    tc_index: usize,
}

struct Placed {
    cs: usize,
    ce: usize,
    cell: Option<CellIn>,
    tc: Option<Tc>,
    tc_index: usize,
    /// Row of the cell it continues (vertical merge).
    absorbed: bool,
    row_span: usize,
    id: NodeId,
}

pub(crate) struct BuildCtx<'a> {
    pub doc: &'a mut RichDocument,
    pub nodes: &'a mut BTreeMap<NodeId, Node>,
    pub tables: &'a mut ImportedDocTables,
    pub rep: &'a mut Repairs,
    pub limits: &'a DocLimits,
    pub depth: usize,
}

/// Why a table is flattened instead of built.
pub(crate) enum Flatten {
    Invalid,
    Fatal(DocError),
}

fn cluster(mut positions: Vec<i32>) -> Vec<i32> {
    positions.sort_unstable();
    positions.dedup();
    let mut out: Vec<i32> = Vec::with_capacity(positions.len());
    for p in positions {
        match out.last() {
            Some(&l) if p - l <= 3 => {}
            _ => out.push(p),
        }
    }
    out
}

fn col_of(clusters: &[i32], p: i32) -> usize {
    let i = clusters.partition_point(|c| *c <= p + 3);
    i.saturating_sub(1)
}

fn groups_of(row: &RowIn, rep: &mut Repairs) -> Vec<Group> {
    let n = row.cells.len();
    let mut out: Vec<Group> = Vec::new();
    let def = row.tap.as_ref().and_then(|t| t.def.as_ref());
    if let Some(d) = def {
        let mut i = 0;
        while i < d.tcs.len() {
            let tc = d.tcs[i];
            let mut j = i + 1;
            if tc.horz_merge() == 1 {
                while j < d.tcs.len() && d.tcs[j].horz_merge() == 2 {
                    j += 1;
                }
            }
            out.push(Group {
                left: i32::from(d.centers[i]),
                right: i32::from(d.centers[j]),
                tc,
                tc_index: i,
            });
            i = j;
        }
    } else if n > 0 {
        rep.table("row without cell definition");
    }
    if out.len() != n {
        rep.table("cell count differs from the row definition");
        let mut next = out.last().map_or(0, |g| g.right);
        let w = out
            .last()
            .map_or(1440, |g| (g.right - g.left).clamp(1, 20_000));
        while out.len() < n {
            let i = out.len();
            out.push(Group {
                left: next,
                right: next + w,
                tc: Tc::default(),
                tc_index: i,
            });
            next += w;
        }
    }
    out
}

fn align_of(jc: Option<u8>) -> Option<TableAlign> {
    jc.and_then(|j| match j {
        0 => Some(TableAlign::Left),
        1 => Some(TableAlign::Center),
        2 => Some(TableAlign::Right),
        _ => None,
    })
}

fn sides_apply(b: &mut Borders, sides: u8, brc: Brc) {
    let v = brc.to_border();
    for (bit, slot) in [
        (1, &mut b.top),
        (2, &mut b.left),
        (4, &mut b.bottom),
        (8, &mut b.right),
    ] {
        if sides & bit != 0 {
            *slot = v.clone();
        }
    }
}

fn margins_apply(m: &mut CellMargins, sides: u8, w: TableWidth) {
    for (bit, slot) in [
        (1, &mut m.top),
        (2, &mut m.left),
        (4, &mut m.bottom),
        (8, &mut m.right),
    ] {
        if sides & bit != 0 {
            *slot = Some(w);
        }
    }
}

fn cell_props(
    tap: Option<&TapDelta>,
    g: Option<(&Tc, usize, i32)>,
    rep: &mut Repairs,
) -> CellProps {
    let mut p = CellProps::default();
    let Some((tc, idx, width)) = g else {
        return p;
    };
    p.width = Some(TableWidth::Dxa(width));
    p.borders = Borders {
        top: tc.brc[0].and_then(Brc::to_border),
        left: tc.brc[1].and_then(Brc::to_border),
        bottom: tc.brc[2].and_then(Brc::to_border),
        right: tc.brc[3].and_then(Brc::to_border),
        ..Borders::default()
    };
    p.vertical_align = match tc.vert_align() {
        0 => None,
        1 => Some(CellVerticalAlign::Center),
        _ => Some(CellVerticalAlign::Bottom),
    };
    p.no_wrap = (tc.grf & 0x1000 != 0).then_some(true);
    p.fit_text = (tc.grf & 0x0800 != 0).then_some(true);
    p.hide_mark = (tc.grf & 0x2000 != 0).then_some(true);
    if tc.text_flow() != 0 {
        p.text_direction = Some(tc.text_flow());
        rep.table("cell text direction");
    }
    if let Some(t) = tap {
        p.shading = t.shd.get(idx).cloned().flatten();
        for s in &t.set_brc {
            if (s.first as usize) <= idx && idx < s.lim as usize {
                sides_apply(&mut p.borders, s.sides, s.brc);
            }
        }
        for (first, lim, sides, w) in &t.padding {
            if (*first as usize) <= idx && idx < *lim as usize {
                margins_apply(&mut p.margins, *sides, *w);
            }
        }
    }
    p
}

fn table_props(tap: Option<&TapDelta>) -> (TableProps, RowProps) {
    let mut t = TableProps::default();
    let mut r = RowProps::default();
    let Some(tap) = tap else {
        return (t, r);
    };
    t.align = align_of(tap.jc);
    r.align = t.align;
    t.indent = tap.dxa_left.map(|v| TableWidth::Dxa(i32::from(v)));
    t.gap_half = tap.gap_half.map(i32::from);
    t.width = tap.width.map(|(f, w)| width_of(f, w));
    if let Some(b) = &tap.borders {
        t.borders = Borders {
            top: b[0].and_then(Brc::to_border),
            left: b[1].and_then(Brc::to_border),
            bottom: b[2].and_then(Brc::to_border),
            right: b[3].and_then(Brc::to_border),
            inside_h: b[4].and_then(Brc::to_border),
            inside_v: b[5].and_then(Brc::to_border),
        };
    }
    for (sides, w) in &tap.padding_default {
        margins_apply(&mut t.cell_margins, *sides, *w);
    }
    r.height = tap.row_height.and_then(|h| match h {
        0 => None,
        h if h > 0 => Some(RowHeight {
            twips: h as u32,
            rule: HeightRule::AtLeast,
        }),
        h => Some(RowHeight {
            twips: u32::from(h.unsigned_abs()),
            rule: HeightRule::Exact,
        }),
    });
    r.header = tap.header;
    r.cant_split = tap.cant_split;
    (t, r)
}

fn is_blank(nodes: &BTreeMap<NodeId, Node>, c: &CellIn) -> bool {
    c.empty
        || c.blocks.iter().all(|id| {
            matches!(nodes.get(id), Some(Node::Paragraph(p))
                if p.text.is_empty() && p.inline_ids().is_empty())
        })
}

/// Builds the table node of `rows` in `ctx.nodes` and returns its id. The
/// rows are normalised to an exactly rectangular grid (repairs are counted in
/// `ctx.rep`); `Err(Flatten::Invalid)` asks the caller to flatten the table.
pub(crate) fn build_table(ctx: &mut BuildCtx<'_>, mut rows: Vec<RowIn>) -> Result<NodeId, Flatten> {
    let fatal =
        |e: atelier_text_model::rich::RichError| Flatten::Fatal(DocError::Model(e.to_string()));
    if rows.is_empty() {
        return Err(Flatten::Invalid);
    }
    let max_cols = ctx.limits.max_table_columns;
    let all_groups: Vec<Vec<Group>> = rows.iter().map(|r| groups_of(r, ctx.rep)).collect();
    // Grid columns: union of the cell boundaries of every row.
    let mut positions = Vec::new();
    for g in all_groups.iter().flatten() {
        positions.push(g.left);
        positions.push(g.right);
    }
    let mut clusters = cluster(positions);
    let by_index = clusters.len().saturating_sub(1) > max_cols;
    if by_index {
        ctx.rep.table("too many grid columns");
        clusters.clear();
    }
    // Placement of every cell of every row on the grid.
    let mut placed: Vec<Vec<Placed>> = Vec::with_capacity(rows.len());
    let mut ncols = 0usize;
    for (r, groups) in all_groups.iter().enumerate() {
        let mut line: Vec<Placed> = Vec::with_capacity(groups.len());
        let mut cur = 0usize;
        for (i, g) in groups.iter().enumerate() {
            let (a, b) = if by_index {
                (i, i + 1)
            } else {
                (col_of(&clusters, g.left), col_of(&clusters, g.right))
            };
            let cs = a.max(cur);
            let ce = b.max(cs + 1);
            if cs > cur {
                ctx.rep.table("gap between cells filled");
                line.push(synthetic(ctx.doc, cur, cs).map_err(fatal)?);
            }
            let cell = rows[r].cells.get_mut(i).map(|c| CellIn {
                blocks: std::mem::take(&mut c.blocks),
                empty: c.empty,
            });
            line.push(Placed {
                cs,
                ce,
                cell,
                tc: Some(g.tc),
                tc_index: g.tc_index,
                absorbed: false,
                row_span: 1,
                id: ctx.doc.alloc_id().map_err(fatal)?,
            });
            cur = ce;
        }
        ncols = ncols.max(cur);
        placed.push(line);
    }
    if ncols == 0 || ncols > max_cols.max(1) + 1 {
        return Err(Flatten::Invalid);
    }
    for line in &mut placed {
        let end = line.last().map_or(0, |p| p.ce);
        if end < ncols {
            ctx.rep.table("short row completed");
            line.push(synthetic(ctx.doc, end, ncols).map_err(fatal)?);
        }
    }
    // Vertical merges: continuation cells disappear under their start cell.
    let mut open: BTreeMap<(usize, usize), (usize, usize, usize)> = BTreeMap::new();
    for r in 0..placed.len() {
        let mut next_open = BTreeMap::new();
        for i in 0..placed[r].len() {
            let (cs, ce) = (placed[r][i].cs, placed[r][i].ce);
            let vm = placed[r][i].tc.map_or(0, |t| t.vert_merge());
            match (vm, open.get(&(cs, ce)).copied()) {
                (1, Some((sr, si, last))) if last + 1 == r => {
                    placed[sr][si].row_span += 1;
                    placed[r][i].absorbed = true;
                    next_open.insert((cs, ce), (sr, si, r));
                    if let Some(c) = placed[r][i].cell.take() {
                        if is_blank(ctx.nodes, &c) {
                            for id in c.blocks {
                                remove_subtree(ctx.nodes, id);
                            }
                        } else {
                            ctx.rep.table("content of a vertically merged cell moved");
                            if let Some(target) = placed[sr][si].cell.as_mut() {
                                target.blocks.extend(c.blocks);
                                target.empty = false;
                            }
                        }
                    }
                }
                (vm, _) => {
                    if vm == 1 {
                        ctx.rep.table("vertical merge without a start cell");
                    }
                    if vm == 2 {
                        next_open.insert((cs, ce), (r, i, r));
                    }
                }
            }
        }
        open = next_open;
    }
    // Nodes.
    let table_id = ctx.doc.alloc_id().map_err(fatal)?;
    let (tprops, _) = table_props(rows.first().and_then(|r| r.tap.as_ref()));
    let mut row_ids = Vec::with_capacity(placed.len());
    let mut imported_rows = Vec::new();
    let mut imported_cells = Vec::new();
    for (r, line) in placed.into_iter().enumerate() {
        let row_id = ctx.doc.alloc_id().map_err(fatal)?;
        let tap = rows[r].tap.as_ref();
        let (_, rprops) = table_props(tap);
        let mut cell_ids = Vec::new();
        for p in line {
            if p.absorbed {
                continue;
            }
            let mut blocks = p.cell.map(|c| c.blocks).unwrap_or_default();
            let has_paragraph = blocks
                .iter()
                .any(|id| matches!(ctx.nodes.get(id), Some(Node::Paragraph(_))));
            if !has_paragraph {
                let pid = ctx.doc.alloc_id().map_err(fatal)?;
                ctx.nodes.insert(pid, Node::Paragraph(Paragraph::default()));
                blocks.push(pid);
            }
            let direct: Vec<NodeId> = blocks
                .iter()
                .copied()
                .filter(|id| matches!(ctx.nodes.get(id), Some(Node::Paragraph(_))))
                .collect();
            for pid in direct {
                ctx.tables.paragraph_cell.insert(pid, p.id);
            }
            let width = tap.and_then(|t| t.def.as_ref()).map_or(0, |_| 0);
            let _ = width;
            let tc = p.tc;
            let span_w = (clusters.get(p.ce).copied().unwrap_or(0)
                - clusters.get(p.cs).copied().unwrap_or(0))
            .max(0);
            ctx.nodes.insert(
                p.id,
                Node::Cell(Cell {
                    col_span: p.ce - p.cs,
                    row_span: p.row_span,
                    blocks: BlockList::from_ids(&blocks).map_err(fatal)?,
                    opaque: Vec::new(),
                }),
            );
            imported_cells.push((
                p.id,
                ImportedCell {
                    props: cell_props(tap, tc.as_ref().map(|t| (t, p.tc_index, span_w)), ctx.rep),
                    table: Some(table_id),
                    row: Some(row_id),
                    row_index: r,
                    column_index: p.cs,
                    row_span: p.row_span,
                    col_span: p.ce - p.cs,
                    synthetic: tc.is_none(),
                },
            ));
            cell_ids.push(p.id);
        }
        ctx.nodes.insert(
            row_id,
            Node::Row(Row {
                cells: cell_ids,
                opaque: Vec::new(),
            }),
        );
        imported_rows.push((
            row_id,
            ImportedRow {
                props: rprops,
                table: Some(table_id),
                index: r,
            },
        ));
        row_ids.push(row_id);
    }
    // The grid must be exactly rectangular: check before installing.
    if !grid_ok(ctx.nodes, &row_ids, ncols) {
        for (id, _) in imported_cells {
            ctx.nodes.remove(&id);
        }
        for id in row_ids {
            ctx.nodes.remove(&id);
        }
        return Err(Flatten::Invalid);
    }
    ctx.nodes.insert(
        table_id,
        Node::Table(Table {
            columns: ncols,
            rows: row_ids.clone(),
            style: None,
            opaque: Vec::new(),
        }),
    );
    let grid: Vec<Option<u32>> = if by_index {
        vec![None; ncols]
    } else {
        (0..ncols)
            .map(|c| match (clusters.get(c), clusters.get(c + 1)) {
                (Some(a), Some(b)) if b > a => Some((b - a) as u32),
                _ => None,
            })
            .collect()
    };
    ctx.tables.tables.insert(
        table_id,
        ImportedTable {
            props: tprops,
            grid,
            columns: ncols,
            rows: row_ids,
            depth: ctx.depth,
        },
    );
    ctx.tables.rows.extend(imported_rows);
    ctx.tables.cells.extend(imported_cells);
    Ok(table_id)
}

fn synthetic(
    doc: &mut RichDocument,
    cs: usize,
    ce: usize,
) -> Result<Placed, atelier_text_model::rich::RichError> {
    Ok(Placed {
        cs,
        ce,
        cell: None,
        tc: None,
        tc_index: 0,
        absorbed: false,
        row_span: 1,
        id: doc.alloc_id()?,
    })
}

/// Occupancy check equivalent to the model's: every slot covered exactly once.
fn grid_ok(nodes: &BTreeMap<NodeId, Node>, rows: &[NodeId], cols: usize) -> bool {
    let mut owner = vec![vec![false; cols]; rows.len()];
    for (r, rid) in rows.iter().enumerate() {
        let Some(Node::Row(row)) = nodes.get(rid) else {
            return false;
        };
        let mut c = 0;
        for cid in &row.cells {
            let Some(Node::Cell(cell)) = nodes.get(cid) else {
                return false;
            };
            while c < cols && owner[r][c] {
                c += 1;
            }
            if cell.col_span == 0
                || cell.row_span == 0
                || c + cell.col_span > cols
                || r + cell.row_span > rows.len()
            {
                return false;
            }
            for line in &mut owner[r..r + cell.row_span] {
                for slot in &mut line[c..c + cell.col_span] {
                    if *slot {
                        return false;
                    }
                    *slot = true;
                }
            }
            c += cell.col_span;
        }
        if owner[r].iter().any(|s| !*s) {
            return false;
        }
    }
    true
}
