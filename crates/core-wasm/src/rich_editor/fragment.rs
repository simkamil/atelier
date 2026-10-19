// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rich fragment (T0): `CopyFragment` and `PasteFragment`
//! (`docs/us-327-multidoc-protocol.md`, section 5).
//!
//! A fragment is the content of a flat range as the model holds it, so that a
//! copy between two documents loses nothing the paste can carry: paragraphs
//! (text, direct paragraph properties), runs (direct character attributes),
//! the styles they name (definitions, base chains), pictures, line and page
//! breaks, symbols, fields and links. Tables and opaque blocks are flattened
//! or dropped and bookmarks and opaque objects (shapes, charts: their data
//! refers to the package they come from) are not carried: the reply says
//! `lossy`.
//!
//! Format, version 1, little-endian (`u32` lengths, UTF-8 strings):
//! `"ATFR"`, `u16` version, `u16` flags (0), `u32` style count and the styles,
//! `u32` paragraph count (1 or more) and the paragraphs. Every optional field
//! is a `u8` tag (0 absent, 1 present) then its value.
//!
//! Only the public API of the model is read here. `CharAttrs`, `ParaProps`
//! and `Style` are destructured **without** `..`: a field added to them stops
//! the compilation here, so that it is carried (or consciously left out)
//! rather than silently dropped by a copy.

use std::collections::BTreeMap;

use atelier_text_model::inline_image::IMAGE_OBJECT_KIND;
use atelier_text_model::rich::flat::{BlockKind, FlatEdit, FlatIndex, LayoutContext, OBJECT_LEN};
use atelier_text_model::rich::{
    Alignment, AnchoredObject, BreakKind, CharAttrs, CharRun, ClearSide, Color, Field, InlineBreak,
    InlineSymbol, LineRule, LineSpacing, Link, LinkTarget, ListRef, NodeId, OpaqueAttr, ParaProps,
    Paragraph, RichDocument, Style, StyleCharAttrs, StyleKind, ThemeColor, ThemeFont, Underline,
    VerticalAlign,
};

use super::styles::layout_doc;
use super::{DEFAULT_LAYOUT_BUDGET, Session, merge};
use crate::rich_layout::{PageDelta, RichPageCache};
use crate::rich_protocol::{
    ErrorCode, MAX_FRAGMENT_BYTES, MAX_FRAGMENT_TEXT_BYTES, Response, check_range, to_position,
};

const MAGIC: [u8; 4] = *b"ATFR";
/// Version of the fragment format this engine writes and reads.
pub const FRAGMENT_VERSION: u16 = 1;
/// Longest string of a fragment (a style name, a font, a field instruction).
const MAX_STRING_BYTES: usize = 64 * 1024;
/// Most styles a fragment carries.
const MAX_STYLES: usize = 256;
/// Largest picture of a fragment (the insertion bound, `images::MAX_IMAGE_BYTES`).
const MAX_OBJECT_BYTES: usize = 8 * 1024 * 1024;
/// Chain of `based_on` followed when exporting a style.
const MAX_STYLE_DEPTH: usize = 32;

const UNDERLINES: [Underline; 7] = [
    Underline::None,
    Underline::Single,
    Underline::Double,
    Underline::Thick,
    Underline::Dotted,
    Underline::Dashed,
    Underline::Wave,
];
const VALIGNS: [VerticalAlign; 3] = [
    VerticalAlign::Baseline,
    VerticalAlign::Superscript,
    VerticalAlign::Subscript,
];
const ALIGNMENTS: [Alignment; 4] = [
    Alignment::Left,
    Alignment::Center,
    Alignment::Right,
    Alignment::Justify,
];
const RULES: [LineRule; 3] = [LineRule::Auto, LineRule::Exact, LineRule::AtLeast];
const CLEARS: [ClearSide; 4] = [
    ClearSide::None,
    ClearSide::Left,
    ClearSide::Right,
    ClearSide::All,
];
const THEME_COLORS: [ThemeColor; 12] = [
    ThemeColor::Dark1,
    ThemeColor::Light1,
    ThemeColor::Dark2,
    ThemeColor::Light2,
    ThemeColor::Accent1,
    ThemeColor::Accent2,
    ThemeColor::Accent3,
    ThemeColor::Accent4,
    ThemeColor::Accent5,
    ThemeColor::Accent6,
    ThemeColor::Hyperlink,
    ThemeColor::FollowedHyperlink,
];
const THEME_FONTS: [ThemeFont; 2] = [ThemeFont::Major, ThemeFont::Minor];

