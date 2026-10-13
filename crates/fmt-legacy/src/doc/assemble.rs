// SPDX-License-Identifier: AGPL-3.0-or-later
//! From the flat list of paragraphs of a story to model blocks: tables are
//! rebuilt from the table paragraph properties (`itap`, cell and row ends),
//! including nested tables, and the blocks are turned into model nodes.

use super::tables::{BuildCtx, CellIn, Flatten, ImportedDocTables, RowIn, TapDelta, build_table};
use super::{DocError, DocLimits, ImportNotice};
use atelier_text_model::rich::{BlockList, Node, NodeId, Paragraph, RichDocument};
use std::collections::BTreeMap;
use std::iter::Peekable;
use std::vec::IntoIter;

/// One paragraph of a story with what the table reconstruction needs.
pub(super) struct Rec {
    pub id: NodeId,
    pub para: Paragraph,
    /// Table nesting depth (0 outside tables).
    pub itap: usize,
    pub cell_end: bool,
    pub row_end: bool,
    pub tap: Option<Box<TapDelta>>,
    pub section: usize,
}

pub(super) enum Block {
    Para(Box<Rec>),
    Table(Vec<RowBlock>),
}

pub(super) struct RowBlock {
    pub cells: Vec<Vec<Block>>,
    pub tap: Option<Box<TapDelta>>,
}

impl Block {
    pub fn section(&self) -> usize {
        match self {
            Block::Para(r) => r.section,
            Block::Table(rows) => rows
                .first()
                .and_then(|r| r.cells.first())
                .and_then(|c| c.first())
                .map_or(0, Block::section),
        }
    }
}

/// Counters of repairs, reported as notices.
#[derive(Default)]
pub(super) struct Repairs {
    pub tables: BTreeMap<&'static str, usize>,
    pub flattened_cells: usize,
}

impl Repairs {
    pub fn table(&mut self, what: &'static str) {
        *self.tables.entry(what).or_insert(0) += 1;
    }
    pub fn notices(&self, out: &mut Vec<ImportNotice>) {
        if self.flattened_cells > 0 {
            out.push(ImportNotice::TableCellsFlattened {
                cells: self.flattened_cells,
            });
        }
        for (&what, &count) in &self.tables {
            out.push(ImportNotice::TableRepaired { what, count });
        }
    }
}

pub(super) fn remove_subtree(nodes: &mut BTreeMap<NodeId, Node>, root: NodeId) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(n) = nodes.remove(&id) {
            stack.extend(n.children());
        }
    }
}

/// Groups the paragraphs of a story into blocks.
pub(super) struct Grouper {
    it: Peekable<IntoIter<Rec>>,
    max_depth: usize,
    pub rep_depth: usize,
}

impl Grouper {
    pub fn new(recs: Vec<Rec>, max_depth: usize) -> Self {
        Self {
            it: recs.into_iter().peekable(),
            max_depth,
            rep_depth: 0,
        }
    }

    pub fn run(mut self) -> (Vec<Block>, usize) {
        let blocks = self.blocks(0);
        (blocks, self.rep_depth)
    }

    fn blocks(&mut self, level: usize) -> Vec<Block> {
        let mut out = Vec::new();
        while let Some(r) = self.it.peek() {
            let itap = r.itap.min(self.max_depth);
            if itap < level {
                break;
            }
            if itap == level {
                if level > 0 && r.row_end {
                    break;
                }
                let Some(r) = self.it.next() else { break };
                let cell_end = level > 0 && r.cell_end;
                out.push(Block::Para(Box::new(r)));
                if cell_end {
                    break;
                }
                continue;
            }
            let rows = self.table(level + 1);
            out.push(Block::Table(rows));
        }
        out
    }

    fn table(&mut self, level: usize) -> Vec<RowBlock> {
        let mut rows = Vec::new();
        loop {
            let mut cells = Vec::new();
            loop {
                match self.it.peek() {
                    Some(r) if r.itap.min(self.max_depth) >= level => {
                        if r.itap.min(self.max_depth) == level && r.row_end {
                            break;
                        }
                    }
                    _ => break,
                }
                let content = self.blocks(level);
                cells.push(content);
            }
            let mut tap = None;
            if matches!(self.it.peek(), Some(r) if r.itap.min(self.max_depth) == level && r.row_end)
            {
                if let Some(r) = self.it.next() {
                    tap = r.tap;
                }
            }
            if !cells.is_empty() || tap.is_some() {
                rows.push(RowBlock { cells, tap });
            }
            match self.it.peek() {
                Some(r) if r.itap.min(self.max_depth) >= level => {}
                _ => break,
            }
        }
        rows
    }
}

/// Turns blocks into nodes of `nodes`.
pub(super) struct Emitter<'a> {
    pub doc: &'a mut RichDocument,
    pub tables: &'a mut ImportedDocTables,
    pub rep: &'a mut Repairs,
    pub limits: &'a DocLimits,
    /// Paragraph ids in document order.
    pub order: Vec<NodeId>,
    pub rows: usize,
    pub cells: usize,
}

impl Emitter<'_> {
    pub fn emit(
        &mut self,
        blocks: Vec<Block>,
        nodes: &mut BTreeMap<NodeId, Node>,
        depth: usize,
    ) -> Result<Vec<NodeId>, DocError> {
        let mut ids = Vec::with_capacity(blocks.len());
        for b in blocks {
            match b {
                Block::Para(r) => {
                    self.order.push(r.id);
                    nodes.insert(r.id, Node::Paragraph(r.para));
                    ids.push(r.id);
                }
                Block::Table(rows) => self.table(rows, nodes, depth, &mut ids)?,
            }
        }
        Ok(ids)
    }

    fn table(
        &mut self,
        rows: Vec<RowBlock>,
        nodes: &mut BTreeMap<NodeId, Node>,
        depth: usize,
        out: &mut Vec<NodeId>,
    ) -> Result<(), DocError> {
        self.rows += rows.len();
        if self.rows > self.limits.max_table_rows {
            return Err(DocError::Limit("table rows"));
        }
        let mut built = Vec::with_capacity(rows.len());
        let mut flat: Vec<Vec<NodeId>> = Vec::new();
        for row in rows {
            let mut cells = Vec::with_capacity(row.cells.len());
            for cell in row.cells {
                self.cells += 1;
                if self.cells > self.limits.max_table_cells {
                    return Err(DocError::Limit("table cells"));
                }
                let blocks = self.emit(cell, nodes, depth + 1)?;
                let empty = blocks.len() == 1
                    && matches!(nodes.get(&blocks[0]), Some(Node::Paragraph(p))
                        if p.text.is_empty() && p.inline_ids().is_empty());
                flat.push(blocks.clone());
                cells.push(CellIn { blocks, empty });
            }
            built.push(RowIn {
                cells,
                tap: row.tap.map(|t| *t),
            });
        }
        let mut ctx = BuildCtx {
            doc: self.doc,
            nodes,
            tables: self.tables,
            rep: self.rep,
            limits: self.limits,
            depth,
        };
        match build_table(&mut ctx, built) {
            Ok(id) => out.push(id),
            Err(Flatten::Fatal(e)) => return Err(e),
            Err(Flatten::Invalid) => {
                // The content stays, as plain blocks of the container.
                self.rep.flattened_cells += flat.len();
                for blocks in flat {
                    out.extend(blocks);
                }
            }
        }
        Ok(())
    }
}

/// Block list of already inserted nodes.
pub(super) fn block_list(ids: &[NodeId]) -> Result<BlockList, DocError> {
    BlockList::from_ids(ids).map_err(|e| DocError::Model(e.to_string()))
}
