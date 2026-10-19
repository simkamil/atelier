// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parts of the DOCX writer other than the body: styles, settings, font
//! table and numbering, regenerated from the model.

use super::super::preserve::{PreservedChild, PreservedElement, PreservedPart};
use super::super::{ppr_rank, rpr_rank};
use super::props::{
    GenEl, Item, Prefixes, ThemeRefs, local_of, local_of_bytes, merge_ppr, merge_rpr, new_ppr,
    new_rpr,
};
use super::xml::Out;
use super::{PageSetup, StyleDecl, WriteNotice};
use atelier_text_model::rich::{
    CharAttrs, Color, DocumentStyleDefaults, ParaProps, Style, StyleCatalog, StyleCharAttrs,
    StyleKind,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const W_URI: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const R_URI: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

pub(super) fn put(out: &mut Out, g: &GenEl) {
    if local_of(&g.name) == "numPr" {
        // Not written in parts other than the body.
        return;
    }
    out.start(
        &g.name,
        g.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())),
    );
    for c in &g.children {
        put(out, c);
    }
    out.end();
}

fn root(out: &mut Out, name: &str, extra: &[(&str, &str)]) {
    out.declaration();
    let mut attrs: Vec<(&str, &str)> = vec![("xmlns:w", W_URI), ("xmlns:r", R_URI)];
    attrs.extend_from_slice(extra);
    out.start(name, attrs);
}

fn plain_prefixes() -> Prefixes {
    Prefixes {
        w: "w".into(),
        r: "r".into(),
    }
}

// ---------------------------------------------------------------------------
// New parts
// ---------------------------------------------------------------------------

fn write_rpr(
    out: &mut Out,
    attrs: &CharAttrs,
    theme: ThemeRefs,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
) {
    let gens = new_rpr(attrs, theme, p, approx);
    if let Some(f) = &attrs.font {
        fonts.insert(f.clone());
    }
    if gens.is_empty() {
        return;
    }
    out.start("w:rPr", std::iter::empty());
    for g in &gens {
        put(out, g);
    }
    out.end();
}

fn write_ppr_new(out: &mut Out, props: &ParaProps, p: &Prefixes, extra: &[GenEl]) {
    let mut gens = new_ppr(props, p, None);
    gens.extend_from_slice(extra);
    gens.sort_by_key(|g| ppr_rank(local_of(&g.name)));
    if gens.is_empty() {
        return;
    }
    out.start("w:pPr", std::iter::empty());
    for g in &gens {
        put(out, g);
    }
    out.end();
}

/// WordprocessingML name of the built-in styles of Atelier's default profile.
fn builtin_name(id: &str) -> Option<String> {
    Some(
        match id {
            "atelier.normal" => "Normal",
            "atelier.title" => "Title",
            "atelier.subtitle" => "Subtitle",
            "atelier.quote" => "Quote",
            "atelier.quote.intense" => "Intense Quote",
            "atelier.emphasis" => "Emphasis",
            "atelier.strong" => "Strong",
            "atelier.caption" => "caption",
            "atelier.header" => "header",
            "atelier.footer" => "footer",
            "atelier.hyperlink" => "Hyperlink",
            "atelier.list.paragraph" => "List Paragraph",
            "atelier.no_spacing" => "No Spacing",
            "atelier.table.grid" => "Table Grid",
            _ => {
                let n = id.strip_prefix("atelier.heading.")?;
                return Some(format!("heading {n}"));
            }
        }
        .to_owned(),
    )
}

