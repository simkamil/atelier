// SPDX-License-Identifier: AGPL-3.0-or-later
//! Word 97-2003 (`.doc`) import: text, character and paragraph formatting and
//! styles (US-312).
//!
//! The container is read with `atelier-cfb`; the `WordDocument` stream, the
//! table stream selected by the FIB and, only when needed, the `Data` stream
//! are interpreted according to [MS-DOC]. The result mirrors the DOCX import
//! of `atelier-fmt-ooxml`: a validated `RichDocument`, a `StyleEngine`, import
//! notices for what the rich model cannot express, typed errors and explicit
//! resource limits. Encrypted documents are rejected with
//! [`DocError::PasswordProtected`] before any text is read.

mod build;
mod fib;
mod fkp;
mod fonts;
mod sprm;
mod stsh;
mod summary;
mod text;

pub use fib::{Fib, N_FIB_WORD97, W_IDENT};
pub use sprm::OPAQUE_NS;

use atelier_cfb::{CfbError, Compound, Limits as CfbLimits};
use atelier_text_model::rich::{
    CharAttrs, NodeId, Paragraph, ResolvedProperties, RichDocument, StyleEngine,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use std::fmt;

pub(crate) fn rd_u16(b: &[u8], o: usize) -> Option<u16> {
    let s = b.get(o..o.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

pub(crate) fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Ceilings applied in addition to the container limits. All are checked
/// before the corresponding allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocLimits {
    pub cfb: CfbLimits,
    /// Characters (UTF-16 units) of the main document story (`ccpText`).
    pub max_text_chars: usize,
    /// Pieces of the piece table (and `Prc` entries).
    pub max_pieces: usize,
    /// Formatted disk pages referenced by one bin table.
    pub max_fkp_pages: usize,
    /// Style definitions (`cstd`).
    pub max_styles: usize,
    /// Font table entries.
    pub max_fonts: usize,
    /// Paragraphs imported.
    pub max_paragraphs: usize,
    /// Character formatting runs (segments) built.
    pub max_runs: usize,
    /// Sprms interpreted in total.
    pub max_sprms: usize,
}

impl Default for DocLimits {
    fn default() -> Self {
        Self {
            cfb: CfbLimits::default(),
            max_text_chars: 64 * 1024 * 1024,
            max_pieces: 1_000_000,
            max_fkp_pages: 1_000_000,
            max_styles: 4096,
            max_fonts: 4096,
            max_paragraphs: 1_000_000,
            max_runs: 4_000_000,
            max_sprms: 64 * 1024 * 1024,
        }
    }
}

/// Typed import failure. No partially imported document is returned for these;
/// recoverable damage becomes an [`ImportNotice`] instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocError {
    Cfb(CfbError),
    /// The container has no usable `WordDocument` identifier.
    NotWordDocument,
    MissingStream(&'static str),
    InvalidFib(&'static str),
    /// `fEncrypted` is set (`obfuscated`: XOR obfuscation instead of RC4).
    /// Nothing was read: decryption is out of scope (US-315).
    PasswordProtected {
        obfuscated: bool,
    },
    /// Word 6/95 (`nFib` below 0x00C1) or another unreadable FIB layout.
    UnsupportedVersion {
        n_fib: u16,
    },
    InvalidStructure(&'static str),
    Limit(&'static str),
    Style(String),
    Model(String),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cfb(e) => write!(f, "DOC container error: {e}"),
            Self::NotWordDocument => f.write_str("not a Word binary document"),
            Self::MissingStream(s) => write!(f, "DOC stream {s} is missing"),
            Self::InvalidFib(s) => write!(f, "invalid DOC FIB: {s}"),
            Self::PasswordProtected { obfuscated } => write!(
                f,
                "document protégé par mot de passe (password-protected{})",
                if *obfuscated { ", XOR obfuscation" } else { "" }
            ),
            Self::UnsupportedVersion { n_fib } => {
                write!(f, "unsupported Word version (nFib {n_fib:#06x})")
            }
            Self::InvalidStructure(s) => write!(f, "invalid DOC structure: {s}"),
            Self::Limit(s) => write!(f, "DOC limit exceeded: {s}"),
            Self::Style(s) => write!(f, "DOC style error: {s}"),
            Self::Model(s) => write!(f, "DOC model error: {s}"),
        }
    }
}
impl std::error::Error for DocError {}
impl From<CfbError> for DocError {
    fn from(value: CfbError) -> Self {
        Self::Cfb(value)
    }
}

/// Story of the file that is not imported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRegion {
    Footnotes,
    HeadersFooters,
    Comments,
    Endnotes,
    Textboxes,
    HeaderTextboxes,
}

/// Kind of a style definition (`sgc`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocStyleKind {
    Paragraph,
    Character,
    Table,
    Numbering,
}

/// Information that cannot be represented in the current rich model, or damage
/// that was worked around. Counts are numbers of occurrences in the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportNotice {
    /// Read but not representable; kept as opaque attributes when applicable.
    UnsupportedProperty {
        name: &'static str,
        count: usize,
    },
    /// Mapped to the closest model value.
    ApproximatedProperty {
        name: &'static str,
        count: usize,
    },
    /// Sprms that are not interpreted (skipped by size).
    UnknownSprms {
        count: usize,
    },
    /// A story other than the main document was not imported.
    IgnoredText {
        region: TextRegion,
        characters: usize,
    },
    /// Table cells kept as plain paragraphs (US-313).
    TableCellsFlattened {
        cells: usize,
    },
    /// Field codes dropped, results kept (US-313).
    FieldCodesDropped {
        fields: usize,
    },
    /// Anchors of pictures, drawings, notes or comments, and control
    /// characters, removed from the text.
    ObjectsDropped {
        kind: &'static str,
        count: usize,
    },
    UnsupportedStyleKind {
        istd: u16,
        kind: DocStyleKind,
    },
    InvalidStyleReference {
        istd: u16,
        reason: &'static str,
    },
    /// Simple (`Prm0`) property modifiers of the piece table are not applied.
    SimplePrmIgnored {
        count: usize,
    },
    /// The summary information stream is absent or unreadable.
    SummaryInformation {
        problem: &'static str,
    },
    /// Damaged data skipped; the rest of the file was imported.
    DamagedData {
        what: &'static str,
        count: usize,
    },
}

