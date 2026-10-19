// SPDX-License-Identifier: AGPL-3.0-or-later
//! Preservation of what the DOCX import does not interpret (US-306).
//!
//! One rule drives everything: [`classify`] says, for a child element and the
//! [`Level`] of its parent, whether the import interprets it (`Known`), looks
//! through it (`Transparent`, a content wrapper such as `w:hyperlink` or
//! `w:sdt`) or ignores it (`Capture`). Captured elements are copied as US-032
//! opaque subtrees while the XML is parsed, so nothing depends on the import
//! having understood them. The import then records, per node, the *skeleton*
//! of the source: element names, all attributes, the order of children,
//! the text ranges of text leaves, and references to the opaque copies.
//!
//! The model slots used are `Node::Opaque` (unknown blocks) and
//! `Paragraph::anchored` (unknown inline elements). Everything else lives in
//! [`Preserved`], which the import returns in `ImportedDocx::preserved`.

use super::{DocxError, DocxLimits, ImportNotice, W_NS, W_STRICT_NS, XmlNode, is_w, tables};
use atelier_text_model::rich::NodeId;
use atelier_xml::{Name, OpaqueSubtree, Writer};
use std::collections::{BTreeMap, BTreeSet};

/// Markup Compatibility and Extensibility namespace (ECMA-376 part 3).
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// Namespaces the import reads or deliberately passes through; any other
/// namespace listed in `mc:MustUnderstand` produces an import notice.
const UNDERSTOOD: &[&str] = &[W_NS, W_STRICT_NS, MC_NS, R_NS, XML_NS];

/// An attribute of a preserved element, exactly as written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreservedAttr {
    /// Qualified name with the source prefix (`w14:paraId`).
    pub name: String,
    /// Namespace URI, empty for unprefixed attributes.
    pub namespace: String,
    pub value: String,
}

/// A child in the skeleton of a preserved element, in source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreservedChild {
    Element(PreservedElement),
    /// An unknown element kept as XML (UTF-8, with its namespace
    /// declarations) where the model has no slot for it.
    Opaque(Vec<u8>),
    /// A block of the model: a paragraph, a table or a `Node::Opaque`.
    Block(NodeId),
    /// An `AnchoredObject` of the owning paragraph holding the XML.
    Anchored(NodeId),
}

/// An element of the source whose structure the import keeps.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreservedElement {
    /// Qualified name with the source prefix.
    pub name: String,
    pub namespace: String,
    /// Every attribute of the source element (interpreted or not).
    pub attrs: Vec<PreservedAttr>,
    /// The model node built from this element (paragraph, table, row, cell).
    pub node: Option<NodeId>,
    /// Text byte range: the text a `w:t`, `w:tab`, `w:br`, `w:sym`...
    /// produced, or the text of a whole run. Offsets are those of the source
    /// text of the paragraph, where each break counts one `\n` and each
    /// symbol its character; [`crate::source_text`] gives that text from the
    /// paragraph of the model, which keeps them as inline items.
    pub range: Option<(usize, usize)>,
    /// The content of this element is an interpreted copy of the chosen
    /// `mc:AlternateContent` branch; it is not to be written out, the
    /// `mc:AlternateContent` opaque copy preceding it carries the source.
    pub shadow: bool,
    /// The element is a property container (`w:tblPr`, `w:trPr`, `w:tcPr`,
    /// `w:tblPrEx`, `w:tblGrid`) whose content sits in the `opaque` slot
    /// named like it on the table, row or cell node.
    pub slot: bool,
    pub children: Vec<PreservedChild>,
}

/// The skeleton of an interpreted part, with its namespace declarations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreservedPart {
    pub uri: String,
    /// Every `xmlns` declaration of the root, in source order
    /// (`("w14", uri)`; the default namespace has the empty prefix).
    pub declarations: Vec<(String, String)>,
    /// Namespaces listed by `mc:Ignorable` (anywhere in the part).
    pub ignorable: Vec<String>,
    /// The root element; its `w:body` (or style, ...) children included.
    pub root: PreservedElement,
}

/// Everything the import keeps besides the model itself (US-306).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Preserved {
    pub document: Option<PreservedPart>,
    pub styles: Option<PreservedPart>,
    pub settings: Option<PreservedPart>,
    /// The `w:p` skeleton of each paragraph, by paragraph id.
    pub paragraphs: BTreeMap<NodeId, PreservedElement>,
    /// The `w:tbl`, `w:tr` and `w:tc` skeletons, by table, row and cell id.
    pub tables: BTreeMap<NodeId, PreservedElement>,
    /// Total bytes of opaque copies taken while parsing.
    pub opaque_bytes: usize,
    /// Number of opaque copies taken while parsing.
    pub opaque_items: usize,
}

