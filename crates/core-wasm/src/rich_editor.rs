// SPDX-License-Identifier: AGPL-3.0-or-later
//! Session of the rich editor (ATED v2, US-217).
//!
//! One active document: a `RichDocument` edited in place through `FlatEdit`
//! and its `PageCache`, driven by the commands of
//! `docs/us-203-rich-editor-protocol.md`. Nothing is cloned per command; a
//! refused command leaves the session unchanged (every check runs before the
//! first mutation, and the model rolls back a failed edit itself).
//!
//! Replies carry the pages that changed (`rich_protocol::split_page_delta`),
//! never the whole document. Every edit, move and selection command also runs
//! `DEFAULT_LAYOUT_BUDGET` paragraphs of pending layout.

use atelier_text_model::plain_io::{OpenOptions, open_text};
use atelier_text_model::rich::flat::{FlatEdit, FlatIndex, LayoutContext};
use atelier_text_model::rich::{RichDocument, Theme, default_rich_document};

use crate::protocol::{Message, MessageType};
use crate::rich_layout::{LayoutError, PageCache, PageDelta, RichPageCache};
use crate::rich_preview::import_preview_docx;
use crate::rich_protocol::{
    self as proto, Caret, Command, DEFAULT_LAYOUT_BUDGET, DecodeError, DeleteUnit, Direction,
    ErrorCode, HitMode, OpenKind, PageDisplay, Profile, Reply, Response, State, TextWindowData,
};

mod accessible;
#[cfg(test)]
mod tests;

/// Bytes of text sent on each side of the focus in a `State` window.
const WINDOW_HALF: usize = 2048;
/// Most paragraphs a TXT file may open with.
const MAX_TXT_PARAGRAPHS: usize = 1 << 20;
/// Pages a `State` or an `Open` reply paints at most.
const FIRST_PAGES: u32 = proto::MAX_PAGES_PER_RESPONSE as u32;

/// True for a payload that the rich editor answers: an ATED frame of at least
/// the 8 bytes of a v1 frame whose version is 2. Shorter frames stay with v1.
pub fn is_rich(payload: &[u8]) -> bool {
    payload.len() >= 8 && payload[..4] == proto::MAGIC && payload[4..6] == [2, 0]
}

/// The rich editor of one engine instance (at most one active document).
#[derive(Default)]
pub struct RichEditor {
    active: Option<Session>,
}

struct Session {
    id: u32,
    doc: RichDocument,
    cache: PageCache,
    /// Model revision at `Open`; revisions on the wire count from there.
    base: u64,
    /// Revision (relative) of the last `MarkSaved`.
    saved: u64,
    anchor: usize,
    focus: usize,
    upstream: bool,
    /// X kept across vertical moves.
    preferred_x: Option<f64>,
}

impl RichEditor {
    /// Answers one `Editor` message whose payload is an ATED v2 frame.
    pub fn respond(&mut self, message: &Message) -> Message {
        let (kind, payload) = match self.transact(&message.payload) {
            Ok(frame) => (MessageType::EditorResult, frame),
            Err(code) => (MessageType::Failed, code.name().as_bytes().to_vec()),
        };
        Message {
            kind,
            request_id: message.request_id,
            payload,
        }
    }

    /// Document id of the active document, if any.
    pub fn active_document(&self) -> Option<u32> {
        self.active.as_ref().map(|s| s.id)
    }

    fn transact(&mut self, payload: &[u8]) -> Result<Vec<u8>, ErrorCode> {
        let request = match proto::decode_request(payload) {
            Ok(request) => request,
            // Document id 0 has no reply to carry it: the host-level failure.
            Err(DecodeError {
                code,
                context: Some((doc_id, request_id)),
            }) if doc_id != 0 => {
                return proto::encode_reply(&Reply {
                    doc_id,
                    request_id,
                    response: Response::Error(code),
                });
            }
            Err(error) => return Err(error.code),
        };
        let response = self
            .execute(request.doc_id, request.command)
            .unwrap_or_else(Response::Error);
        let mut reply = Reply {
            doc_id: request.doc_id,
            request_id: request.request_id,
            response,
        };
        proto::encode_reply(&reply).or_else(|code| {
            reply.response = Response::Error(code);
            proto::encode_reply(&reply)
        })
    }

    fn execute(&mut self, id: u32, command: Command) -> Result<Response, ErrorCode> {
        if let Command::Open {
            kind,
            profile,
            data,
        } = command
        {
            // The previous document survives a refused `Open`.
            let mut session = Session::open(id, kind, profile, &data)?;
            let state = session.open_state()?;
            self.active = Some(session);
            return Ok(Response::State(state));
        }
        let session = match self.active.as_mut() {
            Some(s) if s.id == id => s,
            _ => return Err(ErrorCode::UnknownDocument),
        };
        if matches!(command, Command::Close) {
            self.active = None;
            return Ok(Response::Closed);
        }
        session.run(command)
    }
}

