// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Go To command: resolves a target to a document position (US-291).
//!
//! Every target kind is a numbered sequence of *starts*; a position `cur` sits
//! after `c1` of them (the starts at or before it). "Next" is element `c1+1`,
//! "Previous" the last start strictly before `cur`. `+n` repeats Next `n`
//! times, `-n` repeats Previous `n` times, `n` is absolute.

use super::bookmarks::{BookmarkInfo, BookmarkOrder, list_with};
use super::name::name_key;
use super::pages::PageMap;
use super::{Direction, Reading};
use crate::rich::{Node, NodeId, RichDocument};
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
}

impl fmt::Display for GotoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "invalid Go To input: {e:?}"),
            Self::BookmarkNotFound(n) => write!(f, "no bookmark named {n:?}"),
            Self::UnknownPosition => write!(f, "position outside the document"),
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
/// `Heading1`, `heading 2`, `Titre3` (case ignored). Hypothesis: the style
/// is read from the paragraph itself, not inherited through a style chain.
pub fn is_heading_style(style: &str) -> bool {
    let lower = style.to_lowercase();
    let rest = lower
        .strip_prefix("atelier.heading.")
        .or_else(|| lower.strip_prefix("heading"))
        .or_else(|| lower.strip_prefix("titre"));
    matches!(rest.map(|r| r.trim_start().as_bytes()), Some([b'1'..=b'9']))
}

#[derive(Clone, Copy)]
struct Place {
    position: TextPosition,
    end: Option<TextPosition>,
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
            out.push(Place {
                position: at(*id, 0),
                end: None,
            });
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
                out.push(Place {
                    position: at(p, 0),
                    end: None,
                });
            }
            if matches!(node, Node::Table(_) | Node::Row(_) | Node::Cell(_)) {
                stack.extend(node.children().into_iter().rev());
            }
        }
    }
    out
}

fn paragraph_items(doc: &RichDocument, reading: &Reading, kind: GotoKind) -> Vec<Place> {
    let mut out = Vec::new();
    for (id, zone, _) in &reading.paras {
        let (true, Ok(p)) = (in_body(*zone), doc.paragraph(*id)) else {
            continue;
        };
        if kind == GotoKind::Object {
            out.extend(p.anchored.iter().map(|o| Place {
                position: at(*id, o.offset),
                end: None,
            }));
        } else if p.props.style.as_deref().is_some_and(is_heading_style) {
            out.push(Place {
                position: at(*id, 0),
                end: None,
            });
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

/// A numbered sequence of starts, in reading order.
struct Seq<'a> {
    count: usize,
    get: Box<dyn Fn(usize) -> Option<Place> + 'a>,
    /// Number of starts at or before the current position.
    after: usize,
}

fn list_seq<'a>(places: Vec<Place>, reading: &Reading, cur: (usize, usize)) -> Seq<'a> {
    let after = places.partition_point(|p| reading.key(p.position).is_some_and(|k| k <= cur));
    Seq {
        count: places.len(),
        get: Box::new(move |i| places.get(i.checked_sub(1)?).copied()),
        after,
    }
}

fn layout_seq<'a>(kind: GotoKind, pages: &'a dyn PageMap, from: TextPosition) -> Seq<'a> {
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
    Seq {
        count,
        get: Box::new(move |i| {
            let p = if line {
                pages.line_start(i)
            } else {
                pages.page_start(i)
            };
            p.map(|position| Place {
                position,
                end: None,
            })
        }),
        after: rank.min(count),
    }
}

/// 1-based index reached and the clamp, for a request on `seq`.
fn pick(
    seq: &Seq<'_>,
    amount: GotoAmount,
    reading: &Reading,
    cur: (usize, usize),
) -> (usize, Option<Clamp>) {
    let after = seq.after as i64;
    // Starts strictly before the position.
    let before = match (seq.get)(seq.after).and_then(|p| reading.key(p.position)) {
        Some(k) if seq.after > 0 && k == cur => after - 1,
        _ => after,
    };
    let target = match amount {
        GotoAmount::Absolute(n) => n as i64,
        GotoAmount::Relative(0) => after,
        GotoAmount::Relative(d) if d > 0 => after + d,
        GotoAmount::Relative(d) => before + d + 1,
        GotoAmount::PageInSection { page, .. } => page as i64,
    };
    clamp_index(target, seq.count)
}

fn clamp_index(target: i64, count: usize) -> (usize, Option<Clamp>) {
    if target < 1 {
        (1, Some(Clamp::ToFirst))
    } else if target > count as i64 {
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
    }))
}

