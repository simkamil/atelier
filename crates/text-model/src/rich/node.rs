// SPDX-License-Identifier: AGPL-3.0-or-later
//! Node kinds of the rich document and the indexed block sequence.

use super::NodeId;
use super::attrs::OpaqueAttr;
use super::error::RichError;
use super::paragraph::Paragraph;
use atelier_model::{IndexedTree, IndexedTreeError};
use core::fmt;
use std::collections::BTreeMap;

pub(crate) fn map_tree_error(error: IndexedTreeError) -> RichError {
    match error {
        IndexedTreeError::PositionOutOfBounds { position, len } => {
            RichError::PositionOutOfBounds { position, len }
        }
        IndexedTreeError::IdentifierExhausted => RichError::IdentifiersExhausted,
        IndexedTreeError::CapacityExhausted => RichError::CapacityExhausted,
    }
}

/// An ordered sequence of child ids, stored in the balanced indexed tree of
/// `atelier-model` (positional insert, removal and lookup in O(log n)).
#[derive(Clone, Default)]
pub struct BlockList(IndexedTree<NodeId>);

impl BlockList {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, position: usize) -> Option<NodeId> {
        self.0.get(position).copied()
    }
    pub fn insert(&mut self, position: usize, id: NodeId) -> Result<(), RichError> {
        self.0
            .insert(position, id)
            .map(|_| ())
            .map_err(map_tree_error)
    }
    pub fn remove(&mut self, position: usize) -> Result<NodeId, RichError> {
        self.0.remove(position).map_err(map_tree_error)
    }
    pub fn to_vec(&self) -> Vec<NodeId> {
        (0..self.len()).filter_map(|i| self.get(i)).collect()
    }
    /// A list holding `ids` in order.
    pub fn from_ids(ids: &[NodeId]) -> Result<Self, RichError> {
        let mut list = Self::default();
        for (i, id) in ids.iter().enumerate() {
            list.insert(i, *id)?;
        }
        Ok(list)
    }
}

impl PartialEq for BlockList {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.to_vec() == other.to_vec()
    }
}
impl Eq for BlockList {}
impl fmt::Debug for BlockList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.to_vec()).finish()
    }
}

/// Which header/footer of a section.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HfVariant {
    Default,
    First,
    Even,
}

/// Header or footer slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HfKind {
    Header,
    Footer,
}

/// A section: body blocks plus its headers and footers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Section {
    pub body: BlockList,
    /// `HeaderFooter` nodes, sorted by variant, one per variant.
    pub headers: Vec<(HfVariant, NodeId)>,
    pub footers: Vec<(HfVariant, NodeId)>,
    /// Uninterpreted section properties.
    pub opaque: Vec<OpaqueAttr>,
}

/// The blocks of a header or footer, attached to a section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeaderFooter {
    pub blocks: BlockList,
}

/// A table. The grid is `columns` wide; rows hold the cells anchored in them.
/// A cell with `row_span > 1` covers grid slots of the following rows, which
/// then have no cell of their own there (HTML-style spans).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    pub columns: usize,
    pub rows: Vec<NodeId>,
    /// Optional table style identifier resolved by the US-201 style engine.
    pub style: Option<String>,
    pub opaque: Vec<OpaqueAttr>,
}

/// A table row: its cells in left-to-right grid order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Row {
    pub cells: Vec<NodeId>,
    pub opaque: Vec<OpaqueAttr>,
}

/// A table cell holding blocks (at least one paragraph).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub col_span: usize,
    pub row_span: usize,
    pub blocks: BlockList,
    pub opaque: Vec<OpaqueAttr>,
}

/// A block this model does not understand, preserved verbatim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueBlock {
    /// Element name or tag, e.g. `w:sdt`.
    pub kind: String,
    pub data: Vec<u8>,
}

/// Any node stored in the document arena.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Node {
    Section(Section),
    HeaderFooter(HeaderFooter),
    Paragraph(Paragraph),
    Table(Table),
    Row(Row),
    Cell(Cell),
    Opaque(OpaqueBlock),
}

impl Node {
    /// Ids of the direct child nodes (not inline items).
    pub fn children(&self) -> Vec<NodeId> {
        match self {
            Node::Section(s) => {
                let mut v = s.body.to_vec();
                v.extend(s.headers.iter().map(|h| h.1));
                v.extend(s.footers.iter().map(|h| h.1));
                v
            }
            Node::HeaderFooter(h) => h.blocks.to_vec(),
            Node::Table(t) => t.rows.clone(),
            Node::Row(r) => r.cells.clone(),
            Node::Cell(c) => c.blocks.to_vec(),
            Node::Paragraph(_) | Node::Opaque(_) => Vec::new(),
        }
    }

    /// Whether the node may sit in a block sequence of a section, header,
    /// footer or cell.
    pub fn is_block(&self) -> bool {
        matches!(self, Node::Paragraph(_) | Node::Table(_) | Node::Opaque(_))
    }
}

/// A detached piece of the tree: a root and all its descendants. Used to
/// insert, remove and restore whole branches with their stable ids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subtree {
    pub root: NodeId,
    pub nodes: BTreeMap<NodeId, Node>,
}

impl Subtree {
    /// A subtree made of one childless node.
    pub fn leaf(id: NodeId, node: Node) -> Self {
        Self {
            root: id,
            nodes: BTreeMap::from([(id, node)]),
        }
    }
}
