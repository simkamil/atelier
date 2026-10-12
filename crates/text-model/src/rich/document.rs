// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rich document: an arena of nodes with stable ids, ordered by indexed
//! block sequences, mutated only through [`RichOp`](super::RichOp)s.

use super::NodeId;
use super::error::RichError;
use super::node::{
    BlockList, Cell, HeaderFooter, Node, OpaqueBlock, Row, Section, Subtree, Table, map_tree_error,
};
use super::ops::RichOp;
use super::paragraph::Paragraph;
use super::validate::{Walk, validate_subtree};
use atelier_model::{IndexedTree, Transaction, TransactionError, TransactionalState};
use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

/// Ids and names released by branches that are about to be removed.
#[derive(Default)]
pub(crate) struct Ids {
    pub nodes: BTreeSet<NodeId>,
    pub inline: BTreeSet<NodeId>,
    pub names: BTreeSet<String>,
}

/// A word-processing document.
///
/// Equality compares the content (nodes and their order). The identifier
/// minter and the lookup indexes are bookkeeping and are not compared.
#[derive(Clone)]
pub struct RichDocument {
    pub(crate) nodes: BTreeMap<NodeId, Node>,
    pub(crate) sections: BlockList,
    /// Inline item id -> owning paragraph (derived).
    pub(crate) inline_index: BTreeMap<NodeId, NodeId>,
    /// Bookmark name -> bookmark id (derived).
    pub(crate) bookmark_names: BTreeMap<String, NodeId>,
    /// Source of fresh ids: ids come from the indexed tree of `atelier-model`
    /// and are never reused.
    minter: IndexedTree<()>,
}

impl PartialEq for RichDocument {
    fn eq(&self, other: &Self) -> bool {
        self.sections == other.sections && self.nodes == other.nodes
    }
}
impl Eq for RichDocument {}

impl fmt::Debug for RichDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RichDocument")
            .field("sections", &self.sections)
            .field("nodes", &self.nodes)
            .finish()
    }
}

impl TransactionalState for RichDocument {
    type Checkpoint = RichDocument;
    fn checkpoint(&self) -> RichDocument {
        self.clone()
    }
    fn restore(&mut self, checkpoint: RichDocument) {
        // The minter is kept so that an id handed out before the failed
        // transaction can never be handed out again.
        let minter = std::mem::take(&mut self.minter);
        *self = checkpoint;
        self.minter = minter;
    }
}

impl Default for RichDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl RichDocument {
    /// A document with one section holding one empty paragraph.
    pub fn new() -> Self {
        let mut doc = Self {
            nodes: BTreeMap::new(),
            sections: BlockList::default(),
            inline_index: BTreeMap::new(),
            bookmark_names: BTreeMap::new(),
            minter: IndexedTree::new(),
        };
        // A fresh minter cannot be exhausted.
        let section = doc.new_section().expect("fresh minter");
        let root = section.root;
        doc.commit_install(section);
        doc.sections.insert(0, root).expect("empty list");
        doc
    }

    /// Hands out a fresh, never reused node identifier.
    pub fn alloc_id(&mut self) -> Result<NodeId, RichError> {
        let id = self.minter.insert(0, ()).map_err(map_tree_error)?;
        self.minter.remove(0).map_err(map_tree_error)?;
        Ok(id)
    }

    /// Applies a transaction atomically and returns the one that undoes it.
    pub fn apply(
        &mut self,
        transaction: &Transaction<RichOp>,
    ) -> Result<Transaction<RichOp>, TransactionError<RichError>> {
        transaction.apply(self)
    }

    // ----- read access ---------------------------------------------------

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn paragraph(&self, id: NodeId) -> Result<&Paragraph, RichError> {
        match self.nodes.get(&id) {
            Some(Node::Paragraph(p)) => Ok(p),
            Some(_) => Err(RichError::WrongNodeKind {
                id,
                expected: "paragraph",
            }),
            None => Err(RichError::NodeNotFound(id)),
        }
    }

    /// Section ids in order.
    pub fn section_ids(&self) -> Vec<NodeId> {
        self.sections.to_vec()
    }

