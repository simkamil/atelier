// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded semantic import of OpenDocument text documents (US-310).
//!
//! The entry points mirror the DOCX import: [`import_odt`] and
//! [`import_odt_with_limits`] return an [`ImportedOdt`] holding the validated
//! `RichDocument`, the US-201 `StyleEngine` and typed side tables for what the
//! rich model cannot carry (list definitions, page geometry, image metadata,
//! next-style links, ...). Everything unsupported is reported in `notices`.
//! ODF is read-only: nothing here writes ODF.

mod body;
mod styles;
mod tree;

use atelier_text_model::rich::{
    CharAttrs, NodeId, Paragraph, ResolvedProperties, RichDocument, StyleEngine,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use atelier_xml::Limits as XmlLimits;
use atelier_zip::{Archive, Limits as ZipLimits, ZipError};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use styles::{Scope, Styles};
use tree::*;

pub use styles::NS_IMPORT;

const MIME_TEXT: &str = "application/vnd.oasis.opendocument.text";
const MIME_TEMPLATE: &str = "application/vnd.oasis.opendocument.text-template";
const MIME_PREFIX: &str = "application/vnd.oasis.opendocument.";
const EMPTY_CONTENT: &str = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"><office:body><office:text/></office:body></office:document-content>"#;

/// ODT-specific ceilings applied in addition to the ZIP and XML budgets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OdtLimits {
    pub zip: ZipLimits,
    pub xml: XmlLimits,
    /// Largest XML part read (bytes, uncompressed).
    pub max_part_bytes: usize,
    pub max_paragraphs: usize,
    pub max_text_bytes: usize,
    pub max_styles: usize,
    /// Maximum nesting of lists, tables and sections.
    pub max_nesting: usize,
    /// Total grid cells and rows over all tables.
    pub max_table_cells: usize,
    pub max_images: usize,
    pub max_image_bytes: usize,
    pub max_total_image_bytes: usize,
    pub max_notices: usize,
}

impl Default for OdtLimits {
    fn default() -> Self {
        Self {
            zip: ZipLimits::default(),
            xml: XmlLimits::default(),
            max_part_bytes: 128 * 1024 * 1024,
            max_paragraphs: 1_000_000,
            max_text_bytes: 256 * 1024 * 1024,
            max_styles: 100_000,
            max_nesting: 32,
            max_table_cells: 1_000_000,
            max_images: 10_000,
            max_image_bytes: 64 * 1024 * 1024,
            max_total_image_bytes: 256 * 1024 * 1024,
            max_notices: 10_000,
        }
    }
}

/// Typed import failure. No partially imported document is returned for these;
/// recoverable problems become [`ImportNotice`]s instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OdtError {
    Zip(ZipError),
    /// The input is neither an ODF package nor a flat ODF document.
    NotOdf(&'static str),
    /// A valid ODF document of another kind (spreadsheet, presentation, ...).
    UnsupportedDocumentType {
        mime_type: String,
    },
    /// The manifest declares encrypted entries.
    PasswordProtected,
    MissingPart(&'static str),
    Xml {
        part: String,
        message: String,
    },
    InvalidStructure {
        part: String,
        detail: &'static str,
    },
    Limit(&'static str),
    Style(String),
    Model(String),
}

impl fmt::Display for OdtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(e) => write!(f, "ODT import error: {e}"),
            Self::NotOdf(d) => write!(f, "ODT import error: not an OpenDocument file ({d})"),
            Self::UnsupportedDocumentType { mime_type } => {
                write!(f, "ODT import error: unsupported document type {mime_type}")
            }
            Self::PasswordProtected => {
                f.write_str("ODT import error: document is password protected")
            }
            _ => write!(f, "ODT import error: {self:?}"),
        }
    }
}
impl std::error::Error for OdtError {}
impl From<ZipError> for OdtError {
    fn from(value: ZipError) -> Self {
        Self::Zip(value)
    }
}

