// SPDX-License-Identifier: AGPL-3.0-or-later
//! Session of the rich editor (ATED v2, US-217).
//!
//! Several documents (T0, `docs/us-327-multidoc-protocol.md`): each one a
//! `Session` holding a `RichDocument` edited in place through `FlatEdit` and
//! its `PageCache`, driven by the commands of
//! `docs/us-203-rich-editor-protocol.md`. A command names its document by the
//! envelope's document id and touches that session only. Nothing is cloned per
//! command; a refused command leaves the session unchanged (every check runs
//! before the first mutation, and the model rolls back a failed edit itself).
//!
//! Replies carry the pages that changed (`rich_protocol::split_page_delta`),
//! never the whole document. Every edit, move and selection command also runs
//! `DEFAULT_LAYOUT_BUDGET` paragraphs of pending layout.

use atelier_fmt_ooxml::scan_revisions;
use atelier_text_model::plain_io::{LineEndingStyle, OpenOptions, open_text};
use atelier_text_model::rich::flat::{FlatEdit, FlatIndex, LayoutContext};
use atelier_text_model::rich::{RichDocument, Theme, default_rich_document};

use crate::protocol::{Message, MessageType};
use crate::rich_layout::{LayoutError, PageCache, PageDelta, RichPageCache};
use crate::rich_preview::import_docx_for_editor;
use crate::rich_protocol::{
    self as proto, AtlasDelta, Caret, Command, DEFAULT_LAYOUT_BUDGET, DecodeError, DeleteUnit,
    Direction, ErrorCode, HitMode, OpenKind, PageDisplay, Profile, Reply, Response, State,
    TextWindowData,
};

mod accessible;
mod find;
#[cfg(test)]
mod find_tests;
mod fragment;
#[cfg(test)]
mod fragment_tests;
// Formatting (F0): `format_state` is written; the three others are the
// extension modules of US-240, US-243 and US-250.
mod format_char;
#[cfg(test)]
mod format_char_tests;
mod format_para;
#[cfg(test)]
mod format_para_tests;
mod format_state;
#[cfg(test)]
mod format_tests;
mod io;
mod sel;
mod styles;
use styles::layout_doc;
#[cfg(test)]
mod styles_tests;
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

/// Documents an engine instance keeps open at once (`RICH_TOO_MANY_DOCUMENTS`).
pub const MAX_DOCUMENTS: usize = 8;
/// Budget of the estimated memory of all documents, in bytes (`RICH_MEMORY`).
/// The estimate is a proxy, not a measurement: see `Session::weight`.
pub const MEMORY_BUDGET: usize = 1 << 30;
/// Estimated engine bytes per byte of source (or of flat text), whichever is larger.
const WEIGHT_FACTOR: usize = 8;
/// Estimated fixed cost of a document (layout caches, glyph atlas, index).
const WEIGHT_BASE: usize = 2 << 20;
/// Estimated engine bytes per byte of a picture or of a pasted fragment.
const WEIGHT_PER_PAYLOAD_BYTE: usize = 4;

/// Bounds of one engine instance (tests lower them).
#[derive(Clone, Copy)]
struct Limits {
    max_documents: usize,
    memory_budget: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_documents: MAX_DOCUMENTS,
            memory_budget: MEMORY_BUDGET,
        }
    }
}

/// The rich editor of one engine instance: the open documents, by id.
#[derive(Default)]
pub struct RichEditor {
    sessions: Vec<Session>,
    /// Highest atlas epoch of a session that was closed or replaced (a new
    /// `Open` never reuses an epoch of its document id).
    last_epoch: u32,
    /// Font faces given by `LoadFonts` to any document: a document opened
    /// later receives them at `Open` (the faces themselves are interned for
    /// the life of the engine, `format_char::intern_fonts`).
    faces: Vec<&'static [u8]>,
    /// Document id of the last request answered: the document whose reply is
    /// lost when the host calls `mark_resync`.
    last_doc: Option<u32>,
    limits: Limits,
}

