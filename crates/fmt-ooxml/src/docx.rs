// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded semantic import for WordprocessingML documents.
//!
//! The original OPC package is retained in [`ImportedDocx::package`], including
//! parts that this version does not understand. This keeps import lossless at
//! the package boundary while exposing the supported text and style subset to
//! the rich text model.

#![forbid(unsafe_code)]

use crate::{OpcError, Package, Part, PartUri};
use atelier_font::FontFace;
use atelier_font::manager::{FontManager, FontSource};
use atelier_model::Transaction;
use atelier_text_model::rich::{
    Alignment, AnchoredObject, CharAttrs, Color, DocumentStyleDefaults, LineRule, LineSpacing,
    Node, NodeId, OpaqueAttr, ParaProps, Paragraph, ResolvedProperties, RichDocument, RichOp,
    Style, StyleCatalog, StyleCharAttrs, StyleEngine, StyleKind, Theme, ThemeColor, ThemeFont,
    Underline, VerticalAlign,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use atelier_xml::{Attribute, Event, Limits as XmlLimits, Name, Reader};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod preserve;
mod tables;
mod write;
pub use preserve::{
    MC_NS, Preserved, PreservedAttr, PreservedChild, PreservedElement, PreservedPart,
    interpreted_elements, para_mark_rpr_rank, ppr_rank, rpr_rank, sect_pr_rank,
};
pub use tables::{
    Border, Borders, CellMargins, CellProps, CellVerticalAlign, ConditionalFormat, ConditionalKind,
    HeightRule, ImportedCell, ImportedRow, ImportedTable, ImportedTables, ResolvedCell, RowHeight,
    RowProps, Shading, TableAlign, TableColor, TableLayout, TableLook, TableProps, TableStyleDef,
    TableWidth, VMerge,
};
pub use write::{
    DocxVariant, DocxWriteOptions, NewDocument, PageSetup, StyleDecl, WriteError, WriteNotice,
    WrittenDocx, write_docx, write_new_docx,
};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_STRICT_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const REL_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_DOC_STRICT: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const REL_FONTS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable";
const REL_BASE_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
const CONTENT_DOC: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// DOCX-specific ceilings applied in addition to the OPC and XML budgets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocxLimits {
    pub package: crate::Limits,
    pub xml: XmlLimits,
    pub max_paragraphs: usize,
    pub max_runs: usize,
    pub max_text_bytes: usize,
    pub max_styles: usize,
    pub max_fonts: usize,
    pub max_font_bytes: usize,
    /// Deepest nesting of tables inside table cells (a body table is level 1).
    pub max_table_depth: usize,
    /// Rows of one table.
    pub max_table_rows: usize,
    /// Grid columns of one table; larger spans and grids are clamped.
    pub max_table_columns: usize,
    /// Cells of all tables of the document.
    pub max_table_cells: usize,
    /// Total bytes of unknown XML copied verbatim (US-306), all parts.
    pub max_opaque_bytes: usize,
    /// Number of unknown elements copied verbatim (US-306), all parts.
    pub max_opaque_items: usize,
    /// Elements of the preserved source skeleton (US-306).
    pub max_preserved_nodes: usize,
}
impl Default for DocxLimits {
    fn default() -> Self {
        Self {
            package: crate::Limits::default(),
            xml: XmlLimits::default(),
            max_paragraphs: 1_000_000,
            max_runs: 4_000_000,
            max_text_bytes: 256 * 1024 * 1024,
            max_styles: 100_000,
            max_fonts: 1024,
            max_font_bytes: 256 * 1024 * 1024,
            max_table_depth: 32,
            max_table_rows: 1_000_000,
            max_table_columns: 1024,
            max_table_cells: 2_000_000,
            max_opaque_bytes: 128 * 1024 * 1024,
            max_opaque_items: 8_000_000,
            max_preserved_nodes: 32_000_000,
        }
    }
}

/// Contextual DOCX import failure. No partially imported document is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocxError {
    Opc(OpcError),
    Xml { part: String, message: String },
    MissingPart(&'static str),
    InvalidStructure { part: String, detail: &'static str },
    InvalidValue { part: String, detail: String },
    Limit(&'static str),
    Style(String),
    Model(String),
    Font(String),
}
impl fmt::Display for DocxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DOCX import error: {self:?}")
    }
}
impl std::error::Error for DocxError {}
impl From<OpcError> for DocxError {
    fn from(value: OpcError) -> Self {
        Self::Opc(value)
    }
}

/// Information that cannot be represented in the current rich model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportNotice {
    UnknownRelationship {
        source: String,
        rel_type: String,
    },
    UnsupportedElement {
        part: String,
        namespace: String,
        local_name: String,
    },
    ExternalRelationship {
        source: String,
        target: String,
    },
    OpaquePart {
        part: String,
        content_type: String,
    },
    UnsupportedProperty {
        part: String,
        namespace: String,
        local_name: String,
        value: String,
    },
}

/// Text produced by `w:br`/`w:cr`, retaining the source break type and location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedBreak {
    pub paragraph: NodeId,
    pub offset: usize,
    pub kind: String,
    pub break_type: Option<String>,
}

/// A Word symbol's source font and hexadecimal character code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedSymbol {
    pub paragraph: NodeId,
    pub offset: usize,
    pub font: String,
    pub character_code: String,
}

/// Run style references omitted from the editing model but needed for style resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedRunStyle {
    pub paragraph: NodeId,
    pub start: usize,
    pub end: usize,
    pub style_id: Option<String>,
}

/// Settings used by paragraph import and layout consumers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocxSettings {
    pub default_tab_stop_twips: Option<u32>,
    pub compatibility: BTreeMap<String, String>,
    pub latent_style_count: Option<usize>,
}

/// One latent style exception or the document-wide latent style defaults.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LatentStyleInfo {
    pub name: Option<String>,
    pub locked: Option<bool>,
    pub semi_hidden: Option<bool>,
    pub unhide_when_used: Option<bool>,
    pub ui_priority: Option<u32>,
}

/// Font table metadata retained alongside the original fontTable XML.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FontRecord {
    pub name: String,
    pub alternate_name: Option<String>,
    pub embedded_variants: Vec<EmbeddedFontRef>,
}

/// Location and obfuscation metadata for an embedded font variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedFontRef {
    pub variant: String,
    pub relationship_id: String,
    pub part: String,
    pub font_key: String,
    pub byte_index: usize,
}

