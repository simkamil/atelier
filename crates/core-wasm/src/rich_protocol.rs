// SPDX-License-Identifier: AGPL-3.0-or-later
//! ATED v2 envelope and message codec of the rich editor (US-203).
//!
//! Specification: `docs/us-203-rich-editor-protocol.md`. This module only
//! encodes, decodes and validates frames; it holds no document state and is
//! not routed by `editor.rs` yet (the integrator does that). Positions are
//! flat UTF-8 byte offsets, see the specification. No function panics on any
//! input.

use std::fmt;

pub const MAGIC: [u8; 4] = *b"ATED";
pub const VERSION: u16 = 2;
pub const HEADER_BYTES: usize = 20;
/// Largest complete frame, in either direction.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
/// Largest source accepted by `Open`.
pub const MAX_OPEN_BYTES: usize = MAX_FRAME_BYTES - HEADER_BYTES - 8;
/// Largest text carried by `InsertText` and `ReplaceRange`.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Largest text window, requested or returned.
pub const MAX_WINDOW_BYTES: usize = 256 * 1024;
/// Largest number of page display lists in one response.
pub const MAX_PAGES_PER_RESPONSE: usize = 16;
/// Largest display list of one page.
pub const MAX_PAGE_DISPLAY_BYTES: usize = 16 * 1024 * 1024;
/// Largest sum of the display lists of one response, unless it carries a
/// single page (a page alone may reach `MAX_PAGE_DISPLAY_BYTES`).
pub const MAX_RESPONSE_PAGE_BYTES: usize = 8 * 1024 * 1024;
/// Smallest valid display list (an empty `ATDL` page).
pub const MIN_PAGE_DISPLAY_BYTES: usize = 44;
/// Largest number of glyph atlas entries of a document session (indices are below it).
pub const MAX_ATLAS_ENTRIES: u32 = 4096;
/// Largest display list of one atlas entry.
pub const MAX_ATLAS_ENTRY_BYTES: usize = 64 * 1024;
/// Largest sum of the entries of one response (the whole atlas fits).
pub const MAX_ATLAS_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Largest number of invalidation ranges in one state.
pub const MAX_INVALIDATIONS: usize = 1024;
/// Largest page count and exclusive bound of page indices.
pub const MAX_PAGES: u32 = 1 << 20;
/// Paragraphs of background layout that every edit command also runs.
pub const DEFAULT_LAYOUT_BUDGET: u32 = 16;
/// Largest budget (in paragraphs) of one `Layout` command.
pub const MAX_LAYOUT_BUDGET: u32 = 4096;
/// `Caret::page` value meaning "no caret geometry".
pub const NO_CARET: u32 = u32::MAX;
/// Largest number of paragraphs in one `AccessibleWindow` reply.
pub const MAX_ACCESSIBLE_ITEMS: usize = 256;
/// Roles of an accessible paragraph.
pub mod role {
    pub const PARAGRAPH: u8 = 0;
    pub const HEADING: u8 = 1;
    pub const LIST_ITEM: u8 = 2;
    pub const TABLE_CELL: u8 = 3;
    pub const OPAQUE: u8 = 4;
}

pub mod opcode {
    pub const OPEN: u16 = 0x0101;
    pub const CLOSE: u16 = 0x0102;
    pub const INSERT_TEXT: u16 = 0x0103;
    pub const REPLACE_RANGE: u16 = 0x0104;
    pub const DELETE: u16 = 0x0105;
    pub const MOVE: u16 = 0x0106;
    pub const SET_SELECTION: u16 = 0x0107;
    pub const HIT_TEST: u16 = 0x0108;
    pub const UNDO: u16 = 0x0109;
    pub const REDO: u16 = 0x010A;
    pub const TEXT_WINDOW: u16 = 0x010B;
    pub const PAGES: u16 = 0x010C;
    pub const MARK_SAVED: u16 = 0x010D;
    pub const LAYOUT: u16 = 0x010E;
    pub const ACCESSIBLE_WINDOW: u16 = 0x010F;
    pub const RESYNC: u16 = 0x0118;
    // Open, create and save (US-218, R3b).
    pub const SAVE: u16 = 0x0110;
    pub const INSERT_IMAGE: u16 = 0x0111;
    pub const MAKE_EDITABLE_COPY: u16 = 0x0112;
    // Selection highlight, pictures, word and paragraph selection (R7b-1).
    pub const SELECTION_RECTS: u16 = 0x0120;
    pub const IMAGE_BITMAP: u16 = 0x0121;
    pub const SELECT_UNIT: u16 = 0x0122;
    // Text drag and drop (R7c).
    pub const MOVE_TEXT: u16 = 0x0140;
    // Search, replace, statistics, properties (R7b-2).
    pub const SEARCH: u16 = 0x0130;
    pub const STATISTICS: u16 = 0x0131;
    pub const SEARCH_RECTS: u16 = 0x0132;
    pub const REPLACE: u16 = 0x0133;
    pub const REPLACE_ALL: u16 = 0x0134;
    pub const PROPERTIES_READ: u16 = 0x0135;
    pub const PROPERTIES_APPLY: u16 = 0x0136;
    pub const R_ERROR: u16 = 0x8000;
    // Formatting (F0): characters, paragraphs, styles.
    pub const FORMAT_STATE: u16 = 0x0150;
    pub const APPLY_CHAR_FORMAT: u16 = 0x0151;
    pub const APPLY_PARA_FORMAT: u16 = 0x0152;
    pub const LIST_STYLES: u16 = 0x0153;
    pub const APPLY_STYLE: u16 = 0x0154;
    pub const PREVIEW_STYLE: u16 = 0x0155;
    pub const LIST_FONTS: u16 = 0x0156;
    pub const FORMAT_MARKS: u16 = 0x0157;
    // 0x0158 to 0x016E: later formatting commands (US-241, US-242, US-247, US-248).
    /// Font faces loaded on demand (US-240, T240-F); answered by `State`.
    pub const LOAD_FONTS: u16 = 0x016F;
    // T0 (multi-documents): rich fragment. 0x0170 to 0x017F belong to it; the
    // formatting commands keep 0x0150 to 0x016F, MoveText 0x0140.
    pub const COPY_FRAGMENT: u16 = 0x0170;
    pub const PASTE_FRAGMENT: u16 = 0x0171;
    pub const R_FORMAT_STATE: u16 = 0x8130;
    pub const R_STYLE_LIST: u16 = 0x8131;
    pub const R_FONT_LIST: u16 = 0x8132;
    pub const R_FORMAT_MARKS: u16 = 0x8133;
    // 0x8134 to 0x813F: replies of the later formatting commands.

    /// Reply of `CopyFragment` (0x8140 to 0x814F: fragment replies).
    pub const R_FRAGMENT: u16 = 0x8140;

    pub const R_STATE: u16 = 0x8100;
    pub const R_TEXT_WINDOW: u16 = 0x8101;
    pub const R_PAGES: u16 = 0x8102;
    pub const R_HIT: u16 = 0x8103;
    pub const R_CLOSED: u16 = 0x8104;
    pub const R_ACCESSIBLE_WINDOW: u16 = 0x8105;
    pub const R_SAVE_DATA: u16 = 0x8106;
    pub const R_SELECTION_RECTS: u16 = 0x8110;
    pub const R_IMAGE_BITMAP: u16 = 0x8111;
    pub const R_SEARCH: u16 = 0x8120;
    pub const R_STATISTICS: u16 = 0x8121;
    pub const R_SEARCH_RECTS: u16 = 0x8122;
    pub const R_REPLACED: u16 = 0x8123;
    pub const R_PROPERTIES: u16 = 0x8125;
    pub const R_PROPERTIES_APPLIED: u16 = 0x8126;
}

