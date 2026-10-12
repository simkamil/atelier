// SPDX-License-Identifier: AGPL-3.0-or-later
//! The summary information stream (OLE property set, [MS-OLEPS]): only the
//! properties this crate exposes are read. Strings are decoded with the
//! stream's code page (`VT_LPSTR`) or as UTF-16 (`VT_LPWSTR`); dates stay
//! FILETIME values.

use super::{DocumentProperties, rd_u16, rd_u32};
use atelier_unicode::{Encoding, decode};

/// FMTID_SummaryInformation {F29F85E0-4FF9-1068-AB91-08002B27B3D9}, file order.
const FMTID_SUMMARY: [u8; 16] = [
    0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9,
];
const MAX_PROPERTIES: usize = 1024;

const VT_LPSTR: u16 = 0x1E;
const VT_LPWSTR: u16 = 0x1F;
const VT_FILETIME: u16 = 0x40;

enum Value {
    Text(String),
    Time(u64),
}

fn code_page_encoding(cp: u16) -> Option<Encoding> {
    Some(match cp {
        1252 => Encoding::Windows1252,
        65001 => Encoding::Utf8,
        28591 => Encoding::Iso8859_1,
        437 => Encoding::Cp437,
        10000 => Encoding::MacRoman,
        _ => return None,
    })
}

fn read_value(set: &[u8], off: usize, encoding: Encoding) -> Option<Value> {
    let ty = rd_u16(set, off)? & 0x0FFF;
    let body = off.checked_add(4)?;
    match ty {
        VT_LPSTR => {
            let len = rd_u32(set, body)? as usize;
            let start = body.checked_add(4)?;
            let bytes = set.get(start..start.checked_add(len)?)?;
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            Some(Value::Text(decode(&bytes[..end], encoding)))
        }
        VT_LPWSTR => {
            let chars = rd_u32(set, body)? as usize;
            let start = body.checked_add(4)?;
            let bytes = set.get(start..start.checked_add(chars.checked_mul(2)?)?)?;
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|u| *u != 0)
                .collect();
            Some(Value::Text(
                char::decode_utf16(units)
                    .map(|r| r.unwrap_or('\u{FFFD}'))
                    .collect(),
            ))
        }
        VT_FILETIME => {
            let lo = u64::from(rd_u32(set, body)?);
            let hi = u64::from(rd_u32(set, body.checked_add(4)?)?);
            Some(Value::Time(hi << 32 | lo))
        }
        _ => None,
    }
}

/// Parses the stream. `Err` describes why nothing could be read; the second
/// element counts properties that were skipped because they were damaged or
/// used an unsupported type or code page.
pub(crate) fn parse(data: &[u8]) -> Result<(DocumentProperties, usize), &'static str> {
    if rd_u16(data, 0) != Some(0xFFFE) {
        return Err("bad byte order mark");
    }
    let sets = rd_u32(data, 24).ok_or("truncated header")? as usize;
    if sets == 0 || sets > 4 {
        return Err("unexpected property set count");
    }
    let mut section = None;
    for i in 0..sets {
        let at = 28 + i * 20;
        let id = data.get(at..at + 16).ok_or("truncated section list")?;
        if id == FMTID_SUMMARY {
            section = Some(rd_u32(data, at + 16).ok_or("truncated section list")? as usize);
            break;
        }
    }
    let base = section.ok_or("no summary information section")?;
    let size = rd_u32(data, base).ok_or("section outside the stream")? as usize;
    let set = data
        .get(base..base.checked_add(size).ok_or("bad section size")?)
        .ok_or("section outside the stream")?;
    let count = rd_u32(set, 4).ok_or("truncated section")? as usize;
    if count > MAX_PROPERTIES || 8 + count * 8 > set.len() {
        return Err("bad property count");
    }
    let entry = |i: usize| -> Option<(u32, usize)> {
        Some((rd_u32(set, 8 + i * 8)?, rd_u32(set, 12 + i * 8)? as usize))
    };
    let mut encoding = Encoding::Windows1252;
    let mut skipped = 0usize;
    for i in 0..count {
        if let Some((1, off)) = entry(i) {
            if rd_u16(set, off) == Some(2) {
                match rd_u16(set, off + 4).and_then(code_page_encoding) {
                    Some(e) => encoding = e,
                    None => skipped += 1,
                }
            }
        }
    }
    let mut props = DocumentProperties::default();
    for i in 0..count {
        let Some((id, off)) = entry(i) else { continue };
        if !matches!(id, 2..=6 | 8 | 11..=13 | 18) {
            continue;
        }
        match read_value(set, off, encoding) {
            Some(Value::Text(t)) => {
                let slot = match id {
                    2 => &mut props.title,
                    3 => &mut props.subject,
                    4 => &mut props.author,
                    5 => &mut props.keywords,
                    6 => &mut props.comments,
                    8 => &mut props.last_author,
                    18 => &mut props.application,
                    _ => continue,
                };
                *slot = Some(t).filter(|s| !s.is_empty());
            }
            Some(Value::Time(t)) => {
                let slot = match id {
                    11 => &mut props.last_printed,
                    12 => &mut props.created,
                    13 => &mut props.modified,
                    _ => continue,
                };
                *slot = Some(t).filter(|t| *t != 0);
            }
            None => skipped += 1,
        }
    }
    Ok((props, skipped))
}
