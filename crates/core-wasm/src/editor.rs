// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict ATED command handling and session-owned plain-text editor state.

use atelier_text_model::{PlainTextDocument, PlainTextLimits, Selection};

use crate::editor_layout::Layout;
use crate::protocol::{Message, MessageType};

const MAX_TEXT: usize = 128 * 1024;
const MAX_HISTORY: usize = 4 * 1024 * 1024;
const MAX_RESPONSE: usize = 32 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct EditorSession {
    document: Option<PlainTextDocument>,
    revision: u64,
    saved_bytes: Vec<u8>,
    layout: Option<Layout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorError(pub &'static str);

impl EditorSession {
    pub fn prepare_message(&self, message: &Message) -> (Option<Self>, Message) {
        let response = if message.kind != MessageType::Editor {
            Err(EditorError("EDITOR_INVALID_ENVELOPE"))
        } else {
            self.transact(&message.payload)
        };
        match response {
            Ok((candidate, payload)) => (
                Some(candidate),
                Message {
                    kind: MessageType::EditorResult,
                    request_id: message.request_id,
                    payload,
                },
            ),
            Err(error) => (
                None,
                Message {
                    kind: MessageType::Failed,
                    request_id: message.request_id,
                    payload: error.0.as_bytes().to_vec(),
                },
            ),
        }
    }

    pub fn transact(&self, payload: &[u8]) -> Result<(Self, Vec<u8>), EditorError> {
        let (opcode, body) = parse_command(payload)?;
        let mut candidate = self.clone();
        if opcode == 10 {
            let document = candidate.ensure_document()?;
            let exported = document
                .export_utf8()
                .map_err(|_| EditorError("EDITOR_LIMIT"))?;
            let mut out = Writer::with_capacity(
                16usize
                    .checked_add(exported.len())
                    .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?,
            )?;
            out.bytes(b"ATEX");
            out.u16(1);
            out.u16(0);
            out.u64(candidate.revision);
            out.bytes(&exported);
            return Ok((candidate, out.finish(MAX_RESPONSE)?));
        }
        match opcode {
            0 => {
                candidate.ensure_document()?;
            }
            1 => {
                if body.len() > MAX_TEXT + 3 {
                    return Err(EditorError("EDITOR_TEXT_LIMIT"));
                }
                let doc = PlainTextDocument::from_utf8(body, limits()).map_err(map_text_error)?;
                let loaded_bytes = doc.export_utf8().map_err(|_| EditorError("EDITOR_LIMIT"))?;
                let content_changed = candidate
                    .document
                    .as_ref()
                    .map(|current| current.export_utf8().map(|bytes| bytes != loaded_bytes))
                    .transpose()
                    .map_err(|_| EditorError("EDITOR_LIMIT"))?
                    .unwrap_or(!loaded_bytes.is_empty());
                // A load replaces the document and resets its selection even
                // when its bytes match; a cached layout belongs to the old
                // document instance and must never survive that replacement.
                candidate.layout = None;
                candidate.document = Some(doc);
                if content_changed {
                    candidate.bump_revision()?;
                }
                candidate.saved_bytes = loaded_bytes;
            }
            2 => {
                let mut r = Reader::new(body);
                let now = r.u64()?;
                let text = r.rest();
                let inserted =
                    core::str::from_utf8(text).map_err(|_| EditorError("EDITOR_INVALID_UTF8"))?;
                let doc = candidate.ensure_document()?;
                let before = doc.text().to_owned();
                doc.insert_text(inserted, now).map_err(map_text_error)?;
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
            }
            3 | 4 => {
                let mut r = Reader::new(body);
                let now = r.u64()?;
                let doc = candidate.ensure_document()?;
                let before = doc.text().to_owned();
                if opcode == 3 {
                    doc.delete_backward(now).map_err(map_text_error)?;
                } else {
                    doc.delete_forward(now).map_err(map_text_error)?;
                }
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
            }
            5 | 6 => {
                let doc = candidate.ensure_document()?;
                let before = doc.text().to_owned();
                if opcode == 5 {
                    doc.undo().map_err(map_text_error)?;
                } else {
                    doc.redo().map_err(map_text_error)?;
                }
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
            }
            7 => {
                let mut r = Reader::new(body);
                let anchor = r.u32()? as usize;
                let focus = r.u32()? as usize;
                candidate
                    .ensure_document()?
                    .set_selection(Selection { anchor, focus })
                    .map_err(map_text_error)?;
                candidate.layout = None;
            }
            8 => {
                let mut r = Reader::new(body);
                let direction = r.u8()?;
                let extend = r.flag()?;
                r.reserved_u16()?;
                if direction > 9 {
                    return Err(EditorError("EDITOR_INVALID_COMMAND"));
                }
                let current = candidate.ensure_document()?.selection();
                let collapsed = if !extend && current.anchor != current.focus && direction <= 1 {
                    Some(if direction == 0 {
                        current.anchor.min(current.focus)
                    } else {
                        current.anchor.max(current.focus)
                    })
                } else {
                    None
                };
                let next = candidate
                    .current_layout()?
                    .move_offset(collapsed.unwrap_or(current.focus), direction);
                let next = collapsed.unwrap_or(next);
                candidate
                    .document
                    .as_mut()
                    .unwrap()
                    .set_selection(if extend {
                        Selection {
                            anchor: current.anchor,
                            focus: next,
                        }
                    } else {
                        Selection::caret(next)
                    })
                    .map_err(map_text_error)?;
                candidate.layout = None;
            }
            9 => {
                let mut r = Reader::new(body);
                let page = r.u32()? as usize;
                let x = r.f64()?;
                let y = r.f64()?;
                let extend = r.flag()?;
                if !x.is_finite() || !y.is_finite() {
                    return Err(EditorError("EDITOR_INVALID_COMMAND"));
                }
                let offset = candidate
                    .current_layout()?
                    .hit_test(page, x, y)
                    .ok_or(EditorError("EDITOR_INVALID_PAGE"))?;
                let current = candidate.document.as_ref().unwrap().selection();
                candidate
                    .document
                    .as_mut()
                    .unwrap()
                    .set_selection(if extend {
                        Selection {
                            anchor: current.anchor,
                            focus: offset,
                        }
                    } else {
                        Selection::caret(offset)
                    })
                    .map_err(map_text_error)?;
                candidate.layout = None;
            }
            11 => {
                let mut r = Reader::new(body);
                let exported_revision = r.u64()?;
                if exported_revision != candidate.revision {
                    return Err(EditorError("EDITOR_STALE_SAVE"));
                }
                let exported = candidate
                    .ensure_document()?
                    .export_utf8()
                    .map_err(|_| EditorError("EDITOR_LIMIT"))?;
                candidate.saved_bytes = exported;
            }
            _ => return Err(EditorError("EDITOR_INVALID_COMMAND")),
        }
        let response = candidate.state_response()?;
        Ok((candidate, response))
    }

    fn bump_revision(&mut self) -> Result<(), EditorError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(EditorError("EDITOR_REVISION_LIMIT"))?;
        self.layout = None;
        Ok(())
    }
    fn ensure_document(&mut self) -> Result<&mut PlainTextDocument, EditorError> {
        if self.document.is_none() {
            self.document =
                Some(PlainTextDocument::from_utf8(b"", limits()).map_err(map_text_error)?);
            self.saved_bytes.clear();
            self.layout = None;
        }
        Ok(self.document.as_mut().unwrap())
    }
    fn current_layout(&mut self) -> Result<&Layout, EditorError> {
        self.ensure_document()?;
        if self.layout.is_none() {
            let doc = self.document.as_ref().unwrap();
            self.layout = Some(
                Layout::new(doc.text(), doc.selection().anchor, doc.selection().focus).map_err(
                    |e| match e {
                        crate::editor_layout::LayoutError::TooManyPages => {
                            EditorError("EDITOR_PAGE_LIMIT")
                        }
                        crate::editor_layout::LayoutError::TooLarge => {
                            EditorError("EDITOR_RESPONSE_LIMIT")
                        }
                        _ => EditorError("EDITOR_LAYOUT_FAILED"),
                    },
                )?,
            );
        }
        Ok(self.layout.as_ref().unwrap())
    }
    fn state_response(&mut self) -> Result<Vec<u8>, EditorError> {
        self.ensure_document()?;
        if self.layout.is_none() {
            let doc = self.document.as_ref().unwrap();
            self.layout = Some(
                Layout::new(doc.text(), doc.selection().anchor, doc.selection().focus).map_err(
                    |e| match e {
                        crate::editor_layout::LayoutError::TooManyPages => {
                            EditorError("EDITOR_PAGE_LIMIT")
                        }
                        crate::editor_layout::LayoutError::TooLarge => {
                            EditorError("EDITOR_RESPONSE_LIMIT")
                        }
                        _ => EditorError("EDITOR_LAYOUT_FAILED"),
                    },
                )?,
            );
        }
        let layout = self.layout.as_ref().unwrap();
        let doc = self.document.as_ref().unwrap();
        let selection = doc.selection();
        let text = doc.text().as_bytes();
        let exported = doc.export_utf8().map_err(|_| EditorError("EDITOR_LIMIT"))?;
        let dirty = exported != self.saved_bytes;
        let caret = layout.caret(selection.focus);
        let capacity = layout
            .pages
            .iter()
            .try_fold(
                64usize
                    .checked_add(text.len())
                    .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?,
                |sum, page| {
                    sum.checked_add(4)
                        .and_then(|n| n.checked_add(page.len()))
                        .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))
                },
            )?
            .checked_add(
                layout
                    .selection_rects
                    .len()
                    .checked_mul(36)
                    .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?,
            )
            .ok_or(EditorError("EDITOR_RESPONSE_LIMIT"))?;
        if capacity > MAX_RESPONSE {
            return Err(EditorError("EDITOR_RESPONSE_LIMIT"));
        }
        let mut out = Writer::with_capacity(capacity)?;
        out.bytes(b"ATES");
        out.u16(1);
        let flags = (u16::from(doc.can_undo()))
            | (u16::from(doc.can_redo()) << 1)
            | (u16::from(dirty) << 2)
            | (u16::from(exported.starts_with(&[0xef, 0xbb, 0xbf])) << 3);
        out.u16(flags);
        out.u64(self.revision);
        out.u32(u32_len(selection.anchor)?);
        out.u32(u32_len(selection.focus)?);
        out.u32(u32_len(text.len())?);
        out.u32(u32_len(layout.pages.len())?);
        out.u32(u32_len(caret.page)?);
        out.f64(caret.x);
        out.f64(caret.y);
        out.f64(14.0);
        out.u32(u32_len(layout.selection_rects.len())?);
        out.bytes(text);
        for page in &layout.pages {
            out.u32(u32_len(page.len())?);
            out.bytes(page);
        }
        for &(page, x, y, width, height) in &layout.selection_rects {
            out.u32(u32_len(page)?);
            out.f64(x);
            out.f64(y);
            out.f64(width);
            out.f64(height);
        }
        out.finish(MAX_RESPONSE)
    }
}

