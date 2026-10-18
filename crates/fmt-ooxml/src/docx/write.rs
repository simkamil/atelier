// SPDX-License-Identifier: AGPL-3.0-or-later
//! DOCX writer (US-307).
//!
//! [`write_docx`] writes an [`ImportedDocx`] (possibly edited through model
//! operations) back to a package; [`write_new_docx`] writes a rich document
//! created from scratch. Parts the import interprets are regenerated from the
//! model when the model changed (see [`DocxWriteOptions::regenerate_all`] and the
//! "touched part" rule in `docs/us-307-docx-writer.md`) and copied byte for
//! byte otherwise; every other part of the package is copied as is.
//!
//! The writer never modifies the model: it reads the document, the source
//! skeleton (US-306) and the baseline model snapshot
//! kept by the import, which tells it what the caller changed.

use super::preserve::PreservedPart;
use super::{
    Baseline, CONTENT_DOC, DocxError, DocxLimits, ImportedDocx, REL_DOC, REL_DOC_STRICT, W_NS,
    W_STRICT_NS,
};
use crate::{OpcError, Package, PartUri, SaveContext, SaveReport, TargetMode};
use atelier_text_model::rich::{DefaultRichDocument, NodeId, RichDocument, StyleEngine};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod body;
mod parts;
mod props;
mod tbl;
mod xml;

use body::Gen;
use props::Prefixes;
use xml::Out;

/// Main part content types of the four Word variants.
pub(super) const MAIN_CONTENT_TYPES: [&str; 4] = [
    CONTENT_DOC,
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
    "application/vnd.ms-word.document.macroEnabled.main+xml",
    "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
];
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const REL_FONTS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CT_FONTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const MAX_NOTICES: usize = 512;

/// The Word file variants this writer produces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocxVariant {
    /// `.docx`
    Document,
    /// `.dotx`
    Template,
    /// `.docm`
    MacroDocument,
    /// `.dotm`
    MacroTemplate,
}

impl DocxVariant {
    /// Content type of the main document part.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Document => MAIN_CONTENT_TYPES[0],
            Self::Template => MAIN_CONTENT_TYPES[1],
            Self::MacroDocument => MAIN_CONTENT_TYPES[2],
            Self::MacroTemplate => MAIN_CONTENT_TYPES[3],
        }
    }

    pub fn from_content_type(content_type: &str) -> Option<Self> {
        [
            Self::Document,
            Self::Template,
            Self::MacroDocument,
            Self::MacroTemplate,
        ]
        .into_iter()
        .find(|v| v.content_type() == content_type)
    }

    /// File name extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Document => "docx",
            Self::Template => "dotx",
            Self::MacroDocument => "docm",
            Self::MacroTemplate => "dotm",
        }
    }

    pub fn has_macros(self) -> bool {
        matches!(self, Self::MacroDocument | Self::MacroTemplate)
    }
}

/// A style the catalog holds but the writer cannot enumerate: the catalog has
/// no listing, so styles added after the import are named by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleDecl {
    pub id: String,
    /// Display name (`w:name`).
    pub name: String,
    /// Style that follows (`w:next`).
    pub next: Option<String>,
}

/// Options of [`write_docx`] and [`write_new_docx`].
#[derive(Clone, Debug)]
pub struct DocxWriteOptions {
    /// Variant to write; `None` keeps the one of the source (a new document
    /// is a `.docx`).
    pub variant: Option<DocxVariant>,
    /// Values for the update of the document properties (US-328
    /// `update_on_save`); `None` leaves the properties as they are.
    pub save: Option<SaveContext>,
    /// Regenerate every interpreted part from the model, even when nothing
    /// changed. Used to prove the writer against a source; slower.
    pub regenerate_all: bool,
    /// Styles added to the style engine since the import.
    pub added_styles: Vec<StyleDecl>,
    /// Limits of the import (unused by the writer since the baseline is kept by the import).
    pub limits: DocxLimits,
    /// Largest XML part the writer produces, in bytes.
    pub max_part_bytes: usize,
}