/// Break kept as a `\n` in the paragraph text (same convention as the DOCX
/// import): `kind` is `"br"` (line, page, column) or `"section"`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedBreak {
    pub paragraph: NodeId,
    pub offset: usize,
    pub kind: String,
    pub break_type: Option<String>,
}

/// Character style applied to `start..end` of a paragraph (byte offsets).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedRunStyle {
    pub paragraph: NodeId,
    pub start: usize,
    pub end: usize,
    pub style_id: Option<String>,
}

/// One font of `SttbfFfn`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocFont {
    pub name: String,
    pub alternate_name: Option<String>,
    /// Windows character set (`chs`).
    pub charset: u8,
    /// Font family (`ff`): 0 unspecified, 1 roman, 2 swiss, 3 modern, 4 script, 5 decorative.
    pub family: u8,
    /// Pitch (`prq`).
    pub pitch: u8,
    pub true_type: bool,
    pub weight: u16,
}

/// A style definition of the sheet, including those that are not imported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleRecord {
    pub istd: u16,
    /// Built-in style identifier (`sti`), 0x0FFE for user styles.
    pub sti: u16,
    pub name: String,
    pub kind: DocStyleKind,
    /// Identifier in the style engine (`None` for table/numbering styles).
    pub id: Option<String>,
    /// `istdBase` after cycle and validity checks.
    pub based_on: Option<u16>,
    /// `istdNext` when it designates an imported style.
    pub next: Option<u16>,
}

/// Word generation identified by the FIB.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocVersion {
    Word97,
    Word2000,
    Word2002,
    Word2003,
    Word2007,
    Other(u16),
}