fn index_of<T: PartialEq>(list: &[T], value: &T) -> u8 {
    list.iter().position(|x| x == value).unwrap_or(0) as u8
}

// ---- writing ---------------------------------------------------------------

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    fn opt<T>(&mut self, v: &Option<T>, put: impl FnOnce(&mut Self, &T)) {
        match v {
            Some(x) => {
                self.u8(1);
                put(self, x);
            }
            None => self.u8(0),
        }
    }
    fn flag(&mut self, v: Option<bool>) {
        self.opt(&v, |w, b| w.u8(u8::from(*b)));
    }
    fn color(&mut self, c: &Color) {
        self.0.extend_from_slice(&[c.r, c.g, c.b]);
    }
    fn opaque(&mut self, list: &[OpaqueAttr]) {
        self.u32(list.len() as u32);
        for a in list {
            self.str(&a.namespace);
            self.str(&a.name);
            self.bytes(&a.value);
        }
    }
    fn char_attrs(&mut self, a: &CharAttrs) {
        let CharAttrs {
            bold,
            italic,
            underline,
            strike,
            vertical_align,
            font,
            size_half_points,
            color,
            highlight,
            char_style,
            opaque,
        } = a;
        self.flag(*bold);
        self.flag(*italic);
        self.opt(underline, |w, u| w.u8(index_of(&UNDERLINES, u)));
        self.flag(*strike);
        self.opt(vertical_align, |w, v| w.u8(index_of(&VALIGNS, v)));
        self.opt(font, |w, f| w.str(f));
        self.opt(size_half_points, |w, n| w.u32(*n));
        self.opt(color, |w, c| w.color(c));
        self.opt(highlight, |w, c| w.color(c));
        self.opt(char_style, |w, s| w.str(s));
        self.opaque(opaque);
    }
    fn para_props(&mut self, p: &ParaProps) {
        let ParaProps {
            alignment,
            indent_left,
            indent_right,
            indent_first_line,
            space_before,
            space_after,
            line_spacing,
            style,
            list,
            opaque,
        } = p;
        self.opt(alignment, |w, a| w.u8(index_of(&ALIGNMENTS, a)));
        self.opt(indent_left, |w, n| w.i32(*n));
        self.opt(indent_right, |w, n| w.i32(*n));
        self.opt(indent_first_line, |w, n| w.i32(*n));
        self.opt(space_before, |w, n| w.u32(*n));
        self.opt(space_after, |w, n| w.u32(*n));
        self.opt(line_spacing, |w, l| {
            w.u8(index_of(&RULES, &l.rule));
            w.u32(l.value);
        });
        self.opt(style, |w, s| w.str(s));
        self.opt(list, |w, l| {
            w.u32(l.list_id);
            w.u8(l.level);
        });
        self.opaque(opaque);
    }
    fn style(&mut self, s: &Style) {
        let Style {
            id,
            kind,
            based_on,
            linked_style: _,
            character,
            paragraph,
            name,
            quick,
            is_default: _,
            ui_priority,
        } = s;
        let StyleCharAttrs {
            values,
            theme_font,
            theme_color,
            theme_highlight,
        } = character;
        self.str(id);
        self.u8(u8::from(*kind == StyleKind::Character));
        self.opt(based_on, |w, b| w.str(b));
        self.char_attrs(values);
        self.opt(theme_font, |w, f| w.u8(index_of(&THEME_FONTS, f)));
        self.opt(theme_color, |w, c| w.u8(index_of(&THEME_COLORS, c)));
        self.opt(theme_highlight, |w, c| w.u8(index_of(&THEME_COLORS, c)));
        self.para_props(paragraph);
        self.opt(name, |w, n| w.str(n));
        self.u8(u8::from(*quick));
        self.opt(ui_priority, |w, n| w.u32(*n));
    }
    fn paragraph(&mut self, p: &Paragraph) {
        self.str(&p.text);
        self.para_props(&p.props);
        self.u32(p.runs.len() as u32);
        for r in &p.runs {
            self.u32(r.start as u32);
            self.u32(r.end as u32);
            self.char_attrs(&r.attrs);
        }
        let pictures: Vec<&AnchoredObject> = p
            .anchored
            .iter()
            .filter(|o| o.kind == IMAGE_OBJECT_KIND)
            .collect();
        self.u32(pictures.len() as u32);
        for o in pictures {
            self.u32(o.offset as u32);
            self.str(&o.kind);
            self.bytes(&o.data);
        }
        self.u32(p.breaks.len() as u32);
        for b in &p.breaks {
            self.u32(b.offset as u32);
            match b.kind {
                BreakKind::Line => self.u8(0),
                BreakKind::Page => self.u8(1),
                BreakKind::Column => self.u8(2),
                BreakKind::Wrapping {
                    explicit_type,
                    clear,
                } => {
                    self.u8(3);
                    self.u8(u8::from(explicit_type));
                    self.opt(&clear, |w, c| w.u8(index_of(&CLEARS, c)));
                }
            }
        }
        self.u32(p.symbols.len() as u32);
        for s in &p.symbols {
            self.u32(s.offset as u32);
            self.str(&s.font);
            self.u32(s.code);
        }
        self.u32(p.fields.len() as u32);
        for f in &p.fields {
            self.u32(f.start as u32);
            self.u32(f.end as u32);
            self.str(&f.instruction);
        }
        self.u32(p.links.len() as u32);
        for l in &p.links {
            self.u32(l.start as u32);
            self.u32(l.end as u32);
            match &l.target {
                LinkTarget::External(t) => {
                    self.u8(0);
                    self.str(t);
                }
                LinkTarget::Bookmark(t) => {
                    self.u8(1);
                    self.str(t);
                }
            }
            self.opt(&l.tooltip, |w, t| w.str(t));
        }
    }
}