impl Default for DocxWriteOptions {
    fn default() -> Self {
        Self {
            variant: None,
            save: None,
            regenerate_all: false,
            added_styles: Vec::new(),
            limits: DocxLimits::default(),
            max_part_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Failure to write. No partial output is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteError {
    Opc(OpcError),
    /// The import that establishes the baseline failed.
    Import(DocxError),
    /// A budget was exceeded (output size, nesting).
    Limit(&'static str),
    Invalid {
        part: String,
        detail: &'static str,
    },
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DOCX write error: {self:?}")
    }
}
impl std::error::Error for WriteError {}
impl From<OpcError> for WriteError {
    fn from(value: OpcError) -> Self {
        Self::Opc(value)
    }
}
impl From<DocxError> for WriteError {
    fn from(value: DocxError) -> Self {
        Self::Import(value)
    }
}

/// What could not be written faithfully. At most 512 are kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteNotice {
    /// Macro parts were removed because the target variant has no macros.
    MacrosRemoved {
        parts: Vec<String>,
    },
    /// A modified paragraph was written from the model: its source envelopes
    /// (`w:hyperlink`, `w:ins`...) and run structure are not kept.
    ParagraphRebuilt {
        paragraph: NodeId,
    },
    /// An envelope all of whose text was deleted was dropped.
    EnvelopeDropped {
        name: String,
    },
    /// Text inside an `mc:AlternateContent` branch was edited: the edited
    /// content replaces the element and the other branch is dropped.
    AlternateContentFlattened,
    /// Blocks were moved: the wrappers of the source were dropped.
    BlocksReordered,
    /// The color of a highlight is not one of the sixteen Word highlights.
    HighlightApproximated {
        paragraph: NodeId,
        color: (u8, u8, u8),
    },
    HighlightColorApproximated {
        color: (u8, u8, u8),
    },
    /// A character XML 1.0 cannot carry was left out.
    UnsupportedCharacter {
        paragraph: NodeId,
        character: u32,
    },
    /// The table grid of the source was edited in a way the writer cannot
    /// keep consistent (vertical merges).
    MergeStructureChanged {
        table: NodeId,
    },
    /// The theme differs from the import; the theme part is written as is.
    ThemeNotWritten,
    /// Headers and footers of the model are not written (US-304).
    HeaderFooterNotWritten,
    /// A paragraph refers to a list (`w:numId` = identifier + 1) that the numbering
    /// part of the package does not define; Word shows it without numbering.
    ListDefinitionMissing {
        list_id: u32,
    },
    /// Further notices were dropped.
    MoreNotices {
        count: usize,
    },
}

/// A written package.
#[derive(Clone, Debug)]
pub struct WrittenDocx {
    pub bytes: Vec<u8>,
    pub notices: Vec<WriteNotice>,
    /// Interpreted parts rebuilt from the model.
    pub regenerated_parts: Vec<String>,
    /// Interpreted parts copied byte for byte.
    pub reused_parts: Vec<String>,
    /// Parts the model does not interpret, re-emitted from the opaque store kept
    /// with the model (`ImportedDocx::package`, US-306), in package order.
    pub opaque_parts: Vec<String>,
    /// Result of the properties update when `DocxWriteOptions::save` was given.
    pub save: Option<SaveReport>,
}

/// Page geometry of a new document, in twips.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSetup {
    pub width: u32,
    pub height: u32,
    pub margin_top: u32,
    pub margin_right: u32,
    pub margin_bottom: u32,
    pub margin_left: u32,
    pub landscape: bool,
}

impl Default for PageSetup {
    /// A4 portrait with 2.5 cm margins.
    fn default() -> Self {
        Self {
            width: 11906,
            height: 16838,
            margin_top: 1417,
            margin_right: 1417,
            margin_bottom: 1417,
            margin_left: 1417,
            landscape: false,
        }
    }
}

impl PageSetup {
    fn text_width(&self) -> u32 {
        self.width
            .saturating_sub(self.margin_left)
            .saturating_sub(self.margin_right)
            .max(1000)
    }
}

/// A rich document created from scratch, with its styles.
pub struct NewDocument<'a> {
    pub document: &'a RichDocument,
    pub styles: &'a StyleEngine,
    /// Display names of the styles, by style id: the catalog cannot be listed,
    /// so these are the styles written.
    pub style_names: &'a BTreeMap<String, String>,
    /// `w:next` of each style.
    pub next_styles: Option<&'a BTreeMap<String, String>>,
    /// Default paragraph style (`w:default="1"`).
    pub default_style: Option<&'a str>,
    pub page: PageSetup,
    /// Default tab stop in twips.
    pub default_tab_stop: u32,
}

