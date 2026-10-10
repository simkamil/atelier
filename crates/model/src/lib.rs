// SPDX-License-Identifier: AGPL-3.0-or-later
//! A position-indexed sequence backed by an AVL tree.
//!
//! Positions are implicit in the in-order traversal. Subtree sizes make rank
//! lookup and insertion logarithmic, while parent links make the reverse
//! identity-to-position lookup logarithmic as well.

#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeMap;

/// Opaque identity assigned to an element for the lifetime of its tree.
///
/// Identities are local to the `IndexedTree` that created them. Different trees
/// may issue equal `NodeId` values; use an ID only with its originating tree.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(u64);

/// An invalid position or exhausted identity space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedTreeError {
    /// The requested position was outside the current sequence.
    PositionOutOfBounds { position: usize, len: usize },
    /// Every possible `u64` identity has already been issued.
    IdentifierExhausted,
    /// The position and identity are valid, but no arena slot can be represented.
    CapacityExhausted,
}

impl fmt::Display for IndexedTreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionOutOfBounds { position, len } => {
                write!(f, "position {position} is outside a tree of length {len}")
            }
            Self::IdentifierExhausted => f.write_str("node identifier space exhausted"),
            Self::CapacityExhausted => f.write_str("indexed tree capacity exhausted"),
        }
    }
}

impl std::error::Error for IndexedTreeError {}

struct Node<T> {
    id: NodeId,
    value: T,
    left: Option<usize>,
    right: Option<usize>,
    parent: Option<usize>,
    height: usize,
    size: usize,
}

impl<T> Node<T> {
    fn new(id: NodeId, value: T) -> Self {
        Self {
            id,
            value,
            left: None,
            right: None,
            parent: None,
            height: 1,
            size: 1,
        }
    }
}

/// A sequence of values with stable identities and logarithmic positional access.
///
/// The AVL tree stores elements in in-order sequence. Its radix arena reuses
/// slots after deletion, so allocated node storage is bounded by the peak slot
/// range. Growing the arena never relocates existing nodes.
pub struct IndexedTree<T> {
    root: Option<usize>,
    arena: RadixArena<T>,
    by_id: BTreeMap<NodeId, usize>,
    next_id: Option<u64>,
    len: usize,
}

const RADIX_BITS: usize = 8;
const RADIX_FANOUT: usize = 1 << RADIX_BITS;
const RADIX_LEVELS: usize = (usize::BITS as usize).div_ceil(RADIX_BITS);

enum RadixPage<T> {
    Branch(Box<[Option<RadixPage<T>>; RADIX_FANOUT]>),
    Leaf(Box<[Option<ArenaSlot<T>>; RADIX_FANOUT]>),
}

enum ArenaSlot<T> {
    Occupied(Box<Node<T>>),
    Free(Option<usize>),
}

fn empty_page<T>(leaf: bool) -> RadixPage<T> {
    if leaf {
        RadixPage::Leaf(Box::new(std::array::from_fn(|_| None)))
    } else {
        RadixPage::Branch(Box::new(std::array::from_fn(|_| None)))
    }
}

struct RadixArena<T> {
    root: Option<RadixPage<T>>,
    next_slot: Option<usize>,
    free: Option<usize>,
}

impl<T> RadixArena<T> {
    fn new() -> Self {
        Self {
            root: None,
            next_slot: Some(0),
            free: None,
        }
    }

    fn can_allocate(&self) -> bool {
        self.free.is_some() || self.next_slot.is_some()
    }

    fn allocate(&mut self, node: Node<T>) -> Option<usize> {
        let slot = if let Some(slot) = self.free {
            let entry = self.slot_mut(slot, false)?.take()?;
            let ArenaSlot::Free(next) = entry else {
                unreachable!("free-list head points to a live slot")
            };
            self.free = next;
            *self.slot_mut(slot, false)? = Some(ArenaSlot::Occupied(Box::new(node)));
            return Some(slot);
        } else {
            let slot = self.next_slot?;
            self.next_slot = slot.checked_add(1);
            slot
        };
        *self.slot_mut(slot, true)? = Some(ArenaSlot::Occupied(Box::new(node)));
        Some(slot)
    }