/// One `w:style` built from the model.
#[allow(clippy::too_many_arguments)]
fn write_style_new(
    out: &mut Out,
    style: &Style,
    name: &str,
    next: Option<&str>,
    is_default: bool,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
) {
    let kind = match style.kind {
        StyleKind::Paragraph => "paragraph",
        StyleKind::Character => "character",
        StyleKind::Table => "table",
    };
    let mut attrs = vec![("w:type", kind), ("w:styleId", style.id.as_str())];
    if is_default || style.is_default {
        attrs.push(("w:default", "1"));
    }
    out.start("w:style", attrs);
    let val = |out: &mut Out, el: &str, v: &str| out.empty(el, [("w:val", v)]);
    val(
        out,
        "w:name",
        style
            .name
            .clone()
            .or_else(|| builtin_name(&style.id))
            .as_deref()
            .unwrap_or(name),
    );
    if let Some(b) = &style.based_on {
        val(out, "w:basedOn", b);
    }
    if let Some(n) = next.filter(|n| *n != style.id) {
        if style.kind == StyleKind::Paragraph {
            val(out, "w:next", n);
        }
    }
    if let Some(l) = &style.linked_style {
        val(out, "w:link", l);
    }
    if let Some(priority) = style.ui_priority {
        val(out, "w:uiPriority", &priority.to_string());
    } else if builtin_name(&style.id).is_some() {
        val(out, "w:uiPriority", "99");
    }
    // A style that declares no name (converted from another format) keeps the
    // former behaviour: it is offered in the gallery.
    if style.quick || style.name.is_none() {
        out.empty("w:qFormat", std::iter::empty());
    }
    let heading = Some(u32::from(style.heading_level())).filter(|level| *level > 0);
    let mut extra = Vec::new();
    if let Some(level) = heading {
        extra.push(p.el("keepNext", &[]));
        extra.push(p.el("keepLines", &[]));
        extra.push(p.el(
            "outlineLvl",
            &[("val", (level.saturating_sub(1)).to_string())],
        ));
    }
    if style.kind != StyleKind::Character {
        write_ppr_new(out, &style.paragraph, p, &extra);
    }
    if style.kind != StyleKind::Table {
        let theme = ThemeRefs {
            font: style.character.theme_font,
            color: style.character.theme_color,
        };
        write_rpr(out, &style.character.values, theme, p, fonts, approx);
    }
    if style.kind == StyleKind::Table {
        out.start("w:tblPr", std::iter::empty());
        if style.id == "atelier.table.grid" {
            out.start("w:tblBorders", std::iter::empty());
            for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
                out.empty(
                    &format!("w:{side}"),
                    [
                        ("w:val", "single"),
                        ("w:sz", "4"),
                        ("w:space", "0"),
                        ("w:color", "auto"),
                    ],
                );
            }
            out.end();
        }
        out.end();
    }
    out.end();
}

fn write_defaults_new(
    out: &mut Out,
    d: &DocumentStyleDefaults,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
) {
    out.start("w:docDefaults", std::iter::empty());
    out.start("w:rPrDefault", std::iter::empty());
    let theme = ThemeRefs {
        font: d.character.theme_font,
        color: d.character.theme_color,
    };
    write_rpr(out, &d.character.values, theme, p, fonts, approx);
    out.end();
    out.start("w:pPrDefault", std::iter::empty());
    write_ppr_new(out, &d.paragraph, p, &[]);
    out.end();
    out.end();
}

pub(super) struct NewStyles<'a> {
    pub catalog: &'a StyleCatalog,
    pub names: &'a BTreeMap<String, String>,
    pub next: Option<&'a BTreeMap<String, String>>,
    pub default_style: Option<&'a str>,
}

pub(super) fn styles_new(
    s: &NewStyles<'_>,
    fonts: &mut BTreeSet<String>,
    notices: &mut Vec<WriteNotice>,
) -> Vec<u8> {
    let p = plain_prefixes();
    let mut out = Out::new();
    root(&mut out, "w:styles", &[]);
    let mut approx = Vec::new();
    write_defaults_new(&mut out, s.catalog.defaults(), &p, fonts, &mut approx);
    for (id, name) in s.names {
        let Some(style) = s.catalog.style(id) else {
            continue;
        };
        let next = s.next.and_then(|n| n.get(id)).map(String::as_str);
        write_style_new(
            &mut out,
            style,
            name,
            next,
            s.default_style == Some(id.as_str()),
            &p,
            fonts,
            &mut approx,
        );
    }
    for c in approx {
        notices.push(WriteNotice::HighlightColorApproximated {
            color: (c.r, c.g, c.b),
        });
    }
    out.end();
    out.into_bytes()
}

