// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Go To command: resolves a target to a document position (US-291).
//!
//! Every target kind is a numbered sequence of *starts*; a position `cur` sits
//! after `c1` of them (the starts at or before it). "Next" is element `c1+1`,
//! "Previous" the last start strictly before `cur`. `+n` repeats Next `n`
//! times, `-n` repeats Previous `n` times, `n` is absolute.

use super::bookmarks::{BookmarkInfo, BookmarkOrder, Resolver, list_with};
use super::pages::PageMap;
use super::{Direction, Reading};
use crate::rich::{Node, NodeId, RichDocument, StyleCatalog};
use crate::search::Zone;
use atelier_model::selection::{Affinity, TextPosition};
use core::fmt;

/// What the dialog goes to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GotoKind {
    /// From the layout ([`PageMap`]).
    Page,
    /// Starts at the first paragraph of the section body.
    Section,
    /// From the layout; numbered over the whole document.
    Line,
    /// Input is a name, or a number/sign in the list of visible bookmarks by position.
    Bookmark,
    /// Tables of the body, nested ones included, in reading order.
    Table,
    /// Anchored objects (pictures, shapes...) of the body and its tables, in
    /// reading order. Word's "Graphic" and "Object" are this one kind.
    Object,
    /// Paragraphs of a heading style (see [`is_heading_style`]), body and tables.
    Heading,
}

/// A parsed input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GotoAmount {
    Absolute(u64),
    /// `+3`, `-2`; the empty input is `Relative(1)`.
    Relative(i64),
    /// `p3s2`: page 3 of section 2 (pages only).
    PageInSection {
        page: u64,
        section: u64,
    },
}

/// An input that is not a number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    /// Anything but `[+|-] digits` (or `p<n>s<m>` for pages).
    InvalidNumber,
    /// More than [`MAX_AMOUNT`].
    TooLarge,
}

/// Largest accepted number.
pub const MAX_AMOUNT: u64 = 1_000_000_000;

/// Parses `12`, `+3`, `-2`, empty (next) and, for pages, `p3s2`. Spaces
/// around, after a sign and inside `p3s2` are tolerated (`1 2` is not a
/// number); case is ignored.
pub fn parse_amount(kind: GotoKind, input: &str) -> Result<GotoAmount, ParseError> {
    let number = |t: &str| -> Result<u64, ParseError> {
        if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseError::InvalidNumber);
        }
        match t.parse::<u64>() {
            Ok(n) if n <= MAX_AMOUNT => Ok(n),
            _ => Err(ParseError::TooLarge),
        }
    };
    let s = input.trim();
    if s.is_empty() {
        return Ok(GotoAmount::Relative(1));
    }
    if s.starts_with(['p', 'P']) {
        let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let rest = compact[1..].to_ascii_lowercase();
        let (page, section) = rest.split_once('s').ok_or(ParseError::InvalidNumber)?;
        if kind != GotoKind::Page {
            return Err(ParseError::InvalidNumber);
        }
        return Ok(GotoAmount::PageInSection {
            page: number(page)?,
            section: number(section)?,
        });
    }
    if let Some(t) = s.strip_prefix('+') {
        return Ok(GotoAmount::Relative(number(t.trim_start())? as i64));
    }
    if let Some(t) = s.strip_prefix('-') {
        return Ok(GotoAmount::Relative(-(number(t.trim_start())? as i64)));
    }
    Ok(GotoAmount::Absolute(number(s)?))
}

/// The target was past an end: Word goes to the nearest element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Clamp {
    /// Beyond the end: went to the last element.
    ToLast,
    /// Before the start (or `0`): went to the first element.
    ToFirst,
}

/// Where to put the caret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Landing {
    pub position: TextPosition,
    /// A bookmark over a range: the other end of the range to select.
    pub selection_end: Option<TextPosition>,
    pub zone: Zone,
    pub section: usize,
    /// 1-based rank of the element reached among `count`.
    pub index: usize,
    pub count: usize,
    pub clamp: Option<Clamp>,
    /// Bookmark reached by name: no exact match, several up to case; the
    /// first in reading order was taken.
    pub ambiguous: bool,
}

/// Result of a Go To.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GotoOutcome {
    Reached(Landing),
    /// The document has no element of this kind.
    NoElements,
}

