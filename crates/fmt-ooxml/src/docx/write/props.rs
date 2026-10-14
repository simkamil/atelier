// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property elements (`w:rPr`, `w:pPr`) of the DOCX writer: elements built from
//! the model, and their merge with the source skeleton so that unchanged
//! properties keep their source form and position (US-306 ordering rules).

use super::super::preserve::{PreservedChild, PreservedElement};
use super::super::{parse_highlight, ppr_rank, rpr_rank};
use atelier_text_model::rich::{
    Alignment, CharAttrs, Color, LineRule, ParaProps, ThemeColor, ThemeFont, Underline,
    VerticalAlign,
};

/// An element built by the writer: qualified name, qualified attribute names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct GenEl {
    pub name: String,
    pub attrs: Vec<(String, String)>,
}

/// One child of a merged property element.
pub(super) enum Item<'a> {
    Skel(&'a PreservedChild),
    Gen(GenEl),
    Raw(Vec<u8>),
}

/// The prefix bound to the WordprocessingML namespace in the part being written.
#[derive(Clone, Debug)]
pub(super) struct Prefixes {
    pub w: String,
    pub r: String,
}

impl Prefixes {
    pub fn el(&self, local: &str, attrs: &[(&str, String)]) -> GenEl {
        GenEl {
            name: format!("{}:{local}", self.w),
            attrs: attrs
                .iter()
                .map(|(k, v)| (format!("{}:{k}", self.w), v.clone()))
                .collect(),
        }
    }
}

pub(super) fn local_of(qualified: &str) -> &str {
    qualified.rsplit(':').next().unwrap_or(qualified)
}

/// Local name of the root element of serialized XML (`<w:foo ...`).
pub(super) fn local_of_bytes(xml: &[u8]) -> &str {
    if xml.first() != Some(&b'<') {
        return "";
    }
    let end = xml[1..]
        .iter()
        .position(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        .map_or(xml.len(), |p| p + 1);
    let name = std::str::from_utf8(&xml[1..end]).unwrap_or("");
    local_of(name)
}

fn child_local(c: &PreservedChild) -> &str {
    match c {
        PreservedChild::Element(e) => local_of(&e.name),
        PreservedChild::Opaque(b) => local_of_bytes(b),
        PreservedChild::Block(_) | PreservedChild::Anchored(_) => "",
    }
}

fn item_local<'a>(i: &'a Item<'_>) -> &'a str {
    match i {
        Item::Skel(c) => child_local(c),
        Item::Gen(g) => local_of(&g.name),
        Item::Raw(b) => local_of_bytes(b),
    }
}

/// Inserts `item` so the sequence keeps the schema order given by `rank`:
/// after the last item whose rank is not greater.
pub(super) fn insert_ranked<'a>(
    items: &mut Vec<Item<'a>>,
    item: Item<'a>,
    rank: fn(&str) -> usize,
) {
    let r = rank(item_local(&item));
    let at = items
        .iter()
        .rposition(|i| rank(item_local(i)) <= r)
        .map_or(0, |p| p + 1);
    items.insert(at, item);
}

// ---------------------------------------------------------------------------
// Character properties
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CharFam {
    Bold,
    Italic,
    Strike,
    Underline,
    VAlign,
    Font,
    Size,
    SizeCs,
    Color,
    Highlight,
}

pub(super) const CHAR_FAMILIES: [CharFam; 10] = [
    CharFam::Font,
    CharFam::Bold,
    CharFam::Italic,
    CharFam::Strike,
    CharFam::Color,
    CharFam::Size,
    CharFam::SizeCs,
    CharFam::Highlight,
    CharFam::Underline,
    CharFam::VAlign,
];

impl CharFam {
    fn owns(self) -> &'static [&'static str] {
        match self {
            Self::Bold => &["b"],
            Self::Italic => &["i"],
            Self::Strike => &["strike", "dstrike"],
            Self::Underline => &["u"],
            Self::VAlign => &["vertAlign"],
            Self::Font => &["rFonts"],
            Self::Size => &["sz"],
            Self::SizeCs => &["szCs"],
            Self::Color => &["color"],
            Self::Highlight => &["highlight"],
        }
    }
}