/// Stable error codes; the numeric values never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    InvalidEnvelope = 1,
    UnsupportedVersion = 2,
    UnknownOpcode = 3,
    InvalidBody = 4,
    InvalidDocumentId = 5,
    UnknownDocument = 6,
    PositionOutOfRange = 7,
    PositionNotBoundary = 8,
    InvalidRange = 9,
    InvalidUtf8 = 10,
    InvalidText = 11,
    FrameTooLarge = 12,
    WindowTooLarge = 13,
    PageRange = 14,
    TooManyPages = 15,
    StaleRevision = 16,
    OpenFailed = 17,
    Internal = 18,
    Limit = 19,
    NotReady = 20,
    /// The session is read-only (revisions or comments, US-218).
    ReadOnly = 21,
    /// The document could not be written.
    SaveFailed = 22,
    /// The picture is not a PNG, JPEG, GIF or BMP, or is corrupt.
    ImageInvalid = 23,
    /// The revisions of the document cannot be accepted faithfully.
    RevisionsUnsupported = 24,
    /// `ImageBitmap` names an object that is not a picture of the document (R7b-1).
    UnknownObject = 30,
    // Search, replace, properties (R7b-2): codes 40 to 49.
    /// A replacement query is empty.
    EmptyQuery = 40,
    /// `Replace` names a range that is not an occurrence, or is not the selection.
    StaleMatch = 41,
    /// A query is longer than 1024 bytes.
    QueryTooLarge = 42,
    /// The properties of the document could not be read or written.
    PropertiesFailed = 43,
    /// `ReplaceAll` would replace more occurrences than one undo step holds.
    TooManyMatches = 44,
    // Text drag and drop (R7c): codes 50 to 59.
    /// `MoveText` names a range with objects, tables or opaque blocks, or a
    /// target in another container.
    MoveUnsupported = 50,
    // Formatting (F0): codes 60 to 69.
    /// The formatting command is defined by the contract but its handler is
    /// not written yet (stub of `format_char.rs`, `format_para.rs`, `styles.rs`).
    FormatNotImplemented = 60,
    /// `ApplyStyle` or `PreviewStyle` names a style that is not in the catalog
    /// (or is of the other kind).
    UnknownStyle = 61,
    /// A formatting command on a plain-text (TXT profile) document, which holds
    /// no formatting.
    FormatPlainText = 62,
    /// A formatting command carries the `command_id` of the last one the
    /// engine accepted: a repeat after a lost reply; nothing changes.
    DuplicateCommand = 63,
    // Several documents and fragment (T0): codes 70 to 79.
    /// `Open` would exceed the number of documents the engine keeps.
    TooManyDocuments = 70,
    /// `Open`, `InsertImage` or `PasteFragment` would exceed the memory budget
    /// of the documents of the engine; the other documents are untouched.
    Memory = 71,
    /// The bytes of `PasteFragment` are not a fragment this engine reads
    /// (magic, version, structure, bounds).
    FragmentInvalid = 72,
    /// `CopyFragment` of an empty range.
    EmptySelection = 73,
}

impl ErrorCode {
    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::InvalidEnvelope,
            2 => Self::UnsupportedVersion,
            3 => Self::UnknownOpcode,
            4 => Self::InvalidBody,
            5 => Self::InvalidDocumentId,
            6 => Self::UnknownDocument,
            7 => Self::PositionOutOfRange,
            8 => Self::PositionNotBoundary,
            9 => Self::InvalidRange,
            10 => Self::InvalidUtf8,
            11 => Self::InvalidText,
            12 => Self::FrameTooLarge,
            13 => Self::WindowTooLarge,
            14 => Self::PageRange,
            15 => Self::TooManyPages,
            16 => Self::StaleRevision,
            17 => Self::OpenFailed,
            18 => Self::Internal,
            19 => Self::Limit,
            20 => Self::NotReady,
            21 => Self::ReadOnly,
            22 => Self::SaveFailed,
            23 => Self::ImageInvalid,
            24 => Self::RevisionsUnsupported,
            30 => Self::UnknownObject,
            40 => Self::EmptyQuery,
            41 => Self::StaleMatch,
            42 => Self::QueryTooLarge,
            43 => Self::PropertiesFailed,
            44 => Self::TooManyMatches,
            50 => Self::MoveUnsupported,
            60 => Self::FormatNotImplemented,
            61 => Self::UnknownStyle,
            62 => Self::FormatPlainText,
            63 => Self::DuplicateCommand,
            70 => Self::TooManyDocuments,
            71 => Self::Memory,
            72 => Self::FragmentInvalid,
            73 => Self::EmptySelection,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::InvalidEnvelope => "RICH_INVALID_ENVELOPE",
            Self::UnsupportedVersion => "RICH_UNSUPPORTED_VERSION",
            Self::UnknownOpcode => "RICH_UNKNOWN_OPCODE",
            Self::InvalidBody => "RICH_INVALID_BODY",
            Self::InvalidDocumentId => "RICH_INVALID_DOCUMENT_ID",
            Self::UnknownDocument => "RICH_UNKNOWN_DOCUMENT",
            Self::PositionOutOfRange => "RICH_POSITION_OUT_OF_RANGE",
            Self::PositionNotBoundary => "RICH_POSITION_NOT_BOUNDARY",
            Self::InvalidRange => "RICH_INVALID_RANGE",
            Self::InvalidUtf8 => "RICH_INVALID_UTF8",
            Self::InvalidText => "RICH_INVALID_TEXT",
            Self::FrameTooLarge => "RICH_FRAME_TOO_LARGE",
            Self::WindowTooLarge => "RICH_WINDOW_TOO_LARGE",
            Self::PageRange => "RICH_PAGE_RANGE",
            Self::TooManyPages => "RICH_TOO_MANY_PAGES",
            Self::StaleRevision => "RICH_STALE_REVISION",
            Self::OpenFailed => "RICH_OPEN_FAILED",
            Self::Internal => "RICH_INTERNAL",
            Self::Limit => "RICH_LIMIT",
            Self::NotReady => "RICH_NOT_READY",
            Self::ReadOnly => "RICH_READ_ONLY",
            Self::SaveFailed => "RICH_SAVE_FAILED",
            Self::ImageInvalid => "RICH_IMAGE_INVALID",
            Self::RevisionsUnsupported => "RICH_REVISIONS_UNSUPPORTED",
            Self::UnknownObject => "RICH_UNKNOWN_OBJECT",
            Self::MoveUnsupported => "RICH_MOVE_UNSUPPORTED",
            Self::EmptyQuery => "RICH_EMPTY_QUERY",
            Self::StaleMatch => "RICH_STALE_MATCH",
            Self::QueryTooLarge => "RICH_QUERY_TOO_LARGE",
            Self::PropertiesFailed => "RICH_PROPERTIES_FAILED",
            Self::TooManyMatches => "RICH_TOO_MANY_MATCHES",
            Self::FormatNotImplemented => "RICH_FORMAT_NOT_IMPLEMENTED",
            Self::UnknownStyle => "RICH_UNKNOWN_STYLE",
            Self::FormatPlainText => "RICH_FORMAT_PLAIN_TEXT",
            Self::DuplicateCommand => "RICH_DUPLICATE_COMMAND",
            Self::TooManyDocuments => "RICH_TOO_MANY_DOCUMENTS",
            Self::Memory => "RICH_MEMORY",
            Self::FragmentInvalid => "RICH_FRAGMENT_INVALID",
            Self::EmptySelection => "RICH_EMPTY_SELECTION",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A decoding failure. `context` is `(document id, request id)` when the
/// header was readable, so the caller can answer with an `Error` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError {
    pub code: ErrorCode,
    pub context: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    Blank = 0,
    Txt = 1,
    Docx = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Document = 0,
    Txt = 1,
}

/// Same numbering as the v1 `Move` direction byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left = 0,
    Right = 1,
    Up = 2,
    Down = 3,
    Home = 4,
    End = 5,
    DocumentStart = 6,
    DocumentEnd = 7,
    PageUp = 8,
    PageDown = 9,
    WordLeft = 10,
    WordRight = 11,
    ParagraphUp = 12,
    ParagraphDown = 13,
}