/// Why a Go To cannot even be attempted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GotoError {
    Parse(ParseError),
    BookmarkNotFound(String),
    /// The current position (or one given by the layout) is not in the document.
    UnknownPosition,
    /// The `PageMap` contradicts itself or the document (a start that does
    /// not exist, a position outside the document).
    InconsistentPageMap,
}

impl fmt::Display for GotoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "invalid Go To input: {e:?}"),
            Self::BookmarkNotFound(n) => write!(f, "no bookmark named {n:?}"),
            Self::UnknownPosition => write!(f, "position outside the document"),
            Self::InconsistentPageMap => write!(f, "inconsistent page map"),
        }
    }
}
impl std::error::Error for GotoError {}

impl From<ParseError> for GotoError {
    fn from(e: ParseError) -> Self {
        Self::Parse(e)
    }
}

/// Heading style identifiers: `atelier.heading.1`..`9`, and the docx-style
/// `Heading1`, `heading 2`, `Titre3` (case ignored). This reads one
/// identifier only: without a style catalogue, inheritance (a style based on
/// a heading) is **not** seen; this is an approximation. Use
/// [`is_heading_style_in`] / [`goto_with_styles`] when the catalogue is known.
pub fn is_heading_style(style: &str) -> bool {
    let lower = style.to_lowercase();
    let rest = lower
        .strip_prefix("atelier.heading.")
        .or_else(|| lower.strip_prefix("heading"))
        .or_else(|| lower.strip_prefix("titre"));
    matches!(rest.map(|r| r.trim_start().as_bytes()), Some([b'1'..=b'9']))
}

/// Longest `based_on` chain followed by [`is_heading_style_in`]; it also ends
/// any cycle.
pub const MAX_STYLE_CHAIN: usize = 64;

/// Whether the style, or one of the styles it is (transitively) based on, is a
/// heading style ([`is_heading_style`]). The chain is followed at most
/// [`MAX_STYLE_CHAIN`] steps, so a cycle or a missing style ends it with
/// `false`. The model has no outline level stored on the paragraph itself
/// (`ParaProps` has none), so only styles define headings here.
pub fn is_heading_style_in(style: &str, catalog: &StyleCatalog) -> bool {
    let mut id = style;
    for _ in 0..MAX_STYLE_CHAIN {
        if is_heading_style(id) {
            return true;
        }
        match catalog.style(id).and_then(|s| s.based_on.as_deref()) {
            Some(next) => id = next,
            None => return false,
        }
    }
    false
}

#[derive(Clone, Copy)]
struct Place {
    position: TextPosition,
    end: Option<TextPosition>,
}

fn place(position: TextPosition) -> Place {
    Place {
        position,
        end: None,
    }
}

fn at(node: NodeId, offset: usize) -> TextPosition {
    TextPosition::new(node, offset, Affinity::After)
}

fn first_paragraph(doc: &RichDocument, mut id: NodeId) -> Option<NodeId> {
    loop {
        match doc.node(id)? {
            Node::Paragraph(_) => return Some(id),
            node => id = *node.children().first()?,
        }
    }
}

fn in_body(zone: Zone) -> bool {
    matches!(zone, Zone::Body | Zone::Table)
}

fn sections(doc: &RichDocument, reading: &Reading) -> Vec<Place> {
    let mut seen = vec![false; doc.section_ids().len()];
    let mut out = Vec::new();
    for (id, zone, section) in &reading.paras {
        if in_body(*zone) && !std::mem::replace(&mut seen[*section], true) {
            out.push(place(at(*id, 0)));
        }
    }
    out
}

fn tables(doc: &RichDocument) -> Vec<Place> {
    let mut out = Vec::new();
    for section in doc.section_ids() {
        let Some(Node::Section(s)) = doc.node(section) else {
            continue;
        };
        // Explicit stack: nesting depth is not bounded by the model.
        let mut stack: Vec<NodeId> = s.body.to_vec().into_iter().rev().collect();
        while let Some(id) = stack.pop() {
            let Some(node) = doc.node(id) else { continue };
            if matches!(node, Node::Table(_))
                && let Some(p) = first_paragraph(doc, id)
            {
                out.push(place(at(p, 0)));
            }
            if matches!(node, Node::Table(_) | Node::Row(_) | Node::Cell(_)) {
                stack.extend(node.children().into_iter().rev());
            }
        }
    }
    out
}