fn opaque_value<'a>(a: &'a CharAttrs, name: &str) -> Option<&'a [u8]> {
    a.opaque
        .iter()
        .find(|o| o.name == name)
        .map(|o| o.value.as_slice())
}

/// Whether the two attribute sets agree on a family.
pub(super) fn char_fam_eq(fam: CharFam, a: &CharAttrs, b: &CharAttrs) -> bool {
    match fam {
        CharFam::Bold => a.bold == b.bold,
        CharFam::Italic => a.italic == b.italic,
        CharFam::Strike => a.strike == b.strike,
        CharFam::Underline => a.underline == b.underline,
        CharFam::VAlign => a.vertical_align == b.vertical_align,
        CharFam::Font => a.font == b.font,
        CharFam::Size => a.size_half_points == b.size_half_points,
        CharFam::SizeCs => opaque_value(a, "szCs") == opaque_value(b, "szCs"),
        CharFam::Color => a.color == b.color && opaque_value(a, "color") == opaque_value(b, "color"),
        CharFam::Highlight => {
            a.highlight == b.highlight && opaque_value(a, "highlight") == opaque_value(b, "highlight")
        }
    }
}

pub(super) fn hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

const HIGHLIGHTS: [&str; 16] = [
    "black",
    "blue",
    "cyan",
    "green",
    "magenta",
    "red",
    "yellow",
    "white",
    "darkBlue",
    "darkCyan",
    "darkGreen",
    "darkMagenta",
    "darkRed",
    "darkYellow",
    "darkGray",
    "lightGray",
];

/// The WordprocessingML highlight name closest to `c`, and whether it is exact.
pub(super) fn highlight_name(c: Color) -> (&'static str, bool) {
    let mut best = ("yellow", u32::MAX);
    for name in HIGHLIGHTS {
        if let Some(h) = parse_highlight(name) {
            let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
            let dist = d(h.r, c.r) + d(h.g, c.g) + d(h.b, c.b);
            if dist < best.1 {
                best = (name, dist);
            }
        }
    }
    (best.0, best.1 == 0)
}

fn underline_name(u: Underline) -> &'static str {
    match u {
        Underline::None => "none",
        Underline::Single => "single",
        Underline::Double => "double",
        Underline::Thick => "thick",
        Underline::Dotted => "dotted",
        Underline::Dashed => "dash",
        Underline::Wave => "wave",
    }
}

fn on_off(p: &Prefixes, local: &str, value: bool) -> GenEl {
    if value {
        p.el(local, &[])
    } else {
        p.el(local, &[("val", "0".into())])
    }
}

pub(super) fn theme_font_name(f: ThemeFont) -> &'static str {
    match f {
        ThemeFont::Major => "majorHAnsi",
        ThemeFont::Minor => "minorHAnsi",
    }
}

pub(super) fn theme_color_name(c: ThemeColor) -> &'static str {
    match c {
        ThemeColor::Dark1 => "dark1",
        ThemeColor::Light1 => "light1",
        ThemeColor::Dark2 => "dark2",
        ThemeColor::Light2 => "light2",
        ThemeColor::Accent1 => "accent1",
        ThemeColor::Accent2 => "accent2",
        ThemeColor::Accent3 => "accent3",
        ThemeColor::Accent4 => "accent4",
        ThemeColor::Accent5 => "accent5",
        ThemeColor::Accent6 => "accent6",
        ThemeColor::Hyperlink => "hyperlink",
        ThemeColor::FollowedHyperlink => "followedHyperlink",
    }
}

/// Theme references of a style (`StyleCharAttrs`), when writing a style.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ThemeRefs {
    pub font: Option<ThemeFont>,
    pub color: Option<ThemeColor>,
}

