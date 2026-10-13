// SPDX-License-Identifier: AGPL-3.0-or-later
//! OPC package reader and deterministic writer used by OOXML formats.
//!
//! The crate deliberately models a package as parts plus content types and
//! relationships. It does not interpret WordprocessingML document content.

#![forbid(unsafe_code)]

pub mod docx;
mod properties;
pub use docx::{
    Border, Borders, CellMargins, CellProps, CellVerticalAlign, ConditionalFormat, ConditionalKind,
    DocxError, DocxLimits, DocxSettings, EmbeddedFontRef, FontRecord, HeightRule, ImportNotice,
    ImportedBreak, ImportedCell, ImportedDocx, ImportedRow, ImportedRunStyle, ImportedSymbol,
    ImportedTable, ImportedTables, LatentStyleInfo, MC_NS, Preserved, PreservedAttr,
    PreservedChild, PreservedElement, PreservedPart, ResolvedCell, RowHeight, RowProps, Shading,
    TableAlign, TableColor, TableLayout, TableLook, TableProps, TableStyleDef, TableWidth, VMerge,
    import_docx, import_docx_with_limits, interpreted_elements, para_mark_rpr_rank, ppr_rank,
    rpr_rank, sect_pr_rank,
};

pub use properties::{
    AppExtra, ApplyReport, CoreExtra, DateError, DatePrecision, DocumentStatistics,
    EditableProperties, MAX_COMMENTS_CHARS, MAX_HYPERLINK_BASE_CHARS, MAX_KEYWORDS_CHARS,
    MAX_LANGUAGE_CHARS, MAX_LINE_CHARS, PdfDateFields, PdfMetadata, PropertiesSnapshot,
    PropertyChanges, PropertyDate, PropertyError, PropertyNotice, ReadOnlyProperties, SaveContext,
    SaveReport, W3cDateTime,
};

use atelier_xml::{Attribute, Event, Limits as XmlLimits, Reader};
use atelier_zip::{Archive, Limits as ZipLimits, Method, WriteOptions, Writer as ZipWriter};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::io::Cursor;

const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CORE_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const DCTERMS_NS: &str = "http://purl.org/dc/terms/";
const APP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const REL_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CORE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const APP_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CORE_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.extended-properties+xml";

/// Resource limits for package metadata. ZIP member limits are applied in
/// addition to these XML and aggregate limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    pub zip: ZipLimits,
    pub xml: XmlLimits,
    pub max_xml_part_bytes: usize,
    pub max_total_part_bytes: u64,
    pub max_parts: usize,
    pub max_relationships: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            zip: ZipLimits::default(),
            xml: XmlLimits::default(),
            max_xml_part_bytes: 16 * 1024 * 1024,
            max_total_part_bytes: 1024 * 1024 * 1024,
            max_parts: 10_000,
            max_relationships: 100_000,
        }
    }
}

/// A bounded, typed OPC diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpcError {
    Zip(String),
    Xml(String),
    InvalidPackage(&'static str),
    InvalidUri(String),
    Duplicate(String),
    Missing(String),
    Limit(&'static str),
    InvalidRelationship(String),
}
impl std::fmt::Display for OpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OPC error: {self:?}")
    }
}
impl std::error::Error for OpcError {}
impl From<atelier_zip::ZipError> for OpcError {
    fn from(e: atelier_zip::ZipError) -> Self {
        Self::Zip(e.to_string())
    }
}
impl From<atelier_xml::XmlError> for OpcError {
    fn from(e: atelier_xml::XmlError) -> Self {
        Self::Xml(e.to_string())
    }
}

/// An OPC part URI, canonicalized as a slash-separated path without the
/// package-leading slash (for example `word/document.xml`).
#[derive(Clone, Debug)]
pub struct PartUri(String);
impl PartUri {
    pub fn parse(value: &str) -> Result<Self, OpcError> {
        Self::parse_with_relationship_metadata(value, false)
    }

    fn parse_with_relationship_metadata(
        value: &str,
        allow_relationship_metadata: bool,
    ) -> Result<Self, OpcError> {
        let value = value.strip_prefix('/').unwrap_or(value);
        if value.is_empty()
            || value.starts_with('/')
            || value.contains(['\\', '?', '#', ':'])
            || value.contains('\0')
        {
            return Err(OpcError::InvalidUri(value.to_owned()));
        }
        let mut parts = Vec::new();
        for segment in value.split('/') {
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.ends_with('.')
                || segment.bytes().any(|b| b <= 0x20 || b == 0x7f)
            {
                return Err(OpcError::InvalidUri(value.to_owned()));
            }
            if !valid_iri_segment(segment) {
                return Err(OpcError::InvalidUri(value.to_owned()));
            }
            if segment.contains('%') {
                let bytes = segment.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] == b'%' {
                        if i + 2 >= bytes.len()
                            || !bytes[i + 1].is_ascii_hexdigit()
                            || !bytes[i + 2].is_ascii_hexdigit()
                        {
                            return Err(OpcError::InvalidUri(value.to_owned()));
                        }
                        let code = u8::from_str_radix(&segment[i + 1..i + 3], 16)
                            .map_err(|_| OpcError::InvalidUri(value.to_owned()))?;
                        if code == b'/' || code == b'\\' || code == 0 {
                            return Err(OpcError::InvalidUri(value.to_owned()));
                        }
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
                reject_unreserved_escapes(segment, value)?;
            }
            parts.push(segment);
        }
        if !allow_relationship_metadata && is_reserved_relationship_part(&parts) {
            return Err(OpcError::InvalidUri(value.to_owned()));
        }
        Ok(Self(parts.join("/")))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn rels_path(&self) -> String {
        let (dir, name) = self.0.rsplit_once('/').unwrap_or(("", self.0.as_str()));
        if dir.is_empty() {
            format!("_rels/{name}.rels")
        } else {
            format!("{dir}/_rels/{name}.rels")
        }
    }
}
impl PartialEq for PartUri {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}
impl Eq for PartUri {}
impl PartialOrd for PartUri {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PartUri {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(other.0.bytes().map(|b| b.to_ascii_lowercase()))
    }
}
impl Hash for PartUri {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            state.write_u8(byte.to_ascii_lowercase());
        }
    }
}