fn limits() -> PlainTextLimits {
    PlainTextLimits {
        max_text_bytes: MAX_TEXT,
        max_history_bytes: MAX_HISTORY,
    }
}
fn map_text_error(error: atelier_text_model::PlainTextError) -> EditorError {
    use atelier_text_model::PlainTextError::*;
    match error {
        InvalidUtf8 => EditorError("EDITOR_INVALID_UTF8"),
        TextTooLarge { .. } => EditorError("EDITOR_TEXT_LIMIT"),
        InvalidOffset { .. } | OffsetNotCharBoundary { .. } => {
            EditorError("EDITOR_INVALID_SELECTION")
        }
        HistoryBudgetExceeded { .. } => EditorError("EDITOR_HISTORY_LIMIT"),
        AllocationFailed => EditorError("EDITOR_LIMIT"),
        HistoryFailure => EditorError("EDITOR_HISTORY_FAILED"),
    }
}
fn u32_len(value: usize) -> Result<u32, EditorError> {
    u32::try_from(value).map_err(|_| EditorError("EDITOR_LIMIT"))
}

pub(crate) fn parse_command(payload: &[u8]) -> Result<(u16, &[u8]), EditorError> {
    if payload.len() < 8 || &payload[..4] != b"ATED" {
        return Err(EditorError("EDITOR_INVALID_COMMAND"));
    }
    if u16::from_le_bytes([payload[4], payload[5]]) != 1 {
        return Err(EditorError("EDITOR_UNSUPPORTED_VERSION"));
    }
    let opcode = u16::from_le_bytes([payload[6], payload[7]]);
    let body = &payload[8..];
    let expected = match opcode {
        0 | 5 | 6 | 10 => Some(0),
        1 => None,
        2 => {
            if body.len() >= 8 {
                None
            } else {
                Some(usize::MAX)
            }
        }
        3 | 4 => Some(8),
        7 => Some(8),
        8 => Some(4),
        9 => Some(21),
        11 => Some(8),
        _ => return Err(EditorError("EDITOR_INVALID_COMMAND")),
    };
    if expected.is_some_and(|len| len != body.len()) {
        return Err(EditorError("EDITOR_INVALID_COMMAND"));
    }
    if opcode == 8 && body.len() == 4 && (body[1] > 1 || body[2] != 0 || body[3] != 0) {
        return Err(EditorError("EDITOR_INVALID_COMMAND"));
    }
    if opcode == 9 && body.len() == 21 && body[20] > 1 {
        return Err(EditorError("EDITOR_INVALID_COMMAND"));
    }
    if opcode == 2 && body.len() < 8 {
        return Err(EditorError("EDITOR_INVALID_COMMAND"));
    }
    Ok((opcode, body))
}