/// The bytes of a fragment.
pub(super) fn encode(pieces: &[Paragraph], styles: &[Style]) -> Result<Vec<u8>, ErrorCode> {
    let mut w = W::default();
    w.0.extend_from_slice(&MAGIC);
    w.0.extend_from_slice(&FRAGMENT_VERSION.to_le_bytes());
    w.0.extend_from_slice(&0u16.to_le_bytes());
    w.u32(styles.len() as u32);
    for s in styles {
        w.style(s);
    }
    w.u32(pieces.len() as u32);
    for p in pieces {
        w.paragraph(p);
    }
    if w.0.len() > MAX_FRAGMENT_BYTES {
        return Err(ErrorCode::Limit);
    }
    Ok(w.0)
}

// ---- reading ---------------------------------------------------------------

struct R<'a> {
    bytes: &'a [u8],
    at: usize,
}

type Res<T> = Result<T, ErrorCode>;

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(ErrorCode::FragmentInvalid)?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(ErrorCode::FragmentInvalid)?;
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Res<i32> {
        Ok(self.u32()? as i32)
    }
    /// A count of elements that each take at least one byte.
    fn count(&mut self) -> Res<usize> {
        let n = self.u32()? as usize;
        if n > self.bytes.len() - self.at {
            return Err(ErrorCode::FragmentInvalid);
        }
        Ok(n)
    }
    fn bytes(&mut self, max: usize) -> Res<&'a [u8]> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(ErrorCode::FragmentInvalid);
        }
        self.take(n)
    }
    fn str(&mut self) -> Res<String> {
        let raw = self.bytes(MAX_STRING_BYTES)?;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| ErrorCode::FragmentInvalid)
    }
    fn tag(&mut self) -> Res<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ErrorCode::FragmentInvalid),
        }
    }
    fn opt<T>(&mut self, get: impl FnOnce(&mut Self) -> Res<T>) -> Res<Option<T>> {
        if self.tag()? {
            get(self).map(Some)
        } else {
            Ok(None)
        }
    }
    fn flag(&mut self) -> Res<Option<bool>> {
        self.opt(|r| r.tag())
    }
    fn choice<T: Copy>(&mut self, list: &[T]) -> Res<T> {
        list.get(usize::from(self.u8()?))
            .copied()
            .ok_or(ErrorCode::FragmentInvalid)
    }
    fn color(&mut self) -> Res<Color> {
        let b = self.take(3)?;
        Ok(Color {
            r: b[0],
            g: b[1],
            b: b[2],
        })
    }
    fn opaque(&mut self) -> Res<Vec<OpaqueAttr>> {
        let n = self.count()?;
        let mut list: Vec<OpaqueAttr> = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            list.push(OpaqueAttr {
                namespace: self.str()?,
                name: self.str()?,
                value: self.bytes(MAX_STRING_BYTES)?.to_vec(),
            });
        }
        Ok(list)
    }
    fn char_attrs(&mut self) -> Res<CharAttrs> {
        Ok(CharAttrs {
            bold: self.flag()?,
            italic: self.flag()?,
            underline: self.opt(|r| r.choice(&UNDERLINES))?,
            strike: self.flag()?,
            vertical_align: self.opt(|r| r.choice(&VALIGNS))?,
            font: self.opt(|r| r.str())?,
            size_half_points: self.opt(|r| r.u32())?,
            color: self.opt(|r| r.color())?,
            highlight: self.opt(|r| r.color())?,
            char_style: self.opt(|r| r.str())?,
            opaque: self.opaque()?,
        })
    }
    fn para_props(&mut self) -> Res<ParaProps> {
        Ok(ParaProps {
            alignment: self.opt(|r| r.choice(&ALIGNMENTS))?,
            indent_left: self.opt(|r| r.i32())?,
            indent_right: self.opt(|r| r.i32())?,
            indent_first_line: self.opt(|r| r.i32())?,
            space_before: self.opt(|r| r.u32())?,
            space_after: self.opt(|r| r.u32())?,
            line_spacing: self.opt(|r| {
                Ok(LineSpacing {
                    rule: r.choice(&RULES)?,
                    value: r.u32()?,
                })
            })?,
            style: self.opt(|r| r.str())?,
            list: self.opt(|r| {
                Ok(ListRef {
                    list_id: r.u32()?,
                    level: r.u8()?,
                })
            })?,
            opaque: self.opaque()?,
        })
    }
    fn style(&mut self) -> Res<Style> {
        let id = self.str()?;
        let kind = if self.tag()? {
            StyleKind::Character
        } else {
            StyleKind::Paragraph
        };
        let based_on = self.opt(|r| r.str())?;
        let values = self.char_attrs()?;
        let theme_font = self.opt(|r| r.choice(&THEME_FONTS))?;
        let theme_color = self.opt(|r| r.choice(&THEME_COLORS))?;
        let theme_highlight = self.opt(|r| r.choice(&THEME_COLORS))?;
        let paragraph = self.para_props()?;
        let name = self.opt(|r| r.str())?;
        let quick = self.tag()?;
        let ui_priority = self.opt(|r| r.u32())?;
        Ok(Style {
            id,
            kind,
            based_on,
            linked_style: None,
            character: StyleCharAttrs {
                values,
                theme_font,
                theme_color,
                theme_highlight,
            },
            paragraph,
            name,
            quick,
            is_default: false,
            ui_priority,
        })
    }
    /// An offset into a text of `len` bytes, on a scalar boundary.
    fn offset(&mut self, text: &str) -> Res<usize> {
        let at = self.u32()? as usize;
        if at > text.len() || !text.is_char_boundary(at) {
            return Err(ErrorCode::FragmentInvalid);
        }
        Ok(at)
    }
    fn paragraph(&mut self, mint: &mut dyn FnMut() -> Result<NodeId, ErrorCode>) -> Res<Paragraph> {
        let mut p = Paragraph {
            text: self.str()?,
            props: self.para_props()?,
            ..Paragraph::default()
        };
        for _ in 0..self.count()? {
            let (start, end) = (self.offset(&p.text)?, self.offset(&p.text)?);
            let attrs = self.char_attrs()?;
            p.runs.push(CharRun { start, end, attrs });
        }
        for _ in 0..self.count()? {
            let offset = self.offset(&p.text)?;
            let kind = self.str()?;
            let data = self.bytes(MAX_OBJECT_BYTES)?.to_vec();
            if kind != IMAGE_OBJECT_KIND {
                return Err(ErrorCode::FragmentInvalid);
            }
            p.anchored.push(AnchoredObject {
                id: mint()?,
                offset,
                kind,
                data,
            });
        }
        for _ in 0..self.count()? {
            let offset = self.offset(&p.text)?;
            let kind = match self.u8()? {
                0 => BreakKind::Line,
                1 => BreakKind::Page,
                2 => BreakKind::Column,
                3 => BreakKind::Wrapping {
                    explicit_type: self.tag()?,
                    clear: self.opt(|r| r.choice(&CLEARS))?,
                },
                _ => return Err(ErrorCode::FragmentInvalid),
            };
            p.breaks.push(InlineBreak {
                id: mint()?,
                offset,
                kind,
            });
        }
        for _ in 0..self.count()? {
            let offset = self.offset(&p.text)?;
            p.symbols.push(InlineSymbol {
                id: mint()?,
                offset,
                font: self.str()?,
                code: self.u32()?,
            });
        }
        for _ in 0..self.count()? {
            let (start, end) = (self.offset(&p.text)?, self.offset(&p.text)?);
            p.fields.push(Field {
                id: mint()?,
                start,
                end,
                instruction: self.str()?,
            });
        }
        for _ in 0..self.count()? {
            let (start, end) = (self.offset(&p.text)?, self.offset(&p.text)?);
            let target = match self.u8()? {
                0 => LinkTarget::External(self.str()?),
                1 => LinkTarget::Bookmark(self.str()?),
                _ => return Err(ErrorCode::FragmentInvalid),
            };
            p.links.push(Link {
                id: mint()?,
                start,
                end,
                target,
                tooltip: self.opt(|r| r.str())?,
            });
        }
        p.validate_local().map_err(|_| ErrorCode::FragmentInvalid)?;
        Ok(p)
    }
}

