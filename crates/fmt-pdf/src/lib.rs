// SPDX-License-Identifier: AGPL-3.0-or-later
//! PDF 1.7 writer: replays `atelier-render` display lists as page content,
//! embeds subset fonts with a `ToUnicode` map, and writes images, links and
//! bookmarks. Pure `std` plus workspace crates, deterministic, no clock.
//!
//! The resource provider extends the SVG/raster one (`glyph_outline`, `image`)
//! with the font program behind a `font_id` and the text behind a glyph; see
//! [`PdfResourceProvider`].

#![forbid(unsafe_code)]

mod content;
mod doc;
mod font;
mod image;
mod out;

pub use doc::PdfWriter;

use atelier_font::FontError;
use atelier_font::subset::SubsetError;
use atelier_image::ImageError;
use atelier_render::DisplayList;
pub use atelier_render::{SvgImage as PdfImage, SvgResourceProvider};
use core::fmt;
use std::io;

/// A font program and the scale at which a `font_id` is used.
#[derive(Clone, Copy, Debug)]
pub struct PdfFont<'a> {
    /// Identity of the font program. Every `font_id` (typically one per size)
    /// with the same `program_id` and `face_index` shares one embedded font.
    pub program_id: u64,
    /// The complete font file (TrueType, OpenType/CFF or collection).
    pub data: &'a [u8],
    /// Face number inside a collection (0 otherwise).
    pub face_index: usize,
    /// Em size in page units for this `font_id`.
    pub size: f64,
}

/// Resource lookup for [`render_pdf`]. Besides outlines and images (shared
/// with the SVG and raster backends) the PDF needs the font program and the
/// source text of each glyph.
pub trait PdfResourceProvider: SvgResourceProvider {
    /// The font program and size behind a display-list `font_id`.
    fn font(&self, font_id: u64) -> Option<PdfFont<'_>>;

    /// Characters a glyph stands for (`"fi"` for a ligature, one character
    /// otherwise). Used for the `ToUnicode` map, hence for selection, copy
    /// and search. A glyph without text is reported as a notice.
    fn glyph_text(&self, font_id: u64, glyph_id: u32) -> Option<&str> {
        let _ = (font_id, glyph_id);
        None
    }
}

/// Date for the document information dictionary, supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Offset from UTC in minutes (east positive).
    pub utc_offset_minutes: i16,
}

/// Document information dictionary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    /// Defaults to `Atelier`.
    pub producer: Option<String>,
    pub creation_date: Option<PdfDate>,
    pub modification_date: Option<PdfDate>,
}

impl Default for PdfInfo {
    fn default() -> Self {
        Self {
            title: None,
            author: None,
            subject: None,
            keywords: None,
            creator: None,
            producer: Some("Atelier".to_owned()),
            creation_date: None,
            modification_date: None,
        }
    }
}

/// A position in the document, in display-list page coordinates (origin top left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    /// Zero-based page index.
    pub page: usize,
    /// Distance from the top of the page, in page units; `None` keeps the
    /// viewer's vertical position.
    pub top: Option<f64>,
}

/// A bookmark and its children.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub destination: Destination,
    pub children: Vec<OutlineItem>,
    /// Whether the children are shown when the document opens.
    pub open: bool,
}

/// Budgets, checked before the corresponding allocation or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfLimits {
    pub max_pages: usize,
    pub max_output_bytes: u64,
    pub max_objects: usize,
    /// Largest page content stream before compression.
    pub max_content_bytes: usize,
    /// Content kept in memory while waiting for the final CIDs of CFF fonts.
    pub max_pending_bytes: usize,
    pub max_image_bytes: usize,
    pub max_image_pixels: u64,
    pub max_fonts: usize,
    pub max_outline_items: usize,
    pub max_outline_depth: usize,
}

impl Default for PdfLimits {
    fn default() -> Self {
        Self {
            max_pages: 100_000,
            max_output_bytes: 1 << 30,
            max_objects: 4_000_000,
            max_content_bytes: 256 << 20,
            max_pending_bytes: 512 << 20,
            max_image_bytes: 64 << 20,
            max_image_pixels: 64 << 20,
            max_fonts: 4096,
            max_outline_items: 100_000,
            max_outline_depth: 32,
        }
    }
}

/// Everything the caller chooses for a document.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    pub info: PdfInfo,
    /// Natural language of the document (`/Lang`, e.g. `fr-FR`).
    pub language: Option<String>,
    /// Targets of `Link` commands whose target is `#name`.
    pub named_destinations: Vec<(String, Destination)>,
    pub outlines: Vec<OutlineItem>,
    /// Insert a space when two glyphs of a run are further apart than a word
    /// gap and no glyph for the space was supplied, so that extracted text
    /// keeps its words.
    pub infer_spaces: bool,
    /// Deflate streams (turn off to read the file by eye).
    pub compress: bool,
    pub limits: PdfLimits,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            info: PdfInfo::default(),
            language: None,
            named_destinations: Vec::new(),
            outlines: Vec::new(),
            infer_spaces: true,
            compress: true,
            limits: PdfLimits::default(),
        }
    }
}