/// A fully imported DOCX. `package` owns all original/unknown OPC parts and
/// `font_bytes` owns the bytes used to register embedded fonts.
pub struct ImportedDocx {
    pub package: Package,
    pub document: RichDocument,
    pub styles: StyleEngine,
    pub theme: Theme,
    pub font_bytes: Vec<Vec<u8>>,
    pub breaks: Vec<ImportedBreak>,
    pub symbols: Vec<ImportedSymbol>,
    pub run_styles: Vec<ImportedRunStyle>,
    pub notices: Vec<ImportNotice>,
    pub settings: DocxSettings,
    pub latent_styles: Vec<LatentStyleInfo>,
    pub font_records: Vec<FontRecord>,
    /// Table properties without a slot in the rich model, table styles and
    /// the paragraph-to-cell index (US-303).
    pub tables: ImportedTables,
    /// What the import does not interpret, with its position (US-306).
    pub preserved: Preserved,
    paragraph_order: Vec<NodeId>,
}
impl ImportedDocx {
    /// Resolve paragraph and run properties using the US-201 style engine.
    /// The returned runs contain effective character properties; the separate
    /// `ResolvedProperties` carries paragraph/default character properties.
    pub fn resolved_paragraph(
        &mut self,
        paragraph_id: NodeId,
    ) -> Result<(Paragraph, ResolvedProperties), DocxError> {
        let source = self
            .document
            .paragraph(paragraph_id)
            .map_err(model_error)?
            .clone();
        let mut default = self
            .styles
            .resolve(&self.document, paragraph_id, None, &CharAttrs::default())
            .map_err(|e| DocxError::Style(e.to_string()))?;
        // Conditional table-style formatting sits between the table style and
        // the paragraph style: it only fills what neither the paragraph style
        // nor direct formatting defines.
        let conditional = self
            .tables
            .paragraph_cell
            .get(&paragraph_id)
            .and_then(|cell| self.tables.resolve_cell(*cell, &self.theme))
            .filter(|cell| !cell.paragraph.is_empty() || !cell.character.is_empty());
        let (style_para, style_char) = tables::chain_formatting(
            self.styles.catalog(),
            source.props.style.as_deref(),
            &self.theme,
        );
        if let Some(cell) = &conditional {
            let mut para = cell.paragraph.clone();
            let mut upper = style_para.clone();
            let mut direct = source.props.clone();
            direct.style = None;
            upper.overlay(&direct);
            para.clear(&upper);
            default.paragraph.overlay(&para);
            let mut ch = cell.character.clone();
            ch.clear(&style_char);
            default.character.overlay(&ch);
        }
        let mut output = source.clone();
        let mut resolved_runs = Vec::new();
        let boundaries = grapheme_boundaries(&source.text);
        let paragraph_run_styles: Vec<_> = self
            .run_styles
            .iter()
            .filter(|run| run.paragraph == paragraph_id)
            .collect();
        let mut style_cursor = 0;
        for pair in boundaries.boundaries().windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let direct = source.attrs_at(start);
            while style_cursor < paragraph_run_styles.len()
                && paragraph_run_styles[style_cursor].end <= start
            {
                style_cursor += 1;
            }
            let style = paragraph_run_styles
                .get(style_cursor)
                .filter(|run| run.start <= start && run.end >= end)
                .and_then(|run| run.style_id.as_deref());
            let mut effective = self
                .styles
                .resolve(&self.document, paragraph_id, style, &direct)
                .map_err(|e| DocxError::Style(e.to_string()))?;
            if let Some(cell) = &conditional {
                let mut upper = style_char.clone();
                if let Some(id) = style {
                    let (_, c) =
                        tables::chain_formatting(self.styles.catalog(), Some(id), &self.theme);
                    upper.overlay(&c);
                }
                upper.overlay(&direct);
                let mut ch = cell.character.clone();
                ch.clear(&upper);
                effective.character.overlay(&ch);
            }
            if !effective.character.is_empty() {
                if let Some(last) = resolved_runs.last_mut() {
                    let last: &mut atelier_text_model::rich::CharRun = last;
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

    pub fn paragraph_ids(&self) -> &[NodeId] {
        &self.paragraph_order
    }

    /// Register owned document fonts in a borrowing font manager.
    pub fn register_fonts<'a>(&'a self, manager: &mut FontManager<'a>) -> Result<(), DocxError> {
        for bytes in &self.font_bytes {
            manager
                .register(bytes, 0, FontSource::Document)
                .map_err(|e| DocxError::Font(e.to_string()))?;
        }
        Ok(())
    }
}

/// Import a DOCX from bytes with default resource limits.
pub fn import_docx(bytes: &[u8]) -> Result<ImportedDocx, DocxError> {
    import_docx_with_limits(bytes, DocxLimits::default())
}

/// Import a DOCX from bytes with explicit, deterministic budgets.
pub fn import_docx_with_limits(
    bytes: &[u8],
    limits: DocxLimits,
) -> Result<ImportedDocx, DocxError> {
    let package = Package::read_with_limits(bytes, limits.package.clone())?;
    import_package(package, limits)
}

/// Import from an already read package (the writer re-imports the package of
/// an `ImportedDocx` to know what the source model was).
pub(crate) fn import_package(
    package: Package,
    limits: DocxLimits,
) -> Result<ImportedDocx, DocxError> {
    let mut notices = Vec::new();
    let main_rel = package
        .relationships(None)
        .iter()
        .find(|r| r.rel_type == REL_DOC || r.rel_type == REL_DOC_STRICT)
        .ok_or(DocxError::MissingPart("officeDocument relationship"))?;
    if main_rel.mode != crate::TargetMode::Internal {
        return Err(DocxError::InvalidValue {
            part: "_rels/.rels".into(),
            detail: "main document relationship must be internal".into(),
        });
    }
    let main_uri = main_rel
        .resolved_target
        .clone()
        .ok_or(DocxError::MissingPart("main document target"))?;
    let main = package
        .part(&main_uri)
        .ok_or(DocxError::MissingPart("main document target"))?;
    if !write::MAIN_CONTENT_TYPES.contains(&main.content_type.as_str()) {
        return Err(DocxError::InvalidStructure {
            part: main_uri.to_string(),
            detail: "main part has a non-DOCX content type",
        });
    }
    let mut budget = preserve::CaptureBudget::new(&limits, &Preserved::default());
    let main_xml = parse_xml_captured(main, &limits.xml, &mut budget)?;
    if !is_w(&main_xml, "document") {
        return Err(DocxError::InvalidStructure {
            part: main_uri.to_string(),
            detail: "main root is not WordprocessingML document",
        });
    }
    let mut styles_xml = None;
    let mut theme_xml = None;
    let mut font_table_xml = None;
    let mut settings_xml = None;
    for rel in package.relationships(Some(&main_uri)) {
        if rel.mode == crate::TargetMode::External {
            notices.push(ImportNotice::ExternalRelationship {
                source: main_uri.to_string(),
                target: rel.target.clone(),
            });
            continue;
        }
        let Some(target) = rel
            .resolved_target
            .as_ref()
            .and_then(|uri| package.part(uri))
        else {
            continue;
        };
        match rel.rel_type.as_str() {
            _ if rel.rel_type == REL_STYLES
                || rel.rel_type == format!("{REL_BASE_STRICT}styles") =>
            {
                let xml = parse_xml_captured(target, &limits.xml, &mut budget)?;
                if !is_w(&xml, "styles") {
                    return Err(DocxError::InvalidStructure {
                        part: target.uri.to_string(),
                        detail: "styles relationship target is not WordprocessingML styles",
                    });
                }
                styles_xml = Some((target, xml));
            }
            _ if rel.rel_type == REL_THEME || rel.rel_type == format!("{REL_BASE_STRICT}theme") => {
                let xml = parse_xml(target, &limits.xml)?;
                if xml.name.local != "theme" || xml.name.namespace.as_deref() != Some(A_NS) {
                    return Err(DocxError::InvalidStructure {
                        part: target.uri.to_string(),
                        detail: "theme relationship target is not DrawingML theme",
                    });
                }
                theme_xml = Some((target, xml));
            }
            _ if rel.rel_type == REL_SETTINGS
                || rel.rel_type == format!("{REL_BASE_STRICT}settings") =>
            {
                let xml = parse_xml_captured(target, &limits.xml, &mut budget)?;
                if !is_w(&xml, "settings") {
                    return Err(DocxError::InvalidStructure {
                        part: target.uri.to_string(),
                        detail: "settings relationship target is not WordprocessingML settings",
                    });
                }
                settings_xml = Some((target, xml));
            }
            _ if rel.rel_type == REL_FONTS
                || rel.rel_type == format!("{REL_BASE_STRICT}fontTable") =>
            {
                let xml = parse_xml(target, &limits.xml)?;
                if !is_w(&xml, "fonts") {
                    return Err(DocxError::InvalidStructure {
                        part: target.uri.to_string(),
                        detail: "font table relationship target is not WordprocessingML fonts",
                    });
                }
                font_table_xml = Some((target, xml));
            }
            _ => notices.push(ImportNotice::UnknownRelationship {
                source: main_uri.to_string(),
                rel_type: rel.rel_type.clone(),
            }),
        }
    }
    let mut pres = preserve::Pres::new(&limits);
    pres.decls = preserve::root_declarations(&main.bytes);
    collect_unsupported_resets(&main_xml, &main_uri, &mut notices);
    if let Some((part, xml)) = &styles_xml {
        collect_unsupported_resets(xml, &part.uri, &mut notices);
    }
    let (
        catalog,
        style_definitions,
        _paragraph_style_ids,
        default_paragraph_style,
        mut latent_styles,
    ) = match &styles_xml {
        Some((_, xml)) => parse_styles(xml, &limits)?,
        None => (
            StyleCatalog::new(DocumentStyleDefaults::default()),
            Vec::new(),
            BTreeSet::new(),
            None,
            Vec::new(),
        ),
    };
    let theme = if let Some((_, xml)) = theme_xml {
        parse_theme(&xml)?
    } else {
        Theme::default()
    };
    let mut style_engine = StyleEngine::new(catalog, theme.clone(), 1024);
    for style in style_definitions {
        style_engine
            .set_style(style)
            .map_err(|e| DocxError::Style(e.to_string()))?;
    }
    let mut fonts = Vec::new();
    let mut font_records = Vec::new();
    if let Some((font_part, xml)) = font_table_xml {
        let parsed = parse_embedded_fonts(font_part, &xml, &package, &limits)?;
        fonts = parsed.0;
        font_records = parsed.1;
    }
    let settings = if let Some((_, xml)) = &settings_xml {
        let (settings, mut latent) = parse_settings(xml)?;
        latent_styles.append(&mut latent);
        settings
    } else {
        DocxSettings::default()
    };

    let body = main_xml
        .find(W_NS, "body")
        .or_else(|| main_xml.find(W_STRICT_NS, "body"))
        .ok_or(DocxError::InvalidStructure {
            part: main_uri.to_string(),
            detail: "w:body is missing",
        })?;
    let mut paragraphs = Vec::new();
    collect_paragraphs(body, &mut paragraphs);
    collect_unsupported(&main_xml, &main_uri, &mut notices);
    if paragraphs.len() > limits.max_paragraphs {
        return Err(DocxError::Limit("paragraph count"));
    }
    let mut document = RichDocument::new();
    let section = *document
        .section_ids()
        .first()
        .ok_or(DocxError::InvalidStructure {
            part: main_uri.to_string(),
            detail: "rich document has no section",
        })?;
    let initial = document
        .block_ids(Some(section))
        .map_err(model_error)?
        .first()
        .copied()
        .ok_or(DocxError::InvalidStructure {
            part: main_uri.to_string(),
            detail: "rich section has no paragraph",
        })?;
    if let Some((part, xml)) = &styles_xml {
        let root = pres.prop_tree(xml, preserve::Level::StylesRoot)?;
        let info = pres.part(part.uri.as_str(), &part.bytes, xml, root);
        preserve::markup_notices(xml, &info, &mut notices);
        pres.out.styles = Some(info);
    }
    if let Some((part, xml)) = &settings_xml {
        let root = pres.prop_tree(xml, preserve::Level::SettingsRoot)?;
        let info = pres.part(part.uri.as_str(), &part.bytes, xml, root);
        preserve::markup_notices(xml, &info, &mut notices);
        pres.out.settings = Some(info);
    }
    pres.out.opaque_bytes = budget.bytes;
    pres.out.opaque_items = budget.items;
    let mut sink = ParagraphSink::new(&limits, &theme, default_paragraph_style.clone(), pres);
    let mut table_builder =
        tables::TableBuilder::new(&limits, style_engine.catalog(), &theme, main_uri.as_str());
    if let Some((part, xml)) = &styles_xml {
        table_builder.read_styles(xml, part.uri.as_str());
    }
    let events = preserve::events(body, preserve::Level::Body, &sink.pres.decls);
    let mut body_tree = preserve::TreeBuilder::new(sink.pres.shell(body)?);
    let mut position = 0usize;
    let mut initial_used = false;
    let mut pending: Vec<RichOp> = Vec::new();
    for event in &events {
        match event {
            preserve::Ev::Open(node, shadow) => {
                let mut element = sink.pres.shell(node)?;
                element.shadow = *shadow;
                body_tree.open(element);
            }
            preserve::Ev::Close => body_tree.close(),
            preserve::Ev::Prop(node) => {
                body_tree.push(PreservedChild::Opaque(preserve::xml_of(node)));
            }
            preserve::Ev::Opaque(node) if is_w(node, "sectPr") => {
                body_tree.push(PreservedChild::Opaque(preserve::xml_of(node)));
            }
            preserve::Ev::Opaque(node) => {
                let subtree = document
                    .new_opaque(&node.name.qualified(), &preserve::xml_of(node))
                    .map_err(model_error)?;
                body_tree.push(PreservedChild::Block(subtree.root));
                insert_block(&mut document, &mut pending, section, subtree, position)?;
                position += 1;
            }
            preserve::Ev::Known(xml_table) if is_w(xml_table, "tbl") => {
                if let Some(subtree) =
                    table_builder.build_table(&mut document, &mut sink, xml_table)?
                {
                    body_tree.push(PreservedChild::Block(subtree.root));
                    insert_block(&mut document, &mut pending, section, subtree, position)?;
                    position += 1;
                }
            }
            preserve::Ev::Known(xml_paragraph) => {
                let (mut paragraph, meta) = sink.read(&mut document, xml_paragraph)?;
                let para_id = if !initial_used {
                    initial_used = true;
                    initial
                } else {
                    let mut subtree = document.new_paragraph("").map_err(model_error)?;
                    let id = subtree.root;
                    if let Some(Node::Paragraph(p)) = subtree.nodes.get_mut(&id) {
                        *p = paragraph.clone();
                    }
                    insert_block(&mut document, &mut pending, section, subtree, position)?;
                    id
                };
                // Set text and source formatting in a single transaction. All run
                // boundaries are normalized to grapheme boundaries after concatenation.
                if para_id == initial {
                    let text = std::mem::take(&mut paragraph.text);
                    let mut ops = vec![RichOp::SetParaProps {
                        para: para_id,
                        set: paragraph.props.clone(),
                        clear: ParaProps::default(),
                    }];
                    if !text.is_empty() {
                        ops.push(RichOp::InsertText {
                            para: para_id,
                            offset: 0,
                            text,
                        });
                    }
                    for r in paragraph.runs.drain(..) {
                        if r.start < r.end {
                            ops.push(RichOp::SetCharAttrs {
                                para: para_id,
                                start: r.start,
                                end: r.end,
                                set: r.attrs,
                                clear: CharAttrs::default(),
                            });
                        }
                    }
                    for object in paragraph.anchored.drain(..) {
                        ops.push(RichOp::InsertInline {
                            para: para_id,
                            item: atelier_text_model::rich::Inline::Anchored(object),
                        });
                    }
                    document
                        .apply(&Transaction::new(ops).map_err(|e| DocxError::Model(e.to_string()))?)
                        .map_err(model_error)?;
                }
                body_tree.push(PreservedChild::Block(para_id));
                sink.record(para_id, meta);
                position += 1;
            }
        }
    }
    flush_blocks(&mut document, &mut pending)?;
    let body_element = body_tree.finish();
    {
        let root = sink.pres.shell(&main_xml)?;
        let mut tree = preserve::TreeBuilder::new(root);
        for child in &main_xml.children {
            if is_w(child, "body") {
                tree.push(PreservedChild::Element(body_element.clone()));
            } else {
                tree.push(PreservedChild::Opaque(preserve::xml_of(child)));
            }
        }
        let info = sink
            .pres
            .part(main_uri.as_str(), &main.bytes, &main_xml, tree.finish());
        preserve::markup_notices(&main_xml, &info, &mut notices);
        sink.pres.out.document = Some(info);
    }
    let (table_info, table_notices) = table_builder.finish();
    notices.extend(table_notices);
    let ParagraphSink {
        breaks: break_records,
        symbols: symbol_records,
        run_styles: run_style_records,
        pres,
        ..
    } = sink;
    // Every paragraph of the document, in document order: cells, the empty
    // paragraph kept after a final table and padding paragraphs included.
    let ids = document.paragraph_ids();
    document.validate().map_err(model_error)?;
    for part in package.parts().values() {
        if part.uri != main_uri
            && part.content_type.contains("xml")
            && !["styles", "theme", "settings", "fontTable"]
                .iter()
                .any(|s| part.uri.as_str().to_ascii_lowercase().contains(s))
        {
            notices.push(ImportNotice::OpaquePart {
                part: part.uri.to_string(),
                content_type: part.content_type.clone(),
            });
        }
    }
    // Parse validation above makes malformed font relationships fail before
    // publishing a result; actual registration is caller-owned and borrow-safe.
    let _ = &mut style_engine;
    Ok(ImportedDocx {
        package,
        document,
        styles: style_engine,
        theme,
        font_bytes: fonts,
        breaks: break_records,
        symbols: symbol_records,
        run_styles: run_style_records,
        notices,
        settings,
        latent_styles,
        font_records,
        tables: table_info,
        preserved: pres.out,
        paragraph_order: ids,
    })
}

#[derive(Clone, Debug)]
struct XmlNode {
    name: Name,
    attrs: Vec<Attribute>,
    children: Vec<XmlNode>,
    text: String,
    /// The element and its subtree as a US-032 opaque copy (US-306), when the
    /// import does not interpret it. `children` stay available for notices.
    opaque: Option<Vec<u8>>,
}
impl XmlNode {
    fn find(&self, ns: &str, local: &str) -> Option<&XmlNode> {
        self.children
            .iter()
            .find(|n| n.name.local == local && n.name.namespace.as_deref() == Some(ns))
    }
    fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name.local == local && a.name.namespace.as_deref() == Some(ns))
            .map(|a| a.value.as_str())
    }
    fn no_ns_attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name.local == local && a.name.namespace.is_none())
            .map(|a| a.value.as_str())
    }
    fn w_attr(&self, local: &str) -> Option<&str> {
        self.attr(W_NS, local)
            .or_else(|| self.attr(W_STRICT_NS, local))
    }
    fn descendants<'a>(&'a self, ns: &'a str, local: &'a str, out: &mut Vec<&'a XmlNode>) {
        for child in &self.children {
            if child.name.local == local && child.name.namespace.as_deref() == Some(ns) {
                out.push(child);
            }
            child.descendants(ns, local, out);
        }
    }
}
/// Blocks inserted per transaction. A transaction takes a checkpoint of the
/// whole document, so one transaction per block copies the document (and every
/// captured byte in it) once per block: quadratic in the number of blocks.
const INSERT_BATCH: usize = 512;