// ---------------------------------------------------------------------------
// Levels and classification
// ---------------------------------------------------------------------------

/// The kind of parent whose children are being classified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Level {
    DocRoot,
    Body,
    Cell,
    Table,
    Row,
    Para,
    PPr,
    RPr,
    Run,
    StylesRoot,
    DocDefaults,
    RPrDefault,
    PPrDefault,
    Style,
    SettingsRoot,
    SdtShell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Class {
    Known,
    Transparent,
    Capture,
}

const BLOCK_CONTAINERS: &[&str] = &["sdt", "customXml", "ins", "moveTo"];
const TABLE_CONTAINERS: &[&str] = &["sdt", "customXml", "ins", "moveTo", "smartTag"];
const INLINE_CONTAINERS: &[&str] = &[
    "hyperlink",
    "ins",
    "moveTo",
    "smartTag",
    "customXml",
    "sdt",
    "fldSimple",
    "dir",
    "bdo",
];
const RPR_KNOWN: &[&str] = &[
    "rStyle",
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
];

/// The WordprocessingML elements the import interprets as children of a
/// parent of this kind. This is the declaration the tests hold the import
/// code to; [`classify`] is built on it.
pub(super) fn interpreted(level: Level) -> &'static [&'static str] {
    match level {
        Level::DocRoot => &["body"],
        Level::Body => &["p", "tbl"],
        Level::Cell => &["tcPr", "p", "tbl"],
        Level::Table => &["tblPr", "tblGrid", "tr"],
        Level::Row => &["tblPrEx", "trPr", "tc"],
        Level::Para => &["pPr", "r"],
        Level::PPr => &["pStyle", "jc", "ind", "spacing", "pBdr", "shd"],
        Level::RPr => RPR_KNOWN,
        Level::Run => &[
            "rPr",
            "t",
            "tab",
            "br",
            "cr",
            "sym",
            "noBreakHyphen",
            "softHyphen",
        ],
        Level::StylesRoot => &["docDefaults", "style"],
        Level::DocDefaults => &["rPrDefault", "pPrDefault"],
        Level::RPrDefault => &["rPr"],
        Level::PPrDefault => &["pPr"],
        Level::Style => &["basedOn", "link", "pPr", "rPr"],
        Level::SettingsRoot => &["defaultTabStop"],
        Level::SdtShell => &[],
    }
}

/// Every `(level, element)` pair the import interprets, plus the content
/// wrappers it looks through (`Container`). Tests hold the import code to it.
pub fn interpreted_elements() -> Vec<(String, &'static str)> {
    const LEVELS: &[Level] = &[
        Level::DocRoot,
        Level::Body,
        Level::Cell,
        Level::Table,
        Level::Row,
        Level::Para,
        Level::PPr,
        Level::RPr,
        Level::Run,
        Level::StylesRoot,
        Level::DocDefaults,
        Level::RPrDefault,
        Level::PPrDefault,
        Level::Style,
        Level::SettingsRoot,
        Level::SdtShell,
    ];
    let mut out = Vec::new();
    for level in LEVELS {
        for name in interpreted(*level) {
            out.push((format!("{level:?}"), *name));
        }
        for name in containers(*level) {
            out.push((format!("{level:?}/container"), *name));
        }
    }
    out.push((format!("{:?}/container", Level::SdtShell), "sdtContent"));
    out
}

/// Content wrappers the import looks through at this level.
pub(super) fn containers(level: Level) -> &'static [&'static str] {
    match level {
        Level::Body | Level::Cell => BLOCK_CONTAINERS,
        Level::Table | Level::Row => TABLE_CONTAINERS,
        Level::Para => INLINE_CONTAINERS,
        _ => &[],
    }
}

fn w_local(name: &Name) -> Option<&str> {
    match name.namespace.as_deref() {
        Some(W_NS | W_STRICT_NS) => Some(name.local.as_str()),
        _ => None,
    }
}