impl<'a> NewDocument<'a> {
    /// The default profile of US-202.
    pub fn from_default(d: &'a DefaultRichDocument) -> Self {
        let p = &d.presentation;
        let twips = |h: u32| (u64::from(h) * 1440 / 2540) as u32;
        Self {
            document: &d.document,
            styles: &d.styles,
            style_names: &d.style_names,
            next_styles: Some(&d.next_styles),
            default_style: Some("atelier.normal"),
            page: PageSetup {
                width: twips(p.page_width_hundredths_mm),
                height: twips(p.page_height_hundredths_mm),
                margin_top: twips(p.margin_top_hundredths_mm),
                margin_right: twips(p.margin_right_hundredths_mm),
                margin_bottom: twips(p.margin_bottom_hundredths_mm),
                margin_left: twips(p.margin_left_hundredths_mm),
                landscape: !p.portrait,
            },
            default_tab_stop: 720,
        }
    }
}

// ---------------------------------------------------------------------------
// Namespace prefixes
// ---------------------------------------------------------------------------

/// Prefixes of the WordprocessingML and relationships namespaces in a part,
/// with the declarations to add when it has none.
fn prefixes(decls: &[(String, String)], need_r: bool) -> (Prefixes, Vec<(String, String)>) {
    let find = |uris: &[&str]| {
        decls
            .iter()
            .rev()
            .find(|(p, u)| !p.is_empty() && uris.contains(&u.as_str()))
            .map(|(p, _)| p.clone())
    };
    let mut extra = Vec::new();
    let mut pick = |found: Option<String>, wanted: &str, uri: &str| match found {
        Some(p) => p,
        None => {
            let mut name = wanted.to_owned();
            while decls.iter().any(|(p, _)| *p == name) {
                name.push('x');
            }
            extra.push((name.clone(), uri.to_owned()));
            name
        }
    };
    let w = pick(find(&[W_NS, W_STRICT_NS]), "w", W_NS);
    let found_r = find(&[R_NS]);
    let r = if need_r || found_r.is_some() {
        pick(found_r, "r", R_NS)
    } else {
        "r".to_owned()
    };
    (Prefixes { w, r }, extra)
}

/// Whether any paragraph of the document carries a hyperlink of the model.
fn has_links(doc: &RichDocument) -> bool {
    doc.paragraph_ids()
        .iter()
        .any(|id| doc.paragraph(*id).is_ok_and(|p| !p.links.is_empty()))
}

fn root_attrs(part: &PreservedPart, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (p, u) in part.declarations.iter().chain(extra) {
        out.push((
            if p.is_empty() {
                "xmlns".to_owned()
            } else {
                format!("xmlns:{p}")
            },
            u.clone(),
        ));
    }
    for a in &part.root.attrs {
        out.push((a.name.clone(), a.value.clone()));
    }
    out
}

