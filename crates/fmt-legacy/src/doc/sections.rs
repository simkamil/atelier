// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sections ([MS-DOC] 2.8.25 `PlcfSed`, 2.9.220 `Sed`, 2.9.221 `Sepx`): the
//! character positions of the section marks and the section properties
//! (`SEPX`) mapped to the companion structures. The model carries a section
//! per section of the file; page geometry, columns and numbering have no slot
//! in it and are kept in [`ImportedSection`] (and as opaque attributes).

use super::lists::NumberFormat;
use super::sprm::{Grpprl, Prl, Stats};
use super::{DocError, DocLimits, rd_u16, rd_u32};
use atelier_text_model::rich::{HfKind, HfVariant, NodeId};

// Section sprms. 0x301D, 0xB01F..0xB022, 0x9023, 0x9024, 0xB017, 0xB018, 0x500B,
// 0x900C, 0x3005, 0x3009, 0x3011, 0x3019, 0x501C, 0x300E and 0x300A are written by
// LibreOffice (0x5032 and 0x9031 are written too but are not column sprms); the rest come from
// [MS-DOC] only.
const SPRM_S_BKC: u16 = 0x3009;
const SPRM_S_FTITLEPAGE: u16 = 0x300A;
const SPRM_S_CCOLUMNS: u16 = 0x500B;
const SPRM_S_DXACOLUMNS: u16 = 0x900C;
const SPRM_S_FEVENLYSPACED: u16 = 0x3005;
const SPRM_S_LBETWEEN: u16 = 0x3019;
const SPRM_S_DXACOLWIDTH: u16 = 0xF203;
const SPRM_S_DXACOLSPACING: u16 = 0xF204;
const SPRM_S_NFCPGN: u16 = 0x300E;
const SPRM_S_FPGNRESTART: u16 = 0x3011;
const SPRM_S_PGNSTART97: u16 = 0x501C;
const SPRM_S_PGNSTART: u16 = 0x7044;
const SPRM_S_DMORIENTPAGE: u16 = 0x301D;
const SPRM_S_XAPAGE: u16 = 0xB01F;
const SPRM_S_YAPAGE: u16 = 0xB020;
const SPRM_S_DXALEFT: u16 = 0xB021;
const SPRM_S_DXARIGHT: u16 = 0xB022;
const SPRM_S_DYATOP: u16 = 0x9023;
const SPRM_S_DYABOTTOM: u16 = 0x9024;
const SPRM_S_DZAGUTTER: u16 = 0xB025;
const SPRM_S_DYAHDRTOP: u16 = 0xB017;
const SPRM_S_DYAHDRBOTTOM: u16 = 0xB018;
/// Known and ignored without notice (printer trays, line numbers,
/// text-flow, revision ids, bidi...).
const S_SILENT: [u16; 15] = [
    0x3014, 0x5032, 0x9031, 0x303C, 0x5040, 0x5042, 0x7030, 0x5033, 0x3228, 0x3006, 0x5007, 0x5008,
    0x302A, 0x3012, 0x301A,
];

/// Type of section break (`sprmSBkc`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionBreak {
    Continuous,
    NewColumn,
    NewPage,
    EvenPage,
    OddPage,
}

/// Page geometry in twips.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageSetup {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub landscape: bool,
    pub margin_top: Option<i32>,
    pub margin_bottom: Option<i32>,
    pub margin_left: Option<i32>,
    pub margin_right: Option<i32>,
    pub gutter: Option<i32>,
    pub header_distance: Option<i32>,
    pub footer_distance: Option<i32>,
}

/// Columns of a section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnSetup {
    pub count: u32,
    /// Spacing between columns (twips) when the columns are evenly spaced.
    pub spacing: Option<i32>,
    pub evenly_spaced: bool,
    pub separator: bool,
    /// `(width, spacing after)` of each column when they differ.
    pub widths: Vec<(i32, Option<i32>)>,
}

