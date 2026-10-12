// SPDX-License-Identifier: AGPL-3.0-or-later
//! ODF style sheet: common and automatic styles, font declarations, list
//! styles, page layouts and master pages; plus conversion of ODF properties to
//! the rich model's character and paragraph properties.

use super::tree::*;
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, LineRule, LineSpacing, OpaqueAttr, ParaProps, Underline,
    VerticalAlign,
};
use std::collections::{BTreeMap, HashMap};

/// Namespace of attributes synthesized by the importer (not ODF names).
pub const NS_IMPORT: &str = "urn:atelier:odf-import";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Family {
    Paragraph,
    Text,
    Table,
    TableColumn,
    TableRow,
    TableCell,
    Graphic,
    Section,
    Other,
}

impl Family {
    pub fn parse(s: &str) -> Self {
        match s {
            "paragraph" => Self::Paragraph,
            "text" => Self::Text,
            "table" => Self::Table,
            "table-column" => Self::TableColumn,
            "table-row" => Self::TableRow,
            "table-cell" => Self::TableCell,
            "graphic" => Self::Graphic,
            "section" => Self::Section,
            _ => Self::Other,
        }
    }
}

/// Property maps keyed by `prefix:local` with the prefixes of [`canon`].
pub(crate) type Props = BTreeMap<String, String>;

pub(crate) fn canon(ns: &str, local: &str) -> String {
    let p = match ns {
        NS_FO => "fo",
        NS_STYLE => "style",
        NS_SVG => "svg",
        NS_TEXT => "text",
        NS_TABLE => "table",
        NS_DRAW => "draw",
        NS_XLINK => "xlink",
        other => return format!("{{{other}}}{local}"),
    };
    format!("{p}:{local}")
}