fn insert_block(
    document: &mut RichDocument,
    pending: &mut Vec<RichOp>,
    section: NodeId,
    subtree: atelier_text_model::rich::Subtree,
    position: usize,
) -> Result<(), DocxError> {
    pending.push(RichOp::InsertBlock {
        parent: Some(section),
        position,
        subtree,
    });
    if pending.len() >= INSERT_BATCH {
        flush_blocks(document, pending)?;
    }
    Ok(())
}

fn flush_blocks(document: &mut RichDocument, pending: &mut Vec<RichOp>) -> Result<(), DocxError> {
    if pending.is_empty() {
        return Ok(());
    }
    let ops = std::mem::take(pending);
    document
        .apply(&Transaction::new(ops).map_err(|e| DocxError::Model(e.to_string()))?)
        .map_err(model_error)?;
    Ok(())
}
fn parse_xml(part: &Part, limits: &XmlLimits) -> Result<XmlNode, DocxError> {
    if part.bytes.len() > 16 * 1024 * 1024 {
        return Err(DocxError::Limit("XML part bytes"));
    }
    parse_xml_bytes(&part.bytes, &part.uri, limits, None)
}
/// Like [`parse_xml`], also copying the elements the import does not
/// interpret (see `preserve::classify`) as opaque subtrees.
fn parse_xml_captured(
    part: &Part,
    limits: &XmlLimits,
    budget: &mut preserve::CaptureBudget,
) -> Result<XmlNode, DocxError> {
    if part.bytes.len() > 16 * 1024 * 1024 {
        return Err(DocxError::Limit("XML part bytes"));
    }
    budget.context = preserve::root_declarations(&part.bytes);
    parse_xml_bytes(&part.bytes, &part.uri, limits, Some(budget))
}
fn parse_xml_bytes(
    bytes: &[u8],
    uri: &PartUri,
    limits: &XmlLimits,
    mut capture: Option<&mut preserve::CaptureBudget>,
) -> Result<XmlNode, DocxError> {
    let mut reader = Reader::with_limits(bytes, limits.clone());
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root = None;
    let xml_error = |e: atelier_xml::XmlError| DocxError::Xml {
        part: uri.to_string(),
        message: e.to_string(),
    };
    while let Some(event) = reader.next_event().map_err(xml_error)? {
        match event {
            Event::StartElement {
                name,
                attributes,
                empty,
            } => {
                if let Some(budget) = capture.as_deref_mut() {
                    let captured = preserve::level_of_children(&stack).is_some_and(|level| {
                        preserve::classify(level, &name) == preserve::Class::Capture
                    });
                    if captured {
                        let subtree = reader.capture_opaque().map_err(xml_error)?;
                        let xml = preserve::subtree_bytes(&subtree)?;
                        // The root declarations are kept once per part, not in
                        // every copy (see `PreservedPart::standalone`).
                        let xml = preserve::strip_context(xml, &budget.context);
                        budget.charge(xml.len())?;
                        let wrapped = preserve::reparse_input(&xml, &budget.context);
                        let mut node = parse_xml_bytes(&wrapped, uri, limits, None)?
                            .children
                            .pop()
                            .ok_or(DocxError::InvalidStructure {
                                part: uri.to_string(),
                                detail: "empty opaque capture",
                            })?;
                        node.opaque = Some(xml);
                        attach_node(node, &mut stack, &mut root, uri)?;
                        continue;
                    }
                }
                let node = XmlNode {
                    name,
                    attrs: attributes,
                    children: Vec::new(),
                    text: String::new(),
                    opaque: None,
                };
                if empty {
                    attach_node(node, &mut stack, &mut root, uri)?;
                } else {
                    stack.push(node);
                }
            }
            Event::EndElement { .. } => {
                let node = stack.pop().ok_or(DocxError::InvalidStructure {
                    part: uri.to_string(),
                    detail: "unbalanced XML event stream",
                })?;
                attach_node(node, &mut stack, &mut root, uri)?;
            }
            Event::Text(text) => {
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&text);
                }
            }
            // Not preserved outside the copies of captured elements (US-306 doc).
            Event::Comment(_) | Event::ProcessingInstruction { .. } => {}
        }
    }
    if !stack.is_empty() {
        return Err(DocxError::InvalidStructure {
            part: uri.to_string(),
            detail: "XML is incomplete",
        });
    }
    root.ok_or(DocxError::InvalidStructure {
        part: uri.to_string(),
        detail: "XML has no root element",
    })
}
fn attach_node(
    node: XmlNode,
    stack: &mut [XmlNode],
    root: &mut Option<XmlNode>,
    uri: &PartUri,
) -> Result<(), DocxError> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(DocxError::InvalidStructure {
            part: uri.to_string(),
            detail: "XML has multiple root elements",
        });
    }
    Ok(())
}

