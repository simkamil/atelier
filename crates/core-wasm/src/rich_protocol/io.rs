// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages of "open, create and save" (US-218, tranche R3b): `Save`,
//! `InsertImage`, `MakeEditableCopy` and the `SaveData` reply. Grouped here so
//! that the envelope module only holds the hooks; layout in
//! `docs/us-218-open-save.md`.

use super::{
    Command, ErrorCode, HEADER_BYTES, MAX_FRAME_BYTES, Reader, Response, put_u32, put_u64,
};

/// Largest encoded picture accepted by `InsertImage` (US-270).
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Longest alternative text of a picture, in bytes (US-270).
pub const MAX_ALT_BYTES: usize = 512;
/// Most notices in one `SaveData`.
pub const MAX_NOTICES: usize = 64;
/// Longest text of one notice, in bytes.
pub const MAX_NOTICE_BYTES: usize = 255;
/// Largest document `SaveData` carries: what is left of a frame after the
/// header, the fixed fields and the notices at their maximum.
pub const MAX_SAVE_BYTES: usize =
    MAX_FRAME_BYTES - HEADER_BYTES - (8 + 4 + 2) - MAX_NOTICES * (4 + MAX_NOTICE_BYTES);

/// Format of a `Save`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    Txt = 1,
    Docx = 2,
    Docm = 3,
    Dotx = 4,
}

impl SaveFormat {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Txt,
            2 => Self::Docx,
            3 => Self::Docm,
            4 => Self::Dotx,
            _ => return None,
        })
    }
}

/// Something the saved file does not keep faithfully. `code` is stable, `text`
/// is a short English description for logs (the interface localises by code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub code: u16,
    pub text: String,
}

/// Notice codes of `SaveData` (and of the copy made by `MakeEditableCopy`).
pub mod notice {
    /// DOCX writer notices, `0x0100 + index` of `WriteNotice`.
    pub const WRITE_BASE: u16 = 0x0100;
    /// Text save: formatting lost, `0x0200 + index` of `LossCategory`
    /// (the value is the number of items lost).
    pub const LOSS_BASE: u16 = 0x0200;
    /// Text save: characters the encoding cannot hold were replaced by `?`.
    pub const REPLACED_CHARACTERS: u16 = 0x0210;
    /// Text save: several kinds of line break, saved with the dominant one.
    pub const LINE_BREAKS_NORMALISED: u16 = 0x0212;
    /// Text save: malformed sequences had been replaced when opening.
    pub const OPEN_REPLACED: u16 = 0x0213;
    /// Editable copy: tracked changes accepted.
    pub const REVISIONS_ACCEPTED: u16 = 0x0300;
    /// Editable copy: comments removed.
    pub const COMMENTS_REMOVED: u16 = 0x0301;
    /// Editable copy: rows with a tracked deletion removed.
    pub const ROWS_REMOVED: u16 = 0x0302;
    /// Further notices were dropped.
    pub const MORE: u16 = 0x03ff;
}

pub(super) fn parse_command(opcode: u16, r: &mut Reader<'_>) -> Result<Command, ErrorCode> {
    Ok(match opcode {
        super::opcode::SAVE => {
            let format = SaveFormat::from_u8(r.u8()?).ok_or(ErrorCode::InvalidBody)?;
            r.zero(3)?;
            Command::Save { format }
        }
        super::opcode::INSERT_IMAGE => {
            let alt = r.text16()?;
            if alt.len() > MAX_ALT_BYTES || alt.chars().any(|c| c < ' ' || c == '\u{7f}') {
                return Err(ErrorCode::InvalidText);
            }
            let len = r.bytes.len() - r.at;
            if len == 0 {
                return Err(ErrorCode::InvalidBody);
            }
            if len > MAX_IMAGE_BYTES {
                return Err(ErrorCode::Limit);
            }
            let bytes = r.take(len)?.to_vec();
            Command::InsertImage { alt, bytes }
        }
        super::opcode::MAKE_EDITABLE_COPY => Command::MakeEditableCopy,
        _ => return Err(ErrorCode::UnknownOpcode),
    })
}

pub(super) fn encode_command(command: &Command, b: &mut Vec<u8>) -> Result<u16, ErrorCode> {
    Ok(match command {
        Command::Save { format } => {
            b.extend_from_slice(&[*format as u8, 0, 0, 0]);
            super::opcode::SAVE
        }
        Command::InsertImage { alt, bytes } => {
            let len = u16::try_from(alt.len()).map_err(|_| ErrorCode::InvalidText)?;
            b.extend_from_slice(&len.to_le_bytes());
            b.extend_from_slice(alt.as_bytes());
            b.extend_from_slice(bytes);
            super::opcode::INSERT_IMAGE
        }
        Command::MakeEditableCopy => super::opcode::MAKE_EDITABLE_COPY,
        _ => return Err(ErrorCode::Internal),
    })
}

pub(super) fn put_save_data(
    out: &mut Vec<u8>,
    revision: u64,
    bytes: &[u8],
    notices: &[Notice],
) -> Result<(), ErrorCode> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(ErrorCode::Limit);
    }
    if notices.len() > MAX_NOTICES || notices.iter().any(|n| n.text.len() > MAX_NOTICE_BYTES) {
        return Err(ErrorCode::InvalidBody);
    }
    put_u64(out, revision);
    put_u32(
        out,
        u32::try_from(bytes.len()).map_err(|_| ErrorCode::Limit)?,
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(&(notices.len() as u16).to_le_bytes());
    for notice in notices {
        out.extend_from_slice(&notice.code.to_le_bytes());
        out.extend_from_slice(&(notice.text.len() as u16).to_le_bytes());
        out.extend_from_slice(notice.text.as_bytes());
    }
    Ok(())
}

pub(super) fn read_save_data(r: &mut Reader<'_>) -> Result<Response, ErrorCode> {
    let revision = r.u64()?;
    let len = r.u32()? as usize;
    if len > MAX_SAVE_BYTES {
        return Err(ErrorCode::Limit);
    }
    let bytes = r.take(len)?.to_vec();
    let count = usize::from(r.u16()?);
    if count > MAX_NOTICES {
        return Err(ErrorCode::InvalidBody);
    }
    let mut notices = Vec::with_capacity(count);
    for _ in 0..count {
        let code = r.u16()?;
        let text = r.text16()?;
        if text.len() > MAX_NOTICE_BYTES {
            return Err(ErrorCode::InvalidBody);
        }
        notices.push(Notice { code, text });
    }
    Ok(Response::SaveData {
        revision,
        bytes,
        notices,
    })
}