/// The paragraphs and styles of a fragment. Item ids come from `mint`; every
/// paragraph passed the model's local validation.
pub(super) fn decode(
    bytes: &[u8],
    mint: &mut dyn FnMut() -> Result<NodeId, ErrorCode>,
) -> Res<(Vec<Paragraph>, Vec<Style>)> {
    if bytes.len() > MAX_FRAGMENT_BYTES {
        return Err(ErrorCode::Limit);
    }
    let mut r = R { bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err(ErrorCode::FragmentInvalid);
    }
    let version = r.take(2)?;
    if u16::from_le_bytes([version[0], version[1]]) != FRAGMENT_VERSION || r.take(2)? != [0, 0] {
        return Err(ErrorCode::FragmentInvalid);
    }
    let count = r.count()?;
    if count > MAX_STYLES {
        return Err(ErrorCode::FragmentInvalid);
    }
    let mut styles = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        styles.push(r.style()?);
    }
    let count = r.count()?;
    if count == 0 {
        return Err(ErrorCode::FragmentInvalid);
    }
    let mut pieces = Vec::with_capacity(count.min(1024));
    let mut text = 0usize;
    for _ in 0..count {
        let p = r.paragraph(mint)?;
        text += p.text.len();
        if text > MAX_FRAGMENT_TEXT_BYTES {
            return Err(ErrorCode::Limit);
        }
        pieces.push(p);
    }
    if r.at != bytes.len() {
        return Err(ErrorCode::FragmentInvalid);
    }
    Ok((pieces, styles))
}