    fn take(&mut self, slot: usize) -> Option<Node<T>> {
        match self.slot_mut(slot, false)?.take()? {
            ArenaSlot::Occupied(node) => Some(*node),
            ArenaSlot::Free(next) => {
                *self.slot_mut(slot, false)? = Some(ArenaSlot::Free(next));
                None
            }
        }
    }

    fn put(&mut self, slot: usize, node: Node<T>) {
        *self
            .slot_mut(slot, false)
            .expect("allocated slot has a leaf") = Some(ArenaSlot::Occupied(Box::new(node)));
    }

    fn release(&mut self, slot: usize) {
        *self
            .slot_mut(slot, false)
            .expect("released slot has a leaf") = Some(ArenaSlot::Free(self.free));
        self.free = Some(slot);
    }

    fn get(&self, slot: usize) -> Option<&Node<T>> {
        match self.slot(slot)? {
            Some(ArenaSlot::Occupied(node)) => Some(node),
            _ => None,
        }
    }

    fn get_mut(&mut self, slot: usize) -> Option<&mut Node<T>> {
        match self.slot_mut(slot, false)? {
            Some(ArenaSlot::Occupied(node)) => Some(node),
            _ => None,
        }
    }

    fn slot(&self, slot: usize) -> Option<&Option<ArenaSlot<T>>> {
        let mut page = self.root.as_ref()?;
        for byte in (1..RADIX_LEVELS).rev() {
            let RadixPage::Branch(children) = page else {
                return None;
            };
            let offset = (slot >> (byte * RADIX_BITS)) & (RADIX_FANOUT - 1);
            page = children.get(offset)?.as_ref()?;
        }
        let RadixPage::Leaf(slots) = page else {
            return None;
        };
        slots.get(slot & (RADIX_FANOUT - 1))
    }

    fn slot_mut(&mut self, slot: usize, create: bool) -> Option<&mut Option<ArenaSlot<T>>> {
        if self.root.is_none() {
            if !create {
                return None;
            }
            self.root = Some(empty_page::<T>(RADIX_LEVELS == 1));
        }
        let mut page = self.root.as_mut()?;
        for byte in (1..RADIX_LEVELS).rev() {
            let offset = (slot >> (byte * RADIX_BITS)) & (RADIX_FANOUT - 1);
            let RadixPage::Branch(children) = page else {
                return None;
            };
            if children[offset].is_none() {
                if !create {
                    return None;
                }
                children[offset] = Some(empty_page::<T>(byte == 1));
            }
            page = children[offset].as_mut()?;
        }
        let RadixPage::Leaf(slots) = page else {
            return None;
        };
        slots.get_mut(slot & (RADIX_FANOUT - 1))
    }
}

impl<T> Default for IndexedTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> IndexedTree<T> {
    /// Creates an empty tree.
    pub fn new() -> Self {
        Self {
            root: None,
            arena: RadixArena::new(),
            by_id: BTreeMap::new(),
            next_id: Some(0),
            len: 0,
        }
    }

    /// Returns the number of live elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the tree has no live elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Inserts `value` before `position` and returns its stable identity.
    ///
    /// `position == len()` appends. Invalid positions and exhausted identities
    /// leave the tree unchanged.
    pub fn insert(&mut self, position: usize, value: T) -> Result<NodeId, IndexedTreeError> {
        if position > self.len {
            return Err(IndexedTreeError::PositionOutOfBounds {
                position,
                len: self.len,
            });
        }
        if !self.arena.can_allocate() {
            return Err(IndexedTreeError::CapacityExhausted);
        }
        let id_value = self.next_id.ok_or(IndexedTreeError::IdentifierExhausted)?;
        let id = NodeId(id_value);
        let node = self
            .arena
            .allocate(Node::new(id, value))
            .expect("arena capacity was checked before allocation");
        self.next_id = id_value.checked_add(1);
        self.root = Some(self.insert_rec(self.root, position, node));
        if let Some(root) = self.root {
            self.node_mut(root).parent = None;
        }
        self.by_id.insert(id, node);
        self.len += 1;
        Ok(id)
    }