fn collect_paragraphs<'a>(node: &'a XmlNode, out: &mut Vec<&'a XmlNode>) {
    for child in &node.children {
        if !matches!(child.name.namespace.as_deref(), Some(W_NS | W_STRICT_NS))
            || is_w(child, "del")
            || is_w(child, "moveFrom")
        {
            continue;
        }
        if is_w(child, "p") {
            out.push(child);
        } else {
            collect_paragraphs(child, out);
        }
    }
}
fn collect_unsupported(root: &XmlNode, part: &PartUri, notices: &mut Vec<ImportNotice>) {
    const HANDLED: &[&str] = &[
        "document",
        "body",
        "p",
        "pPr",
        "r",
        "rPr",
        "t",
        "tab",
        "br",
        "cr",
        "sym",
        "noBreakHyphen",
        "softHyphen",
        "b",
        "i",
        "strike",
        "dstrike",
        "u",
        "vertAlign",
        "rFonts",
        "sz",
        "szCs",
        "color",
        "highlight",
        "pStyle",
        "jc",
        "ind",
        "spacing",
        "sectPr",
    ];
    fn walk(
        node: &XmlNode,
        part: &PartUri,
        seen: &mut BTreeSet<(String, String)>,
        out: &mut Vec<ImportNotice>,
    ) {
        if (node.name.namespace.as_deref() == Some(W_NS)
            || node.name.namespace.as_deref() == Some(W_STRICT_NS))
            && !HANDLED.contains(&node.name.local.as_str())
            && !tables::is_handled_element(&node.name.local)
            && seen.insert((
                node.name.namespace.clone().unwrap_or_default(),
                node.name.local.clone(),
            ))
        {
            out.push(ImportNotice::UnsupportedElement {
                part: part.to_string(),
                namespace: node.name.namespace.clone().unwrap_or_default(),
                local_name: node.name.local.clone(),
            });
        }
        for child in &node.children {
            walk(child, part, seen, out);
        }
    }
    let mut seen = BTreeSet::new();
    walk(root, part, &mut seen, notices);
}
fn collect_unsupported_resets(root: &XmlNode, part: &PartUri, notices: &mut Vec<ImportNotice>) {
    fn walk(node: &XmlNode, part: &PartUri, out: &mut Vec<ImportNotice>) {
        let value = match node.name.local.as_str() {
            "color" if is_w(node, "color") && node.w_attr("val") == Some("auto") => Some("auto"),
            "highlight" if is_w(node, "highlight") && node.w_attr("val") == Some("none") => {
                Some("none")
            }
            "szCs" if is_w(node, "szCs") => node.w_attr("val"),
            _ => None,
        };
        if let Some(value) = value {
            out.push(ImportNotice::UnsupportedProperty {
                part: part.to_string(),
                namespace: node.name.namespace.clone().unwrap_or_default(),
                local_name: node.name.local.clone(),
                value: value.to_owned(),
            });
        }
        for child in &node.children {
            walk(child, part, out);
        }
    }
    walk(root, part, notices);
}
type ParagraphImport = (
    Paragraph,
    Vec<(usize, usize, Option<String>)>,
    Vec<(usize, String, Option<String>)>,
    Vec<(usize, String, String)>,
    PreservedElement,
);