impl Direction {
    fn from_u8(value: u8) -> Option<Self> {
        use Direction::*;
        const ALL: [Direction; 14] = [
            Left,
            Right,
            Up,
            Down,
            Home,
            End,
            DocumentStart,
            DocumentEnd,
            PageUp,
            PageDown,
            WordLeft,
            WordRight,
            ParagraphUp,
            ParagraphDown,
        ];
        ALL.get(usize::from(value)).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteUnit {
    Grapheme = 0,
    Word = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitMode {
    /// Report the position only; no state change, answered by `HitResult`.
    Probe = 0,
    /// Collapse the selection at the position, answered by `State`.
    SetCaret = 1,
    /// Move the focus to the position, answered by `State`.
    Extend = 2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open {
        kind: OpenKind,
        profile: Profile,
        data: Vec<u8>,
    },
    Close,
    InsertText {
        text: String,
    },
    ReplaceRange {
        start: u64,
        end: u64,
        text: String,
    },
    Delete {
        forward: bool,
        unit: DeleteUnit,
    },
    Move {
        direction: Direction,
        extend: bool,
    },
    SetSelection {
        anchor: u64,
        focus: u64,
        /// Affinity of the focus at a soft line wrap: true draws the caret at
        /// the end of the previous line.
        upstream: bool,
    },
    HitTest {
        page: u32,
        mode: HitMode,
        x: f64,
        y: f64,
    },
    Undo,
    Redo,
    TextWindow {
        start: u64,
        len: u32,
    },
    Pages {
        first: u32,
        count: u32,
    },
    MarkSaved {
        revision: u64,
    },
    /// Runs at most `budget` paragraphs of pending layout; answered by `State`.
    Layout {
        budget: u32,
    },
    /// Paragraphs intersecting the flat range `[start, end)`, for the
    /// accessible DOM mirror; `end - start` is at most `MAX_WINDOW_BYTES`.
    AccessibleWindow {
        start: u64,
        end: u64,
    },
    /// Restarts the atlas epoch and invalidates every page after a reply the
    /// client abandoned; empty body, answered by `State`, idempotent.
    Resync,
    /// Serialises the document; answered by `SaveData`, no state change.
    Save {
        format: SaveFormat,
        stamp: Stamp,
    },
    /// Inserts a picture as an inline object at the caret (replacing the
    /// selection); answered by `State`.
    InsertImage {
        alt: String,
        bytes: Vec<u8>,
    },
    /// An editable copy of a read-only document, revisions accepted and
    /// comments removed, as a DOCX; answered by `SaveData` (the session does
    /// not change, the client opens the bytes).
    MakeEditableCopy {
        stamp: Stamp,
    },
    /// Selection rectangles of the pages `first .. first + count` (clamped to
    /// the page count), for the session's selection; answered by
    /// `SelectionRects`, no state change (R7b-1).
    SelectionRects {
        first: u32,
        count: u32,
    },
    /// The bitmap of a picture (`object_id` is the `DrawImage` identity of the
    /// display lists), EXIF orientation applied; answered by `ImageBitmap`.
    ImageBitmap {
        object_id: u64,
    },
    /// Selects the word or paragraph around `position` (a double or triple
    /// click); `extend` grows the selection by units, answered by `State`.
    SelectUnit {
        position: u64,
        unit: SelectionUnit,
        extend: bool,
    },
    /// Moves (or copies) the flat range `start..end` to `target`, keeping the
    /// character formatting, in one undo step (R7c); answered by `State`.
    MoveText {
        start: u64,
        end: u64,
        target: u64,
        copy: bool,
    },
    /// Occurrences of `query` (R7b-2); answered by `SearchResult`. `revision`
    /// must be the current one (`RICH_STALE_REVISION`).
    Search {
        revision: u64,
        flags: SearchFlags,
        query: String,
    },
    /// Counts of the document and of the selection; answered by `Statistics`.
    Statistics,
    /// Highlight rectangles of flat ranges on pages `first_page ..
    /// first_page + page_count`; answered by `SearchRects`.
    SearchRects {
        revision: u64,
        first_page: u32,
        page_count: u32,
        ranges: Vec<(u64, u64)>,
    },
    /// Replaces the occurrence `start..end` (which must be the selection)
    /// by `replacement`; answered by `Replaced`.
    Replace {
        revision: u64,
        flags: SearchFlags,
        start: u64,
        end: u64,
        query: String,
        replacement: String,
    },
    /// Replaces every occurrence in one undo step; answered by `Replaced`.
    ReplaceAll {
        revision: u64,
        flags: SearchFlags,
        query: String,
        replacement: String,
    },
    /// The document properties; answered by `Properties`.
    PropertiesRead,
    /// Changes properties (`None` leaves a field alone, `Some("")` removes it);
    /// answered by `PropertiesApplied`.
    PropertiesApply {
        changes: [Option<String>; PROPERTY_COUNT],
    },
    /// Formatting of the selection or the caret; answered by `FormatState`.
    FormatState,
    /// Sets or clears character attributes; answered by `State`.
    ApplyCharFormat(CharFormat),
    /// Sets or clears paragraph attributes of every paragraph touched; `State`.
    ApplyParaFormat(ParaFormat),
    /// The styles of the catalog; answered by `StyleList`.
    ListStyles {
        filter: StyleFilter,
        used_only: bool,
    },
    /// Applies a paragraph or character style; answered by `State`.
    ApplyStyle {
        base_revision: u64,
        command_id: u32,
        target: StyleTarget,
        id: String,
    },
    /// Shows (`show`) or cancels the preview of a style, without touching the
    /// document or its history; answered by `State`.
    PreviewStyle {
        target: StyleTarget,
        show: bool,
        id: String,
    },
    /// The fonts the engine draws and those the document names; answered by `FontList`.
    ListFonts,
    /// Font files (TrueType, one face each) the client fetched from the application
    /// resources for a family the document uses (US-240, T240-F); answered by `State`.
    LoadFonts(Vec<Vec<u8>>),
    /// Formatting marks (paragraph ends, spaces, tabs...) of pages
    /// `first..first + count`, drawn over the pages by the client; `FormatMarks`.
    FormatMarks {
        first: u32,
        count: u32,
    },
    /// The rich fragment of the flat range `start..end` (T0); `revision` must
    /// be the current one; answered by `Fragment`, no state change.
    CopyFragment {
        revision: u64,
        start: u64,
        end: u64,
    },
    /// Replaces the selection by a fragment, in one undo step; answered by `State`.
    PasteFragment {
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub doc_id: u32,
    pub request_id: u32,
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDisplay {
    pub index: u32,
    /// An `ATDL` display list (`docs/us-080-display-list.md`).
    pub display: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextWindowData {
    pub start: u64,
    pub text: String,
}

/// Glyph atlas section of a `State` or `Pages` reply: the entries
/// `first .. first + entries.len()` of the atlas of `epoch`, new since the
/// previous reply. Each entry is the `ATDL` display list of one contour (a
/// black `FillPath` at the origin) that `DrawGlyphRun` commands of font 0
/// reference by index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AtlasDelta {
    pub epoch: u32,
    pub first: u32,
    pub entries: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub revision: u64,
    pub anchor: u64,
    pub focus: u64,
    /// Affinity of the focus (bit 5 of the flags).
    pub focus_upstream: bool,
    pub doc_len: u64,
    pub modified: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub layout_complete: bool,
    pub page_count: u32,
    pub caret: Option<Caret>,
    /// Half-open page ranges `[first, end)`, sorted, disjoint, to drop from
    /// the client cache.
    pub invalidated: Vec<(u32, u32)>,
    /// New glyph atlas entries, sent before the pages that use them.
    pub atlas: AtlasDelta,
    /// Re-rendered pages, strictly increasing indices.
    pub pages: Vec<PageDisplay>,
    pub window: Option<TextWindowData>,
    /// The session refuses edits (bit 6 of the flags, US-218).
    pub read_only: bool,
    /// The source had tracked changes or comments (bit 7 of the flags, US-218).
    pub has_revisions: bool,
}

/// Kinds of an inline item (every U+FFFC of an accessible paragraph).
pub mod inline_kind {
    /// Anchored object; the value is its alternative text.
    pub const OBJECT: u8 = 0;
    /// Line break or text wrapping break; empty value.
    pub const LINE_BREAK: u8 = 1;
    /// Page or column break; empty value.
    pub const PAGE_BREAK: u8 = 2;
    /// Symbol; the value is its displayable character.
    pub const SYMBOL: u8 = 3;
}

/// The inline item behind one U+FFFC of an accessible paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibleInline {
    /// Flat offset relative to the paragraph start, on a U+FFFC.
    pub offset: u64,
    pub kind: u8,
    pub value: String,
}

/// One paragraph of an `AccessibleWindow` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibleItem {
    pub para_start: u64,
    pub para_end: u64,
    pub role: u8,
    /// Heading level 1-9 (role heading), list level 0-8 (role list item), else 0.
    pub level: u8,
    /// Computed list label (a number and a dot, a bullet), possibly empty.
    pub label: String,
    /// Flat text of the paragraph (`para_end - para_start` bytes).
    pub text: String,
    /// Exactly one per U+FFFC of `text`, in text order.
    pub inlines: Vec<AccessibleInline>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Error(ErrorCode),
    State(State),
    TextWindow {
        revision: u64,
        start: u64,
        doc_len: u64,
        text: String,
    },
    Pages {
        revision: u64,
        page_count: u32,
        atlas: AtlasDelta,
        pages: Vec<PageDisplay>,
    },
    HitResult {
        revision: u64,
        page: u32,
        position: u64,
        upstream: bool,
    },
    Closed,
    AccessibleWindow {
        revision: u64,
        items: Vec<AccessibleItem>,
    },
    /// The serialised document (US-218).
    SaveData {
        revision: u64,
        bytes: Vec<u8>,
        notices: Vec<Notice>,
    },
    /// Rectangles of the selection on the requested pages (R7b-1).
    SelectionRects {
        revision: u64,
        rects: Vec<SelectionRect>,
    },
    /// Oriented RGBA8 pixels, straight alpha, top-down (R7b-1).
    ImageBitmap {
        object_id: u64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    SearchResult {
        revision: u64,
        /// Occurrences counted, at most `SEARCH_COUNT_CAP`.
        total: u32,
        /// More occurrences than `items` carries.
        truncated: bool,
        /// `total` stopped at the cap.
        total_capped: bool,
        items: Vec<SearchItem>,
    },
    Statistics {
        revision: u64,
        anchor: u64,
        focus: u64,
        /// The line and page counts are final.
        layout_complete: bool,
        document: StatCounts,
        selection: StatCounts,
    },
    SearchRects {
        revision: u64,
        rects: Vec<HighlightRect>,
    },
    Replaced {
        count: u32,
        state: State,
    },
    Properties {
        revision: u64,
        snapshot: Box<PropertiesSnapshot>,
    },
    PropertiesApplied(PropertiesOutcome),
    FormatState(Box<FormatStateData>),
    StyleList {
        revision: u64,
        styles: Vec<StyleEntry>,
    },
    FontList(FontListData),
    FormatMarks {
        revision: u64,
        marks: Vec<FormatMark>,
    },
    /// The fragment of a `CopyFragment`: `text` is the plain text of the range
    /// (what `TextWindow` gives, objects as U+FFFC), `lossy` says that tables or
    /// opaque blocks were flattened or dropped.
    Fragment {
        revision: u64,
        lossy: bool,
        text: String,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub doc_id: u32,
    pub request_id: u32,
    pub response: Response,
}

type Fail = ErrorCode;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Fail> {
        let end = self.at.checked_add(n).ok_or(Fail::InvalidBody)?;
        let slice = self.bytes.get(self.at..end).ok_or(Fail::InvalidBody)?;
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, Fail> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Fail> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| Fail::InvalidBody)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, Fail> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| Fail::InvalidBody)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, Fail> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| Fail::InvalidBody)?,
        ))
    }
    fn f64(&mut self) -> Result<f64, Fail> {
        let value = f64::from_bits(self.u64()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Fail::InvalidBody)
        }
    }
    fn zero(&mut self, n: usize) -> Result<(), Fail> {
        if self.take(n)?.iter().all(|b| *b == 0) {
            Ok(())
        } else {
            Err(Fail::InvalidBody)
        }
    }
    fn flag(&mut self) -> Result<bool, Fail> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Fail::InvalidBody),
        }
    }
    /// A `u16` length followed by that many UTF-8 bytes.
    fn text16(&mut self) -> Result<String, Fail> {
        let len = usize::from(self.u16()?);
        let raw = self.take(len)?;
        Ok(core::str::from_utf8(raw)
            .map_err(|_| Fail::InvalidUtf8)?
            .to_owned())
    }
    fn finish(&self) -> Result<(), Fail> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(Fail::InvalidBody)
        }
    }
    /// A `u32` length followed by that many UTF-8 bytes, at most `max`.
    fn text(&mut self, max: usize, too_big: Fail) -> Result<String, Fail> {
        let len = self.u32()? as usize;
        if len > max {
            return Err(too_big);
        }
        let raw = self.take(len)?;
        let text = core::str::from_utf8(raw).map_err(|_| Fail::InvalidUtf8)?;
        Ok(text.to_owned())
    }
}

