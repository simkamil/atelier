// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages of the rich fragment (T0, `docs/us-327-multidoc-protocol.md`):
//! `CopyFragment`, `PasteFragment` and the `Fragment` reply. The fragment
//! itself is an opaque byte string for the codec; its format is
//! `rich_editor/fragment.rs`.

use super::{Command, ErrorCode, MAX_TEXT_BYTES, Reader, Response, put_text, put_u32, put_u64};

/// Largest serialised fragment, copied or pasted.
pub const MAX_FRAGMENT_BYTES: usize = 16 * 1024 * 1024;
/// Largest plain text of a fragment (the text the system clipboard receives).
pub const MAX_FRAGMENT_TEXT_BYTES: usize = MAX_TEXT_BYTES;

pub(super) fn parse_command(opcode: u16, r: &mut Reader<'_>) -> Result<Command, ErrorCode> {
    Ok(match opcode {
        super::opcode::COPY_FRAGMENT => {
            let (revision, start, end) = (r.u64()?, r.u64()?, r.u64()?);
            if start > end {
                return Err(ErrorCode::InvalidRange);
            }
            Command::CopyFragment {
                revision,
                start,
                end,
            }
        }
        super::opcode::PASTE_FRAGMENT => {
            let len = r.u32()? as usize;
            if len > MAX_FRAGMENT_BYTES {
                return Err(ErrorCode::Limit);
            }
            Command::PasteFragment {
                bytes: r.take(len)?.to_vec(),
            }
        }
        _ => return Err(ErrorCode::UnknownOpcode),
    })
}

pub(super) fn encode_command(command: &Command, b: &mut Vec<u8>) -> Result<u16, ErrorCode> {
    Ok(match command {
        Command::CopyFragment {
            revision,
            start,
            end,
        } => {
            put_u64(b, *revision);
            put_u64(b, *start);
            put_u64(b, *end);
            super::opcode::COPY_FRAGMENT
        }
        Command::PasteFragment { bytes } => {
            if bytes.len() > MAX_FRAGMENT_BYTES {
                return Err(ErrorCode::Limit);
            }
            put_u32(b, bytes.len() as u32);
            b.extend_from_slice(bytes);
            super::opcode::PASTE_FRAGMENT
        }
        _ => return Err(ErrorCode::Internal),
    })
}

pub(super) fn put_fragment(
    out: &mut Vec<u8>,
    revision: u64,
    lossy: bool,
    text: &str,
    bytes: &[u8],
) -> Result<(), ErrorCode> {
    if text.len() > MAX_FRAGMENT_TEXT_BYTES || bytes.len() > MAX_FRAGMENT_BYTES {
        return Err(ErrorCode::Limit);
    }
    put_u64(out, revision);
    put_u32(out, u32::from(lossy));
    put_text(out, text)?;
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn read_fragment(r: &mut Reader<'_>) -> Result<Response, ErrorCode> {
    let revision = r.u64()?;
    let flags = r.u32()?;
    if flags > 1 {
        return Err(ErrorCode::InvalidBody);
    }
    let text = r.text(MAX_FRAGMENT_TEXT_BYTES, ErrorCode::Limit)?;
    let len = r.u32()? as usize;
    if len > MAX_FRAGMENT_BYTES {
        return Err(ErrorCode::Limit);
    }
    Ok(Response::Fragment {
        revision,
        lossy: flags == 1,
        text,
        bytes: r.take(len)?.to_vec(),
    })
}
