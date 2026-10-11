// SPDX-License-Identifier: AGPL-3.0-or-later
//! Invariant checking shared by whole-document validation and by the checks run
//! before a branch is installed.

use super::NodeId;
use super::error::RichError;
use super::node::{BlockList, Node, Subtree, Table};
use std::collections::{BTreeMap, BTreeSet};

/// Occupancy of a table grid.
pub(crate) struct Grid {
    pub rows: usize,
    pub cols: usize,
    /// `owner[row][col]` is the cell covering that slot.
    pub owner: Vec<Vec<NodeId>>,
    /// Top-left slot of each cell.
    pub anchor: BTreeMap<NodeId, (usize, usize)>,
}

/// Computes the grid of a table, failing unless it is exactly rectangular:
/// every slot covered once, no cell leaving the grid, no hole.
pub(crate) fn compute_grid(
    nodes: &BTreeMap<NodeId, Node>,
    table: &Table,
) -> Result<Grid, RichError> {
    let (rows, cols) = (table.rows.len(), table.columns);
    if rows == 0 || cols == 0 {
        return Err(RichError::InvalidTable("a table needs a row and a column"));
    }
    let mut owner: Vec<Vec<Option<NodeId>>> = vec![vec![None; cols]; rows];
    let mut anchor = BTreeMap::new();
    for (r, row_id) in table.rows.iter().enumerate() {
        let row = match nodes.get(row_id) {
            Some(Node::Row(row)) => row,
            Some(_) => {
                return Err(RichError::WrongNodeKind {
                    id: *row_id,
                    expected: "row",
                });
            }
            None => return Err(RichError::NodeNotFound(*row_id)),
        };
        let mut c = 0;
        for cell_id in &row.cells {
            let cell = match nodes.get(cell_id) {
                Some(Node::Cell(cell)) => cell,
                Some(_) => {
                    return Err(RichError::WrongNodeKind {
                        id: *cell_id,
                        expected: "cell",
                    });
                }
                None => return Err(RichError::NodeNotFound(*cell_id)),
            };
            if cell.col_span == 0 || cell.row_span == 0 {
                return Err(RichError::InvalidTable("zero span"));
            }
            while c < cols && owner[r][c].is_some() {
                c += 1;
            }
            if c + cell.col_span > cols || r + cell.row_span > rows {
                return Err(RichError::InvalidTable("cell leaves the grid"));
            }
            for line in &mut owner[r..r + cell.row_span] {
                for slot in &mut line[c..c + cell.col_span] {
                    if slot.is_some() {
                        return Err(RichError::InvalidTable("overlapping cells"));
                    }
                    *slot = Some(*cell_id);
                }
            }
            if anchor.insert(*cell_id, (r, c)).is_some() {
                return Err(RichError::DuplicateId(*cell_id));
            }
            c += cell.col_span;
        }
        if owner[r].iter().any(Option::is_none) {
            return Err(RichError::InvalidTable("grid is not rectangular"));
        }
    }
    let owner = owner
        .into_iter()
        .map(|line| line.into_iter().flatten().collect())
        .collect();
    Ok(Grid {
        rows,
        cols,
        owner,
        anchor,
    })
}

/// Depth-first validation of a branch of the tree.
pub(crate) struct Walk<'a> {
    nodes: &'a BTreeMap<NodeId, Node>,
    /// Every node id and inline id met (one id space).
    ids: BTreeSet<NodeId>,
    pub node_count: usize,
    /// Inline item id -> owning paragraph.
    pub inline: BTreeMap<NodeId, NodeId>,
    /// Bookmark name -> bookmark id.
    pub names: BTreeMap<String, NodeId>,
}