/// Information that cannot be represented, or was repaired, during import.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ImportNotice {
    /// A part is absent (`styles.xml`, `meta.xml`, manifest, `mimetype`).
    MissingPart {
        part: String,
    },
    /// A non-essential part could not be read and was skipped.
    UnreadablePart {
        part: String,
        message: String,
    },
    /// The ZIP central directory was damaged; entries were recovered locally.
    RepairedArchive,
    UnsupportedElement {
        part: String,
        name: String,
    },
    /// A property was read but the model has no slot for it; it is kept as an
    /// opaque attribute (namespace of the ODF property).
    UnsupportedProperty {
        style: String,
        property: String,
        value: String,
    },
    UnknownStyle {
        family: String,
        name: String,
    },
    StyleCycle {
        name: String,
    },
    NestingLimit {
        what: &'static str,
    },
    FlattenedSection {
        name: String,
    },
    NoteContentIgnored {
        id: String,
    },
    TrackedChangesIgnored,
    /// Numbering definitions are in [`ImportedOdt::lists`], not in the model.
    ListNumbering {
        style: String,
    },
    /// An object kept as an opaque anchored object.
    ObjectKept {
        kind: String,
    },
    ImageUnavailable {
        href: String,
        reason: String,
    },
    FieldReplacedByResult {
        name: String,
    },
    DuplicateBookmark {
        name: String,
    },
    BookmarkSpansParagraphs {
        name: String,
    },
    OverlappingLink,
    /// Further notices were dropped (`OdtLimits::max_notices`).
    TooManyNotices,
}

pub(crate) struct Notices {
    list: Vec<ImportNotice>,
    seen: HashSet<ImportNotice>,
    max: usize,
    truncated: bool,
}

impl Notices {
    fn new(max: usize) -> Self {
        Self {
            list: Vec::new(),
            seen: HashSet::new(),
            max,
            truncated: false,
        }
    }
    pub fn add(&mut self, n: ImportNotice) {
        if self.seen.contains(&n) {
            return;
        }
        if self.list.len() >= self.max {
            self.truncated = true;
            return;
        }
        self.seen.insert(n.clone());
        self.list.push(n);
    }
}

/// Character style reference of a text range (named character styles are not
/// representable in `CharRun`; same technique as the DOCX import).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedRunStyle {
    pub paragraph: NodeId,
    pub start: usize,
    pub end: usize,
    pub style_id: Option<String>,
}

/// Page geometry in twips; the rich model has no page-setup slot, so it is
/// also stored as import-namespace opaque attributes of the section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageSetup {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub landscape: Option<bool>,
    pub margin_top: Option<i32>,
    pub margin_bottom: Option<i32>,
    pub margin_left: Option<i32>,
    pub margin_right: Option<i32>,
}

impl PageSetup {
    pub(crate) fn entries(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        let mut add = |k: &'static str, o: Option<i32>| {
            if let Some(x) = o {
                v.push((k, x.to_string()));
            }
        };
        add("page-width", self.width);
        add("page-height", self.height);
        add("margin-top", self.margin_top);
        add("margin-bottom", self.margin_bottom);
        add("margin-left", self.margin_left);
        add("margin-right", self.margin_right);
        if let Some(l) = self.landscape {
            v.push((
                "orientation",
                if l { "landscape" } else { "portrait" }.to_owned(),
            ));
        }
        v
    }
}

/// A section with the master page it was created from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedSection {
    pub section: NodeId,
    pub master_page: Option<String>,
    pub page: PageSetup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListLevelKind {
    Bullet(String),
    Number {
        format: String,
        prefix: String,
        suffix: String,
        start: u32,
    },
    Image,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedListLevel {
    /// Zero-based level.
    pub level: u8,
    pub kind: ListLevelKind,
    pub space_before: Option<i32>,
    pub min_label_width: Option<i32>,
}

/// Definition behind a `ListRef::list_id` used by the imported paragraphs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedList {
    pub list_id: u32,
    pub style_name: Option<String>,
    pub levels: Vec<ImportedListLevel>,
}