    /// Removes and returns the value at `position`.
    pub fn remove(&mut self, position: usize) -> Result<T, IndexedTreeError> {
        if position >= self.len {
            return Err(IndexedTreeError::PositionOutOfBounds {
                position,
                len: self.len,
            });
        }
        let (root, removed) =
            self.remove_rec(self.root.expect("nonempty tree has a root"), position);
        self.root = root;
        if let Some(root) = self.root {
            self.node_mut(root).parent = None;
        }
        let removed_node = self.arena.take(removed).expect("removed node is live");
        self.by_id.remove(&removed_node.id);
        self.arena.release(removed);
        self.len -= 1;
        Ok(removed_node.value)
    }

    /// Returns the value at `position`, or `None` when it is out of range.
    pub fn get(&self, position: usize) -> Option<&T> {
        let node = self.node_at(position)?;
        Some(&self.node(node).value)
    }

    /// Returns the value belonging to `id`, or `None` if it is absent.
    ///
    /// `id` must have been issued by this tree; IDs from different trees may
    /// have equal representations and are not globally unique.
    pub fn get_by_id(&self, id: NodeId) -> Option<&T> {
        let node = *self.by_id.get(&id)?;
        Some(&self.node(node).value)
    }

    /// Returns the current position of `id`, or `None` if it is absent.
    ///
    /// `id` must have been issued by this tree; IDs from different trees may
    /// have equal representations and are not globally unique.
    pub fn position_of(&self, id: NodeId) -> Option<usize> {
        let mut current = *self.by_id.get(&id)?;
        let mut position = self.subtree_size(self.node(current).left);
        while let Some(parent) = self.node(current).parent {
            if self.node(parent).right == Some(current) {
                position += 1 + self.subtree_size(self.node(parent).left);
            }
            current = parent;
        }
        Some(position)
    }

    fn node(&self, index: usize) -> &Node<T> {
        self.arena
            .get(index)
            .expect("tree link points to a live node")
    }

    fn node_mut(&mut self, index: usize) -> &mut Node<T> {
        self.arena
            .get_mut(index)
            .expect("tree link points to a live node")
    }

    fn subtree_size(&self, index: Option<usize>) -> usize {
        index.map_or(0, |index| self.node(index).size)
    }

    fn subtree_height(&self, index: Option<usize>) -> usize {
        index.map_or(0, |index| self.node(index).height)
    }

    fn refresh(&mut self, index: usize) {
        let (left, right) = (self.node(index).left, self.node(index).right);
        let height = 1 + self.subtree_height(left).max(self.subtree_height(right));
        let size = 1 + self.subtree_size(left) + self.subtree_size(right);
        let node = self.node_mut(index);
        node.height = height;
        node.size = size;
    }

    fn balance_factor(&self, index: usize) -> isize {
        self.subtree_height(self.node(index).left) as isize
            - self.subtree_height(self.node(index).right) as isize
    }

    fn rotate_right(&mut self, root: usize) -> usize {
        let pivot = self
            .node(root)
            .left
            .expect("right rotation needs a left child");
        let middle = self.node(pivot).right;
        let parent = self.node(root).parent;
        self.node_mut(root).left = middle;
        if let Some(middle) = middle {
            self.node_mut(middle).parent = Some(root);
        }
        self.node_mut(pivot).right = Some(root);
        self.node_mut(root).parent = Some(pivot);
        self.node_mut(pivot).parent = parent;
        self.refresh(root);
        self.refresh(pivot);
        pivot
    }

    fn rotate_left(&mut self, root: usize) -> usize {
        let pivot = self
            .node(root)
            .right
            .expect("left rotation needs a right child");
        let middle = self.node(pivot).left;
        let parent = self.node(root).parent;
        self.node_mut(root).right = middle;
        if let Some(middle) = middle {
            self.node_mut(middle).parent = Some(root);
        }
        self.node_mut(pivot).left = Some(root);
        self.node_mut(root).parent = Some(pivot);
        self.node_mut(pivot).parent = parent;
        self.refresh(root);
        self.refresh(pivot);
        pivot
    }