pub(super) fn classify(level: Level, name: &Name) -> Class {
    let Some(local) = w_local(name) else {
        return Class::Capture;
    };
    if interpreted(level).contains(&local) {
        Class::Known
    } else if containers(level).contains(&local)
        || (level == Level::SdtShell && local == "sdtContent")
    {
        Class::Transparent
    } else {
        Class::Capture
    }
}

/// The level of the children of the innermost open element of a parse stack,
/// or `None` where nothing is captured (foreign parents, other parts).
pub(super) fn level_of_children(stack: &[XmlNode]) -> Option<Level> {
    let mut skip_sdt = false;
    for (index, node) in stack.iter().enumerate().rev() {
        let local = w_local(&node.name)?;
        match local {
            "sdtContent" => skip_sdt = true,
            "sdt" => {
                if skip_sdt {
                    skip_sdt = false;
                } else {
                    return Some(Level::SdtShell);
                }
            }
            "customXml" | "ins" | "moveTo" | "smartTag" | "hyperlink" | "fldSimple" | "dir"
            | "bdo" => {}
            "document" if index == 0 => return Some(Level::DocRoot),
            "styles" if index == 0 => return Some(Level::StylesRoot),
            "settings" if index == 0 => return Some(Level::SettingsRoot),
            "body" => return Some(Level::Body),
            "tc" => return Some(Level::Cell),
            "tbl" => return Some(Level::Table),
            "tr" => return Some(Level::Row),
            "p" => return Some(Level::Para),
            "pPr" => return Some(Level::PPr),
            "rPr" => return Some(Level::RPr),
            "r" => return Some(Level::Run),
            "docDefaults" => return Some(Level::DocDefaults),
            "rPrDefault" => return Some(Level::RPrDefault),
            "pPrDefault" => return Some(Level::PPrDefault),
            "style" => return Some(Level::Style),
            _ => return None,
        }
    }
    None
}

fn sub_level(level: Level, local: &str) -> Option<Level> {
    Some(match (level, local) {
        (Level::DocRoot, "body") => Level::Body,
        (Level::StylesRoot, "docDefaults") => Level::DocDefaults,
        (Level::StylesRoot, "style") => Level::Style,
        (Level::DocDefaults, "rPrDefault") => Level::RPrDefault,
        (Level::DocDefaults, "pPrDefault") => Level::PPrDefault,
        (Level::RPrDefault, "rPr") | (Level::Style, "rPr") => Level::RPr,
        (Level::PPrDefault, "pPr") | (Level::Style, "pPr") => Level::PPr,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Events over the children of a container
// ---------------------------------------------------------------------------

/// One step over the children of a container, wrappers expanded in place.
pub(super) enum Ev<'a> {
    /// An element the import interprets.
    Known(&'a XmlNode),
    /// A content wrapper opens; the flag marks the interpreted copy of a
    /// chosen `mc:AlternateContent` branch.
    Open(&'a XmlNode, bool),
    Close,
    /// An element that is captured (`node.opaque` holds its XML).
    Opaque(&'a XmlNode),
    /// A captured property of the enclosing wrapper (`w:sdtPr`,
    /// `w:smartTagPr`...): part of the wrapper, not content.
    Prop(&'a XmlNode),
}

fn is_wrapper_property(node: &XmlNode) -> bool {
    matches!(
        w_local(&node.name),
        Some("sdtPr" | "sdtEndPr" | "smartTagPr" | "customXmlPr" | "fldData")
    )
}

pub(super) fn events<'a>(
    node: &'a XmlNode,
    level: Level,
    decls: &[(String, String)],
) -> Vec<Ev<'a>> {
    let mut out = Vec::new();
    walk(node, level, decls, false, &mut out);
    out
}

fn is_mc(node: &XmlNode, local: &str) -> bool {
    node.name.local == local && node.name.namespace.as_deref() == Some(MC_NS)
}

fn walk<'a>(
    node: &'a XmlNode,
    level: Level,
    decls: &[(String, String)],
    shadow: bool,
    out: &mut Vec<Ev<'a>>,
) {
    for child in &node.children {
        match classify(level, &child.name) {
            Class::Known => out.push(Ev::Known(child)),
            Class::Transparent => {
                out.push(Ev::Open(child, false));
                if is_w(child, "sdt") {
                    for part in &child.children {
                        if classify(Level::SdtShell, &part.name) == Class::Transparent {
                            out.push(Ev::Open(part, false));
                            walk(part, level, decls, shadow, out);
                            out.push(Ev::Close);
                        } else if !shadow {
                            out.push(Ev::Prop(part));
                        }
                    }
                } else {
                    walk(child, level, decls, shadow, out);
                }
                out.push(Ev::Close);
            }
            Class::Capture if shadow => {}
            Class::Capture
                if is_wrapper_property(child)
                    && !matches!(node.name.local.as_str(), "body" | "tc" | "p" | "tbl" | "tr") =>
            {
                out.push(Ev::Prop(child));
            }
            Class::Capture => {
                out.push(Ev::Opaque(child));
                if is_mc(child, "AlternateContent")
                    && matches!(level, Level::Body | Level::Cell | Level::Para)
                {
                    if let Some(branch) = choose_branch(child, decls) {
                        out.push(Ev::Open(branch, true));
                        walk(branch, level, decls, true, out);
                        out.push(Ev::Close);
                    }
                }
            }
        }
    }
}

fn namespace_of<'a>(decls: &'a [(String, String)], prefix: &str) -> Option<&'a str> {
    decls
        .iter()
        .rev()
        .find(|(p, _)| p == prefix)
        .map(|(_, u)| u.as_str())
}

/// The branch of an `mc:AlternateContent` this import takes: the first
/// `mc:Choice` whose `Requires` prefixes all name understood namespaces,
/// else `mc:Fallback`.
pub(super) fn choose_branch<'a>(
    alternate: &'a XmlNode,
    decls: &[(String, String)],
) -> Option<&'a XmlNode> {
    for choice in alternate.children.iter().filter(|c| is_mc(c, "Choice")) {
        let ok = choice.no_ns_attr("Requires").is_some_and(|requires| {
            requires
                .split_ascii_whitespace()
                .all(|p| namespace_of(decls, p).is_some_and(|ns| UNDERSTOOD.contains(&ns)))
        });
        if ok {
            return Some(choice);
        }
    }
    alternate.children.iter().find(|c| is_mc(c, "Fallback"))
}