struct Session {
    id: u32,
    doc: RichDocument,
    cache: PageCache,
    /// Model revision at `Open`; revisions on the wire count from there.
    base: u64,
    /// Revision (relative) of the last `MarkSaved`.
    saved: u64,
    /// History position at the last `MarkSaved` (or at `Open`): the document is
    /// clean when undo and redo bring the history back to it.
    saved_point: (u64, u64),
    anchor: usize,
    focus: usize,
    upstream: bool,
    /// X kept across vertical moves.
    preferred_x: Option<f64>,
    /// Span of the unit gesture in progress (double or triple click and its
    /// drag, R7b-1); any other selection change forgets it.
    unit_base: Option<(usize, usize)>,
    txt: bool,
    /// Where the document comes from, read-only state, pictures (US-218).
    origin: io::Origin,
    /// Page changes found by a layout step outside a `State` (while a move
    /// waits for the layout to reach its target); the next `State` reports them.
    pending: PageDelta,
    /// Document counts of `Statistics` (without lines and pages), by revision.
    stats_memo: Option<(
        (u64, usize, usize),
        atelier_text_model::statistics::Statistics,
    )>,
    /// A reply was lost (encoding failure, ABI limit): the next `State`
    /// invalidates every page and carries the text window.
    resync: bool,
    /// Epoch of the glyph atlas of `cache`: changes at `Open`, at a resync and
    /// whenever the cache is rebuilt, i.e. whenever atlas indices restart at 0.
    epoch: u32,
    /// Attributes of the next typed text (US-240).
    typing: format_char::Typing,
    /// Style preview in progress (US-250).
    preview: styles::Preview,
    /// Built-in styles instantiated into the catalog since the open (US-250); the save writes those the document uses.
    added_styles: Vec<atelier_text_model::rich::Style>,
    /// `command_id` of the last formatting command accepted (`begin_format`).
    last_command_id: u32,
    /// `ListFonts` answer and the revision it was computed at.
    fonts_memo: Option<(u64, crate::rich_protocol::FontListData)>,
    /// Font faces added to `cache` (`LoadFonts`, or the engine's at `Open`):
    /// a rebuilt cache receives them again.
    fonts: Vec<&'static [u8]>,
    /// Estimated engine memory of the document, in bytes (`RICH_MEMORY`): the
    /// larger of the source and the flat text times `WEIGHT_FACTOR`, a fixed
    /// base, and the payload of pictures and pasted fragments.
    weight: usize,
}