fn parse_paragraph(
    node: &XmlNode,
    limits: &DocxLimits,
    run_count: &mut usize,
    theme: &Theme,
    document: &mut RichDocument,
    pres: &mut preserve::Pres,
) -> Result<ParagraphImport, DocxError> {
    let mut paragraph = Paragraph::default();
    let ppr = node
        .find(W_NS, "pPr")
        .or_else(|| node.find(W_STRICT_NS, "pPr"));
    if let Some(ppr) = ppr {
        paragraph.props = parse_para_props(ppr)?;
    }
    let mut tree = preserve::TreeBuilder::new(pres.shell(node)?);
    let mut source_metadata = Vec::new();
    let mut breaks = Vec::new();
    let mut symbols = Vec::new();
    let events = preserve::events(node, preserve::Level::Para, &pres.decls);
    let mut source_runs = Vec::new();
    for event in events {
        let child = match event {
            preserve::Ev::Known(c) if is_w(c, "pPr") => {
                tree.push(if ppr.is_some_and(|first| std::ptr::eq(first, c)) {
                    PreservedChild::Element(pres.prop_tree(c, preserve::Level::PPr)?)
                } else {
                    PreservedChild::Opaque(preserve::xml_of(c))
                });
                continue;
            }
            preserve::Ev::Known(c) => c,
            preserve::Ev::Open(c, shadow) => {
                let mut element = pres.shell(c)?;
                element.shadow = shadow;
                tree.open(element);
                continue;
            }
            preserve::Ev::Close => {
                tree.close();
                continue;
            }
            preserve::Ev::Prop(c) => {
                tree.push(PreservedChild::Opaque(preserve::xml_of(c)));
                continue;
            }
            preserve::Ev::Opaque(c) => {
                let id = document.alloc_id().map_err(model_error)?;
                paragraph.anchored.push(AnchoredObject {
                    id,
                    offset: paragraph.text.len(),
                    kind: c.name.qualified(),
                    data: preserve::xml_of(c),
                });
                tree.push(PreservedChild::Anchored(id));
                continue;
            }
        };
        let mut run_tree = preserve::TreeBuilder::new(pres.shell(child)?);
        *run_count = run_count
            .checked_add(1)
            .ok_or(DocxError::Limit("run count"))?;
        if *run_count > limits.max_runs {
            return Err(DocxError::Limit("run count"));
        }
        let start = paragraph.text.len();
        let rpr = child
            .find(W_NS, "rPr")
            .or_else(|| child.find(W_STRICT_NS, "rPr"));
        let attrs = match rpr {
            Some(rpr) => parse_char_attrs(rpr, theme)?,
            None => CharAttrs::default(),
        };
        let style_id = rpr
            .and_then(|r| {
                r.find(W_NS, "rStyle")
                    .or_else(|| r.find(W_STRICT_NS, "rStyle"))
            })
            .and_then(|s| s.w_attr("val"))
            .map(str::to_owned);
        for item in &child.children {
            let item_start = paragraph.text.len();
            let mut leaf = true;
            match item.name.local.as_str() {
                "rPr" if is_w(item, "rPr") => {
                    leaf = false;
                    run_tree.push(if rpr.is_some_and(|first| std::ptr::eq(first, item)) {
                        PreservedChild::Element(pres.prop_tree(item, preserve::Level::RPr)?)
                    } else {
                        PreservedChild::Opaque(preserve::xml_of(item))
                    });
                }
                "t" if is_w(item, "t") => paragraph.text.push_str(&item.text),
                "tab" if is_w(item, "tab") => paragraph.text.push('\t'),
                "br" | "cr" if is_w(item, "br") || is_w(item, "cr") => {
                    let offset = paragraph.text.len();
                    paragraph.text.push('\n');
                    breaks.push((
                        offset,
                        item.name.local.clone(),
                        item.w_attr("type").map(str::to_owned),
                    ));
                }
                "noBreakHyphen" if is_w(item, "noBreakHyphen") => paragraph.text.push('\u{2011}'),
                "softHyphen" if is_w(item, "softHyphen") => paragraph.text.push('\u{00ad}'),
                "sym" if is_w(item, "sym") => {
                    let code = item.w_attr("char").ok_or_else(|| DocxError::InvalidValue {
                        part: "word/document.xml".into(),
                        detail: "w:sym has no w:char".into(),
                    })?;
                    let scalar = u32::from_str_radix(code, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| DocxError::InvalidValue {
                            part: "word/document.xml".into(),
                            detail: format!("invalid w:sym character {code:?}"),
                        })?;
                    let font = item
                        .w_attr("font")
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| DocxError::InvalidValue {
                            part: "word/document.xml".into(),
                            detail: "w:sym has no w:font".into(),
                        })?
                        .to_owned();
                    let offset = paragraph.text.len();
                    paragraph.text.push(scalar);
                    symbols.push((offset, font, code.to_owned()));
                }
                _ => {
                    leaf = false;
                    let id = document.alloc_id().map_err(model_error)?;
                    paragraph.anchored.push(AnchoredObject {
                        id,
                        offset: paragraph.text.len(),
                        kind: item.name.qualified(),
                        data: preserve::xml_of(item),
                    });
                    run_tree.push(PreservedChild::Anchored(id));
                }
            }
            if leaf {
                let mut element = pres.shell(item)?;
                element.range = Some((item_start, paragraph.text.len()));
                // An element the schema gives no children (invalid input):
                // keep them as copies after the text the element produced.
                for child in &item.children {
                    element
                        .children
                        .push(PreservedChild::Opaque(preserve::xml_of(child)));
                }
                run_tree.push(PreservedChild::Element(element));
            }
        }
        let end = paragraph.text.len();
        let mut run_element = run_tree.finish();
        run_element.range = Some((start, end));
        tree.push(PreservedChild::Element(run_element));
        if end > start {
            source_runs.push(atelier_text_model::rich::CharRun { start, end, attrs });
        }
        source_metadata.push((start, end, style_id));
    }
    // Normalize ranges to full extended grapheme clusters. A combining mark in
    // a later XML run inherits the first cluster's style, preventing invalid
    // UTF-8/grapheme offsets in the rich model.
    let bounds = grapheme_boundaries(&paragraph.text);
    paragraph.runs.clear();
    let mut source_cursor = 0;
    let mut metadata_cursor = 0;
    let mut normalized_metadata: Vec<(usize, usize, Option<String>)> = Vec::new();
    for pair in bounds.boundaries().windows(2) {
        let (start, end) = (pair[0], pair[1]);
        while source_cursor < source_runs.len() && source_runs[source_cursor].end <= start {
            source_cursor += 1;
        }
        if let Some(source) = source_runs
            .get(source_cursor)
            .filter(|run| run.start <= start && start < run.end)
        {
            if !source.attrs.is_empty() {
                if let Some(last) = paragraph.runs.last_mut() {
                    if last.end == start && last.attrs == source.attrs {
                        last.end = end;
                    } else {
                        paragraph.runs.push(atelier_text_model::rich::CharRun {
                            start,
                            end,
                            attrs: source.attrs.clone(),
                        });
                    }
                } else {
                    paragraph.runs.push(atelier_text_model::rich::CharRun {
                        start,
                        end,
                        attrs: source.attrs.clone(),
                    });
                }
            }
        }
        while metadata_cursor < source_metadata.len() && source_metadata[metadata_cursor].1 <= start
        {
            metadata_cursor += 1;
        }
        if let Some(source) = source_metadata
            .get(metadata_cursor)
            .filter(|run| run.0 <= start && start < run.1)
        {
            if let Some(last) = normalized_metadata.last_mut() {
                if last.1 == start && last.2 == source.2 {
                    last.1 = end;
                    continue;
                }
            }
            normalized_metadata.push((start, end, source.2.clone()));
        }
    }
    let metadata = normalized_metadata;
    for object in &mut paragraph.anchored {
        object.offset = bounds.floor_boundary(object.offset).unwrap_or(0);
    }
    paragraph.anchored.sort_by_key(|x| (x.offset, x.id));
    for (offset, font, _) in &symbols {
        let char_bytes = paragraph.text[*offset..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        let start = bounds.floor_boundary(*offset).unwrap_or(0);
        let end = bounds
            .ceil_boundary(offset.saturating_add(char_bytes))
            .unwrap_or(paragraph.text.len());
        let mut replacement = Vec::new();
        let mut covered = false;
        for run in paragraph.runs.drain(..) {
            if run.end <= start || run.start >= end {
                replacement.push(run);
                continue;
            }
            if run.start < start {
                replacement.push(atelier_text_model::rich::CharRun {
                    start: run.start,
                    end: start,
                    attrs: run.attrs.clone(),
                });
            }
            let mut attrs = run.attrs.clone();
            attrs.font = Some(font.clone());
            replacement.push(atelier_text_model::rich::CharRun { start, end, attrs });
            covered = true;
            if run.end > end {
                replacement.push(atelier_text_model::rich::CharRun {
                    start: end,
                    end: run.end,
                    attrs: run.attrs,
                });
            }
        }
        if !covered {
            replacement.push(atelier_text_model::rich::CharRun {
                start,
                end,
                attrs: CharAttrs {
                    font: Some(font.clone()),
                    ..CharAttrs::default()
                },
            });
        }
        replacement.sort_by_key(|r| (r.start, r.end));
        paragraph.runs = replacement;
    }
    Ok((paragraph, metadata, breaks, symbols, tree.finish()))
}
/// Paragraph metadata collected while reading a `w:p`.
struct ParagraphMeta {
    element: PreservedElement,
    runs: Vec<(usize, usize, Option<String>)>,
    breaks: Vec<(usize, String, Option<String>)>,
    symbols: Vec<(usize, String, String)>,
}

