// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prefix-sum index over the paragraphs of the flat text (US-204).
//!
//! An implicit treap (positional balanced tree) whose nodes are the
//! paragraphs of the flat index in document order. Each node carries the
//! number of paragraphs and the number of bytes (the paragraph's flat length
//! plus its separator) of its subtree, so that the position of a paragraph,
//! the paragraph at a position, a length change and the replacement of a run
//! of paragraphs are all `O(log n)` (the last one plus the size of the run).
//! No dependency, no `unsafe`; priorities come from a fixed-seed xorshift so
//! the shape of the tree is deterministic.

use super::NodeId;
use super::flat::{BlockKind, LIST_LEVELS};
use std::collections::BTreeMap;

const NIL: u32 = u32::MAX;

/// Ids of the table, row and cell containing a table-cell paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellContext {
    pub table: NodeId,
    pub row: NodeId,
    pub cell: NodeId,
}

/// One paragraph of the flat index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Node id of the paragraph (or of the opaque block).
    pub id: NodeId,
    pub opaque: bool,
    pub cell: Option<CellContext>,
    /// Container whose block sequence holds the node: the section or the cell.
    pub parent: NodeId,
    /// Section holding the node.
    pub section: NodeId,
    /// Flat length in bytes (without the separator).
    pub len: usize,
}

impl Entry {
    pub fn kind(&self) -> BlockKind {
        if self.opaque {
            BlockKind::Opaque
        } else if let Some(c) = self.cell {
            BlockKind::TableCell {
                table: c.table,
                row: c.row,
                cell: c.cell,
            }
        } else {
            BlockKind::Body
        }
    }
}

struct TNode {
    left: u32,
    right: u32,
    parent: u32,
    prio: u32,
    count: usize,
    /// Bytes of the subtree: each entry counts its length plus one separator.
    sum: usize,
    entry: Entry,
}

/// Counters of every list before a checkpoint paragraph.
pub(crate) type ListState = BTreeMap<u32, [u32; LIST_LEVELS]>;

pub(crate) struct FlatTree {
    nodes: Vec<TNode>,
    free: Vec<u32>,
    root: u32,
    seed: u64,
    /// Node id of an entry -> its node, for [`FlatTree::index_of`].
    slots: BTreeMap<NodeId, u32>,
    /// `(index, state)`: list counters before paragraph `index`, kept for the
    /// indices computed so far; truncated by any change at a lower index.
    pub lists: Vec<(usize, ListState)>,
}

