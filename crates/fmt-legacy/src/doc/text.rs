// SPDX-License-Identifier: AGPL-3.0-or-later
//! Piece table (`Clx` -> `Prc*` + `Pcdt` -> `PlcPcd`, [MS-DOC] 2.9.38) and
//! decoding of compressed (8 bit) and Unicode (UTF-16LE) pieces.

use super::{DocError, rd_u16, rd_u32};
use std::ops::Range;

/// Windows-1252 as used by compressed pieces: 0x80..=0x9F follow the code
/// page, the five undefined positions keep their Latin-1 code point.
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];

/// Maps one byte of a compressed piece to a UTF-16 code unit.
pub(crate) fn compressed_unit(b: u8) -> u16 {
    if (0x80..0xA0).contains(&b) {
        CP1252_HIGH[(b - 0x80) as usize]
    } else {
        u16::from(b)
    }
}

/// One `Pcd`: characters `cp_start..cp_end` stored at `offset` in `WordDocument`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Piece {
    pub cp_start: usize,
    pub cp_end: usize,
    /// Byte offset of the first character in the `WordDocument` stream.
    pub offset: usize,
    pub compressed: bool,
    /// Raw `Prm` word.
    pub prm: u16,
}

impl Piece {
    pub fn width(&self) -> usize {
        if self.compressed { 1 } else { 2 }
    }
    /// File offset (`fc`) of the character at `cp` (must lie in this piece).
    pub fn fc_of(&self, cp: usize) -> usize {
        self.offset
            .saturating_add((cp - self.cp_start).saturating_mul(self.width()))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Clx {
    pub pieces: Vec<Piece>,
    /// `Prc` grpprl ranges inside the table stream, indexed by `igrpprl`.
    pub prcs: Vec<Range<usize>>,
}

impl Clx {
    /// Parses the CLX located at `fc..fc+lcb` of the table stream.
    pub fn parse(table: &[u8], fc: usize, lcb: usize, max_pieces: usize) -> Result<Clx, DocError> {
        let end = fc
            .checked_add(lcb)
            .filter(|e| *e <= table.len())
            .ok_or(DocError::InvalidStructure("Clx outside the table stream"))?;
        if lcb == 0 {
            return Err(DocError::InvalidStructure("empty Clx"));
        }
        let mut pos = fc;
        let mut prcs = Vec::new();
        loop {
            let t = *table
                .get(pos)
                .filter(|_| pos < end)
                .ok_or(DocError::InvalidStructure("Clx without Pcdt"))?;
            match t {
                1 => {
                    let cb = rd_u16(table, pos + 1)
                        .ok_or(DocError::InvalidStructure("truncated Prc"))?
                        as i16;
                    if cb < 0 {
                        return Err(DocError::InvalidStructure("negative Prc size"));
                    }
                    let start = pos + 3;
                    let stop = start + cb as usize;
                    if stop > end {
                        return Err(DocError::InvalidStructure("Prc beyond Clx"));
                    }
                    if prcs.len() >= max_pieces {
                        return Err(DocError::Limit("Prc count"));
                    }
                    prcs.push(start..stop);
                    pos = stop;
                }
                2 => {
                    let size = rd_u32(table, pos + 1)
                        .ok_or(DocError::InvalidStructure("truncated Pcdt"))?
                        as usize;
                    let start = pos + 5;
                    if start.checked_add(size).is_none_or(|e| e > end) {
                        return Err(DocError::InvalidStructure("Pcdt beyond Clx"));
                    }
                    let pieces = parse_plc_pcd(&table[start..start + size], max_pieces)?;
                    return Ok(Clx { pieces, prcs });
                }
                _ => return Err(DocError::InvalidStructure("unknown Clx entry type")),
            }
        }
    }

    /// Index of the piece holding `cp`, if any.
    pub fn piece_index(&self, cp: usize) -> Option<usize> {
        let i = self.pieces.partition_point(|p| p.cp_end <= cp);
        self.pieces.get(i).filter(|p| p.cp_start <= cp).map(|_| i)
    }
}

fn parse_plc_pcd(plc: &[u8], max_pieces: usize) -> Result<Vec<Piece>, DocError> {
    if plc.len() < 4 {
        return Err(DocError::InvalidStructure("PlcPcd too small"));
    }
    let n = (plc.len() - 4) / 12;
    if n > max_pieces {
        return Err(DocError::Limit("piece count"));
    }
    let cp_at = |i: usize| rd_u32(plc, i * 4).unwrap_or(0) as usize;
    let pcd_base = (n + 1) * 4;
    let mut pieces = Vec::with_capacity(n);
    let mut previous = cp_at(0);
    for i in 0..n {
        let (a, b) = (previous, cp_at(i + 1));
        if b < a {
            return Err(DocError::InvalidStructure("piece positions not increasing"));
        }
        previous = b;
        let o = pcd_base + i * 8;
        let raw = rd_u32(plc, o + 2).unwrap_or(0);
        let prm = rd_u16(plc, o + 6).unwrap_or(0);
        let compressed = raw & 0x4000_0000 != 0;
        let fc = (raw & 0x3FFF_FFFF) as usize;
        let offset = if compressed { fc / 2 } else { fc };
        if b > a {
            pieces.push(Piece {
                cp_start: a,
                cp_end: b,
                offset,
                compressed,
                prm,
            });
        }
    }
    Ok(pieces)
}

/// Decodes `cp_a..cp_b` of piece `p` into UTF-16 units. Returns the number of
/// units produced, which is smaller than requested when the piece runs past
/// the end of the stream.
pub(crate) fn decode_piece(
    word: &[u8],
    p: &Piece,
    cp_a: usize,
    cp_b: usize,
    out: &mut Vec<u16>,
) -> usize {
    let start = p.fc_of(cp_a);
    let wanted = cp_b - cp_a;
    let avail = word.len().saturating_sub(start) / p.width();
    let n = wanted.min(avail);
    if n == 0 {
        return 0;
    }
    if p.compressed {
        out.extend(word[start..start + n].iter().map(|&b| compressed_unit(b)));
    } else {
        out.extend(
            word[start..start + 2 * n]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])),
        );
    }
    n
}
