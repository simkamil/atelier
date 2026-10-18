// SPDX-License-Identifier: AGPL-3.0-or-later
//! Codage commun des réponses ATED : erreur d'éditeur, écrivain d'octets et trame d'aperçu.
//!
//! La session texte brut (éditeur v1) a été retirée en R8 ; ce module ne garde que ce que
//! `staged.rs` et `properties.rs` partagent encore.

const MAX_RESPONSE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorError(pub &'static str);

pub(crate) fn encode_preview(
    preview: crate::rich_preview::RichPreview,
) -> Result<Vec<u8>, EditorError> {
    let response_base = 20usize
        .checked_add(preview.text.len())
        .and_then(|size| match preview.glyph_atlas.as_ref() {
            Some(atlas) => size.checked_add(4)?.checked_add(atlas.len()),
            None => Some(size),
        })
        .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?;
    let size = preview
        .pages
        .iter()
        .try_fold(response_base, |size, page| {
            size.checked_add(4)?.checked_add(page.len())
        })
        .filter(|size| *size <= MAX_RESPONSE)
        .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?;
    let mut out = Writer::with_capacity(size)?;
    out.bytes(b"ATRP");
    out.u16(if preview.glyph_atlas.is_some() { 2 } else { 1 });
    out.u16(0);
    out.u32(u32_len(preview.pages.len())?);
    out.u32(u32_len(preview.text.len())?);
    out.u32(preview.notice_count);
    out.bytes(preview.text.as_bytes());
    if let Some(atlas) = preview.glyph_atlas {
        out.u32(u32_len(atlas.len())?);
        out.bytes(&atlas);
    }
    for page in preview.pages {
        out.u32(u32_len(page.len())?);
        out.bytes(&page);
    }
    out.finish(MAX_RESPONSE)
}

fn u32_len(value: usize) -> Result<u32, EditorError> {
    u32::try_from(value).map_err(|_| EditorError("EDITOR_LIMIT"))
}

pub(super) struct Writer {
    bytes: Vec<u8>,
}
impl Writer {
    pub(super) fn with_capacity(capacity: usize) -> Result<Self, EditorError> {
        if capacity > MAX_RESPONSE {
            return Err(EditorError("EDITOR_RESPONSE_LIMIT"));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| EditorError("EDITOR_LIMIT"))?;
        Ok(Self { bytes })
    }
    pub(super) fn bytes(&mut self, v: &[u8]) {
        self.bytes.extend_from_slice(v);
    }
    pub(super) fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes())
    }
    pub(super) fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes())
    }
    pub(super) fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes())
    }
    pub(super) fn finish(self, max: usize) -> Result<Vec<u8>, EditorError> {
        if self.bytes.len() > max {
            Err(EditorError("EDITOR_RESPONSE_LIMIT"))
        } else {
            Ok(self.bytes)
        }
    }
}
