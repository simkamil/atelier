// SPDX-License-Identifier: AGPL-3.0-or-later
//! Table editing on a detached copy of a table branch. The caller validates the
//! edited branch (including the rectangular grid) before installing it, so a
//! failing edit never touches the document.

use super::NodeId;
use super::document::{RichDocument, new_cell_node};
use super::error::RichError;
use super::node::{Cell, Node, Row, Subtree, Table};
use super::ops::RichOp;
use super::paragraph::Paragraph;
use super::validate::{Grid, compute_grid};
use std::collections::{BTreeMap, BTreeSet};

/// A fresh cell id with the id of the paragraph it will hold.
pub type NewCell = (NodeId, NodeId);

type Nodes = BTreeMap<NodeId, Node>;

fn table_of(sub: &Subtree) -> Result<Table, RichError> {
    match sub.nodes.get(&sub.root) {
        Some(Node::Table(t)) => Ok(t.clone()),
        Some(_) => Err(RichError::WrongNodeKind {
            id: sub.root,
            expected: "table",
        }),
        None => Err(RichError::NodeNotFound(sub.root)),
    }
}

fn row_mut(nodes: &mut Nodes, id: NodeId) -> Result<&mut Row, RichError> {
    match nodes.get_mut(&id) {
        Some(Node::Row(r)) => Ok(r),
        Some(_) => Err(RichError::WrongNodeKind {
            id,
            expected: "row",
        }),
        None => Err(RichError::NodeNotFound(id)),
    }
}

fn cell_mut(nodes: &mut Nodes, id: NodeId) -> Result<&mut Cell, RichError> {
    match nodes.get_mut(&id) {
        Some(Node::Cell(c)) => Ok(c),
        Some(_) => Err(RichError::WrongNodeKind {
            id,
            expected: "cell",
        }),
        None => Err(RichError::NodeNotFound(id)),
    }
}

fn add_cell(nodes: &mut Nodes, (cell, para): NewCell) -> Result<(), RichError> {
    for id in [cell, para] {
        if nodes.contains_key(&id) {
            return Err(RichError::DuplicateId(id));
        }
    }
    if cell == para {
        return Err(RichError::DuplicateId(cell));
    }
    nodes.insert(para, Node::Paragraph(Paragraph::default()));
    nodes.insert(cell, new_cell_node(para)?);
    Ok(())
}

fn remove_nodes(nodes: &mut Nodes, root: NodeId) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(node) = nodes.remove(&id) {
            stack.extend(node.children());
        }
    }
}

fn put_table(sub: &mut Subtree, table: Table) {
    sub.nodes.insert(sub.root, Node::Table(table));
}

fn grid_of(sub: &Subtree, table: &Table) -> Result<Grid, RichError> {
    compute_grid(&sub.nodes, table)
}

/// For a new row at `index`: which grid columns are already covered by a cell
/// of an earlier row, and which cells those are.
fn row_plan(grid: &Grid, index: usize) -> (Vec<bool>, BTreeSet<NodeId>) {
    let mut covered = vec![false; grid.cols];
    let mut crossing = BTreeSet::new();
    if index > 0 && index < grid.rows {
        for (c, slot) in covered.iter_mut().enumerate() {
            let owner = grid.owner[index][c];
            if grid.anchor[&owner].0 < index {
                *slot = true;
                crossing.insert(owner);
            }
        }
    }
    (covered, crossing)
}

/// For a new column at `index`: which grid rows are already covered by a cell
/// of an earlier column, and which cells those are.
fn column_plan(grid: &Grid, index: usize) -> (Vec<bool>, BTreeSet<NodeId>) {
    let mut covered = vec![false; grid.rows];
    let mut crossing = BTreeSet::new();
    if index > 0 && index < grid.cols {
        for (r, slot) in covered.iter_mut().enumerate() {
            let owner = grid.owner[r][index];
            if grid.anchor[&owner].1 < index {
                *slot = true;
                crossing.insert(owner);
            }
        }
    }
    (covered, crossing)
}

pub(crate) fn insert_row(
    sub: &mut Subtree,
    index: usize,
    row_id: NodeId,
    cells: &[NewCell],
) -> Result<(), RichError> {
    let mut table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    if index > grid.rows {
        return Err(RichError::PositionOutOfBounds {
            position: index,
            len: grid.rows,
        });
    }
    let (covered, crossing) = row_plan(&grid, index);
    if cells.len() != covered.iter().filter(|c| !**c).count() {
        return Err(RichError::InvalidTable("wrong number of new cells"));
    }
    if sub.nodes.contains_key(&row_id) {
        return Err(RichError::DuplicateId(row_id));
    }
    for cell in crossing {
        cell_mut(&mut sub.nodes, cell)?.row_span += 1;
    }
    let mut ids = Vec::new();
    for new in cells {
        add_cell(&mut sub.nodes, *new)?;
        ids.push(new.0);
    }
    sub.nodes.insert(
        row_id,
        Node::Row(Row {
            cells: ids,
            opaque: Vec::new(),
        }),
    );
    table.rows.insert(index, row_id);
    put_table(sub, table);
    Ok(())
}