fn paragraph_items(
    doc: &RichDocument,
    reading: &Reading,
    kind: GotoKind,
    styles: Option<&StyleCatalog>,
) -> Vec<Place> {
    let is_heading = |style: &str| match styles {
        Some(catalog) => is_heading_style_in(style, catalog),
        None => is_heading_style(style),
    };
    let mut out = Vec::new();
    for (id, zone, _) in &reading.paras {
        let (true, Ok(p)) = (in_body(*zone), doc.paragraph(*id)) else {
            continue;
        };
        if kind == GotoKind::Object {
            out.extend(p.anchored.iter().map(|o| place(at(*id, o.offset))));
        } else if p.props.style.as_deref().is_some_and(is_heading) {
            out.push(place(at(*id, 0)));
        }
    }
    out
}

fn bookmark_place(b: &BookmarkInfo) -> Place {
    Place {
        position: at(b.paragraph, b.start),
        end: (b.end > b.start).then(|| TextPosition::new(b.paragraph, b.end, Affinity::Before)),
    }
}

/// A numbered sequence of starts, in reading order. Several elements may
/// start at the same position.
struct Seq<'a> {
    count: usize,
    get: Box<dyn Fn(usize) -> Option<Place> + 'a>,
    /// Number of starts at or before the current position.
    after: usize,
    /// Number of starts strictly before it.
    before: usize,
}

fn list_seq<'a>(places: Vec<Place>, reading: &Reading, cur: (usize, usize)) -> Seq<'a> {
    let key = |p: &Place| reading.key(p.position);
    let before = places.partition_point(|p| key(p).is_some_and(|k| k < cur));
    let after = places.partition_point(|p| key(p).is_some_and(|k| k <= cur));
    Seq {
        count: places.len(),
        get: Box::new(move |i| places.get(i.checked_sub(1)?).copied()),
        after,
        before,
    }
}

fn layout_seq<'a>(
    kind: GotoKind,
    pages: &'a dyn PageMap,
    reading: &Reading,
    from: TextPosition,
    cur: (usize, usize),
) -> Seq<'a> {
    let line = kind == GotoKind::Line;
    let count = if line {
        pages.line_count()
    } else {
        pages.page_count()
    };
    let rank = if line {
        pages.line_of(from)
    } else {
        pages.page_of(from)
    };
    let get = move |i: usize| {
        if line {
            pages.line_start(i)
        } else {
            pages.page_start(i)
        }
        .map(place)
    };
    let after = rank.min(count);
    let starts_here = after > 0 && get(after).and_then(|p| reading.key(p.position)) == Some(cur);
    Seq {
        count,
        get: Box::new(get),
        after,
        before: if starts_here { after - 1 } else { after },
    }
}

/// 1-based index reached and the clamp, for a request on `seq`. `hint` is the
/// rank of the element the caret was last put on, honoured only if that
/// element starts at the caret. All arithmetic is done in `i128`.
fn pick(seq: &Seq<'_>, amount: GotoAmount, hint: Option<usize>) -> (usize, Option<Clamp>) {
    let (mut after, mut before) = (seq.after as i128, seq.before as i128);
    if let Some(r) = hint.filter(|r| *r > seq.before && *r <= seq.after) {
        after = r as i128;
        before = after - 1;
    }
    let target = match amount {
        GotoAmount::Absolute(n) => i128::from(n),
        GotoAmount::Relative(0) => after,
        GotoAmount::Relative(d) if d > 0 => after + i128::from(d),
        GotoAmount::Relative(d) => before + i128::from(d) + 1,
        GotoAmount::PageInSection { page, .. } => i128::from(page),
    };
    clamp_index(target, seq.count)
}

/// `target` brought into `1..=count` (`count >= 1`).
fn clamp_index(target: i128, count: usize) -> (usize, Option<Clamp>) {
    if target < 1 {
        (1, Some(Clamp::ToFirst))
    } else if target > count as i128 {
        (count, Some(Clamp::ToLast))
    } else {
        (target as usize, None)
    }
}

fn landing(
    reading: &Reading,
    place: Place,
    index: usize,
    count: usize,
    clamp: Option<Clamp>,
) -> Result<GotoOutcome, GotoError> {
    let rank = reading
        .rank(place.position.node)
        .ok_or(GotoError::UnknownPosition)?;
    let (_, zone, section) = reading.paras[rank];
    Ok(GotoOutcome::Reached(Landing {
        position: place.position,
        selection_end: place.end,
        zone,
        section,
        index,
        count,
        clamp,
        ambiguous: false,
    }))
}

