// SPDX-License-Identifier: AGPL-3.0-or-later
//! Lists ([MS-DOC] 2.9.147 `PlfLst`, 2.9.150 `PlfLfo`, 2.9.150 `LVL`): list
//! definitions (`LSTF` + `LVL`), list instances (`LFO` with `LFOLVL`
//! overrides) and their conversion to the companion structures. A paragraph
//! carries `ListRef { list_id, level }` where `list_id` is the 1-based index of
//! its `LFO` (`sprmPIlfo`), so two paragraphs of the same `LFO` share one
//! numbering sequence.

use super::sprm::{CharDelta, ParaDelta, Stats};
use super::{DocError, DocFont, DocLimits, rd_u16, rd_u32};
use atelier_text_model::rich::{CharAttrs, ParaProps};

/// Number format (`nfc`). Values 0-4, 23 and 255 are written by LibreOffice;
/// the other names come from [MS-DOC] only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormat {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetter,
    LowerLetter,
    Ordinal,
    CardinalText,
    OrdinalText,
    Hex,
    DecimalZero,
    Bullet,
    None,
    /// Any other `nfc` value, kept as read.
    Other(u8),
}

impl NumberFormat {
    pub fn from_nfc(nfc: u8) -> NumberFormat {
        match nfc {
            0 => NumberFormat::Decimal,
            1 => NumberFormat::UpperRoman,
            2 => NumberFormat::LowerRoman,
            3 => NumberFormat::UpperLetter,
            4 => NumberFormat::LowerLetter,
            5 => NumberFormat::Ordinal,
            6 => NumberFormat::CardinalText,
            7 => NumberFormat::OrdinalText,
            8 => NumberFormat::Hex,
            22 => NumberFormat::DecimalZero,
            23 => NumberFormat::Bullet,
            255 => NumberFormat::None,
            n => NumberFormat::Other(n),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelAlignment {
    Left,
    Center,
    Right,
}

/// What follows the number (`ixchFollow`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LevelFollow {
    Tab,
    Space,
    Nothing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListLevelKind {
    /// The bullet text (usually one character, possibly in a symbol font).
    Bullet(String),
    /// `text` is the number text with `%1`..`%9` standing for the number of
    /// level 1..9, e.g. `"%1.%2)"`.
    Number {
        format: NumberFormat,
        text: String,
        start: u32,
    },
    /// No number (`nfc` 255).
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedListLevel {
    /// Zero-based level.
    pub level: u8,
    pub kind: ListLevelKind,
    pub alignment: LevelAlignment,
    pub follow: LevelFollow,
    /// `fLegal`: legal numbering (arabic numbers for all levels).
    pub legal: bool,
    /// `fNoRestart`: the level does not restart after a higher one.
    pub no_restart: bool,
    /// `ilvlRestartLim` as read (0: no limit stored).
    pub restart_limit: u8,
    /// Paragraph properties of the level (`grpprlPapx`): indents, alignment.
    pub para_props: ParaProps,
    /// Character properties of the number (`grpprlChpx`).
    pub char_attrs: CharAttrs,
    /// The `LFO` replaced the start value (`fStartAt`).
    pub start_overridden: bool,
    /// The `LFO` replaced the whole level (`fFormatting`).
    pub overridden: bool,
}

/// List definition behind a `ListRef::list_id` (an `LFO`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedList {
    /// 1-based `LFO` index, the value of `ListRef::list_id`.
    pub list_id: u32,
    /// `LSTF.lsid`.
    pub lsid: u32,
    /// `LSTF.tplc`.
    pub template_id: u32,
    /// Single-level list (`fSimpleList`).
    pub simple: bool,
    /// Another `LFO` of the document refers to the same `LSTF`.
    pub shared_definition: bool,
    pub levels: Vec<ImportedListLevel>,
}

/// List information of one paragraph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedListItem {
    pub paragraph: atelier_text_model::rich::NodeId,
    pub list_id: u32,
    pub level: u8,
    /// The list reference comes from the paragraph style.
    pub from_style: bool,
}

#[derive(Clone, Debug)]
struct Lvl {
    start: i32,
    nfc: u8,
    flags: u8,
    follow: u8,
    restart_lim: u8,
    papx: Vec<u8>,
    chpx: Vec<u8>,
    text: Vec<u16>,
}

#[derive(Clone, Debug)]
struct Lstf {
    lsid: u32,
    tplc: u32,
    simple: bool,
    levels: Vec<Lvl>,
}

#[derive(Clone, Debug)]
struct LfoLvl {
    level: u8,
    start_at: Option<i32>,
    format: Option<Lvl>,
}

#[derive(Clone, Debug)]
struct Lfo {
    lsid: u32,
    levels: Vec<LfoLvl>,
}

/// Parsed list tables.
#[derive(Clone, Debug, Default)]
pub(crate) struct ListTables {
    lstf: Vec<Lstf>,
    lfo: Vec<Lfo>,
    pub damaged: usize,
}

/// Reads one `LVL` at `pos`; returns it and the position after it.
fn read_lvl(t: &[u8], pos: usize) -> Option<(Lvl, usize)> {
    let h = t.get(pos..pos.checked_add(28)?)?;
    let cb_chpx = usize::from(h[24]);
    let cb_papx = usize::from(h[25]);
    let papx_at = pos + 28;
    let chpx_at = papx_at.checked_add(cb_papx)?;
    let xst_at = chpx_at.checked_add(cb_chpx)?;
    let papx = t.get(papx_at..chpx_at)?.to_vec();
    let chpx = t.get(chpx_at..xst_at)?.to_vec();
    let cch = usize::from(rd_u16(t, xst_at)?);
    let text_end = xst_at.checked_add(2)?.checked_add(cch.checked_mul(2)?)?;
    let text = t
        .get(xst_at + 2..text_end)?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some((
        Lvl {
            start: rd_u32(h, 0)? as i32,
            nfc: h[4],
            flags: h[5],
            follow: h[15],
            restart_lim: h[26],
            papx,
            chpx,
            text,
        },
        text_end,
    ))
}

impl ListTables {
    /// Parses `PlfLst` (`fc_lst`, `lcb_lst`) and `PlfLfo`. The `LVL` arrays
    /// follow the `LSTF` array in the table stream.
    pub fn parse(
        table: &[u8],
        lst: (usize, usize),
        lfo: (usize, usize),
        limits: &DocLimits,
    ) -> Result<ListTables, DocError> {
        let mut out = ListTables::default();
        let (fc, lcb) = lst;
        if lcb >= 2 && fc.checked_add(lcb).is_some_and(|e| e <= table.len()) {
            let n = usize::from(rd_u16(table, fc).unwrap_or(0));
            if n > limits.max_lists {
                return Err(DocError::Limit("list count"));
            }
            let mut pos = fc + 2;
            let mut heads = Vec::with_capacity(n.min(table.len() / 28 + 1));
            for _ in 0..n {
                let Some(h) = table.get(pos..pos + 28) else {
                    out.damaged += 1;
                    break;
                };
                let flags = h[26];
                heads.push((
                    rd_u32(h, 0).unwrap_or(0),
                    rd_u32(h, 4).unwrap_or(0),
                    flags & 1 != 0,
                ));
                pos += 28;
            }
            // The LVLs start after the PlfLst proper (`lcb` covers only the
            // header and the LSTF array).
            let mut at = fc + lcb;
            'lists: for (lsid, tplc, simple) in heads {
                let count = if simple { 1 } else { 9 };
                let mut levels = Vec::with_capacity(count);
                for _ in 0..count {
                    match read_lvl(table, at) {
                        Some((l, next)) => {
                            levels.push(l);
                            at = next;
                        }
                        None => {
                            out.damaged += 1;
                            out.lstf.push(Lstf {
                                lsid,
                                tplc,
                                simple,
                                levels,
                            });
                            break 'lists;
                        }
                    }
                }
                out.lstf.push(Lstf {
                    lsid,
                    tplc,
                    simple,
                    levels,
                });
            }
        }
        let (fc, lcb) = lfo;
        if lcb >= 4 && fc.checked_add(lcb).is_some_and(|e| e <= table.len()) {
            let n = rd_u32(table, fc).unwrap_or(0) as usize;
            if n > limits.max_lists || n > lcb / 16 {
                return Err(DocError::Limit("list instance count"));
            }
            let end = fc + lcb;
            let mut heads = Vec::with_capacity(n);
            for i in 0..n {
                let o = fc + 4 + 16 * i;
                let Some(h) = table.get(o..o + 16).filter(|_| o + 16 <= end) else {
                    out.damaged += 1;
                    break;
                };
                heads.push((rd_u32(h, 0).unwrap_or(0), usize::from(h[12])));
            }
            let mut at = fc + 4 + 16 * n;
            for (lsid, clfolvl) in heads {
                let mut levels = Vec::new();
                // LFOData: cp (4 bytes, 0xFFFFFFFF), then the LFOLVLs.
                at = at.saturating_add(4);
                for _ in 0..clfolvl {
                    let Some(h) = table
                        .get(at..at.saturating_add(8))
                        .filter(|_| at + 8 <= end)
                    else {
                        out.damaged += 1;
                        break;
                    };
                    let bits = h[4];
                    at += 8;
                    let mut format = None;
                    if bits & 0x20 != 0 {
                        match read_lvl(table, at) {
                            Some((l, next)) if next <= end => {
                                format = Some(l);
                                at = next;
                            }
                            _ => {
                                out.damaged += 1;
                                break;
                            }
                        }
                    }
                    levels.push(LfoLvl {
                        level: bits & 0x0F,
                        start_at: (bits & 0x10 != 0).then(|| rd_u32(h, 0).unwrap_or(0) as i32),
                        format,
                    });
                }
                out.lfo.push(Lfo { lsid, levels });
            }
        }
        Ok(out)
    }

    /// Companion definitions, one per `LFO` (1-based `list_id`).
    pub fn convert(&self, fonts: &[DocFont], _stats: &mut Stats) -> Vec<ImportedList> {
        // Properties of list levels are reported by the lists, not as notices.
        let mut stats = Stats::default();
        let stats = &mut stats;
        let mut out = Vec::with_capacity(self.lfo.len());
        for (i, lfo) in self.lfo.iter().enumerate() {
            let def = self.lstf.iter().find(|l| l.lsid == lfo.lsid);
            let shared = self
                .lfo
                .iter()
                .enumerate()
                .any(|(j, o)| j != i && o.lsid == lfo.lsid);
            let mut levels = Vec::new();
            if let Some(def) = def {
                for (n, l) in def.levels.iter().enumerate() {
                    let mut lvl = l;
                    let (mut start_over, mut overridden) = (None, false);
                    for o in lfo.levels.iter().filter(|o| usize::from(o.level) == n) {
                        if let Some(f) = &o.format {
                            lvl = f;
                            overridden = true;
                        }
                        if let Some(s) = o.start_at {
                            start_over = Some(s);
                        }
                    }
                    levels.push(convert_level(
                        n as u8, lvl, start_over, overridden, fonts, stats,
                    ));
                }
            }
            out.push(ImportedList {
                list_id: (i + 1) as u32,
                lsid: lfo.lsid,
                template_id: def.map_or(0, |d| d.tplc),
                simple: def.is_some_and(|d| d.simple),
                shared_definition: shared,
                levels,
            });
        }
        out
    }
}

fn convert_level(
    level: u8,
    l: &Lvl,
    start_over: Option<i32>,
    overridden: bool,
    fonts: &[DocFont],
    stats: &mut Stats,
) -> ImportedListLevel {
    let mut text = String::new();
    for &u in &l.text {
        match u {
            0..=8 => {
                text.push('%');
                text.push(char::from(b'1' + u as u8));
            }
            _ => text.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
        }
    }
    // Surrogate pairs of bullets: decode from the raw units.
    let decoded: String = char::decode_utf16(l.text.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect();
    let start = start_over.unwrap_or(l.start);
    let start_u = start.max(0) as u32;
    let kind = match l.nfc {
        255 => ListLevelKind::None,
        23 => ListLevelKind::Bullet(decoded),
        n => ListLevelKind::Number {
            format: NumberFormat::from_nfc(n),
            text,
            start: start_u,
        },
    };
    let mut pd = ParaDelta::default();
    pd.apply_all(&l.papx, stats);
    let para_props = pd.to_props(stats);
    let mut cd = CharDelta::default();
    cd.apply_all(&l.chpx, stats);
    let char_attrs = cd.to_attrs(Default::default(), fonts, stats);
    ImportedListLevel {
        level,
        kind,
        alignment: match l.flags & 3 {
            1 => LevelAlignment::Center,
            2 => LevelAlignment::Right,
            _ => LevelAlignment::Left,
        },
        follow: match l.follow {
            1 => LevelFollow::Space,
            2 => LevelFollow::Nothing,
            _ => LevelFollow::Tab,
        },
        legal: l.flags & 0x04 != 0,
        no_restart: l.flags & 0x08 != 0,
        restart_limit: l.restart_lim,
        para_props,
        char_attrs,
        start_overridden: start_over.is_some(),
        overridden,
    }
}