/// Elements for one family. `lossy` receives the highlight colors that had to
/// be approximated and the opaque attributes that cannot be written.
pub(super) fn char_fam_elems(
    fam: CharFam,
    a: &CharAttrs,
    theme: ThemeRefs,
    p: &Prefixes,
    approximated: &mut Vec<Color>,
) -> Vec<GenEl> {
    let mut out = Vec::new();
    match fam {
        CharFam::Bold => out.extend(a.bold.map(|v| on_off(p, "b", v))),
        CharFam::Italic => out.extend(a.italic.map(|v| on_off(p, "i", v))),
        CharFam::Strike => out.extend(a.strike.map(|v| on_off(p, "strike", v))),
        CharFam::Underline => {
            out.extend(a.underline.map(|u| p.el("u", &[("val", underline_name(u).into())])));
        }
        CharFam::VAlign => out.extend(a.vertical_align.map(|v| {
            let name = match v {
                VerticalAlign::Baseline => "baseline",
                VerticalAlign::Superscript => "superscript",
                VerticalAlign::Subscript => "subscript",
            };
            p.el("vertAlign", &[("val", name.into())])
        })),
        CharFam::Font => {
            if let Some(font) = &a.font {
                out.push(p.el(
                    "rFonts",
                    &[
                        ("ascii", font.clone()),
                        ("hAnsi", font.clone()),
                        ("cs", font.clone()),
                    ],
                ));
            } else if let Some(t) = theme.font {
                let n = theme_font_name(t);
                out.push(p.el("rFonts", &[("asciiTheme", n.into()), ("hAnsiTheme", n.into())]));
            }
        }
        CharFam::Size => {
            out.extend(a.size_half_points.map(|v| p.el("sz", &[("val", v.to_string())])));
        }
        CharFam::SizeCs => {
            if let Some(v) = opaque_value(a, "szCs") {
                out.push(p.el("szCs", &[("val", String::from_utf8_lossy(v).into_owned())]));
            }
        }
        CharFam::Color => {
            if let Some(c) = a.color {
                out.push(p.el("color", &[("val", hex(c))]));
            } else if opaque_value(a, "color") == Some(b"auto") {
                out.push(p.el("color", &[("val", "auto".into())]));
            } else if let Some(t) = theme.color {
                out.push(p.el(
                    "color",
                    &[("val", "auto".into()), ("themeColor", theme_color_name(t).into())],
                ));
            }
        }
        CharFam::Highlight => {
            if let Some(c) = a.highlight {
                let (name, exact) = highlight_name(c);
                if !exact {
                    approximated.push(c);
                }
                out.push(p.el("highlight", &[("val", name.into())]));
            } else if opaque_value(a, "highlight") == Some(b"none") {
                out.push(p.el("highlight", &[("val", "none".into())]));
            }
        }
    }
    out
}

/// Attributes of a source `w:rFonts` kept when the font is rewritten.
fn fonts_merge(src: &PreservedElement, new: GenEl) -> GenEl {
    const REPLACED: [&str; 4] = ["ascii", "hAnsi", "asciiTheme", "hAnsiTheme"];
    let mut attrs: Vec<_> = src
        .attrs
        .iter()
        .filter(|a| !REPLACED.contains(&local_of(&a.name)))
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect();
    attrs.extend(new.attrs);
    GenEl {
        name: src.name.clone(),
        attrs,
    }
}

/// The `w:rPr` children for `cur`, merged with the source children `skel`.
/// Families on which `cur` and `base` agree keep the source elements; the
/// others are rebuilt, in place when the source had one, else by rank.
#[allow(clippy::too_many_arguments)]
pub(super) fn merge_rpr<'a>(
    skel: &'a [PreservedChild],
    base: Option<&CharAttrs>,
    cur: &CharAttrs,
    theme: ThemeRefs,
    p: &Prefixes,
    rank: fn(&str) -> usize,
    approximated: &mut Vec<Color>,
) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = skel.iter().map(Item::Skel).collect();
    for fam in CHAR_FAMILIES {
        if base.is_some_and(|b| char_fam_eq(fam, b, cur)) {
            continue;
        }
        let new = char_fam_elems(fam, cur, theme, p, approximated);
        replace_family(&mut items, fam.owns(), new, rank, fam == CharFam::Font);
    }
    items
}