    fn rebalance(&mut self, index: usize) -> usize {
        self.refresh(index);
        let balance = self.balance_factor(index);
        if balance > 1 {
            let left = self
                .node(index)
                .left
                .expect("left-heavy node has a left child");
            if self.balance_factor(left) < 0 {
                let new_left = self.rotate_left(left);
                self.node_mut(index).left = Some(new_left);
                self.node_mut(new_left).parent = Some(index);
            }
            self.rotate_right(index)
        } else if balance < -1 {
            let right = self
                .node(index)
                .right
                .expect("right-heavy node has a right child");
            if self.balance_factor(right) > 0 {
                let new_right = self.rotate_right(right);
                self.node_mut(index).right = Some(new_right);
                self.node_mut(new_right).parent = Some(index);
            }
            self.rotate_left(index)
        } else {
            index
        }
    }

    fn insert_rec(&mut self, root: Option<usize>, position: usize, inserted: usize) -> usize {
        let Some(root) = root else {
            return inserted;
        };
        let left_size = self.subtree_size(self.node(root).left);
        if position <= left_size {
            let child = self.insert_rec(self.node(root).left, position, inserted);
            self.node_mut(root).left = Some(child);
            self.node_mut(child).parent = Some(root);
        } else {
            let child = self.insert_rec(self.node(root).right, position - left_size - 1, inserted);
            self.node_mut(root).right = Some(child);
            self.node_mut(child).parent = Some(root);
        }
        self.rebalance(root)
    }

    fn node_at(&self, mut position: usize) -> Option<usize> {
        if position >= self.len {
            return None;
        }
        let mut current = self.root?;
        loop {
            let left_size = self.subtree_size(self.node(current).left);
            if position < left_size {
                current = self.node(current).left?;
            } else if position == left_size {
                return Some(current);
            } else {
                position -= left_size + 1;
                current = self.node(current).right?;
            }
        }
    }

    fn remove_rec(&mut self, root: usize, position: usize) -> (Option<usize>, usize) {
        let left = self.node(root).left;
        let left_size = self.subtree_size(left);
        if position < left_size {
            let child = left.expect("position in left subtree implies child");
            let (new_child, removed) = self.remove_rec(child, position);
            self.node_mut(root).left = new_child;
            if let Some(new_child) = new_child {
                self.node_mut(new_child).parent = Some(root);
            }
            (Some(self.rebalance(root)), removed)
        } else if position > left_size {
            let child = self
                .node(root)
                .right
                .expect("position in right subtree implies child");
            let (new_child, removed) = self.remove_rec(child, position - left_size - 1);
            self.node_mut(root).right = new_child;
            if let Some(new_child) = new_child {
                self.node_mut(new_child).parent = Some(root);
            }
            (Some(self.rebalance(root)), removed)
        } else {
            let (left, right, parent) = {
                let node = self.node(root);
                (node.left, node.right, node.parent)
            };
            match (left, right) {
                (None, child) | (child, None) => {
                    if let Some(child) = child {
                        self.node_mut(child).parent = parent;
                    }
                    (child, root)
                }
                (Some(_), Some(right_root)) => {
                    let successor = self.minimum(right_root);
                    let removed_id = self.node(root).id;
                    let successor_id = self.node(successor).id;
                    self.swap_payload(root, successor);
                    self.by_id.insert(removed_id, successor);
                    self.by_id.insert(successor_id, root);
                    let (new_right, removed) = self.remove_rec(right_root, 0);
                    self.node_mut(root).right = new_right;
                    if let Some(new_right) = new_right {
                        self.node_mut(new_right).parent = Some(root);
                    }
                    (Some(self.rebalance(root)), removed)
                }
            }
        }
    }

    fn swap_payload(&mut self, first: usize, second: usize) {
        debug_assert_ne!(first, second);
        let mut left = self.arena.take(first).expect("first node is live");
        let mut right = self.arena.take(second).expect("second node is live");
        std::mem::swap(&mut left.id, &mut right.id);
        std::mem::swap(&mut left.value, &mut right.value);
        self.arena.put(first, left);
        self.arena.put(second, right);
    }