// ---- extraction ------------------------------------------------------------

/// The paragraphs of the flat range `start..end` (valid positions) as pieces,
/// the styles they name, and whether something was left out.
fn extract(
    doc: &RichDocument,
    start: usize,
    end: usize,
) -> Result<(Vec<Paragraph>, Vec<Style>, bool), ErrorCode> {
    let first = doc.locate(start)?;
    let last = doc.locate(end)?;
    let mut lossy = false;
    let mut pieces: Vec<Paragraph> = Vec::new();
    for index in first.paragraph..=last.paragraph {
        match doc.block_kind(index)? {
            BlockKind::Opaque => {
                lossy = true;
                continue;
            }
            BlockKind::TableCell { .. } => lossy = true,
            BlockKind::Body => {}
        }
        let Some(p) = LayoutContext::paragraph(doc, index) else {
            continue;
        };
        let total = doc.paragraph_flat(index)?.len();
        let from = if index == first.paragraph {
            first.offset
        } else {
            0
        };
        let to = if index == last.paragraph {
            last.offset
        } else {
            total
        };
        let (ts, te) = (doc.flat_to_text(index, from)?, doc.flat_to_text(index, to)?);
        let mut piece = Paragraph {
            text: p.text[ts..te].to_owned(),
            props: p.props.clone(),
            ..Paragraph::default()
        };
        for r in p.runs.iter().filter(|r| r.end > ts && r.start < te) {
            piece.runs.push(CharRun {
                start: r.start.max(ts) - ts,
                end: r.end.min(te) - ts,
                attrs: r.attrs.clone(),
            });
        }
        // The flat position of the j-th item is its offset plus three bytes per item before it.
        let mut order: Vec<(usize, NodeId)> = Vec::new();
        order.extend(p.anchored.iter().map(|x| (x.offset, x.id)));
        order.extend(p.breaks.iter().map(|x| (x.offset, x.id)));
        order.extend(p.symbols.iter().map(|x| (x.offset, x.id)));
        order.sort();
        let inside = |offset: usize, id: NodeId| {
            let rank = order.iter().position(|x| *x == (offset, id)).unwrap_or(0);
            let flat = offset + OBJECT_LEN * rank;
            from <= flat && flat + OBJECT_LEN <= to
        };
        for x in &p.anchored {
            if inside(x.offset, x.id) {
                if x.kind == IMAGE_OBJECT_KIND {
                    piece.anchored.push(AnchoredObject {
                        offset: x.offset - ts,
                        ..x.clone()
                    });
                } else {
                    lossy = true;
                }
            }
        }
        for x in p.breaks.iter().filter(|x| inside(x.offset, x.id)) {
            piece.breaks.push(InlineBreak {
                offset: x.offset - ts,
                ..x.clone()
            });
        }
        for x in p.symbols.iter().filter(|x| inside(x.offset, x.id)) {
            piece.symbols.push(InlineSymbol {
                offset: x.offset - ts,
                ..x.clone()
            });
        }
        for x in p.fields.iter().filter(|x| x.start >= ts && x.end <= te) {
            piece.fields.push(Field {
                start: x.start - ts,
                end: x.end - ts,
                ..x.clone()
            });
        }
        for x in p.links.iter().filter(|x| x.end > ts && x.start < te) {
            piece.links.push(Link {
                start: x.start.max(ts) - ts,
                end: x.end.min(te) - ts,
                ..x.clone()
            });
        }
        lossy |= !p.bookmarks.is_empty() && p.bookmarks.iter().any(|b| b.end > ts && b.start < te);
        pieces.push(piece);
    }
    if pieces.is_empty() {
        return Err(ErrorCode::EmptySelection);
    }
    let styles = referenced_styles(doc, &pieces);
    Ok((pieces, styles, lossy))
}