struct Writer {
    bytes: Vec<u8>,
}
impl Writer {
    fn with_capacity(capacity: usize) -> Result<Self, EditorError> {
        if capacity > MAX_RESPONSE {
            return Err(EditorError("EDITOR_RESPONSE_LIMIT"));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| EditorError("EDITOR_LIMIT"))?;
        Ok(Self { bytes })
    }
    fn bytes(&mut self, v: &[u8]) {
        self.bytes.extend_from_slice(v);
    }
    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes())
    }
    fn f64(&mut self, v: f64) {
        self.bytes(&v.to_le_bytes())
    }
    fn finish(self, max: usize) -> Result<Vec<u8>, EditorError> {
        if self.bytes.len() > max {
            Err(EditorError("EDITOR_RESPONSE_LIMIT"))
        } else {
            Ok(self.bytes)
        }
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], EditorError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(EditorError("EDITOR_INVALID_COMMAND"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(EditorError("EDITOR_INVALID_COMMAND"))?;
        self.at = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8, EditorError> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, EditorError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(EditorError("EDITOR_INVALID_COMMAND")),
        }
    }
    fn reserved_u16(&mut self) -> Result<(), EditorError> {
        if self.u16()? == 0 {
            Ok(())
        } else {
            Err(EditorError("EDITOR_INVALID_COMMAND"))
        }
    }
    fn u16(&mut self) -> Result<u16, EditorError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, EditorError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, EditorError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, EditorError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }
}