pub(crate) fn delete_row(sub: &mut Subtree, index: usize) -> Result<(), RichError> {
    let mut table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    if index >= grid.rows {
        return Err(RichError::PositionOutOfBounds {
            position: index,
            len: grid.rows,
        });
    }
    if grid.rows < 2 {
        return Err(RichError::InvalidTable("cannot delete the last row"));
    }
    let row_id = table.rows[index];
    let own_cells = row_mut(&mut sub.nodes, row_id)?.cells.clone();
    let above: BTreeSet<NodeId> = grid.owner[index]
        .iter()
        .copied()
        .filter(|c| grid.anchor[c].0 < index)
        .collect();
    for cell in above {
        cell_mut(&mut sub.nodes, cell)?.row_span -= 1;
    }
    let col_of: BTreeMap<NodeId, usize> = grid.anchor.iter().map(|(k, v)| (*k, v.1)).collect();
    for cell_id in own_cells {
        if cell_mut(&mut sub.nodes, cell_id)?.row_span > 1 {
            cell_mut(&mut sub.nodes, cell_id)?.row_span -= 1;
            let next = *table
                .rows
                .get(index + 1)
                .ok_or(RichError::InvalidTable("spanning cell without a next row"))?;
            let list = &mut row_mut(&mut sub.nodes, next)?.cells;
            let pos = list
                .iter()
                .filter(|x| col_of[*x] < col_of[&cell_id])
                .count();
            list.insert(pos, cell_id);
        } else {
            remove_nodes(&mut sub.nodes, cell_id);
        }
    }
    sub.nodes.remove(&row_id);
    table.rows.remove(index);
    put_table(sub, table);
    Ok(())
}

pub(crate) fn insert_column(
    sub: &mut Subtree,
    index: usize,
    cells: &[NewCell],
) -> Result<(), RichError> {
    let mut table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    if index > grid.cols {
        return Err(RichError::PositionOutOfBounds {
            position: index,
            len: grid.cols,
        });
    }
    let (covered, crossing) = column_plan(&grid, index);
    if cells.len() != covered.iter().filter(|c| !**c).count() {
        return Err(RichError::InvalidTable("wrong number of new cells"));
    }
    for cell in crossing {
        cell_mut(&mut sub.nodes, cell)?.col_span += 1;
    }
    let mut fresh = cells.iter();
    for (r, row_id) in table.rows.iter().enumerate() {
        if covered[r] {
            continue;
        }
        let Some(new) = fresh.next() else {
            return Err(RichError::InvalidTable("wrong number of new cells"));
        };
        add_cell(&mut sub.nodes, *new)?;
        let list = &mut row_mut(&mut sub.nodes, *row_id)?.cells;
        let pos = list.iter().filter(|x| grid.anchor[*x].1 < index).count();
        list.insert(pos, new.0);
    }
    table.columns += 1;
    put_table(sub, table);
    Ok(())
}

pub(crate) fn delete_column(sub: &mut Subtree, index: usize) -> Result<(), RichError> {
    let mut table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    if index >= grid.cols {
        return Err(RichError::PositionOutOfBounds {
            position: index,
            len: grid.cols,
        });
    }
    if grid.cols < 2 {
        return Err(RichError::InvalidTable("cannot delete the last column"));
    }
    let crossing: BTreeSet<NodeId> = (0..grid.rows).map(|r| grid.owner[r][index]).collect();
    for cell_id in crossing {
        if cell_mut(&mut sub.nodes, cell_id)?.col_span > 1 {
            cell_mut(&mut sub.nodes, cell_id)?.col_span -= 1;
        } else {
            let row_id = table.rows[grid.anchor[&cell_id].0];
            row_mut(&mut sub.nodes, row_id)?
                .cells
                .retain(|c| *c != cell_id);
            remove_nodes(&mut sub.nodes, cell_id);
        }
    }
    table.columns -= 1;
    put_table(sub, table);
    Ok(())
}

fn is_blank(nodes: &Nodes, cell: &Cell) -> bool {
    let ids = cell.blocks.to_vec();
    ids.len() == 1
        && matches!(nodes.get(&ids[0]), Some(Node::Paragraph(p))
            if p.text.is_empty() && p.inline_ids().is_empty())
}

