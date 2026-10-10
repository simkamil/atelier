// SPDX-License-Identifier: AGPL-3.0-or-later
//! Indexed document nodes, atomic transactions, history, selections and recovery.
//!
//! Positions are implicit in the in-order traversal. Subtree sizes make rank
//! lookup and insertion logarithmic, while parent links make the reverse
//! identity-to-position lookup logarithmic as well.

#![forbid(unsafe_code)]

use core::fmt;
use std::collections::BTreeMap;

pub mod journal;
pub mod selection;

mod history;
pub use history::{EditKind, History, HistoryAction, HistoryError};

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

#[derive(Clone)]
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
#[derive(Clone)]
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

#[derive(Clone)]
enum RadixPage<T> {
    Branch(Box<[Option<RadixPage<T>>; RADIX_FANOUT]>),
    Leaf(Box<[Option<ArenaSlot<T>>; RADIX_FANOUT]>),
}

#[derive(Clone)]
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

#[derive(Clone)]
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

    fn restore_id(
        &mut self,
        position: usize,
        id: NodeId,
        value: T,
    ) -> Result<(), IndexedTreeError> {
        if position > self.len {
            return Err(IndexedTreeError::PositionOutOfBounds {
                position,
                len: self.len,
            });
        }
        if self.next_id.is_some_and(|next| id.0 >= next) {
            return Err(IndexedTreeError::IdentifierExhausted);
        }
        if self.by_id.contains_key(&id) {
            return Err(IndexedTreeError::IdentifierExhausted);
        }
        if !self.arena.can_allocate() {
            return Err(IndexedTreeError::CapacityExhausted);
        }
        let node = self
            .arena
            .allocate(Node::new(id, value))
            .ok_or(IndexedTreeError::CapacityExhausted)?;
        self.root = Some(self.insert_rec(self.root, position, node));
        if let Some(root) = self.root {
            self.node_mut(root).parent = None;
        }
        self.by_id.insert(id, node);
        self.len += 1;
        Ok(())
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

/// Maximum encoded transaction size accepted by the version-1 codec.
pub const MAX_TRANSACTION_BYTES: usize = 1_048_576;
/// Maximum number of operations in one encoded transaction.
pub const MAX_TRANSACTION_OPERATIONS: usize = 4_096;
/// Maximum encoded payload for one operation.
pub const MAX_OPERATION_BYTES: usize = 65_536;
const TRANSACTION_MAGIC: &[u8; 4] = b"ATOP";
const TRANSACTION_VERSION: u16 = 1;

/// A bounded-codec failure. Decoding never allocates beyond the declared limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecError {
    TooLarge,
    TooManyOperations,
    UnknownVersion(u16),
    InvalidData,
    Truncated,
    TrailingData,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "transaction codec error: {self:?}")
    }
}

impl std::error::Error for CodecError {}

/// Writer supplied to operation codecs. Its buffer is capped by the transaction limit.
pub struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }
    pub fn write_u8(&mut self, value: u8) -> Result<(), CodecError> {
        self.write_bytes(&[value])
    }
    pub fn write_u32(&mut self, value: u32) -> Result<(), CodecError> {
        self.write_bytes(&value.to_le_bytes())
    }
    pub fn write_u64(&mut self, value: u64) -> Result<(), CodecError> {
        self.write_bytes(&value.to_le_bytes())
    }
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_OPERATION_BYTES)
            .is_none()
        {
            return Err(CodecError::TooLarge);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

/// Reader supplied to operation codecs. All reads are checked and bounded.
pub struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub fn read_u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.read_bytes(1)?[0])
    }
    pub fn read_u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.read_bytes(4)?
                .try_into()
                .map_err(|_| CodecError::Truncated)?,
        ))
    }
    pub fn read_u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(
            self.read_bytes(8)?
                .try_into()
                .map_err(|_| CodecError::Truncated)?,
        ))
    }
    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], CodecError> {
        let end = self.offset.checked_add(len).ok_or(CodecError::TooLarge)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(CodecError::Truncated)?;
        self.offset = end;
        Ok(result)
    }
    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