// ---------------------------------------------------------------------------
// Opaque copies
// ---------------------------------------------------------------------------

const XML_DECLARATION: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

/// The XML of a captured subtree, without XML declaration.
pub(super) fn subtree_bytes(subtree: &OpaqueSubtree) -> Result<Vec<u8>, DocxError> {
    let to_error = |e: atelier_xml::XmlError| DocxError::Xml {
        part: "opaque capture".into(),
        message: e.to_string(),
    };
    let mut writer = Writer::new(Vec::new()).map_err(to_error)?;
    writer.write_opaque(subtree).map_err(to_error)?;
    let mut bytes = writer.finish().map_err(to_error)?;
    if bytes.starts_with(XML_DECLARATION) {
        bytes.drain(..XML_DECLARATION.len());
    }
    Ok(bytes)
}

/// Totals of the opaque copies taken while parsing one part.
pub(super) struct CaptureBudget {
    max_bytes: usize,
    max_items: usize,
    pub bytes: usize,
    pub items: usize,
    /// Root declarations of the part being parsed (see [`strip_context`]).
    pub context: Vec<(String, String)>,
}
impl CaptureBudget {
    pub fn new(limits: &DocxLimits, spent: &Preserved) -> Self {
        Self {
            max_bytes: limits.max_opaque_bytes,
            max_items: limits.max_opaque_items,
            bytes: spent.opaque_bytes,
            items: spent.opaque_items,
            context: Vec::new(),
        }
    }
    pub fn charge(&mut self, bytes: usize) -> Result<(), DocxError> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.items = self.items.saturating_add(1);
        if self.bytes > self.max_bytes {
            return Err(DocxError::Limit("opaque capture bytes"));
        }
        if self.items > self.max_items {
            return Err(DocxError::Limit("opaque capture count"));
        }
        Ok(())
    }
}

/// The XML of an element: its capture when it has one, else a serialization
/// of the parsed tree (elements the import reads but the model cannot hold).
pub(super) fn xml_of(node: &XmlNode) -> Vec<u8> {
    node.opaque
        .clone()
        .unwrap_or_else(|| tables::dom_bytes(node))
}

// ---------------------------------------------------------------------------
// Skeleton construction
// ---------------------------------------------------------------------------

/// Skeleton under construction plus the node budget.
pub(super) struct Pres {
    nodes: usize,
    max_nodes: usize,
    /// Declarations of the main document root, for `mc:AlternateContent`.
    pub decls: Vec<(String, String)>,
    pub out: Preserved,
}