impl FlatTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            free: Vec::new(),
            root: NIL,
            seed: 0x9e37_79b9_7f4a_7c15,
            slots: BTreeMap::new(),
            lists: Vec::new(),
        }
    }

    pub fn from_entries(entries: Vec<Entry>) -> Self {
        let mut tree = Self::new();
        tree.nodes.reserve(entries.len());
        for entry in entries {
            let node = tree.alloc(entry);
            tree.root = tree.merge(tree.root, node);
        }
        tree.fix_root();
        tree
    }

    fn fix_root(&mut self) {
        if self.root != NIL {
            self.nodes[self.root as usize].parent = NIL;
        }
    }

    fn next_priority(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        (x >> 32) as u32
    }

    fn alloc(&mut self, entry: Entry) -> u32 {
        let prio = self.next_priority();
        let id = entry.id;
        let node = TNode {
            left: NIL,
            right: NIL,
            parent: NIL,
            prio,
            count: 1,
            sum: entry.len + 1,
            entry,
        };
        let slot = match self.free.pop() {
            Some(slot) => {
                self.nodes[slot as usize] = node;
                slot
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            }
        };
        self.slots.insert(id, slot);
        slot
    }

    fn count_of(&self, n: u32) -> usize {
        if n == NIL {
            0
        } else {
            self.nodes[n as usize].count
        }
    }

    fn sum_of(&self, n: u32) -> usize {
        if n == NIL {
            0
        } else {
            self.nodes[n as usize].sum
        }
    }

    fn pull(&mut self, n: u32) {
        let (l, r) = (self.nodes[n as usize].left, self.nodes[n as usize].right);
        let count = 1 + self.count_of(l) + self.count_of(r);
        let sum = self.nodes[n as usize].entry.len + 1 + self.sum_of(l) + self.sum_of(r);
        let node = &mut self.nodes[n as usize];
        node.count = count;
        node.sum = sum;
        for child in [l, r] {
            if child != NIL {
                self.nodes[child as usize].parent = n;
            }
        }
    }

    /// Splits into the first `k` entries and the rest.
    fn split(&mut self, n: u32, k: usize) -> (u32, u32) {
        if n == NIL {
            return (NIL, NIL);
        }
        let left = self.nodes[n as usize].left;
        let left_count = self.count_of(left);
        if k <= left_count {
            let (a, b) = self.split(left, k);
            self.nodes[n as usize].left = b;
            self.pull(n);
            (a, n)
        } else {
            let right = self.nodes[n as usize].right;
            let (a, b) = self.split(right, k - left_count - 1);
            self.nodes[n as usize].right = a;
            self.pull(n);
            (n, b)
        }
    }

    fn merge(&mut self, a: u32, b: u32) -> u32 {
        if a == NIL {
            return b;
        }
        if b == NIL {
            return a;
        }
        if self.nodes[a as usize].prio > self.nodes[b as usize].prio {
            let right = self.nodes[a as usize].right;
            let merged = self.merge(right, b);
            self.nodes[a as usize].right = merged;
            self.pull(a);
            a
        } else {
            let left = self.nodes[b as usize].left;
            let merged = self.merge(a, left);
            self.nodes[b as usize].left = merged;
            self.pull(b);
            b
        }
    }

    fn release(&mut self, root: u32) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if n == NIL {
                continue;
            }
            stack.push(self.nodes[n as usize].left);
            stack.push(self.nodes[n as usize].right);
            let id = self.nodes[n as usize].entry.id;
            if self.slots.get(&id) == Some(&n) {
                self.slots.remove(&id);
            }
            self.free.push(n);
        }
    }

    /// Number of paragraphs.
    pub fn count(&self) -> usize {
        self.count_of(self.root)
    }

    /// Bytes of flat text: lengths plus one separator between paragraphs.
    pub fn total(&self) -> usize {
        self.sum_of(self.root).saturating_sub(1)
    }

    /// Paragraph index and offset of a flat position; a position on a
    /// separator belongs to the paragraph that ends there. `None` beyond the
    /// end.
    pub fn find(&self, position: usize) -> Option<(usize, usize)> {
        if self.root == NIL || position > self.total() {
            return None;
        }
        let (mut n, mut index, mut rest) = (self.root, 0usize, position);
        while n != NIL {
            let node = &self.nodes[n as usize];
            let left_sum = self.sum_of(node.left);
            if rest < left_sum {
                n = node.left;
                continue;
            }
            rest -= left_sum;
            index += self.count_of(node.left);
            let weight = node.entry.len + 1;
            if rest < weight {
                return Some((index, rest));
            }
            rest -= weight;
            index += 1;
            n = node.right;
        }
        None
    }

    fn node_at(&self, index: usize) -> Option<u32> {
        let (mut n, mut rest) = (self.root, index);
        while n != NIL {
            let node = &self.nodes[n as usize];
            let left_count = self.count_of(node.left);
            if rest < left_count {
                n = node.left;
            } else if rest == left_count {
                return Some(n);
            } else {
                rest -= left_count + 1;
                n = node.right;
            }
        }
        None
    }

    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.node_at(index).map(|n| &self.nodes[n as usize].entry)
    }

    /// Index of the entry whose node id is `id`.
    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        let slot = *self.slots.get(&id)?;
        let mut index = self.count_of(self.nodes[slot as usize].left);
        let mut current = slot;
        loop {
            let parent = self.nodes[current as usize].parent;
            if parent == NIL {
                return Some(index);
            }
            if self.nodes[parent as usize].right == current {
                index += self.count_of(self.nodes[parent as usize].left) + 1;
            }
            current = parent;
        }
    }

    /// Flat position of the first byte of paragraph `index`.
    pub fn start(&self, index: usize) -> Option<usize> {
        if index >= self.count() {
            return None;
        }
        let (mut n, mut rest, mut start) = (self.root, index, 0usize);
        while n != NIL {
            let node = &self.nodes[n as usize];
            let left_count = self.count_of(node.left);
            if rest < left_count {
                n = node.left;
            } else {
                start += self.sum_of(node.left);
                if rest == left_count {
                    return Some(start);
                }
                start += node.entry.len + 1;
                rest -= left_count + 1;
                n = node.right;
            }
        }
        None
    }

    /// Entries `first..first + count` in order (clamped to the end).
    pub fn range(&self, first: usize, count: usize) -> Vec<Entry> {
        (first..first.saturating_add(count))
            .map_while(|i| self.entry(i).copied())
            .collect()
    }

    /// Every entry in order.
    #[cfg(test)]
    pub fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::with_capacity(self.count());
        let mut stack = Vec::new();
        let mut n = self.root;
        while n != NIL || !stack.is_empty() {
            while n != NIL {
                stack.push(n);
                n = self.nodes[n as usize].left;
            }
            if let Some(top) = stack.pop() {
                out.push(self.nodes[top as usize].entry);
                n = self.nodes[top as usize].right;
            }
        }
        out
    }

    /// Replaces the `remove` entries starting at `first` by `new`.
    pub fn splice(&mut self, first: usize, remove: usize, new: &[Entry]) {
        let first = first.min(self.count());
        let (a, rest) = self.split(self.root, first);
        let remove = remove.min(self.count_of(rest));
        let (mid, c) = self.split(rest, remove);
        self.release(mid);
        let mut inserted = NIL;
        for entry in new {
            let node = self.alloc(*entry);
            inserted = self.merge(inserted, node);
        }
        let left = self.merge(a, inserted);
        self.root = self.merge(left, c);
        self.fix_root();
        self.lists.retain(|(k, _)| *k <= first);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::RichDocument;

    fn entries(doc: &mut RichDocument, lens: &[usize]) -> Vec<Entry> {
        let parent = doc.alloc_id().unwrap();
        lens.iter()
            .map(|len| Entry {
                id: doc.alloc_id().unwrap(),
                opaque: false,
                cell: None,
                parent,
                section: parent,
                len: *len,
            })
            .collect()
    }

    #[test]
    fn prefix_sums_find_and_splice_agree_with_a_vector() {
        let mut lens: Vec<usize> = (0..200).map(|i| (i * 7) % 11).collect();
        let mut doc = RichDocument::new();
        let mut tree = FlatTree::from_entries(entries(&mut doc, &lens));
        let check = |tree: &FlatTree, lens: &[usize]| {
            assert_eq!(tree.count(), lens.len());
            let total: usize = lens.iter().sum::<usize>() + lens.len() - 1;
            assert_eq!(tree.total(), total);
            let mut start = 0;
            let mut starts = Vec::new();
            for (i, len) in lens.iter().enumerate() {
                assert_eq!(tree.start(i), Some(start));
                assert_eq!(tree.entry(i).map(|e| e.len), Some(*len));
                starts.push(start);
                start += len + 1;
            }
            for position in 0..=total {
                let i = starts.partition_point(|s| *s <= position) - 1;
                assert_eq!(tree.find(position), Some((i, position - starts[i])));
            }
            assert_eq!(tree.find(total + 1), None);
            assert_eq!(tree.entries().len(), lens.len());
            for (i, entry) in tree.entries().iter().enumerate() {
                assert_eq!(tree.index_of(entry.id), Some(i));
            }
        };
        check(&tree, &lens);
        let mut seed = 12345u64;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 33) as usize) % n.max(1)
        };
        for _ in 0..300 {
            let first = rnd(lens.len());
            let remove = rnd(4).min(lens.len() - first);
            let new: Vec<usize> = (0..rnd(4)).map(|_| rnd(9)).collect();
            if lens.len() - remove + new.len() == 0 {
                continue;
            }
            tree.splice(first, remove, &entries(&mut doc, &new));
            lens.splice(first..first + remove, new);
            check(&tree, &lens);
        }
    }
}