/// Shared accounting and records for every paragraph of the document, whether
/// it sits in the body or in a table cell.
struct ParagraphSink<'a> {
    limits: &'a DocxLimits,
    theme: &'a Theme,
    default_style: Option<String>,
    run_count: usize,
    total_text: usize,
    breaks: Vec<ImportedBreak>,
    symbols: Vec<ImportedSymbol>,
    run_styles: Vec<ImportedRunStyle>,
    pres: preserve::Pres,
}
impl<'a> ParagraphSink<'a> {
    fn new(
        limits: &'a DocxLimits,
        theme: &'a Theme,
        default_style: Option<String>,
        pres: preserve::Pres,
    ) -> Self {
        Self {
            pres,
            limits,
            theme,
            default_style,
            run_count: 0,
            total_text: 0,
            breaks: Vec::new(),
            symbols: Vec::new(),
            run_styles: Vec::new(),
        }
    }
    fn read(
        &mut self,
        document: &mut RichDocument,
        node: &XmlNode,
    ) -> Result<(Paragraph, ParagraphMeta), DocxError> {
        let (mut paragraph, runs, breaks, symbols, element) = parse_paragraph(
            node,
            self.limits,
            &mut self.run_count,
            self.theme,
            document,
            &mut self.pres,
        )?;
        if paragraph.props.style.is_none() {
            paragraph.props.style = self.default_style.clone();
        }
        self.total_text = self
            .total_text
            .checked_add(paragraph.text.len())
            .ok_or(DocxError::Limit("text bytes"))?;
        if self.total_text > self.limits.max_text_bytes {
            return Err(DocxError::Limit("text bytes"));
        }
        Ok((
            paragraph,
            ParagraphMeta {
                element,
                runs,
                breaks,
                symbols,
            },
        ))
    }
    fn record(&mut self, para_id: NodeId, meta: ParagraphMeta) {
        let mut element = meta.element;
        element.node = Some(para_id);
        self.pres.out.paragraphs.insert(para_id, element);
        for (start, end, style_id) in meta.runs {
            self.run_styles.push(ImportedRunStyle {
                paragraph: para_id,
                start,
                end,
                style_id,
            });
        }
        for (offset, kind, break_type) in meta.breaks {
            self.breaks.push(ImportedBreak {
                paragraph: para_id,
                offset,
                kind,
                break_type,
            });
        }
        for (offset, font, character_code) in meta.symbols {
            self.symbols.push(ImportedSymbol {
                paragraph: para_id,
                offset,
                font,
                character_code,
            });
        }
    }
}
fn is_w(node: &XmlNode, local: &str) -> bool {
    node.name.local == local && matches!(node.name.namespace.as_deref(), Some(W_NS | W_STRICT_NS))
}

fn parse_on_off(value: Option<&str>, default: bool) -> Result<bool, DocxError> {
    match value {
        None => Ok(default),
        Some("1" | "true" | "on") => Ok(true),
        Some("0" | "false" | "off") => Ok(false),
        Some(v) => Err(DocxError::InvalidValue {
            part: "WordprocessingML".into(),
            detail: format!("invalid on/off value {v:?}"),
        }),
    }
}
fn bool_val(node: &XmlNode) -> Result<Option<bool>, DocxError> {
    Ok(Some(parse_on_off(node.w_attr("val"), true)?))
}
fn parse_char_attrs(node: &XmlNode, theme: &Theme) -> Result<CharAttrs, DocxError> {
    let mut a = CharAttrs::default();
    for c in &node.children {
        if !is_w(c, &c.name.local) {
            continue;
        }
        match c.name.local.as_str() {
            "b" => a.bold = bool_val(c)?,
            "i" => a.italic = bool_val(c)?,
            "strike" | "dstrike" => a.strike = bool_val(c)?,
            "u" => {
                a.underline = Some(match c.w_attr("val").unwrap_or("single") {
                    "none" | "0" => Underline::None,
                    "double" => Underline::Double,
                    "thick" => Underline::Thick,
                    "dotted" => Underline::Dotted,
                    "dash" | "dashed" => Underline::Dashed,
                    "wave" => Underline::Wave,
                    _ => Underline::Single,
                })
            }
            "vertAlign" => {
                a.vertical_align = match c.w_attr("val") {
                    Some("superscript") => Some(VerticalAlign::Superscript),
                    Some("subscript") => Some(VerticalAlign::Subscript),
                    Some("baseline") => Some(VerticalAlign::Baseline),
                    _ => None,
                }
            }
            "rFonts" => {
                let theme_font = c
                    .w_attr("asciiTheme")
                    .or_else(|| c.w_attr("hAnsiTheme"))
                    .and_then(theme_font_slot);
                a.font = theme_font
                    .and_then(|slot| theme.fonts.get(&slot).cloned())
                    .or_else(|| {
                        c.w_attr("ascii")
                            .or_else(|| c.w_attr("hAnsi"))
                            .map(str::to_owned)
                    });
            }
            "sz" => {
                a.size_half_points = c
                    .w_attr("val")
                    .map(|v| parse_u32_value(v, "run size"))
                    .transpose()?;
            }
            "szCs" => {
                if let Some(value) = c.w_attr("val") {
                    a.opaque.push(OpaqueAttr {
                        namespace: W_NS.into(),
                        name: "szCs".into(),
                        value: value.as_bytes().to_vec(),
                    });
                }
            }
            "color" => {
                a.color = if let Some(theme_value) = c.w_attr("themeColor") {
                    let slot =
                        theme_color_slot(theme_value).ok_or_else(|| DocxError::InvalidValue {
                            part: "WordprocessingML".into(),
                            detail: format!("unknown theme color {theme_value:?}"),
                        })?;
                    theme.colors.get(&slot).copied()
                } else if c.w_attr("val") == Some("auto") {
                    a.opaque.push(OpaqueAttr {
                        namespace: W_NS.into(),
                        name: "color".into(),
                        value: b"auto".to_vec(),
                    });
                    None
                } else if let Some(slot) = c.w_attr("val").and_then(theme_color_slot) {
                    theme.colors.get(&slot).copied()
                } else {
                    parse_color(c.w_attr("val"))?
                };
            }
            "highlight" => {
                let value = c.w_attr("val").unwrap_or("yellow");
                if value == "none" {
                    a.opaque.push(OpaqueAttr {
                        namespace: W_NS.into(),
                        name: "highlight".into(),
                        value: b"none".to_vec(),
                    });
                } else {
                    a.highlight =
                        Some(
                            parse_highlight(value).ok_or_else(|| DocxError::InvalidValue {
                                part: "WordprocessingML".into(),
                                detail: format!("invalid highlight color {value:?}"),
                            })?,
                        );
                }
            }
            _ => {}
        }
    }
    a.opaque
        .sort_by(|left, right| (&left.namespace, &left.name).cmp(&(&right.namespace, &right.name)));
    Ok(a)
}
fn parse_para_props(node: &XmlNode) -> Result<ParaProps, DocxError> {
    let mut p = ParaProps::default();
    for c in &node.children {
        if !is_w(c, &c.name.local) {
            continue;
        }
        match c.name.local.as_str() {
            "pStyle" => p.style = c.w_attr("val").map(str::to_owned),
            "jc" => {
                p.alignment = match c.w_attr("val") {
                    Some("center") => Some(Alignment::Center),
                    Some("right" | "end") => Some(Alignment::Right),
                    Some("both" | "distribute") => Some(Alignment::Justify),
                    Some(_) => Some(Alignment::Left),
                    None => None,
                }
            }
            "ind" => {
                p.indent_left = c
                    .w_attr("left")
                    .or_else(|| c.w_attr("start"))
                    .map(|v| parse_i32_value(v, "indent left"))
                    .transpose()?;
                p.indent_right = c
                    .w_attr("right")
                    .or_else(|| c.w_attr("end"))
                    .map(|v| parse_i32_value(v, "indent right"))
                    .transpose()?;
                p.indent_first_line = if let Some(value) = c.w_attr("firstLine") {
                    Some(parse_i32_value(value, "firstLine")?)
                } else if let Some(value) = c.w_attr("hanging") {
                    Some(
                        parse_i32_value(value, "hanging")?
                            .checked_neg()
                            .ok_or_else(|| invalid_value("hanging overflows i32"))?,
                    )
                } else {
                    None
                };
            }
            "spacing" => {
                p.space_before = c
                    .w_attr("before")
                    .map(|v| parse_u32_value(v, "spacing before"))
                    .transpose()?;
                p.space_after = c
                    .w_attr("after")
                    .map(|v| parse_u32_value(v, "spacing after"))
                    .transpose()?;
                p.line_spacing = c
                    .w_attr("line")
                    .map(|v| parse_u32_value(v, "spacing line"))
                    .transpose()?
                    .map(|value| match c.w_attr("lineRule") {
                        Some("exact") => LineSpacing {
                            rule: LineRule::Exact,
                            value,
                        },
                        Some("atLeast") => LineSpacing {
                            rule: LineRule::AtLeast,
                            value,
                        },
                        _ => LineSpacing {
                            rule: LineRule::Auto,
                            value,
                        },
                    });
            }
            _ => {}
        }
    }
    Ok(p)
}
fn invalid_value(detail: &str) -> DocxError {
    DocxError::InvalidValue {
        part: "WordprocessingML".into(),
        detail: detail.into(),
    }
}
fn parse_i32_value(value: &str, field: &str) -> Result<i32, DocxError> {
    value
        .parse()
        .map_err(|_| invalid_value(&format!("invalid {field} value {value:?}")))
}
fn parse_u32_value(value: &str, field: &str) -> Result<u32, DocxError> {
    value
        .parse()
        .map_err(|_| invalid_value(&format!("invalid {field} value {value:?}")))
}
fn parse_color(value: Option<&str>) -> Result<Option<Color>, DocxError> {
    let Some(s) = value else { return Ok(None) };
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(DocxError::InvalidValue {
            part: "WordprocessingML".into(),
            detail: format!("invalid RGB color {s:?}"),
        });
    }
    let b = s.as_bytes();
    Ok(Some(Color {
        r: u8::from_str_radix(std::str::from_utf8(&b[0..2]).unwrap(), 16).unwrap(),
        g: u8::from_str_radix(std::str::from_utf8(&b[2..4]).unwrap(), 16).unwrap(),
        b: u8::from_str_radix(std::str::from_utf8(&b[4..6]).unwrap(), 16).unwrap(),
    }))
}
fn parse_highlight(value: &str) -> Option<Color> {
    Some(match value {
        "black" => Color { r: 0, g: 0, b: 0 },
        "blue" => Color { r: 0, g: 0, b: 255 },
        "cyan" => Color {
            r: 0,
            g: 255,
            b: 255,
        },
        "green" => Color { r: 0, g: 255, b: 0 },
        "magenta" => Color {
            r: 255,
            g: 0,
            b: 255,
        },
        "red" => Color { r: 255, g: 0, b: 0 },
        "yellow" => Color {
            r: 255,
            g: 255,
            b: 0,
        },
        "white" => Color {
            r: 255,
            g: 255,
            b: 255,
        },
        "darkBlue" => Color { r: 0, g: 0, b: 128 },
        "darkCyan" => Color {
            r: 0,
            g: 128,
            b: 128,
        },
        "darkGreen" => Color { r: 0, g: 128, b: 0 },
        "darkMagenta" => Color {
            r: 128,
            g: 0,
            b: 128,
        },
        "darkRed" => Color { r: 128, g: 0, b: 0 },
        "darkYellow" => Color {
            r: 128,
            g: 128,
            b: 0,
        },
        "darkGray" => Color {
            r: 128,
            g: 128,
            b: 128,
        },
        "lightGray" => Color {
            r: 192,
            g: 192,
            b: 192,
        },
        _ => return None,
    })
}