impl Pres {
    pub fn new(limits: &DocxLimits) -> Self {
        Self {
            nodes: 0,
            max_nodes: limits.max_preserved_nodes,
            decls: Vec::new(),
            out: Preserved::default(),
        }
    }

    pub fn count(&mut self) -> Result<(), DocxError> {
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            return Err(DocxError::Limit("preserved nodes"));
        }
        Ok(())
    }

    /// The element itself (name and attributes), without children.
    pub fn shell(&mut self, node: &XmlNode) -> Result<PreservedElement, DocxError> {
        self.count()?;
        Ok(PreservedElement {
            name: node.name.qualified(),
            namespace: node.name.namespace.clone().unwrap_or_default(),
            attrs: node
                .attrs
                .iter()
                .map(|a| PreservedAttr {
                    name: a.name.qualified(),
                    namespace: a.name.namespace.clone().unwrap_or_default(),
                    value: a.value.clone(),
                })
                .collect(),
            ..PreservedElement::default()
        })
    }

    /// Skeleton of a property-like element whose children are classified at
    /// `level` (`w:pPr`, `w:rPr`, style, defaults, settings...). Interpreted
    /// children keep their attributes and position; the others keep their XML.
    pub fn prop_tree(
        &mut self,
        node: &XmlNode,
        level: Level,
    ) -> Result<PreservedElement, DocxError> {
        let mut element = self.shell(node)?;
        for child in &node.children {
            if classify(level, &child.name) == Class::Known {
                let local = child.name.local.as_str();
                if let Some(next) = sub_level(level, local) {
                    element
                        .children
                        .push(PreservedChild::Element(self.prop_tree(child, next)?));
                } else if child.children.is_empty() {
                    let leaf = self.shell(child)?;
                    element.children.push(PreservedChild::Element(leaf));
                } else {
                    // A property with element children: keep it whole.
                    element.children.push(PreservedChild::Opaque(xml_of(child)));
                }
            } else {
                element.children.push(PreservedChild::Opaque(xml_of(child)));
            }
        }
        Ok(element)
    }

    /// Records a part's skeleton with its declarations and `mc:Ignorable`.
    pub fn part(
        &mut self,
        uri: &str,
        bytes: &[u8],
        root_xml: &XmlNode,
        root: PreservedElement,
    ) -> PreservedPart {
        let declarations = root_declarations(bytes);
        let mut ignorable = BTreeSet::new();
        collect_ignorable(root_xml, &declarations, &mut ignorable);
        PreservedPart {
            uri: uri.to_owned(),
            declarations,
            ignorable: ignorable.into_iter().collect(),
            root,
        }
    }
}