/// A position or text failure of the model or the layout, as a protocol code.
/// A position the document accepted but the layout has not reached is not
/// ready, not out of range.
fn layout<T>(result: Result<T, LayoutError>) -> Result<T, ErrorCode> {
    result.map_err(|e| match e {
        LayoutError::PositionOutOfRange => ErrorCode::NotReady,
        other => other.into(),
    })
}

fn merge(into: &mut PageDelta, more: PageDelta) {
    into.page_count = more.page_count;
    into.layout_complete = more.layout_complete;
    // A page the later step invalidated is not worth sending.
    into.changed
        .retain(|p| !more.invalidated.iter().any(|(a, b)| a <= p && p < b));
    into.changed.extend(more.changed);
    into.changed.sort_unstable();
    into.changed.dedup();
    into.invalidated.extend(more.invalidated);
}

impl Session {
    fn open(id: u32, kind: OpenKind, profile: Profile, data: &[u8]) -> Result<Self, ErrorCode> {
        let txt = profile == Profile::Txt;
        fn failed<E>(_: E) -> ErrorCode {
            ErrorCode::OpenFailed
        }
        let doc = match kind {
            OpenKind::Blank if txt => RichDocument::new(),
            OpenKind::Blank => {
                let default = default_rich_document();
                let mut doc = default.document;
                doc.set_styles(default.styles.catalog().clone(), Theme::default());
                doc
            }
            OpenKind::Txt => open_text(data, &OpenOptions::default())
                .and_then(|opened| opened.to_rich_document(MAX_TXT_PARAGRAPHS))
                .map_err(failed)?,
            OpenKind::Docx => {
                let imported = import_preview_docx(data).map_err(failed)?;
                let mut doc = imported.document;
                doc.set_styles(imported.styles.catalog().clone(), imported.theme);
                doc
            }
        };
        Self::with_document(id, doc, txt)
    }

    fn with_document(id: u32, doc: RichDocument, txt: bool) -> Result<Self, ErrorCode> {
        let cache = PageCache::new(&doc, txt).map_err(|_| ErrorCode::OpenFailed)?;
        let base = doc.revision();
        Ok(Self {
            id,
            doc,
            cache,
            base,
            saved: 0,
            anchor: 0,
            focus: 0,
            upstream: false,
            preferred_x: None,
        })
    }

    fn revision(&self) -> u64 {
        self.doc.revision().saturating_sub(self.base)
    }