/// The styles the pieces name, each after the style it is based on.
fn referenced_styles(doc: &RichDocument, pieces: &[Paragraph]) -> Vec<Style> {
    let catalog = LayoutContext::catalog(doc);
    let mut wanted: Vec<&str> = Vec::new();
    for p in pieces {
        wanted.extend(p.props.style.as_deref());
        wanted.extend(p.runs.iter().filter_map(|r| r.attrs.char_style.as_deref()));
    }
    let mut out: Vec<Style> = Vec::new();
    for id in wanted {
        let mut chain: Vec<&Style> = Vec::new();
        let mut at = catalog.style(id);
        while let Some(style) = at {
            if chain.len() >= MAX_STYLE_DEPTH || chain.iter().any(|s| s.id == style.id) {
                break;
            }
            chain.push(style);
            at = style.based_on.as_deref().and_then(|b| catalog.style(b));
        }
        for style in chain.into_iter().rev() {
            if out.len() < MAX_STYLES && !out.iter().any(|s| s.id == style.id) {
                let mut copy = style.clone();
                copy.linked_style = None;
                copy.is_default = false;
                out.push(copy);
            }
        }
    }
    out
}

// ---- session ---------------------------------------------------------------

impl Session {
    /// `CopyFragment`: the fragment of `start..end` at the current revision.
    pub(super) fn copy_fragment(
        &mut self,
        revision: u64,
        start: u64,
        end: u64,
    ) -> Result<Response, ErrorCode> {
        if revision != self.revision() {
            return Err(ErrorCode::StaleRevision);
        }
        check_range(self.doc.len() as u64, start, end)?;
        let (start, end) = (to_position(start)?, to_position(end)?);
        self.doc.check_position(start)?;
        self.doc.check_position(end)?;
        if start == end {
            return Err(ErrorCode::EmptySelection);
        }
        let text = self.doc.text(start, end)?;
        if text.len() > MAX_FRAGMENT_TEXT_BYTES {
            return Err(ErrorCode::Limit);
        }
        let (pieces, styles, lossy) = extract(&self.doc, start, end)?;
        Ok(Response::Fragment {
            revision,
            lossy,
            text,
            bytes: encode(&pieces, &styles)?,
        })
    }

