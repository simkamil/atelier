// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmarks ([MS-DOC] 2.9.286 `SttbfBkmk`, 2.8.4 `PlcfBkf`, 2.8.5 `PlcfBkl`):
//! names, start positions (`PlcfBkf` entries) and end positions (`PlcfBkl`,
//! designated by `BKF.ibkl`). Positions are character positions of the file.

use super::{DocLimits, rd_u16, rd_u32};

/// A bookmark of the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BookmarkDef {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Parses the three structures; the result is empty when any of them is
/// missing. The second value counts unusable entries.
pub(crate) fn parse(
    table: &[u8],
    sttb: (usize, usize),
    bkf: (usize, usize),
    bkl: (usize, usize),
    limits: &DocLimits,
) -> (Vec<BookmarkDef>, usize) {
    let slice = |(fc, lcb): (usize, usize)| fc.checked_add(lcb).and_then(|e| table.get(fc..e));
    let (Some(names), Some(f), Some(l)) = (slice(sttb), slice(bkf), slice(bkl)) else {
        return (Vec::new(), usize::from(sttb.1 + bkf.1 + bkl.1 > 0));
    };
    if names.len() < 6 || f.len() < 4 || l.len() < 4 {
        return (Vec::new(), 0);
    }
    // STTB: fExtend 0xFFFF, cData, cbExtra, then cch + UTF-16 + extra.
    let (extended, count, extra) = (
        rd_u16(names, 0) == Some(0xFFFF),
        usize::from(rd_u16(names, 2).unwrap_or(0)),
        usize::from(rd_u16(names, 4).unwrap_or(0)),
    );
    let n_bkf = (f.len() - 4) / 8;
    let n = count.min(n_bkf);
    if n > limits.max_bookmarks || !extended {
        return (Vec::new(), usize::from(n > 0));
    }
    let mut out = Vec::with_capacity(n);
    let (mut pos, mut damaged) = (6usize, 0usize);
    for i in 0..n {
        let Some(cch) = rd_u16(names, pos).map(usize::from) else {
            damaged += n - i;
            break;
        };
        let end = pos + 2 + cch * 2;
        let Some(raw) = names.get(pos + 2..end) else {
            damaged += n - i;
            break;
        };
        pos = end + extra;
        let name = char::decode_utf16(
            raw.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])),
        )
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect::<String>();
        // BKF i: start in the PlcfBkf, ibkl designates the end.
        let start = rd_u32(f, i * 4).unwrap_or(0) as usize;
        let ibkl = rd_u16(f, (n_bkf + 1) * 4 + i * 4).map_or(usize::MAX, usize::from);
        let end_cp = ibkl
            .checked_mul(4)
            .and_then(|o| rd_u32(l, o))
            .map(|v| v as usize);
        match end_cp {
            Some(end) if !name.is_empty() => out.push(BookmarkDef { name, start, end }),
            _ => damaged += 1,
        }
    }
    (out, damaged)
}