    fn minimum(&self, mut node: usize) -> usize {
        while let Some(left) = self.node(node).left {
            node = left;
        }
        node
    }

    #[cfg(test)]
    fn validate(&self) -> (usize, usize) {
        fn walk<T>(
            tree: &IndexedTree<T>,
            node: Option<usize>,
            parent: Option<usize>,
            seen: &mut BTreeMap<NodeId, usize>,
        ) -> (usize, usize) {
            let Some(index) = node else { return (0, 0) };
            let item = tree.node(index);
            assert_eq!(item.parent, parent);
            assert!(seen.insert(item.id, index).is_none());
            let (lh, ls) = walk(tree, item.left, Some(index), seen);
            let (rh, rs) = walk(tree, item.right, Some(index), seen);
            assert!((lh as isize - rh as isize).abs() <= 1);
            assert_eq!(item.height, 1 + lh.max(rh));
            assert_eq!(item.size, 1 + ls + rs);
            (item.height, item.size)
        }
        let mut seen = BTreeMap::new();
        let result = walk(self, self.root, None, &mut seen);
        assert_eq!(result.1, self.len);
        assert_eq!(seen.len(), self.by_id.len());
        for (id, index) in &self.by_id {
            assert_eq!(seen.get(id), Some(index));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{IndexedTree, IndexedTreeError};

    fn assert_matches(tree: &IndexedTree<i32>, model: &[(super::NodeId, i32)]) {
        assert_eq!(tree.len(), model.len());
        assert_eq!(tree.is_empty(), model.is_empty());
        for (position, (id, value)) in model.iter().enumerate() {
            assert_eq!(tree.get(position), Some(value));
            assert_eq!(tree.get_by_id(*id), Some(value));
            assert_eq!(tree.position_of(*id), Some(position));
        }
        assert_eq!(tree.get(model.len()), None);
        tree.validate();
    }

    #[test]
    fn insert_positions_and_stable_ids() {
        let mut tree = IndexedTree::new();
        let a = tree.insert(0, 10).unwrap();
        let b = tree.insert(1, 30).unwrap();
        let c = tree.insert(1, 20).unwrap();
        assert_eq!(
            (tree.get(0), tree.get(1), tree.get(2)),
            (Some(&10), Some(&20), Some(&30))
        );
        assert_eq!(
            [
                tree.position_of(a),
                tree.position_of(c),
                tree.position_of(b)
            ],
            [Some(0), Some(1), Some(2)]
        );
        tree.validate();
    }

    #[test]
    fn invalid_positions_do_not_mutate() {
        let mut tree = IndexedTree::new();
        assert_eq!(
            tree.insert(1, 1),
            Err(IndexedTreeError::PositionOutOfBounds {
                position: 1,
                len: 0
            })
        );
        let id = tree.insert(0, 7).unwrap();
        assert_eq!(
            tree.remove(1),
            Err(IndexedTreeError::PositionOutOfBounds {
                position: 1,
                len: 1
            })
        );
        assert_eq!(tree.get_by_id(id), Some(&7));
        assert_eq!(tree.position_of(id), Some(0));
        tree.validate();
    }

    #[test]
    fn remove_all_then_insert_keeps_id_unique_and_reuses_slot() {
        let mut tree = IndexedTree::new();
        let first = tree.insert(0, 1).unwrap();
        let first_slot = tree.by_id[&first];
        assert_eq!(tree.remove(0), Ok(1));
        let second = tree.insert(0, 2).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            tree.by_id[&second], first_slot,
            "freed arena slot is reused"
        );
        assert_eq!(tree.get_by_id(first), None);
        assert_eq!(tree.get_by_id(second), Some(&2));
        tree.validate();
    }

    #[test]
    fn radix_arena_crosses_leaf_and_branch_boundaries() {
        let mut arena = super::RadixArena::new();
        let slots = [0, 255, 256, 65_535, 65_536, 16_777_215, 16_777_216];
        for (value, slot) in slots.into_iter().enumerate() {
            *arena.slot_mut(slot, true).unwrap() = Some(super::ArenaSlot::Occupied(Box::new(
                super::Node::new(super::NodeId(value as u64), value),
            )));
        }
        for (value, slot) in slots.into_iter().enumerate() {
            assert_eq!(arena.get(slot).map(|node| node.value), Some(value));
        }
        assert!(arena.get(257).is_none());
    }

    #[test]
    fn node_ids_are_local_to_their_tree() {
        let mut left = IndexedTree::new();
        let mut right = IndexedTree::new();
        let left_id = left.insert(0, 10).unwrap();
        let right_id = right.insert(0, 20).unwrap();
        assert_eq!(left_id, right_id);
        assert_eq!(left.get_by_id(left_id), Some(&10));
        assert_eq!(right.get_by_id(right_id), Some(&20));
        assert_eq!(left.position_of(left_id), Some(0));
        assert_eq!(right.position_of(right_id), Some(0));
    }

    #[test]
    fn deleting_a_two_child_node_preserves_successor_identity() {
        let mut tree = IndexedTree::new();
        let ids: Vec<_> = (0..15)
            .map(|value| tree.insert(tree.len(), value).unwrap())
            .collect();
        let root = tree.root.unwrap();
        assert!(tree.node(root).left.is_some());
        assert!(tree.node(root).right.is_some());
        let removed_id = tree.node(root).id;
        let position = tree.position_of(removed_id).unwrap();
        let removed_value = *tree.get(position).unwrap();
        assert_eq!(tree.remove(position), Ok(removed_value));
        assert_eq!(tree.get_by_id(removed_id), None);
        for id in ids.into_iter().filter(|id| *id != removed_id) {
            let current = tree
                .position_of(id)
                .expect("surviving identity remains indexed");
            assert_eq!(tree.get_by_id(id), tree.get(current));
        }
        tree.validate();
    }

    #[test]
    fn mixed_operations_match_vec_oracle_and_keep_avl_invariants() {
        let mut tree = IndexedTree::new();
        let mut model: Vec<(super::NodeId, i32)> = Vec::new();
        let mut state = 0x9e37_79b9_u64;
        for step in 0..4_000_i32 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            if !model.is_empty() && state % 3 == 0 {
                let position = (state as usize) % model.len();
                let expected = model.remove(position).1;
                assert_eq!(tree.remove(position), Ok(expected));
            } else {
                let position = if model.is_empty() {
                    0
                } else {
                    (state as usize) % (model.len() + 1)
                };
                let id = tree.insert(position, step).unwrap();
                model.insert(position, (id, step));
            }
            assert_matches(&tree, &model);
        }
    }