/// Per-paragraph list information the model cannot express.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedListItem {
    pub paragraph: NodeId,
    pub list_id: u32,
    pub level: u8,
    /// Paragraph of a `text:list-header` (not numbered).
    pub header: bool,
    pub start_value: Option<u32>,
    /// Second or later paragraph of one `text:list-item` (not numbered in ODF).
    pub continuation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageAnchor {
    Char,
    AsChar,
    Paragraph,
    Page,
    Frame,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageStatus {
    /// Bytes read from the package.
    Embedded,
    /// Bytes decoded from `office:binary-data` (flat documents).
    InlineBinary,
    /// External link: never fetched.
    External,
    /// Referenced part absent, unreadable or too large.
    Missing,
}

/// Metadata of a picture; its bytes are the `data` of the anchored object
/// `object` in `paragraph`, whose `kind` is the media type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedImage {
    pub paragraph: NodeId,
    pub object: NodeId,
    pub href: Option<String>,
    pub media_type: Option<String>,
    pub status: ImageStatus,
    pub anchor: ImageAnchor,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OdtMetadata {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub initial_creator: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub language: Option<String>,
    pub generator: Option<String>,
    /// ISO 8601 text as stored in the file.
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub page_count: Option<u32>,
    pub word_count: Option<u32>,
    pub character_count: Option<u32>,
    pub paragraph_count: Option<u32>,
    pub table_count: Option<u32>,
    pub image_count: Option<u32>,
}

/// A fully imported ODT.
pub struct ImportedOdt {
    pub document: RichDocument,
    pub styles: StyleEngine,
    pub mime_type: String,
    /// True for `.ott` templates.
    pub is_template: bool,
    /// Character styles of text ranges.
    pub run_styles: Vec<ImportedRunStyle>,
    pub sections: Vec<ImportedSection>,
    pub lists: Vec<ImportedList>,
    pub list_items: Vec<ImportedListItem>,
    pub images: Vec<ImportedImage>,
    /// Outline level of each `text:h`.
    pub headings: Vec<(NodeId, u8)>,
    /// Model style id -> ODF display name.
    pub style_names: BTreeMap<String, String>,
    /// Model style id -> id of the style applied to the next paragraph.
    pub next_styles: BTreeMap<String, String>,
    pub metadata: OdtMetadata,
    pub notices: Vec<ImportNotice>,
    paragraph_order: Vec<NodeId>,
}

impl ImportedOdt {
    /// Body paragraphs (tables and lists included, headers/footers excluded) in
    /// document order.
    pub fn paragraph_ids(&self) -> &[NodeId] {
        &self.paragraph_order
    }

    /// Resolve paragraph and run properties through the US-201 style engine.
    /// Runs carry effective character properties; the second value holds the
    /// effective paragraph properties.
    pub fn resolved_paragraph(
        &mut self,
        paragraph_id: NodeId,
    ) -> Result<(Paragraph, ResolvedProperties), OdtError> {
        let source = self
            .document
            .paragraph(paragraph_id)
            .map_err(|e| OdtError::Model(e.to_string()))?
            .clone();
        let default = self
            .styles
            .resolve(&self.document, paragraph_id, None, &CharAttrs::default())
            .map_err(|e| OdtError::Style(e.to_string()))?;
        let mut output = source.clone();
        let mut resolved_runs: Vec<atelier_text_model::rich::CharRun> = Vec::new();
        let boundaries = grapheme_boundaries(&source.text);
        let run_styles: Vec<&ImportedRunStyle> = self
            .run_styles
            .iter()
            .filter(|r| r.paragraph == paragraph_id)
            .collect();
        let mut cursor = 0;
        for pair in boundaries.boundaries().windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let direct = source.attrs_at(start);
            while cursor < run_styles.len() && run_styles[cursor].end <= start {
                cursor += 1;
            }
            let style = run_styles
                .get(cursor)
                .filter(|r| r.start <= start && r.end >= end)
                .and_then(|r| r.style_id.as_deref());
            let effective = self
                .styles
                .resolve(&self.document, paragraph_id, style, &direct)
                .map_err(|e| OdtError::Style(e.to_string()))?;
            if !effective.character.is_empty() {
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
        }
        output.runs = resolved_runs;
        output.props = default.paragraph.clone();
        Ok((output, default))
    }
}

/// Import an ODT/OTT/FODT from bytes with default resource limits.
pub fn import_odt(bytes: &[u8]) -> Result<ImportedOdt, OdtError> {
    import_odt_with_limits(bytes, OdtLimits::default())
}

fn is_flat(bytes: &[u8]) -> bool {
    let b = bytes.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(bytes);
    b.iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'<')
}