/// Builds an element tree from events while the import walks a container.
pub(super) struct TreeBuilder {
    stack: Vec<PreservedElement>,
}
impl TreeBuilder {
    pub fn new(root: PreservedElement) -> Self {
        Self { stack: vec![root] }
    }
    pub fn open(&mut self, element: PreservedElement) {
        self.stack.push(element);
    }
    pub fn close(&mut self) {
        if self.stack.len() > 1 {
            if let Some(done) = self.stack.pop() {
                if let Some(parent) = self.stack.last_mut() {
                    parent.children.push(PreservedChild::Element(done));
                }
            }
        }
    }
    pub fn push(&mut self, child: PreservedChild) {
        if let Some(top) = self.stack.last_mut() {
            top.children.push(child);
        }
    }
    pub fn finish(mut self) -> PreservedElement {
        while self.stack.len() > 1 {
            self.close();
        }
        self.stack.pop().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Namespace declarations and Markup Compatibility
// ---------------------------------------------------------------------------

fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The `xmlns` declarations of the root start tag, in source order. The XML
/// reader resolves namespaces and hides declarations, so the root tag is
/// scanned directly; this is bounded to the first megabyte.
pub(super) fn root_declarations(bytes: &[u8]) -> Vec<(String, String)> {
    let head = &bytes[..bytes.len().min(1 << 20)];
    let text = if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) {
        let big = head[0] == 0xfe;
        let units: Vec<u16> = head[2..]
            .chunks_exact(2)
            .map(|c| {
                if big {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(head).into_owned()
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    // Skip the prolog up to the first element.
    loop {
        while i < chars.len() && chars[i] != '<' {
            i += 1;
        }
        if i + 1 >= chars.len() {
            return Vec::new();
        }
        match chars[i + 1] {
            '?' => {
                while i + 1 < chars.len() && !(chars[i] == '?' && chars[i + 1] == '>') {
                    i += 1;
                }
                i += 2;
            }
            '!' => {
                if chars[i..].starts_with(&['<', '!', '-', '-']) {
                    while i + 2 < chars.len()
                        && !(chars[i] == '-' && chars[i + 1] == '-' && chars[i + 2] == '>')
                    {
                        i += 1;
                    }
                    i += 3;
                } else {
                    while i < chars.len() && chars[i] != '>' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => break,
        }
    }
    i += 1;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
        i += 1;
    }
    let mut out = Vec::new();
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] == '>' || chars[i] == '/' {
            break;
        }
        let start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect();
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '=') {
            i += 1;
        }
        let Some(&quote) = chars.get(i).filter(|c| **c == '"' || **c == '\'') else {
            break;
        };
        i += 1;
        let value_start = i;
        while i < chars.len() && chars[i] != quote {
            i += 1;
        }
        let value: String = chars[value_start..i.min(chars.len())].iter().collect();
        i += 1;
        if key == "xmlns" {
            out.push((String::new(), unescape(&value)));
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            out.push((prefix.to_owned(), unescape(&value)));
        }
    }
    out
}

fn collect_ignorable(node: &XmlNode, decls: &[(String, String)], out: &mut BTreeSet<String>) {
    if let Some(list) = node.attr(MC_NS, "Ignorable") {
        for prefix in list.split_ascii_whitespace() {
            if let Some(ns) = namespace_of(decls, prefix) {
                out.insert(ns.to_owned());
            }
        }
    }
    for child in &node.children {
        collect_ignorable(child, decls, out);
    }
}

/// Notices for markup the import cannot honour: `mc:MustUnderstand` naming a
/// namespace this import does not know, and top-level captured elements of a
/// foreign namespace that no `mc:Ignorable` lists.
pub(super) fn markup_notices(
    root: &XmlNode,
    part: &PreservedPart,
    notices: &mut Vec<ImportNotice>,
) {
    fn walk(
        node: &XmlNode,
        part: &PreservedPart,
        seen: &mut BTreeSet<(String, String)>,
        out: &mut Vec<ImportNotice>,
    ) {
        if let Some(list) = node.attr(MC_NS, "MustUnderstand") {
            for prefix in list.split_ascii_whitespace() {
                let ns = namespace_of(&part.declarations, prefix).unwrap_or("");
                if !UNDERSTOOD.contains(&ns)
                    && seen.insert((ns.to_owned(), "MustUnderstand".into()))
                {
                    out.push(ImportNotice::UnsupportedProperty {
                        part: part.uri.clone(),
                        namespace: ns.to_owned(),
                        local_name: "MustUnderstand".into(),
                        value: prefix.to_owned(),
                    });
                }
            }
        }
        let ns = node.name.namespace.as_deref().unwrap_or("");
        if node.opaque.is_some()
            && !UNDERSTOOD.contains(&ns)
            && !part.ignorable.iter().any(|i| i == ns)
            && seen.insert((ns.to_owned(), node.name.local.clone()))
        {
            out.push(ImportNotice::UnsupportedElement {
                part: part.uri.clone(),
                namespace: ns.to_owned(),
                local_name: node.name.local.clone(),
            });
        }
        if node.opaque.is_none() {
            for child in &node.children {
                walk(child, part, seen, out);
            }
        }
    }
    let mut seen = BTreeSet::new();
    walk(root, part, &mut seen, notices);
}

// ---------------------------------------------------------------------------
// Namespace context shared by the copies of a part
// ---------------------------------------------------------------------------

/// The attributes of the first start tag of `xml` that are namespace
/// declarations: `(start, end, prefix, uri)` where `start..end` covers the
/// leading whitespace and the declaration. Also returns the end of the name.
fn root_tag_declarations(xml: &[u8]) -> (usize, Vec<(usize, usize, String, String)>) {
    let mut found = Vec::new();
    if xml.first() != Some(&b'<') {
        return (0, found);
    }
    let mut i = 1;
    while i < xml.len() && !xml[i].is_ascii_whitespace() && xml[i] != b'>' && xml[i] != b'/' {
        i += 1;
    }
    let name_end = i;
    loop {
        let start = i;
        while i < xml.len() && xml[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= xml.len() || xml[i] == b'>' || xml[i] == b'/' {
            break;
        }
        let key_start = i;
        while i < xml.len() && xml[i] != b'=' && !xml[i].is_ascii_whitespace() {
            i += 1;
        }
        let key = &xml[key_start..i];
        while i < xml.len() && (xml[i] == b'=' || xml[i].is_ascii_whitespace()) {
            i += 1;
        }
        let Some(&quote) = xml.get(i).filter(|c| **c == b'"' || **c == b'\'') else {
            break;
        };
        i += 1;
        let value_start = i;
        while i < xml.len() && xml[i] != quote {
            i += 1;
        }
        let value = String::from_utf8_lossy(&xml[value_start..i.min(xml.len())]).into_owned();
        i += 1;
        if key == b"xmlns" {
            found.push((start, i, String::new(), unescape(&value)));
        } else if let Some(prefix) = key.strip_prefix(b"xmlns:") {
            found.push((
                start,
                i,
                String::from_utf8_lossy(prefix).into_owned(),
                unescape(&value),
            ));
        }
    }
    (name_end, found)
}

/// Removes from the root start tag of a captured copy the namespace
/// declarations the part root already makes with the same value (`context`,
/// see [`root_declarations`]), so that a copy only carries what it introduces
/// or changes. [`PreservedPart::standalone`] puts them back.
pub(super) fn strip_context(xml: Vec<u8>, context: &[(String, String)]) -> Vec<u8> {
    let (_, declared) = root_tag_declarations(&xml);
    let has_default = context.iter().any(|(p, _)| p.is_empty());
    let redundant = |prefix: &str, uri: &str| {
        if prefix.is_empty() && uri.is_empty() {
            !has_default
        } else {
            namespace_of(context, prefix) == Some(uri)
        }
    };
    if !declared.iter().any(|(_, _, p, u)| redundant(p, u)) {
        return xml;
    }
    let mut out = Vec::with_capacity(xml.len());
    let mut at = 0;
    for (start, end, prefix, uri) in &declared {
        if redundant(prefix, uri) {
            out.extend_from_slice(&xml[at..*start]);
            at = *end;
        }
    }
    out.extend_from_slice(&xml[at..]);
    out
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Input for re-parsing a stripped copy: the copy under a wrapper element that
/// declares the context prefixes the copy mentions, so that the parse does not
/// pay for the whole declaration table on each copy.
pub(super) fn reparse_input(copy: &[u8], context: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(copy.len() + 64);
    out.extend_from_slice(b"<c");
    let mut seen: Vec<&str> = Vec::new();
    for (prefix, _) in context {
        if seen.contains(&prefix.as_str()) {
            continue;
        }
        seen.push(prefix);
        let uri = namespace_of(context, prefix).unwrap_or("");
        let needed = if prefix.is_empty() {
            true
        } else {
            let pattern = format!("{prefix}:");
            copy.windows(pattern.len()).any(|w| w == pattern.as_bytes())
        };
        if needed {
            if prefix.is_empty() {
                out.extend_from_slice(b" xmlns=\"");
            } else {
                out.extend_from_slice(format!(" xmlns:{prefix}=\"").as_bytes());
            }
            out.extend_from_slice(escape_attr(uri).as_bytes());
            out.push(b'"');
        }
    }
    out.push(b'>');
    out.extend_from_slice(copy);
    out.extend_from_slice(b"</c>");
    out
}

impl PreservedPart {
    /// A copy of this part (`Opaque` bytes, anchored object or opaque block
    /// data) made self-contained: the namespace declarations of the part root
    /// that the copy does not declare itself are put back on its root element.
    /// Copies are stored without them, since every copy of a part shares the
    /// same root declarations ([`PreservedPart::declarations`]).
    pub fn standalone(&self, fragment: &[u8]) -> Vec<u8> {
        let (name_end, declared) = root_tag_declarations(fragment);
        if name_end == 0 {
            return fragment.to_vec();
        }
        let mut add = String::new();
        for (prefix, uri) in &self.declarations {
            if declared.iter().any(|(_, _, p, _)| p == prefix) {
                continue;
            }
            // The last declaration of a prefix wins.
            if self.declarations.iter().rev().find(|(p, _)| p == prefix)
                != Some(&(prefix.clone(), uri.clone()))
            {
                continue;
            }
            if prefix.is_empty() {
                add.push_str(" xmlns=\"");
            } else {
                add.push_str(" xmlns:");
                add.push_str(prefix);
                add.push_str("=\"");
            }
            add.push_str(&escape_attr(uri));
            add.push('"');
        }
        let mut out = Vec::with_capacity(fragment.len() + add.len());
        out.extend_from_slice(&fragment[..name_end]);
        out.extend_from_slice(add.as_bytes());
        out.extend_from_slice(&fragment[name_end..]);
        out
    }
}
// ---------------------------------------------------------------------------
// Schema order (for the writer)
// ---------------------------------------------------------------------------

/// Position of a child element in the ECMA-376 sequence of `w:rPr`
/// (CT_RPr, §17.3.2); unknown names sort last. The writer places elements it
/// adds or changes with this rank; elements read from the source keep their
/// recorded relative order.
pub fn rpr_rank(local: &str) -> usize {
    const ORDER: &[&str] = &[
        "rStyle",
        "rFonts",
        "b",
        "bCs",
        "i",
        "iCs",
        "caps",
        "smallCaps",
        "strike",
        "dstrike",
        "outline",
        "shadow",
        "emboss",
        "imprint",
        "noProof",
        "snapToGrid",
        "vanish",
        "webHidden",
        "color",
        "spacing",
        "w",
        "kern",
        "position",
        "sz",
        "szCs",
        "highlight",
        "u",
        "effect",
        "bdr",
        "shd",
        "fitText",
        "vertAlign",
        "rtl",
        "cs",
        "em",
        "lang",
        "eastAsianLayout",
        "specVanish",
        "oMath",
        "rPrChange",
    ];
    ORDER
        .iter()
        .position(|n| *n == local)
        .unwrap_or(ORDER.len())
}

/// Position of a child element in the `w:rPr` of a paragraph mark
/// (`w:pPr/w:rPr`, CT_ParaRPr, §17.3.1.29): the revision marks `w:ins`,
/// `w:del`, `w:moveFrom` and `w:moveTo` come first, then the properties of
/// CT_RPr in their order ([`rpr_rank`]), then `w:rPrChange`. In a run's
/// `w:rPr` (CT_RPr) `w:ins`/`w:del` are not part of the sequence, hence the
/// separate function: use this one for the mark of a paragraph.
pub fn para_mark_rpr_rank(local: &str) -> usize {
    const REVISIONS: &[&str] = &["ins", "del", "moveFrom", "moveTo"];
    REVISIONS
        .iter()
        .position(|n| *n == local)
        .unwrap_or(REVISIONS.len() + rpr_rank(local))
}

/// Position of a child element in the sequence of `w:pPr` (CT_PPr, §17.3.1).
pub fn ppr_rank(local: &str) -> usize {
    const ORDER: &[&str] = &[
        "pStyle",
        "keepNext",
        "keepLines",
        "pageBreakBefore",
        "framePr",
        "widowControl",
        "numPr",
        "suppressLineNumbers",
        "pBdr",
        "shd",
        "tabs",
        "suppressAutoHyphens",
        "kinsoku",
        "wordWrap",
        "overflowPunct",
        "topLinePunct",
        "autoSpaceDE",
        "autoSpaceDN",
        "bidi",
        "adjustRightInd",
        "snapToGrid",
        "spacing",
        "ind",
        "contextualSpacing",
        "mirrorIndents",
        "suppressOverlap",
        "jc",
        "textDirection",
        "textAlignment",
        "textboxTightWrap",
        "outlineLvl",
        "divId",
        "cnfStyle",
        "rPr",
        "sectPr",
        "pPrChange",
    ];
    ORDER
        .iter()
        .position(|n| *n == local)
        .unwrap_or(ORDER.len())
}

/// Position of a child element in the sequence of `w:sectPr` (§17.6.17).
pub fn sect_pr_rank(local: &str) -> usize {
    const ORDER: &[&str] = &[
        "headerReference",
        "footerReference",
        "footnotePr",
        "endnotePr",
        "type",
        "pgSz",
        "pgMar",
        "paperSrc",
        "pgBorders",
        "lnNumType",
        "pgNumType",
        "cols",
        "formProt",
        "vAlign",
        "noEndnote",
        "titlePg",
        "textDirection",
        "bidi",
        "rtlGutter",
        "docGrid",
        "printerSettings",
        "sectPrChange",
    ];
    ORDER
        .iter()
        .position(|n| *n == local)
        .unwrap_or(ORDER.len())
}