/// `p<page>s<section>`: page `page` counted from the first page of the
/// section. A `PageMap` that contradicts itself (page numbers out of
/// `1..=page_count`, missing starts) never panics: numbers are brought into
/// range, and a missing start is `InconsistentPageMap`.
fn page_in_section(
    doc: &RichDocument,
    pages: &dyn PageMap,
    reading: &Reading,
    page: u64,
    section: u64,
) -> Result<GotoOutcome, GotoError> {
    let starts = sections(doc, reading);
    let total = pages.page_count();
    if starts.is_empty() || total == 0 {
        return Ok(GotoOutcome::NoElements);
    }
    let (s, s_clamp) = clamp_index(i128::from(section), starts.len());
    let first = pages.page_of(starts[s - 1].position).clamp(1, total);
    let last = match starts.get(s) {
        Some(next) => {
            let np = pages.page_of(next.position).clamp(1, total);
            let begins_page =
                pages.page_start(np).and_then(|p| reading.key(p)) == reading.key(next.position);
            if begins_page { np - 1 } else { np }
        }
        None => total,
    }
    .max(first);
    let (n, p_clamp) = clamp_index(i128::from(page), last - first + 1);
    let target = first + (n - 1);
    let start = pages
        .page_start(target)
        .ok_or(GotoError::InconsistentPageMap)?;
    landing(reading, place(start), target, total, s_clamp.or(p_clamp))
        .map_err(|_| GotoError::InconsistentPageMap)
}

/// Where a request starts: the caret and, optionally, the rank of the element
/// of the requested kind the caret was last put on by Go To.
///
/// Several elements can start at the same position (a table nested at the
/// start of the first cell of another, objects anchored at the same offset).
/// Given the position alone, the caret counts as being on the last of them;
/// with `rank` set to `Landing::index` of the previous result, "Next" and
/// "Previous" step through them one by one. The rank is ignored when the
/// element of that rank does not start at the caret (the caret moved).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GotoOrigin {
    pub position: TextPosition,
    pub rank: Option<usize>,
}

impl From<TextPosition> for GotoOrigin {
    fn from(position: TextPosition) -> Self {
        Self {
            position,
            rank: None,
        }
    }
}

impl Landing {
    /// The origin for the next request of the same kind, if the caret stays.
    pub fn origin(&self) -> GotoOrigin {
        GotoOrigin {
            position: self.position,
            rank: Some(self.index),
        }
    }
}

struct Ctx<'a> {
    doc: &'a RichDocument,
    pages: &'a dyn PageMap,
    reading: Reading,
    styles: Option<&'a StyleCatalog>,
}

/// Resolves a Go To request from `from` (a `TextPosition` or a [`GotoOrigin`]).
///
/// `input` is parsed by [`parse_amount`]. For [`GotoKind::Bookmark`] the text
/// is first looked up as a **name** (exact name first, else case ignored,
/// hidden bookmarks reachable; `Landing::ambiguous` is set when several
/// bookmarks differ only by case and the first was taken); only when no
/// bookmark has that name is it read as a number. An unknown name starting
/// with a letter or `_` is `BookmarkNotFound`. A kind without any element is
/// [`GotoOutcome::NoElements`]; beyond the ends the nearest element is
/// reached and `clamp` says so. Headings are recognized from the paragraph's
/// own style only (see [`is_heading_style`]); [`goto_with_styles`] follows
/// style inheritance. Linear in the document size.
pub fn goto(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: impl Into<GotoOrigin>,
    kind: GotoKind,
    input: &str,
) -> Result<GotoOutcome, GotoError> {
    goto_impl(doc, pages, from.into(), kind, input, None)
}

/// [`goto`] with the style catalogue, so that a style based on a heading style
/// (directly or through a chain) counts as a heading.
pub fn goto_with_styles(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: impl Into<GotoOrigin>,
    kind: GotoKind,
    input: &str,
    styles: &StyleCatalog,
) -> Result<GotoOutcome, GotoError> {
    goto_impl(doc, pages, from.into(), kind, input, Some(styles))
}

/// The "Next" and "Previous" buttons: `+1` and `-1`.
pub fn step(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: impl Into<GotoOrigin>,
    kind: GotoKind,
    direction: Direction,
) -> Result<GotoOutcome, GotoError> {
    step_impl(doc, pages, from.into(), kind, direction, None)
}