/// `p<page>s<section>`: page `page` counted from the first page of the section.
fn page_in_section(
    doc: &RichDocument,
    pages: &dyn PageMap,
    reading: &Reading,
    page: u64,
    section: u64,
) -> Result<GotoOutcome, GotoError> {
    let starts = sections(doc, reading);
    if starts.is_empty() || pages.page_count() == 0 {
        return Ok(GotoOutcome::NoElements);
    }
    let (s, s_clamp) = clamp_index(section as i64, starts.len());
    let first = pages.page_of(starts[s - 1].position).max(1);
    let last = match starts.get(s) {
        Some(next) => {
            let np = pages.page_of(next.position).max(1);
            let starts_page =
                pages.page_start(np).and_then(|p| reading.key(p)) == reading.key(next.position);
            (if starts_page { np - 1 } else { np }).max(first)
        }
        None => pages.page_count(),
    };
    let (n, p_clamp) = clamp_index(page as i64, last - first + 1);
    let target = first + n - 1;
    let place = pages.page_start(target).ok_or(GotoError::UnknownPosition)?;
    landing(
        reading,
        Place {
            position: place,
            end: None,
        },
        target,
        pages.page_count(),
        s_clamp.or(p_clamp),
    )
}

/// Resolves a Go To request from the position `from`.
///
/// `input` is parsed by [`parse_amount`]; for [`GotoKind::Bookmark`] a text
/// that starts with a letter or an underscore is a bookmark name (case
/// ignored, hidden bookmarks reachable), anything else is a number as above.
/// An unknown name is an error; a kind without any element is
/// [`GotoOutcome::NoElements`]; beyond the ends the nearest element is reached
/// and `clamp` says so. Linear in the document size.
pub fn goto(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: TextPosition,
    kind: GotoKind,
    input: &str,
) -> Result<GotoOutcome, GotoError> {
    let reading = Reading::new(doc);
    let trimmed = input.trim();
    if kind == GotoKind::Bookmark && trimmed.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return goto_name(doc, &reading, trimmed);
    }
    let amount = parse_amount(kind, input)?;
    resolve(doc, pages, &reading, from, kind, amount)
}

/// The "Next" and "Previous" buttons: `+1` and `-1`.
pub fn step(
    doc: &RichDocument,
    pages: &dyn PageMap,
    from: TextPosition,
    kind: GotoKind,
    direction: Direction,
) -> Result<GotoOutcome, GotoError> {
    let d = if direction == Direction::Next { 1 } else { -1 };
    resolve(
        doc,
        pages,
        &Reading::new(doc),
        from,
        kind,
        GotoAmount::Relative(d),
    )
}

fn goto_name(doc: &RichDocument, reading: &Reading, name: &str) -> Result<GotoOutcome, GotoError> {
    let key = name_key(name);
    let all = list_with(doc, reading, BookmarkOrder::Position, true);
    let found = all
        .iter()
        .find(|b| name_key(&b.name) == key)
        .ok_or_else(|| GotoError::BookmarkNotFound(name.to_owned()))?;
    let visible: Vec<&BookmarkInfo> = all.iter().filter(|b| !b.hidden).collect();
    let index = visible
        .iter()
        .position(|b| b.id == found.id)
        .map_or(0, |i| i + 1);
    landing(reading, bookmark_place(found), index, visible.len(), None)
}

fn resolve(
    doc: &RichDocument,
    pages: &dyn PageMap,
    reading: &Reading,
    from: TextPosition,
    kind: GotoKind,
    amount: GotoAmount,
) -> Result<GotoOutcome, GotoError> {
    let cur = reading.key(from).ok_or(GotoError::UnknownPosition)?;
    if let GotoAmount::PageInSection { page, section } = amount {
        return page_in_section(doc, pages, reading, page, section);
    }
    let seq = match kind {
        GotoKind::Page | GotoKind::Line => layout_seq(kind, pages, from),
        GotoKind::Section => list_seq(sections(doc, reading), reading, cur),
        GotoKind::Table => list_seq(tables(doc), reading, cur),
        GotoKind::Object | GotoKind::Heading => {
            list_seq(paragraph_items(doc, reading, kind), reading, cur)
        }
        GotoKind::Bookmark => {
            let all = list_with(doc, reading, BookmarkOrder::Position, false);
            list_seq(all.iter().map(bookmark_place).collect(), reading, cur)
        }
    };
    if seq.count == 0 {
        return Ok(GotoOutcome::NoElements);
    }
    let (index, clamp) = pick(&seq, amount, reading, cur);
    let place = (seq.get)(index).ok_or(GotoError::UnknownPosition)?;
    landing(reading, place, index, seq.count, clamp)
}