/// Text accepted by edit commands: no U+FFFC (objects are inserted by their
/// own commands), no `\r`, no C0 control character except `\n` and `\t`.
/// DEL (U+007F) and the C1 controls (U+0080..U+009F) are accepted.
pub fn validate_edit_text(text: &str) -> Result<(), ErrorCode> {
    if text
        .chars()
        .any(|c| c == '\u{FFFC}' || (c < ' ' && c != '\n' && c != '\t'))
    {
        Err(ErrorCode::InvalidText)
    } else {
        Ok(())
    }
}

/// Checks `start <= end <= doc_len`; scalar and grapheme boundaries need the
/// text and stay with the engine.
pub fn check_range(doc_len: u64, start: u64, end: u64) -> Result<(), ErrorCode> {
    if start > end {
        Err(ErrorCode::InvalidRange)
    } else if end > doc_len {
        Err(ErrorCode::PositionOutOfRange)
    } else {
        Ok(())
    }
}

fn header(opcode: u16, doc_id: u32, request_id: u32, body: &[u8]) -> Result<Vec<u8>, ErrorCode> {
    let total = HEADER_BYTES
        .checked_add(body.len())
        .ok_or(ErrorCode::FrameTooLarge)?;
    if total > MAX_FRAME_BYTES {
        return Err(ErrorCode::FrameTooLarge);
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(&doc_id.to_le_bytes());
    out.extend_from_slice(&request_id.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(body.len())
            .map_err(|_| ErrorCode::FrameTooLarge)?
            .to_le_bytes(),
    );
    out.extend_from_slice(body);
    Ok(out)
}

/// Splits a frame into `(opcode, doc_id, request_id, body)`.
fn split(bytes: &[u8]) -> Result<(u16, u32, u32, &[u8]), DecodeError> {
    let bare = |code| DecodeError {
        code,
        context: None,
    };
    if bytes.len() < 6 || bytes[..4] != MAGIC {
        return Err(bare(ErrorCode::InvalidEnvelope));
    }
    if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
        return Err(bare(ErrorCode::UnsupportedVersion));
    }
    if bytes.len() < HEADER_BYTES {
        return Err(bare(ErrorCode::InvalidEnvelope));
    }
    let word =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let opcode = u16::from_le_bytes([bytes[6], bytes[7]]);
    let (doc_id, request_id, body_len) = (word(8), word(12), word(16) as usize);
    let context = Some((doc_id, request_id));
    if body_len > MAX_FRAME_BYTES - HEADER_BYTES {
        return Err(DecodeError {
            code: ErrorCode::FrameTooLarge,
            context,
        });
    }
    if bytes.len() != HEADER_BYTES + body_len {
        return Err(DecodeError {
            code: ErrorCode::InvalidEnvelope,
            context,
        });
    }
    Ok((opcode, doc_id, request_id, &bytes[HEADER_BYTES..]))
}