/// Why a font is not embedded as a selectable font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoEmbedReason {
    /// `OS/2.fsType` forbids embedding: glyphs are drawn as outlines.
    Restricted,
    /// Only bitmaps may be embedded: glyphs are drawn as outlines.
    BitmapOnly,
    /// Subsetting is forbidden and the font is a collection that cannot be embedded whole.
    CollectionNotEmbeddableWhole,
}

/// Non-fatal observations returned with the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfNotice {
    /// Text of this font is drawn as paths and is not selectable.
    FontDrawnAsPaths { font_id: u64, reason: NoEmbedReason },
    /// The font forbids subsetting, so it is embedded whole.
    FontEmbeddedWhole { font_id: u64 },
    /// The provider gave no text for a glyph: it is not extractable.
    MissingGlyphText { font_id: u64, glyph_id: u32 },
    /// The provider gave different texts for one glyph of a font program; the first is kept.
    ConflictingGlyphText { font_id: u64, glyph_id: u32 },
    /// A `Link` target could not be resolved; no annotation was written.
    UnresolvedLink { page: usize, target: String },
    /// More notices were produced than the list keeps.
    NoticesTruncated,
}

/// Typed failures. No partial document is returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfError {
    /// The display list failed validation (unbalanced stacks, non-finite values, ...).
    InvalidList,
    /// A coordinate does not fit a PDF number (non-finite or beyond 10^9).
    CoordinateOutOfRange,
    /// A page is larger than 14 400 units on a side.
    PageTooLarge,
    /// Options are inconsistent; the text says which.
    InvalidOptions(&'static str),
    /// The provider has no font for this `font_id`.
    MissingFont(u64),
    /// A glyph run uses a glyph outside the font.
    UnknownGlyph {
        font_id: u64,
        glyph_id: u32,
    },
    /// A glyph drawn as a path has no outline.
    MissingOutline {
        font_id: u64,
        glyph_id: u32,
    },
    /// The provider has no image for this id.
    MissingImage(u64),
    Image(ImageError),
    Font(FontError),
    Subset(SubsetError),
    /// A font format the writer cannot embed.
    UnsupportedFont(&'static str),
    /// A destination or link refers to a page that does not exist.
    InvalidDestination,
    /// A budget was exceeded.
    LimitExceeded(&'static str),
    /// A document needs at least one page.
    EmptyDocument,
    /// The sink failed.
    Io(io::ErrorKind),
    /// A bug in the writer (reported rather than panicking).
    Internal(&'static str),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidList => f.write_str("invalid display list"),
            Self::CoordinateOutOfRange => f.write_str("coordinate out of range for PDF"),
            Self::PageTooLarge => f.write_str("page larger than 14400 units"),
            Self::InvalidOptions(s) => write!(f, "invalid options: {s}"),
            Self::MissingFont(id) => write!(f, "no font for font id {id}"),
            Self::UnknownGlyph { font_id, glyph_id } => {
                write!(f, "glyph {glyph_id} is not in font {font_id}")
            }
            Self::MissingOutline { font_id, glyph_id } => {
                write!(f, "no outline for glyph {glyph_id} of font {font_id}")
            }
            Self::MissingImage(id) => write!(f, "no image for image id {id}"),
            Self::Image(e) => write!(f, "image: {e}"),
            Self::Font(e) => write!(f, "font: {e}"),
            Self::Subset(e) => write!(f, "font subset: {e}"),
            Self::UnsupportedFont(s) => write!(f, "unsupported font: {s}"),
            Self::InvalidDestination => f.write_str("destination refers to a missing page"),
            Self::LimitExceeded(s) => write!(f, "limit exceeded: {s}"),
            Self::EmptyDocument => f.write_str("a PDF needs at least one page"),
            Self::Io(k) => write!(f, "output error: {k}"),
            Self::Internal(s) => write!(f, "internal error: {s}"),
        }
    }
}

impl std::error::Error for PdfError {}

/// A finished document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfOutput {
    pub bytes: Vec<u8>,
    pub notices: Vec<PdfNotice>,
}

/// Renders `pages` (one display list per page) into one PDF.
pub fn render_pdf<P: PdfResourceProvider + ?Sized>(
    pages: &[DisplayList],
    provider: &P,
    options: &PdfOptions,
) -> Result<PdfOutput, PdfError> {
    let mut writer = PdfWriter::new(Vec::new(), options.clone())?;
    for page in pages {
        writer.add_page(page, provider)?;
    }
    let (bytes, notices) = writer.finish(provider)?;
    Ok(PdfOutput { bytes, notices })
}