    /// Children of a container: the sections for `None`, otherwise the blocks
    /// of a section, header/footer or cell.
    pub fn block_ids(&self, parent: Option<NodeId>) -> Result<Vec<NodeId>, RichError> {
        Ok(self.blocks_of(parent)?.to_vec())
    }

    /// Number of nodes in the arena.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Paragraph that owns an inline item (anchored object, field, bookmark, link).
    pub fn owner_of_inline(&self, id: NodeId) -> Option<NodeId> {
        self.inline_index.get(&id).copied()
    }

    /// Bookmark id for a name.
    pub fn bookmark_named(&self, name: &str) -> Option<NodeId> {
        self.bookmark_names.get(name).copied()
    }

    /// Every paragraph id in document order (bodies, headers, footers, cells).
    pub fn paragraph_ids(&self) -> Vec<NodeId> {
        fn walk(doc: &RichDocument, id: NodeId, out: &mut Vec<NodeId>) {
            match doc.nodes.get(&id) {
                Some(Node::Paragraph(_)) => out.push(id),
                Some(node) => {
                    for child in node.children() {
                        walk(doc, child, out);
                    }
                }
                None => {}
            }
        }
        let mut out = Vec::new();
        for id in self.sections.to_vec() {
            walk(self, id, &mut out);
        }
        out
    }

    /// Checks every invariant of the document.
    pub fn validate(&self) -> Result<(), RichError> {
        if self.sections.is_empty() {
            return Err(RichError::Invariant("a document needs a section"));
        }
        let mut walk = Walk::new(&self.nodes);
        for id in self.sections.to_vec() {
            walk.section(id)?;
        }
        if walk.node_count != self.nodes.len() {
            return Err(RichError::Invariant("nodes unreachable from the document"));
        }
        if walk.inline != self.inline_index || walk.names != self.bookmark_names {
            return Err(RichError::Invariant("lookup indexes out of date"));
        }
        Ok(())
    }

    // ----- builders (allocate fresh ids) -----------------------------------

    /// A detached paragraph holding `text`.
    pub fn new_paragraph(&mut self, text: &str) -> Result<Subtree, RichError> {
        let id = self.alloc_id()?;
        Ok(Subtree::leaf(
            id,
            Node::Paragraph(Paragraph::with_text(text)),
        ))
    }

    /// A detached opaque block.
    pub fn new_opaque(&mut self, kind: &str, data: &[u8]) -> Result<Subtree, RichError> {
        let id = self.alloc_id()?;
        Ok(Subtree::leaf(
            id,
            Node::Opaque(OpaqueBlock {
                kind: kind.to_owned(),
                data: data.to_vec(),
            }),
        ))
    }

    /// A detached section holding one empty paragraph.
    pub fn new_section(&mut self) -> Result<Subtree, RichError> {
        let (section, para) = (self.alloc_id()?, self.alloc_id()?);
        let mut nodes = BTreeMap::new();
        nodes.insert(para, Node::Paragraph(Paragraph::default()));
        nodes.insert(
            section,
            Node::Section(Section {
                body: BlockList::from_ids(&[para])?,
                ..Section::default()
            }),
        );
        Ok(Subtree {
            root: section,
            nodes,
        })
    }

    /// A detached header/footer holding one paragraph of `text`.
    pub fn new_header_footer(&mut self, text: &str) -> Result<Subtree, RichError> {
        let (hf, para) = (self.alloc_id()?, self.alloc_id()?);
        let mut nodes = BTreeMap::new();
        nodes.insert(para, Node::Paragraph(Paragraph::with_text(text)));
        nodes.insert(
            hf,
            Node::HeaderFooter(HeaderFooter {
                blocks: BlockList::from_ids(&[para])?,
            }),
        );
        Ok(Subtree { root: hf, nodes })
    }