type ParsedStyles = (
    StyleCatalog,
    Vec<Style>,
    BTreeSet<String>,
    Option<String>,
    Vec<LatentStyleInfo>,
);

fn parse_styles(root: &XmlNode, limits: &DocxLimits) -> Result<ParsedStyles, DocxError> {
    let mut defaults = DocumentStyleDefaults::default();
    if let Some(dd) = root
        .find(W_NS, "docDefaults")
        .or_else(|| root.find(W_STRICT_NS, "docDefaults"))
    {
        if let Some(r) = dd
            .find(W_NS, "rPrDefault")
            .or_else(|| dd.find(W_STRICT_NS, "rPrDefault"))
            .and_then(|n| n.find(W_NS, "rPr").or_else(|| n.find(W_STRICT_NS, "rPr")))
        {
            defaults.character = parse_style_char_attrs(r)?;
        }
        if let Some(p) = dd
            .find(W_NS, "pPrDefault")
            .or_else(|| dd.find(W_STRICT_NS, "pPrDefault"))
            .and_then(|n| n.find(W_NS, "pPr").or_else(|| n.find(W_STRICT_NS, "pPr")))
        {
            defaults.paragraph = parse_para_props(p)?;
        }
    }
    let catalog = StyleCatalog::new(defaults);
    let mut definitions = Vec::new();
    let mut ids = BTreeSet::new();
    let mut default_para = None;
    let mut styles = Vec::new();
    root.descendants(W_NS, "style", &mut styles);
    if styles.is_empty() {
        root.descendants(W_STRICT_NS, "style", &mut styles);
    }
    if styles.len() > limits.max_styles {
        return Err(DocxError::Limit("style count"));
    }
    for n in styles {
        let id = n
            .w_attr("styleId")
            .ok_or_else(|| DocxError::InvalidValue {
                part: "word/styles.xml".into(),
                detail: "style lacks w:styleId".into(),
            })?
            .to_owned();
        let kind = match n.w_attr("type").unwrap_or("paragraph") {
            "character" => StyleKind::Character,
            "table" => StyleKind::Table,
            _ => StyleKind::Paragraph,
        };
        // Word also has a default numbering style; its unsupported model kind
        // must not be mistaken for a second default paragraph style.
        if n.w_attr("type").unwrap_or("paragraph") == "paragraph"
            && n.w_attr("default")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?
                .unwrap_or(false)
            && default_para.replace(id.clone()).is_some()
        {
            return Err(DocxError::InvalidStructure {
                part: "word/styles.xml".into(),
                detail: "more than one default paragraph style",
            });
        }
        let ppr = n.find(W_NS, "pPr").or_else(|| n.find(W_STRICT_NS, "pPr"));
        let rpr = n.find(W_NS, "rPr").or_else(|| n.find(W_STRICT_NS, "rPr"));
        let based = n
            .find(W_NS, "basedOn")
            .or_else(|| n.find(W_STRICT_NS, "basedOn"))
            .and_then(|x| x.w_attr("val"))
            .map(str::to_owned);
        let linked = n
            .find(W_NS, "link")
            .or_else(|| n.find(W_STRICT_NS, "link"))
            .and_then(|x| x.w_attr("val"))
            .map(str::to_owned);
        ids.insert(id.clone());
        let style = Style {
            id,
            kind,
            based_on: based,
            linked_style: linked,
            character: rpr
                .map(parse_style_char_attrs)
                .transpose()?
                .unwrap_or_default(),
            paragraph: ppr.map(parse_para_props).transpose()?.unwrap_or_default(),
        };
        definitions.push(style);
    }
    let mut latent = Vec::new();
    if let Some(container) = root
        .find(W_NS, "latentStyles")
        .or_else(|| root.find(W_STRICT_NS, "latentStyles"))
    {
        latent.push(LatentStyleInfo {
            name: None,
            locked: container
                .w_attr("defLockedState")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            semi_hidden: container
                .w_attr("defSemiHidden")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            unhide_when_used: container
                .w_attr("defUnhideWhenUsed")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            ui_priority: container
                .w_attr("defUIPriority")
                .and_then(|v| v.parse().ok()),
        });
        for exception in container
            .children
            .iter()
            .filter(|n| is_w(n, "lsdException"))
        {
            latent.push(LatentStyleInfo {
                name: exception.w_attr("name").map(str::to_owned),
                locked: exception
                    .w_attr("locked")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                semi_hidden: exception
                    .w_attr("semiHidden")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                unhide_when_used: exception
                    .w_attr("unhideWhenUsed")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                ui_priority: exception.w_attr("uiPriority").and_then(|v| v.parse().ok()),
            });
        }
    }
    Ok((catalog, definitions, ids, default_para, latent))
}