fn ascii_fold(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}
fn is_reserved_relationship_part(segments: &[&str]) -> bool {
    if segments.len() == 2
        && segments[0].eq_ignore_ascii_case("_rels")
        && segments[1].eq_ignore_ascii_case(".rels")
    {
        return true;
    }
    segments.len() >= 2
        && segments[segments.len() - 2].eq_ignore_ascii_case("_rels")
        && segments[segments.len() - 1]
            .to_ascii_lowercase()
            .ends_with(".rels")
}

fn reject_unreserved_escapes(segment: &str, original: &str) -> Result<(), OpcError> {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let mut escaped = Vec::new();
        while index + 2 < bytes.len() && bytes[index] == b'%' {
            let byte = u8::from_str_radix(&segment[index + 1..index + 3], 16).unwrap_or(0);
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                return Err(OpcError::InvalidUri(original.to_owned()));
            }
            escaped.push(byte);
            index += 3;
        }
        if let Ok(text) = std::str::from_utf8(&escaped) {
            if text.chars().any(is_iri_unreserved) {
                return Err(OpcError::InvalidUri(original.to_owned()));
            }
        }
    }
    Ok(())
}

fn valid_iri_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        let ch = segment[index..].chars().next().unwrap();
        if ch.is_ascii() {
            if !(ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '-' | '.'
                        | '_'
                        | '~'
                        | '!'
                        | '$'
                        | '&'
                        | '\''
                        | '('
                        | ')'
                        | '*'
                        | '+'
                        | ','
                        | ';'
                        | '='
                        | '@'
                ))
            {
                return false;
            }
        } else if !is_iri_unreserved(ch) {
            return false;
        }
        index += ch.len_utf8();
    }
    true
}
fn is_iri_unreserved(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(ch, '-' | '.' | '_' | '~')
        || matches!(ch as u32,
            0x00A0..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFEF
            | 0x10000..=0x1FFFD | 0x20000..=0x2FFFD | 0x30000..=0x3FFFD
            | 0x40000..=0x4FFFD | 0x50000..=0x5FFFD | 0x60000..=0x6FFFD
            | 0x70000..=0x7FFFD | 0x80000..=0x8FFFD | 0x90000..=0x9FFFD
            | 0xA0000..=0xAFFFD | 0xB0000..=0xBFFFD | 0xC0000..=0xCFFFD
            | 0xD0000..=0xDFFFD | 0xE1000..=0xEFFFD)
}
impl std::fmt::Display for PartUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A part's bytes are opaque to this layer. `content_type` is resolved from
/// `[Content_Types].xml` when read, and emitted as an override when written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Part {
    pub uri: PartUri,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Target mode from the OPC relationship markup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetMode {
    Internal,
    External,
}
/// A relation retains its original target text; internal targets also have a
/// normalized part URI. External targets are never fetched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub mode: TargetMode,
    pub resolved_target: Option<PartUri>,
}

