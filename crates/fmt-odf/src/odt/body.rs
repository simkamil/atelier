// SPDX-License-Identifier: AGPL-3.0-or-later
//! Conversion of `office:text` (and master-page headers/footers) into the rich
//! document: paragraphs, lists, tables, frames, fields, links and bookmarks.

use super::styles::*;
use super::tree::*;
use super::*;
use atelier_model::Transaction;
use atelier_text_model::rich::{
    AnchoredObject, Bookmark, Cell, CharAttrs, CharRun, DocumentStyleDefaults, Field, HeaderFooter,
    HfVariant, Link, LinkTarget, ListRef, MAX_LIST_LEVEL, Node, NodeId, ParaProps, Paragraph,
    RichDocument, RichOp, Row, Section, Style, StyleCatalog, StyleCharAttrs, StyleEngine,
    StyleKind, Subtree, Table, Theme,
};
use atelier_unicode::segmentation::grapheme_boundaries;
use atelier_zip::Archive;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

fn model(e: impl std::fmt::Display) -> OdtError {
    OdtError::Model(e.to_string())
}

/// Built-in LibreOffice style names with a clear model equivalent.
fn builtin_id(family: Family, name: &str) -> Option<String> {
    let id = match (family, name) {
        (Family::Paragraph, "Standard") => "Normal",
        (Family::Paragraph, "Title") => "Title",
        (Family::Paragraph, "Subtitle") => "Subtitle",
        (Family::Paragraph, "Text_20_body") => "BodyText",
        (Family::Paragraph, "Header") => "Header",
        (Family::Paragraph, "Footer") => "Footer",
        (Family::Paragraph, "Caption") => "Caption",
        (Family::Paragraph, "Quotations") => "Quote",
        (Family::Paragraph, "List") => "List",
        (Family::Text, "Strong_20_Emphasis") => "Strong",
        (Family::Text, "Emphasis") => "Emphasis",
        (Family::Text, "Internet_20_link") => "Hyperlink",
        (Family::Text, "Visited_20_Internet_20_Link") => "FollowedHyperlink",
        (Family::Paragraph, n) => {
            let level = n.strip_prefix("Heading_20_")?;
            return match level.parse::<u8>() {
                Ok(l) if (1..=9).contains(&l) => Some(format!("Heading{l}")),
                _ => None,
            };
        }
        _ => return None,
    };
    Some(id.to_owned())
}

struct Seg {
    start: usize,
    end: usize,
    attrs: CharAttrs,
    style: Option<String>,
}

#[derive(Clone)]
struct InlineState {
    direct: CharAttrs,
    style: Option<String>,
    size: u32,
}

struct ParaBuilder {
    id: NodeId,
    text: String,
    budget: usize,
    prev_space: bool,
    segs: Vec<Seg>,
    links: Vec<(usize, usize, LinkTarget, Option<String>)>,
    bookmarks: Vec<(String, usize, usize)>,
    open: Vec<(String, usize)>,
    fields: Vec<(usize, usize, String)>,
    anchored: Vec<AnchoredObject>,
}

impl ParaBuilder {
    fn new(id: NodeId, budget: usize) -> Self {
        Self {
            id,
            text: String::new(),
            budget,
            prev_space: true,
            segs: Vec::new(),
            links: Vec::new(),
            bookmarks: Vec::new(),
            open: Vec::new(),
            fields: Vec::new(),
            anchored: Vec::new(),
        }
    }

    fn mark(&mut self, start: usize, st: &InlineState) {
        let end = self.text.len();
        if end == start {
            return;
        }
        match self.segs.last_mut() {
            Some(l) if l.end == start && l.attrs == st.direct && l.style == st.style => l.end = end,
            _ => self.segs.push(Seg {
                start,
                end,
                attrs: st.direct.clone(),
                style: st.style.clone(),
            }),
        }
    }

    fn check(&self) -> Result<(), OdtError> {
        if self.text.len() > self.budget {
            Err(OdtError::Limit("text bytes"))
        } else {
            Ok(())
        }
    }

    /// Append character data, collapsing white space per ODF 6.1.2.
    fn collapsed(&mut self, s: &str, st: &InlineState) -> Result<(), OdtError> {
        let start = self.text.len();
        for ch in s.chars() {
            if matches!(ch, ' ' | '\t' | '\r' | '\n') {
                if !self.prev_space {
                    self.text.push(' ');
                    self.prev_space = true;
                }
            } else {
                self.text.push(ch);
                self.prev_space = false;
            }
        }
        self.check()?;
        self.mark(start, st);
        Ok(())
    }

    fn raw(&mut self, s: &str, st: &InlineState) -> Result<(), OdtError> {
        let start = self.text.len();
        self.text.push_str(s);
        self.prev_space = false;
        self.check()?;
        self.mark(start, st);
        Ok(())
    }
}

#[derive(Clone)]
struct ListCtx {
    id: u32,
    level: u8,
    style: Option<String>,
}

#[derive(Clone)]
struct Ctx {
    scope: Scope,
    depth: usize,
    list: Option<ListCtx>,
}

struct ParaInfo {
    style: Option<String>,
    direct: ParaProps,
    base_char: CharAttrs,
    size: u32,
    master: Option<String>,
    list_style: Option<String>,
}

struct TextInfo {
    /// Size (half-points) defined by the common parent style chain, if any.
    base: Option<u32>,
    style: Option<String>,
    direct: CharAttrs,
    size: Option<Size>,
}

struct ItemMark {
    header: bool,
    start_value: Option<u32>,
    has_paragraph: bool,
}

struct FinishedSection {
    master: Option<String>,
    body: Vec<NodeId>,
}

pub(super) struct Output {
    pub document: RichDocument,
    pub engine: StyleEngine,
    pub run_styles: Vec<ImportedRunStyle>,
    pub paragraph_order: Vec<NodeId>,
    pub sections: Vec<ImportedSection>,
    pub lists: Vec<ImportedList>,
    pub list_items: Vec<ImportedListItem>,
    pub images: Vec<ImportedImage>,
    pub headings: Vec<(NodeId, u8)>,
    pub style_names: BTreeMap<String, String>,
    pub next_styles: BTreeMap<String, String>,
}

pub(super) struct Importer<'a> {
    limits: &'a OdtLimits,
    styles: &'a Styles,
    archive: Option<&'a Archive<'a>>,
    media_types: &'a HashMap<String, String>,
    notices: &'a mut Notices,
    doc: RichDocument,
    engine: StyleEngine,
    para_ids: HashMap<String, String>,
    text_ids: HashMap<String, String>,
    names: BTreeMap<String, String>,
    next_styles: BTreeMap<String, String>,
    nodes: BTreeMap<NodeId, Node>,
    stack: Vec<Vec<NodeId>>,
    finished: Vec<FinishedSection>,
    cur_master: Option<String>,
    section_nodes: Vec<BTreeMap<NodeId, Node>>,
    para_cache: HashMap<(u8, Option<String>), Rc<ParaInfo>>,
    text_cache: HashMap<(u8, String), Rc<TextInfo>>,
    run_styles: Vec<ImportedRunStyle>,
    para_order: Vec<NodeId>,
    in_header: bool,
    paragraphs: usize,
    text_bytes: usize,
    cells: usize,
    image_bytes: usize,
    bookmark_names: HashSet<String>,
    images: Vec<ImportedImage>,
    headings: Vec<(NodeId, u8)>,
    list_ids: HashMap<String, u32>,
    list_by_style: HashMap<String, u32>,
    list_defs: Vec<(u32, Option<String>)>,
    next_list: u32,
    list_items: Vec<ImportedListItem>,
    item: Option<ItemMark>,
}

