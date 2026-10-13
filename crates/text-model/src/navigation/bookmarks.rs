// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmarks: insert or move, rename, delete, list (US-291).
//!
//! The model only forbids two bookmarks with the same *exact* name; the
//! case-insensitive uniqueness of Word is enforced here, on top of it.

use super::Reading;
use super::name::{
    NameError, NameMode, is_hidden_name, name_key, normalize_name, validate_bookmark_name,
};
use crate::rich::{Bookmark, Inline, Link, LinkTarget, NodeId, RichDocument, RichError, RichOp};
use crate::search::Zone;
use atelier_model::Transaction;
use atelier_model::selection::TextPosition;
use atelier_unicode::case::to_case_folded;
use atelier_unicode::normalization::nfd;
use atelier_unicode::properties::general_category;
use atelier_unicode::segmentation::grapheme_boundaries;
use core::fmt;
use std::collections::BTreeMap;

/// A bookmark and where it sits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkInfo {
    pub id: NodeId,
    pub name: String,
    pub paragraph: NodeId,
    /// Byte offsets in the paragraph text, `start <= end`.
    pub start: usize,
    pub end: usize,
    pub zone: Zone,
    pub section: usize,
    /// Rank of the paragraph in reading order.
    pub paragraph_index: usize,
    pub hidden: bool,
}

/// Sort order of [`list_bookmarks`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookmarkOrder {
    /// Alphabetical, ignoring case and accents (ties: reading order).
    Name,
    /// Reading order of the start.
    Position,
}

/// An internal link that targets the bookmark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkRef {
    pub paragraph: NodeId,
    pub link: NodeId,
    pub zone: Zone,
    pub section: usize,
}

/// A bookmark change, ready to be applied (`RichDocument::apply` or `History`).
#[derive(Clone)]
pub struct BookmarkEdit {
    pub transaction: Transaction<RichOp>,
    /// Rename: the internal links the transaction retargets. Delete: the
    /// internal links left as they are, now pointing nowhere. Empty otherwise.
    pub links: Vec<LinkRef>,
    /// The bookmark was found only up to case among several (see
    /// [`BookmarkMatch::ambiguous`]): the first in reading order was used.
    pub ambiguous: bool,
}

/// Why a bookmark change was refused.
#[derive(Debug, Eq, PartialEq)]
pub enum BookmarkError {
    Name(NameError),
    /// No bookmark has this name (case ignored).
    NotFound(String),
    /// Rename: another bookmark already has the new name.
    NameTaken(String),
    /// The model carries a bookmark in one paragraph only.
    RangeSpansParagraphs,
    /// The range or paragraph is invalid for the model.
    Model(RichError),
    /// More operations than a transaction can hold (rename with huge link count).
    TooLarge,
}

impl fmt::Display for BookmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(e) => e.fmt(f),
            Self::NotFound(n) => write!(f, "no bookmark named {n:?}"),
            Self::NameTaken(n) => write!(f, "the name {n:?} is already used"),
            Self::RangeSpansParagraphs => write!(f, "a bookmark cannot span several paragraphs"),
            Self::Model(e) => write!(f, "{e:?}"),
            Self::TooLarge => write!(f, "too many operations for one transaction"),
        }
    }
}
impl std::error::Error for BookmarkError {}

impl From<NameError> for BookmarkError {
    fn from(e: NameError) -> Self {
        Self::Name(e)
    }
}
impl From<RichError> for BookmarkError {
    fn from(e: RichError) -> Self {
        Self::Model(e)
    }
}

/// Every bookmark (hidden ones too) in reading order.
fn collect(doc: &RichDocument, reading: &Reading) -> Vec<BookmarkInfo> {
    let mut out = Vec::new();
    for (index, (id, zone, section)) in reading.paras.iter().enumerate() {
        let Ok(p) = doc.paragraph(*id) else { continue };
        for b in &p.bookmarks {
            out.push(BookmarkInfo {
                id: b.id,
                name: b.name.clone(),
                paragraph: *id,
                start: b.start,
                end: b.end,
                zone: *zone,
                section: *section,
                paragraph_index: index,
                hidden: is_hidden_name(&b.name),
            });
        }
    }
    out
}

/// Sort key of the alphabetical order: case folded, accents removed.
fn sort_key(name: &str) -> String {
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    nfd(&to_case_folded(&nfd(name)))
        .chars()
        .filter(|c| general_category(*c) != "Mn")
        .collect()
}

/// The bookmarks, hidden ones only when `include_hidden`.
pub fn list_bookmarks(
    doc: &RichDocument,
    order: BookmarkOrder,
    include_hidden: bool,
) -> Vec<BookmarkInfo> {
    list_with(doc, &Reading::new(doc), order, include_hidden)
}