/// A state chooses its own checkpoint representation and restoration strategy.
pub trait TransactionalState {
    type Checkpoint;
    fn checkpoint(&self) -> Self::Checkpoint;
    fn restore(&mut self, checkpoint: Self::Checkpoint);
}

/// One reversible model operation. `invert` prepares the inverse from the state
/// immediately before this operation is applied.
pub trait Operation<S>: Sized {
    type Error;
    fn apply(&self, state: &mut S) -> Result<(), Self::Error>;
    fn invert(&self, state: &S) -> Result<Self, Self::Error>;
    fn encode(&self, encoder: &mut Encoder) -> Result<(), CodecError>;
    fn decode(decoder: &mut Decoder<'_>) -> Result<Self, CodecError>;
}

/// An ordered group of operations. The codec is deterministic and versioned.
pub struct Transaction<O> {
    operations: Vec<O>,
}

impl<O> Transaction<O> {
    pub fn new(operations: Vec<O>) -> Result<Self, CodecError> {
        if operations.len() > MAX_TRANSACTION_OPERATIONS {
            return Err(CodecError::TooManyOperations);
        }
        Ok(Self { operations })
    }
    pub fn operations(&self) -> &[O] {
        &self.operations
    }

    pub(crate) fn into_operations(self) -> Vec<O> {
        self.operations
    }
}

/// Identifies whether preparing an inverse or applying an operation failed.
#[derive(Debug, PartialEq, Eq)]
pub enum TransactionError<E> {
    Inversion { index: usize, source: E },
    Application { index: usize, source: E },
}

impl<E: fmt::Display> fmt::Display for TransactionError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inversion { index, source } => write!(
                f,
                "could not prepare inverse for operation {index}: {source}"
            ),
            Self::Application { index, source } => write!(f, "operation {index} failed: {source}"),
        }
    }
}
impl<E: fmt::Debug + fmt::Display> std::error::Error for TransactionError<E> {}

impl<O> Transaction<O> {
    /// Applies every operation in order. Any failure restores the exact checkpoint.
    /// On success, returns the operations that undo the transaction in reverse order.
    pub fn apply<S>(&self, state: &mut S) -> Result<Self, TransactionError<O::Error>>
    where
        S: TransactionalState,
        O: Operation<S>,
    {
        let checkpoint = state.checkpoint();
        let mut inverses = Vec::with_capacity(self.operations.len());
        for (index, operation) in self.operations.iter().enumerate() {
            let inverse = match operation.invert(state) {
                Ok(inverse) => inverse,
                Err(source) => {
                    state.restore(checkpoint);
                    return Err(TransactionError::Inversion { index, source });
                }
            };
            if let Err(source) = operation.apply(state) {
                state.restore(checkpoint);
                return Err(TransactionError::Application { index, source });
            }
            inverses.push(inverse);
        }
        inverses.reverse();
        Ok(Self {
            operations: inverses,
        })
    }
}