/// Import with explicit, deterministic budgets.
pub fn import_odt_with_limits(bytes: &[u8], limits: OdtLimits) -> Result<ImportedOdt, OdtError> {
    let mut notices = Notices::new(limits.max_notices);
    let mut styles = Styles::default();
    if is_flat(bytes) {
        let root = parse_part(bytes, "document", &limits)?;
        if !root.is(NS_OFFICE, "document") {
            return Err(OdtError::NotOdf("root element is not office:document"));
        }
        let mime = root
            .attr(NS_OFFICE, "mimetype")
            .unwrap_or(MIME_TEXT)
            .trim()
            .to_owned();
        check_mime(&mime)?;
        styles
            .absorb(&root, Scope::Content, limits.max_styles)
            .map_err(OdtError::Limit)?;
        styles.auto[1] = styles.auto[0].clone();
        styles.auto_lists[1] = styles.auto_lists[0].clone();
        return assemble(
            &root,
            Some(&root),
            mime,
            styles,
            None,
            &HashMap::new(),
            &limits,
            notices,
        );
    }
    let archive = Archive::open(bytes, limits.zip).map_err(OdtError::Zip)?;
    if !archive.is_complete {
        notices.add(ImportNotice::RepairedArchive);
    }
    let read =
        |name: &str, notices: &mut Notices, required: bool| -> Result<Option<Vec<u8>>, OdtError> {
            let Some(e) = archive.entry(name) else {
                if !required {
                    notices.add(ImportNotice::MissingPart { part: name.into() });
                }
                return Ok(None);
            };
            if e.uncompressed_size() > limits.max_part_bytes as u64 {
                return Err(OdtError::Limit("part bytes"));
            }
            match e.read_all() {
                Ok(v) => Ok(Some(v)),
                Err(err) if required => Err(OdtError::Xml {
                    part: name.into(),
                    message: format!("{err}"),
                }),
                Err(err) => {
                    notices.add(ImportNotice::UnreadablePart {
                        part: name.into(),
                        message: format!("{err}"),
                    });
                    Ok(None)
                }
            }
        };
    // Manifest: encryption and media types.
    let mut types: HashMap<String, String> = HashMap::new();
    let mut manifest_root_type: Option<String> = None;
    if let Some(m) = read("META-INF/manifest.xml", &mut notices, false)? {
        match parse_part(&m, "META-INF/manifest.xml", &limits) {
            Ok(root) => {
                for entry in root.elements().filter(|e| e.is(NS_MANIFEST, "file-entry")) {
                    if has_encryption(entry) {
                        return Err(OdtError::PasswordProtected);
                    }
                    if let (Some(path), Some(t)) = (
                        entry.attr(NS_MANIFEST, "full-path"),
                        entry.attr(NS_MANIFEST, "media-type"),
                    ) {
                        if path == "/" {
                            manifest_root_type = Some(t.trim().to_owned());
                        } else if !t.is_empty() {
                            types.insert(path.to_owned(), t.to_owned());
                        }
                    }
                }
            }
            Err(OdtError::Xml { part, message }) => {
                notices.add(ImportNotice::UnreadablePart { part, message })
            }
            Err(e) => return Err(e),
        }
    }
    let mimetype = read("mimetype", &mut notices, false)?
        .map(|v| String::from_utf8_lossy(&v).trim().to_owned());
    let mime = match (mimetype, manifest_root_type) {
        (Some(m), _) if !m.is_empty() => m,
        (_, Some(m)) => m,
        _ => {
            if archive.entry("content.xml").is_none() {
                return Err(OdtError::NotOdf("no mimetype entry and no content.xml"));
            }
            MIME_TEXT.to_owned()
        }
    };
    check_mime(&mime)?;
    // styles.xml first: its automatic styles live in their own scope.
    if let Some(b) = read("styles.xml", &mut notices, false)? {
        match parse_part(&b, "styles.xml", &limits) {
            Ok(root) => styles
                .absorb(&root, Scope::Styles, limits.max_styles)
                .map_err(OdtError::Limit)?,
            Err(OdtError::Xml { part, message }) => {
                notices.add(ImportNotice::UnreadablePart { part, message });
            }
            Err(e) => return Err(e),
        }
    }
    // A package without content.xml (a style-only template) opens as an empty document.
    let root = if archive.entry("content.xml").is_some() {
        let content_bytes =
            read("content.xml", &mut notices, true)?.ok_or(OdtError::MissingPart("content.xml"))?;
        parse_part(&content_bytes, "content.xml", &limits)?
    } else {
        notices.add(ImportNotice::MissingPart {
            part: "content.xml".into(),
        });
        parse_part(EMPTY_CONTENT.as_bytes(), "content.xml", &limits)?
    };
    if !root.is(NS_OFFICE, "document-content") {
        return Err(OdtError::InvalidStructure {
            part: "content.xml".into(),
            detail: "root element is not office:document-content",
        });
    }
    styles
        .absorb(&root, Scope::Content, limits.max_styles)
        .map_err(OdtError::Limit)?;
    let meta = match read("meta.xml", &mut notices, false)? {
        Some(b) => match parse_part(&b, "meta.xml", &limits) {
            Ok(r) => Some(r),
            Err(OdtError::Xml { part, message }) => {
                notices.add(ImportNotice::UnreadablePart { part, message });
                None
            }
            Err(e) => return Err(e),
        },
        None => None,
    };
    assemble(
        &root,
        meta.as_ref(),
        mime,
        styles,
        Some(&archive),
        &types,
        &limits,
        notices,
    )
}