pub(crate) fn list_with(
    doc: &RichDocument,
    reading: &Reading,
    order: BookmarkOrder,
    include_hidden: bool,
) -> Vec<BookmarkInfo> {
    let mut all = collect(doc, reading);
    all.retain(|b| include_hidden || !b.hidden);
    if order == BookmarkOrder::Name {
        // Stable: equal keys keep the reading order.
        all.sort_by_cached_key(|b| sort_key(&b.name));
    }
    all
}

/// A name lookup result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkMatch {
    pub info: BookmarkInfo,
    /// No bookmark has exactly this name and several differ only by case
    /// (possible in an imported document): the first in reading order was taken.
    pub ambiguous: bool,
}

/// The bookmark with this name: the exact name if one exists, else the first
/// in reading order equal up to case (`ambiguous` tells when several are).
/// Hidden bookmarks included.
pub fn lookup_bookmark(doc: &RichDocument, name: &str) -> Option<BookmarkMatch> {
    let all = collect(doc, &Reading::new(doc));
    let (i, ambiguous) = Resolver::new(&all).resolve(name)?;
    Some(BookmarkMatch {
        info: all.into_iter().nth(i)?,
        ambiguous,
    })
}

/// [`lookup_bookmark`] without the ambiguity flag.
pub fn find_bookmark(doc: &RichDocument, name: &str) -> Option<BookmarkInfo> {
    lookup_bookmark(doc, name).map(|m| m.info)
}

fn transaction(
    ops: Vec<RichOp>,
    links: Vec<LinkRef>,
    ambiguous: bool,
) -> Result<BookmarkEdit, BookmarkError> {
    let transaction = Transaction::new(ops).map_err(|_| BookmarkError::TooLarge)?;
    Ok(BookmarkEdit {
        transaction,
        links,
        ambiguous,
    })
}

/// Name lookup shared by every command: the **exact** name wins; without an
/// exact one, the case-insensitive match (first in reading order).
pub(crate) struct Resolver {
    exact: BTreeMap<String, usize>,
    keyed: BTreeMap<String, (usize, usize)>,
}

impl Resolver {
    pub(crate) fn new(all: &[BookmarkInfo]) -> Self {
        let mut exact = BTreeMap::new();
        let mut keyed: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (i, b) in all.iter().enumerate() {
            exact.entry(b.name.clone()).or_insert(i);
            keyed.entry(name_key(&b.name)).or_insert((i, 0)).1 += 1;
        }
        Self { exact, keyed }
    }

    /// Index in `all` and whether the choice is ambiguous (no exact match and
    /// several bookmarks equal up to case).
    pub(crate) fn resolve(&self, name: &str) -> Option<(usize, bool)> {
        if let Some(i) = self.exact.get(name) {
            return Some((*i, false));
        }
        let composed = normalize_name(name);
        if let Some(i) = self.exact.get(&composed) {
            return Some((*i, false));
        }
        self.keyed
            .get(&name_key(name))
            .map(|(i, count)| (*i, *count > 1))
    }
}

/// Internal links (and the paragraph they sit in) that resolve to bookmark `target`.
fn links_to(
    doc: &RichDocument,
    reading: &Reading,
    resolver: &Resolver,
    target: usize,
) -> Vec<(LinkRef, Link)> {
    let mut out = Vec::new();
    for (id, zone, section) in &reading.paras {
        let Ok(p) = doc.paragraph(*id) else { continue };
        for l in &p.links {
            if matches!(&l.target, LinkTarget::Bookmark(n)
                if resolver.resolve(n).is_some_and(|(i, _)| i == target))
            {
                let link = LinkRef {
                    paragraph: *id,
                    link: l.id,
                    zone: *zone,
                    section: *section,
                };
                out.push((link, l.clone()));
            }
        }
    }
    out
}

