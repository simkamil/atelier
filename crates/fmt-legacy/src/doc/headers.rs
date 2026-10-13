// SPDX-License-Identifier: AGPL-3.0-or-later
//! Header and footer stories ([MS-DOC] 2.8.14 `PlcfHdd`): the story of the
//! header document holds, after six note separator stories, six stories per
//! section (even header, odd header, even footer, odd footer, first header,
//! first footer). A non-empty story ends with one extra paragraph mark that is
//! not content; an empty story (zero length) is inherited from the previous
//! section.

use super::{DocLimits, rd_u32};
use atelier_text_model::rich::{HfKind, HfVariant};

/// Stories before the first section's (note separators).
const SEPARATOR_STORIES: usize = 6;

/// Slot order inside a section, with its kind and variant.
pub(crate) const SLOTS: [(HfKind, HfVariant); 6] = [
    (HfKind::Header, HfVariant::Even),
    (HfKind::Header, HfVariant::Default),
    (HfKind::Footer, HfVariant::Even),
    (HfKind::Footer, HfVariant::Default),
    (HfKind::Header, HfVariant::First),
    (HfKind::Footer, HfVariant::First),
];

/// Character positions of the stories, relative to the start of the header
/// document.
#[derive(Clone, Debug, Default)]
pub(crate) struct Hdd {
    cps: Vec<usize>,
}

impl Hdd {
    pub fn parse(table: &[u8], fc: usize, lcb: usize, limits: &DocLimits) -> Option<Hdd> {
        let data = fc.checked_add(lcb).and_then(|e| table.get(fc..e))?;
        let n = data.len() / 4;
        if n == 0 || n > SEPARATOR_STORIES + 6 * limits.max_sections + 2 {
            return None;
        }
        let cps = (0..n)
            .map(|i| rd_u32(data, i * 4).unwrap_or(0) as usize)
            .collect();
        Some(Hdd { cps })
    }

    /// Range (relative to the header document) of `slot` of `section`, when
    /// the table declares it.
    pub fn story(&self, section: usize, slot: usize) -> Option<(usize, usize)> {
        let i = SEPARATOR_STORIES
            .checked_add(section.checked_mul(6)?)?
            .checked_add(slot)?;
        let a = *self.cps.get(i)?;
        let b = *self.cps.get(i + 1)?;
        (b >= a).then_some((a, b))
    }
}