impl<'a> Importer<'a> {
    pub fn new(
        limits: &'a OdtLimits,
        styles: &'a Styles,
        archive: Option<&'a Archive<'a>>,
        media_types: &'a HashMap<String, String>,
        notices: &'a mut Notices,
    ) -> Result<Self, OdtError> {
        let mut me = Self {
            limits,
            styles,
            archive,
            media_types,
            notices,
            doc: RichDocument::new(),
            engine: StyleEngine::new(StyleCatalog::default(), Theme::default(), 1024),
            para_ids: HashMap::new(),
            text_ids: HashMap::new(),
            names: BTreeMap::new(),
            next_styles: BTreeMap::new(),
            nodes: BTreeMap::new(),
            stack: vec![Vec::new()],
            finished: Vec::new(),
            cur_master: None,
            section_nodes: Vec::new(),
            para_cache: HashMap::new(),
            text_cache: HashMap::new(),
            run_styles: Vec::new(),
            para_order: Vec::new(),
            in_header: false,
            paragraphs: 0,
            text_bytes: 0,
            cells: 0,
            image_bytes: 0,
            bookmark_names: HashSet::new(),
            images: Vec::new(),
            headings: Vec::new(),
            list_ids: HashMap::new(),
            list_by_style: HashMap::new(),
            list_defs: Vec::new(),
            next_list: 1,
            list_items: Vec::new(),
            item: None,
        };
        me.build_catalog()?;
        me.cur_master = me
            .styles
            .masters
            .iter()
            .find(|m| m.name == "Standard")
            .or_else(|| me.styles.masters.first())
            .map(|m| m.name.clone());
        Ok(me)
    }

    // ----- style catalog -------------------------------------------------

    fn unsupported(&mut self, style: &str, rest: &[(String, String)]) {
        for (k, v) in rest {
            self.notices.add(ImportNotice::UnsupportedProperty {
                style: style.to_owned(),
                property: k.clone(),
                value: v.clone(),
            });
        }
    }

    fn build_catalog(&mut self) -> Result<(), OdtError> {
        let styles = self.styles;
        let mut defaults = DocumentStyleDefaults::default();
        if let Some(d) = &styles.default_para {
            let c = convert_text(&d.text, &styles.fonts);
            defaults.character.values = c.attrs;
            if let Some(Size::Abs(v)) = c.size {
                defaults.character.values.size_half_points = Some(v);
            }
            self.unsupported("(default)", &c.unsupported);
            let p = convert_para(&d.para);
            defaults.paragraph = p.props;
            self.unsupported("(default)", &p.unsupported);
        }
        self.engine = StyleEngine::new(StyleCatalog::new(defaults), Theme::default(), 1024);
        // Assign ids: built-in equivalents first, then the raw ODF name.
        let mut used: HashSet<String> = HashSet::new();
        let order: Vec<(Family, String)> = styles
            .common_order
            .iter()
            .filter(|(f, _)| matches!(f, Family::Paragraph | Family::Text))
            .cloned()
            .collect();
        for (family, name) in &order {
            let mut id = builtin_id(*family, name).unwrap_or_else(|| name.clone());
            let mut n = 2;
            while !used.insert(id.clone()) {
                id = format!(
                    "{}-{n}",
                    builtin_id(*family, name).unwrap_or_else(|| name.clone())
                );
                n += 1;
            }
            match family {
                Family::Paragraph => self.para_ids.insert(name.clone(), id.clone()),
                _ => self.text_ids.insert(name.clone(), id.clone()),
            };
            if let Some(s) = styles.common.get(&(*family, name.clone())) {
                self.names.insert(
                    id.clone(),
                    s.display_name.clone().unwrap_or_else(|| name.clone()),
                );
            }
        }
        // Detect inheritance cycles before handing styles to the engine.
        let mut broken: HashSet<(Family, String)> = HashSet::new();
        for (family, name) in &order {
            let mut seen = HashSet::new();
            let mut cur = Some(name.clone());
            while let Some(c) = cur {
                if !seen.insert(c.clone()) {
                    if c == *name {
                        broken.insert((*family, name.clone()));
                        self.notices
                            .add(ImportNotice::StyleCycle { name: name.clone() });
                    }
                    break;
                }
                cur = styles
                    .common
                    .get(&(*family, c))
                    .and_then(|s| s.parent.clone());
            }
        }
        for (family, name) in &order {
            let Some(s) = styles.common.get(&(*family, name.clone())) else {
                continue;
            };
            let ids = if *family == Family::Paragraph {
                &self.para_ids
            } else {
                &self.text_ids
            };
            let id = ids[name].clone();
            let based_on = match &s.parent {
                Some(p) if !broken.contains(&(*family, name.clone())) => {
                    if let Some(pid) = ids.get(p) {
                        Some(pid.clone())
                    } else {
                        let fam = format!("{family:?}");
                        self.notices.add(ImportNotice::UnknownStyle {
                            family: fam,
                            name: p.clone(),
                        });
                        None
                    }
                }
                _ => None,
            };
            let parent_size = match &s.parent {
                Some(p) => styles.effective_size(Scope::Styles, *family, p),
                None => styles.default_size(),
            };
            let c = convert_text(&s.text, &styles.fonts);
            let mut values = c.attrs;
            if let Some(sz) = c.size {
                values.size_half_points = Some(resolve_size(sz, parent_size));
            }
            self.unsupported(name, &c.unsupported);
            let mut paragraph = ParaProps::default();
            let mut kind = StyleKind::Character;
            if *family == Family::Paragraph {
                kind = StyleKind::Paragraph;
                let p = convert_para(&s.para);
                paragraph = p.props;
                self.unsupported(name, &p.unsupported);
                if let Some(ls) = &s.list_style {
                    let id = self.implicit_list(ls);
                    paragraph.list = Some(ListRef {
                        list_id: id,
                        level: 0,
                    });
                }
                if let Some(n) = &s.next {
                    if let Some(nid) = self.para_ids.get(n) {
                        self.next_styles.insert(id.clone(), nid.clone());
                    }
                }
            }
            self.engine
                .set_style(Style {
                    id,
                    kind,
                    based_on,
                    linked_style: None,
                    character: StyleCharAttrs {
                        values,
                        ..StyleCharAttrs::default()
                    },
                    paragraph,
                })
                .map_err(|e| OdtError::Style(e.to_string()))?;
        }
        Ok(())
    }

    // ----- lists ---------------------------------------------------------

    fn implicit_list(&mut self, style: &str) -> u32 {
        if let Some(id) = self.list_by_style.get(style) {
            return *id;
        }
        let id = self.fresh_list(Some(style.to_owned()));
        self.list_by_style.insert(style.to_owned(), id);
        id
    }

    fn fresh_list(&mut self, style: Option<String>) -> u32 {
        let id = self.next_list;
        self.next_list = self.next_list.saturating_add(1);
        self.list_defs.push((id, style.clone()));
        if let Some(s) = style {
            self.notices.add(ImportNotice::ListNumbering { style: s });
        }
        id
    }

    // ----- paragraph / text style resolution -----------------------------

    fn para_info(&mut self, scope: Scope, name: Option<&str>) -> Rc<ParaInfo> {
        let key = (scope as u8, name.map(str::to_owned));
        if let Some(i) = self.para_cache.get(&key) {
            return i.clone();
        }
        let styles = self.styles;
        let mut autos: Vec<&OdfStyle> = Vec::new();
        let mut common: Option<&OdfStyle> = None;
        let mut cur = name.map(str::to_owned);
        let mut steps = 0;
        while let Some(c) = cur.take() {
            steps += 1;
            if steps > 64 {
                self.notices.add(ImportNotice::StyleCycle { name: c });
                break;
            }
            match styles.lookup(scope, Family::Paragraph, &c) {
                Some((s, true)) => {
                    autos.push(s);
                    cur = s.parent.clone();
                }
                Some((s, false)) => {
                    common = Some(s);
                    break;
                }
                None => {
                    self.notices.add(ImportNotice::UnknownStyle {
                        family: "Paragraph".into(),
                        name: c,
                    });
                    break;
                }
            }
        }
        let mut size = match common {
            Some(c) => styles.effective_size(scope, Family::Paragraph, &c.name),
            None => styles.default_size(),
        };
        let mut direct = ParaProps::default();
        let mut base_char = CharAttrs::default();
        for s in autos.iter().rev() {
            let p = convert_para(&s.para);
            direct.overlay(&p.props);
            let label = s.name.clone();
            self.unsupported(&label, &p.unsupported);
            let c = convert_text(&s.text, &styles.fonts);
            base_char.overlay(&c.attrs);
            if let Some(sz) = c.size {
                size = resolve_size(sz, size);
                base_char.size_half_points = Some(size);
            }
            self.unsupported(&label, &c.unsupported);
        }
        let master = autos
            .iter()
            .find_map(|s| s.master_page.clone())
            .or_else(|| common.and_then(|c| c.master_page.clone()));
        let list_style = autos.iter().find_map(|s| s.list_style.clone());
        let style = common.and_then(|c| self.para_ids.get(&c.name).cloned());
        let info = Rc::new(ParaInfo {
            style,
            direct,
            base_char,
            size,
            master,
            list_style,
        });
        self.para_cache.insert(key, info.clone());
        info
    }