/// Replaces the elements named `owns` by `new`, at the position of the first
/// one removed, or by rank when there was none.
pub(super) fn replace_family<'a>(
    items: &mut Vec<Item<'a>>,
    owns: &[&str],
    new: Vec<GenEl>,
    rank: fn(&str) -> usize,
    merge_attrs: bool,
) {
    let mut first = None;
    let mut source: Option<&'a PreservedElement> = None;
    let mut i = 0;
    while i < items.len() {
        if owns.contains(&item_local(&items[i])) {
            if first.is_none() {
                first = Some(i);
                if let Item::Skel(PreservedChild::Element(e)) = &items[i] {
                    source = Some(e);
                }
            }
            items.remove(i);
        } else {
            i += 1;
        }
    }
    let mut new = new;
    if merge_attrs {
        if let (Some(src), Some(first_new)) = (source, new.first_mut()) {
            *first_new = fonts_merge(src, std::mem::take(first_new));
        }
    }
    match first {
        Some(at) => {
            for (k, g) in new.into_iter().enumerate() {
                items.insert(at + k, Item::Gen(g));
            }
        }
        None => {
            for g in new {
                insert_ranked(items, Item::Gen(g), rank);
            }
        }
    }
}

/// All children of a new `w:rPr` for `cur`, in schema order.
pub(super) fn new_rpr(
    cur: &CharAttrs,
    theme: ThemeRefs,
    p: &Prefixes,
    approximated: &mut Vec<Color>,
) -> Vec<GenEl> {
    let mut out: Vec<GenEl> = Vec::new();
    for fam in CHAR_FAMILIES {
        out.extend(char_fam_elems(fam, cur, theme, p, approximated));
    }
    out.sort_by_key(|g| rpr_rank(local_of(&g.name)));
    out
}

// ---------------------------------------------------------------------------
// Paragraph properties
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParaFam {
    Style,
    Align,
    Indent,
    Spacing,
    Numbering,
}

pub(super) const PARA_FAMILIES: [ParaFam; 5] = [
    ParaFam::Style,
    ParaFam::Numbering,
    ParaFam::Spacing,
    ParaFam::Indent,
    ParaFam::Align,
];

pub(super) fn para_fam_eq(fam: ParaFam, a: &ParaProps, b: &ParaProps) -> bool {
    match fam {
        ParaFam::Style => a.style == b.style,
        ParaFam::Align => a.alignment == b.alignment,
        ParaFam::Indent => {
            a.indent_left == b.indent_left
                && a.indent_right == b.indent_right
                && a.indent_first_line == b.indent_first_line
        }
        ParaFam::Spacing => {
            a.space_before == b.space_before
                && a.space_after == b.space_after
                && a.line_spacing == b.line_spacing
        }
        ParaFam::Numbering => a.list == b.list,
    }
}

fn para_fam_owns(fam: ParaFam) -> &'static [&'static str] {
    match fam {
        ParaFam::Style => &["pStyle"],
        ParaFam::Align => &["jc"],
        ParaFam::Indent => &["ind"],
        ParaFam::Spacing => &["spacing"],
        ParaFam::Numbering => &["numPr"],
    }
}

/// Attributes (local names) of a family element that the model owns.
fn para_fam_attrs(fam: ParaFam) -> &'static [&'static str] {
    match fam {
        ParaFam::Indent => &["left", "start", "right", "end", "firstLine", "hanging"],
        ParaFam::Spacing => &["before", "after", "line", "lineRule"],
        _ => &[],
    }
}