/// What the FIB says about the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocInfo {
    pub n_fib: u16,
    pub n_fib_new: Option<u16>,
    pub version: DocVersion,
    /// Template (`fDot`).
    pub template: bool,
    /// Fast-saved (`cQuickSaves` > 0) or complex (`fComplex`) file.
    pub fast_saved: bool,
    pub table_stream: &'static str,
    pub lid: u16,
    /// Characters of the main story as declared by the FIB.
    pub main_text_chars: usize,
}

/// Properties of `\x05SummaryInformation`; dates are FILETIME values.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocumentProperties {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub author: Option<String>,
    pub keywords: Option<String>,
    pub comments: Option<String>,
    pub last_author: Option<String>,
    pub application: Option<String>,
    pub created: Option<u64>,
    pub modified: Option<u64>,
    pub last_printed: Option<u64>,
}

impl DocumentProperties {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Seconds since 1970-01-01 UTC of a FILETIME, `None` before that date.
    pub fn filetime_to_unix_seconds(filetime: u64) -> Option<u64> {
        (filetime / 10_000_000).checked_sub(11_644_473_600)
    }
}

/// A fully imported `.doc`.
pub struct ImportedDoc {
    pub document: RichDocument,
    pub styles: StyleEngine,
    pub info: DocInfo,
    pub properties: DocumentProperties,
    pub fonts: Vec<DocFont>,
    pub style_records: Vec<StyleRecord>,
    pub breaks: Vec<ImportedBreak>,
    pub run_styles: Vec<ImportedRunStyle>,
    pub notices: Vec<ImportNotice>,
    paragraph_order: Vec<NodeId>,
}

impl ImportedDoc {
    pub fn paragraph_ids(&self) -> &[NodeId] {
        &self.paragraph_order
    }

    /// Resolves the effective character properties of every run of a paragraph
    /// and its effective paragraph properties (document defaults, style
    /// inheritance, character style, direct formatting), as the DOCX import does.
    pub fn resolved_paragraph(
        &mut self,
        paragraph_id: NodeId,
    ) -> Result<(Paragraph, ResolvedProperties), DocError> {
        let source = self
            .document
            .paragraph(paragraph_id)
            .map_err(|e| DocError::Model(e.to_string()))?
            .clone();
        let default = self
            .styles
            .resolve(&self.document, paragraph_id, None, &CharAttrs::default())
            .map_err(|e| DocError::Style(e.to_string()))?;
        let mut output = source.clone();
        let mut resolved_runs: Vec<atelier_text_model::rich::CharRun> = Vec::new();
        let boundaries = grapheme_boundaries(&source.text);
        let run_styles: Vec<_> = self
            .run_styles
            .iter()
            .filter(|run| run.paragraph == paragraph_id)
            .collect();
        for pair in boundaries.boundaries().windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let direct = source.attrs_at(start);
            let style = run_styles
                .iter()
                .find(|run| run.start <= start && run.end >= end)
                .and_then(|run| run.style_id.as_deref());
            let effective = self
                .styles
                .resolve(&self.document, paragraph_id, style, &direct)
                .map_err(|e| DocError::Style(e.to_string()))?;
            if effective.character.is_empty() {
                continue;
            }
            if let Some(last) = resolved_runs.last_mut() {
                if last.end == start && last.attrs == effective.character {
                    last.end = end;
                    continue;
                }
            }
            resolved_runs.push(atelier_text_model::rich::CharRun {
                start,
                end,
                attrs: effective.character,
            });
        }
        output.runs = resolved_runs;
        output.props = default.paragraph.clone();
        Ok((output, default))
    }
}

/// Imports a `.doc` from bytes with default limits.
pub fn import_doc(bytes: &[u8]) -> Result<ImportedDoc, DocError> {
    import_doc_with_limits(bytes, DocLimits::default())
}

/// Imports a `.doc` from bytes with explicit, deterministic budgets.
pub fn import_doc_with_limits(bytes: &[u8], limits: DocLimits) -> Result<ImportedDoc, DocError> {
    let cfb = Compound::open(bytes, limits.cfb)?;
    build::import(&cfb, &limits)
}