impl<O> Transaction<O> {
    /// Encodes a transaction as `ATOP`, version, count, then length-delimited operations.
    pub fn encode<S>(&self) -> Result<Vec<u8>, CodecError>
    where
        O: Operation<S>,
    {
        if self.operations.len() > MAX_TRANSACTION_OPERATIONS {
            return Err(CodecError::TooManyOperations);
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(TRANSACTION_MAGIC);
        bytes.extend_from_slice(&TRANSACTION_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(self.operations.len() as u32).to_le_bytes());
        for operation in &self.operations {
            let mut encoder = Encoder::new();
            operation.encode(&mut encoder)?;
            let len = u32::try_from(encoder.bytes.len()).map_err(|_| CodecError::TooLarge)?;
            let new_len = bytes
                .len()
                .checked_add(4 + len as usize)
                .ok_or(CodecError::TooLarge)?;
            if new_len > MAX_TRANSACTION_BYTES {
                return Err(CodecError::TooLarge);
            }
            bytes.extend_from_slice(&len.to_le_bytes());
            bytes.extend_from_slice(&encoder.bytes);
        }
        Ok(bytes)
    }

    /// Decodes and validates all operations before a caller can apply the transaction.
    pub fn decode<S>(bytes: &[u8]) -> Result<Self, CodecError>
    where
        O: Operation<S>,
    {
        if bytes.len() > MAX_TRANSACTION_BYTES {
            return Err(CodecError::TooLarge);
        }
        let mut decoder = Decoder::new(bytes);
        if decoder.read_bytes(4)? != TRANSACTION_MAGIC {
            return Err(CodecError::InvalidData);
        }
        let version = u16::from_le_bytes(
            decoder
                .read_bytes(2)?
                .try_into()
                .map_err(|_| CodecError::Truncated)?,
        );
        if version != TRANSACTION_VERSION {
            return Err(CodecError::UnknownVersion(version));
        }
        let count = decoder.read_u32()? as usize;
        if count > MAX_TRANSACTION_OPERATIONS {
            return Err(CodecError::TooManyOperations);
        }
        let mut operations = Vec::with_capacity(count);
        for _ in 0..count {
            let len = decoder.read_u32()? as usize;
            if len > MAX_OPERATION_BYTES {
                return Err(CodecError::TooLarge);
            }
            let payload = decoder.read_bytes(len)?;
            let mut operation_decoder = Decoder::new(payload);
            let operation = O::decode(&mut operation_decoder)?;
            if !operation_decoder.is_finished() {
                return Err(CodecError::TrailingData);
            }
            operations.push(operation);
        }
        if !decoder.is_finished() {
            return Err(CodecError::TrailingData);
        }
        Ok(Self { operations })
    }
}

/// Errors for the `IndexedTree` operation adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationError {
    PreconditionFailed,
    Tree(IndexedTreeError),
    InvalidData,
}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "operation failed: {self:?}")
    }
}
impl std::error::Error for OperationError {}

/// Values supported by the built-in deterministic tree-operation codec.
pub trait BinaryValue: Sized {
    fn encode_value(&self, encoder: &mut Encoder) -> Result<(), CodecError>;
    fn decode_value(decoder: &mut Decoder<'_>) -> Result<Self, CodecError>;
}

impl BinaryValue for i32 {
    fn encode_value(&self, e: &mut Encoder) -> Result<(), CodecError> {
        e.write_bytes(&self.to_le_bytes())
    }
    fn decode_value(d: &mut Decoder<'_>) -> Result<Self, CodecError> {
        Ok(i32::from_le_bytes(
            d.read_bytes(4)?
                .try_into()
                .map_err(|_| CodecError::Truncated)?,
        ))
    }
}
impl BinaryValue for u64 {
    fn encode_value(&self, e: &mut Encoder) -> Result<(), CodecError> {
        e.write_u64(*self)
    }
    fn decode_value(d: &mut Decoder<'_>) -> Result<Self, CodecError> {
        d.read_u64()
    }
}
impl BinaryValue for String {
    fn encode_value(&self, e: &mut Encoder) -> Result<(), CodecError> {
        let len = u32::try_from(self.len()).map_err(|_| CodecError::TooLarge)?;
        e.write_u32(len)?;
        e.write_bytes(self.as_bytes())
    }
    fn decode_value(d: &mut Decoder<'_>) -> Result<Self, CodecError> {
        let len = d.read_u32()? as usize;
        String::from_utf8(d.read_bytes(len)?.to_vec()).map_err(|_| CodecError::InvalidData)
    }
}

/// Concrete insert/remove operations for `IndexedTree`. The clone bound lives
/// only on this adapter; generic operations and states need not be cloneable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexedTreeOperation<T> {
    Insert {
        position: usize,
        value: T,
    },
    Remove {
        id: NodeId,
    },
    Restore {
        position: usize,
        id: NodeId,
        value: T,
    },
}

impl<T: Clone> TransactionalState for IndexedTree<T> {
    type Checkpoint = Self;
    fn checkpoint(&self) -> Self::Checkpoint {
        self.clone()
    }
    fn restore(&mut self, checkpoint: Self::Checkpoint) {
        *self = checkpoint;
    }
}