    /// A detached `rows` x `columns` table of empty cells.
    pub fn new_table(&mut self, rows: usize, columns: usize) -> Result<Subtree, RichError> {
        if rows == 0 || columns == 0 {
            return Err(RichError::InvalidTable("a table needs a row and a column"));
        }
        let table = self.alloc_id()?;
        let mut nodes = BTreeMap::new();
        let mut row_ids = Vec::new();
        for _ in 0..rows {
            let row = self.alloc_id()?;
            let mut cells = Vec::new();
            for _ in 0..columns {
                let (cell, para) = (self.alloc_id()?, self.alloc_id()?);
                nodes.insert(para, Node::Paragraph(Paragraph::default()));
                nodes.insert(cell, new_cell_node(para)?);
                cells.push(cell);
            }
            nodes.insert(
                row,
                Node::Row(Row {
                    cells,
                    opaque: Vec::new(),
                }),
            );
            row_ids.push(row);
        }
        nodes.insert(
            table,
            Node::Table(Table {
                columns,
                rows: row_ids,
                style: None,
                opaque: Vec::new(),
            }),
        );
        Ok(Subtree { root: table, nodes })
    }

    // ----- internal primitives ---------------------------------------------

    pub(crate) fn blocks_of(&self, parent: Option<NodeId>) -> Result<&BlockList, RichError> {
        let Some(id) = parent else {
            return Ok(&self.sections);
        };
        match self.nodes.get(&id) {
            Some(Node::Section(s)) => Ok(&s.body),
            Some(Node::HeaderFooter(h)) => Ok(&h.blocks),
            Some(Node::Cell(c)) => Ok(&c.blocks),
            Some(_) => Err(RichError::WrongNodeKind {
                id,
                expected: "section, header, footer or cell",
            }),
            None => Err(RichError::NodeNotFound(id)),
        }
    }

    pub(crate) fn blocks_of_mut(
        &mut self,
        parent: Option<NodeId>,
    ) -> Result<&mut BlockList, RichError> {
        let Some(id) = parent else {
            return Ok(&mut self.sections);
        };
        match self.nodes.get_mut(&id) {
            Some(Node::Section(s)) => Ok(&mut s.body),
            Some(Node::HeaderFooter(h)) => Ok(&mut h.blocks),
            Some(Node::Cell(c)) => Ok(&mut c.blocks),
            Some(_) => Err(RichError::WrongNodeKind {
                id,
                expected: "section, header, footer or cell",
            }),
            None => Err(RichError::NodeNotFound(id)),
        }
    }