pub(crate) fn merge_cells(
    sub: &mut Subtree,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
) -> Result<(), RichError> {
    let table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    if rows == 0 || cols == 0 || rows.saturating_mul(cols) < 2 {
        return Err(RichError::InvalidTable("a merge needs at least two slots"));
    }
    if row.saturating_add(rows) > grid.rows || col.saturating_add(cols) > grid.cols {
        return Err(RichError::InvalidTable("merge region leaves the grid"));
    }
    let mut region = BTreeSet::new();
    for line in &grid.owner[row..row + rows] {
        region.extend(line[col..col + cols].iter().copied());
    }
    for id in &region {
        let (ar, ac) = grid.anchor[id];
        let cell = cell_mut(&mut sub.nodes, *id)?;
        if ar < row
            || ac < col
            || ar + cell.row_span > row + rows
            || ac + cell.col_span > col + cols
        {
            return Err(RichError::InvalidTable(
                "merge region cuts through a merged cell",
            ));
        }
    }
    let target = grid.owner[row][col];
    let mut others: Vec<NodeId> = region.into_iter().filter(|c| *c != target).collect();
    others.sort_by_key(|c| grid.anchor[c]);
    for other in others {
        let cell = match sub.nodes.get(&other) {
            Some(Node::Cell(c)) => c.clone(),
            _ => return Err(RichError::NodeNotFound(other)),
        };
        let row_id = table.rows[grid.anchor[&other].0];
        row_mut(&mut sub.nodes, row_id)?
            .cells
            .retain(|c| *c != other);
        if is_blank(&sub.nodes, &cell) {
            remove_nodes(&mut sub.nodes, other);
        } else {
            sub.nodes.remove(&other);
            for block in cell.blocks.to_vec() {
                let list = &mut cell_mut(&mut sub.nodes, target)?.blocks;
                let end = list.len();
                list.insert(end, block)?;
            }
        }
    }
    let target_cell = cell_mut(&mut sub.nodes, target)?;
    target_cell.col_span = cols;
    target_cell.row_span = rows;
    Ok(())
}

pub(crate) fn split_cell(
    sub: &mut Subtree,
    cell: NodeId,
    cells: &[NewCell],
) -> Result<(), RichError> {
    let table = table_of(sub)?;
    let grid = grid_of(sub, &table)?;
    let (r, c) = *grid
        .anchor
        .get(&cell)
        .ok_or(RichError::NodeNotFound(cell))?;
    let (rs, cs) = {
        let cell = cell_mut(&mut sub.nodes, cell)?;
        (cell.row_span, cell.col_span)
    };
    if rs * cs < 2 {
        return Err(RichError::InvalidTable("the cell is not merged"));
    }
    if cells.len() != rs * cs - 1 {
        return Err(RichError::InvalidTable("wrong number of new cells"));
    }
    let origin = cell_mut(&mut sub.nodes, cell)?;
    origin.row_span = 1;
    origin.col_span = 1;
    let mut col_of: BTreeMap<NodeId, usize> = grid.anchor.iter().map(|(k, v)| (*k, v.1)).collect();
    let mut fresh = cells.iter();
    for rr in r..r + rs {
        for cc in c..c + cs {
            if (rr, cc) == (r, c) {
                continue;
            }
            let Some(new) = fresh.next() else {
                return Err(RichError::InvalidTable("wrong number of new cells"));
            };
            add_cell(&mut sub.nodes, *new)?;
            col_of.insert(new.0, cc);
            let list = &mut row_mut(&mut sub.nodes, table.rows[rr])?.cells;
            let pos = list.iter().filter(|x| col_of[*x] < cc).count();
            list.insert(pos, new.0);
        }
    }
    Ok(())
}

impl RichDocument {
    fn fresh_cells(&mut self, count: usize) -> Result<Vec<NewCell>, RichError> {
        (0..count)
            .map(|_| Ok((self.alloc_id()?, self.alloc_id()?)))
            .collect()
    }

    fn grid_of_table(&self, table: NodeId) -> Result<Grid, RichError> {
        let sub = self.extract(table)?;
        let t = table_of(&sub)?;
        grid_of(&sub, &t)
    }

    /// Builds an `InsertRow` with the right number of fresh cells.
    pub fn insert_row_op(&mut self, table: NodeId, index: usize) -> Result<RichOp, RichError> {
        let grid = self.grid_of_table(table)?;
        let needed = row_plan(&grid, index).0.iter().filter(|c| !**c).count();
        let row = self.alloc_id()?;
        let cells = self.fresh_cells(needed)?;
        Ok(RichOp::InsertRow {
            table,
            index,
            row,
            cells,
        })
    }

    /// Builds an `InsertColumn` with the right number of fresh cells.
    pub fn insert_column_op(&mut self, table: NodeId, index: usize) -> Result<RichOp, RichError> {
        let grid = self.grid_of_table(table)?;
        let needed = column_plan(&grid, index).0.iter().filter(|c| !**c).count();
        let cells = self.fresh_cells(needed)?;
        Ok(RichOp::InsertColumn {
            table,
            index,
            cells,
        })
    }

    /// Builds a `SplitCell` with the right number of fresh cells.
    pub fn split_cell_op(&mut self, table: NodeId, cell: NodeId) -> Result<RichOp, RichError> {
        let sub = self.extract(table)?;
        let spans = match sub.nodes.get(&cell) {
            Some(Node::Cell(c)) => c.row_span * c.col_span,
            _ => return Err(RichError::NodeNotFound(cell)),
        };
        let cells = self.fresh_cells(spans.saturating_sub(1))?;
        Ok(RichOp::SplitCell { table, cell, cells })
    }
}