fn parse_command(opcode: u16, body: &[u8]) -> Result<Command, Fail> {
    let mut r = Reader { bytes: body, at: 0 };
    let command = match opcode {
        opcode::OPEN => {
            let kind = match r.u8()? {
                0 => OpenKind::Blank,
                1 => OpenKind::Txt,
                2 => OpenKind::Docx,
                _ => return Err(Fail::InvalidBody),
            };
            let profile = match r.u8()? {
                0 => Profile::Document,
                1 => Profile::Txt,
                _ => return Err(Fail::InvalidBody),
            };
            r.zero(2)?;
            let len = r.u32()? as usize;
            if len > MAX_OPEN_BYTES {
                return Err(Fail::FrameTooLarge);
            }
            let data = r.take(len)?.to_vec();
            let consistent = match kind {
                OpenKind::Blank => data.is_empty(),
                OpenKind::Txt => profile == Profile::Txt,
                OpenKind::Docx => profile == Profile::Document,
            };
            if !consistent {
                return Err(Fail::InvalidBody);
            }
            Command::Open {
                kind,
                profile,
                data,
            }
        }
        opcode::CLOSE => Command::Close,
        opcode::UNDO => Command::Undo,
        opcode::REDO => Command::Redo,
        opcode::INSERT_TEXT => {
            let text = r.text(MAX_TEXT_BYTES, Fail::FrameTooLarge)?;
            if text.is_empty() {
                return Err(Fail::InvalidBody);
            }
            validate_edit_text(&text)?;
            Command::InsertText { text }
        }
        opcode::REPLACE_RANGE => {
            let (start, end) = (r.u64()?, r.u64()?);
            check_range(u64::MAX, start, end)?;
            let text = r.text(MAX_TEXT_BYTES, Fail::FrameTooLarge)?;
            validate_edit_text(&text)?;
            Command::ReplaceRange { start, end, text }
        }
        opcode::DELETE => {
            let forward = r.flag()?;
            let unit = match r.u8()? {
                0 => DeleteUnit::Grapheme,
                1 => DeleteUnit::Word,
                _ => return Err(Fail::InvalidBody),
            };
            r.zero(2)?;
            Command::Delete { forward, unit }
        }
        opcode::MOVE => {
            let direction = Direction::from_u8(r.u8()?).ok_or(Fail::InvalidBody)?;
            let extend = r.flag()?;
            r.zero(2)?;
            Command::Move { direction, extend }
        }
        opcode::SET_SELECTION => {
            let (anchor, focus) = (r.u64()?, r.u64()?);
            let upstream = r.flag()?;
            r.zero(3)?;
            Command::SetSelection {
                anchor,
                focus,
                upstream,
            }
        }
        opcode::HIT_TEST => {
            let page = r.u32()?;
            let mode = match r.u8()? {
                0 => HitMode::Probe,
                1 => HitMode::SetCaret,
                2 => HitMode::Extend,
                _ => return Err(Fail::InvalidBody),
            };
            r.zero(3)?;
            if page >= MAX_PAGES {
                return Err(Fail::PageRange);
            }
            Command::HitTest {
                page,
                mode,
                x: r.f64()?,
                y: r.f64()?,
            }
        }
        opcode::TEXT_WINDOW => {
            let (start, len) = (r.u64()?, r.u32()?);
            if len == 0 {
                return Err(Fail::InvalidBody);
            }
            if len as usize > MAX_WINDOW_BYTES {
                return Err(Fail::WindowTooLarge);
            }
            Command::TextWindow { start, len }
        }
        opcode::PAGES => {
            let (first, count) = (r.u32()?, r.u32()?);
            if count == 0 {
                return Err(Fail::InvalidBody);
            }
            if count as usize > MAX_PAGES_PER_RESPONSE {
                return Err(Fail::TooManyPages);
            }
            if first >= MAX_PAGES {
                return Err(Fail::PageRange);
            }
            Command::Pages { first, count }
        }
        opcode::MARK_SAVED => Command::MarkSaved { revision: r.u64()? },
        opcode::LAYOUT => {
            let budget = r.u32()?;
            if !(1..=MAX_LAYOUT_BUDGET).contains(&budget) {
                return Err(Fail::InvalidBody);
            }
            Command::Layout { budget }
        }
        opcode::ACCESSIBLE_WINDOW => {
            let (start, end) = (r.u64()?, r.u64()?);
            check_range(u64::MAX, start, end)?;
            if end - start > MAX_WINDOW_BYTES as u64 {
                return Err(Fail::WindowTooLarge);
            }
            Command::AccessibleWindow { start, end }
        }
        opcode::RESYNC => Command::Resync,
        opcode::SAVE | opcode::INSERT_IMAGE | opcode::MAKE_EDITABLE_COPY => {
            io::parse_command(opcode, &mut r)?
        }
        opcode::SELECTION_RECTS
        | opcode::IMAGE_BITMAP
        | opcode::SELECT_UNIT
        | opcode::MOVE_TEXT => sel::parse_command(opcode, &mut r)?,
        opcode::SEARCH
        | opcode::STATISTICS
        | opcode::SEARCH_RECTS
        | opcode::REPLACE
        | opcode::REPLACE_ALL
        | opcode::PROPERTIES_READ
        | opcode::PROPERTIES_APPLY => find::parse_command(opcode, &mut r)?,
        opcode::FORMAT_STATE
        | opcode::APPLY_CHAR_FORMAT
        | opcode::APPLY_PARA_FORMAT
        | opcode::LIST_STYLES
        | opcode::APPLY_STYLE
        | opcode::PREVIEW_STYLE
        | opcode::LIST_FONTS
        | opcode::FORMAT_MARKS
        | opcode::LOAD_FONTS => format::parse_command(opcode, &mut r)?,
        opcode::COPY_FRAGMENT | opcode::PASTE_FRAGMENT => fragment::parse_command(opcode, &mut r)?,
        _ => return Err(Fail::UnknownOpcode),
    };
    r.finish()?;
    Ok(command)
}