pub(super) fn settings_new(tab_stop: u32) -> Vec<u8> {
    let mut out = Out::new();
    root(&mut out, "w:settings", &[]);
    out.empty("w:zoom", [("w:percent", "100")]);
    out.empty(
        "w:defaultTabStop",
        [("w:val", tab_stop.to_string().as_str())],
    );
    out.empty("w:characterSpacingControl", [("w:val", "doNotCompress")]);
    out.start("w:compat", std::iter::empty());
    out.empty(
        "w:compatSetting",
        [
            ("w:name", "compatibilityMode"),
            ("w:uri", "http://schemas.microsoft.com/office/word"),
            ("w:val", "15"),
        ],
    );
    out.end();
    out.end();
    out.into_bytes()
}

pub(super) fn font_table_new(fonts: &BTreeSet<String>) -> Vec<u8> {
    let mut out = Out::new();
    root(&mut out, "w:fonts", &[]);
    for f in fonts {
        out.start("w:font", [("w:name", f.as_str())]);
        out.empty("w:charset", [("w:val", "00")]);
        out.empty("w:family", [("w:val", "auto")]);
        out.empty("w:pitch", [("w:val", "variable")]);
        out.end();
    }
    out.end();
    out.into_bytes()
}

/// A numbering part with one bullet list definition per list identifier.
pub(super) fn numbering_new(lists: &BTreeSet<u32>) -> Vec<u8> {
    let mut out = Out::new();
    root(&mut out, "w:numbering", &[]);
    for id in lists {
        let n = (id + 1).to_string();
        out.start("w:abstractNum", [("w:abstractNumId", n.as_str())]);
        out.empty("w:multiLevelType", [("w:val", "hybridMultilevel")]);
        for level in 0..9u32 {
            out.start("w:lvl", [("w:ilvl", level.to_string().as_str())]);
            out.empty("w:start", [("w:val", "1")]);
            out.empty("w:numFmt", [("w:val", "bullet")]);
            out.empty("w:lvlText", [("w:val", "\u{2022}")]);
            out.empty("w:lvlJc", [("w:val", "left")]);
            out.start("w:pPr", std::iter::empty());
            let left = 720 * (level + 1);
            out.empty(
                "w:ind",
                [("w:left", left.to_string().as_str()), ("w:hanging", "360")],
            );
            out.end();
            out.start("w:rPr", std::iter::empty());
            out.empty(
                "w:rFonts",
                [
                    ("w:ascii", "Arial"),
                    ("w:hAnsi", "Arial"),
                    ("w:cs", "Arial"),
                    ("w:hint", "default"),
                ],
            );
            out.end();
            out.end();
        }
        out.end();
    }
    for id in lists {
        let n = (id + 1).to_string();
        out.start("w:num", [("w:numId", n.as_str())]);
        out.empty("w:abstractNumId", [("w:val", n.as_str())]);
        out.end();
    }
    out.end();
    out.into_bytes()
}