impl Default for ColumnSetup {
    fn default() -> Self {
        Self {
            count: 1,
            spacing: None,
            evenly_spaced: true,
            separator: false,
            widths: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageNumbering {
    pub format: Option<NumberFormat>,
    pub restart: bool,
    pub start: Option<u32>,
}

/// State of a header or footer slot of a section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HfState {
    /// The story is empty in the file: the previous section's applies.
    Inherited,
    /// An explicit header with no content.
    Empty,
    Present(NodeId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportedHeaderFooter {
    pub kind: HfKind,
    pub variant: HfVariant,
    pub state: HfState,
}

/// A section of the file with its model section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedSection {
    pub section: NodeId,
    pub index: usize,
    /// First character position of the section and position after its mark.
    pub first_cp: usize,
    pub end_cp: usize,
    pub break_type: SectionBreak,
    pub page: PageSetup,
    pub columns: ColumnSetup,
    pub page_numbers: PageNumbering,
    /// `sprmSFTitlePage`: first page different.
    pub title_page: bool,
    /// Header and footer slots that apply to the section.
    pub headers_footers: Vec<ImportedHeaderFooter>,
}

/// Properties read from a `SEPX`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sepx {
    pub bkc: Option<u8>,
    pub title_page: bool,
    pub page: PageSetup,
    pub columns: ColumnSetup,
    pub numbering: PageNumbering,
}

pub(crate) struct SectionDef {
    pub first_cp: usize,
    pub end_cp: usize,
    pub sepx: Sepx,
}

fn s16(p: &Prl<'_>) -> Option<i32> {
    p.i16().map(i32::from)
}

impl Sepx {
    fn apply(&mut self, p: &Prl<'_>, stats: &mut Stats) {
        let page = &mut self.page;
        match p.op {
            SPRM_S_BKC => self.bkc = p.byte(),
            SPRM_S_FTITLEPAGE => self.title_page = p.byte().is_some_and(|b| b & 1 != 0),
            SPRM_S_CCOLUMNS => {
                self.columns.count = p.u16().map_or(1, |c| u32::from(c).saturating_add(1));
            }
            SPRM_S_DXACOLUMNS => self.columns.spacing = s16(p),
            SPRM_S_FEVENLYSPACED => {
                self.columns.evenly_spaced = p.byte().is_some_and(|b| b & 1 != 0)
            }
            SPRM_S_LBETWEEN => self.columns.separator = p.byte().is_some_and(|b| b & 1 != 0),
            SPRM_S_DXACOLWIDTH => {
                // itc (1 byte), width (2 bytes)
                if let (Some(&i), Some(w)) = (p.operand.first(), p.operand.get(1..3)) {
                    let i = usize::from(i);
                    if i < 45 {
                        if self.columns.widths.len() <= i {
                            self.columns.widths.resize(i + 1, (0, None));
                        }
                        self.columns.widths[i].0 = i32::from(i16::from_le_bytes([w[0], w[1]]));
                    }
                }
            }
            SPRM_S_DXACOLSPACING => {
                if let (Some(&i), Some(w)) = (p.operand.first(), p.operand.get(1..3)) {
                    let i = usize::from(i);
                    if i < 45 {
                        if self.columns.widths.len() <= i {
                            self.columns.widths.resize(i + 1, (0, None));
                        }
                        self.columns.widths[i].1 =
                            Some(i32::from(i16::from_le_bytes([w[0], w[1]])));
                    }
                }
            }
            SPRM_S_NFCPGN => self.numbering.format = p.byte().map(NumberFormat::from_nfc),
            SPRM_S_FPGNRESTART => self.numbering.restart = p.byte().is_some_and(|b| b & 1 != 0),
            SPRM_S_PGNSTART97 => self.numbering.start = p.u16().map(u32::from),
            SPRM_S_PGNSTART => self.numbering.start = p.u32(),
            SPRM_S_DMORIENTPAGE => page.landscape = p.byte() == Some(2),
            SPRM_S_XAPAGE => page.width = s16(p),
            SPRM_S_YAPAGE => page.height = s16(p),
            SPRM_S_DXALEFT => page.margin_left = s16(p),
            SPRM_S_DXARIGHT => page.margin_right = s16(p),
            SPRM_S_DYATOP => page.margin_top = s16(p),
            SPRM_S_DYABOTTOM => page.margin_bottom = s16(p),
            SPRM_S_DZAGUTTER => page.gutter = s16(p),
            SPRM_S_DYAHDRTOP => page.header_distance = s16(p),
            SPRM_S_DYAHDRBOTTOM => page.footer_distance = s16(p),
            op if S_SILENT.contains(&op) => {}
            _ => stats.unsupported("sectionProperties"),
        }
    }
}

/// Reads the `PlcfSed` (`fc`, `lcb` in the table stream) and the `SEPX` of
/// every section. `text_end` is the end of the main story.
pub(crate) fn parse(
    table: &[u8],
    word: &[u8],
    (fc, lcb): (usize, usize),
    text_end: usize,
    limits: &DocLimits,
    stats: &mut Stats,
) -> Result<(Vec<SectionDef>, usize), DocError> {
    let Some(data) = fc
        .checked_add(lcb)
        .and_then(|e| table.get(fc..e))
        .filter(|d| d.len() >= 4)
    else {
        return Ok((Vec::new(), 0));
    };
    let n = (data.len() - 4) / 16;
    if n > limits.max_sections {
        return Err(DocError::Limit("section count"));
    }
    let mut out = Vec::with_capacity(n);
    let mut damaged = 0;
    let mut prev = 0usize;
    for i in 0..n {
        let end = (rd_u32(data, (i + 1) * 4).unwrap_or(0) as usize).max(prev);
        let first = prev;
        prev = end;
        let sed = (n + 1) * 4 + i * 12;
        let fc_sepx = rd_u32(data, sed + 2).unwrap_or(0xFFFF_FFFF);
        let mut sepx = Sepx::default();
        if fc_sepx != 0xFFFF_FFFF && fc_sepx != 0 {
            let at = fc_sepx as usize;
            let grpprl = rd_u16(word, at)
                .map(usize::from)
                .and_then(|cb| word.get(at + 2..at.checked_add(2)?.checked_add(cb)?));
            match grpprl {
                Some(g) => {
                    let mut it = Grpprl::new(g);
                    for prl in it.by_ref() {
                        stats.sprms += 1;
                        sepx.apply(&prl, stats);
                    }
                    if it.truncated {
                        stats.truncated_grpprls += 1;
                    }
                }
                None => damaged += 1,
            }
        }
        out.push(SectionDef {
            first_cp: first,
            end_cp: end.min(text_end.max(first)),
            sepx,
        });
    }
    Ok((out, damaged))
}

impl SectionBreak {
    pub(crate) fn from_bkc(b: Option<u8>) -> SectionBreak {
        match b {
            Some(0) => SectionBreak::Continuous,
            Some(1) => SectionBreak::NewColumn,
            Some(3) => SectionBreak::EvenPage,
            Some(4) => SectionBreak::OddPage,
            _ => SectionBreak::NewPage,
        }
    }
}