    /// `PasteFragment`: the selection is replaced by the fragment, in one undo
    /// step. A TXT document takes the text only.
    pub(super) fn paste_fragment(&mut self, bytes: &[u8]) -> Result<Response, ErrorCode> {
        self.deny_read_only()?;
        let doc = &mut self.doc;
        let (mut pieces, styles) =
            decode(bytes, &mut || doc.alloc_id().map_err(|_| ErrorCode::Limit))?;
        let (start, end) = self.selection_range();
        if self.txt {
            let text = plain_text(&pieces);
            return self.edit(start, end, &text);
        }
        let (count, total) = self.origin.images;
        let pictures: Vec<usize> = pieces
            .iter()
            .flat_map(|p| p.anchored.iter().map(|o| o.data.len()))
            .collect();
        let added: usize = pictures.iter().sum();
        if count + pictures.len() > super::io::MAX_IMAGES
            || total.saturating_add(added) > super::io::MAX_IMAGE_STORE_BYTES
        {
            return Err(ErrorCode::Limit);
        }
        let map = self.import_styles(&styles)?;
        for p in &mut pieces {
            if let Some(id) = &p.props.style {
                p.props.style = map.get(id).cloned();
            }
            for r in &mut p.runs {
                if let Some(id) = &r.attrs.char_style {
                    r.attrs.char_style = map.get(id).cloned();
                }
            }
        }
        let before = self.doc.revision();
        let outcome = self.doc.replace_with_paragraphs(start, end, &pieces)?;
        let mut delta = PageDelta::default();
        if outcome.revision != before {
            let more = self.cache.apply_edit(layout_doc!(self), &outcome);
            merge(&mut delta, more);
            let (anchor, focus) = outcome.selection;
            self.select(self.snap(anchor), self.snap(focus), false, None);
            self.origin.images = (count + pictures.len(), total + added);
        }
        self.finish(delta, DEFAULT_LAYOUT_BUDGET, true)
    }

    /// Word's rule for the styles of pasted text: a style the document has
    /// under the same name (and kind) is used, whatever its definition; an
    /// absent one is added with its definition. Returns the identifier in this
    /// document of every style of `styles`. The catalog is outside the edit
    /// history: an undone paste leaves its styles defined but unused (the save
    /// writes the styles the document uses).
    fn import_styles(&mut self, styles: &[Style]) -> Result<BTreeMap<String, String>, ErrorCode> {
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for style in styles {
            let catalog = LayoutContext::catalog(&self.doc);
            let same_name = catalog.styles().find(|t| {
                t.kind == style.kind && t.display_name().eq_ignore_ascii_case(style.display_name())
            });
            if let Some(found) = same_name {
                map.insert(style.id.clone(), found.id.clone());
                continue;
            }
            let mut id = style.id.clone();
            let mut n = 1u32;
            while catalog.style(&id).is_some() {
                n += 1;
                id = format!("{}_{n}", style.id);
            }
            let mut added = style.clone();
            added.id = id.clone();
            added.based_on = style.based_on.as_ref().and_then(|b| map.get(b).cloned());
            self.doc
                .add_style(added.clone())
                .map_err(|_| ErrorCode::Limit)?;
            self.added_styles.push(added);
            map.insert(style.id.clone(), id);
        }
        Ok(map)
    }
}

/// The text of pieces for a plain-text document: paragraphs joined by `\n`,
/// line breaks as `\n`, symbols as their character; pictures and page breaks
/// are left out.
fn plain_text(pieces: &[Paragraph]) -> String {
    let mut out = String::new();
    for (i, p) in pieces.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut marks: Vec<(usize, NodeId, Option<char>)> = Vec::new();
        marks.extend(p.breaks.iter().map(|b| {
            let c = matches!(b.kind, BreakKind::Line | BreakKind::Wrapping { .. }).then_some('\n');
            (b.offset, b.id, c)
        }));
        marks.extend(p.symbols.iter().map(|s| {
            let c = if (0xF000..=0xF0FF).contains(&s.code) {
                Some(char::from(s.code as u8))
            } else {
                char::from_u32(s.code)
            };
            (s.offset, s.id, c.filter(|c| !c.is_control() || *c == '\t'))
        }));
        marks.sort_by_key(|m| (m.0, m.1));
        let mut at = 0;
        for (offset, _, c) in marks {
            out.push_str(&p.text[at..offset]);
            at = offset;
            out.extend(c);
        }
        out.push_str(&p.text[at..]);
    }
    out
}