fn as_refs(attrs: &[(String, String)]) -> Vec<(&str, &str)> {
    attrs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

fn part_uri(text: &str) -> Result<PartUri, WriteError> {
    Ok(PartUri::parse(text)?)
}

fn replace_part(package: &mut Package, uri: &PartUri, bytes: Vec<u8>) {
    if let Some(p) = package.parts.get_mut(uri) {
        p.bytes = bytes;
    }
}

/// Content types and relationship types of the macro parts of a package.
fn macro_parts(package: &Package) -> Vec<PartUri> {
    package
        .parts
        .values()
        .filter(|p| {
            let ct = p.content_type.as_str();
            ct == "application/vnd.ms-office.vbaProject"
                || ct == "application/vnd.ms-office.vbaProjectSignature"
                || ct == "application/vnd.ms-word.vbaData+xml"
                || ct == "application/vnd.ms-office.vbaProjectSignatureAgile"
                || ct == "application/vnd.ms-office.vbaProjectSignatureV3"
        })
        .map(|p| p.uri.clone())
        .collect()
}

fn remove_parts(package: &mut Package, uris: &[PartUri]) {
    for uri in uris {
        package.parts.remove(uri);
        package.relationships.remove(&Some(uri.clone()));
    }
    for list in package.relationships.values_mut() {
        list.retain(|r| r.resolved_target.as_ref().is_none_or(|t| !uris.contains(t)));
    }
}

/// Applies the variant, the macro rule and the properties update.
fn finish_package(
    package: &mut Package,
    main: &PartUri,
    options: &DocxWriteOptions,
    notices: &mut Vec<WriteNotice>,
) -> Result<Option<SaveReport>, WriteError> {
    let current = package
        .parts
        .get(main)
        .and_then(|p| DocxVariant::from_content_type(&p.content_type))
        .unwrap_or(DocxVariant::Document);
    let target = options.variant.unwrap_or(current);
    if let Some(p) = package.parts.get_mut(main) {
        p.content_type = target.content_type().to_owned();
    }
    if !target.has_macros() {
        let macros = macro_parts(package);
        if !macros.is_empty() {
            notices.push(WriteNotice::MacrosRemoved {
                parts: macros.iter().map(ToString::to_string).collect(),
            });
            remove_parts(package, &macros);
        }
    }
    Ok(options.save.as_ref().map(|ctx| package.update_on_save(ctx)))
}

fn finish_notices(mut notices: Vec<WriteNotice>, dropped: usize) -> Vec<WriteNotice> {
    notices.truncate(MAX_NOTICES);
    if dropped > 0 {
        notices.push(WriteNotice::MoreNotices { count: dropped });
    }
    notices
}

struct Sections {
    blocks: Vec<atelier_model::NodeId>,
    after: BTreeMap<atelier_model::NodeId, Vec<u8>>,
    has_header_footer: bool,
}

fn sections(doc: &RichDocument, page: &PageSetup) -> Sections {
    let ids = doc.section_ids();
    let mut blocks = Vec::new();
    let mut after = BTreeMap::new();
    let mut has_hf = false;
    for (k, sec) in ids.iter().enumerate() {
        if let Some(atelier_text_model::rich::Node::Section(s)) = doc.node(*sec) {
            has_hf |= !s.headers.is_empty() || !s.footers.is_empty();
        }
        let list = doc.block_ids(Some(*sec)).unwrap_or_default();
        if k + 1 < ids.len() {
            if let Some(last) = list.last() {
                after.insert(*last, parts::sect_pr(page, Some("nextPage"), false));
            }
        }
        blocks.extend(list);
    }
    Sections {
        blocks,
        after,
        has_header_footer: has_hf,
    }
}

fn default_style_of(imported: &ImportedDocx) -> Option<String> {
    let part = imported.preserved.styles.as_ref()?;
    part.root.children.iter().find_map(|c| match c {
        super::preserve::PreservedChild::Element(e)
            if props::local_of(&e.name) == "style"
                && e.attrs
                    .iter()
                    .any(|a| props::local_of(&a.name) == "type" && a.value == "paragraph")
                && e.attrs.iter().any(|a| {
                    props::local_of(&a.name) == "default"
                        && matches!(a.value.as_str(), "1" | "true" | "on")
                }) =>
        {
            e.attrs
                .iter()
                .find(|a| props::local_of(&a.name) == "styleId")
                .map(|a| a.value.clone())
        }
        _ => None,
    })
}

/// Writes an imported document, possibly edited through model operations.
pub fn write_docx(
    imported: &ImportedDocx,
    options: &DocxWriteOptions,
) -> Result<WrittenDocx, WriteError> {
    let mut package = imported.package.clone();
    if options.regenerate_all {
        if imported.preserved.document.is_none() {
            return Err(WriteError::Invalid {
                part: "word/document.xml".into(),
                detail: "the model has no source skeleton to regenerate from",
            });
        }
        package.canonicalize();
    }
    let base = &imported.baseline;
    let main_uri = package
        .relationships(None)
        .iter()
        .find(|r| r.rel_type == REL_DOC || r.rel_type == REL_DOC_STRICT)
        .and_then(|r| r.resolved_target.clone())
        .ok_or(WriteError::Invalid {
            part: "_rels/.rels".into(),
            detail: "no main document relationship",
        })?;
    let mut notices = Vec::new();
    let mut regenerated = Vec::new();
    let mut reused = Vec::new();
    let mut dropped = 0;
    let mut fonts = BTreeSet::new();

    // ---- document.xml
    let document_changed = options.regenerate_all || imported.document != base.document;
    if let Some(part) = imported
        .preserved
        .document
        .as_ref()
        .filter(|_| document_changed)
    {
        let (decl_prefixes, extra) = prefixes(&part.declarations, has_links(&imported.document));
        let page = PageSetup::default();
        let secs = sections(&imported.document, &page);
        if secs.has_header_footer {
            notices.push(WriteNotice::HeaderFooterNotWritten);
        }
        let mut g = Gen {
            doc: &imported.document,
            base: Some(&base.document),
            pres: Some(&imported.preserved),
            p: decl_prefixes,
            default_style: default_style_of(imported),
            out: Out::new(),
            notices: Vec::new(),
            dropped_notices: 0,
            links: Vec::new(),
            link_prefix: free_link_prefix(&package, &main_uri),
            bookmark_ids: BTreeMap::new(),
            fonts: BTreeSet::new(),
            lists: BTreeSet::new(),
            max_output: options.max_part_bytes,
            failed: None,
            depth: 0,
            text_width: 9000,
            sect_after: secs.after,
            table_stack: Vec::new(),
            tables_info: Some(&imported.tables),
            terminate_body: false,
        };
        g.out.declaration();
        let attrs = root_attrs(part, &extra);
        g.out.start(&part.root.name, as_refs(&attrs));
        for c in &part.root.children {
            match c {
                super::preserve::PreservedChild::Element(body) => {
                    g.out.start(
                        &body.name,
                        body.attrs
                            .iter()
                            .map(|a| (a.name.as_str(), a.value.as_str())),
                    );
                    g.terminate_body = true;
                    g.emit_container(&body.children, &secs.blocks, None);
                    g.out.end();
                }
                super::preserve::PreservedChild::Opaque(b) => g.out.raw(b),
                _ => {}
            }
        }
        g.out.end();
        if let Some(reason) = g.failed {
            return Err(WriteError::Limit(reason));
        }
        notices.append(&mut g.notices);
        dropped += g.dropped_notices;
        fonts.append(&mut g.fonts);
        let lists = std::mem::take(&mut g.lists);
        let links = std::mem::take(&mut g.links);
        let uri = part_uri(&part.uri)?;
        replace_part(&mut package, &uri, g.out.into_bytes());
        for (rid, target) in links {
            package.add_relationship(
                Some(main_uri.clone()),
                rid,
                REL_HYPERLINK,
                target,
                TargetMode::External,
            )?;
        }
        if !lists.is_empty() {
            for list_id in add_numbering_if_missing(&mut package, &main_uri, &lists)? {
                notices.push(WriteNotice::ListDefinitionMissing { list_id });
            }
        }
        regenerated.push(part.uri.clone());
    } else if let Some(part) = &imported.preserved.document {
        reused.push(part.uri.clone());
    }

    // ---- styles.xml
    if let Some(part) = &imported.preserved.styles {
        let mut added = options.added_styles.clone();
        added.dedup();
        let changed =
            options.regenerate_all || !added.is_empty() || styles_changed(imported, base, part);
        if changed {
            let (p, extra) = prefixes(&part.declarations, false);
            let attrs = root_attrs(part, &extra);
            let kinds = BTreeMap::new();
            let bytes = parts::styles_imported(
                &parts::ImportedStyles {
                    part,
                    prefixes: &p,
                    catalog: imported.styles.catalog(),
                    base: base.styles.catalog(),
                    added: &added,
                    kinds: &kinds,
                },
                &attrs,
                &mut fonts,
                &mut notices,
            );
            replace_part(&mut package, &part_uri(&part.uri)?, bytes);
            regenerated.push(part.uri.clone());
        } else {
            reused.push(part.uri.clone());
        }
    }

    // ---- settings.xml
    if let Some(part) = &imported.preserved.settings {
        let tab = imported.settings.default_tab_stop_twips;
        if options.regenerate_all || tab != base.settings.default_tab_stop_twips {
            let (p, extra) = prefixes(&part.declarations, false);
            let attrs = root_attrs(part, &extra);
            let bytes = parts::settings_imported(part, &attrs, &p, tab);
            replace_part(&mut package, &part_uri(&part.uri)?, bytes);
            regenerated.push(part.uri.clone());
        } else {
            reused.push(part.uri.clone());
        }
    }
    if imported.theme != base.theme {
        notices.push(WriteNotice::ThemeNotWritten);
    }

    let save = finish_package(&mut package, &main_uri, options, &mut notices)?;
    let opaque: Vec<String> = package
        .parts()
        .keys()
        .map(ToString::to_string)
        .filter(|u| !regenerated.contains(u) && !reused.contains(u))
        .collect();
    let bytes = package.write()?;
    Ok(WrittenDocx {
        bytes,
        notices: finish_notices(notices, dropped),
        regenerated_parts: regenerated,
        reused_parts: reused,
        opaque_parts: opaque,
        save,
    })
}

fn styles_changed(imported: &ImportedDocx, base: &Baseline, part: &PreservedPart) -> bool {
    let (cur, old) = (imported.styles.catalog(), base.styles.catalog());
    if cur.defaults() != old.defaults() {
        return true;
    }
    part.root.children.iter().any(|c| match c {
        super::preserve::PreservedChild::Element(e) if props::local_of(&e.name) == "style" => e
            .attrs
            .iter()
            .find(|a| props::local_of(&a.name) == "styleId")
            .is_some_and(|a| cur.style(&a.value) != old.style(&a.value)),
        _ => false,
    })
}

/// A numbering part for lists the model uses, when the package has none.
fn add_numbering_if_missing(
    package: &mut Package,
    main: &PartUri,
    lists: &BTreeSet<u32>,
) -> Result<Vec<u32>, WriteError> {
    let existing = package
        .relationships(Some(main))
        .iter()
        .find(|r| r.rel_type == REL_NUMBERING)
        .and_then(|r| r.resolved_target.clone());
    if let Some(uri) = existing {
        // The model holds list references, not definitions: a reference to a
        // list the copied numbering part does not define is reported.
        let defined = package
            .part(&uri)
            .map(|p| defined_num_ids(&p.bytes))
            .unwrap_or_default();
        return Ok(lists
            .iter()
            .copied()
            .filter(|id| !defined.contains(&(id + 1)))
            .collect());
    }
    let uri = part_uri("word/numbering.xml")?;
    package.insert_part(uri, CT_NUMBERING, parts::numbering_new(lists))?;
    package.add_relationship(
        Some(main.clone()),
        "rIdAtelierNumbering",
        REL_NUMBERING,
        "numbering.xml",
        TargetMode::Internal,
    )?;
    Ok(Vec::new())
}

/// `w:numId` of every `w:num` of a numbering part (any prefix).
fn defined_num_ids(bytes: &[u8]) -> BTreeSet<u32> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = BTreeSet::new();
    for tag in text.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if name.rsplit(':').next() != Some("num") {
            continue;
        }
        for attr in tag.split_whitespace().skip(1) {
            let Some((k, v)) = attr.split_once('=') else {
                continue;
            };
            if k.rsplit(':').next() == Some("numId") {
                if let Ok(n) = v.trim_matches(|c| matches!(c, '"' | '\'' | '/')).parse() {
                    out.insert(n);
                }
            }
        }
    }
    out
}