    /// Clones a branch out of the arena.
    pub fn extract(&self, root: NodeId) -> Result<Subtree, RichError> {
        let mut nodes = BTreeMap::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let node = self.nodes.get(&id).ok_or(RichError::NodeNotFound(id))?;
            stack.extend(node.children());
            nodes.insert(id, node.clone());
        }
        Ok(Subtree { root, nodes })
    }

    /// Ids and names that disappear when the given branches are removed.
    fn released_by(&self, roots: &[NodeId]) -> Result<Ids, RichError> {
        let mut ids = Ids::default();
        for root in roots {
            let branch = self.extract(*root)?;
            for (id, node) in &branch.nodes {
                ids.nodes.insert(*id);
                if let Node::Paragraph(p) = node {
                    ids.inline.extend(p.inline_ids());
                    ids.names.extend(p.bookmarks.iter().map(|b| b.name.clone()));
                }
            }
        }
        Ok(ids)
    }

    /// Verifies that a branch is valid and collides with nothing that is not
    /// released. `claimed` accumulates what earlier branches of the same
    /// operation already take.
    fn check_install(
        &self,
        branch: &Subtree,
        released: &Ids,
        claimed: &mut Ids,
    ) -> Result<(), RichError> {
        let walk = validate_subtree(branch)?;
        let taken = |id: &NodeId, claimed: &Ids| {
            (self.nodes.contains_key(id) && !released.nodes.contains(id))
                || (self.inline_index.contains_key(id) && !released.inline.contains(id))
                || claimed.nodes.contains(id)
                || claimed.inline.contains(id)
        };
        for id in branch.nodes.keys() {
            if taken(id, claimed) {
                return Err(RichError::DuplicateId(*id));
            }
        }
        for id in walk.inline.keys() {
            if taken(id, claimed) || branch.nodes.contains_key(id) {
                return Err(RichError::DuplicateId(*id));
            }
        }
        for name in walk.names.keys() {
            if (self.bookmark_names.contains_key(name) && !released.names.contains(name))
                || claimed.names.contains(name)
            {
                return Err(RichError::DuplicateBookmarkName(name.clone()));
            }
        }
        claimed.nodes.extend(branch.nodes.keys().copied());
        claimed.inline.extend(walk.inline.keys().copied());
        claimed.names.extend(walk.names.keys().cloned());
        Ok(())
    }

    /// Installs a branch already accepted by `check_install`. Cannot fail.
    pub(crate) fn commit_install(&mut self, branch: Subtree) {
        for (id, node) in branch.nodes {
            if let Node::Paragraph(p) = &node {
                self.index_paragraph(id, p);
            }
            self.nodes.insert(id, node);
        }
    }

    /// Removes a branch from the arena (the parent sequence is the caller's
    /// business) and returns it. Cannot fail for an existing branch.
    pub(crate) fn remove_branch(&mut self, root: NodeId) -> Result<Subtree, RichError> {
        let branch = self.extract(root)?;
        for (id, node) in &branch.nodes {
            if let Node::Paragraph(p) = node {
                self.unindex_paragraph(p);
            }
            self.nodes.remove(id);
        }
        Ok(branch)
    }

    /// Checks, without mutating, that `remove` can be replaced by `install`.
    pub(crate) fn check_replace(
        &self,
        remove: &[NodeId],
        install: &[Subtree],
    ) -> Result<(), RichError> {
        let released = self.released_by(remove)?;
        let mut claimed = Ids::default();
        for branch in install {
            self.check_install(branch, &released, &mut claimed)?;
        }
        Ok(())
    }

    /// Replaces the branches `remove` by the branches `install`, atomically:
    /// everything is checked before the first mutation.
    pub(crate) fn replace_branches(
        &mut self,
        remove: &[NodeId],
        install: Vec<Subtree>,
    ) -> Result<(), RichError> {
        self.check_replace(remove, &install)?;
        for root in remove {
            self.remove_branch(*root)?;
        }
        for branch in install {
            self.commit_install(branch);
        }
        Ok(())
    }

    fn index_paragraph(&mut self, id: NodeId, p: &Paragraph) {
        for item in p.inline_ids() {
            self.inline_index.insert(item, id);
        }
        for b in &p.bookmarks {
            self.bookmark_names.insert(b.name.clone(), b.id);
        }
    }

    fn unindex_paragraph(&mut self, p: &Paragraph) {
        for item in p.inline_ids() {
            self.inline_index.remove(&item);
        }
        for b in &p.bookmarks {
            self.bookmark_names.remove(&b.name);
        }
    }

    /// Replaces the content of a paragraph, keeping its id and place.
    /// Atomic: nothing changes unless the new content is acceptable.
    pub(crate) fn replace_paragraph(
        &mut self,
        id: NodeId,
        new: Paragraph,
    ) -> Result<(), RichError> {
        new.validate_local()?;
        let old = self.paragraph(id)?;
        let owned_by_old = |item: &NodeId| self.inline_index.get(item) == Some(&id);
        for item in new.inline_ids() {
            if self.nodes.contains_key(&item)
                || (self.inline_index.contains_key(&item) && !owned_by_old(&item))
            {
                return Err(RichError::DuplicateId(item));
            }
        }
        let old_bookmarks: BTreeSet<NodeId> = old.bookmarks.iter().map(|b| b.id).collect();
        for b in &new.bookmarks {
            if self
                .bookmark_names
                .get(&b.name)
                .is_some_and(|holder| !old_bookmarks.contains(holder))
            {
                return Err(RichError::DuplicateBookmarkName(b.name.clone()));
            }
        }
        let old = old.clone();
        self.unindex_paragraph(&old);
        self.index_paragraph(id, &new);
        self.nodes.insert(id, Node::Paragraph(new));
        Ok(())
    }
}

/// A 1x1 cell holding the paragraph `para`.
pub(crate) fn new_cell_node(para: NodeId) -> Result<Node, RichError> {
    Ok(Node::Cell(Cell {
        col_span: 1,
        row_span: 1,
        blocks: BlockList::from_ids(&[para])?,
        opaque: Vec::new(),
    }))
}