    #[test]
    fn monotone_insertions_stay_balanced() {
        let mut tree = IndexedTree::new();
        let mut ids = Vec::new();
        for value in 0..20_000_i32 {
            ids.push(tree.insert(tree.len(), value).unwrap());
        }
        let (height, size) = tree.validate();
        assert_eq!(size, 20_000);
        assert!(height < 32, "AVL height {height} is not logarithmic");
        for (position, id) in ids.iter().enumerate() {
            assert_eq!(tree.position_of(*id), Some(position));
        }
    }

    #[test]
    fn identity_exhaustion_is_atomic() {
        let mut tree = IndexedTree::new();
        tree.next_id = Some(u64::MAX);
        let last_id = tree.insert(0, 8).unwrap();
        assert_eq!(last_id, super::NodeId(u64::MAX));
        assert_eq!(
            tree.insert(1, 9),
            Err(IndexedTreeError::IdentifierExhausted)
        );
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.get_by_id(last_id), Some(&8));
        tree.validate();

        let mut capacity_limited = IndexedTree::new();
        capacity_limited.arena.next_slot = None;
        assert_eq!(
            capacity_limited.insert(0, 1),
            Err(IndexedTreeError::CapacityExhausted)
        );
        assert!(capacity_limited.is_empty());
        assert_eq!(capacity_limited.next_id, Some(0));
    }
}