/// Paragraphs of layout a move, a hit test or a caret may run to reach its
/// target, in slices of `REACH_SLICE`. Measured in release at about 0.17 ms
/// per paragraph (3 000 and 12 000 paragraph documents): 32 paragraphs plus
/// the 16 of the closing `finish` stay near 8 ms per interactive command. What
/// is not reached is finished by the client's `Layout` pump.
const REACH_BUDGET: u32 = 32;
/// Paragraphs per layout step while reaching a target.
const REACH_SLICE: u32 = 16;

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

    /// Id of the document opened last that is still open (the application of
    /// one document opens its id again), `None` when none is open.
    pub fn active_document(&self) -> Option<u32> {
        self.sessions.last().map(|s| s.id)
    }

    /// Ids of the open documents, in the order they were opened.
    pub fn document_ids(&self) -> Vec<u32> {
        self.sessions.iter().map(|s| s.id).collect()
    }

    /// The only document of a single-document test.
    #[cfg(test)]
    fn session(&self) -> &Session {
        self.sessions.last().expect("a document is open")
    }

    #[cfg(test)]
    fn session_mut(&mut self) -> &mut Session {
        self.sessions.last_mut().expect("a document is open")
    }

    /// Replaces every document by `session` (tests that build a document with items).
    #[cfg(test)]
    fn install(&mut self, session: Session) {
        self.sessions.clear();
        self.sessions.push(session);
    }

    /// An engine with other bounds (tests).
    #[cfg(test)]
    fn with_limits(max_documents: usize, memory_budget: usize) -> Self {
        Self {
            limits: Limits {
                max_documents,
                memory_budget,
            },
            ..Self::default()
        }
    }

    /// Estimated memory of every open document.
    fn total_weight(&self) -> usize {
        self.sessions.iter().map(|s| s.weight).sum()
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
        self.last_doc = Some(request.doc_id);
        let mut reply = Reply {
            doc_id: request.doc_id,
            request_id: request.request_id,
            response,
        };
        match proto::encode_reply(&reply) {
            Ok(frame) => Ok(frame),
            Err(code) => {
                // The reply is lost: the client resynchronises on the next State.
                self.mark_resync();
                reply.response = Response::Error(code);
                proto::encode_reply(&reply)
            }
        }
    }

    /// Records that the last reply never reached the client (the host calls it
    /// when the ABI refuses a response, `STATUS_LIMIT`, `EDITOR_RESPONSE_LIMIT`):
    /// the document of that reply resynchronises, the others are untouched.
    pub fn mark_resync(&mut self) {
        let doc = self.last_doc;
        if let Some(session) = self.sessions.iter_mut().find(|s| Some(s.id) == doc) {
            session.mark_resync();
        }
    }

    fn execute(&mut self, id: u32, command: Command) -> Result<Response, ErrorCode> {
        let slot = self.sessions.iter().position(|s| s.id == id);
        if let Command::Open {
            kind,
            profile,
            data,
        } = command
        {
            return self.open(id, slot, kind, profile, &data);
        }
        let Some(slot) = slot else {
            return Err(ErrorCode::UnknownDocument);
        };
        if matches!(command, Command::Close) {
            let session = self.sessions.remove(slot);
            self.last_epoch = self.last_epoch.max(session.epoch);
            return Ok(Response::Closed);
        }
        // Payload that stays in memory for the life of the document.
        let extra = match &command {
            Command::InsertImage { bytes, .. } | Command::PasteFragment { bytes } => {
                bytes.len().saturating_mul(WEIGHT_PER_PAYLOAD_BYTE)
            }
            _ => 0,
        };
        if extra > 0 && self.total_weight().saturating_add(extra) > self.limits.memory_budget {
            return Err(ErrorCode::Memory);
        }
        let loaded = match &command {
            Command::LoadFonts(files) => Some(files.clone()),
            _ => None,
        };
        let response = self.sessions[slot].run(command)?;
        let session = &mut self.sessions[slot];
        session.weight = session.weight.saturating_add(extra);
        if let Some(files) = loaded {
            // The session accepted them: they are interned, so this only
            // finds them again. Every later document receives them at `Open`.
            if let Some(faces) = format_char::intern_fonts(files) {
                for face in faces {
                    if !session.fonts.contains(&face) {
                        session.fonts.push(face);
                    }
                    if !self.faces.contains(&face) {
                        self.faces.push(face);
                    }
                }
            }
        }
        Ok(response)
    }

    /// `Open`: builds the new session first, so that a refusal (limit, memory,
    /// unreadable source) leaves every open document as it was. An id already
    /// open is replaced (the application of one document opens id 1 again).
    fn open(
        &mut self,
        id: u32,
        slot: Option<usize>,
        kind: OpenKind,
        profile: Profile,
        data: &[u8],
    ) -> Result<Response, ErrorCode> {
        if slot.is_none() && self.sessions.len() >= self.limits.max_documents {
            return Err(ErrorCode::TooManyDocuments);
        }
        let others: usize = self
            .sessions
            .iter()
            .filter(|s| s.id != id)
            .map(|s| s.weight)
            .sum();
        let budget = self.limits.memory_budget;
        // Refused on the source alone, before it is parsed.
        if others.saturating_add(Session::estimate(data.len(), 0)) > budget {
            return Err(ErrorCode::Memory);
        }
        let mut session = Session::open(id, kind, profile, data)?;
        session.weight = Session::estimate(data.len(), session.doc.len());
        if others.saturating_add(session.weight) > budget {
            return Err(ErrorCode::Memory);
        }
        let replaced = slot.map_or(0, |i| self.sessions[i].epoch);
        session.epoch = self.last_epoch.max(replaced).wrapping_add(1);
        session.give_fonts(&self.faces);
        let state = session.open_state()?;
        self.last_epoch = self.last_epoch.max(session.epoch);
        match slot {
            Some(i) => self.sessions[i] = session,
            None => self.sessions.push(session),
        }
        Ok(Response::State(state))
    }
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
        let (doc, origin) = match kind {
            OpenKind::Blank if txt => (RichDocument::new(), io::Origin::new(io::Source::New)),
            OpenKind::Blank => {
                let default = default_rich_document();
                let mut doc = default.document;
                doc.set_styles(default.styles.catalog().clone(), Theme::default());
                (doc, io::Origin::new(io::Source::New))
            }
            OpenKind::Txt => {
                let opened = open_text(data, &OpenOptions::default()).map_err(failed)?;
                let doc = opened
                    .to_rich_document(MAX_TXT_PARAGRAPHS)
                    .map_err(failed)?;
                // Several kinds of break: each paragraph keeps its own.
                let eols = if opened.line_endings.style() == LineEndingStyle::Mixed {
                    let breaks = io::line_breaks(data, &opened);
                    doc.paragraph_ids().into_iter().zip(breaks).collect()
                } else {
                    Default::default()
                };
                let origin = io::Origin::new(io::Source::Text(io::TextOrigin {
                    format: opened.format(),
                    eols,
                    replaced_on_open: opened.invalid_sequences > 0,
                }));
                (doc, origin)
            }
            OpenKind::Docx => {
                let mut imported = import_docx_for_editor(data).map_err(failed)?;
                let report = scan_revisions(&imported.package);
                // The model moves into the session; the import keeps the
                // package and the baseline the writer compares with.
                let marked = report.has_revisions() || report.has_comments();
                let mut doc = if marked {
                    // The import keeps the whole model (the source of a save
                    // of the read-only session); the session shows a copy
                    // without the marks of deleted text, moves and comments.
                    let mut shown = imported.document.clone();
                    io::show_accepted(&mut shown).map_err(failed)?;
                    shown
                } else {
                    std::mem::take(&mut imported.document)
                };
                doc.set_styles(imported.styles.catalog().clone(), imported.theme.clone());
                let mut origin = io::Origin::new(io::Source::Docx(Box::new(imported)));
                // Revisions and comments: everything accepted is shown, edits
                // are refused until the copy made by `MakeEditableCopy` is opened.
                origin.has_revisions = marked;
                origin.unreadable = report.unreadable > 0;
                origin.read_only = marked;
                (doc, origin)
            }
        };
        Self::with_document(id, doc, txt, origin)
    }

    fn with_document(
        id: u32,
        doc: RichDocument,
        txt: bool,
        origin: io::Origin,
    ) -> Result<Self, ErrorCode> {
        let cache = PageCache::new(&doc, txt).map_err(|_| ErrorCode::OpenFailed)?;
        let base = doc.revision();
        let saved_point = doc.history_position();
        Ok(Self {
            id,
            doc,
            cache,
            base,
            saved: 0,
            saved_point,
            anchor: 0,
            focus: 0,
            upstream: false,
            preferred_x: None,
            unit_base: None,
            txt,
            origin,
            pending: PageDelta::default(),
            stats_memo: None,
            resync: false,
            epoch: 0,
            typing: format_char::Typing::default(),
            preview: styles::Preview::default(),
            added_styles: Vec::new(),
            last_command_id: 0,
            fonts_memo: None,
            fonts: Vec::new(),
            weight: 0,
        })
    }

    /// Estimated memory of a document of `source` bytes and `text` flat bytes.
    fn estimate(source: usize, text: usize) -> usize {
        source
            .max(text)
            .saturating_mul(WEIGHT_FACTOR)
            .saturating_add(WEIGHT_BASE)
    }

    /// Gives the cache the font faces the engine already holds. A face the
    /// font manager refuses is left out: the document is laid out without it.
    fn give_fonts(&mut self, faces: &[&'static [u8]]) {
        if faces.is_empty() {
            return;
        }
        if self.cache.add_fonts(layout_doc!(self), faces).is_ok() {
            self.fonts = faces.to_vec();
        }
    }

    /// Restarts the glyph atlas: new epoch, no entry, no painted page.
    /// The client may have missed a reply: the next `State` invalidates every
    /// page, and a new atlas epoch starts (atlas and painted pages emptied).
    fn mark_resync(&mut self) {
        self.resync = true;
        self.new_atlas_epoch();
    }

    fn new_atlas_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.cache.reset_atlas();
    }

    /// Adds the changes of a layout step done outside a `State`.
    fn carry(&mut self, more: PageDelta) {
        let mut base = std::mem::take(&mut self.pending);
        merge(&mut base, more);
        self.pending = base;
    }

    /// Replaces a cache whose layout gave up, so that moves do not stay
    /// blocked until the next `Open`.
    fn rebuild_cache(&mut self) {
        let old = self.cache.page_count();
        if let Ok(cache) = PageCache::new(layout_doc!(self), self.txt) {
            self.cache = cache;
            // The faces loaded on demand belong to the cache that is gone.
            if !self.fonts.is_empty() {
                let faces = std::mem::take(&mut self.fonts);
                self.give_fonts(&faces);
            }
            // A fresh cache numbers its atlas from 0.
            self.epoch = self.epoch.wrapping_add(1);
            let count = self.cache.page_count();
            self.carry(PageDelta {
                page_count: count,
                layout_complete: self.cache.layout_complete(),
                changed: (0..count.min(FIRST_PAGES)).collect(),
                invalidated: vec![(0, old.max(count))],
            });
        }
    }

    /// Runs `query` against the layout, laying out the tail in slices of
    /// `REACH_SLICE` paragraphs (`REACH_BUDGET` at most, so a command never
    /// blocks for long) until it answers. `None` when the budget is spent or
    /// the layout cannot answer; the caller then answers without a caret.
    fn ready<T>(
        &mut self,
        query: impl Fn(&PageCache) -> Result<T, LayoutError>,
    ) -> Result<Option<T>, ErrorCode> {
        let (mut spent, mut rebuilt) = (0u32, false);
        loop {
            match query(&self.cache) {
                Ok(value) => return Ok(Some(value)),
                // A position the document accepts but the layout has not
                // reached is not ready, whatever the layout calls it.
                Err(LayoutError::NotReady | LayoutError::PositionOutOfRange) => {}
                Err(other) => return Err(other.into()),
            }
            if !self.cache.layout_complete() {
                if spent >= REACH_BUDGET {
                    return Ok(None);
                }
                let more = self.cache.step(layout_doc!(self), REACH_SLICE);
                self.carry(more);
                spent += REACH_SLICE;
            } else if !rebuilt {
                rebuilt = true;
                self.rebuild_cache();
            } else {
                return Ok(None);
            }
        }
    }

    /// Lays the document out as far as the caret, so that the `State` has one.
    fn ensure_caret(&mut self) {
        let (focus, upstream) = (self.focus, self.upstream);
        let _ = self.ready(|c| c.caret(focus, upstream));
    }

    /// The target of a move was not reached within the budget: nothing moves,
    /// the `State` has no caret and says the layout is not complete.
    fn unreached(&mut self) -> Result<Response, ErrorCode> {
        self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
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
        // A style preview ends before any command that changes the document
        // or the selection (F0 hook, `styles::cancels_preview`).
        if styles::cancels_preview(&command) {
            styles::cancel_preview(self);
        }
        match command {
            // Handled by the caller.
            Command::Open { .. } | Command::Close => Err(ErrorCode::Internal),
            Command::InsertText { text } => {
                self.deny_read_only()?;
                let (start, end) = self.selection_range();
                self.edit(start, end, &text)
            }
            Command::ReplaceRange { start, end, text } => {
                self.deny_read_only()?;
                proto::check_range(self.doc.len() as u64, start, end)?;
                let (start, end) = (proto::to_position(start)?, proto::to_position(end)?);
                self.edit(start, end, &text)
            }
            Command::Delete { forward, unit } => {
                self.deny_read_only()?;
                self.delete(forward, unit)
            }
            Command::Move { direction, extend } => self.move_selection(direction, extend),
            Command::SetSelection {
                anchor,
                focus,
                upstream,
            } => {
                let anchor = self.doc.snap_back(proto::to_position(anchor)?)?;
                let focus = self.doc.snap_back(proto::to_position(focus)?)?;
                self.select(anchor, focus, upstream, None);
                self.ensure_caret();
                self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
            }
            Command::HitTest { page, mode, x, y } => self.hit_test(page, mode, x, y),
            Command::Undo => {
                self.deny_read_only()?;
                let outcome = self.doc.undo();
                self.after_history(outcome)
            }
            Command::Redo => {
                self.deny_read_only()?;
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
                let mut pages: Vec<PageDisplay> = Vec::new();
                let mut total = 0usize;
                for index in first..end {
                    let display = self.cache.page_display(layout_doc!(self), index)?;
                    // Pages past the response bound are asked for again.
                    if !proto::page_fits(pages.len(), total, display.len()) {
                        if pages.is_empty() {
                            return Err(ErrorCode::FrameTooLarge);
                        }
                        break;
                    }
                    total += display.len();
                    pages.push(PageDisplay { index, display });
                }
                Ok(Response::Pages {
                    revision: self.revision(),
                    page_count,
                    atlas: self.take_atlas(),
                    pages,
                })
            }
            Command::MarkSaved { revision } => {
                if revision != self.revision() {
                    return Err(ErrorCode::StaleRevision);
                }
                self.saved = revision;
                self.saved_point = self.doc.history_position();
                self.origin.props.mark_saved();
                Ok(Response::State(self.state(PageDelta::default(), false)?))
            }
            Command::Layout { budget } => self.finish(PageDelta::default(), budget, false),
            // Idempotent: a lost reply is replayed, each answer starts a new
            // epoch whose atlas begins at 0.
            Command::Resync => {
                self.mark_resync();
                self.finish(PageDelta::default(), 1, true)
            }
            Command::Save { format, stamp } => self.save(format, &stamp),
            Command::InsertImage { alt, bytes } => self.insert_image(&alt, &bytes),
            Command::MakeEditableCopy { stamp } => self.editable_copy(&stamp),
            Command::SelectionRects { first, count } => self.selection_rects(first, count),
            Command::ImageBitmap { object_id } => self.image_bitmap(object_id),
            Command::SelectUnit {
                position,
                unit,
                extend,
            } => self.select_unit(position, unit, extend),
            Command::MoveText {
                start,
                end,
                target,
                copy,
            } => {
                self.deny_read_only()?;
                proto::check_range(self.doc.len() as u64, start, end)?;
                let target = proto::to_position(target)?;
                self.doc.check_position(target)?;
                let (start, end) = (proto::to_position(start)?, proto::to_position(end)?);
                self.move_text(start, end, target, copy)
            }
            Command::Search {
                revision,
                flags,
                query,
            } => self.search(revision, flags, &query),
            Command::Statistics => self.statistics(),
            Command::SearchRects {
                revision,
                first_page,
                page_count,
                ranges,
            } => self.search_rects(revision, first_page, page_count, &ranges),
            Command::Replace {
                revision,
                flags,
                start,
                end,
                query,
                replacement,
            } => self.replace_current(revision, flags, (start, end), &query, &replacement),
            Command::ReplaceAll {
                revision,
                flags,
                query,
                replacement,
            } => self.replace_all(revision, flags, &query, &replacement),
            Command::PropertiesRead => self.properties_read(),
            Command::PropertiesApply { changes } => self.properties_apply(changes),
            Command::FormatState => self.format_state(),
            Command::ApplyCharFormat(format) => self.apply_char_format(format),
            Command::ApplyParaFormat(format) => self.apply_para_format(format),
            Command::ListStyles { filter, used_only } => self.list_styles(filter, used_only),
            Command::ApplyStyle {
                base_revision,
                command_id,
                target,
                id,
            } => self.apply_style(base_revision, command_id, target, &id),
            Command::PreviewStyle { target, show, id } => self.preview_style(target, show, &id),
            Command::ListFonts => self.list_fonts(),
            Command::LoadFonts(fonts) => self.load_fonts(fonts),
            Command::FormatMarks { first, count } => self.format_marks(first, count),
            Command::CopyFragment {
                revision,
                start,
                end,
            } => self.copy_fragment(revision, start, end),
            Command::PasteFragment { bytes } => self.paste_fragment(&bytes),
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
        self.unit_base = None;
        self.typing.forget();
    }

    /// Replaces `start..end` and lays the change out.
    fn edit(&mut self, start: usize, end: usize, text: &str) -> Result<Response, ErrorCode> {
        let before = self.doc.revision();
        let outcome = self.doc.replace_range(start, end, text)?;
        // F0 hook of US-240: typing attributes join the insertion.
        let outcome = format_char::after_text_edit(self, start, text.len(), outcome);
        let delta = if outcome.revision == before {
            PageDelta::default()
        } else {
            self.cache.apply_edit(layout_doc!(self), &outcome)
        };
        // An edit that changed nothing (a separator between a paragraph and a
        // table, a cell or an opaque block cannot be deleted) leaves the
        // selection where it was.
        if outcome.revision != before {
            let (anchor, focus) = outcome.selection;
            self.select(self.snap(anchor), self.snap(focus), false, None);
        }
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    /// Moves or copies `start..end` to `target` as one edit; the moved text
    /// ends up selected.
    fn move_text(
        &mut self,
        start: usize,
        end: usize,
        target: usize,
        copy: bool,
    ) -> Result<Response, ErrorCode> {
        let before = self.doc.revision();
        let outcome = self.doc.move_range(start, end, target, copy)?;
        let delta = if outcome.revision == before {
            PageDelta::default()
        } else {
            self.cache.apply_edit(layout_doc!(self), &outcome)
        };
        if outcome.revision != before {
            let (anchor, focus) = outcome.selection;
            self.select(self.snap(anchor), self.snap(focus), false, None);
        }
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    fn after_history(
        &mut self,
        outcome: Option<atelier_text_model::rich::flat::EditOutcome>,
    ) -> Result<Response, ErrorCode> {
        let mut delta = PageDelta::default();
        if let Some(outcome) = outcome {
            delta = self.cache.apply_edit(layout_doc!(self), &outcome);
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
                let x0 = self.preferred_x;
                let Some((to, x)) = self.ready(|c| c.move_vertical(focus, lines, x0))? else {
                    return self.unreached();
                };
                keep_x = Some(x);
                to
            }
            PageUp | PageDown => {
                let pages = if direction == PageUp { -1 } else { 1 };
                let x0 = self.preferred_x;
                let Some((to, x)) = self.ready(|c| c.move_page(focus, pages, x0))? else {
                    return self.unreached();
                };
                keep_x = Some(x);
                to
            }
            Home => {
                let up = self.upstream;
                let Some(edges) = self.ready(|c| c.line_edges(focus, up))? else {
                    return self.unreached();
                };
                edges.0
            }
            End => {
                let up = self.upstream;
                let Some(edges) = self.ready(|c| c.line_edges(focus, up))? else {
                    return self.unreached();
                };
                let end = edges.1;
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
        self.ensure_caret();
        self.finish(PageDelta::default(), DEFAULT_LAYOUT_BUDGET, true)
    }

    fn hit_test(
        &mut self,
        page: u32,
        mode: HitMode,
        x: f64,
        y: f64,
    ) -> Result<Response, ErrorCode> {
        let Some((position, upstream)) = self.ready(|c| c.hit_test(page, x, y))? else {
            // A probe has no State to answer with; a page that was painted is
            // always reachable, so this only follows a refused budget.
            return match mode {
                HitMode::Probe => Err(ErrorCode::NotReady),
                _ => self.unreached(),
            };
        };
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
                self.ensure_caret();
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
        let more = self.cache.step(layout_doc!(self), budget);
        merge(&mut delta, more);
        Ok(Response::State(self.state(delta, window)?))
    }

    fn state(&mut self, delta: PageDelta, mut window: bool) -> Result<State, ErrorCode> {
        let page_count = self.cache.page_count();
        let mut carried = std::mem::take(&mut self.pending);
        merge(&mut carried, delta);
        let mut delta = carried;
        delta.page_count = page_count;
        delta.layout_complete = self.cache.layout_complete();
        if std::mem::take(&mut self.resync) {
            if page_count > 0 {
                delta.invalidated.push((0, page_count));
            }
            window = true;
        }
        // A page that cannot be painted yet is reported as invalidated, so
        // that the client asks for it again.
        let (pages, invalidated) = loop {
            let (send, invalidated) = proto::split_page_delta(&delta);
            let mut pages = Vec::with_capacity(send.len());
            let mut failed = Vec::new();
            let mut total = 0usize;
            for index in send {
                // Over the response bound, a page is invalidated like one
                // that cannot be painted yet: the client asks for it again.
                match self.cache.page_display(layout_doc!(self), index) {
                    Ok(display) if proto::page_fits(pages.len(), total, display.len()) => {
                        total += display.len();
                        pages.push(PageDisplay { index, display });
                    }
                    _ => failed.push(index),
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
        let atlas = self.take_atlas();
        Ok(State {
            revision,
            anchor: self.anchor as u64,
            focus: self.focus as u64,
            focus_upstream: self.upstream,
            doc_len: self.doc.len() as u64,
            modified: self.doc.history_position() != self.saved_point || self.origin.props.dirty(),
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            layout_complete: delta.layout_complete,
            page_count,
            caret,
            invalidated,
            atlas,
            pages,
            window: if window {
                self.window_around_focus()
            } else {
                None
            },
            read_only: self.origin.read_only,
            has_revisions: self.origin.has_revisions,
        })
    }

    /// The glyph atlas entries the pages painted so far need and no reply
    /// carried yet.
    fn take_atlas(&mut self) -> AtlasDelta {
        let (first, entries) = self.cache.take_atlas();
        AtlasDelta {
            epoch: self.epoch,
            first,
            entries,
        }
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