/// `w:sectPr` of a section of the model.
pub(super) fn sect_pr(page: &PageSetup, kind: Option<&str>, extra_ns: bool) -> Vec<u8> {
    let _ = extra_ns;
    let mut out = Out::new();
    out.start("w:sectPr", std::iter::empty());
    if let Some(k) = kind {
        out.empty("w:type", [("w:val", k)]);
    }
    let (w, h) = (page.width.to_string(), page.height.to_string());
    let mut size = vec![("w:w", w.as_str()), ("w:h", h.as_str())];
    if page.landscape {
        size.push(("w:orient", "landscape"));
    }
    out.empty("w:pgSz", size);
    let m = [
        page.margin_top.to_string(),
        page.margin_right.to_string(),
        page.margin_bottom.to_string(),
        page.margin_left.to_string(),
    ];
    out.empty(
        "w:pgMar",
        [
            ("w:top", m[0].as_str()),
            ("w:right", m[1].as_str()),
            ("w:bottom", m[2].as_str()),
            ("w:left", m[3].as_str()),
            ("w:header", "708"),
            ("w:footer", "708"),
            ("w:gutter", "0"),
        ],
    );
    out.empty("w:cols", [("w:space", "708")]);
    out.end();
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// Regenerated parts of an imported document
// ---------------------------------------------------------------------------

const STYLE_ORDER: &[&str] = &[
    "name",
    "aliases",
    "basedOn",
    "next",
    "link",
    "autoRedefine",
    "hidden",
    "uiPriority",
    "semiHidden",
    "unhideWhenUsed",
    "qFormat",
    "locked",
    "personal",
    "personalCompose",
    "personalReply",
    "rsid",
    "pPr",
    "rPr",
    "tblPr",
    "trPr",
    "tcPr",
    "tblStylePr",
];

fn style_rank(local: &str) -> usize {
    STYLE_ORDER
        .iter()
        .position(|n| *n == local)
        .unwrap_or(STYLE_ORDER.len())
}

pub(super) struct ImportedStyles<'a> {
    pub part: &'a PreservedPart,
    pub prefixes: &'a Prefixes,
    pub catalog: &'a StyleCatalog,
    pub base: &'a StyleCatalog,
    pub added: &'a [StyleDecl],
    pub kinds: &'a BTreeMap<String, String>,
}

fn attr<'e>(e: &'e PreservedElement, local: &str) -> Option<&'e str> {
    e.attrs
        .iter()
        .find(|a| local_of(&a.name) == local)
        .map(|a| a.value.as_str())
}

fn write_plain(out: &mut Out, e: &PreservedElement) {
    out.start(
        &e.name,
        e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
    );
    for c in &e.children {
        match c {
            PreservedChild::Element(x) => write_plain(out, x),
            PreservedChild::Opaque(b) => out.raw(b),
            _ => {}
        }
    }
    out.end();
}

fn write_item(out: &mut Out, item: &Item<'_>) {
    match item {
        Item::Skel(PreservedChild::Element(e)) => write_plain(out, e),
        Item::Skel(PreservedChild::Opaque(b)) => out.raw(b),
        Item::Skel(_) => {}
        Item::Gen(g) => put(out, g),
        Item::Raw(b) => out.raw(b),
    }
}

fn find_child<'e>(e: &'e PreservedElement, local: &str) -> Option<&'e PreservedElement> {
    e.children.iter().find_map(|c| match c {
        PreservedChild::Element(x) if local_of(&x.name) == local => Some(x),
        _ => None,
    })
}

fn char_theme_changed(a: &StyleCharAttrs, b: &StyleCharAttrs) -> (bool, bool) {
    (a.theme_font != b.theme_font, a.theme_color != b.theme_color)
}

