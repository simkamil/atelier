// SPDX-License-Identifier: AGPL-3.0-or-later
//! Font table `SttbfFfn` ([MS-DOC] 2.9.269, 2.9.108): names, character set,
//! family and alternate name, addressed by the `ftc` index of font sprms.

use super::{DocError, DocFont, rd_u16};

/// Fixed part of an `Ffn` after its length byte: ffid, wWeight, chs, ixchSzAlt,
/// panose (10) and font signature (24).
const FFN_FIXED: usize = 1 + 2 + 1 + 1 + 10 + 24;

fn utf16_until_nul(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// Reads the table at `fc..fc+lcb`. A damaged record ends the list (the fonts
/// read so far stay usable); the returned flag tells whether that happened.
pub(crate) fn parse(
    table: &[u8],
    fc: usize,
    lcb: usize,
    max_fonts: usize,
) -> Result<(Vec<DocFont>, bool), DocError> {
    if lcb == 0 {
        return Ok((Vec::new(), false));
    }
    let end =
        fc.checked_add(lcb)
            .filter(|e| *e <= table.len())
            .ok_or(DocError::InvalidStructure(
                "SttbfFfn outside the table stream",
            ))?;
    let data = &table[fc..end];
    let count = rd_u16(data, 0).ok_or(DocError::InvalidStructure("truncated SttbfFfn"))? as usize;
    if count > max_fonts {
        return Err(DocError::Limit("font count"));
    }
    let mut pos = 4usize; // cData, cbExtra
    let mut fonts = Vec::with_capacity(count.min(data.len() / FFN_FIXED + 1));
    let mut damaged = false;
    for _ in 0..count {
        let Some(&cb) = data.get(pos) else {
            damaged = true;
            break;
        };
        let rec_end = pos + 1 + cb as usize;
        let Some(rec) = data.get(pos + 1..rec_end).filter(|r| r.len() >= FFN_FIXED) else {
            damaged = true;
            break;
        };
        let ffid = rec[0];
        let weight = rd_u16(rec, 1).unwrap_or(400);
        let charset = rec[3];
        let alt_index = rec[4] as usize;
        let names = &rec[FFN_FIXED..];
        let name = utf16_until_nul(names);
        let alternate = (alt_index > 0)
            .then(|| names.get(alt_index * 2..).map(utf16_until_nul))
            .flatten()
            .filter(|s| !s.is_empty());
        fonts.push(DocFont {
            name,
            alternate_name: alternate,
            charset,
            family: (ffid >> 4) & 7,
            pitch: ffid & 3,
            true_type: ffid & 4 != 0,
            weight,
        });
        pos = rec_end;
    }
    Ok((fonts, damaged))
}