impl<'a> Walk<'a> {
    pub fn new(nodes: &'a BTreeMap<NodeId, Node>) -> Self {
        Self {
            nodes,
            ids: BTreeSet::new(),
            node_count: 0,
            inline: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }

    fn enter(&mut self, id: NodeId) -> Result<&'a Node, RichError> {
        if !self.ids.insert(id) {
            return Err(RichError::DuplicateId(id));
        }
        let node = self.nodes.get(&id).ok_or(RichError::NodeNotFound(id))?;
        self.node_count += 1;
        Ok(node)
    }

    pub fn section(&mut self, id: NodeId) -> Result<(), RichError> {
        let Node::Section(section) = self.enter(id)? else {
            return Err(RichError::WrongNodeKind {
                id,
                expected: "section",
            });
        };
        self.blocks(&section.body)?;
        for list in [&section.headers, &section.footers] {
            if !list.windows(2).all(|w| w[0].0 < w[1].0) {
                return Err(RichError::Invariant(
                    "header/footer variants unsorted or repeated",
                ));
            }
            for (_, hf) in list {
                self.header_footer(*hf)?;
            }
        }
        Ok(())
    }

    pub fn header_footer(&mut self, id: NodeId) -> Result<(), RichError> {
        let Node::HeaderFooter(hf) = self.enter(id)? else {
            return Err(RichError::WrongNodeKind {
                id,
                expected: "header or footer",
            });
        };
        self.blocks(&hf.blocks).map(|_| ())
    }

    /// Validates a block sequence; returns how many paragraphs it holds.
    fn blocks(&mut self, list: &BlockList) -> Result<usize, RichError> {
        let mut paragraphs = 0;
        for id in list.to_vec() {
            if self.block(id)? {
                paragraphs += 1;
            }
        }
        Ok(paragraphs)
    }

    /// Validates a block; returns true for a paragraph.
    pub fn block(&mut self, id: NodeId) -> Result<bool, RichError> {
        match self.enter(id)? {
            Node::Paragraph(p) => {
                p.validate_local()?;
                for item in p.inline_ids() {
                    if !self.ids.insert(item) {
                        return Err(RichError::DuplicateId(item));
                    }
                    self.inline.insert(item, id);
                }
                for b in &p.bookmarks {
                    if self.names.insert(b.name.clone(), b.id).is_some() {
                        return Err(RichError::DuplicateBookmarkName(b.name.clone()));
                    }
                }
                Ok(true)
            }
            Node::Opaque(_) => Ok(false),
            Node::Table(table) => {
                let grid = compute_grid(self.nodes, table)?;
                for row_id in &table.rows {
                    let Node::Row(row) = self.enter(*row_id)? else {
                        return Err(RichError::WrongNodeKind {
                            id: *row_id,
                            expected: "row",
                        });
                    };
                    for cell_id in &row.cells {
                        let Node::Cell(cell) = self.enter(*cell_id)? else {
                            return Err(RichError::WrongNodeKind {
                                id: *cell_id,
                                expected: "cell",
                            });
                        };
                        if self.blocks(&cell.blocks)? == 0 {
                            return Err(RichError::Invariant("a cell needs a paragraph"));
                        }
                    }
                }
                debug_assert_eq!(grid.rows, table.rows.len());
                Ok(false)
            }
            _ => Err(RichError::WrongNodeKind {
                id,
                expected: "paragraph, table or opaque block",
            }),
        }
    }
}

/// Validates a detached branch on its own. Its root must be a section, a
/// header/footer or a block.
pub(crate) fn validate_subtree(st: &Subtree) -> Result<Walk<'_>, RichError> {
    let mut walk = Walk::new(&st.nodes);
    match st.nodes.get(&st.root) {
        None => return Err(RichError::NodeNotFound(st.root)),
        Some(Node::Section(_)) => walk.section(st.root)?,
        Some(Node::HeaderFooter(_)) => walk.header_footer(st.root)?,
        Some(_) => {
            walk.block(st.root)?;
        }
    }
    if walk.node_count != st.nodes.len() {
        return Err(RichError::Invariant(
            "detached branch holds unreachable nodes",
        ));
    }
    Ok(walk)
}