/// Writes a rich document created from scratch.
pub fn write_new_docx(
    input: &NewDocument<'_>,
    options: &DocxWriteOptions,
) -> Result<WrittenDocx, WriteError> {
    let mut notices = Vec::new();
    let mut fonts = BTreeSet::new();
    let secs = sections(input.document, &input.page);
    if secs.has_header_footer {
        notices.push(WriteNotice::HeaderFooterNotWritten);
    }
    let decls = [
        ("w".to_owned(), W_NS.to_owned()),
        ("r".to_owned(), R_NS.to_owned()),
        ("mc".to_owned(), MC_NS.to_owned()),
        ("w14".to_owned(), W14_NS.to_owned()),
    ];
    let mut g = Gen {
        doc: input.document,
        base: None,
        pres: None,
        p: Prefixes {
            w: "w".into(),
            r: "r".into(),
        },
        default_style: input.default_style.map(str::to_owned),
        out: Out::new(),
        notices: Vec::new(),
        dropped_notices: 0,
        links: Vec::new(),
        link_prefix: "rIdLink".into(),
        bookmark_ids: BTreeMap::new(),
        fonts: BTreeSet::new(),
        lists: BTreeSet::new(),
        max_output: options.max_part_bytes,
        failed: None,
        depth: 0,
        text_width: input.page.text_width(),
        sect_after: secs.after,
        table_stack: Vec::new(),
        tables_info: None,
        terminate_body: false,
    };
    g.out.declaration();
    let mut attrs: Vec<(String, String)> = decls
        .iter()
        .map(|(p, u)| (format!("xmlns:{p}"), u.clone()))
        .collect();
    attrs.push(("mc:Ignorable".into(), "w14".into()));
    g.out.start("w:document", as_refs(&attrs));
    g.out.start("w:body", std::iter::empty());
    g.terminate_body = true;
    g.emit_container(&[], &secs.blocks, None);
    g.out.raw(&parts::sect_pr(&input.page, None, false));
    g.out.end();
    g.out.end();
    if let Some(reason) = g.failed {
        return Err(WriteError::Limit(reason));
    }
    notices.append(&mut g.notices);
    let dropped = g.dropped_notices;
    fonts.append(&mut g.fonts);
    let lists = std::mem::take(&mut g.lists);
    let links = std::mem::take(&mut g.links);
    let document_xml = g.out.into_bytes();

    let styles_xml = parts::styles_new(
        &parts::NewStyles {
            catalog: input.styles.catalog(),
            names: input.style_names,
            next: input.next_styles,
            default_style: input.default_style,
        },
        &mut fonts,
        &mut notices,
    );
    let settings_xml = parts::settings_new(input.default_tab_stop);
    let fonts_xml = parts::font_table_new(&fonts);

    let variant = options.variant.unwrap_or(DocxVariant::Document);
    let main = part_uri("word/document.xml")?;
    let mut package = Package::new();
    package.insert_part(main.clone(), variant.content_type(), document_xml)?;
    package.add_relationship(
        None,
        "rId1",
        REL_DOC,
        "word/document.xml",
        TargetMode::Internal,
    )?;
    for (rid, uri, ct, rel, target, bytes) in [
        (
            "rId1",
            "word/styles.xml",
            CT_STYLES,
            REL_STYLES,
            "styles.xml",
            styles_xml,
        ),
        (
            "rId2",
            "word/settings.xml",
            CT_SETTINGS,
            REL_SETTINGS,
            "settings.xml",
            settings_xml,
        ),
        (
            "rId3",
            "word/fontTable.xml",
            CT_FONTS,
            REL_FONTS,
            "fontTable.xml",
            fonts_xml,
        ),
    ] {
        package.insert_part(part_uri(uri)?, ct, bytes)?;
        package.add_relationship(Some(main.clone()), rid, rel, target, TargetMode::Internal)?;
    }
    if !lists.is_empty() {
        add_numbering_if_missing(&mut package, &main, &lists)?;
    }
    for (rid, target) in links {
        package.add_relationship(
            Some(main.clone()),
            rid,
            REL_HYPERLINK,
            target,
            TargetMode::External,
        )?;
    }
    let save = finish_package(&mut package, &main, options, &mut notices)?;
    let bytes = package.write()?;
    Ok(WrittenDocx {
        bytes,
        notices: finish_notices(notices, dropped),
        regenerated_parts: vec![
            "word/document.xml".into(),
            "word/styles.xml".into(),
            "word/settings.xml".into(),
            "word/fontTable.xml".into(),
        ],
        reused_parts: Vec::new(),
        opaque_parts: Vec::new(),
        save,
    })
}

/// Prefix of the relationship identifiers given to new hyperlinks: the first of
/// `rIdAtelierLink`, `rIdAtelierLinkx`... that no relationship of the main part
/// already uses, so a document saved before can be saved again with new links.
fn free_link_prefix(package: &Package, main: &PartUri) -> String {
    let mut prefix = String::from("rIdAtelierLink");
    while package
        .relationships(Some(main))
        .iter()
        .any(|r| r.id.starts_with(&prefix))
    {
        prefix.push('x');
    }
    prefix
}