pub(crate) fn expand(key: &str) -> (String, String) {
    if let Some((p, l)) = key.split_once(':') {
        let ns = match p {
            "fo" => NS_FO,
            "style" => NS_STYLE,
            "svg" => NS_SVG,
            "text" => NS_TEXT,
            "table" => NS_TABLE,
            "draw" => NS_DRAW,
            "xlink" => NS_XLINK,
            _ => "",
        };
        if !ns.is_empty() {
            return (ns.to_owned(), l.to_owned());
        }
    }
    if let Some(rest) = key.strip_prefix('{') {
        if let Some((ns, l)) = rest.split_once('}') {
            return (ns.to_owned(), l.to_owned());
        }
    }
    (NS_IMPORT.to_owned(), key.to_owned())
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OdfStyle {
    pub name: String,
    pub display_name: Option<String>,
    pub parent: Option<String>,
    pub next: Option<String>,
    pub list_style: Option<String>,
    pub master_page: Option<String>,
    pub text: Props,
    pub para: Props,
    pub cell: Props,
    pub column: Props,
    pub table: Props,
}

#[derive(Clone, Debug)]
pub(crate) enum ListKind {
    Bullet(String),
    Number {
        format: String,
        prefix: String,
        suffix: String,
        start: u32,
    },
    Image,
}

#[derive(Clone, Debug)]
pub(crate) struct ListLevel {
    pub level: u8,
    pub kind: ListKind,
    pub space_before: Option<i32>,
    pub min_label_width: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ListStyle {
    pub levels: Vec<ListLevel>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PageLayout {
    pub props: Props,
}

#[derive(Clone, Debug)]
pub(crate) struct MasterPage {
    pub name: String,
    pub layout: Option<String>,
    pub node: XNode,
}

/// Index into the two automatic-style scopes (content.xml / styles.xml).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scope {
    Content = 0,
    Styles = 1,
}

#[derive(Default)]
pub(crate) struct Styles {
    pub common: HashMap<(Family, String), OdfStyle>,
    /// Document order of common styles (for deterministic catalog creation).
    pub common_order: Vec<(Family, String)>,
    pub auto: [HashMap<(Family, String), OdfStyle>; 2],
    pub default_para: Option<OdfStyle>,
    pub fonts: HashMap<String, String>,
    pub common_lists: HashMap<String, ListStyle>,
    pub auto_lists: [HashMap<String, ListStyle>; 2],
    pub layouts: HashMap<String, PageLayout>,
    pub masters: Vec<MasterPage>,
}

impl Styles {
    pub fn lookup(&self, scope: Scope, family: Family, name: &str) -> Option<(&OdfStyle, bool)> {
        let key = (family, name.to_owned());
        if let Some(s) = self.auto[scope as usize].get(&key) {
            return Some((s, true));
        }
        self.common.get(&key).map(|s| (s, false))
    }

    /// Default font size (half-points) from the paragraph default style.
    pub fn default_size(&self) -> u32 {
        self.default_para
            .as_ref()
            .and_then(|d| d.text.get("fo:font-size"))
            .and_then(|v| font_size(v))
            .map(|s| resolve_size(s, 24))
            .unwrap_or(24)
    }

    /// Effective font size of a style after following its parents (bounded).
    pub fn effective_size(&self, scope: Scope, family: Family, name: &str) -> u32 {
        let mut chain: Vec<&OdfStyle> = Vec::new();
        let mut cur = Some(name.to_owned());
        while let Some(c) = cur.take() {
            if chain.len() >= 64 {
                break;
            }
            match self.lookup(scope, family, &c) {
                Some((s, _)) => {
                    chain.push(s);
                    cur = s.parent.clone();
                }
                None => break,
            }
        }
        let mut size = self.default_size();
        for s in chain.iter().rev() {
            if let Some(sz) = s.text.get("fo:font-size").and_then(|v| font_size(v)) {
                size = resolve_size(sz, size);
            }
        }
        size
    }

    pub fn list(&self, scope: Scope, name: &str) -> Option<&ListStyle> {
        self.auto_lists[scope as usize]
            .get(name)
            .or_else(|| self.common_lists.get(name))
    }

    /// Absorb the font declarations, common styles, automatic styles and master
    /// pages found under `root` (a document-styles, document-content or flat
    /// document element). `scope` selects the automatic-style namespace.
    pub fn absorb(
        &mut self,
        root: &XNode,
        scope: Scope,
        max_styles: usize,
    ) -> Result<(), &'static str> {
        for section in root.elements() {
            if section.ns != NS_OFFICE {
                continue;
            }
            match section.local.as_str() {
                "font-face-decls" => {
                    for face in section.elements().filter(|e| e.is(NS_STYLE, "font-face")) {
                        let name = face.attr(NS_STYLE, "name");
                        let family = face.attr(NS_SVG, "font-family");
                        if let (Some(n), Some(f)) = (name, family) {
                            self.fonts.entry(n.to_owned()).or_insert_with(|| unquote(f));
                        }
                    }
                }
                "styles" => self.absorb_styles(section, None, max_styles)?,
                "automatic-styles" => self.absorb_styles(section, Some(scope), max_styles)?,
                "master-styles" => {
                    for m in section.elements().filter(|e| e.is(NS_STYLE, "master-page")) {
                        if let Some(name) = m.attr(NS_STYLE, "name") {
                            self.masters.push(MasterPage {
                                name: name.to_owned(),
                                layout: m.attr(NS_STYLE, "page-layout-name").map(str::to_owned),
                                node: m.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn absorb_styles(
        &mut self,
        container: &XNode,
        auto: Option<Scope>,
        max_styles: usize,
    ) -> Result<(), &'static str> {
        for e in container.elements() {
            match (e.ns.as_str(), e.local.as_str()) {
                (NS_STYLE, "style") | (NS_STYLE, "default-style") => {
                    let style = parse_style(e);
                    let default = e.local == "default-style";
                    let family = Family::parse(e.attr(NS_STYLE, "family").unwrap_or(""));
                    if default {
                        if family == Family::Paragraph {
                            self.default_para = Some(style);
                        }
                        continue;
                    }
                    if style.name.is_empty() {
                        continue;
                    }
                    let key = (family, style.name.clone());
                    match auto {
                        Some(scope) => {
                            let map = &mut self.auto[scope as usize];
                            if map.len() >= max_styles {
                                return Err("style count");
                            }
                            map.insert(key, style);
                        }
                        None => {
                            if self.common.len() >= max_styles {
                                return Err("style count");
                            }
                            if self.common.insert(key.clone(), style).is_none() {
                                self.common_order.push(key);
                            }
                        }
                    }
                }
                (NS_TEXT, "list-style") => {
                    if let Some(name) = e.attr(NS_STYLE, "name") {
                        let list = parse_list_style(e);
                        match auto {
                            Some(scope) => {
                                self.auto_lists[scope as usize].insert(name.to_owned(), list)
                            }
                            None => self.common_lists.insert(name.to_owned(), list),
                        };
                    }
                }
                (NS_STYLE, "page-layout") => {
                    if let Some(name) = e.attr(NS_STYLE, "name") {
                        let mut props = Props::new();
                        if let Some(p) = e.find(NS_STYLE, "page-layout-properties") {
                            for a in &p.attrs {
                                props.insert(canon(&a.ns, &a.local), a.value.clone());
                            }
                        }
                        self.layouts.insert(name.to_owned(), PageLayout { props });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix('\'')
        .and_then(|x| x.strip_suffix('\''))
        .unwrap_or(t);
    let t = t
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(t);
    t.to_owned()
}

fn collect_props(node: &XNode, into: &mut Props) {
    for a in &node.attrs {
        into.insert(canon(&a.ns, &a.local), a.value.clone());
    }
}

fn parse_style(e: &XNode) -> OdfStyle {
    let mut s = OdfStyle {
        name: e.attr(NS_STYLE, "name").unwrap_or("").to_owned(),
        display_name: e.attr(NS_STYLE, "display-name").map(str::to_owned),
        parent: e.attr(NS_STYLE, "parent-style-name").map(str::to_owned),
        next: e.attr(NS_STYLE, "next-style-name").map(str::to_owned),
        list_style: e
            .attr(NS_STYLE, "list-style-name")
            .filter(|n| !n.is_empty())
            .map(str::to_owned),
        master_page: e
            .attr(NS_STYLE, "master-page-name")
            .filter(|m| !m.is_empty())
            .map(str::to_owned),
        ..OdfStyle::default()
    };
    for c in e.elements() {
        if c.ns != NS_STYLE {
            continue;
        }
        match c.local.as_str() {
            "text-properties" => {
                // Some producers put paragraph properties in text-properties
                // (invalid but common in templates): route them to the paragraph.
                let mut all = Props::new();
                collect_props(c, &mut all);
                for (k, v) in all {
                    if matches!(
                        k.as_str(),
                        "fo:text-align" | "fo:line-height" | "fo:text-indent"
                    ) {
                        s.para.entry(k).or_insert(v);
                    } else {
                        s.text.insert(k, v);
                    }
                }
            }
            "paragraph-properties" => {
                collect_props(c, &mut s.para);
                if let Some(stops) = c.find(NS_STYLE, "tab-stops") {
                    let list: Vec<String> = stops
                        .elements()
                        .filter(|t| t.is(NS_STYLE, "tab-stop"))
                        .map(|t| {
                            format!(
                                "{}:{}:{}",
                                t.attr(NS_STYLE, "position")
                                    .and_then(length_twips)
                                    .unwrap_or(0),
                                t.attr(NS_STYLE, "type").unwrap_or("left"),
                                t.attr(NS_STYLE, "leader-text")
                                    .or_else(|| t.attr(NS_STYLE, "leader-style"))
                                    .unwrap_or("")
                            )
                        })
                        .collect();
                    if !list.is_empty() {
                        s.para.insert("style:tab-stops".into(), list.join(";"));
                    }
                }
            }
            "table-cell-properties" => collect_props(c, &mut s.cell),
            "table-column-properties" => collect_props(c, &mut s.column),
            "table-properties" => collect_props(c, &mut s.table),
            _ => {}
        }
    }
    s
}

fn parse_list_style(e: &XNode) -> ListStyle {
    let mut levels = Vec::new();
    for l in e.elements() {
        if l.ns != NS_TEXT {
            continue;
        }
        let kind = match l.local.as_str() {
            "list-level-style-bullet" => {
                ListKind::Bullet(l.attr(NS_TEXT, "bullet-char").unwrap_or("").to_owned())
            }
            "list-level-style-number" => ListKind::Number {
                format: l.attr(NS_STYLE, "num-format").unwrap_or("").to_owned(),
                prefix: l.attr(NS_STYLE, "num-prefix").unwrap_or("").to_owned(),
                suffix: l.attr(NS_STYLE, "num-suffix").unwrap_or("").to_owned(),
                start: l
                    .attr(NS_TEXT, "start-value")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(1),
            },
            "list-level-style-image" => ListKind::Image,
            _ => continue,
        };
        let level = l
            .attr(NS_TEXT, "level")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(1)
            .clamp(1, 10) as u8
            - 1;
        let props = l.find(NS_STYLE, "list-level-properties");
        let space_before = props
            .and_then(|p| p.attr(NS_TEXT, "space-before"))
            .and_then(length_twips);
        let min_label_width = props
            .and_then(|p| p.attr(NS_TEXT, "min-label-width"))
            .and_then(length_twips);
        levels.push(ListLevel {
            level,
            kind,
            space_before,
            min_label_width,
        });
    }
    ListStyle { levels }
}

// ---------------------------------------------------------------------------
// Units and values

/// Parse an ODF length into twips. Percentages and font-relative units are
/// rejected.
pub(crate) fn length_twips(s: &str) -> Option<i32> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value: f64 = num.parse().ok()?;
    let per_unit = match unit.trim() {
        "cm" => 1440.0 / 2.54,
        "mm" => 1440.0 / 25.4,
        "in" => 1440.0,
        "pt" => 20.0,
        "pc" => 240.0,
        "px" => 15.0,
        "" => return None,
        _ => return None,
    };
    let twips = value * per_unit;
    if !twips.is_finite() || twips.abs() > f64::from(i32::MAX / 4) {
        return None;
    }
    Some(twips.round() as i32)
}

pub(crate) fn percent(s: &str) -> Option<f64> {
    let v: f64 = s.trim().strip_suffix('%')?.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

pub(crate) fn color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    })
}

/// A font size: absolute half-points or a percentage of the parent size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Size {
    Abs(u32),
    Rel(f64),
}

pub(crate) fn font_size(s: &str) -> Option<Size> {
    if let Some(p) = percent(s) {
        return (p > 0.0).then_some(Size::Rel(p));
    }
    let tw = length_twips(s)?;
    (tw > 0).then(|| Size::Abs(((tw as f64) / 10.0).round().max(1.0) as u32))
}

pub(crate) fn resolve_size(size: Size, base_half_points: u32) -> u32 {
    match size {
        Size::Abs(v) => v,
        Size::Rel(p) => ((f64::from(base_half_points) * p / 100.0).round() as u32).max(1),
    }
}

fn opaque_sorted(mut list: Vec<OpaqueAttr>) -> Vec<OpaqueAttr> {
    list.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    list.dedup_by(|a, b| a.namespace == b.namespace && a.name == b.name);
    list
}

pub(crate) fn opaque_from(rest: &[(String, String)]) -> Vec<OpaqueAttr> {
    opaque_sorted(
        rest.iter()
            .map(|(k, v)| {
                let (namespace, name) = expand(k);
                OpaqueAttr {
                    namespace,
                    name,
                    value: v.clone().into_bytes(),
                }
            })
            .collect(),
    )
}

pub(crate) fn synthetic_opaque(name: &str, value: String) -> OpaqueAttr {
    OpaqueAttr {
        namespace: NS_IMPORT.to_owned(),
        name: name.to_owned(),
        value: value.into_bytes(),
    }
}

pub(crate) fn insert_opaque(list: &mut Vec<OpaqueAttr>, attr: OpaqueAttr) {
    list.push(attr);
    let sorted = opaque_sorted(std::mem::take(list));
    *list = sorted;
}

/// Kept as opaque attributes but not worth a notice: typographic hints with no
/// visible effect on the text model, and producer extensions.
fn is_silent(key: &str) -> bool {
    key.starts_with('{')
        || key.starts_with("fo:hyphenat")
        || matches!(
            key,
            "style:line-break"
                | "style:punctuation-wrap"
                | "style:text-autospace"
                | "text:line-number"
                | "text:number-lines"
                | "style:contextual-spacing"
                | "style:auto-text-indent"
                | "style:register-true"
                | "style:snap-to-layout-grid"
                | "style:justify-single-word"
                | "style:page-number"
                | "fo:country"
                | "fo:language"
                | "style:use-window-font-color"
                | "style:text-underline-color"
                | "style:text-line-through-color"
        )
}

fn is_noise(key: &str) -> bool {
    key.ends_with("-asian")
        || key.ends_with("-complex")
        || matches!(
            key,
            "style:font-family-generic"
                | "style:font-pitch"
                | "style:font-charset"
                | "style:font-style-name"
                | "style:letter-kerning"
                | "style:rfc-language-tag"
                | "fo:hyphenate"
                | "fo:hyphenation-remain-char-count"
                | "fo:hyphenation-push-char-count"
                | "style:script-type"
                | "style:text-underline-width"
                | "style:text-underline-type"
                | "style:text-line-through-type"
                | "style:text-line-through-width"
        )
}

/// Result of converting one `style:text-properties`.
pub(crate) struct CharConv {
    pub attrs: CharAttrs,
    pub size: Option<Size>,
    /// Properties read without a model slot (also kept as opaque attributes).
    pub unsupported: Vec<(String, String)>,
}

pub(crate) fn convert_text(p: &Props, fonts: &HashMap<String, String>) -> CharConv {
    let mut rem: Props = p.clone();
    let mut a = CharAttrs::default();
    if let Some(v) = rem.remove("fo:font-weight") {
        a.bold = match v.trim() {
            "bold" => Some(true),
            "normal" => Some(false),
            n => n.parse::<u32>().ok().map(|w| w >= 600),
        };
    }
    if let Some(v) = rem.remove("fo:font-style") {
        a.italic = match v.trim() {
            "italic" | "oblique" => Some(true),
            "normal" => Some(false),
            _ => None,
        };
    }
    if let Some(v) = rem.remove("style:text-underline-style") {
        let double = p
            .get("style:text-underline-type")
            .is_some_and(|t| t == "double");
        let thick = p
            .get("style:text-underline-width")
            .is_some_and(|w| w == "bold" || w == "thick");
        a.underline = match v.trim() {
            "none" => Some(Underline::None),
            "solid" if double => Some(Underline::Double),
            "solid" if thick => Some(Underline::Thick),
            "solid" => Some(Underline::Single),
            "dotted" | "dot-dash" | "dot-dot-dash" => Some(Underline::Dotted),
            "dash" | "long-dash" => Some(Underline::Dashed),
            "wave" => Some(Underline::Wave),
            _ => None,
        };
    }
    if let Some(v) = rem.remove("style:text-line-through-style") {
        a.strike = Some(v.trim() != "none");
    }
    if let Some(v) = rem.remove("style:text-position") {
        let first = v.split_whitespace().next().unwrap_or("");
        a.vertical_align = match first {
            "super" => Some(VerticalAlign::Superscript),
            "sub" => Some(VerticalAlign::Subscript),
            _ => percent(first).map(|n| {
                if n > 0.0 {
                    VerticalAlign::Superscript
                } else if n < 0.0 {
                    VerticalAlign::Subscript
                } else {
                    VerticalAlign::Baseline
                }
            }),
        };
    }
    let font_name = rem.remove("style:font-name");
    let family = rem.remove("fo:font-family");
    a.font = font_name
        .as_deref()
        .map(|n| fonts.get(n).cloned().unwrap_or_else(|| n.to_owned()))
        .or_else(|| family.as_deref().map(unquote))
        .filter(|f| !f.is_empty());
    let size = rem.remove("fo:font-size").and_then(|v| font_size(&v));
    if let Some(v) = rem.get("fo:color").and_then(|v| color(v)) {
        a.color = Some(v);
        rem.remove("fo:color");
    }
    if let Some(v) = rem.get("fo:background-color").and_then(|v| color(v)) {
        a.highlight = Some(v);
        rem.remove("fo:background-color");
    }
    let kept: Vec<(String, String)> = rem.into_iter().filter(|(k, _)| !is_noise(k)).collect();
    a.opaque = opaque_from(&kept);
    CharConv {
        attrs: a,
        size,
        unsupported: kept.into_iter().filter(|(k, _)| !is_silent(k)).collect(),
    }
}

pub(crate) struct ParaConv {
    pub props: ParaProps,
    pub unsupported: Vec<(String, String)>,
}

pub(crate) fn convert_para(p: &Props) -> ParaConv {
    let mut rem: Props = p.clone();
    let mut pp = ParaProps::default();
    let rtl = rem
        .get("style:writing-mode")
        .is_some_and(|w| w.starts_with("rl"));
    rem.remove("style:writing-mode");
    if let Some(v) = rem.remove("fo:text-align") {
        pp.alignment = match (v.trim(), rtl) {
            ("left", _) | ("start", false) | ("end", true) => Some(Alignment::Left),
            ("right", _) | ("end", false) | ("start", true) => Some(Alignment::Right),
            ("center", _) => Some(Alignment::Center),
            ("justify", _) => Some(Alignment::Justify),
            _ => None,
        };
    }
    let take_len = |rem: &mut Props, key: &str| -> Option<i32> {
        let v = rem.get(key).and_then(|v| length_twips(v))?;
        rem.remove(key);
        Some(v)
    };
    if let Some(all) = take_len(&mut rem, "fo:margin") {
        pp.indent_left = Some(all);
        pp.indent_right = Some(all);
        pp.space_before = Some(all.max(0) as u32);
        pp.space_after = Some(all.max(0) as u32);
    }
    if let Some(v) = take_len(&mut rem, "fo:margin-left") {
        pp.indent_left = Some(v);
    }
    if let Some(v) = take_len(&mut rem, "fo:margin-right") {
        pp.indent_right = Some(v);
    }
    if let Some(v) = take_len(&mut rem, "fo:margin-top") {
        pp.space_before = Some(v.max(0) as u32);
    }
    if let Some(v) = take_len(&mut rem, "fo:margin-bottom") {
        pp.space_after = Some(v.max(0) as u32);
    }
    if let Some(v) = take_len(&mut rem, "fo:text-indent") {
        pp.indent_first_line = Some(v);
    }
    if let Some(v) = rem.get("fo:line-height").cloned() {
        let v = v.trim().to_owned();
        let spacing = if v == "normal" {
            Some(LineSpacing {
                rule: LineRule::Auto,
                value: 240,
            })
        } else if let Some(pc) = percent(&v) {
            (pc > 0.0).then(|| LineSpacing {
                rule: LineRule::Auto,
                value: (pc * 2.4).round().min(f64::from(u32::MAX / 2)) as u32,
            })
        } else {
            length_twips(&v).filter(|n| *n > 0).map(|n| LineSpacing {
                rule: LineRule::Exact,
                value: n as u32,
            })
        };
        if spacing.is_some() {
            rem.remove("fo:line-height");
        }
        pp.line_spacing = spacing;
    }
    if let Some(v) = rem.get("style:line-height-at-least").cloned() {
        if let Some(n) = length_twips(&v).filter(|n| *n > 0) {
            pp.line_spacing = Some(LineSpacing {
                rule: LineRule::AtLeast,
                value: n as u32,
            });
            rem.remove("style:line-height-at-least");
        }
    }
    let kept: Vec<(String, String)> = rem.into_iter().filter(|(k, _)| !is_noise(k)).collect();
    pp.opaque = opaque_from(&kept);
    ParaConv {
        props: pp,
        unsupported: kept.into_iter().filter(|(k, _)| !is_silent(k)).collect(),
    }
}

/// Convert non-property maps (cell, table) wholesale to opaque attributes.
pub(crate) fn convert_opaque_only(p: &Props) -> (Vec<OpaqueAttr>, Vec<(String, String)>) {
    let list: Vec<(String, String)> = p
        .iter()
        .filter(|(k, _)| !is_noise(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let opaque = opaque_from(&list);
    (
        opaque,
        list.into_iter().filter(|(k, _)| !is_silent(k)).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_convert_to_twips() {
        assert_eq!(length_twips("1in"), Some(1440));
        assert_eq!(length_twips("2.54cm"), Some(1440));
        assert_eq!(length_twips("25.4mm"), Some(1440));
        assert_eq!(length_twips("12pt"), Some(240));
        assert_eq!(length_twips("1pc"), Some(240));
        assert_eq!(length_twips("1px"), Some(15));
        assert_eq!(length_twips("-0.5cm"), Some(-283));
        assert_eq!(length_twips("50%"), None);
        assert_eq!(length_twips("1e999cm"), None);
        assert_eq!(length_twips("cm"), None);
    }

    #[test]
    fn sizes_and_colors() {
        assert_eq!(font_size("12pt"), Some(Size::Abs(24)));
        assert_eq!(resolve_size(Size::Rel(150.0), 24), 36);
        assert_eq!(
            color("#ff8000"),
            Some(Color {
                r: 255,
                g: 128,
                b: 0
            })
        );
        assert_eq!(color("transparent"), None);
    }
}