    fn selection_range(&self) -> (usize, usize) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }

    fn snap(&self, position: usize) -> usize {
        self.doc
            .snap_back(position.min(self.doc.len()))
            .unwrap_or(0)
    }

    fn run(&mut self, command: Command) -> Result<Response, ErrorCode> {
        match command {
            // Handled by the caller.
            Command::Open { .. } | Command::Close => Err(ErrorCode::Internal),
            Command::InsertText { text } => {
                let (start, end) = self.selection_range();
                self.edit(start, end, &text)
            }
            Command::ReplaceRange { start, end, text } => {
                proto::check_range(self.doc.len() as u64, start, end)?;
                let (start, end) = (proto::to_position(start)?, proto::to_position(end)?);
                self.edit(start, end, &text)
            }
            Command::Delete { forward, unit } => self.delete(forward, unit),
            Command::Move { direction, extend } => self.move_selection(direction, extend),
            Command::SetSelection {
                anchor,
                focus,
                upstream,
            } => {
                let anchor = self.doc.snap_back(proto::to_position(anchor)?)?;
                let focus = self.doc.snap_back(proto::to_position(focus)?)?;
                self.select(anchor, focus, upstream, None);
                self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
            }
            Command::HitTest { page, mode, x, y } => self.hit_test(page, mode, x, y),
            Command::Undo => {
                let outcome = self.doc.undo();
                self.after_history(outcome)
            }
            Command::Redo => {
                let outcome = self.doc.redo();
                self.after_history(outcome)
            }
            Command::TextWindow { start, len } => {
                let start = proto::to_position(start)?;
                self.doc.check_position(start)?;
                let end = start.saturating_add(len as usize).min(self.doc.len());
                let end = self.doc.snap_back(end)?.max(start);
                Ok(Response::TextWindow {
                    revision: self.revision(),
                    start: start as u64,
                    doc_len: self.doc.len() as u64,
                    text: self.doc.text(start, end)?,
                })
            }
            Command::Pages { first, count } => {
                let page_count = self.cache.page_count();
                if first >= page_count {
                    return Err(ErrorCode::PageRange);
                }
                let end = first.saturating_add(count).min(page_count);
                let mut pages = Vec::new();
                for index in first..end {
                    let display = self.cache.page_display(&self.doc, index)?;
                    pages.push(PageDisplay { index, display });
                }
                Ok(Response::Pages {
                    revision: self.revision(),
                    page_count,
                    pages,
                })
            }
            Command::MarkSaved { revision } => {
                if revision != self.revision() {
                    return Err(ErrorCode::StaleRevision);
                }
                self.saved = revision;
                Ok(Response::State(self.state(PageDelta::default(), false)?))
            }
            Command::Layout { budget } => self.finish(PageDelta::default(), budget, false),
            Command::AccessibleWindow { start, end } => Ok(Response::AccessibleWindow {
                revision: self.revision(),
                items: accessible::window(
                    &self.doc,
                    proto::to_position(start)?,
                    proto::to_position(end)?,
                )?,
            }),
        }
    }

    fn select(&mut self, anchor: usize, focus: usize, upstream: bool, preferred_x: Option<f64>) {
        self.anchor = anchor;
        self.focus = focus;
        self.upstream = upstream;
        self.preferred_x = preferred_x;
    }

    /// Replaces `start..end` and lays the change out.
    fn edit(&mut self, start: usize, end: usize, text: &str) -> Result<Response, ErrorCode> {
        let before = self.doc.revision();
        let outcome = self.doc.replace_range(start, end, text)?;
        let delta = if outcome.revision == before {
            PageDelta::default()
        } else {
            self.cache.apply_edit(&self.doc, &outcome)
        };
        let (anchor, focus) = outcome.selection;
        self.select(self.snap(anchor), self.snap(focus), false, None);
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    fn after_history(
        &mut self,
        outcome: Option<atelier_text_model::rich::flat::EditOutcome>,
    ) -> Result<Response, ErrorCode> {
        let mut delta = PageDelta::default();
        if let Some(outcome) = outcome {
            delta = self.cache.apply_edit(&self.doc, &outcome);
            let (anchor, focus) = outcome.selection;
            self.select(self.snap(anchor), self.snap(focus), false, None);
        }
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    fn delete(&mut self, forward: bool, unit: DeleteUnit) -> Result<Response, ErrorCode> {
        let (low, high) = self.selection_range();
        if low != high {
            return self.edit(low, high, "");
        }
        let focus = self.focus;
        let other = match (forward, unit) {
            (false, DeleteUnit::Grapheme) => self.doc.prev_grapheme(focus)?,
            (true, DeleteUnit::Grapheme) => self.doc.next_grapheme(focus)?,
            (false, DeleteUnit::Word) => self.doc.prev_word(focus)?,
            (true, DeleteUnit::Word) => self.doc.next_word(focus)?,
        };
        match other {
            Some(other) => self.edit(focus.min(other), focus.max(other), ""),
            None => self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true),
        }
    }

    fn paragraph_end(&self, paragraph: usize) -> Result<usize, ErrorCode> {
        Ok(if paragraph + 1 < self.doc.paragraph_count() {
            self.doc.paragraph_start(paragraph + 1)? - 1
        } else {
            self.doc.len()
        })
    }

    fn move_selection(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Response, ErrorCode> {
        use Direction::*;
        let (low, high) = self.selection_range();
        let focus = self.focus;
        let collapse = !extend && low != high;
        let (mut keep_x, mut upstream) = (None, false);
        let target = match direction {
            Left if collapse => low,
            Right if collapse => high,
            Left => self.doc.prev_grapheme(focus)?.unwrap_or(focus),
            Right => self.doc.next_grapheme(focus)?.unwrap_or(focus),
            WordLeft => self.doc.prev_word(focus)?.unwrap_or(focus),
            WordRight => self.doc.next_word(focus)?.unwrap_or(focus),
            Up | Down => {
                let lines = if direction == Up { -1 } else { 1 };
                let (to, x) = layout(self.cache.move_vertical(focus, lines, self.preferred_x))?;
                keep_x = Some(x);
                to
            }
            PageUp | PageDown => {
                let pages = if direction == PageUp { -1 } else { 1 };
                let (to, x) = layout(self.cache.move_page(focus, pages, self.preferred_x))?;
                keep_x = Some(x);
                to
            }
            Home => layout(self.cache.line_edges(focus, self.upstream))?.0,
            End => {
                let end = layout(self.cache.line_edges(focus, self.upstream))?.1;
                let paragraph = self.doc.locate(end)?.paragraph;
                // The end of a soft-wrapped line is drawn at that line's end.
                upstream = end < self.paragraph_end(paragraph)?;
                end
            }
            DocumentStart => 0,
            DocumentEnd => self.doc.len(),
            ParagraphUp | ParagraphDown => {
                let at = self.doc.locate(focus)?;
                let down = direction == ParagraphDown;
                if down && at.paragraph + 1 < self.doc.paragraph_count() {
                    self.doc.paragraph_start(at.paragraph + 1)?
                } else if !down && at.offset > 0 {
                    self.doc.paragraph_start(at.paragraph)?
                } else if !down && at.paragraph > 0 {
                    self.doc.paragraph_start(at.paragraph - 1)?
                } else {
                    focus
                }
            }
        };
        let target = self.doc.snap_back(target)?;
        let anchor = if extend { self.anchor } else { target };
        self.select(anchor, target, upstream, keep_x);
        self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
    }

    fn hit_test(
        &mut self,
        page: u32,
        mode: HitMode,
        x: f64,
        y: f64,
    ) -> Result<Response, ErrorCode> {
        let (position, upstream) = layout(self.cache.hit_test(page, x, y))?;
        let position = self.doc.snap_back(position)?;
        match mode {
            HitMode::Probe => Ok(Response::HitResult {
                revision: self.revision(),
                page,
                position: position as u64,
                upstream,
            }),
            HitMode::SetCaret | HitMode::Extend => {
                let anchor = if mode == HitMode::Extend {
                    self.anchor
                } else {
                    position
                };
                self.select(anchor, position, upstream, None);
                self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
            }
        }
    }

    /// First `State` of a document: its first pages.
    fn open_state(&mut self) -> Result<State, ErrorCode> {
        let page_count = self.cache.page_count();
        let mut delta = PageDelta {
            changed: (0..page_count.min(FIRST_PAGES)).collect(),
            ..PageDelta::default()
        };
        if page_count > 0 {
            delta.invalidated.push((0, page_count));
        }
        self.state(delta, true)
    }

    /// Runs `budget` paragraphs of pending layout, then builds the `State`.
    fn finish(
        &mut self,
        mut delta: PageDelta,
        budget: u32,
        window: bool,
    ) -> Result<Response, ErrorCode> {
        let more = self.cache.step(&self.doc, budget);
        merge(&mut delta, more);
        Ok(Response::State(self.state(delta, window)?))
    }

    fn state(&mut self, mut delta: PageDelta, window: bool) -> Result<State, ErrorCode> {
        let page_count = self.cache.page_count();
        delta.page_count = page_count;
        delta.layout_complete = self.cache.layout_complete();
        // A page that cannot be painted yet is reported as invalidated, so
        // that the client asks for it again.
        let (pages, invalidated) = loop {
            let (send, invalidated) = proto::split_page_delta(&delta);
            let mut pages = Vec::with_capacity(send.len());
            let mut failed = Vec::new();
            for index in send {
                match self.cache.page_display(&self.doc, index) {
                    Ok(display) => pages.push(PageDisplay { index, display }),
                    Err(_) => failed.push(index),
                }
            }
            if failed.is_empty() {
                break (pages, invalidated);
            }
            delta.changed.retain(|p| !failed.contains(p));
            delta
                .invalidated
                .extend(failed.iter().map(|p| (*p, *p + 1)));
        };
        let caret = self
            .cache
            .caret(self.focus, self.upstream)
            .ok()
            .filter(|c| c.page < page_count)
            .map(|c| Caret {
                page: c.page,
                x: c.x,
                y: c.y,
                height: c.height,
            });
        let revision = self.revision();
        Ok(State {
            revision,
            anchor: self.anchor as u64,
            focus: self.focus as u64,
            focus_upstream: self.upstream,
            doc_len: self.doc.len() as u64,
            modified: revision != self.saved,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            layout_complete: delta.layout_complete,
            page_count,
            caret,
            invalidated,
            pages,
            window: if window {
                self.window_around_focus()
            } else {
                None
            },
        })
    }

    fn window_around_focus(&self) -> Option<TextWindowData> {
        let start = self
            .doc
            .snap_back(self.focus.saturating_sub(WINDOW_HALF))
            .ok()?;
        let end = self
            .doc
            .snap_back(self.focus.saturating_add(WINDOW_HALF).min(self.doc.len()))
            .ok()?;
        Some(TextWindowData {
            start: start as u64,
            text: self.doc.text(start, end.max(start)).ok()?,
        })
    }
}