/// Bookmarks `start..end` (in one paragraph; reversed ends are swapped; equal
/// ends make a point bookmark). The name is normalized to NFC, then must
/// follow `mode`. If a bookmark already has this name (exact match first, else
/// case ignored) it is **moved**, as in Word: same id, same stored spelling,
/// so links to it stay valid. A range over several paragraphs is refused
/// (`RangeSpansParagraphs`), never silently shortened.
pub fn insert_bookmark(
    doc: &mut RichDocument,
    name: &str,
    start: TextPosition,
    end: TextPosition,
    mode: NameMode,
) -> Result<BookmarkEdit, BookmarkError> {
    validate_bookmark_name(name, mode)?;
    let name = normalize_name(name);
    if start.node != end.node {
        return Err(BookmarkError::RangeSpansParagraphs);
    }
    let (s, e) = (start.offset.min(end.offset), start.offset.max(end.offset));
    let para = start.node;
    {
        let p = doc.paragraph(para)?;
        let len = p.text.len();
        let index = grapheme_boundaries(&p.text);
        for offset in [s, e] {
            if offset > len {
                return Err(RichError::OffsetOutOfBounds { offset, len }.into());
            }
            if !matches!(index.is_boundary(offset), Ok(true)) {
                return Err(RichError::NotGraphemeBoundary { offset }.into());
            }
        }
    }
    let all = collect(doc, &Reading::new(doc));
    let found = Resolver::new(&all).resolve(&name);
    let ambiguous = found.is_some_and(|f| f.1);
    let ops = match found.map(|f| &all[f.0]) {
        None => {
            let id = doc.alloc_id()?;
            vec![RichOp::InsertInline {
                para,
                item: Inline::Bookmark(Bookmark {
                    id,
                    name,
                    start: s,
                    end: e,
                }),
            }]
        }
        Some(old) => {
            let moved = Bookmark {
                id: old.id,
                name: old.name.clone(),
                start: s,
                end: e,
            };
            if old.paragraph == para {
                vec![RichOp::ModifyInline {
                    para,
                    item: Inline::Bookmark(moved),
                }]
            } else {
                vec![
                    RichOp::RemoveInline {
                        para: old.paragraph,
                        id: old.id,
                    },
                    RichOp::InsertInline {
                        para,
                        item: Inline::Bookmark(moved),
                    },
                ]
            }
        }
    };
    transaction(ops, Vec::new(), ambiguous)
}

/// Renames `old` (exact name first, else case ignored) to `new` (NFC, follows
/// `mode`). Internal links that resolve to the bookmark are retargeted in the
/// same transaction and listed in `links`. Any **other** bookmark with the new
/// name, exactly or up to case, is `NameTaken` (checked before the model could
/// refuse it); changing only the case of the bookmark's own name is allowed;
/// the same name gives an empty transaction.
pub fn rename_bookmark(
    doc: &RichDocument,
    old: &str,
    new: &str,
    mode: NameMode,
) -> Result<BookmarkEdit, BookmarkError> {
    validate_bookmark_name(new, mode)?;
    let new = normalize_name(new);
    let reading = Reading::new(doc);
    let all = collect(doc, &reading);
    let resolver = Resolver::new(&all);
    let (idx, ambiguous) = resolver
        .resolve(old)
        .ok_or_else(|| BookmarkError::NotFound(old.to_owned()))?;
    let found = &all[idx];
    let new_key = name_key(&new);
    if all
        .iter()
        .enumerate()
        .any(|(i, b)| i != idx && (b.name == new || name_key(&b.name) == new_key))
    {
        return Err(BookmarkError::NameTaken(new));
    }
    if found.name == new {
        return transaction(Vec::new(), Vec::new(), ambiguous);
    }
    let mut ops = vec![RichOp::ModifyInline {
        para: found.paragraph,
        item: Inline::Bookmark(Bookmark {
            id: found.id,
            name: new.clone(),
            start: found.start,
            end: found.end,
        }),
    }];
    let mut refs = Vec::new();
    for (r, mut link) in links_to(doc, &reading, &resolver, idx) {
        link.target = LinkTarget::Bookmark(new.clone());
        ops.push(RichOp::ModifyInline {
            para: r.paragraph,
            item: Inline::Link(link),
        });
        refs.push(r);
    }
    transaction(ops, refs, ambiguous)
}

/// Deletes the bookmark `name` (exact name first, else case ignored). Internal
/// links to it are **left unchanged** (they now point nowhere, as in Word) and
/// returned in `links` so the caller can warn; undoing restores the bookmark
/// and they work again.
pub fn delete_bookmark(doc: &RichDocument, name: &str) -> Result<BookmarkEdit, BookmarkError> {
    let reading = Reading::new(doc);
    let all = collect(doc, &reading);
    let resolver = Resolver::new(&all);
    let (idx, ambiguous) = resolver
        .resolve(name)
        .ok_or_else(|| BookmarkError::NotFound(name.to_owned()))?;
    let links = links_to(doc, &reading, &resolver, idx)
        .into_iter()
        .map(|(r, _)| r)
        .collect();
    transaction(
        vec![RichOp::RemoveInline {
            para: all[idx].paragraph,
            id: all[idx].id,
        }],
        links,
        ambiguous,
    )
}