fn parse_style_char_attrs(node: &XmlNode) -> Result<StyleCharAttrs, DocxError> {
    let mut attrs = StyleCharAttrs {
        values: parse_char_attrs(node, &Theme::default())?,
        ..StyleCharAttrs::default()
    };
    for child in &node.children {
        if is_w(child, "rFonts") {
            attrs.theme_font = child
                .w_attr("asciiTheme")
                .or_else(|| child.w_attr("hAnsiTheme"))
                .and_then(theme_font_slot);
        } else if is_w(child, "color") {
            attrs.theme_color = child
                .w_attr("themeColor")
                .or_else(|| child.w_attr("val"))
                .and_then(theme_color_slot);
        } else if is_w(child, "highlight") {
            attrs.theme_highlight = child.w_attr("themeColor").and_then(theme_color_slot);
        }
    }
    if attrs.theme_font.is_some() {
        attrs.values.font = None;
    }
    if attrs.theme_color.is_some() {
        attrs.values.color = None;
    }
    if attrs.theme_highlight.is_some() {
        attrs.values.highlight = None;
    }
    Ok(attrs)
}
fn parse_theme(root: &XmlNode) -> Result<Theme, DocxError> {
    let mut theme = Theme::default();
    let mut schemes = Vec::new();
    root.descendants(A_NS, "clrScheme", &mut schemes);
    if let Some(scheme) = schemes.first() {
        for child in &scheme.children {
            let slot = match child.name.local.as_str() {
                "dk1" => Some(ThemeColor::Dark1),
                "lt1" => Some(ThemeColor::Light1),
                "dk2" => Some(ThemeColor::Dark2),
                "lt2" => Some(ThemeColor::Light2),
                "accent1" => Some(ThemeColor::Accent1),
                "accent2" => Some(ThemeColor::Accent2),
                "accent3" => Some(ThemeColor::Accent3),
                "accent4" => Some(ThemeColor::Accent4),
                "accent5" => Some(ThemeColor::Accent5),
                "accent6" => Some(ThemeColor::Accent6),
                "hlink" => Some(ThemeColor::Hyperlink),
                "folHlink" => Some(ThemeColor::FollowedHyperlink),
                _ => None,
            };
            if let Some(slot) = slot {
                if let Some(color_node) = child.children.first() {
                    let value = if color_node.name.local == "sysClr" {
                        color_node
                            .no_ns_attr("lastClr")
                            .or_else(|| color_node.no_ns_attr("val"))
                    } else {
                        color_node
                            .no_ns_attr("val")
                            .or_else(|| color_node.no_ns_attr("lastClr"))
                    };
                    if let Some(color) = parse_color(value)? {
                        theme.colors.insert(slot, color);
                    }
                }
            }
        }
    }
    for (name, slot) in [
        ("majorFont", ThemeFont::Major),
        ("minorFont", ThemeFont::Minor),
    ] {
        let mut nodes = Vec::new();
        root.descendants(A_NS, name, &mut nodes);
        if let Some(family) = nodes
            .first()
            .and_then(|n| n.find(A_NS, "latin"))
            .and_then(|n| n.no_ns_attr("typeface"))
        {
            if !family.is_empty() {
                theme.fonts.insert(slot, family.to_owned());
            }
        }
    }
    Ok(theme)
}
fn theme_font_slot(value: &str) -> Option<ThemeFont> {
    if value.starts_with("major") {
        Some(ThemeFont::Major)
    } else if value.starts_with("minor") {
        Some(ThemeFont::Minor)
    } else {
        None
    }
}
fn theme_color_slot(value: &str) -> Option<ThemeColor> {
    Some(match value {
        "dark1" | "tx1" | "text1" => ThemeColor::Dark1,
        "light1" | "bg1" | "background1" => ThemeColor::Light1,
        "dark2" | "tx2" | "text2" => ThemeColor::Dark2,
        "light2" | "bg2" | "background2" => ThemeColor::Light2,
        "accent1" => ThemeColor::Accent1,
        "accent2" => ThemeColor::Accent2,
        "accent3" => ThemeColor::Accent3,
        "accent4" => ThemeColor::Accent4,
        "accent5" => ThemeColor::Accent5,
        "accent6" => ThemeColor::Accent6,
        "hlink" | "hyperlink" => ThemeColor::Hyperlink,
        "folHlink" | "followedHyperlink" => ThemeColor::FollowedHyperlink,
        _ => return None,
    })
}
fn parse_settings(root: &XmlNode) -> Result<(DocxSettings, Vec<LatentStyleInfo>), DocxError> {
    let mut settings = DocxSettings {
        default_tab_stop_twips: Some(720),
        ..DocxSettings::default()
    };
    for child in &root.children {
        if is_w(child, "defaultTabStop") {
            settings.default_tab_stop_twips = Some(
                child
                    .w_attr("val")
                    .ok_or_else(|| DocxError::InvalidValue {
                        part: "word/settings.xml".into(),
                        detail: "defaultTabStop has no w:val".into(),
                    })?
                    .parse()
                    .map_err(|_| DocxError::InvalidValue {
                        part: "word/settings.xml".into(),
                        detail: "invalid defaultTabStop".into(),
                    })?,
            );
        }
        if is_w(child, "compat") {
            for item in &child.children {
                if is_w(item, "compatSetting") {
                    let name = item
                        .w_attr("uri")
                        .zip(item.w_attr("name"))
                        .map(|(u, n)| format!("{u}#{n}"))
                        .or_else(|| item.w_attr("name").map(str::to_owned))
                        .ok_or_else(|| DocxError::InvalidValue {
                            part: "word/settings.xml".into(),
                            detail: "compatSetting has no name".into(),
                        })?;
                    let value = item.w_attr("val").ok_or_else(|| DocxError::InvalidValue {
                        part: "word/settings.xml".into(),
                        detail: "compatSetting has no value".into(),
                    })?;
                    settings.compatibility.insert(name, value.to_owned());
                } else if is_w(item, &item.name.local) {
                    settings
                        .compatibility
                        .insert(item.name.local.clone(), "true".into());
                }
            }
        }
    }
    let mut latent = Vec::new();
    if let Some(container) = root
        .find(W_NS, "latentStyles")
        .or_else(|| root.find(W_STRICT_NS, "latentStyles"))
    {
        settings.latent_style_count = container
            .w_attr("latentStyleCount")
            .and_then(|v| v.parse().ok());
        latent.push(LatentStyleInfo {
            name: None,
            locked: container
                .w_attr("defLockedState")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            semi_hidden: container
                .w_attr("defSemiHidden")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            unhide_when_used: container
                .w_attr("defUnhideWhenUsed")
                .map(|v| parse_on_off(Some(v), false))
                .transpose()?,
            ui_priority: container
                .w_attr("defUIPriority")
                .and_then(|v| v.parse().ok()),
        });
        for item in container
            .children
            .iter()
            .filter(|n| is_w(n, "lsdException"))
        {
            latent.push(LatentStyleInfo {
                name: item.w_attr("name").map(str::to_owned),
                locked: item
                    .w_attr("locked")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                semi_hidden: item
                    .w_attr("semiHidden")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                unhide_when_used: item
                    .w_attr("unhideWhenUsed")
                    .map(|v| parse_on_off(Some(v), false))
                    .transpose()?,
                ui_priority: item.w_attr("uiPriority").and_then(|v| v.parse().ok()),
            });
        }
    }
    Ok((settings, latent))
}
fn parse_embedded_fonts(
    part: &Part,
    root: &XmlNode,
    package: &Package,
    limits: &DocxLimits,
) -> Result<(Vec<Vec<u8>>, Vec<FontRecord>), DocxError> {
    let mut output = Vec::new();
    let mut total = 0usize;
    let mut records = Vec::new();
    for font in root.children.iter().filter(|n| is_w(n, "font")) {
        let name = font
            .w_attr("name")
            .ok_or_else(|| DocxError::InvalidValue {
                part: part.uri.to_string(),
                detail: "font record has no name".into(),
            })?
            .to_owned();
        let mut record = FontRecord {
            name: name.clone(),
            ..FontRecord::default()
        };
        for child in &font.children {
            if is_w(child, "altName") {
                record.alternate_name = child.w_attr("val").map(str::to_owned);
            }
        }
        for embed in &font.children {
            if ![
                "embedRegular",
                "embedBold",
                "embedItalic",
                "embedBoldItalic",
            ]
            .contains(&embed.name.local.as_str())
                || !is_w(embed, &embed.name.local)
            {
                continue;
            }
            if output.len() >= limits.max_fonts {
                return Err(DocxError::Limit("embedded font count"));
            }
            let rid = embed
                .attr(
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                    "id",
                )
                .or_else(|| {
                    embed.attr(
                        "http://purl.oclc.org/ooxml/officeDocument/relationships",
                        "id",
                    )
                })
                .ok_or_else(|| DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: "font relationship id missing".into(),
                })?;
            let rel = package
                .relationships(Some(&part.uri))
                .iter()
                .find(|r| r.id == rid)
                .ok_or_else(|| DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: format!("font relationship {rid:?} missing"),
                })?;
            if rel.rel_type
                != "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font"
                && rel.rel_type != "http://purl.oclc.org/ooxml/officeDocument/relationships/font"
            {
                return Err(DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: format!("relationship {rid:?} is not a font relation"),
                });
            }
            let target = rel
                .resolved_target
                .as_ref()
                .and_then(|u| package.part(u))
                .ok_or_else(|| DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: format!("font target {rid:?} missing"),
                })?;
            if rel.mode != crate::TargetMode::Internal {
                return Err(DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: "embedded font relationship must be internal".into(),
                });
            }
            let key = embed
                .w_attr("fontKey")
                .ok_or_else(|| DocxError::InvalidValue {
                    part: part.uri.to_string(),
                    detail: "fontKey missing".into(),
                })?;
            total = total
                .checked_add(target.bytes.len())
                .ok_or(DocxError::Limit("embedded font bytes"))?;
            if total > limits.max_font_bytes {
                return Err(DocxError::Limit("embedded font bytes"));
            }
            let mut bytes = target.bytes.clone();
            deobfuscate_odttf(&mut bytes, key)?;
            FontFace::parse(&bytes).map_err(|e| DocxError::Font(e.to_string()))?;
            output.push(bytes);
            record.embedded_variants.push(EmbeddedFontRef {
                variant: embed.name.local.clone(),
                relationship_id: rid.to_owned(),
                part: target.uri.to_string(),
                font_key: key.to_owned(),
                byte_index: output.len() - 1,
            });
        }
        records.push(record);
    }
    Ok((output, records))
}
fn deobfuscate_odttf(bytes: &mut [u8], key: &str) -> Result<(), DocxError> {
    let hex: String = key
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '-'))
        .collect();
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(DocxError::InvalidValue {
            part: "word/fontTable.xml".into(),
            detail: "fontKey must be a GUID".into(),
        });
    }
    let mut key_bytes = [0u8; 16];
    for (i, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        key_bytes[i] =
            u8::from_str_radix(std::str::from_utf8(pair).unwrap_or(""), 16).map_err(|_| {
                DocxError::InvalidValue {
                    part: "word/fontTable.xml".into(),
                    detail: "invalid fontKey".into(),
                }
            })?;
    }
    key_bytes.reverse();
    for (i, b) in bytes.iter_mut().take(32).enumerate() {
        *b ^= key_bytes[i % 16];
    }
    Ok(())
}
fn model_error(e: impl fmt::Display) -> DocxError {
    DocxError::Model(e.to_string())
}
