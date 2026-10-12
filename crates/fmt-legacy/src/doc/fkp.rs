// SPDX-License-Identifier: AGPL-3.0-or-later
//! `PlcfBteChpx` / `PlcfBtePapx` and the 512-byte formatted disk pages (FKP)
//! they reference ([MS-DOC] 2.9.25, 2.9.78, 2.9.23). Entries are indexed by
//! file position (`fc`) and refer to their grpprl by range, never by copy.

use super::{DocError, rd_u32};
use std::ops::Range;

pub(crate) const PAGE: usize = 512;

/// A character or paragraph property exception covering `fc_start..fc_end`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Fkp {
    pub fc_start: u32,
    pub fc_end: u32,
    /// Paragraph style index (always 0 for character entries).
    pub istd: u16,
    /// grpprl bytes inside the `WordDocument` stream.
    pub grpprl: Range<usize>,
}

/// Reads a `Plcf` of `(fc, pn)` pairs: `(first fc, last fc, page number)`.
pub(crate) fn read_bte(
    table: &[u8],
    fc: usize,
    lcb: usize,
    max_pages: usize,
) -> Result<Vec<(u32, u32, u32)>, DocError> {
    if lcb == 0 {
        return Ok(Vec::new());
    }
    let end =
        fc.checked_add(lcb)
            .filter(|e| *e <= table.len())
            .ok_or(DocError::InvalidStructure(
                "bin table outside the table stream",
            ))?;
    if lcb < 4 {
        return Err(DocError::InvalidStructure("bin table too small"));
    }
    let n = (lcb - 4) / 8;
    if n > max_pages {
        return Err(DocError::Limit("FKP page count"));
    }
    let pn_base = fc + (n + 1) * 4;
    debug_assert!(pn_base + n * 4 <= end);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = rd_u32(table, fc + i * 4).unwrap_or(0);
        let b = rd_u32(table, fc + (i + 1) * 4).unwrap_or(0);
        let pn = rd_u32(table, pn_base + i * 4).unwrap_or(0) & 0x003F_FFFF;
        out.push((a, b, pn));
    }
    Ok(out)
}

/// Collects the entries of all pages; `papx` selects the paragraph layout.
/// Returns the entries sorted by `fc_start` and the number of unusable pages.
pub(crate) fn read_pages(
    word: &[u8],
    bte: &[(u32, u32, u32)],
    papx: bool,
    cap: usize,
) -> (Vec<Fkp>, usize) {
    let mut out = Vec::new();
    let mut bad = 0usize;
    // A page referenced twice is read once: repeated references cannot
    // multiply the number of entries.
    let mut seen = std::collections::BTreeSet::new();
    for &(_, _, pn) in bte {
        if !seen.insert(pn) {
            continue;
        }
        if out.len() >= cap {
            bad += 1;
            break;
        }
        let Some(base) = (pn as usize).checked_mul(PAGE) else {
            bad += 1;
            continue;
        };
        let Some(page) = word.get(base..base + PAGE) else {
            bad += 1;
            continue;
        };
        let crun = page[PAGE - 1] as usize;
        let size = if papx { 13 } else { 1 };
        if crun == 0 || 4 * (crun + 1) + crun * size > PAGE - 1 {
            bad += 1;
            continue;
        }
        let bx_base = 4 * (crun + 1);
        for i in 0..crun {
            let fc_start = rd_u32(page, 4 * i).unwrap_or(0);
            let fc_end = rd_u32(page, 4 * (i + 1)).unwrap_or(0);
            if fc_end <= fc_start {
                continue;
            }
            let b = page[bx_base + i * size] as usize;
            if b == 0 {
                continue;
            }
            let off = b * 2;
            let Some(&cb) = page.get(off) else { continue };
            let (istd, rel) = if papx {
                let (len, start) = if cb == 0 {
                    match page.get(off + 1) {
                        Some(&c) => (c as usize * 2, off + 2),
                        None => continue,
                    }
                } else {
                    (cb as usize * 2 - 1, off + 1)
                };
                let Some(content) = page.get(start..start + len) else {
                    continue;
                };
                if content.len() < 2 {
                    continue;
                }
                (
                    u16::from_le_bytes([content[0], content[1]]),
                    start + 2..start + len,
                )
            } else {
                let start = off + 1;
                if start + cb as usize > PAGE - 1 {
                    continue;
                }
                (0, start..start + cb as usize)
            };
            out.push(Fkp {
                fc_start,
                fc_end,
                istd,
                grpprl: base + rel.start..base + rel.end,
            });
        }
    }
    out.sort_by_key(|e| e.fc_start);
    (out, bad)
}

/// The entry containing `fc`, if any.
pub(crate) fn lookup(entries: &[Fkp], fc: u32) -> Option<&Fkp> {
    let i = entries.partition_point(|e| e.fc_start <= fc);
    i.checked_sub(1)
        .and_then(|i| entries.get(i))
        .filter(|e| fc < e.fc_end)
}

/// First entry starting after `fc`.
pub(crate) fn next_start(entries: &[Fkp], fc: u32) -> Option<u32> {
    let i = entries.partition_point(|e| e.fc_start <= fc);
    entries.get(i).map(|e| e.fc_start)
}