/// The element(s) of a family for `cur`. `hide_style` is the default
/// paragraph style, which a new paragraph does not name.
pub(super) fn para_fam_elems(
    fam: ParaFam,
    cur: &ParaProps,
    p: &Prefixes,
    hide_style: Option<&str>,
) -> Vec<GenEl> {
    let mut out = Vec::new();
    match fam {
        ParaFam::Style => {
            if let Some(s) = cur.style.as_deref().filter(|s| Some(*s) != hide_style) {
                out.push(p.el("pStyle", &[("val", s.to_owned())]));
            }
        }
        ParaFam::Align => out.extend(cur.alignment.map(|a| {
            let v = match a {
                Alignment::Left => "left",
                Alignment::Center => "center",
                Alignment::Right => "right",
                Alignment::Justify => "both",
            };
            p.el("jc", &[("val", v.into())])
        })),
        ParaFam::Indent => {
            let mut attrs: Vec<(&str, String)> = Vec::new();
            if let Some(v) = cur.indent_left {
                attrs.push(("left", v.to_string()));
            }
            if let Some(v) = cur.indent_right {
                attrs.push(("right", v.to_string()));
            }
            match cur.indent_first_line {
                Some(v) if v >= 0 => attrs.push(("firstLine", v.to_string())),
                Some(v) => attrs.push(("hanging", v.unsigned_abs().to_string())),
                None => {}
            }
            if !attrs.is_empty() {
                out.push(p.el("ind", &attrs));
            }
        }
        ParaFam::Spacing => {
            let mut attrs: Vec<(&str, String)> = Vec::new();
            if let Some(v) = cur.space_before {
                attrs.push(("before", v.to_string()));
            }
            if let Some(v) = cur.space_after {
                attrs.push(("after", v.to_string()));
            }
            if let Some(l) = cur.line_spacing {
                attrs.push(("line", l.value.to_string()));
                let rule = match l.rule {
                    LineRule::Auto => "auto",
                    LineRule::Exact => "exact",
                    LineRule::AtLeast => "atLeast",
                };
                attrs.push(("lineRule", rule.into()));
            }
            if !attrs.is_empty() {
                out.push(p.el("spacing", &attrs));
            }
        }
        ParaFam::Numbering => {
            if let Some(l) = cur.list {
                let mut el = p.el("numPr", &[]);
                // Children are written by the caller through `Raw`; encode the
                // two values as attributes of a marker the body writer expands.
                el.attrs.push(("ilvl".into(), l.level.to_string()));
                el.attrs.push(("numId".into(), l.list_id.to_string()));
                out.push(el);
            }
        }
    }
    out
}

/// The `w:pPr` children for `cur` merged with the source children `skel`.
pub(super) fn merge_ppr<'a>(
    skel: &'a [PreservedChild],
    base: Option<&ParaProps>,
    cur: &ParaProps,
    p: &Prefixes,
    hide_style: Option<&str>,
    sect: Option<&[u8]>,
) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = skel.iter().map(Item::Skel).collect();
    for fam in PARA_FAMILIES {
        if base.is_some_and(|b| para_fam_eq(fam, b, cur)) {
            continue;
        }
        // A new paragraph does not name the default style; a changed one does.
        let hide = if base.is_none() { hide_style } else { None };
        let mut new = para_fam_elems(fam, cur, p, hide);
        let owns = para_fam_owns(fam);
        let interpreted = para_fam_attrs(fam);
        if !interpreted.is_empty() {
            // Keep the source attributes the model does not own.
            if let Some(src) = items.iter().find_map(|i| match i {
                Item::Skel(PreservedChild::Element(e)) if owns.contains(&local_of(&e.name)) => {
                    Some(e)
                }
                _ => None,
            }) {
                let kept: Vec<_> = src
                    .attrs
                    .iter()
                    .filter(|a| !interpreted.contains(&local_of(&a.name)))
                    .map(|a| (a.name.clone(), a.value.clone()))
                    .collect();
                if let Some(first) = new.first_mut() {
                    first.name.clone_from(&src.name);
                    first.attrs.extend(kept);
                } else if !kept.is_empty() {
                    new.push(GenEl {
                        name: src.name.clone(),
                        attrs: kept,
                    });
                }
            }
        }
        replace_family(&mut items, owns, new, ppr_rank, false);
    }
    if let Some(sect) = sect {
        insert_ranked(&mut items, Item::Raw(sect.to_vec()), ppr_rank);
    }
    items
}

/// Children of a new `w:pPr`, in schema order.
pub(super) fn new_ppr(
    cur: &ParaProps,
    p: &Prefixes,
    hide_style: Option<&str>,
) -> Vec<GenEl> {
    let mut out: Vec<GenEl> = Vec::new();
    for fam in PARA_FAMILIES {
        out.extend(para_fam_elems(fam, cur, p, hide_style));
    }
    out.sort_by_key(|g| ppr_rank(local_of(&g.name)));
    out
}
