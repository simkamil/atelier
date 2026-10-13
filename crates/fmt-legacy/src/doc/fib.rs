// SPDX-License-Identifier: AGPL-3.0-or-later
//! File Information Block ([MS-DOC] 2.5): `FibBase`, `FibRgW`, `FibRgLw` and the
//! variable-size `FibRgFcLcb`, read tolerantly (shorter or longer arrays are
//! accepted; missing pairs read as `(0, 0)`).

use super::{DocError, rd_u16, rd_u32};

/// `wIdent` of every Word binary document.
pub const W_IDENT: u16 = 0xA5EC;
/// First `nFib` with the Word 97 layout; lower values are Word 6/95.
pub const N_FIB_WORD97: u16 = 0x00C1;

/// Index of each `(fc, lcb)` pair used by this reader in `FibRgFcLcb97`.
pub(crate) const PAIR_STSHF: usize = 1;
pub(crate) const PAIR_PLCFSED: usize = 6;
pub(crate) const PAIR_PLCFBTE_CHPX: usize = 12;
pub(crate) const PAIR_PLCFBTE_PAPX: usize = 13;
pub(crate) const PAIR_STTBF_FFN: usize = 15;
pub(crate) const PAIR_PLCFHDD: usize = 11;
pub(crate) const PAIR_PLCFFLD_MOM: usize = 16;
pub(crate) const PAIR_PLCFFLD_HDR: usize = 17;
pub(crate) const PAIR_STTBF_BKMK: usize = 21;
pub(crate) const PAIR_PLCF_BKF: usize = 22;
pub(crate) const PAIR_PLCF_BKL: usize = 23;
pub(crate) const PAIR_DOP: usize = 31;
pub(crate) const PAIR_CLX: usize = 33;
pub(crate) const PAIR_PLCF_LST: usize = 73;
pub(crate) const PAIR_PLF_LFO: usize = 74;

/// Upper bound on `cbRgFcLcb` accepted (Word 2007 uses 183).
const MAX_PAIRS: usize = 1024;

/// Parsed FIB fields (see the module documentation of `doc` for the subset).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fib {
    pub n_fib: u16,
    /// `nFibNew` from `FibRgCswNew` when present (Word 2007 stores 0x0112).
    pub n_fib_new: Option<u16>,
    pub lid: u16,
    pub flags: u16,
    pub n_fib_back: u16,
    pub ccp_text: u32,
    pub ccp_ftn: u32,
    pub ccp_hdd: u32,
    pub ccp_atn: u32,
    pub ccp_edn: u32,
    pub ccp_txbx: u32,
    pub ccp_hdr_txbx: u32,
    pairs: Vec<(u32, u32)>,
}

impl Fib {
    pub fn is_dot(&self) -> bool {
        self.flags & 0x0001 != 0
    }
    pub fn is_complex(&self) -> bool {
        self.flags & 0x0004 != 0
    }
    pub fn quick_saves(&self) -> u8 {
        ((self.flags >> 4) & 0x0F) as u8
    }
    pub fn is_encrypted(&self) -> bool {
        self.flags & 0x0100 != 0
    }
    pub fn which_table_is_1(&self) -> bool {
        self.flags & 0x0200 != 0
    }
    pub fn is_obfuscated(&self) -> bool {
        self.flags & 0x8000 != 0
    }
    /// Number of `(fc, lcb)` pairs present in this file.
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
    /// `(fc, lcb)` of pair `index`, or `(0, 0)` if the file does not have it.
    pub fn pair(&self, index: usize) -> (usize, usize) {
        self.pairs
            .get(index)
            .map_or((0, 0), |&(fc, lcb)| (fc as usize, lcb as usize))
    }

    /// Parses the FIB at the start of the `WordDocument` stream. Encryption is
    /// reported before the version so that a protected Word 6 file still gets
    /// the password message.
    pub fn parse(word: &[u8]) -> Result<Fib, DocError> {
        if word.len() < 32 {
            return Err(DocError::InvalidFib("WordDocument shorter than FibBase"));
        }
        if rd_u16(word, 0) != Some(W_IDENT) {
            return Err(DocError::NotWordDocument);
        }
        let n_fib = rd_u16(word, 2).unwrap_or(0);
        let lid = rd_u16(word, 6).unwrap_or(0);
        let flags = rd_u16(word, 10).unwrap_or(0);
        let n_fib_back = rd_u16(word, 12).unwrap_or(0);
        if flags & 0x0100 != 0 {
            return Err(DocError::PasswordProtected {
                obfuscated: flags & 0x8000 != 0,
            });
        }
        if n_fib < N_FIB_WORD97 {
            return Err(DocError::UnsupportedVersion { n_fib });
        }
        // FibRgW
        let csw = rd_u16(word, 32).ok_or(DocError::InvalidFib("missing csw"))? as usize;
        let lw_count_at = 34 + csw * 2;
        let cslw = rd_u16(word, lw_count_at).ok_or(DocError::InvalidFib("missing cslw"))? as usize;
        let lw_at = lw_count_at + 2;
        let lw = |i: usize| -> u32 {
            if i < cslw {
                rd_u32(word, lw_at + i * 4).unwrap_or(0)
            } else {
                0
            }
        };
        let pairs_count_at = lw_at + cslw * 4;
        let declared = rd_u16(word, pairs_count_at).unwrap_or(0) as usize;
        let pairs_at = pairs_count_at + 2;
        let available = word.len().saturating_sub(pairs_at) / 8;
        let count = declared.min(available).min(MAX_PAIRS);
        let mut pairs = Vec::with_capacity(count);
        for i in 0..count {
            let o = pairs_at + i * 8;
            pairs.push((
                rd_u32(word, o).unwrap_or(0),
                rd_u32(word, o + 4).unwrap_or(0),
            ));
        }
        // FibRgCswNew follows the pairs when the file declares it in full.
        let mut n_fib_new = None;
        if count == declared {
            let new_at = pairs_at + declared * 8;
            if rd_u16(word, new_at).unwrap_or(0) > 0 {
                n_fib_new = rd_u16(word, new_at + 2).filter(|v| *v != 0);
            }
        }
        Ok(Fib {
            n_fib,
            n_fib_new,
            lid,
            flags,
            n_fib_back,
            ccp_text: lw(3),
            ccp_ftn: lw(4),
            ccp_hdd: lw(5),
            ccp_atn: lw(7),
            ccp_edn: lw(8),
            ccp_txbx: lw(9),
            ccp_hdr_txbx: lw(10),
            pairs,
        })
    }
}
