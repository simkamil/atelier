// SPDX-License-Identifier: AGPL-3.0-or-later
//! Messages of the selection highlight, the pictures and the word and
//! paragraph selection (R7b-1): `SelectionRects`, `ImageBitmap`, `SelectUnit`
//! and their replies. Grouped here so that the envelope module only holds the
//! hooks; layout in `docs/us-203-rich-editor-protocol.md` (section R7b-1).

use super::{
    Command, ErrorCode, MAX_PAGES, MAX_PAGES_PER_RESPONSE, Reader, Response, put_f64, put_u32,
    put_u64,
};

/// Largest number of rectangles in one `SelectionRects` reply.
pub const MAX_SELECTION_RECTS: usize = 4096;
/// Longest side of the bitmap of an `ImageBitmap` reply, in pixels (US-270).
pub const MAX_BITMAP_SIDE: u32 = 1600;

/// Unit of a `SelectUnit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionUnit {
    /// The word around the position (`FlatIndex::word_at`).
    Word = 0,
    /// The paragraph, with the break that ends it.
    Paragraph = 1,
}

/// One rectangle of a selection, in points of the page (the display list's
/// coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub(super) fn parse_command(opcode: u16, r: &mut Reader<'_>) -> Result<Command, ErrorCode> {
    Ok(match opcode {
        super::opcode::SELECTION_RECTS => {
            let (first, count) = (r.u32()?, r.u32()?);
            if count == 0 {
                return Err(ErrorCode::InvalidBody);
            }
            if count as usize > MAX_PAGES_PER_RESPONSE {
                return Err(ErrorCode::TooManyPages);
            }
            if first >= MAX_PAGES {
                return Err(ErrorCode::PageRange);
            }
            Command::SelectionRects { first, count }
        }
        super::opcode::IMAGE_BITMAP => Command::ImageBitmap {
            object_id: r.u64()?,
        },
        super::opcode::SELECT_UNIT => {
            let position = r.u64()?;
            let unit = match r.u8()? {
                0 => SelectionUnit::Word,
                1 => SelectionUnit::Paragraph,
                _ => return Err(ErrorCode::InvalidBody),
            };
            let extend = r.flag()?;
            r.zero(2)?;
            Command::SelectUnit {
                position,
                unit,
                extend,
            }
        }
        super::opcode::MOVE_TEXT => {
            let (start, end, target) = (r.u64()?, r.u64()?, r.u64()?);
            let copy = r.flag()?;
            r.zero(3)?;
            if start > end {
                return Err(ErrorCode::InvalidRange);
            }
            Command::MoveText {
                start,
                end,
                target,
                copy,
            }
        }
        _ => return Err(ErrorCode::UnknownOpcode),
    })
}

pub(super) fn encode_command(command: &Command, b: &mut Vec<u8>) -> Result<u16, ErrorCode> {
    Ok(match command {
        Command::SelectionRects { first, count } => {
            put_u32(b, *first);
            put_u32(b, *count);
            super::opcode::SELECTION_RECTS
        }
        Command::ImageBitmap { object_id } => {
            put_u64(b, *object_id);
            super::opcode::IMAGE_BITMAP
        }
        Command::SelectUnit {
            position,
            unit,
            extend,
        } => {
            put_u64(b, *position);
            b.extend_from_slice(&[*unit as u8, u8::from(*extend), 0, 0]);
            super::opcode::SELECT_UNIT
        }
        Command::MoveText {
            start,
            end,
            target,
            copy,
        } => {
            put_u64(b, *start);
            put_u64(b, *end);
            put_u64(b, *target);
            b.extend_from_slice(&[u8::from(*copy), 0, 0, 0]);
            super::opcode::MOVE_TEXT
        }
        _ => return Err(ErrorCode::Internal),
    })
}

pub(super) fn put_selection_rects(
    out: &mut Vec<u8>,
    revision: u64,
    rects: &[SelectionRect],
) -> Result<(), ErrorCode> {
    if rects.len() > MAX_SELECTION_RECTS {
        return Err(ErrorCode::InvalidBody);
    }
    put_u64(out, revision);
    put_u32(
        out,
        u32::try_from(rects.len()).map_err(|_| ErrorCode::InvalidBody)?,
    );
    for rect in rects {
        put_u32(out, rect.page);
        for v in [rect.x, rect.y, rect.width, rect.height] {
            put_f64(out, v);
        }
    }
    Ok(())
}

pub(super) fn read_selection_rects(r: &mut Reader<'_>) -> Result<Response, ErrorCode> {
    let revision = r.u64()?;
    let count = r.u32()? as usize;
    if count > MAX_SELECTION_RECTS {
        return Err(ErrorCode::InvalidBody);
    }
    let mut rects = Vec::with_capacity(count);
    for _ in 0..count {
        let page = r.u32()?;
        let (x, y, width, height) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
        if page >= MAX_PAGES || width < 0.0 || height < 0.0 {
            return Err(ErrorCode::InvalidBody);
        }
        rects.push(SelectionRect {
            page,
            x,
            y,
            width,
            height,
        });
    }
    Ok(Response::SelectionRects { revision, rects })
}

pub(super) fn put_image_bitmap(
    out: &mut Vec<u8>,
    object_id: u64,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), ErrorCode> {
    check_bitmap(width, height, rgba.len())?;
    put_u64(out, object_id);
    put_u32(out, width);
    put_u32(out, height);
    put_u32(
        out,
        u32::try_from(rgba.len()).map_err(|_| ErrorCode::InvalidBody)?,
    );
    out.extend_from_slice(rgba);
    Ok(())
}

pub(super) fn read_image_bitmap(r: &mut Reader<'_>) -> Result<Response, ErrorCode> {
    let object_id = r.u64()?;
    let (width, height) = (r.u32()?, r.u32()?);
    let len = r.u32()? as usize;
    check_bitmap(width, height, len)?;
    let rgba = r.take(len)?.to_vec();
    Ok(Response::ImageBitmap {
        object_id,
        width,
        height,
        rgba,
    })
}

/// A bitmap has 1 to `MAX_BITMAP_SIDE` pixels on each side and exactly four
/// bytes per pixel.
fn check_bitmap(width: u32, height: u32, len: usize) -> Result<(), ErrorCode> {
    let side = 1..=MAX_BITMAP_SIDE;
    if !side.contains(&width) || !side.contains(&height) {
        return Err(ErrorCode::InvalidBody);
    }
    if len as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(ErrorCode::InvalidBody);
    }
    Ok(())
}