/// Common properties exposed by the package layer. Unknown XML children in
/// core/app properties are retained as validated opaque XML fragments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoreProperties {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub keywords: Option<String>,
    pub description: Option<String>,
    pub last_modified_by: Option<String>,
    pub revision: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub category: Option<String>,
    pub content_status: Option<String>,
    pub identifier: Option<String>,
    pub language: Option<String>,
    pub version: Option<String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppProperties {
    pub application: Option<String>,
    pub app_version: Option<String>,
    pub company: Option<String>,
    pub manager: Option<String>,
    pub pages: Option<String>,
    pub words: Option<String>,
    pub characters: Option<String>,
    pub paragraphs: Option<String>,
    pub lines: Option<String>,
    pub template: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Package {
    parts: BTreeMap<PartUri, Part>,
    relationships: BTreeMap<Option<PartUri>, Vec<Relationship>>,
    defaults: BTreeMap<String, String>,
    core: CoreProperties,
    app: AppProperties,
    core_uri: Option<PartUri>,
    app_uri: Option<PartUri>,
    core_dirty: bool,
    app_dirty: bool,
    core_extra: CoreExtra,
    app_extra: AppExtra,
    /// The property part was found by content type, without a relationship.
    core_orphan: bool,
    app_orphan: bool,
}
impl Package {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn parts(&self) -> &BTreeMap<PartUri, Part> {
        &self.parts
    }
    pub fn part(&self, uri: &PartUri) -> Option<&Part> {
        self.parts.get(uri)
    }
    pub fn relationships(&self, source: Option<&PartUri>) -> &[Relationship] {
        self.relationships
            .get(&source.cloned())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn core_properties(&self) -> &CoreProperties {
        &self.core
    }
    pub fn core_properties_mut(&mut self) -> &mut CoreProperties {
        self.core_dirty = true;
        &mut self.core
    }
    pub fn app_properties(&self) -> &AppProperties {
        &self.app
    }
    pub fn app_properties_mut(&mut self) -> &mut AppProperties {
        self.app_dirty = true;
        &mut self.app
    }
    pub fn insert_part(
        &mut self,
        uri: PartUri,
        content_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<(), OpcError> {
        let content_type = content_type.into();
        if !valid_content_type(&content_type) {
            return Err(OpcError::InvalidPackage("invalid content type"));
        }
        if self.parts.contains_key(&uri) {
            return Err(OpcError::Duplicate(uri.to_string()));
        }
        if self
            .parts
            .keys()
            .any(|existing| part_names_conflict(existing, &uri))
        {
            return Err(OpcError::Duplicate(uri.to_string()));
        }
        self.parts.insert(
            uri.clone(),
            Part {
                uri,
                content_type,
                bytes,
            },
        );
        Ok(())
    }
    pub fn add_relationship(
        &mut self,
        source: Option<PartUri>,
        id: impl Into<String>,
        rel_type: impl Into<String>,
        target: impl Into<String>,
        mode: TargetMode,
    ) -> Result<(), OpcError> {
        let id = id.into();
        let rel_type = rel_type.into();
        let target = target.into();
        if id.is_empty()
            || validate_iri_reference(&rel_type).is_err()
            || id.chars().any(char::is_whitespace)
        {
            return Err(OpcError::InvalidRelationship(id));
        }
        let resolved_target = match mode {
            TargetMode::External => {
                validate_iri_reference(&target)?;
                None
            }
            TargetMode::Internal => Some(resolve_target(source.as_ref(), &target)?),
        };
        if self
            .relationships
            .get(&source)
            .is_some_and(|rels| rels.iter().any(|r| r.id == id))
        {
            return Err(OpcError::Duplicate(id));
        }
        if source.is_none() {
            match rel_type.as_str() {
                CORE_REL => self.core_uri = resolved_target.clone(),
                APP_REL => self.app_uri = resolved_target.clone(),
                _ => {}
            }
        }
        let list = self.relationships.entry(source).or_default();
        list.push(Relationship {
            id,
            rel_type,
            target,
            mode,
            resolved_target,
        });
        list.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
    }

    pub fn read(bytes: &[u8]) -> Result<Self, OpcError> {
        Self::read_with_limits(bytes, Limits::default())
    }
    pub fn read_with_limits(bytes: &[u8], limits: Limits) -> Result<Self, OpcError> {
        let archive = Archive::open(bytes, limits.zip).map_err(OpcError::from)?;
        if !archive.is_complete {
            return Err(OpcError::InvalidPackage("incomplete ZIP archive"));
        }
        let mut raw = BTreeMap::<String, Vec<u8>>::new();
        let mut total = 0u64;
        for entry in archive.entries() {
            if entry.error().is_some() || entry.is_duplicate() || !entry.is_name_safe() {
                return Err(OpcError::InvalidPackage("unsafe or duplicate ZIP member"));
            }
            if entry.name().ends_with('/') {
                if entry.uncompressed_size() != 0 {
                    return Err(OpcError::InvalidPackage(
                        "directory ZIP member contains data",
                    ));
                }
                continue;
            }
            let raw_entry_limit = limits
                .max_parts
                .saturating_add(limits.max_relationships)
                .saturating_add(1);
            if raw.len() >= raw_entry_limit {
                return Err(OpcError::Limit("part count"));
            }
            let n = entry.uncompressed_size();
            total = total
                .checked_add(n)
                .ok_or(OpcError::Limit("total part bytes"))?;
            if total > limits.max_total_part_bytes {
                return Err(OpcError::Limit("total part bytes"));
            }
            raw.insert(
                entry.name().to_owned(),
                entry.read_all().map_err(|e| OpcError::Zip(e.to_string()))?,
            );
        }
        let ct = raw
            .remove(CONTENT_TYPES_PATH)
            .ok_or_else(|| OpcError::Missing(CONTENT_TYPES_PATH.into()))?;
        if ct.len() > limits.max_xml_part_bytes {
            return Err(OpcError::Limit("XML part bytes"));
        }
        let (defaults, overrides) = parse_content_types(&ct, limits.xml.clone())?;
        let mut package = Package {
            defaults,
            ..Package::default()
        };
        let mut relationship_part_names = BTreeSet::new();
        let mut relationship_count = 0usize;
        for (name, part_bytes) in raw {
            if is_relationship_part_name(&name) {
                let folded_name = ascii_fold(&name);
                if !relationship_part_names.insert(folded_name.clone()) {
                    return Err(OpcError::Duplicate(name));
                }
                let rel_type = overrides.get(&folded_name).cloned().or_else(|| {
                    extension(&name)
                        .and_then(|e| package.defaults.get(&e.to_ascii_lowercase()).cloned())
                });
                if !rel_type
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(REL_CONTENT_TYPE))
                {
                    return Err(OpcError::InvalidPackage(
                        "relationship part has an invalid content type",
                    ));
                }
                if part_bytes.len() > limits.max_xml_part_bytes {
                    return Err(OpcError::Limit("XML part bytes"));
                }
                let source = relationship_source(&name)?;
                let rels = parse_relationships(&part_bytes, source.as_ref(), limits.xml.clone())?;
                relationship_count = relationship_count.saturating_add(rels.len());
                if relationship_count > limits.max_relationships {
                    return Err(OpcError::Limit("relationship count"));
                }
                if package.relationships.contains_key(&source) {
                    return Err(OpcError::Duplicate(name));
                }
                package.relationships.insert(source, rels);
                continue;
            }
            let uri = PartUri::parse(&name)?;
            if package
                .parts
                .keys()
                .any(|existing| part_names_conflict(existing, &uri))
            {
                return Err(OpcError::Duplicate(uri.to_string()));
            }
            let content_type = overrides
                .get(&ascii_fold(uri.as_str()))
                .cloned()
                .or_else(|| {
                    extension(&name)
                        .and_then(|e| package.defaults.get(&e.to_ascii_lowercase()).cloned())
                })
                .ok_or_else(|| OpcError::Missing(format!("content type for {name}")))?;
            if is_xml_content_type(&content_type) {
                if part_bytes.len() > limits.max_xml_part_bytes {
                    return Err(OpcError::Limit("XML part bytes"));
                }
                validate_xml(&part_bytes, limits.xml.clone())?;
            }
            if package
                .parts
                .insert(
                    uri.clone(),
                    Part {
                        uri: uri.clone(),
                        content_type,
                        bytes: part_bytes,
                    },
                )
                .is_some()
            {
                return Err(OpcError::Duplicate(uri.to_string()));
            }
            if package.parts.len() > limits.max_parts {
                return Err(OpcError::Limit("part count"));
            }
        }
        for override_uri in overrides.keys() {
            if !relationship_part_names.contains(override_uri)
                && !package.parts.contains_key(&PartUri::parse(override_uri)?)
            {
                return Err(OpcError::Missing(format!(
                    "content type override {override_uri}"
                )));
            }
        }
        for (source, rels) in &package.relationships {
            if let Some(source) = source {
                if !package.parts.contains_key(source) {
                    return Err(OpcError::Missing(format!("relationship source {source}")));
                }
            }
            for rel in rels {
                if rel.mode == TargetMode::Internal {
                    let target = rel
                        .resolved_target
                        .as_ref()
                        .ok_or_else(|| OpcError::InvalidRelationship(rel.id.clone()))?;
                    if !package.parts.contains_key(target) {
                        return Err(OpcError::Missing(format!("relationship target {target}")));
                    }
                }
            }
            let _ = source;
        }
        package.load_properties(limits.xml)?;
        Ok(package)
    }

    fn load_properties(&mut self, xml_limits: XmlLimits) -> Result<(), OpcError> {
        let (core_uri, app_uri) =
            self.relationships(None)
                .iter()
                .fold((None, None), |mut uris, rel| {
                    match rel.rel_type.as_str() {
                        CORE_REL => uris.0 = rel.resolved_target.clone(),
                        APP_REL => uris.1 = rel.resolved_target.clone(),
                        _ => {}
                    }
                    uris
                });
        self.core_uri = core_uri;
        self.app_uri = app_uri;
        // Tolerant read of a property part that no relationship points to.
        if self.core_uri.is_none() {
            self.core_uri = self.orphan_part(CORE_CONTENT_TYPE);
            self.core_orphan = self.core_uri.is_some();
        }
        if self.app_uri.is_none() {
            self.app_uri = self.orphan_part(APP_CONTENT_TYPE);
            self.app_orphan = self.app_uri.is_some();
        }
        if let Some(uri) = self.core_uri.clone() {
            if let Some(part) = self.parts.get(&uri) {
                let (p, _) = parse_core(&part.bytes, xml_limits.clone())?;
                self.core = p;
            }
        }
        if let Some(uri) = self.app_uri.clone() {
            if let Some(part) = self.parts.get(&uri) {
                let (p, _) = parse_app(&part.bytes, xml_limits)?;
                self.app = p;
            }
        }
        self.load_extras();
        Ok(())
    }

    fn orphan_part(&self, content_type: &str) -> Option<PartUri> {
        let mut found = self
            .parts
            .values()
            .filter(|p| p.content_type.eq_ignore_ascii_case(content_type));
        let first = found.next()?;
        found.next().is_none().then(|| first.uri.clone())
    }

    pub fn write(&self) -> Result<Vec<u8>, OpcError> {
        let mut package = self.clone();
        package.materialize_dirty_properties()?;
        package.write_materialized()
    }

    fn materialize_dirty_properties(&mut self) -> Result<(), OpcError> {
        if self.core_dirty {
            if let Some(uri) = &self.core_uri {
                if !self.parts.contains_key(uri) {
                    return Err(OpcError::Missing(format!("core properties part {uri}")));
                }
                if self.core_orphan {
                    let uri = uri.clone();
                    self.link_property_part(&uri, CORE_REL)?;
                    self.core_orphan = false;
                }
            } else {
                let bytes = self.render_core_part()?;
                self.core_uri = Some(self.insert_property_part(
                    "docProps/core.xml",
                    CORE_CONTENT_TYPE,
                    CORE_REL,
                    bytes,
                )?);
            }
        }
        if self.app_dirty {
            if let Some(uri) = &self.app_uri {
                if !self.parts.contains_key(uri) {
                    return Err(OpcError::Missing(format!("app properties part {uri}")));
                }
                if self.app_orphan {
                    let uri = uri.clone();
                    self.link_property_part(&uri, APP_REL)?;
                    self.app_orphan = false;
                }
            } else {
                let bytes = self.render_app_part()?;
                self.app_uri = Some(self.insert_property_part(
                    "docProps/app.xml",
                    APP_CONTENT_TYPE,
                    APP_REL,
                    bytes,
                )?);
            }
        }
        Ok(())
    }

    fn insert_property_part(
        &mut self,
        path: &str,
        content_type: &str,
        rel_type: &str,
        bytes: Vec<u8>,
    ) -> Result<PartUri, OpcError> {
        let uri = PartUri::parse(path)?;
        if self.parts.contains_key(&uri) {
            return Err(OpcError::Duplicate(uri.to_string()));
        }
        if self
            .relationships(None)
            .iter()
            .any(|rel| rel.rel_type == rel_type)
        {
            return Err(OpcError::InvalidRelationship(format!(
                "property relation already exists: {rel_type}"
            )));
        }
        self.insert_part(uri.clone(), content_type, bytes)?;
        self.link_property_part(&uri, rel_type)?;
        Ok(uri)
    }

    fn link_property_part(&mut self, uri: &PartUri, rel_type: &str) -> Result<(), OpcError> {
        let base_id = if rel_type == CORE_REL {
            "rIdCoreProperties"
        } else {
            "rIdAppProperties"
        };
        let mut id = base_id.to_owned();
        let mut suffix = 2usize;
        while self.relationships(None).iter().any(|rel| rel.id == id) {
            id = format!("{base_id}{suffix}");
            suffix = suffix.saturating_add(1);
        }
        self.add_relationship(None, id, rel_type, uri.to_string(), TargetMode::Internal)
    }

    fn write_materialized(&self) -> Result<Vec<u8>, OpcError> {
        self.validate_relationship_references()?;
        let mut zip = ZipWriter::new(WriteOptions::default());
        let ct = write_content_types(self)?;
        zip.add(CONTENT_TYPES_PATH, &ct, Method::Deflate)?;
        for (source, rels) in &self.relationships {
            let path = source
                .as_ref()
                .map(PartUri::rels_path)
                .unwrap_or_else(|| "_rels/.rels".into());
            zip.add(&path, &write_relationships(rels)?, Method::Deflate)?;
        }
        let generated_core = if self.core_dirty {
            self.core_uri
                .as_ref()
                .and_then(|u| self.parts.get(u))
                .map(|p| self.render_core_part().map(|b| (p.uri.to_string(), b)))
                .transpose()?
        } else {
            None
        };
        let generated_app = if self.app_dirty {
            self.app_uri
                .as_ref()
                .and_then(|u| self.parts.get(u))
                .map(|p| self.render_app_part().map(|b| (p.uri.to_string(), b)))
                .transpose()?
        } else {
            None
        };
        for part in self.parts.values() {
            let data = if generated_core
                .as_ref()
                .is_some_and(|(p, _)| p == part.uri.as_str())
            {
                &generated_core.as_ref().unwrap().1
            } else if generated_app
                .as_ref()
                .is_some_and(|(p, _)| p == part.uri.as_str())
            {
                &generated_app.as_ref().unwrap().1
            } else {
                &part.bytes
            };
            zip.add(part.uri.as_str(), data, Method::Deflate)?;
        }
        Ok(zip.finish()?)
    }

    fn validate_relationship_references(&self) -> Result<(), OpcError> {
        for (source, rels) in &self.relationships {
            if let Some(source) = source {
                if !self.parts.contains_key(source) {
                    return Err(OpcError::Missing(format!("relationship source {source}")));
                }
            }
            for rel in rels {
                if rel.mode == TargetMode::Internal {
                    let target = rel
                        .resolved_target
                        .as_ref()
                        .ok_or_else(|| OpcError::InvalidRelationship(rel.id.clone()))?;
                    if !self.parts.contains_key(target) {
                        return Err(OpcError::Missing(format!("relationship target {target}")));
                    }
                }
            }
        }
        Ok(())
    }
}

fn part_names_conflict(left: &PartUri, right: &PartUri) -> bool {
    let left: Vec<_> = left.as_str().split('/').collect();
    let right: Vec<_> = right.as_str().split('/').collect();
    left.iter()
        .zip(&right)
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn valid_content_type(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut pos = 0;
    if !consume_http_token(bytes, &mut pos) || bytes.get(pos) != Some(&b'/') {
        return false;
    }
    pos += 1;
    if !consume_http_token(bytes, &mut pos) {
        return false;
    }
    loop {
        while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
            pos += 1;
        }
        if pos == bytes.len() {
            return true;
        }
        if bytes[pos] != b';' {
            return false;
        }
        pos += 1;
        while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
            pos += 1;
        }
        if !consume_http_token(bytes, &mut pos) || bytes.get(pos) != Some(&b'=') {
            return false;
        }
        pos += 1;
        if bytes.get(pos) == Some(&b'"') {
            pos += 1;
            let mut closed = false;
            while let Some(&byte) = bytes.get(pos) {
                match byte {
                    b'"' => {
                        pos += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        pos += 1;
                        if !bytes
                            .get(pos)
                            .is_some_and(|b| *b == b'\t' || (0x20..=0x7e).contains(b) || *b >= 0x80)
                        {
                            return false;
                        }
                        pos += 1;
                    }
                    b'\t' | b' ' | b'!' | 0x23..=0x5b | 0x5d..=0x7e | 0x80..=0xff => {
                        pos += 1;
                    }
                    _ => return false,
                }
            }
            if !closed {
                return false;
            }
        } else if !consume_http_token(bytes, &mut pos) {
            return false;
        }
    }
}
fn consume_http_token(bytes: &[u8], pos: &mut usize) -> bool {
    let start = *pos;
    while bytes.get(*pos).is_some_and(|b| is_http_token_byte(*b)) {
        *pos += 1;
    }
    *pos != start
}
fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}
fn valid_extension(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut pos = 0;
    while pos < bytes.len() {
        let b = bytes[pos];
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b':'
                    | b'='
                    | b'@'
                    | b'-'
                    | b'_'
                    | b'~'
            )
        {
            pos += 1;
        } else if b == b'%'
            && pos + 2 < bytes.len()
            && bytes[pos + 1].is_ascii_hexdigit()
            && bytes[pos + 2].is_ascii_hexdigit()
        {
            pos += 3;
        } else {
            return false;
        }
    }
    true
}
fn is_xml_content_type(s: &str) -> bool {
    let media_type = s.split(';').next().unwrap_or(s).trim();
    media_type.eq_ignore_ascii_case("application/xml")
        || media_type.eq_ignore_ascii_case("text/xml")
        || media_type
            .rsplit_once('/')
            .is_some_and(|(_, subtype)| subtype.to_ascii_lowercase().ends_with("+xml"))
}
fn validate_xml(bytes: &[u8], limits: XmlLimits) -> Result<(), OpcError> {
    let mut reader = Reader::with_limits(Cursor::new(bytes), limits);
    while reader.next_event()?.is_some() {}
    Ok(())
}
fn extension(s: &str) -> Option<String> {
    s.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}
