// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bookmarks and Go To (US-291), above the rich document model.
//!
//! - [`bookmarks`]: insert (or move), rename, delete and list bookmarks; every
//!   change is a [`Transaction`](atelier_model::Transaction) of the existing
//!   `RichOp`s, so it is undoable through `RichDocument::apply` or `History`.
//! - [`goto`]: resolves a Go To target (page, section, line, bookmark, table,
//!   object, heading) from the current position.
//! - [`name`]: the name rules. [`pages`]: what the layout provides.
//!
//! See `docs/us-291-bookmarks-goto.md`. Reading order and zones are those of
//! the search module (`ParagraphSource for RichDocument`). Every function is
//! linear in the size of the document.

mod bookmarks;
mod goto;
mod name;
mod pages;

use crate::rich::{NodeId, RichDocument};
use crate::search::{ParagraphSource, Zone};
use atelier_model::selection::TextPosition;
use std::collections::BTreeMap;

pub use bookmarks::{
    BookmarkEdit, BookmarkError, BookmarkInfo, BookmarkMatch, BookmarkOrder, LinkRef,
    delete_bookmark, find_bookmark, insert_bookmark, list_bookmarks, lookup_bookmark,
    rename_bookmark,
};
pub use goto::{
    Clamp, GotoAmount, GotoError, GotoKind, GotoOutcome, Landing, ParseError, goto,
    is_heading_style, parse_amount, step,
};
pub use name::{
    MAX_NAME_CHARS, NameError, NameMode, is_hidden_name, name_key, normalize_name,
    validate_bookmark_name,
};
pub use pages::{FixedPageMap, PageMap};

/// Direction of the "Next" and "Previous" buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Next,
    Previous,
}

/// The paragraphs in reading order, with their zone and section.
pub(crate) struct Reading {
    pub paras: Vec<(NodeId, Zone, usize)>,
    index: BTreeMap<NodeId, usize>,
}

impl Reading {
    pub fn new(doc: &RichDocument) -> Self {
        let paras: Vec<_> = doc
            .paragraphs()
            .iter()
            .map(|p| (p.id, p.zone, p.section))
            .collect();
        let index = paras.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
        Self { paras, index }
    }

    /// Reading-order key of a position: (paragraph rank, offset).
    pub fn key(&self, position: TextPosition) -> Option<(usize, usize)> {
        self.index
            .get(&position.node)
            .map(|i| (*i, position.offset))
    }

    pub fn rank(&self, node: NodeId) -> Option<usize> {
        self.index.get(&node).copied()
    }
}