    fn text_info(&mut self, scope: Scope, name: &str) -> Rc<TextInfo> {
        let key = (scope as u8, name.to_owned());
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let styles = self.styles;
        let mut autos: Vec<&OdfStyle> = Vec::new();
        let mut common: Option<&OdfStyle> = None;
        let mut cur = Some(name.to_owned());
        let mut steps = 0;
        while let Some(c) = cur.take() {
            steps += 1;
            if steps > 64 {
                self.notices.add(ImportNotice::StyleCycle { name: c });
                break;
            }
            match styles.lookup(scope, Family::Text, &c) {
                Some((s, true)) => {
                    autos.push(s);
                    cur = s.parent.clone();
                }
                Some((s, false)) => {
                    common = Some(s);
                    break;
                }
                None => {
                    self.notices.add(ImportNotice::UnknownStyle {
                        family: "Text".into(),
                        name: c,
                    });
                    break;
                }
            }
        }
        let mut direct = CharAttrs::default();
        let mut size: Option<Size> = None;
        for s in autos.iter().rev() {
            let c = convert_text(&s.text, &styles.fonts);
            direct.overlay(&c.attrs);
            if let Some(sz) = c.size {
                size = Some(match (size, sz) {
                    (Some(Size::Rel(a)), Size::Rel(b)) => Size::Rel(a * b / 100.0),
                    _ => sz,
                });
            }
            let label = s.name.clone();
            self.unsupported(&label, &c.unsupported);
        }
        let style = common.and_then(|c| self.text_ids.get(&c.name).cloned());
        let base = common.and_then(|c| styles.defined_size(Scope::Styles, Family::Text, &c.name));
        let info = Rc::new(TextInfo {
            style,
            direct,
            size,
            base,
        });
        self.text_cache.insert(key, info.clone());
        info
    }

    /// Character properties defined by a style or its ancestors.
    fn style_char_mask(&self, id: &str) -> CharAttrs {
        let mut mask = CharAttrs::default();
        let mut cur = Some(id.to_owned());
        for _ in 0..64 {
            let Some(c) = cur.take() else { break };
            let Some(style) = self.engine.catalog().style(&c) else {
                break;
            };
            mask.overlay(&style.character.values);
            cur = style.based_on.clone();
        }
        mask
    }