impl<T: Clone + BinaryValue> Operation<IndexedTree<T>> for IndexedTreeOperation<T> {
    type Error = OperationError;
    fn invert(&self, tree: &IndexedTree<T>) -> Result<Self, Self::Error> {
        match self {
            Self::Insert { position, .. } => {
                if *position > tree.len {
                    return Err(OperationError::Tree(
                        IndexedTreeError::PositionOutOfBounds {
                            position: *position,
                            len: tree.len,
                        },
                    ));
                }
                let next = tree
                    .next_id
                    .ok_or(OperationError::Tree(IndexedTreeError::IdentifierExhausted))?;
                if !tree.arena.can_allocate() {
                    return Err(OperationError::Tree(IndexedTreeError::CapacityExhausted));
                }
                Ok(Self::Remove { id: NodeId(next) })
            }
            Self::Remove { id } => {
                let position = tree
                    .position_of(*id)
                    .ok_or(OperationError::PreconditionFailed)?;
                let value = tree
                    .get(position)
                    .ok_or(OperationError::PreconditionFailed)?
                    .clone();
                Ok(Self::Restore {
                    position,
                    id: *id,
                    value,
                })
            }
            Self::Restore { id, .. } => {
                if tree.position_of(*id).is_some() {
                    return Err(OperationError::PreconditionFailed);
                }
                Ok(Self::Remove { id: *id })
            }
        }
    }
    fn apply(&self, tree: &mut IndexedTree<T>) -> Result<(), Self::Error> {
        match self {
            Self::Insert { position, value } => tree
                .insert(*position, value.clone())
                .map(|_| ())
                .map_err(OperationError::Tree),
            Self::Remove { id } => {
                let position = tree
                    .position_of(*id)
                    .ok_or(OperationError::PreconditionFailed)?;
                tree.remove(position)
                    .map(|_| ())
                    .map_err(OperationError::Tree)
            }
            Self::Restore {
                position,
                id,
                value,
            } => tree
                .restore_id(*position, *id, value.clone())
                .map_err(OperationError::Tree),
        }
    }
    fn encode(&self, e: &mut Encoder) -> Result<(), CodecError> {
        match self {
            Self::Insert { position, value } => {
                e.write_u8(0)?;
                e.write_u64(u64::try_from(*position).map_err(|_| CodecError::TooLarge)?)?;
                value.encode_value(e)
            }
            Self::Remove { id } => {
                e.write_u8(1)?;
                e.write_u64(id.0)
            }
            Self::Restore {
                position,
                id,
                value,
            } => {
                e.write_u8(2)?;
                e.write_u64(u64::try_from(*position).map_err(|_| CodecError::TooLarge)?)?;
                e.write_u64(id.0)?;
                value.encode_value(e)
            }
        }
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CodecError> {
        match d.read_u8()? {
            0 => {
                let position =
                    usize::try_from(d.read_u64()?).map_err(|_| CodecError::InvalidData)?;
                Ok(Self::Insert {
                    position,
                    value: T::decode_value(d)?,
                })
            }
            1 => Ok(Self::Remove {
                id: NodeId(d.read_u64()?),
            }),
            2 => {
                let position =
                    usize::try_from(d.read_u64()?).map_err(|_| CodecError::InvalidData)?;
                let id = NodeId(d.read_u64()?);
                Ok(Self::Restore {
                    position,
                    id,
                    value: T::decode_value(d)?,
                })
            }
            _ => Err(CodecError::InvalidData),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodecError, IndexedTree, IndexedTreeError, IndexedTreeOperation as TreeOp,
        MAX_TRANSACTION_BYTES, NodeId, OperationError, Transaction, TransactionError,
    };

    struct PlainState(i32);
    impl super::TransactionalState for PlainState {
        type Checkpoint = i32;
        fn checkpoint(&self) -> i32 {
            self.0
        }
        fn restore(&mut self, checkpoint: i32) {
            self.0 = checkpoint;
        }
    }
    #[derive(Clone)]
    struct PlainOp {
        amount: i32,
        fail: bool,
    }
    impl super::Operation<PlainState> for PlainOp {
        type Error = &'static str;
        fn apply(&self, state: &mut PlainState) -> Result<(), Self::Error> {
            state.0 += self.amount;
            if self.fail {
                return Err("requested failure");
            }
            Ok(())
        }
        fn invert(&self, _: &PlainState) -> Result<Self, Self::Error> {
            Ok(Self {
                amount: -self.amount,
                fail: false,
            })
        }
        fn encode(&self, encoder: &mut super::Encoder) -> Result<(), super::CodecError> {
            encoder.write_u32(self.amount as u32)
        }
        fn decode(decoder: &mut super::Decoder<'_>) -> Result<Self, super::CodecError> {
            Ok(Self {
                amount: decoder.read_u32()? as i32,
                fail: false,
            })
        }
    }

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

    #[test]
    fn transaction_apply_and_inverse_restore_ids_and_order_including_two_child_delete() {
        let mut tree = IndexedTree::new();
        let ids: Vec<_> = (0..15)
            .map(|value| tree.insert(tree.len(), value).unwrap())
            .collect();
        let next_before = tree.next_id;
        let root = tree.root.unwrap();
        assert!(tree.node(root).left.is_some() && tree.node(root).right.is_some());
        let remove_id = tree.node(root).id;
        let transaction = Transaction::new(vec![TreeOp::Remove { id: remove_id }]).unwrap();
        let inverse = transaction.apply(&mut tree).unwrap();
        assert_eq!(tree.get_by_id(remove_id), None);
        inverse.apply(&mut tree).unwrap();
        assert_matches(
            &tree,
            &ids.iter()
                .enumerate()
                .map(|(i, id)| (*id, i as i32))
                .collect::<Vec<_>>(),
        );
        assert_eq!(tree.next_id, next_before);
        let fresh = tree.insert(tree.len(), 99).unwrap();
        assert_eq!(fresh, NodeId(15));
    }

    #[test]
    fn transaction_failure_restores_exact_checkpoint_and_provisional_id() {
        let mut tree = IndexedTree::new();
        let original = tree.insert(0, 10).unwrap();
        let transaction = Transaction::new(vec![
            TreeOp::Insert {
                position: 1,
                value: 20,
            },
            TreeOp::Remove { id: NodeId(999) },
        ])
        .unwrap();
        assert!(matches!(
            transaction.apply(&mut tree),
            Err(TransactionError::Inversion {
                index: 1,
                source: OperationError::PreconditionFailed
            })
        ));
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.get_by_id(original), Some(&10));
        assert_eq!(tree.insert(1, 30), Ok(NodeId(1)));
    }

    #[test]
    fn failed_operation_application_rolls_back_prior_mutations() {
        let mut tree = IndexedTree::new();
        let original = tree.insert(0, 10).unwrap();
        let transaction = Transaction::new(vec![
            TreeOp::Insert {
                position: 1,
                value: 20,
            },
            TreeOp::Insert {
                position: 99,
                value: 30,
            },
        ])
        .unwrap();
        assert!(matches!(
            transaction.apply(&mut tree),
            Err(TransactionError::Inversion { index: 1, .. })
        ));
        assert_eq!(tree.get_by_id(original), Some(&10));
        assert_eq!(tree.next_id, Some(1));
    }

    #[test]
    fn generic_non_clone_state_rolls_back_when_apply_fails_mid_transaction() {
        let mut state = PlainState(41);
        let transaction = Transaction::new(vec![
            PlainOp {
                amount: 5,
                fail: false,
            },
            PlainOp {
                amount: 100,
                fail: true,
            },
        ])
        .unwrap();
        assert!(matches!(
            transaction.apply(&mut state),
            Err(TransactionError::Application { index: 1, .. })
        ));
        assert_eq!(state.0, 41);
    }

    #[test]
    fn transaction_codec_is_deterministic_bounded_and_rejects_invalid_input() {
        let transaction = Transaction::new(vec![
            TreeOp::Insert {
                position: 0,
                value: 7,
            },
            TreeOp::Insert {
                position: 1,
                value: -4,
            },
        ])
        .unwrap();
        let encoded = transaction.encode::<IndexedTree<i32>>().unwrap();
        assert_eq!(encoded, transaction.encode::<IndexedTree<i32>>().unwrap());
        let decoded = Transaction::<TreeOp<i32>>::decode::<IndexedTree<i32>>(&encoded).unwrap();
        assert_eq!(decoded.operations(), transaction.operations());
        let mut unknown = encoded.clone();
        unknown[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            Transaction::<TreeOp<i32>>::decode::<IndexedTree<i32>>(&unknown),
            Err(CodecError::UnknownVersion(2))
        ));
        assert!(matches!(
            Transaction::<TreeOp<i32>>::decode::<IndexedTree<i32>>(&encoded[..encoded.len() - 1]),
            Err(CodecError::Truncated)
        ));
        assert!(matches!(
            Transaction::<TreeOp<i32>>::decode::<IndexedTree<i32>>(&vec![
                0;
                MAX_TRANSACTION_BYTES + 1
            ]),
            Err(CodecError::TooLarge)
        ));
    }