fn validate_iri_reference(s: &str) -> Result<(), OpcError> {
    if s.chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err(OpcError::InvalidUri(s.into()));
    }
    let bytes = s.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == b'%' {
            if pos + 2 >= bytes.len()
                || !bytes[pos + 1].is_ascii_hexdigit()
                || !bytes[pos + 2].is_ascii_hexdigit()
            {
                return Err(OpcError::InvalidUri(s.into()));
            }
            pos += 3;
            continue;
        }
        let ch = s[pos..]
            .chars()
            .next()
            .ok_or_else(|| OpcError::InvalidUri(s.into()))?;
        if ch.is_ascii()
            && !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_' | '~' | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '=' | ':' | '@' | '/' | '?' | '#' | '[' | ']')
        {
            return Err(OpcError::InvalidUri(s.into()));
        }
        pos += ch.len_utf8();
    }
    if let Some(colon) = s.find(':') {
        let boundary = s.find(['/', '?', '#']).unwrap_or(s.len());
        if colon < boundary {
            let scheme = &s[..colon];
            let mut chars = scheme.chars();
            if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            {
                return Err(OpcError::InvalidUri(s.into()));
            }
        }
    }
    Ok(())
}
fn resolve_target(source: Option<&PartUri>, target: &str) -> Result<PartUri, OpcError> {
    validate_iri_reference(target)?;
    let path = target
        .split('#')
        .next()
        .unwrap_or(target)
        .split('?')
        .next()
        .unwrap_or(target);
    if path.is_empty() {
        return source
            .cloned()
            .ok_or_else(|| OpcError::InvalidUri(target.into()));
    }
    if path.starts_with("//") || path.ends_with('/') {
        return Err(OpcError::InvalidUri(target.into()));
    }
    let absolute = path.starts_with('/');
    let target_path = path;
    let mut resolved = if absolute {
        Vec::new()
    } else if let Some(src) = source {
        src.as_str()
            .rsplit_once('/')
            .map(|(d, _)| d.split('/').collect())
            .unwrap_or_else(Vec::new)
    } else {
        Vec::new()
    };
    for seg in target_path.trim_start_matches('/').split('/') {
        if seg.is_empty() {
            return Err(OpcError::InvalidUri(target.into()));
        }
        if seg == "." {
            continue;
        }
        if seg == ".." {
            if resolved.pop().is_none() {
                return Err(OpcError::InvalidUri(target.into()));
            }
        } else {
            if seg.contains(':') {
                return Err(OpcError::InvalidUri(target.into()));
            }
            resolved.push(seg);
        }
    }
    PartUri::parse(&resolved.join("/"))
}
fn relationship_source(path: &str) -> Result<Option<PartUri>, OpcError> {
    let segments: Vec<_> = path.split('/').collect();
    if segments.len() == 2
        && segments[0].eq_ignore_ascii_case("_rels")
        && segments[1].eq_ignore_ascii_case(".rels")
    {
        return Ok(None);
    }
    if segments.len() < 2
        || !segments[segments.len() - 2].eq_ignore_ascii_case("_rels")
        || !segments[segments.len() - 1]
            .to_ascii_lowercase()
            .ends_with(".rels")
    {
        return Err(OpcError::InvalidRelationship(path.into()));
    }
    let filename = segments[segments.len() - 1];
    let source_name = &filename[..filename.len() - ".rels".len()];
    let mut source_segments = segments[..segments.len() - 2].to_vec();
    source_segments.push(source_name);
    Ok(Some(PartUri::parse(&source_segments.join("/"))?))
}
fn is_relationship_part_name(path: &str) -> bool {
    let segments: Vec<_> = path.split('/').collect();
    (segments.len() == 2
        && segments[0].eq_ignore_ascii_case("_rels")
        && segments[1].eq_ignore_ascii_case(".rels"))
        || (segments.len() >= 2
            && segments[segments.len() - 2].eq_ignore_ascii_case("_rels")
            && segments[segments.len() - 1]
                .to_ascii_lowercase()
                .ends_with(".rels"))
}
fn attr(attrs: &[Attribute], namespace: &str, local: &str) -> Option<String> {
    attrs
        .iter()
        .find(|a| a.name.local == local && a.name.namespace.as_deref().unwrap_or("") == namespace)
        .map(|a| a.value.clone())
}
type ContentTypeMaps = (BTreeMap<String, String>, BTreeMap<String, String>);
fn parse_content_types(bytes: &[u8], limits: XmlLimits) -> Result<ContentTypeMaps, OpcError> {
    let mut reader = Reader::with_limits(Cursor::new(bytes), limits);
    let mut defaults = BTreeMap::new();
    let mut overrides = BTreeMap::new();
    let mut root = false;
    let mut depth = 0usize;
    while let Some(ev) = reader.next_event()? {
        match ev {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                if !root {
                    if name.local != "Types" || name.namespace.as_deref() != Some(CONTENT_TYPES_NS)
                    {
                        return Err(OpcError::InvalidPackage("invalid content types root"));
                    }
                    root = true;
                    if !empty {
                        depth = 1;
                    }
                    continue;
                }
                if depth != 1 || name.namespace.as_deref() != Some(CONTENT_TYPES_NS) {
                    return Err(OpcError::InvalidPackage(
                        "nested or unexpected content types element",
                    ));
                }
                match name.local.as_str() {
                    "Default" => {
                        let ext = attr(&attributes, "", "Extension")
                            .ok_or(OpcError::InvalidPackage("Default missing Extension"))?
                            .to_ascii_lowercase();
                        let ct = attr(&attributes, "", "ContentType")
                            .ok_or(OpcError::InvalidPackage("Default missing ContentType"))?;
                        if !valid_extension(&ext)
                            || !valid_content_type(&ct)
                            || defaults.insert(ext, ct).is_some()
                        {
                            return Err(OpcError::InvalidPackage("duplicate or invalid Default"));
                        }
                    }
                    "Override" => {
                        let p = attr(&attributes, "", "PartName")
                            .ok_or(OpcError::InvalidPackage("Override missing PartName"))?;
                        let uri = PartUri::parse_with_relationship_metadata(&p, true)?;
                        let ct = attr(&attributes, "", "ContentType")
                            .ok_or(OpcError::InvalidPackage("Override missing ContentType"))?;
                        if !valid_content_type(&ct)
                            || overrides.insert(ascii_fold(uri.as_str()), ct).is_some()
                        {
                            return Err(OpcError::InvalidPackage("duplicate or invalid Override"));
                        }
                    }
                    _ => return Err(OpcError::InvalidPackage("unknown content types element")),
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::EndElement { .. } => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if !root {
        return Err(OpcError::InvalidPackage("empty content types"));
    }
    Ok((defaults, overrides))
}
fn parse_relationships(
    bytes: &[u8],
    source: Option<&PartUri>,
    limits: XmlLimits,
) -> Result<Vec<Relationship>, OpcError> {
    let mut r = Reader::with_limits(Cursor::new(bytes), limits);
    let mut root = false;
    let mut depth = 0usize;
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    while let Some(ev) = r.next_event()? {
        match ev {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                if !root {
                    if name.local != "Relationships" || name.namespace.as_deref() != Some(REL_NS) {
                        return Err(OpcError::InvalidRelationship("invalid root".into()));
                    }
                    root = true;
                    if !empty {
                        depth = 1;
                    }
                    continue;
                }
                if depth != 1
                    || name.namespace.as_deref() != Some(REL_NS)
                    || name.local != "Relationship"
                {
                    return Err(OpcError::InvalidRelationship(
                        "nested or unexpected relationship element".into(),
                    ));
                }
                let id = attr(&attributes, "", "Id")
                    .ok_or(OpcError::InvalidRelationship("missing Id".into()))?;
                let rel_type = attr(&attributes, "", "Type")
                    .ok_or(OpcError::InvalidRelationship(format!("{id}: missing Type")))?;
                let target = attr(&attributes, "", "Target").ok_or(
                    OpcError::InvalidRelationship(format!("{id}: missing Target")),
                )?;
                let mode = match attr(&attributes, "", "TargetMode").as_deref() {
                    None | Some("Internal") => TargetMode::Internal,
                    Some("External") => TargetMode::External,
                    Some(_) => {
                        return Err(OpcError::InvalidRelationship(format!(
                            "{id}: invalid TargetMode"
                        )));
                    }
                };
                if id.is_empty() || id.chars().any(char::is_whitespace) {
                    return Err(OpcError::InvalidRelationship(format!("{id}: invalid Id")));
                }
                if !ids.insert(id.clone()) {
                    return Err(OpcError::Duplicate(id));
                }
                if validate_iri_reference(&rel_type).is_err() {
                    return Err(OpcError::InvalidRelationship(format!("{id}: invalid Type")));
                }
                let resolved_target = match mode {
                    TargetMode::Internal => Some(resolve_target(source, &target)?),
                    TargetMode::External => {
                        validate_iri_reference(&target)?;
                        None
                    }
                };
                out.push(Relationship {
                    id,
                    rel_type,
                    target,
                    mode,
                    resolved_target,
                });
                if !empty {
                    depth += 1;
                }
            }
            Event::EndElement { .. } => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if !root {
        return Err(OpcError::InvalidRelationship("empty relationships".into()));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn read_simple_properties(
    bytes: &[u8],
    root_ns: &str,
    limits: XmlLimits,
) -> Result<BTreeMap<(String, String), String>, OpcError> {
    let mut r = Reader::with_limits(Cursor::new(bytes), limits);
    let mut depth = 0usize;
    let mut active: Option<(String, String)> = None;
    let mut active_depth = 0usize;
    let mut text = String::new();
    let mut out = BTreeMap::new();
    while let Some(ev) = r.next_event()? {
        match ev {
            Event::StartElement { name, empty, .. } => {
                if depth == 0 {
                    if name.namespace.as_deref() != Some(root_ns) {
                        return Err(OpcError::InvalidPackage("invalid property namespace"));
                    }
                } else if depth == 1 {
                    active = Some((name.namespace.unwrap_or_default(), name.local));
                    active_depth = depth;
                    text.clear();
                    if empty {
                        if let Some(k) = active.take() {
                            out.insert(k, text.clone());
                        }
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::Text(s) => {
                if active.is_some() {
                    text.push_str(&s)
                }
            }
            Event::EndElement { .. } => {
                depth = depth.saturating_sub(1);
                if active.is_some() && depth == active_depth {
                    if let Some(k) = active.take() {
                        out.insert(k, text.clone());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}
fn parse_core(
    bytes: &[u8],
    limits: XmlLimits,
) -> Result<(CoreProperties, Vec<atelier_xml::OpaqueSubtree>), OpcError> {
    let values = read_simple_properties(bytes, CORE_NS, limits.clone())?;
    let mut r = Reader::with_limits(Cursor::new(bytes), limits);
    let mut p = CoreProperties::default();
    let mut unknown = Vec::new();
    let mut root = false;
    while let Some(ev) = r.next_event()? {
        if let Event::StartElement { name, .. } = ev {
            if !root {
                if name.local != "coreProperties" || name.namespace.as_deref() != Some(CORE_NS) {
                    return Err(OpcError::InvalidPackage("invalid core properties"));
                }
                root = true;
                continue;
            }
            let ns = name.namespace.as_deref().unwrap_or("").to_owned();
            let local = name.local.clone();
            let sub = r.capture_opaque()?;
            let val = values.get(&(ns.clone(), local.clone())).cloned();
            match (ns.as_str(), local.as_str()) {
                (DC_NS, "title") => p.title = val,
                (DC_NS, "subject") => p.subject = val,
                (DC_NS, "creator") => p.creator = val,
                (DC_NS, "description") => p.description = val,
                (DC_NS, "language") => p.language = val,
                (DC_NS, "identifier") => p.identifier = val,
                (DC_NS, "rights") => unknown.push(sub),
                (CORE_NS, "keywords") => p.keywords = val,
                (CORE_NS, "lastModifiedBy") => p.last_modified_by = val,
                (CORE_NS, "revision") => p.revision = val,
                (CORE_NS, "category") => p.category = val,
                (CORE_NS, "contentStatus") => p.content_status = val,
                (CORE_NS, "version") => p.version = val,
                (DCTERMS_NS, "created") => p.created = val,
                (DCTERMS_NS, "modified") => p.modified = val,
                _ => unknown.push(sub),
            }
        }
    }
    Ok((p, unknown))
}
// Opaque values are intentionally not decoded through a second XML parser;
// common property strings are decoded during the main event traversal below.
fn parse_app(
    bytes: &[u8],
    limits: XmlLimits,
) -> Result<(AppProperties, Vec<atelier_xml::OpaqueSubtree>), OpcError> {
    let values = read_simple_properties(bytes, APP_NS, limits.clone())?;
    let mut r = Reader::with_limits(Cursor::new(bytes), limits);
    let mut p = AppProperties::default();
    let mut unknown = Vec::new();
    let mut root = false;
    while let Some(ev) = r.next_event()? {
        if let Event::StartElement { name, .. } = ev {
            if !root {
                if name.local != "Properties" || name.namespace.as_deref() != Some(APP_NS) {
                    return Err(OpcError::InvalidPackage("invalid app properties"));
                }
                root = true;
                continue;
            }
            let local = name.local.clone();
            let ns = name.namespace.as_deref().unwrap_or("").to_owned();
            let sub = r.capture_opaque()?;
            let val = values.get(&(ns.clone(), local.clone())).cloned();
            match (ns.as_str(), local.as_str()) {
                (APP_NS, "Application") => p.application = val,
                (APP_NS, "AppVersion") => p.app_version = val,
                (APP_NS, "Company") => p.company = val,
                (APP_NS, "Manager") => p.manager = val,
                (APP_NS, "Pages") => p.pages = val,
                (APP_NS, "Words") => p.words = val,
                (APP_NS, "Characters") => p.characters = val,
                (APP_NS, "Paragraphs") => p.paragraphs = val,
                (APP_NS, "Lines") => p.lines = val,
                (APP_NS, "Template") => p.template = val,
                _ => unknown.push(sub),
            }
        }
    }
    Ok((p, unknown))
}
/// Attribute-value escaping for the OPC parts written below.
fn escape_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}
// `[Content_Types].xml` and the relationship parts are written with the
// default namespace declared once on the root. The generic XML writer would
// prefix every element (`ns0:`) and repeat the declaration, which other
// consumers (LibreOffice, Word) do not accept in these parts. The reader
// still accepts both forms.
fn write_relationships(rels: &[Relationship]) -> Result<Vec<u8>, OpcError> {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"{REL_NS}\">"
    );
    for r in rels {
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"",
            escape_attr(&r.id),
            escape_attr(&r.rel_type),
            escape_attr(&r.target)
        ));
        if r.mode == TargetMode::External {
            out.push_str(" TargetMode=\"External\"");
        }
        out.push_str("/>");
    }
    out.push_str("</Relationships>");
    Ok(out.into_bytes())
}
fn write_content_types(p: &Package) -> Result<Vec<u8>, OpcError> {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"{CONTENT_TYPES_NS}\">"
    );
    let mut used = BTreeSet::new();
    for part in p.parts.values() {
        used.insert(part.content_type.clone());
    }
    let has_rels = p.relationships.values().any(|rels| !rels.is_empty());
    let mut wrote_rels = false;
    let default = |ext: &str, ct: &str| {
        format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            escape_attr(ext),
            escape_attr(ct)
        )
    };
    for (ext, ct) in &p.defaults {
        if ext == "rels" && ct == REL_CONTENT_TYPE {
            wrote_rels = true;
        }
        if !used.contains(ct) || (ext == "rels" && has_rels) {
            out.push_str(&default(ext, ct));
        }
    }
    if has_rels && !wrote_rels {
        out.push_str(&default("rels", REL_CONTENT_TYPE));
    }
    for part in p.parts.values() {
        out.push_str(&format!(
            "<Override PartName=\"/{}\" ContentType=\"{}\"/>",
            escape_attr(part.uri.as_str()),
            escape_attr(&part.content_type)
        ));
    }
    out.push_str("</Types>");
    Ok(out.into_bytes())
}
