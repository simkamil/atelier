// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict ATED command handling and session-owned plain-text editor state.

use atelier_text_model::{PlainTextDocument, PlainTextLimits, Selection};
use atelier_unicode::segmentation::{grapheme_boundaries, word_boundaries};

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
    preferred_x: Option<f64>,
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
        if opcode == 15 {
            let mut r = Reader::new(body);
            let page = r.u32()? as usize;
            let x = r.f64()?;
            let y = r.f64()?;
            if !x.is_finite() || !y.is_finite() {
                return Err(EditorError("EDITOR_INVALID_COMMAND"));
            }
            let layout = candidate.current_layout()?;
            let offset = layout
                .hit_test(page, x, y)
                .ok_or(EditorError("EDITOR_INVALID_PAGE"))?;
            let caret = layout.caret(offset);
            let mut out = Writer::with_capacity(48)?;
            out.bytes(b"ATPT");
            out.u16(1);
            out.u16(0);
            out.u64(candidate.revision);
            out.u32(u32_len(offset)?);
            out.u32(u32_len(caret.page)?);
            out.f64(caret.x);
            out.f64(caret.y);
            out.f64(14.0);
            return Ok((candidate, out.finish(48)?));
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
                candidate.preferred_x = None;
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
                candidate.preferred_x = None;
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
                candidate.preferred_x = None;
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
                candidate.preferred_x = None;
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
                candidate.preferred_x = None;
            }
            8 => {
                let mut r = Reader::new(body);
                let direction = r.u8()?;
                let extend = r.flag()?;
                r.reserved_u16()?;
                if direction > 13 {
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
                let offset = collapsed.unwrap_or(current.focus);
                let vertical = matches!(direction, 2 | 3 | 8 | 9);
                let preferred_x = candidate.preferred_x;
                let text = candidate.ensure_document()?.text().to_owned();
                let layout = candidate.current_layout()?;
                let x = preferred_x.unwrap_or_else(|| layout.caret(offset).x);
                let next = match direction {
                    10 => word_move(&text, offset, false),
                    11 => word_move(&text, offset, true),
                    12 | 13 => paragraph_move(&text, offset, direction == 13),
                    _ if vertical => layout.move_offset_at_x(offset, direction, x),
                    _ => layout.move_offset(offset, direction),
                };
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
                candidate.preferred_x = vertical.then_some(x);
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
                candidate.preferred_x = None;
            }
            12 | 13 => {
                let mut r = Reader::new(body);
                let now = r.u64()?;
                let doc = candidate.ensure_document()?;
                let before = doc.text().to_owned();
                let selection = doc.selection();
                if selection.anchor != selection.focus {
                    doc.delete_backward(now).map_err(map_text_error)?;
                } else {
                    let range = word_delete_range(doc.text(), selection.focus, opcode == 13);
                    if range.start != range.end {
                        doc.set_selection(Selection {
                            anchor: range.start,
                            focus: range.end,
                        })
                        .map_err(map_text_error)?;
                        doc.delete_backward(now).map_err(map_text_error)?;
                    }
                }
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
                candidate.preferred_x = None;
            }
            14 => {
                let mut r = Reader::new(body);
                let page = r.u32()? as usize;
                let x = r.f64()?;
                let y = r.f64()?;
                let unit = r.u8()?;
                let extend = r.flag()?;
                r.reserved_u16()?;
                let anchor = r.u32()? as usize;
                let anchor_end = r.u32()? as usize;
                if !x.is_finite() || !y.is_finite() || unit > 2 {
                    return Err(EditorError("EDITOR_INVALID_COMMAND"));
                }
                let offset = candidate
                    .current_layout()?
                    .hit_test(page, x, y)
                    .ok_or(EditorError("EDITOR_INVALID_PAGE"))?;
                let text = candidate.document.as_ref().unwrap().text();
                let selection = if unit == 0 {
                    if extend {
                        if anchor_end != anchor {
                            return Err(EditorError("EDITOR_INVALID_COMMAND"));
                        }
                        validate_grapheme(text, anchor)?;
                        Selection {
                            anchor,
                            focus: offset,
                        }
                    } else {
                        if anchor != 0 || anchor_end != 0 {
                            return Err(EditorError("EDITOR_INVALID_COMMAND"));
                        }
                        Selection::caret(offset)
                    }
                } else {
                    let base = if extend {
                        validate_grapheme(text, anchor)?;
                        validate_grapheme(text, anchor_end)?;
                        if unit != 0 && anchor > anchor_end {
                            return Err(EditorError("EDITOR_INVALID_COMMAND"));
                        }
                        Some(anchor..anchor_end)
                    } else {
                        if anchor != 0 || anchor_end != 0 {
                            return Err(EditorError("EDITOR_INVALID_COMMAND"));
                        }
                        None
                    };
                    let target = if unit == 1 {
                        word_unit(text, offset)
                    } else {
                        paragraph_unit(text, offset)
                    };
                    match base {
                        None => Selection {
                            anchor: target.start,
                            focus: target.end,
                        },
                        Some(base) => extend_unit_selection(base, target),
                    }
                };
                candidate
                    .document
                    .as_mut()
                    .unwrap()
                    .set_selection(selection)
                    .map_err(map_text_error)?;
                candidate.layout = None;
                candidate.preferred_x = None;
            }
            16 => {
                let mut r = Reader::new(body);
                let revision = r.u64()?;
                let now = r.u64()?;
                let anchor = r.u32()? as usize;
                let focus = r.u32()? as usize;
                let target = r.u32()? as usize;
                let kind = r.u8()?;
                if r.u8()? != 0 || r.u8()? != 0 || r.u8()? != 0 || kind > 2 {
                    return Err(EditorError("EDITOR_INVALID_COMMAND"));
                }
                if revision != candidate.revision {
                    return Err(EditorError("EDITOR_STALE_EDIT"));
                }
                let text = r.rest();
                let doc = candidate.ensure_document()?;
                validate_grapheme(doc.text(), target)?;
                let before = doc.text().to_owned();
                if kind == 2 {
                    if anchor != 0 || focus != 0 || text.len() > MAX_TEXT {
                        return Err(EditorError("EDITOR_INVALID_COMMAND"));
                    }
                    let inserted = core::str::from_utf8(text)
                        .map_err(|_| EditorError("EDITOR_INVALID_UTF8"))?;
                    doc.replace_range_isolated(target..target, inserted, now)
                        .map_err(map_text_error)?;
                } else {
                    if !text.is_empty() {
                        return Err(EditorError("EDITOR_INVALID_COMMAND"));
                    }
                    if doc.selection() != (Selection { anchor, focus }) {
                        return Err(EditorError("EDITOR_STALE_EDIT"));
                    }
                    validate_grapheme(doc.text(), anchor)?;
                    validate_grapheme(doc.text(), focus)?;
                    doc.drop_selection(target, kind == 1, now)
                        .map_err(map_text_error)?;
                }
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
                candidate.layout = None;
                candidate.preferred_x = None;
            }
            17 => {
                let mut r = Reader::new(body);
                let revision = r.u64()?;
                let now = r.u64()?;
                let start = r.u32()? as usize;
                let end = r.u32()? as usize;
                if revision != candidate.revision {
                    return Err(EditorError("EDITOR_STALE_EDIT"));
                }
                let text = r.rest();
                if text.len() > MAX_TEXT {
                    return Err(EditorError("EDITOR_TEXT_LIMIT"));
                }
                let inserted =
                    core::str::from_utf8(text).map_err(|_| EditorError("EDITOR_INVALID_UTF8"))?;
                let doc = candidate.ensure_document()?;
                let before = doc.text().to_owned();
                doc.replace_range_isolated(start..end, inserted, now)
                    .map_err(map_text_error)?;
                if doc.text() != before {
                    candidate.bump_revision()?;
                }
                candidate.layout = None;
                candidate.preferred_x = None;
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

fn word_units(text: &str) -> Vec<core::ops::Range<usize>> {
    word_boundaries(text).segments().collect()
}
fn word_unit(text: &str, offset: usize) -> core::ops::Range<usize> {
    let units = word_units(text);
    units
        .iter()
        .find(|r| r.start <= offset && offset < r.end)
        .or_else(|| units.last())
        .cloned()
        .unwrap_or(0..0)
}
fn word_move(text: &str, offset: usize, right: bool) -> usize {
    let units = word_units(text);
    if right {
        let mut index = units.partition_point(|r| r.end <= offset);
        if index >= units.len() {
            return text.len();
        }
        // Always cross at least one UAX #29 segment, even from its interior.
        index += 1;
        while index < units.len() && text[units[index].clone()].chars().all(char::is_whitespace) {
            index += 1;
        }
        units.get(index).map_or(text.len(), |r| r.start)
    } else {
        let mut index = units.partition_point(|r| r.start < offset);
        if index == 0 {
            return 0;
        }
        index -= 1;
        while index > 0 && text[units[index].clone()].chars().all(char::is_whitespace) {
            index -= 1;
        }
        units.get(index).map_or(0, |r| r.start)
    }
}
fn word_delete_range(text: &str, offset: usize, forward: bool) -> core::ops::Range<usize> {
    let next = word_move(text, offset, forward);
    offset.min(next)..offset.max(next)
}
fn paragraph_units(text: &str) -> Vec<core::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' || bytes[i] == b'\n' {
            let end = if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i + 2
            } else {
                i + 1
            };
            out.push(start..end);
            start = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push(start..text.len());
    out
}
fn paragraph_unit(text: &str, offset: usize) -> core::ops::Range<usize> {
    let units = paragraph_units(text);
    units
        .iter()
        .find(|r| r.start <= offset && offset < r.end)
        .or_else(|| units.last())
        .cloned()
        .unwrap_or(0..0)
}
fn paragraph_move(text: &str, offset: usize, down: bool) -> usize {
    let units = paragraph_units(text);
    let index = units
        .iter()
        .position(|r| r.start <= offset && offset < r.end)
        .unwrap_or(units.len().saturating_sub(1));
    let target = if down {
        units.get(index + 1)
    } else if offset > units[index].start {
        units.get(index)
    } else {
        index.checked_sub(1).and_then(|i| units.get(i))
    };
    let Some(target) = target else {
        return offset;
    };
    target.start
}
fn extend_unit_selection(
    base: core::ops::Range<usize>,
    target: core::ops::Range<usize>,
) -> Selection {
    if target.end <= base.start {
        Selection {
            anchor: base.end,
            focus: target.start,
        }
    } else if target.start >= base.end {
        Selection {
            anchor: base.start,
            focus: target.end,
        }
    } else {
        Selection {
            anchor: base.start,
            focus: base.end,
        }
    }
}
fn validate_grapheme(text: &str, offset: usize) -> Result<(), EditorError> {
    grapheme_boundaries(text)
        .is_boundary(offset)
        .map_err(|_| EditorError("EDITOR_INVALID_SELECTION"))?
        .then_some(())
        .ok_or(EditorError("EDITOR_INVALID_SELECTION"))
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
        12 | 13 => Some(8),
        14 => Some(32),
        15 => Some(20),
        16 if (32..=32 + MAX_TEXT).contains(&body.len()) => None,
        17 if (24..=24 + MAX_TEXT).contains(&body.len()) => None,
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
    if opcode == 14
        && body.len() == 32
        && (body[20] > 2 || body[21] > 1 || body[22] != 0 || body[23] != 0)
    {
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

#[cfg(test)]
mod navigation_tests {
    use super::extend_unit_selection;

    #[test]
    fn selecting_a_unit_starting_at_base_end_extends_forward() {
        assert_eq!(
            extend_unit_selection(0..3, 3..9),
            atelier_text_model::Selection {
                anchor: 0,
                focus: 9
            }
        );
    }
}