    fn apply_text_style(&mut self, ctx: &Ctx, name: Option<&str>, st: &InlineState) -> InlineState {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return st.clone();
        };
        let info = self.text_info(ctx.scope, name);
        let mut next = st.clone();
        next.direct.overlay(&info.direct);
        if let Some(b) = info.base {
            next.size = b;
        }
        if let Some(sz) = info.size {
            next.size = resolve_size(sz, next.size);
            next.direct.size_half_points = Some(next.size);
        }
        if info.style.is_some() {
            next.style = info.style.clone();
        }
        next
    }

    // ----- sinks ---------------------------------------------------------

    fn push_block(&mut self, id: NodeId) {
        if let Some(top) = self.stack.last_mut() {
            top.push(id);
        }
    }

    fn begin(&mut self) {
        self.stack.push(Vec::new());
    }

    fn end(&mut self) -> Vec<NodeId> {
        if self.stack.len() > 1 {
            self.stack.pop().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    fn break_section(&mut self, master: String) {
        if self.stack.len() != 1 {
            return;
        }
        if self.stack[0].is_empty() && self.finished.is_empty() {
            self.cur_master = Some(master);
            return;
        }
        let body = std::mem::take(&mut self.stack[0]);
        self.finished.push(FinishedSection {
            master: self.cur_master.take(),
            body,
        });
        // Nodes of the finished section are moved aside so that each section
        // becomes a self-contained subtree.
        let nodes = std::mem::take(&mut self.nodes);
        self.section_nodes.push(nodes);
        self.cur_master = Some(master);
    }

    fn maybe_switch_master(&mut self, master: Option<String>) {
        if self.stack.len() != 1 || self.in_header {
            return;
        }
        if let Some(m) = master {
            if self.cur_master.as_deref() != Some(m.as_str()) {
                if !self.styles.masters.iter().any(|x| x.name == m) {
                    self.notices.add(ImportNotice::UnknownStyle {
                        family: "MasterPage".into(),
                        name: m,
                    });
                    return;
                }
                self.break_section(m);
            }
        }
    }

    // ----- blocks --------------------------------------------------------

    pub fn run(&mut self, text: &XNode) -> Result<(), OdtError> {
        let ctx = Ctx {
            scope: Scope::Content,
            depth: 0,
            list: None,
        };
        self.blocks(text, &ctx)
    }

    fn blocks(&mut self, parent: &XNode, ctx: &Ctx) -> Result<(), OdtError> {
        for e in parent.elements() {
            self.block(e, ctx)?;
        }
        Ok(())
    }

    fn block(&mut self, e: &XNode, ctx: &Ctx) -> Result<(), OdtError> {
        match (e.ns.as_str(), e.local.as_str()) {
            (NS_TEXT, "p") => self.paragraph(e, false, ctx),
            (NS_TEXT, "h") => self.paragraph(e, true, ctx),
            (NS_TEXT, "list") => self.list(e, ctx),
            (NS_TABLE, "table") => self.table(e, ctx),
            (NS_TEXT, "section") => {
                self.notices.add(ImportNotice::FlattenedSection {
                    name: e.attr(NS_TEXT, "name").unwrap_or("").to_owned(),
                });
                self.nested(e, ctx, "sections")
            }
            (NS_TEXT, "table-of-content")
            | (NS_TEXT, "illustration-index")
            | (NS_TEXT, "table-index")
            | (NS_TEXT, "object-index")
            | (NS_TEXT, "user-index")
            | (NS_TEXT, "alphabetical-index")
            | (NS_TEXT, "bibliography") => {
                self.notices.add(ImportNotice::FlattenedSection {
                    name: e.local.clone(),
                });
                if let Some(body) = e.find(NS_TEXT, "index-body") {
                    self.nested(body, ctx, "indexes")?;
                }
                Ok(())
            }
            (NS_TEXT, "index-title") => self.nested(e, ctx, "indexes"),
            (NS_TEXT, "tracked-changes") => {
                self.notices.add(ImportNotice::TrackedChangesIgnored);
                Ok(())
            }
            (NS_TEXT, "soft-page-break")
            | (NS_TEXT, "sequence-decls")
            | (NS_TEXT, "variable-decls")
            | (NS_TEXT, "user-field-decls")
            | (NS_TEXT, "dde-connection-decls")
            | (NS_TEXT, "alphabetical-index-auto-mark-file")
            | (NS_TEXT, "change")
            | (NS_TEXT, "change-start")
            | (NS_TEXT, "change-end")
            | (NS_TABLE, "calculation-settings")
            | (NS_TABLE, "content-validations")
            | (NS_OFFICE, "forms")
            | (NS_TEXT, "page-sequence") => Ok(()),
            (NS_OFFICE, "annotation") | (NS_OFFICE, "annotation-end") => {
                self.notices.add(ImportNotice::UnsupportedElement {
                    part: "content.xml".into(),
                    name: e.qualified(),
                });
                Ok(())
            }
            (NS_DRAW, _) => {
                // A drawing object directly in the text flow: wrap it in a paragraph.
                self.count_paragraph()?;
                let id = self.doc.alloc_id().map_err(model)?;
                let mut pb = ParaBuilder::new(id, self.limits.max_text_bytes - self.text_bytes);
                let st = InlineState {
                    direct: CharAttrs::default(),
                    style: None,
                    size: self.styles.default_size(),
                };
                self.draw(e, &mut pb, &st, ctx)?;
                self.finish_paragraph(pb, &ParaInfoRef::none(), None, ctx, false)
            }
            _ => {
                self.notices.add(ImportNotice::UnsupportedElement {
                    part: "content.xml".into(),
                    name: e.qualified(),
                });
                Ok(())
            }
        }
    }

    fn nested(&mut self, e: &XNode, ctx: &Ctx, what: &'static str) -> Result<(), OdtError> {
        if ctx.depth >= self.limits.max_nesting {
            self.notices.add(ImportNotice::NestingLimit { what });
            return Ok(());
        }
        let inner = Ctx {
            depth: ctx.depth + 1,
            ..ctx.clone()
        };
        self.blocks(e, &inner)
    }

    fn count_paragraph(&mut self) -> Result<(), OdtError> {
        self.paragraphs += 1;
        if self.paragraphs > self.limits.max_paragraphs {
            return Err(OdtError::Limit("paragraph count"));
        }
        Ok(())
    }

    fn paragraph(&mut self, n: &XNode, heading: bool, ctx: &Ctx) -> Result<(), OdtError> {
        self.count_paragraph()?;
        let name = n.attr(NS_TEXT, "style-name").filter(|s| !s.is_empty());
        let info = self.para_info(ctx.scope, name);
        self.maybe_switch_master(info.master.clone());
        let id = self.doc.alloc_id().map_err(model)?;
        let mut pb = ParaBuilder::new(id, self.limits.max_text_bytes - self.text_bytes);
        let st = InlineState {
            direct: CharAttrs::default(),
            style: None,
            size: info.size,
        };
        self.inline_children(n, &mut pb, &st, ctx, 0)?;
        let mut level = None;
        if heading {
            let l = n
                .attr(NS_TEXT, "outline-level")
                .and_then(|v| v.trim().parse::<u8>().ok())
                .unwrap_or(1)
                .clamp(1, 10);
            self.headings.push((id, l));
            level = Some(l);
        }
        let iref = ParaInfoRef::of(&info);
        self.finish_paragraph(pb, &iref, level, ctx, true)
    }

    fn finish_paragraph(
        &mut self,
        mut pb: ParaBuilder,
        info: &ParaInfoRef,
        heading: Option<u8>,
        ctx: &Ctx,
        record_order: bool,
    ) -> Result<(), OdtError> {
        for (name, start) in std::mem::take(&mut pb.open) {
            self.notices
                .add(ImportNotice::BookmarkSpansParagraphs { name: name.clone() });
            let end = pb.text.len();
            pb.bookmarks.push((name, start, end));
        }
        self.text_bytes += pb.text.len();
        if self.text_bytes > self.limits.max_text_bytes {
            return Err(OdtError::Limit("text bytes"));
        }
        let text = std::mem::take(&mut pb.text);
        let idx = grapheme_boundaries(&text);
        let lo = |o: usize| idx.floor_boundary(o.min(text.len())).unwrap_or(0);
        let hi = |o: usize| idx.ceil_boundary(o.min(text.len())).unwrap_or(text.len());
        // Runs and run-style records.
        let mut runs: Vec<CharRun> = Vec::new();
        let mut styled: Vec<ImportedRunStyle> = Vec::new();
        let mut floor = 0usize;
        for seg in &pb.segs {
            let start = lo(seg.start).max(floor);
            let end = hi(seg.end);
            if start >= end {
                continue;
            }
            floor = end;
            let mut attrs = info.base_char.clone();
            if let Some(style) = &seg.style {
                // Paragraph text properties rank below a named character style.
                attrs.clear(&self.style_char_mask(style));
            }
            attrs.overlay(&seg.attrs);
            if !attrs.is_empty() {
                match runs.last_mut() {
                    Some(l) if l.end == start && l.attrs == attrs => l.end = end,
                    _ => runs.push(CharRun { start, end, attrs }),
                }
            }
            if seg.style.is_some() {
                match styled.last_mut() {
                    Some(l) if l.end == start && l.style_id == seg.style => l.end = end,
                    _ => styled.push(ImportedRunStyle {
                        paragraph: pb.id,
                        start,
                        end,
                        style_id: seg.style.clone(),
                    }),
                }
            }
        }
        let mut props = info.direct.clone();
        props.style = info.style.clone();
        if props.style.is_none() {
            if let Some(l) = heading {
                let hid = format!("Heading{l}");
                if self.engine.catalog().style(&hid).is_some() {
                    props.style = Some(hid);
                }
            }
        }
        let mut item_record = None;
        if let Some(l) = &ctx.list {
            props.list = Some(ListRef {
                list_id: l.id,
                level: l.level.min(MAX_LIST_LEVEL),
            });
            if let Some(item) = self.item.as_mut() {
                let continuation = item.has_paragraph;
                item.has_paragraph = true;
                if item.header || item.start_value.is_some() || continuation {
                    item_record = Some(ImportedListItem {
                        paragraph: pb.id,
                        list_id: l.id,
                        level: l.level.min(MAX_LIST_LEVEL),
                        header: item.header,
                        start_value: item.start_value.take(),
                        continuation,
                    });
                }
            }
        } else if let Some(ls) = &info.list_style {
            let id = self.implicit_list(ls);
            props.list = Some(ListRef {
                list_id: id,
                level: 0,
            });
        }
        if let Some(r) = item_record {
            self.list_items.push(r);
        }
        let mut links: Vec<Link> = Vec::new();
        let mut raw_links = std::mem::take(&mut pb.links);
        raw_links.sort_by_key(|l| (lo(l.0), hi(l.1)));
        let mut link_floor = 0usize;
        for (s, e, target, tooltip) in raw_links {
            let (s, e) = (lo(s), hi(e));
            if s >= e {
                continue;
            }
            if s < link_floor {
                self.notices.add(ImportNotice::OverlappingLink);
                continue;
            }
            link_floor = e;
            links.push(Link {
                id: self.doc.alloc_id().map_err(model)?,
                start: s,
                end: e,
                target,
                tooltip,
            });
        }
        let mut fields: Vec<Field> = Vec::new();
        let mut field_floor = 0usize;
        for (s, e, instruction) in pb.fields.drain(..) {
            let (s, e) = (lo(s), hi(e));
            if s < field_floor || s > e {
                continue;
            }
            field_floor = e;
            fields.push(Field {
                id: self.doc.alloc_id().map_err(model)?,
                start: s,
                end: e,
                instruction,
            });
        }
        let mut bookmarks: Vec<Bookmark> = Vec::new();
        for (name, s, e) in pb.bookmarks.drain(..) {
            if name.is_empty() {
                continue;
            }
            if !self.bookmark_names.insert(name.clone()) {
                self.notices.add(ImportNotice::DuplicateBookmark { name });
                continue;
            }
            let (s, e) = (lo(s), hi(e));
            bookmarks.push(Bookmark {
                id: self.doc.alloc_id().map_err(model)?,
                name,
                start: s,
                end: e.max(s),
            });
        }
        bookmarks.sort_by_key(|b| (b.start, b.end, b.id));
        let mut anchored = std::mem::take(&mut pb.anchored);
        for a in &mut anchored {
            a.offset = lo(a.offset);
        }
        anchored.sort_by_key(|a| (a.offset, a.id));
        let paragraph = Paragraph {
            text,
            runs,
            props,
            anchored,
            breaks: Vec::new(),
            symbols: Vec::new(),
            fields,
            bookmarks,
            links,
        };
        paragraph.validate_local().map_err(model)?;
        self.run_styles.extend(styled);
        self.nodes.insert(pb.id, Node::Paragraph(paragraph));
        if record_order && !self.in_header {
            self.para_order.push(pb.id);
        }
        self.push_block(pb.id);
        Ok(())
    }

    // ----- inline content --------------------------------------------------

    fn inline_children(
        &mut self,
        n: &XNode,
        pb: &mut ParaBuilder,
        st: &InlineState,
        ctx: &Ctx,
        depth: usize,
    ) -> Result<(), OdtError> {
        if depth > 512 {
            return Err(OdtError::Limit("inline nesting"));
        }
        for c in &n.children {
            match c {
                Child::Text(t) => pb.collapsed(t, st)?,
                Child::Elem(e) => self.inline(e, pb, st, ctx, depth)?,
            }
        }
        Ok(())
    }

    fn inline(
        &mut self,
        e: &XNode,
        pb: &mut ParaBuilder,
        st: &InlineState,
        ctx: &Ctx,
        depth: usize,
    ) -> Result<(), OdtError> {
        match (e.ns.as_str(), e.local.as_str()) {
            (NS_TEXT, "span") => {
                let next = self.apply_text_style(ctx, e.attr(NS_TEXT, "style-name"), st);
                self.inline_children(e, pb, &next, ctx, depth + 1)
            }
            (NS_TEXT, "s") => {
                let count = match e.attr(NS_TEXT, "c") {
                    None => 1,
                    Some(v) => parse_count(v).unwrap_or_else(|| {
                        self.notices.add(ImportNotice::InvalidAttribute {
                            element: "text:s".into(),
                            attribute: "text:c".into(),
                            value: v.chars().take(40).collect(),
                        });
                        1
                    }),
                };
                if count > pb.budget.saturating_sub(pb.text.len()) {
                    return Err(OdtError::Limit("text bytes"));
                }
                pb.raw(&" ".repeat(count), st)
            }
            (NS_TEXT, "tab") => pb.raw("\t", st),
            (NS_TEXT, "line-break") => pb.raw("\n", st),
            (NS_TEXT, "soft-page-break")
            | (NS_TEXT, "change")
            | (NS_TEXT, "change-start")
            | (NS_TEXT, "change-end")
            | (NS_TEXT, "reference-mark")
            | (NS_TEXT, "reference-mark-start")
            | (NS_TEXT, "reference-mark-end") => Ok(()),
            (NS_TEXT, "a") => {
                let next = self.apply_text_style(ctx, e.attr(NS_TEXT, "style-name"), st);
                let start = pb.text.len();
                self.inline_children(e, pb, &next, ctx, depth + 1)?;
                let end = pb.text.len();
                if let Some(href) = e.attr(NS_XLINK, "href").filter(|h| !h.is_empty()) {
                    let target = match href.strip_prefix('#') {
                        Some(b) => {
                            LinkTarget::Bookmark(b.split('|').next().unwrap_or(b).to_owned())
                        }
                        None => LinkTarget::External(href.to_owned()),
                    };
                    let tooltip = e.attr(NS_OFFICE, "title").map(str::to_owned);
                    if end > start {
                        pb.links.push((start, end, target, tooltip));
                    }
                }
                Ok(())
            }
            (NS_TEXT, "bookmark") => {
                if let Some(name) = e.attr(NS_TEXT, "name") {
                    let at = pb.text.len();
                    pb.bookmarks.push((name.to_owned(), at, at));
                }
                Ok(())
            }
            (NS_TEXT, "bookmark-start") => {
                if let Some(name) = e.attr(NS_TEXT, "name") {
                    pb.open.push((name.to_owned(), pb.text.len()));
                }
                Ok(())
            }
            (NS_TEXT, "bookmark-end") => {
                if let Some(name) = e.attr(NS_TEXT, "name") {
                    if let Some(pos) = pb.open.iter().rposition(|(n, _)| n == name) {
                        let (n, s) = pb.open.remove(pos);
                        let end = pb.text.len();
                        pb.bookmarks.push((n, s, end));
                    }
                }
                Ok(())
            }
            (NS_TEXT, "page-number") | (NS_TEXT, "page-count") => {
                let (instr, fallback) = if e.local == "page-number" {
                    ("PAGE", "1")
                } else {
                    ("NUMPAGES", "1")
                };
                self.field(e, pb, st, instr, fallback)
            }
            (NS_TEXT, "date") | (NS_TEXT, "time") => {
                if e.attr(NS_TEXT, "fixed") == Some("true") {
                    let t = e.text_content();
                    return pb.raw(&t, st);
                }
                let instr = if e.local == "date" { "DATE" } else { "TIME" };
                self.field(e, pb, st, instr, "")
            }
            (NS_TEXT, "note") => {
                self.notices.add(ImportNotice::NoteContentIgnored {
                    id: e.attr(NS_TEXT, "id").unwrap_or("").to_owned(),
                });
                if let Some(cit) = e.find(NS_TEXT, "note-citation") {
                    let mut next = st.clone();
                    next.direct.vertical_align =
                        Some(atelier_text_model::rich::VerticalAlign::Superscript);
                    let t = cit.text_content();
                    pb.raw(&t, &next)?;
                }
                Ok(())
            }
            (NS_TEXT, "ruby") => {
                self.notices.add(ImportNotice::UnsupportedElement {
                    part: "content.xml".into(),
                    name: "text:ruby-text".into(),
                });
                if let Some(base) = e.find(NS_TEXT, "ruby-base") {
                    self.inline_children(base, pb, st, ctx, depth + 1)?;
                }
                Ok(())
            }
            (NS_OFFICE, "annotation") | (NS_OFFICE, "annotation-end") => {
                self.notices.add(ImportNotice::UnsupportedElement {
                    part: "content.xml".into(),
                    name: e.qualified(),
                });
                Ok(())
            }
            (NS_DRAW, _) => self.draw(e, pb, st, ctx),
            (NS_TEXT, _) => {
                // Other fields and inline containers: keep their displayed result.
                self.notices.add(ImportNotice::FieldReplacedByResult {
                    name: e.local.clone(),
                });
                self.inline_children(e, pb, st, ctx, depth + 1)
            }
            _ => {
                self.notices.add(ImportNotice::UnsupportedElement {
                    part: "content.xml".into(),
                    name: e.qualified(),
                });
                self.inline_children(e, pb, st, ctx, depth + 1)
            }
        }
    }

    fn field(
        &mut self,
        e: &XNode,
        pb: &mut ParaBuilder,
        st: &InlineState,
        instr: &str,
        fallback: &str,
    ) -> Result<(), OdtError> {
        let mut result = e.text_content();
        if result.is_empty() {
            result = e
                .attr(NS_TEXT, "date-value")
                .or_else(|| e.attr(NS_TEXT, "time-value"))
                .unwrap_or(fallback)
                .to_owned();
        }
        let start = pb.text.len();
        pb.raw(&result, st)?;
        let end = pb.text.len();
        pb.fields.push((start, end, instr.to_owned()));
        Ok(())
    }

    // ----- frames and drawing objects --------------------------------------

    fn draw(
        &mut self,
        e: &XNode,
        pb: &mut ParaBuilder,
        _st: &InlineState,
        _ctx: &Ctx,
    ) -> Result<(), OdtError> {
        if e.local != "frame" {
            self.notices.add(ImportNotice::ObjectKept {
                kind: e.qualified(),
            });
            return self.push_object(pb, "odf:shape", serialize(e));
        }
        let anchor = match e.attr(NS_TEXT, "anchor-type") {
            Some("as-char") => ImageAnchor::AsChar,
            Some("char") => ImageAnchor::Char,
            Some("paragraph") => ImageAnchor::Paragraph,
            Some("page") => ImageAnchor::Page,
            Some("frame") => ImageAnchor::Frame,
            _ => ImageAnchor::Unknown,
        };
        let width = e.attr(NS_SVG, "width").and_then(length_twips);
        let height = e.attr(NS_SVG, "height").and_then(length_twips);
        let name = e.attr(NS_DRAW, "name").map(str::to_owned);
        let title = e.find(NS_SVG, "title").map(XNode::text_content);
        let description = e.find(NS_SVG, "desc").map(XNode::text_content);
        let has_object = e.elements().any(|c| {
            c.ns == NS_DRAW
                && matches!(
                    c.local.as_str(),
                    "object" | "object-ole" | "plugin" | "applet" | "floating-frame"
                )
        });
        let mut opaque_done = false;
        for c in e.elements() {
            if c.ns != NS_DRAW {
                continue;
            }
            match c.local.as_str() {
                "image" => {
                    if has_object {
                        self.notices.add(ImportNotice::ObjectKept {
                            kind: "fallback image of an embedded object".into(),
                        });
                    }
                    let meta = ImageMeta {
                        anchor,
                        width,
                        height,
                        name: name.clone(),
                        title: title.clone(),
                        description: description.clone(),
                    };
                    self.picture(c, pb, meta)?;
                }
                "text-box" | "object" | "object-ole" | "plugin" | "applet" | "floating-frame" => {
                    if !opaque_done {
                        opaque_done = true;
                        let kind = if c.local == "text-box" {
                            "odf:text-box"
                        } else {
                            "odf:object"
                        };
                        self.notices.add(ImportNotice::ObjectKept {
                            kind: c.qualified(),
                        });
                        self.push_object(pb, kind, serialize(e))?;
                    }
                }
                _ => {
                    if !opaque_done {
                        opaque_done = true;
                        self.notices.add(ImportNotice::ObjectKept {
                            kind: c.qualified(),
                        });
                        self.push_object(pb, "odf:shape", serialize(e))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn push_object(
        &mut self,
        pb: &mut ParaBuilder,
        kind: &str,
        data: Vec<u8>,
    ) -> Result<(), OdtError> {
        self.image_bytes = self.image_bytes.saturating_add(data.len());
        if self.image_bytes > self.limits.max_total_image_bytes {
            return Err(OdtError::Limit("object bytes"));
        }
        let id = self.doc.alloc_id().map_err(model)?;
        pb.prev_space = false;
        pb.anchored.push(AnchoredObject {
            id,
            offset: pb.text.len(),
            kind: kind.to_owned(),
            data,
        });
        Ok(())
    }

    fn picture(
        &mut self,
        c: &XNode,
        pb: &mut ParaBuilder,
        meta: ImageMeta,
    ) -> Result<(), OdtError> {
        if self.images.len() >= self.limits.max_images {
            return Err(OdtError::Limit("image count"));
        }
        let href = c.attr(NS_XLINK, "href").map(str::to_owned);
        let mut status = ImageStatus::Embedded;
        let mut bytes: Option<Vec<u8>> = None;
        if let Some(b) = c.find(NS_OFFICE, "binary-data") {
            match base64_decode(&b.text_content(), self.limits.max_image_bytes) {
                Some(v) => {
                    status = ImageStatus::InlineBinary;
                    bytes = Some(v);
                }
                None => {
                    self.notices.add(ImportNotice::ImageUnavailable {
                        href: "(binary-data)".into(),
                        reason: "invalid or oversized base64".into(),
                    });
                    status = ImageStatus::Missing;
                }
            }
        } else if let Some(h) = &href {
            let lower = h.to_ascii_lowercase();
            if lower.contains("://") || lower.starts_with("file:") || h.starts_with("..") {
                status = ImageStatus::External;
                self.notices.add(ImportNotice::ImageUnavailable {
                    href: h.clone(),
                    reason: "external image is never fetched".into(),
                });
            } else {
                let path = h.trim_start_matches("./").trim_start_matches('/');
                match self.archive.and_then(|a| a.entry(path)) {
                    Some(entry)
                        if entry.uncompressed_size() <= self.limits.max_image_bytes as u64 =>
                    {
                        match entry.read_all() {
                            Ok(v) => bytes = Some(v),
                            Err(e) => {
                                status = ImageStatus::Missing;
                                self.notices.add(ImportNotice::ImageUnavailable {
                                    href: h.clone(),
                                    reason: format!("{e}"),
                                });
                            }
                        }
                    }
                    Some(_) => {
                        status = ImageStatus::Missing;
                        self.notices.add(ImportNotice::ImageUnavailable {
                            href: h.clone(),
                            reason: "image exceeds the size limit".into(),
                        });
                    }
                    None => {
                        status = ImageStatus::Missing;
                        self.notices.add(ImportNotice::ImageUnavailable {
                            href: h.clone(),
                            reason: "part not found in the package".into(),
                        });
                    }
                }
            }
        } else {
            status = ImageStatus::Missing;
        }
        let (kind, data, media_type) = match bytes {
            Some(v) => {
                let mt = href
                    .as_deref()
                    .and_then(|h| self.media_types.get(h.trim_start_matches("./")))
                    .filter(|m| m.starts_with("image/"))
                    .cloned()
                    .or_else(|| sniff_media_type(&v).map(str::to_owned))
                    .unwrap_or_else(|| "application/octet-stream".to_owned());
                (mt.clone(), v, Some(mt))
            }
            None => (
                "odf:image-reference".to_owned(),
                href.clone().unwrap_or_default().into_bytes(),
                None,
            ),
        };
        self.image_bytes = self.image_bytes.saturating_add(data.len());
        if self.image_bytes > self.limits.max_total_image_bytes {
            return Err(OdtError::Limit("image bytes"));
        }
        let id = self.doc.alloc_id().map_err(model)?;
        pb.prev_space = false;
        pb.anchored.push(AnchoredObject {
            id,
            offset: pb.text.len(),
            kind,
            data,
        });
        self.images.push(
            ImageRecord {
                paragraph: pb.id,
                object: id,
                href,
                media_type,
                status,
                meta,
            }
            .into(),
        );
        Ok(())
    }

    // ----- lists ------------------------------------------------------------

    fn list(&mut self, n: &XNode, ctx: &Ctx) -> Result<(), OdtError> {
        if ctx.depth >= self.limits.max_nesting {
            self.notices
                .add(ImportNotice::NestingLimit { what: "lists" });
            return Ok(());
        }
        let style = n
            .attr(NS_TEXT, "style-name")
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .or_else(|| ctx.list.as_ref().and_then(|l| l.style.clone()));
        let (id, level) = match &ctx.list {
            Some(parent) => (parent.id, parent.level.saturating_add(1)),
            None => {
                let continued = n
                    .attr(NS_TEXT, "continue-list")
                    .and_then(|x| self.list_ids.get(x).copied())
                    .or_else(|| {
                        (n.attr(NS_TEXT, "continue-numbering") == Some("true"))
                            .then(|| {
                                style
                                    .as_ref()
                                    .and_then(|s| self.list_by_style.get(s).copied())
                            })
                            .flatten()
                    });
                let id = match continued {
                    Some(id) => id,
                    None => self.fresh_list(style.clone()),
                };
                if let Some(s) = &style {
                    self.list_by_style.insert(s.clone(), id);
                }
                if let Some(x) = n.attr(NS_XML, "id") {
                    self.list_ids.insert(x.to_owned(), id);
                }
                (id, 0)
            }
        };
        if level > MAX_LIST_LEVEL {
            self.notices.add(ImportNotice::NestingLimit {
                what: "list levels",
            });
        }
        let inner = Ctx {
            scope: ctx.scope,
            depth: ctx.depth + 1,
            list: Some(ListCtx { id, level, style }),
        };
        for item in n.elements() {
            let header = item.is(NS_TEXT, "list-header");
            if !header && !item.is(NS_TEXT, "list-item") {
                continue;
            }
            let saved = self.item.take();
            self.item = Some(ItemMark {
                header,
                start_value: item
                    .attr(NS_TEXT, "start-value")
                    .and_then(|v| v.trim().parse().ok()),
                has_paragraph: false,
            });
            self.blocks(item, &inner)?;
            self.item = saved;
        }
        Ok(())
    }

    // ----- tables -----------------------------------------------------------

    fn table(&mut self, n: &XNode, ctx: &Ctx) -> Result<(), OdtError> {
        if ctx.depth >= self.limits.max_nesting {
            self.notices
                .add(ImportNotice::NestingLimit { what: "tables" });
            return Ok(());
        }
        let styles = self.styles;
        let tstyle = n
            .attr(NS_TABLE, "style-name")
            .and_then(|s| styles.lookup(ctx.scope, Family::Table, s))
            .map(|(s, _)| s);
        if let Some(m) = tstyle.and_then(|s| s.master_page.clone()) {
            self.maybe_switch_master(Some(m));
        }
        // Columns.
        let mut widths: Vec<Option<i32>> = Vec::new();
        collect_columns(n, &mut |col| {
            let repeat = repeat_of(col, "number-columns-repeated");
            let w = col
                .attr(NS_TABLE, "style-name")
                .and_then(|s| styles.lookup(ctx.scope, Family::TableColumn, s))
                .and_then(|(s, _)| s.column.get("style:column-width"))
                .and_then(|v| length_twips(v));
            for _ in 0..repeat.min(self.limits.max_table_cells) {
                if widths.len() < self.limits.max_table_cells {
                    widths.push(w);
                }
            }
        });
        // Rows.
        let mut specs: Vec<RowSpec> = Vec::new();
        let mut header_rows = 0usize;
        let mut total = 0usize;
        let mut all_header = true;
        let mut failed = false;
        collect_rows(n, false, &mut |row, header| {
            if failed {
                return;
            }
            let repeat = repeat_of(row, "number-rows-repeated");
            let mut cells: Vec<CellSpec<'_>> = Vec::new();
            for c in row.elements() {
                let covered = c.is(NS_TABLE, "covered-table-cell");
                if !covered && !c.is(NS_TABLE, "table-cell") {
                    continue;
                }
                let r = repeat_of(c, "number-columns-repeated");
                let cs = span_of(c, "number-columns-spanned");
                let rs = span_of(c, "number-rows-spanned");
                total = total.saturating_add(r.saturating_mul(repeat));
                if total > self.limits.max_table_cells {
                    failed = true;
                    return;
                }
                for _ in 0..r {
                    cells.push(CellSpec {
                        node: c,
                        covered,
                        cs,
                        rs,
                    });
                }
            }
            if specs.len().saturating_add(repeat) > self.limits.max_table_cells {
                failed = true;
                return;
            }
            for _ in 0..repeat {
                specs.push(RowSpec {
                    cells: cells.clone(),
                });
            }
            all_header &= header;
            if all_header {
                header_rows = specs.len();
            }
        });
        if failed || self.cells.saturating_add(total) > self.limits.max_table_cells {
            return Err(OdtError::Limit("table cells"));
        }
        if specs.is_empty() {
            self.notices.add(ImportNotice::UnsupportedElement {
                part: "content.xml".into(),
                name: "empty table".into(),
            });
            return Ok(());
        }
        let columns = widths
            .len()
            .max(specs.iter().map(|r| r.cells.len()).max().unwrap_or(0))
            .max(1);
        let rows = specs.len();
        // The grid itself is allocated below: bound columns x rows first.
        let grid = columns.saturating_mul(rows);
        if grid > self.limits.max_table_cells
            || self.cells.saturating_add(grid) > self.limits.max_table_cells
        {
            return Err(OdtError::Limit("table cells"));
        }
        self.cells += grid;
        // Place cells.
        let mut owner: Vec<Vec<Option<usize>>> = vec![vec![None; columns]; rows];
        let mut placed: Vec<Placed<'_>> = Vec::new();
        for (r, row) in specs.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.covered || c >= columns || owner[r][c].is_some() {
                    continue;
                }
                // Free run on this row, then as many rows as stay free.
                let mut cs = 1;
                while cs < cell.cs.min(columns - c) && owner[r][c + cs].is_none() {
                    cs += 1;
                }
                let mut rs = 1;
                while rs < cell.rs.min(rows - r)
                    && owner[r + rs][c..c + cs].iter().all(Option::is_none)
                {
                    rs += 1;
                }
                let index = placed.len();
                for line in &mut owner[r..r + rs] {
                    for slot in &mut line[c..c + cs] {
                        *slot = Some(index);
                    }
                }
                placed.push(Placed {
                    row: r,
                    col: c,
                    cs,
                    rs,
                    node: Some(cell.node),
                });
            }
        }
        for (r, line) in owner.iter_mut().enumerate() {
            for (c, slot) in line.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(placed.len());
                    placed.push(Placed {
                        row: r,
                        col: c,
                        cs: 1,
                        rs: 1,
                        node: None,
                    });
                }
            }
        }
        let inner = Ctx {
            scope: ctx.scope,
            depth: ctx.depth + 1,
            list: None,
        };
        let saved_item = self.item.take();
        let mut row_ids = Vec::new();
        let mut table_nodes: Vec<(NodeId, Node)> = Vec::new();
        let mut by_row: Vec<Vec<usize>> = vec![Vec::new(); rows];
        for (index, p) in placed.iter().enumerate() {
            by_row[p.row].push(index);
        }
        for (r, members) in by_row.iter().enumerate() {
            let _ = r;
            let mut cell_ids = Vec::new();
            for &index in members {
                let p = &placed[index];
                let cell_id = self.doc.alloc_id().map_err(model)?;
                self.begin();
                let mut opaque = Vec::new();
                if let Some(node) = p.node {
                    if let Some((cs, _)) = node
                        .attr(NS_TABLE, "style-name")
                        .and_then(|s| styles.lookup(ctx.scope, Family::TableCell, s))
                    {
                        let (o, list) = convert_opaque_only(&cs.cell);
                        opaque = o;
                        self.unsupported(&cs.name, &list);
                    }
                    self.blocks(node, &inner)?;
                }
                let blocks = self.end();
                let blocks = self.ensure_paragraph(blocks)?;
                table_nodes.push((
                    cell_id,
                    Node::Cell(Cell {
                        col_span: p.cs,
                        row_span: p.rs,
                        blocks: atelier_text_model::rich::BlockList::from_ids(&blocks)
                            .map_err(model)?,
                        opaque,
                    }),
                ));
                cell_ids.push((p.col, cell_id));
            }
            cell_ids.sort_by_key(|c| c.0);
            let row_id = self.doc.alloc_id().map_err(model)?;
            table_nodes.push((
                row_id,
                Node::Row(Row {
                    cells: cell_ids.into_iter().map(|c| c.1).collect(),
                    opaque: Vec::new(),
                }),
            ));
            row_ids.push(row_id);
        }
        self.item = saved_item;
        let mut opaque = Vec::new();
        if let Some(ts) = tstyle {
            let (o, list) = convert_opaque_only(&ts.table);
            opaque = o;
            let name = ts.name.clone();
            self.unsupported(&name, &list);
        }
        if widths.iter().any(Option::is_some) {
            let list: Vec<String> = (0..columns)
                .map(|i| widths.get(i).copied().flatten().unwrap_or(0).to_string())
                .collect();
            insert_opaque(
                &mut opaque,
                synthetic_opaque("column-widths", list.join(",")),
            );
        }
        if header_rows > 0 {
            insert_opaque(
                &mut opaque,
                synthetic_opaque("header-rows", header_rows.to_string()),
            );
        }
        let table_id = self.doc.alloc_id().map_err(model)?;
        for (id, node) in table_nodes {
            self.nodes.insert(id, node);
        }
        self.nodes.insert(
            table_id,
            Node::Table(Table {
                columns,
                rows: row_ids,
                style: None,
                opaque,
            }),
        );
        self.push_block(table_id);
        Ok(())
    }

    /// A container needs at least one paragraph.
    fn ensure_paragraph(&mut self, mut blocks: Vec<NodeId>) -> Result<Vec<NodeId>, OdtError> {
        let has = blocks
            .iter()
            .any(|id| matches!(self.nodes.get(id), Some(Node::Paragraph(_))));
        if !has {
            let id = self.doc.alloc_id().map_err(model)?;
            self.nodes.insert(id, Node::Paragraph(Paragraph::default()));
            blocks.push(id);
        }
        Ok(blocks)
    }

    // ----- sections, headers and footers ------------------------------------

    pub fn finish(mut self) -> Result<Output, OdtError> {
        // Close the last section.
        let body = std::mem::take(&mut self.stack[0]);
        let last = FinishedSection {
            master: self.cur_master.clone(),
            body,
        };
        self.finished.push(last);
        let last_nodes = std::mem::take(&mut self.nodes);
        self.section_nodes.push(last_nodes);
        let finished = std::mem::take(&mut self.finished);
        let mut maps = std::mem::take(&mut self.section_nodes);
        let mut subtrees = Vec::new();
        let mut infos = Vec::new();
        for (acc, mut map) in finished.into_iter().zip(maps.drain(..)) {
            self.nodes = std::mem::take(&mut map);
            let body = self.ensure_paragraph(acc.body)?;
            let mut section = Section {
                body: atelier_text_model::rich::BlockList::from_ids(&body).map_err(model)?,
                ..Section::default()
            };
            let mut page = PageSetup::default();
            if let Some(name) = &acc.master {
                if let Some(master) = self.styles.masters.iter().find(|m| &m.name == name) {
                    page = page_setup(self.styles, master);
                    self.in_header = true;
                    self.attach_headers(master, &mut section)?;
                    self.in_header = false;
                }
            }
            for (k, v) in page.entries() {
                insert_opaque(&mut section.opaque, synthetic_opaque(k, v));
            }
            if let Some(name) = &acc.master {
                insert_opaque(
                    &mut section.opaque,
                    synthetic_opaque("master-page", name.clone()),
                );
            }
            let sid = self.doc.alloc_id().map_err(model)?;
            let mut nodes = std::mem::take(&mut self.nodes);
            nodes.insert(sid, Node::Section(section));
            subtrees.push(Subtree { root: sid, nodes });
            infos.push(ImportedSection {
                section: sid,
                master_page: acc.master,
                page,
            });
        }
        let mut ops = Vec::new();
        for (i, st) in subtrees.into_iter().enumerate() {
            ops.push(RichOp::InsertBlock {
                parent: None,
                position: i + 1,
                subtree: st,
            });
        }
        ops.push(RichOp::RemoveBlock {
            parent: None,
            position: 0,
        });
        let tx = Transaction::new(ops).map_err(model)?;
        self.doc.apply(&tx).map_err(model)?;
        self.doc.validate().map_err(model)?;
        // List definitions.
        let mut lists = Vec::new();
        for (id, style) in &self.list_defs {
            let levels = style
                .as_ref()
                .and_then(|s| self.styles.list(Scope::Content, s))
                .map(|l| {
                    l.levels
                        .iter()
                        .map(|lv| ImportedListLevel {
                            level: lv.level,
                            kind: match &lv.kind {
                                ListKind::Bullet(c) => ListLevelKind::Bullet(c.clone()),
                                ListKind::Number {
                                    format,
                                    prefix,
                                    suffix,
                                    start,
                                } => ListLevelKind::Number {
                                    format: format.clone(),
                                    prefix: prefix.clone(),
                                    suffix: suffix.clone(),
                                    start: *start,
                                },
                                ListKind::Image => ListLevelKind::Image,
                            },
                            space_before: lv.space_before,
                            min_label_width: lv.min_label_width,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if style.is_some()
                && self
                    .styles
                    .list(Scope::Content, style.as_deref().unwrap_or(""))
                    .is_none()
            {
                self.notices.add(ImportNotice::UnknownStyle {
                    family: "List".into(),
                    name: style.clone().unwrap_or_default(),
                });
            }
            lists.push(ImportedList {
                list_id: *id,
                style_name: style.clone(),
                levels,
            });
        }
        Ok(Output {
            document: self.doc,
            engine: self.engine,
            run_styles: self.run_styles,
            paragraph_order: self.para_order,
            sections: infos,
            lists,
            list_items: self.list_items,
            images: self.images,
            headings: self.headings,
            style_names: self.names,
            next_styles: self.next_styles,
        })
    }

    fn attach_headers(
        &mut self,
        master: &MasterPage,
        section: &mut Section,
    ) -> Result<(), OdtError> {
        let ctx = Ctx {
            scope: Scope::Styles,
            depth: 0,
            list: None,
        };
        for e in master.node.elements() {
            if e.ns != NS_STYLE {
                continue;
            }
            let (header, variant) = match e.local.as_str() {
                "header" => (true, HfVariant::Default),
                "header-first" => (true, HfVariant::First),
                "header-left" => (true, HfVariant::Even),
                "footer" => (false, HfVariant::Default),
                "footer-first" => (false, HfVariant::First),
                "footer-left" => (false, HfVariant::Even),
                _ => continue,
            };
            if e.attr(NS_STYLE, "display") == Some("false") {
                continue;
            }
            self.begin();
            self.blocks(e, &ctx)?;
            let ids = self.end();
            if ids.is_empty() {
                continue;
            }
            let hf = self.doc.alloc_id().map_err(model)?;
            self.nodes.insert(
                hf,
                Node::HeaderFooter(HeaderFooter {
                    blocks: atelier_text_model::rich::BlockList::from_ids(&ids).map_err(model)?,
                }),
            );
            let list = if header {
                &mut section.headers
            } else {
                &mut section.footers
            };
            if !list.iter().any(|(v, _)| *v == variant) {
                list.push((variant, hf));
            }
        }
        section.headers.sort_by_key(|h| h.0);
        section.footers.sort_by_key(|h| h.0);
        Ok(())
    }
}

/// The parts of a [`ParaInfo`] that `finish_paragraph` needs, detached from `Rc`.
struct ParaInfoRef {
    style: Option<String>,
    direct: ParaProps,
    base_char: CharAttrs,
    list_style: Option<String>,
}
impl ParaInfoRef {
    fn none() -> Self {
        Self {
            style: None,
            direct: ParaProps::default(),
            base_char: CharAttrs::default(),
            list_style: None,
        }
    }
    fn of(i: &Rc<ParaInfo>) -> Self {
        Self {
            style: i.style.clone(),
            direct: i.direct.clone(),
            base_char: i.base_char.clone(),
            list_style: i.list_style.clone(),
        }
    }
}

struct ImageMeta {
    anchor: ImageAnchor,
    width: Option<i32>,
    height: Option<i32>,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
}

struct ImageRecord {
    paragraph: NodeId,
    object: NodeId,
    href: Option<String>,
    media_type: Option<String>,
    status: ImageStatus,
    meta: ImageMeta,
}

impl From<ImageRecord> for ImportedImage {
    fn from(r: ImageRecord) -> Self {
        ImportedImage {
            paragraph: r.paragraph,
            object: r.object,
            href: r.href,
            media_type: r.media_type,
            status: r.status,
            anchor: r.meta.anchor,
            width: r.meta.width,
            height: r.meta.height,
            name: r.meta.name,
            title: r.meta.title,
            description: r.meta.description,
        }
    }
}

#[derive(Clone)]
struct CellSpec<'x> {
    node: &'x XNode,
    covered: bool,
    cs: usize,
    rs: usize,
}

struct RowSpec<'x> {
    cells: Vec<CellSpec<'x>>,
}

struct Placed<'x> {
    row: usize,
    col: usize,
    cs: usize,
    rs: usize,
    node: Option<&'x XNode>,
}

/// Parse a non-negative count; an all-digit value too large for `usize`
/// saturates so that the usual limits reject it. Anything else is `None`.
fn parse_count(v: &str) -> Option<usize> {
    let v = v.trim();
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(v.parse::<usize>().unwrap_or(usize::MAX))
}

fn repeat_of(n: &XNode, attr: &str) -> usize {
    n.attr(NS_TABLE, attr)
        .and_then(parse_count)
        .unwrap_or(1)
        .max(1)
}

fn span_of(n: &XNode, attr: &str) -> usize {
    n.attr(NS_TABLE, attr)
        .and_then(parse_count)
        .unwrap_or(1)
        .max(1)
}

fn collect_columns<'x>(t: &'x XNode, f: &mut dyn FnMut(&'x XNode)) {
    for c in t.elements() {
        if c.ns != NS_TABLE {
            continue;
        }
        match c.local.as_str() {
            "table-column" => f(c),
            "table-columns" | "table-header-columns" | "table-column-group" => {
                collect_columns(c, f)
            }
            _ => {}
        }
    }
}

fn collect_rows<'x>(t: &'x XNode, header: bool, f: &mut dyn FnMut(&'x XNode, bool)) {
    for c in t.elements() {
        if c.ns != NS_TABLE {
            continue;
        }
        match c.local.as_str() {
            "table-row" => f(c, header),
            "table-header-rows" => collect_rows(c, true, f),
            "table-rows" | "table-row-group" => collect_rows(c, header, f),
            _ => {}
        }
    }
}

fn page_setup(styles: &Styles, master: &MasterPage) -> PageSetup {
    let mut p = PageSetup::default();
    let Some(layout) = master.layout.as_ref().and_then(|n| styles.layouts.get(n)) else {
        return p;
    };
    let get = |k: &str| layout.props.get(k).and_then(|v| length_twips(v));
    p.width = get("fo:page-width");
    p.height = get("fo:page-height");
    p.landscape = layout
        .props
        .get("style:print-orientation")
        .map(|o| o == "landscape")
        .or_else(|| p.width.zip(p.height).map(|(w, h)| w > h));
    p.margin_top = get("fo:margin-top");
    p.margin_bottom = get("fo:margin-bottom");
    p.margin_left = get("fo:margin-left");
    p.margin_right = get("fo:margin-right");
    p
}

pub(super) fn sniff_media_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else if b.starts_with(b"<svg")
        || (b.starts_with(b"<?xml") && b.windows(4).take(512).any(|w| w == b"<svg"))
    {
        Some("image/svg+xml")
    } else if b.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) {
        Some("image/x-wmf")
    } else if b.len() > 44 && &b[40..44] == b" EMF" {
        Some("image/x-emf")
    } else {
        None
    }
}

/// Standard base64 decoder with a size bound; white space is skipped.
pub(super) fn base64_decode(s: &str, max: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
            if out.len() > max {
                return None;
            }
        }
    }
    Some(out)
}
