// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rich word-processing document model (US-200).
//!
//! `Document -> Section* -> Block* (Paragraph | Table | Opaque)`, with headers
//! and footers attached to sections. Nodes carry stable ids minted by the
//! indexed tree of `atelier-model`; block sequences are stored in that same
//! tree; every edit is a reversible [`RichOp`] applied through
//! `atelier_model::Transaction` and `History`.
//!
//! This module is independent of `PlainTextDocument` and is not wired to the
//! editor. Style resolution is provided by [`styles`] (US-201).

mod attrs;
mod defaults;
mod document;
mod error;
pub mod flat;
pub mod flat_double;
mod node;
mod ops;
mod paragraph;
mod styles;
mod table;
mod validate;

/// Stable identifier of a node or inline item (minted by `atelier-model`).
pub type NodeId = atelier_model::NodeId;

pub use attrs::{
    Alignment, CharAttrs, Color, LineRule, LineSpacing, ListRef, MAX_LIST_LEVEL, OpaqueAttr,
    ParaProps, Underline, VerticalAlign,
};
pub use defaults::{DefaultPresentation, DefaultRichDocument, default_rich_document};
pub use document::RichDocument;
pub use error::RichError;
pub use node::{
    BlockList, Cell, HeaderFooter, HfKind, HfVariant, Node, OpaqueBlock, Row, Section, Subtree,
    Table,
};
pub use ops::RichOp;
pub use paragraph::{
    AnchoredObject, Bookmark, BreakKind, CharRun, Field, Inline, InlineBreak, InlineSymbol, Link,
    LinkTarget, Paragraph,
};
pub use styles::{
    DocumentStyleDefaults, ResolvedProperties, Style, StyleCatalog, StyleCharAttrs, StyleEngine,
    StyleError, StyleKind, Theme, ThemeColor, ThemeFont,
};
pub use table::NewCell;