/// Character properties of `cur` against `base` for the merge, so that a
/// change of theme reference counts as a change of font or color.
fn merge_base(cur: &StyleCharAttrs, base: &StyleCharAttrs) -> CharAttrs {
    let mut b = base.values.clone();
    let (font, color) = char_theme_changed(cur, base);
    if font {
        b.font = Some("\u{0}theme".into());
    }
    if color {
        b.color = Some(Color { r: 1, g: 2, b: 3 });
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn write_rpr_merged(
    out: &mut Out,
    skel: Option<&PreservedElement>,
    cur: &StyleCharAttrs,
    base: Option<&StyleCharAttrs>,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
    name: &str,
) {
    let theme = ThemeRefs {
        font: cur.theme_font,
        color: cur.theme_color,
    };
    if let Some(f) = &cur.values.font {
        fonts.insert(f.clone());
    }
    match skel {
        Some(e) => {
            let base_attrs = base.map(|b| merge_base(cur, b));
            let items = merge_rpr(
                &e.children,
                base_attrs.as_ref(),
                &cur.values,
                theme,
                p,
                rpr_rank,
                approx,
            );
            out.start(
                &e.name,
                e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            for i in &items {
                write_item(out, i);
            }
            out.end();
        }
        None => {
            let gens = new_rpr(&cur.values, theme, p, approx);
            if gens.is_empty() {
                return;
            }
            out.start(name, std::iter::empty());
            for g in &gens {
                put(out, g);
            }
            out.end();
        }
    }
}

fn write_ppr_merged(
    out: &mut Out,
    skel: Option<&PreservedElement>,
    cur: &ParaProps,
    base: Option<&ParaProps>,
    p: &Prefixes,
    name: &str,
) {
    match skel {
        Some(e) => {
            let items = merge_ppr(&e.children, base, cur, p, None, None);
            out.start(
                &e.name,
                e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
            );
            for i in &items {
                write_item(out, i);
            }
            out.end();
        }
        None => {
            let gens = new_ppr(cur, p, None);
            if gens.is_empty() {
                return;
            }
            out.start(name, std::iter::empty());
            for g in &gens {
                put(out, g);
            }
            out.end();
        }
    }
}

fn write_style_merged(
    out: &mut Out,
    e: &PreservedElement,
    cur: &Style,
    base: &Style,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
) {
    out.start(
        &e.name,
        e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
    );
    // Children in source order; the four the model owns are rebuilt.
    let mut done_ppr = false;
    let mut done_rpr = false;
    let mut based = cur.based_on == base.based_on;
    let mut link = cur.linked_style == base.linked_style;
    let w = &p.w;
    let emit_ppr_new = |out: &mut Out| {
        let gens = new_ppr(&cur.paragraph, p, None);
        if !gens.is_empty() {
            out.start(&format!("{w}:pPr"), std::iter::empty());
            for g in &gens {
                put(out, g);
            }
            out.end();
        }
    };
    let mut pending_ppr = cur.paragraph != base.paragraph && find_child(e, "pPr").is_none();
    let mut pending_rpr = cur.character != base.character && find_child(e, "rPr").is_none();
    let mut pending_based = !based && find_child(e, "basedOn").is_none();
    let mut pending_link = !link && find_child(e, "link").is_none();
    let before = |local: &str, rank: usize| style_rank(local) > rank;
    for c in &e.children {
        let local = match c {
            PreservedChild::Element(x) => local_of(&x.name),
            PreservedChild::Opaque(b) => local_of_bytes(b),
            _ => "",
        };
        // Elements the source lacks go in front of the first later element.
        if pending_based && before(local, style_rank("basedOn")) {
            pending_based = false;
            if let Some(b) = &cur.based_on {
                out.empty(
                    &format!("{w}:basedOn"),
                    [(&*format!("{w}:val"), b.as_str())],
                );
            }
        }
        if pending_link && before(local, style_rank("link")) {
            pending_link = false;
            if let Some(b) = &cur.linked_style {
                out.empty(&format!("{w}:link"), [(&*format!("{w}:val"), b.as_str())]);
            }
        }
        if pending_ppr && before(local, style_rank("pPr")) {
            pending_ppr = false;
            emit_ppr_new(out);
        }
        if pending_rpr && before(local, style_rank("rPr")) {
            pending_rpr = false;
            write_rpr_merged(
                out,
                None,
                &cur.character,
                None,
                p,
                fonts,
                approx,
                &format!("{w}:rPr"),
            );
        }
        match c {
            PreservedChild::Element(x) => match local {
                "basedOn" if !based => {
                    based = true;
                    if let Some(b) = &cur.based_on {
                        out.empty(&x.name, [(&*format!("{w}:val"), b.as_str())]);
                    }
                }
                "link" if !link => {
                    link = true;
                    if let Some(b) = &cur.linked_style {
                        out.empty(&x.name, [(&*format!("{w}:val"), b.as_str())]);
                    }
                }
                "pPr" if !done_ppr => {
                    done_ppr = true;
                    if cur.paragraph != base.paragraph {
                        write_ppr_merged(
                            out,
                            Some(x),
                            &cur.paragraph,
                            Some(&base.paragraph),
                            p,
                            "",
                        );
                    } else {
                        write_plain(out, x);
                    }
                }
                "rPr" if !done_rpr => {
                    done_rpr = true;
                    if cur.character != base.character {
                        write_rpr_merged(
                            out,
                            Some(x),
                            &cur.character,
                            Some(&base.character),
                            p,
                            fonts,
                            approx,
                            "",
                        );
                    } else {
                        write_plain(out, x);
                    }
                }
                _ => write_plain(out, x),
            },
            PreservedChild::Opaque(b) => out.raw(b),
            _ => {}
        }
    }
    if pending_based {
        if let Some(b) = &cur.based_on {
            out.empty(
                &format!("{w}:basedOn"),
                [(&*format!("{w}:val"), b.as_str())],
            );
        }
    }
    if pending_link {
        if let Some(b) = &cur.linked_style {
            out.empty(&format!("{w}:link"), [(&*format!("{w}:val"), b.as_str())]);
        }
    }
    if pending_ppr {
        emit_ppr_new(out);
    }
    if pending_rpr {
        write_rpr_merged(
            out,
            None,
            &cur.character,
            None,
            p,
            fonts,
            approx,
            &format!("{w}:rPr"),
        );
    }
    out.end();
}

/// `styles.xml` of an imported document: the source skeleton, with the styles
/// the caller changed rebuilt, the removed ones dropped and the added ones appended.
pub(super) fn styles_imported(
    s: &ImportedStyles<'_>,
    root_attrs: &[(String, String)],
    fonts: &mut BTreeSet<String>,
    notices: &mut Vec<WriteNotice>,
) -> Vec<u8> {
    let p = s.prefixes;
    let mut out = Out::new();
    out.declaration();
    let r = &s.part.root;
    let attrs: Vec<(&str, &str)> = root_attrs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    out.start(&r.name, attrs);
    let mut approx = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let defaults_changed = s.catalog.defaults() != s.base.defaults();
    for c in &r.children {
        match c {
            PreservedChild::Element(e) if local_of(&e.name) == "docDefaults" => {
                if defaults_changed {
                    write_defaults_merged(
                        &mut out,
                        e,
                        s.catalog.defaults(),
                        s.base.defaults(),
                        p,
                        fonts,
                        &mut approx,
                    );
                } else {
                    write_plain(&mut out, e);
                }
            }
            PreservedChild::Element(e) if local_of(&e.name) == "style" => {
                let Some(id) = attr(e, "styleId") else {
                    write_plain(&mut out, e);
                    continue;
                };
                seen.insert(id.to_owned());
                match (s.catalog.style(id), s.base.style(id)) {
                    (None, _) => {}
                    (Some(cur), Some(base)) if cur != base => {
                        write_style_merged(&mut out, e, cur, base, p, fonts, &mut approx);
                    }
                    _ => write_plain(&mut out, e),
                }
            }
            PreservedChild::Element(e) => write_plain(&mut out, e),
            PreservedChild::Opaque(b) => out.raw(b),
            _ => {}
        }
    }
    for decl in s.added {
        if seen.contains(&decl.id) {
            continue;
        }
        let Some(style) = s.catalog.style(&decl.id) else {
            continue;
        };
        write_style_new(
            &mut out,
            style,
            &decl.name,
            decl.next.as_deref(),
            false,
            &Prefixes {
                w: p.w.clone(),
                r: p.r.clone(),
            },
            fonts,
            &mut approx,
        );
    }
    let _ = s.kinds;
    for c in approx {
        notices.push(WriteNotice::HighlightColorApproximated {
            color: (c.r, c.g, c.b),
        });
    }
    out.end();
    out.into_bytes()
}

fn write_defaults_merged(
    out: &mut Out,
    e: &PreservedElement,
    cur: &DocumentStyleDefaults,
    base: &DocumentStyleDefaults,
    p: &Prefixes,
    fonts: &mut BTreeSet<String>,
    approx: &mut Vec<Color>,
) {
    out.start(
        &e.name,
        e.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
    );
    let w = &p.w;
    let mut has_r = false;
    let mut has_p = false;
    for c in &e.children {
        match c {
            PreservedChild::Element(x) if local_of(&x.name) == "rPrDefault" => {
                has_r = true;
                out.start(
                    &x.name,
                    x.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                );
                if cur.character != base.character {
                    write_rpr_merged(
                        out,
                        find_child(x, "rPr"),
                        &cur.character,
                        Some(&base.character),
                        p,
                        fonts,
                        approx,
                        &format!("{w}:rPr"),
                    );
                } else if let Some(r) = find_child(x, "rPr") {
                    write_plain(out, r);
                }
                out.end();
            }
            PreservedChild::Element(x) if local_of(&x.name) == "pPrDefault" => {
                has_p = true;
                out.start(
                    &x.name,
                    x.attrs.iter().map(|a| (a.name.as_str(), a.value.as_str())),
                );
                if cur.paragraph != base.paragraph {
                    write_ppr_merged(
                        out,
                        find_child(x, "pPr"),
                        &cur.paragraph,
                        Some(&base.paragraph),
                        p,
                        &format!("{w}:pPr"),
                    );
                } else if let Some(r) = find_child(x, "pPr") {
                    write_plain(out, r);
                }
                out.end();
            }
            PreservedChild::Element(x) => write_plain(out, x),
            PreservedChild::Opaque(b) => out.raw(b),
            _ => {}
        }
    }
    if !has_r && cur.character != base.character {
        out.start(&format!("{w}:rPrDefault"), std::iter::empty());
        write_rpr_merged(
            out,
            None,
            &cur.character,
            None,
            p,
            fonts,
            approx,
            &format!("{w}:rPr"),
        );
        out.end();
    }
    if !has_p && cur.paragraph != base.paragraph {
        out.start(&format!("{w}:pPrDefault"), std::iter::empty());
        write_ppr_merged(out, None, &cur.paragraph, None, p, &format!("{w}:pPr"));
        out.end();
    }
    out.end();
}

/// `settings.xml` with a new default tab stop.
pub(super) fn settings_imported(
    part: &PreservedPart,
    root_attrs: &[(String, String)],
    p: &Prefixes,
    tab_stop: Option<u32>,
) -> Vec<u8> {
    const AFTER: &[&str] = &[
        "autoHyphenation",
        "consecutiveHyphenLimit",
        "hyphenationZone",
        "doNotHyphenateCaps",
        "showEnvelope",
        "summaryLength",
        "clickAndTypeStyle",
        "defaultTableStyle",
        "evenAndOddHeaders",
        "characterSpacingControl",
        "footnotePr",
        "endnotePr",
        "compat",
        "docVars",
        "rsids",
        "mathPr",
        "themeFontLang",
        "clrSchemeMapping",
    ];
    let mut out = Out::new();
    out.declaration();
    let r = &part.root;
    let attrs: Vec<(&str, &str)> = root_attrs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    out.start(&r.name, attrs);
    let w = &p.w;
    let value = tab_stop.map(|v| v.to_string());
    let mut written = false;
    for c in &r.children {
        let local = match c {
            PreservedChild::Element(x) => local_of(&x.name),
            PreservedChild::Opaque(b) => local_of_bytes(b),
            _ => "",
        };
        if !written && local != "defaultTabStop" && AFTER.contains(&local) {
            written = true;
            if let Some(v) = &value {
                out.empty(
                    &format!("{w}:defaultTabStop"),
                    [(&*format!("{w}:val"), v.as_str())],
                );
            }
        }
        match c {
            PreservedChild::Element(x) if local == "defaultTabStop" => {
                written = true;
                if let Some(v) = &value {
                    let mut attrs: Vec<(&str, &str)> = x
                        .attrs
                        .iter()
                        .filter(|a| local_of(&a.name) != "val")
                        .map(|a| (a.name.as_str(), a.value.as_str()))
                        .collect();
                    let key = format!("{w}:val");
                    attrs.push((&key, v.as_str()));
                    out.empty(&x.name, attrs);
                }
            }
            PreservedChild::Element(x) => write_plain(&mut out, x),
            PreservedChild::Opaque(b) => out.raw(b),
            _ => {}
        }
    }
    if !written {
        if let Some(v) = &value {
            out.empty(
                &format!("{w}:defaultTabStop"),
                [(&*format!("{w}:val"), v.as_str())],
            );
        }
    }
    out.end();
    out.into_bytes()
}