fn has_encryption(n: &XNode) -> bool {
    n.elements()
        .any(|c| c.is(NS_MANIFEST, "encryption-data") || has_encryption(c))
}

fn check_mime(mime: &str) -> Result<(), OdtError> {
    if mime == MIME_TEXT || mime == MIME_TEMPLATE {
        Ok(())
    } else if mime.starts_with("application/vnd.") && mime.len() < 200 {
        Err(OdtError::UnsupportedDocumentType {
            mime_type: mime.to_owned(),
        })
    } else {
        Err(OdtError::NotOdf("unexpected mimetype"))
    }
}

fn parse_part(bytes: &[u8], part: &str, limits: &OdtLimits) -> Result<XNode, OdtError> {
    if bytes.len() > limits.max_part_bytes {
        return Err(OdtError::Limit("part bytes"));
    }
    tree::parse(bytes, &limits.xml).map_err(|message| OdtError::Xml {
        part: part.to_owned(),
        message,
    })
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    content: &XNode,
    meta: Option<&XNode>,
    mime: String,
    styles: Styles,
    archive: Option<&Archive<'_>>,
    media_types: &HashMap<String, String>,
    limits: &OdtLimits,
    mut notices: Notices,
) -> Result<ImportedOdt, OdtError> {
    let body = content
        .find(NS_OFFICE, "body")
        .ok_or(OdtError::InvalidStructure {
            part: "content.xml".into(),
            detail: "office:body is missing",
        })?;
    let Some(text) = body.find(NS_OFFICE, "text") else {
        let kind = body
            .elements()
            .next()
            .map(|e| e.local.clone())
            .unwrap_or_default();
        return Err(match kind.as_str() {
            "spreadsheet" | "presentation" | "drawing" | "chart" | "image" | "database" => {
                OdtError::UnsupportedDocumentType {
                    mime_type: format!("{MIME_PREFIX}{kind}"),
                }
            }
            _ => OdtError::InvalidStructure {
                part: "content.xml".into(),
                detail: "office:body has no office:text",
            },
        });
    };
    let out = {
        let mut importer =
            body::Importer::new(limits, &styles, archive, media_types, &mut notices)?;
        importer.run(text)?;
        importer.finish()?
    };
    if notices.truncated {
        notices.list.push(ImportNotice::TooManyNotices);
    }
    let metadata = meta.map(read_metadata).unwrap_or_default();
    Ok(ImportedOdt {
        is_template: mime == MIME_TEMPLATE,
        mime_type: mime,
        document: out.document,
        styles: out.engine,
        run_styles: out.run_styles,
        sections: out.sections,
        lists: out.lists,
        list_items: out.list_items,
        images: out.images,
        headings: out.headings,
        style_names: out.style_names,
        next_styles: out.next_styles,
        metadata,
        notices: notices.list,
        paragraph_order: out.paragraph_order,
    })
}

fn read_metadata(root: &XNode) -> OdtMetadata {
    let mut m = OdtMetadata::default();
    let Some(meta) = root.find(NS_OFFICE, "meta") else {
        return m;
    };
    let text = |n: &XNode| {
        let t = n.text_content().trim().to_owned();
        (!t.is_empty()).then_some(t)
    };
    let num = |n: &XNode, a: &str| {
        n.attr(NS_META, a)
            .and_then(|v| v.trim().parse::<u32>().ok())
    };
    for e in meta.elements() {
        match (e.ns.as_str(), e.local.as_str()) {
            (NS_DC, "title") => m.title = text(e),
            (NS_DC, "subject") => m.subject = text(e),
            (NS_DC, "creator") => m.creator = text(e),
            (NS_DC, "description") => m.description = text(e),
            (NS_DC, "language") => m.language = text(e),
            (NS_DC, "date") => m.modification_date = text(e),
            (NS_META, "initial-creator") => m.initial_creator = text(e),
            (NS_META, "creation-date") => m.creation_date = text(e),
            (NS_META, "generator") => m.generator = text(e),
            (NS_META, "keyword") => {
                if let Some(k) = text(e) {
                    m.keywords.push(k);
                }
            }
            (NS_META, "document-statistic") => {
                m.page_count = num(e, "page-count");
                m.word_count = num(e, "word-count");
                m.character_count = num(e, "character-count");
                m.paragraph_count = num(e, "paragraph-count");
                m.table_count = num(e, "table-count");
                m.image_count = num(e, "image-count");
            }
            _ => {}
        }
    }
    m
}