    #[test]
    fn deterministic_pseudo_random_transactions_round_trip() {
        let mut tree = IndexedTree::new();
        let mut model: Vec<(NodeId, i32)> = (0..12)
            .map(|value| {
                let id = tree.insert(tree.len(), value).unwrap();
                (id, value)
            })
            .collect();
        let mut seed = 0x8a5c_12d3_u64;
        let mut value = 12_i32;
        for turn in 0..200_i32 {
            let before = model.clone();
            let mut planned = before.clone();
            let mut removable_ids: Vec<_> = before.iter().map(|(id, _)| *id).collect();
            let mut planned_next_id = tree.next_id.unwrap();
            let mut operations = Vec::new();

            // Every batch includes a removal of an ID that existed at its start.
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let remove_index = seed as usize % removable_ids.len();
            let removed_id = removable_ids.swap_remove(remove_index);
            let remove_position = planned
                .iter()
                .position(|(id, _)| *id == removed_id)
                .unwrap();
            operations.push(TreeOp::Remove { id: removed_id });
            planned.remove(remove_position);

            // And every batch inserts, so successful inverses must keep IDs monotone.
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let insert_position = seed as usize % (planned.len() + 1);
            operations.push(TreeOp::Insert {
                position: insert_position,
                value,
            });
            planned.insert(insert_position, (NodeId(planned_next_id), value));
            planned_next_id = planned_next_id.checked_add(1).unwrap();
            value += 1;

            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let extra_count = 1 + (seed as usize % 4);
            for _ in 0..extra_count {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let should_remove = !removable_ids.is_empty() && seed & 1 == 0;
                if should_remove {
                    let candidate_index = seed as usize % removable_ids.len();
                    let id = removable_ids.swap_remove(candidate_index);
                    let position = planned
                        .iter()
                        .position(|(candidate, _)| *candidate == id)
                        .unwrap();
                    operations.push(TreeOp::Remove { id });
                    planned.remove(position);
                } else {
                    let position = seed as usize % (planned.len() + 1);
                    operations.push(TreeOp::Insert { position, value });
                    planned.insert(position, (NodeId(planned_next_id), value));
                    planned_next_id = planned_next_id.checked_add(1).unwrap();
                    value += 1;
                }
            }

            let transaction = Transaction::new(operations).unwrap();
            let encoded = transaction.encode::<IndexedTree<i32>>().unwrap();
            let decoded = Transaction::<TreeOp<i32>>::decode::<IndexedTree<i32>>(&encoded).unwrap();
            assert_eq!(decoded.operations(), transaction.operations());
            let inverse = decoded.apply(&mut tree).unwrap();
            model = planned;
            assert_matches(&tree, &model);
            assert_eq!(tree.next_id, Some(planned_next_id));
            let published_ids: Vec<_> = model
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| !before.iter().any(|(old_id, _)| old_id == id))
                .collect();

            inverse.apply(&mut tree).unwrap();
            model = before.clone();
            assert_eq!(model, before);
            assert_matches(&tree, &model);
            assert_eq!(tree.next_id, Some(planned_next_id));

            // Publishing a fresh ID after undo proves that IDs from the undone
            // successful transaction are not recycled for another node.
            let probe_value = -turn - 1;
            let probe = tree.insert(tree.len(), probe_value).unwrap();
            assert_eq!(probe, NodeId(planned_next_id));
            assert!(!before.iter().any(|(id, _)| *id == probe));
            assert!(!published_ids.contains(&probe));
            model.push((probe, probe_value));
            assert_matches(&tree, &model);
            value += 1;
        }
    }
}