/// [`step`] with the style catalogue (see [`goto_with_styles`]).
pub fn step_with_styles(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: impl Into<GotoOrigin>,
    kind: GotoKind,
    direction: Direction,
    styles: &StyleCatalog,
) -> Result<GotoOutcome, GotoError> {
    step_impl(doc, pages, from.into(), kind, direction, Some(styles))
}

fn context<'a>(
    doc: &'a RichDocument,
    pages: &'a dyn PageMap,
    styles: Option<&'a StyleCatalog>,
) -> Ctx<'a> {
    Ctx {
        doc,
        pages,
        reading: Reading::new(doc),
        styles,
    }
}

fn step_impl(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: GotoOrigin,
    kind: GotoKind,
    direction: Direction,
    styles: Option<&StyleCatalog>,
) -> Result<GotoOutcome, GotoError> {
    let d = if direction == Direction::Next { 1 } else { -1 };
    resolve(
        &context(doc, pages, styles),
        from,
        kind,
        GotoAmount::Relative(d),
    )
}

fn goto_impl(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: GotoOrigin,
    kind: GotoKind,
    input: &str,
    styles: Option<&StyleCatalog>,
) -> Result<GotoOutcome, GotoError> {
    let ctx = context(doc, pages, styles);
    let trimmed = input.trim();
    if kind == GotoKind::Bookmark && !trimmed.is_empty() {
        let all = list_with(doc, &ctx.reading, BookmarkOrder::Position, true);
        if let Some((i, ambiguous)) = Resolver::new(&all).resolve(trimmed) {
            return goto_name(&ctx.reading, &all, i, ambiguous);
        }
        if trimmed.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            return Err(GotoError::BookmarkNotFound(trimmed.to_owned()));
        }
    }
    let amount = parse_amount(kind, input)?;
    resolve(&ctx, from, kind, amount)
}

fn goto_name(
    reading: &Reading,
    all: &[BookmarkInfo],
    found: usize,
    ambiguous: bool,
) -> Result<GotoOutcome, GotoError> {
    let found = &all[found];
    let visible: Vec<&BookmarkInfo> = all.iter().filter(|b| !b.hidden).collect();
    let index = visible
        .iter()
        .position(|b| b.id == found.id)
        .map_or(0, |i| i + 1);
    let mut out = landing(reading, bookmark_place(found), index, visible.len(), None)?;
    if let GotoOutcome::Reached(l) = &mut out {
        l.ambiguous = ambiguous;
    }
    Ok(out)
}

fn resolve(
    ctx: &Ctx<'_>,
    from: GotoOrigin,
    kind: GotoKind,
    amount: GotoAmount,
) -> Result<GotoOutcome, GotoError> {
    let Ctx {
        doc,
        pages,
        reading,
        styles,
    } = ctx;
    let cur = reading
        .key(from.position)
        .ok_or(GotoError::UnknownPosition)?;
    if let GotoAmount::PageInSection { page, section } = amount {
        return page_in_section(doc, *pages, reading, page, section);
    }
    let layout = matches!(kind, GotoKind::Page | GotoKind::Line);
    let seq = match kind {
        GotoKind::Page | GotoKind::Line => layout_seq(kind, *pages, reading, from.position, cur),
        GotoKind::Section => list_seq(sections(doc, reading), reading, cur),
        GotoKind::Table => list_seq(tables(doc), reading, cur),
        GotoKind::Object | GotoKind::Heading => {
            list_seq(paragraph_items(doc, reading, kind, *styles), reading, cur)
        }
        GotoKind::Bookmark => {
            let all = list_with(doc, reading, BookmarkOrder::Position, false);
            list_seq(all.iter().map(bookmark_place).collect(), reading, cur)
        }
    };
    if seq.count == 0 {
        return Ok(GotoOutcome::NoElements);
    }
    let (index, clamp) = pick(&seq, amount, from.rank);
    // A position or a start unknown to the document can only come from the layout.
    let fix = |e: GotoError| {
        if layout && e == GotoError::UnknownPosition {
            GotoError::InconsistentPageMap
        } else {
            e
        }
    };
    let found = (seq.get)(index)
        .ok_or(GotoError::UnknownPosition)
        .map_err(fix)?;
    landing(reading, found, index, seq.count, clamp).map_err(fix)
}