pub fn decode_request(bytes: &[u8]) -> Result<Request, DecodeError> {
    let (op, doc_id, request_id, body) = split(bytes)?;
    let context = Some((doc_id, request_id));
    let fail = |code| DecodeError { code, context };
    if doc_id == 0 {
        return Err(fail(ErrorCode::InvalidDocumentId));
    }
    let command = parse_command(op, body).map_err(fail)?;
    Ok(Request {
        doc_id,
        request_id,
        command,
    })
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f64(out: &mut Vec<u8>, v: f64) {
    put_u64(out, v.to_bits());
}
fn put_text(out: &mut Vec<u8>, text: &str) -> Result<(), Fail> {
    put_u32(
        out,
        u32::try_from(text.len()).map_err(|_| Fail::FrameTooLarge)?,
    );
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Encodes, and re-decodes the result so that an encoder can never emit a
/// frame the decoder rejects (replies re-decode in tests only).
pub fn encode_request(request: &Request) -> Result<Vec<u8>, ErrorCode> {
    let mut b = Vec::new();
    let op = match &request.command {
        Command::Open {
            kind,
            profile,
            data,
        } => {
            b.extend_from_slice(&[*kind as u8, *profile as u8, 0, 0]);
            put_u32(
                &mut b,
                u32::try_from(data.len()).map_err(|_| Fail::FrameTooLarge)?,
            );
            b.extend_from_slice(data);
            opcode::OPEN
        }
        Command::Close => opcode::CLOSE,
        Command::Undo => opcode::UNDO,
        Command::Redo => opcode::REDO,
        Command::InsertText { text } => {
            put_text(&mut b, text)?;
            opcode::INSERT_TEXT
        }
        Command::ReplaceRange { start, end, text } => {
            put_u64(&mut b, *start);
            put_u64(&mut b, *end);
            put_text(&mut b, text)?;
            opcode::REPLACE_RANGE
        }
        Command::Delete { forward, unit } => {
            b.extend_from_slice(&[u8::from(*forward), *unit as u8, 0, 0]);
            opcode::DELETE
        }
        Command::Move { direction, extend } => {
            b.extend_from_slice(&[*direction as u8, u8::from(*extend), 0, 0]);
            opcode::MOVE
        }
        Command::SetSelection {
            anchor,
            focus,
            upstream,
        } => {
            put_u64(&mut b, *anchor);
            put_u64(&mut b, *focus);
            b.extend_from_slice(&[u8::from(*upstream), 0, 0, 0]);
            opcode::SET_SELECTION
        }
        Command::Layout { budget } => {
            put_u32(&mut b, *budget);
            opcode::LAYOUT
        }
        Command::Resync => opcode::RESYNC,
        Command::HitTest { page, mode, x, y } => {
            put_u32(&mut b, *page);
            b.extend_from_slice(&[*mode as u8, 0, 0, 0]);
            put_f64(&mut b, *x);
            put_f64(&mut b, *y);
            opcode::HIT_TEST
        }
        Command::TextWindow { start, len } => {
            put_u64(&mut b, *start);
            put_u32(&mut b, *len);
            opcode::TEXT_WINDOW
        }
        Command::Pages { first, count } => {
            put_u32(&mut b, *first);
            put_u32(&mut b, *count);
            opcode::PAGES
        }
        Command::MarkSaved { revision } => {
            put_u64(&mut b, *revision);
            opcode::MARK_SAVED
        }
        Command::AccessibleWindow { start, end } => {
            put_u64(&mut b, *start);
            put_u64(&mut b, *end);
            opcode::ACCESSIBLE_WINDOW
        }
        Command::Save { .. } | Command::InsertImage { .. } | Command::MakeEditableCopy { .. } => {
            io::encode_command(&request.command, &mut b)?
        }
        Command::SelectionRects { .. }
        | Command::ImageBitmap { .. }
        | Command::SelectUnit { .. }
        | Command::MoveText { .. } => sel::encode_command(&request.command, &mut b)?,
        Command::Search { .. }
        | Command::Statistics
        | Command::SearchRects { .. }
        | Command::Replace { .. }
        | Command::ReplaceAll { .. }
        | Command::PropertiesRead
        | Command::PropertiesApply { .. } => find::encode_command(&request.command, &mut b)?,
        Command::FormatState
        | Command::ApplyCharFormat(_)
        | Command::ApplyParaFormat(_)
        | Command::ListStyles { .. }
        | Command::ApplyStyle { .. }
        | Command::PreviewStyle { .. }
        | Command::ListFonts
        | Command::LoadFonts(_)
        | Command::FormatMarks { .. } => format::encode_command(&request.command, &mut b)?,
        Command::CopyFragment { .. } | Command::PasteFragment { .. } => {
            fragment::encode_command(&request.command, &mut b)?
        }
    };
    let frame = header(op, request.doc_id, request.request_id, &b)?;
    decode_request(&frame).map_err(|e| e.code)?;
    Ok(frame)
}

/// True when a page of `next` bytes fits in a response that already carries
/// `count` pages and `total` bytes of display lists: the first page always
/// fits (up to the per-page limit), later ones only under the response sum.
pub fn page_fits(count: usize, total: usize, next: usize) -> bool {
    next <= MAX_PAGE_DISPLAY_BYTES
        && (count == 0 || total.saturating_add(next) <= MAX_RESPONSE_PAGE_BYTES)
}

fn put_pages(out: &mut Vec<u8>, pages: &[PageDisplay]) -> Result<(), Fail> {
    if pages.len() > MAX_PAGES_PER_RESPONSE {
        return Err(Fail::TooManyPages);
    }
    let mut total = 0usize;
    for (n, page) in pages.iter().enumerate() {
        if !page_fits(n, total, page.display.len()) {
            return Err(Fail::FrameTooLarge);
        }
        total += page.display.len();
    }
    put_u32(
        out,
        u32::try_from(pages.len()).map_err(|_| Fail::TooManyPages)?,
    );
    for page in pages {
        put_u32(out, page.index);
        put_u32(
            out,
            u32::try_from(page.display.len()).map_err(|_| Fail::FrameTooLarge)?,
        );
        out.extend_from_slice(&page.display);
    }
    Ok(())
}

fn put_atlas(out: &mut Vec<u8>, atlas: &AtlasDelta) -> Result<(), Fail> {
    let count = atlas.entries.len();
    let mut total = 0usize;
    for entry in &atlas.entries {
        total = total.saturating_add(entry.len());
        if !(MIN_PAGE_DISPLAY_BYTES..=MAX_ATLAS_ENTRY_BYTES).contains(&entry.len()) {
            return Err(Fail::InvalidBody);
        }
    }
    if u64::from(atlas.first) + count as u64 > u64::from(MAX_ATLAS_ENTRIES)
        || total > MAX_ATLAS_RESPONSE_BYTES
    {
        return Err(Fail::InvalidBody);
    }
    put_u32(out, atlas.epoch);
    put_u32(out, atlas.first);
    put_u32(out, count as u32);
    for entry in &atlas.entries {
        put_u32(out, entry.len() as u32);
        out.extend_from_slice(entry);
    }
    Ok(())
}

fn read_atlas(r: &mut Reader<'_>) -> Result<AtlasDelta, Fail> {
    let (epoch, first, count) = (r.u32()?, r.u32()?, r.u32()? as usize);
    if u64::from(first) + count as u64 > u64::from(MAX_ATLAS_ENTRIES) {
        return Err(Fail::InvalidBody);
    }
    let mut entries = Vec::with_capacity(count);
    let mut total = 0usize;
    for _ in 0..count {
        let len = r.u32()? as usize;
        if !(MIN_PAGE_DISPLAY_BYTES..=MAX_ATLAS_ENTRY_BYTES).contains(&len) {
            return Err(Fail::InvalidBody);
        }
        total += len;
        if total > MAX_ATLAS_RESPONSE_BYTES {
            return Err(Fail::FrameTooLarge);
        }
        let entry = r.take(len)?.to_vec();
        if entry[..4] != *b"ATDL" {
            return Err(Fail::InvalidBody);
        }
        entries.push(entry);
    }
    Ok(AtlasDelta {
        epoch,
        first,
        entries,
    })
}

fn read_pages(r: &mut Reader<'_>, page_count: u32) -> Result<Vec<PageDisplay>, Fail> {
    let n = r.u32()? as usize;
    if n > MAX_PAGES_PER_RESPONSE {
        return Err(Fail::TooManyPages);
    }
    let mut pages = Vec::with_capacity(n);
    let mut total = 0usize;
    for _ in 0..n {
        let index = r.u32()?;
        let len = r.u32()? as usize;
        if index >= page_count || pages.last().is_some_and(|p: &PageDisplay| p.index >= index) {
            return Err(Fail::PageRange);
        }
        if !(MIN_PAGE_DISPLAY_BYTES..=MAX_PAGE_DISPLAY_BYTES).contains(&len) {
            return Err(Fail::InvalidBody);
        }
        if !page_fits(pages.len(), total, len) {
            return Err(Fail::FrameTooLarge);
        }
        total += len;
        let display = r.take(len)?.to_vec();
        if display[..4] != *b"ATDL" {
            return Err(Fail::InvalidBody);
        }
        pages.push(PageDisplay { index, display });
    }
    Ok(pages)
}

fn put_text16(out: &mut Vec<u8>, text: &str) -> Result<(), Fail> {
    out.extend_from_slice(
        &u16::try_from(text.len())
            .map_err(|_| Fail::InvalidBody)?
            .to_le_bytes(),
    );
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn put_accessible(out: &mut Vec<u8>, items: &[AccessibleItem]) -> Result<(), Fail> {
    if items.len() > MAX_ACCESSIBLE_ITEMS {
        return Err(Fail::InvalidBody);
    }
    put_u32(
        out,
        u32::try_from(items.len()).map_err(|_| Fail::InvalidBody)?,
    );
    for item in items {
        put_u64(out, item.para_start);
        put_u64(out, item.para_end);
        out.extend_from_slice(&[item.role, item.level, 0, 0]);
        put_text16(out, &item.label)?;
        put_text(out, &item.text)?;
        out.extend_from_slice(
            &u16::try_from(item.inlines.len())
                .map_err(|_| Fail::InvalidBody)?
                .to_le_bytes(),
        );
        for inline in &item.inlines {
            put_u64(out, inline.offset);
            out.extend_from_slice(&[inline.kind, 0]);
            put_text16(out, &inline.value)?;
        }
    }
    Ok(())
}

fn read_accessible(r: &mut Reader<'_>) -> Result<Vec<AccessibleItem>, Fail> {
    const OBJECT: [u8; 3] = [0xEF, 0xBF, 0xBC];
    let count = r.u32()? as usize;
    if count > MAX_ACCESSIBLE_ITEMS {
        return Err(Fail::InvalidBody);
    }
    let mut items: Vec<AccessibleItem> = Vec::with_capacity(count);
    for _ in 0..count {
        let (para_start, para_end) = (r.u64()?, r.u64()?);
        let (role, level) = (r.u8()?, r.u8()?);
        r.zero(2)?;
        let level_ok = match role {
            role::HEADING => (1..=9).contains(&level),
            role::LIST_ITEM => level <= 8,
            role::PARAGRAPH | role::TABLE_CELL | role::OPAQUE => level == 0,
            _ => false,
        };
        let label = r.text16()?;
        let len = r.u32()? as usize;
        let raw = r.take(len)?;
        let text = core::str::from_utf8(raw)
            .map_err(|_| Fail::InvalidUtf8)?
            .to_owned();
        let ordered = items.last().is_none_or(|p| para_start > p.para_end);
        if !level_ok || para_start > para_end || para_end - para_start != len as u64 || !ordered {
            return Err(Fail::InvalidBody);
        }
        let n = usize::from(r.u16()?);
        let mut inlines: Vec<AccessibleInline> = Vec::with_capacity(n);
        let mut floor = 0usize;
        for _ in 0..n {
            let offset = r.u64()?;
            let kind = r.u8()?;
            r.zero(1)?;
            let value = r.text16()?;
            let at = usize::try_from(offset).map_err(|_| Fail::InvalidBody)?;
            if kind > inline_kind::SYMBOL
                || at < floor
                || raw.get(at..at.saturating_add(3)) != Some(&OBJECT[..])
            {
                return Err(Fail::InvalidBody);
            }
            floor = at + 3;
            inlines.push(AccessibleInline {
                offset,
                kind,
                value,
            });
        }
        // Every U+FFFC of the text has its inline.
        if text.matches('\u{FFFC}').count() != n {
            return Err(Fail::InvalidBody);
        }
        items.push(AccessibleItem {
            para_start,
            para_end,
            role,
            level,
            label,
            text,
            inlines,
        });
    }
    Ok(items)
}

fn encode_state(s: &State, b: &mut Vec<u8>) -> Result<(), Fail> {
    if s.invalidated.len() > MAX_INVALIDATIONS {
        return Err(Fail::InvalidBody);
    }
    if s.window
        .as_ref()
        .is_some_and(|w| w.text.len() > MAX_WINDOW_BYTES)
    {
        return Err(Fail::WindowTooLarge);
    }
    for v in [s.revision, s.anchor, s.focus, s.doc_len] {
        put_u64(b, v);
    }
    let flags = u32::from(s.modified)
        | u32::from(s.can_undo) << 1
        | u32::from(s.can_redo) << 2
        | u32::from(s.layout_complete) << 3
        | u32::from(s.window.is_some()) << 4
        | u32::from(s.focus_upstream) << 5
        | u32::from(s.read_only) << 6
        | u32::from(s.has_revisions) << 7;
    put_u32(b, flags);
    put_u32(b, s.page_count);
    let caret = s.caret.unwrap_or(Caret {
        page: NO_CARET,
        x: 0.0,
        y: 0.0,
        height: 0.0,
    });
    put_u32(b, caret.page);
    for v in [caret.x, caret.y, caret.height] {
        put_f64(b, v);
    }
    put_u32(
        b,
        u32::try_from(s.invalidated.len()).map_err(|_| Fail::InvalidBody)?,
    );
    for (first, end) in &s.invalidated {
        put_u32(b, *first);
        put_u32(b, *end);
    }
    put_atlas(b, &s.atlas)?;
    put_pages(b, &s.pages)?;
    if let Some(w) = &s.window {
        put_u64(b, w.start);
        put_text(b, &w.text)?;
    }
    Ok(())
}

fn parse_state(r: &mut Reader<'_>) -> Result<State, Fail> {
    let (revision, anchor, focus, doc_len) = (r.u64()?, r.u64()?, r.u64()?, r.u64()?);
    let flags = r.u32()?;
    if flags & !0xff != 0 || anchor > doc_len || focus > doc_len {
        return Err(Fail::InvalidBody);
    }
    let page_count = r.u32()?;
    if page_count > MAX_PAGES {
        return Err(Fail::TooManyPages);
    }
    let caret_page = r.u32()?;
    let (x, y, height) = (r.f64()?, r.f64()?, r.f64()?);
    let caret = if caret_page == NO_CARET {
        if x.to_bits() | y.to_bits() | height.to_bits() != 0 {
            return Err(Fail::InvalidBody);
        }
        None
    } else if caret_page >= page_count || height < 0.0 {
        return Err(Fail::InvalidBody);
    } else {
        Some(Caret {
            page: caret_page,
            x,
            y,
            height,
        })
    };
    let n = r.u32()? as usize;
    if n > MAX_INVALIDATIONS {
        return Err(Fail::InvalidBody);
    }
    let mut invalidated: Vec<(u32, u32)> = Vec::with_capacity(n);
    for _ in 0..n {
        let (first, end) = (r.u32()?, r.u32()?);
        if first >= end || end > MAX_PAGES || invalidated.last().is_some_and(|p| p.1 >= first) {
            return Err(Fail::PageRange);
        }
        invalidated.push((first, end));
    }
    let atlas = read_atlas(r)?;
    let pages = read_pages(r, page_count)?;
    let window = if flags & 0x10 != 0 {
        let start = r.u64()?;
        let text = r.text(MAX_WINDOW_BYTES, Fail::WindowTooLarge)?;
        if start
            .checked_add(text.len() as u64)
            .is_none_or(|end| end > doc_len)
        {
            return Err(Fail::PositionOutOfRange);
        }
        Some(TextWindowData { start, text })
    } else {
        None
    };
    Ok(State {
        revision,
        anchor,
        focus,
        doc_len,
        modified: flags & 1 != 0,
        can_undo: flags & 2 != 0,
        can_redo: flags & 4 != 0,
        layout_complete: flags & 8 != 0,
        focus_upstream: flags & 0x20 != 0,
        read_only: flags & 0x40 != 0,
        has_revisions: flags & 0x80 != 0,
        page_count,
        caret,
        invalidated,
        atlas,
        pages,
        window,
    })
}

pub fn encode_reply(reply: &Reply) -> Result<Vec<u8>, ErrorCode> {
    let mut b = Vec::new();
    let op = match &reply.response {
        Response::Error(code) => {
            b.extend_from_slice(&(*code as u16).to_le_bytes());
            b.extend_from_slice(&[0, 0]);
            opcode::R_ERROR
        }
        Response::State(state) => {
            encode_state(state, &mut b)?;
            opcode::R_STATE
        }
        Response::TextWindow {
            revision,
            start,
            doc_len,
            text,
        } => {
            if text.len() > MAX_WINDOW_BYTES {
                return Err(ErrorCode::WindowTooLarge);
            }
            put_u64(&mut b, *revision);
            put_u64(&mut b, *start);
            put_u64(&mut b, *doc_len);
            put_text(&mut b, text)?;
            opcode::R_TEXT_WINDOW
        }
        Response::Pages {
            revision,
            page_count,
            atlas,
            pages,
        } => {
            put_u64(&mut b, *revision);
            put_u32(&mut b, *page_count);
            put_atlas(&mut b, atlas)?;
            put_pages(&mut b, pages)?;
            opcode::R_PAGES
        }
        Response::HitResult {
            revision,
            page,
            position,
            upstream,
        } => {
            put_u64(&mut b, *revision);
            put_u32(&mut b, *page);
            put_u64(&mut b, *position);
            b.extend_from_slice(&[u8::from(*upstream), 0, 0, 0]);
            opcode::R_HIT
        }
        Response::Closed => opcode::R_CLOSED,
        Response::AccessibleWindow { revision, items } => {
            put_u64(&mut b, *revision);
            put_accessible(&mut b, items)?;
            opcode::R_ACCESSIBLE_WINDOW
        }
        Response::SaveData {
            revision,
            bytes,
            notices,
        } => {
            io::put_save_data(&mut b, *revision, bytes, notices)?;
            opcode::R_SAVE_DATA
        }
        Response::SelectionRects { revision, rects } => {
            sel::put_selection_rects(&mut b, *revision, rects)?;
            opcode::R_SELECTION_RECTS
        }
        Response::ImageBitmap {
            object_id,
            width,
            height,
            rgba,
        } => {
            sel::put_image_bitmap(&mut b, *object_id, *width, *height, rgba)?;
            opcode::R_IMAGE_BITMAP
        }
        Response::SearchResult { .. }
        | Response::Statistics { .. }
        | Response::SearchRects { .. }
        | Response::Replaced { .. }
        | Response::Properties { .. }
        | Response::PropertiesApplied(_) => find::encode_reply(&reply.response, &mut b)?,
        Response::FormatState(_)
        | Response::StyleList { .. }
        | Response::FontList(_)
        | Response::FormatMarks { .. } => format::encode_reply(&reply.response, &mut b)?,
        Response::Fragment {
            revision,
            lossy,
            text,
            bytes,
        } => {
            fragment::put_fragment(&mut b, *revision, *lossy, text, bytes)?;
            opcode::R_FRAGMENT
        }
    };
    let frame = header(op, reply.doc_id, reply.request_id, &b)?;
    #[cfg(test)]
    decode_reply(&frame).map_err(|e| e.code)?;
    Ok(frame)
}

pub fn decode_reply(bytes: &[u8]) -> Result<Reply, DecodeError> {
    let (op, doc_id, request_id, body) = split(bytes)?;
    let fail = |code| DecodeError {
        code,
        context: Some((doc_id, request_id)),
    };
    if doc_id == 0 {
        return Err(fail(ErrorCode::InvalidDocumentId));
    }
    let mut r = Reader { bytes: body, at: 0 };
    let response = (|| -> Result<Response, Fail> {
        let response = match op {
            opcode::R_ERROR => {
                let code = ErrorCode::from_u16(r.u16()?).ok_or(Fail::InvalidBody)?;
                r.zero(2)?;
                Response::Error(code)
            }
            opcode::R_STATE => Response::State(parse_state(&mut r)?),
            opcode::R_TEXT_WINDOW => {
                let (revision, start, doc_len) = (r.u64()?, r.u64()?, r.u64()?);
                let text = r.text(MAX_WINDOW_BYTES, Fail::WindowTooLarge)?;
                if start
                    .checked_add(text.len() as u64)
                    .is_none_or(|end| end > doc_len)
                {
                    return Err(Fail::PositionOutOfRange);
                }
                Response::TextWindow {
                    revision,
                    start,
                    doc_len,
                    text,
                }
            }
            opcode::R_PAGES => {
                let (revision, page_count) = (r.u64()?, r.u32()?);
                if page_count > MAX_PAGES {
                    return Err(Fail::TooManyPages);
                }
                let atlas = read_atlas(&mut r)?;
                Response::Pages {
                    revision,
                    page_count,
                    atlas,
                    pages: read_pages(&mut r, page_count)?,
                }
            }
            opcode::R_HIT => {
                let (revision, page, position) = (r.u64()?, r.u32()?, r.u64()?);
                let upstream = r.flag()?;
                r.zero(3)?;
                Response::HitResult {
                    revision,
                    page,
                    position,
                    upstream,
                }
            }
            opcode::R_CLOSED => Response::Closed,
            opcode::R_ACCESSIBLE_WINDOW => Response::AccessibleWindow {
                revision: r.u64()?,
                items: read_accessible(&mut r)?,
            },
            opcode::R_SAVE_DATA => io::read_save_data(&mut r)?,
            opcode::R_SELECTION_RECTS => sel::read_selection_rects(&mut r)?,
            opcode::R_IMAGE_BITMAP => sel::read_image_bitmap(&mut r)?,
            opcode::R_SEARCH
            | opcode::R_STATISTICS
            | opcode::R_SEARCH_RECTS
            | opcode::R_REPLACED
            | opcode::R_PROPERTIES
            | opcode::R_PROPERTIES_APPLIED => find::parse_reply(op, &mut r)?,
            opcode::R_FORMAT_STATE
            | opcode::R_STYLE_LIST
            | opcode::R_FONT_LIST
            | opcode::R_FORMAT_MARKS => format::parse_reply(op, &mut r)?,
            opcode::R_FRAGMENT => fragment::read_fragment(&mut r)?,
            _ => return Err(Fail::UnknownOpcode),
        };
        r.finish()?;
        Ok(response)
    })()
    .map_err(fail)?;
    Ok(Reply {
        doc_id,
        request_id,
        response,
    })
}

impl From<atelier_text_model::rich::flat::FlatError> for ErrorCode {
    fn from(error: atelier_text_model::rich::flat::FlatError) -> Self {
        use atelier_text_model::rich::flat::FlatError as F;
        match error {
            F::OutOfRange => Self::PositionOutOfRange,
            F::NotBoundary => Self::PositionNotBoundary,
            F::InvalidRange => Self::InvalidRange,
            F::InvalidText => Self::InvalidText,
            F::Limit => Self::Limit,
            F::Style => Self::Internal,
            F::Unsupported => Self::MoveUnsupported,
        }
    }
}

impl From<crate::rich_layout::LayoutError> for ErrorCode {
    fn from(error: crate::rich_layout::LayoutError) -> Self {
        use crate::rich_layout::LayoutError as L;
        match error {
            L::PageOutOfRange => Self::PageRange,
            L::PositionOutOfRange => Self::PositionOutOfRange,
            L::NotReady => Self::NotReady,
        }
    }
}

/// Wire position to `usize`; a value that does not fit is out of range.
pub fn to_position(value: u64) -> Result<usize, ErrorCode> {
    usize::try_from(value).map_err(|_| ErrorCode::PositionOutOfRange)
}

/// Splits a layout delta into what one `State` carries: at most
/// `MAX_PAGES_PER_RESPONSE` pages to send (the lowest indices of `changed`),
/// and the invalidation ranges. Changed pages that do not fit become
/// invalidated single pages so that the client re-requests them with `Pages`.
/// Pages and ranges at or beyond `delta.page_count` are dropped or clamped
/// (the client also drops its own pages `>= page_count`). Ranges are clamped, sorted, merged when they overlap or
/// touch (the decoder refuses touching ranges), and limited to
/// `MAX_INVALIDATIONS` by merging the tail into one range. The client applies
/// `invalidated` first, then stores `pages`.
pub fn split_page_delta(delta: &crate::rich_layout::PageDelta) -> (Vec<u32>, Vec<(u32, u32)>) {
    let page_count = delta.page_count.min(MAX_PAGES);
    let mut changed: Vec<u32> = delta
        .changed
        .iter()
        .copied()
        .filter(|p| *p < page_count)
        .collect();
    changed.sort_unstable();
    changed.dedup();
    let overflow = changed.split_off(changed.len().min(MAX_PAGES_PER_RESPONSE));
    let mut ranges: Vec<(u32, u32)> = delta
        .invalidated
        .iter()
        .map(|(first, end)| (*first, (*end).min(page_count)))
        .filter(|(first, end)| first < end)
        .chain(overflow.iter().map(|p| (*p, *p + 1)))
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (first, end) in ranges {
        match merged.last_mut() {
            Some(last) if first <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((first, end)),
        }
    }
    if merged.len() > MAX_INVALIDATIONS {
        let end = merged.last().map_or(0, |r| r.1);
        merged.truncate(MAX_INVALIDATIONS);
        if let Some(last) = merged.last_mut() {
            last.1 = end;
        }
    }
    (changed, merged)
}

mod format;
pub use format::{
    BORDER_SIDES, BorderEdit, BorderLine, CharAttr, CharEdit, CharFormat, CharState, CharValue,
    EditOp, FontEntry, FontListData, FontState, FormatMark, FormatScope, FormatStateData,
    MARK_KINDS, MAX_BORDER_EIGHTHS, MAX_BORDER_SPACE, MAX_FONT_FILE_BYTES, MAX_FONT_LIST,
    MAX_FORMAT_EDITS, MAX_FORMAT_MARKS, MAX_HALF_POINTS, MAX_LOAD_FONTS, MAX_NAME_BYTES,
    MAX_STATE_RUNS, MAX_STYLE_LIST, MAX_TWIPS, MIN_HALF_POINTS, NO_REVISION_GUARD, ParaAttr,
    ParaEdit, ParaFormat, ParaState, ParaValue, Rgb, SIZE_STEPS, StyleEntry, StyleFilter,
    StyleTarget, Tri, size_step,
};
mod find;
pub use find::{
    HighlightRect, MAX_EXCERPT_BYTES, MAX_HIGHLIGHT_RANGES, MAX_HIGHLIGHT_RECTS,
    MAX_PROPERTY_BYTES, MAX_QUERY_BYTES, MAX_REPLACE_MATCHES, MAX_SEARCH_ITEMS, PROPERTY_COUNT,
    PropertiesOutcome, PropertiesSnapshot, PropertiesStatus, SEARCH_COUNT_CAP, SearchExcerpt,
    SearchFlags, SearchItem, StatCounts,
};

mod fragment;
pub use fragment::{MAX_FRAGMENT_BYTES, MAX_FRAGMENT_TEXT_BYTES};
mod io;
pub use io::{
    MAX_ALT_BYTES, MAX_IMAGE_BYTES, MAX_NOTICE_BYTES, MAX_NOTICES, MAX_SAVE_BYTES, Notice,
    SaveFormat, Stamp, notice,
};
mod sel;
pub use sel::{MAX_BITMAP_SIDE, MAX_SELECTION_RECTS, SelectionRect, SelectionUnit};
#[cfg(test)]
mod move_tests;
#[cfg(test)]
mod sel_tests;

#[cfg(test)]
mod find_tests;
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod fragment_tests;
#[cfg(test)]
mod io_tests;
#[cfg(test)]
mod tests;
