// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmarks: insert or move, rename, delete, list (US-291).
//!
//! The model only forbids two bookmarks with the same *exact* name; the
//! case-insensitive uniqueness of Word is enforced here, on top of it.

use super::Reading;
use super::name::{NameError, NameMode, is_hidden_name, name_key, validate_bookmark_name};
use crate::rich::{Bookmark, Inline, Link, LinkTarget, NodeId, RichDocument, RichError, RichOp};
use crate::search::Zone;
use atelier_model::Transaction;
use atelier_model::selection::TextPosition;
use atelier_unicode::case::to_case_folded;
use atelier_unicode::normalization::nfd;
use atelier_unicode::properties::general_category;
use atelier_unicode::segmentation::grapheme_boundaries;
use core::fmt;

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

/// The bookmark with this name, case ignored; hidden ones included.
pub fn find_bookmark(doc: &RichDocument, name: &str) -> Option<BookmarkInfo> {
    let key = name_key(name);
    collect(doc, &Reading::new(doc))
        .into_iter()
        .find(|b| name_key(&b.name) == key)
}

fn transaction(ops: Vec<RichOp>, links: Vec<LinkRef>) -> Result<BookmarkEdit, BookmarkError> {
    let transaction = Transaction::new(ops).map_err(|_| BookmarkError::TooLarge)?;
    Ok(BookmarkEdit { transaction, links })
}

/// Internal links (and the paragraph they sit in) aimed at the name `key`.
fn links_to(doc: &RichDocument, reading: &Reading, key: &str) -> Vec<(LinkRef, Link)> {
    let mut out = Vec::new();
    for (id, zone, section) in &reading.paras {
        let Ok(p) = doc.paragraph(*id) else { continue };
        for l in &p.links {
            if matches!(&l.target, LinkTarget::Bookmark(n) if name_key(n) == key) {
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
/// ends make a point bookmark). The name must follow `mode`. If a bookmark
/// already has this name (case ignored) it is **moved**, as in Word: same id,
/// same stored spelling, so links to it stay valid. A range over several
/// paragraphs is refused (`RangeSpansParagraphs`), never silently shortened.
pub fn insert_bookmark(
    doc: &mut RichDocument,
    name: &str,
    start: TextPosition,
    end: TextPosition,
    mode: NameMode,
) -> Result<BookmarkEdit, BookmarkError> {
    validate_bookmark_name(name, mode)?;
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
    let ops = match find_bookmark(doc, name) {
        None => {
            let id = doc.alloc_id()?;
            vec![RichOp::InsertInline {
                para,
                item: Inline::Bookmark(Bookmark {
                    id,
                    name: name.to_owned(),
                    start: s,
                    end: e,
                }),
            }]
        }
        Some(old) => {
            let moved = Bookmark {
                id: old.id,
                name: old.name,
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
    transaction(ops, Vec::new())
}

/// Renames `old` (case ignored) to `new`, which follows `mode`. Internal
/// links to the bookmark are retargeted in the same transaction and listed in
/// `links`. Another bookmark with the new name is `NameTaken`; changing only
/// the case of the name is allowed; the same name gives an empty transaction.
pub fn rename_bookmark(
    doc: &RichDocument,
    old: &str,
    new: &str,
    mode: NameMode,
) -> Result<BookmarkEdit, BookmarkError> {
    validate_bookmark_name(new, mode)?;
    let reading = Reading::new(doc);
    let all = collect(doc, &reading);
    let (old_key, new_key) = (name_key(old), name_key(new));
    let found = all
        .iter()
        .find(|b| name_key(&b.name) == old_key)
        .ok_or_else(|| BookmarkError::NotFound(old.to_owned()))?;
    if old_key != new_key && all.iter().any(|b| name_key(&b.name) == new_key) {
        return Err(BookmarkError::NameTaken(new.to_owned()));
    }
    if found.name == new {
        return transaction(Vec::new(), Vec::new());
    }
    let mut ops = vec![RichOp::ModifyInline {
        para: found.paragraph,
        item: Inline::Bookmark(Bookmark {
            id: found.id,
            name: new.to_owned(),
            start: found.start,
            end: found.end,
        }),
    }];
    let mut refs = Vec::new();
    for (r, mut link) in links_to(doc, &reading, &old_key) {
        link.target = LinkTarget::Bookmark(new.to_owned());
        ops.push(RichOp::ModifyInline {
            para: r.paragraph,
            item: Inline::Link(link),
        });
        refs.push(r);
    }
    transaction(ops, refs)
}

/// Deletes the bookmark `name` (case ignored). Internal links to it are **left
/// unchanged** (they now point nowhere, as in Word) and returned in `links` so
/// the caller can warn; undoing restores the bookmark and they work again.
pub fn delete_bookmark(doc: &RichDocument, name: &str) -> Result<BookmarkEdit, BookmarkError> {
    let reading = Reading::new(doc);
    let key = name_key(name);
    let found = collect(doc, &reading)
        .into_iter()
        .find(|b| name_key(&b.name) == key)
        .ok_or_else(|| BookmarkError::NotFound(name.to_owned()))?;
    let links = links_to(doc, &reading, &key)
        .into_iter()
        .map(|(r, _)| r)
        .collect();
    transaction(
        vec![RichOp::RemoveInline {
            para: found.paragraph,
            id: found.id,
        }],
        links,
    )
}
