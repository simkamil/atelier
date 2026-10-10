// SPDX-License-Identifier: AGPL-3.0-or-later
//! Stable, directed selections and explicit position maps for document edits.
//!
//! Offsets count UTF-8 bytes. This module deliberately has no text content, so
//! the model owning a text node must validate that offsets are character
//! boundaries before publishing edits or selections.

use core::fmt;

use crate::NodeId;

/// Maximum number of ranges retained in a single selection.
pub const MAX_SELECTION_RANGES: usize = 64;

/// Which side of inserted text an endpoint at the insertion point follows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Affinity {
    /// Keep the endpoint before text inserted at its position.
    Before,
    /// Move the endpoint after text inserted at its position.
    After,
}

/// A node identity and a UTF-8 byte offset within that node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextPosition {
    pub node: NodeId,
    pub offset: usize,
    pub affinity: Affinity,
}

impl TextPosition {
    /// Creates a position. UTF-8 boundary validation belongs to the text owner.
    pub const fn new(node: NodeId, offset: usize, affinity: Affinity) -> Self {
        Self {
            node,
            offset,
            affinity,
        }
    }
}

/// A directed range; anchor and head are preserved in their supplied order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionRange {
    pub anchor: TextPosition,
    pub head: TextPosition,
}

impl SelectionRange {
    pub const fn new(anchor: TextPosition, head: TextPosition) -> Self {
        Self { anchor, head }
    }

    /// A caret whose default affinity follows text inserted at the caret.
    pub const fn caret(node: NodeId, offset: usize) -> Self {
        let position = TextPosition::new(node, offset, Affinity::After);
        Self::new(position, position)
    }
}

/// One or more directed ranges and the index of the primary range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    ranges: Vec<SelectionRange>,
    primary: usize,
}

impl Selection {
    /// Constructs a selection while preserving range order.
    pub fn new(ranges: Vec<SelectionRange>, primary: usize) -> Result<Self, SelectionError> {
        if ranges.is_empty() {
            return Err(SelectionError::Empty);
        }
        if ranges.len() > MAX_SELECTION_RANGES {
            return Err(SelectionError::TooManyRanges {
                limit: MAX_SELECTION_RANGES,
            });
        }
        if primary >= ranges.len() {
            return Err(SelectionError::PrimaryOutOfBounds {
                primary,
                len: ranges.len(),
            });
        }
        Ok(Self { ranges, primary })
    }

    /// Creates a collapsed selection with `After` affinity at both endpoints.
    pub fn caret(node: NodeId, offset: usize) -> Self {
        Self {
            ranges: vec![SelectionRange::caret(node, offset)],
            primary: 0,
        }
    }

    pub fn ranges(&self) -> &[SelectionRange] {
        &self.ranges
    }
    pub fn primary_index(&self) -> usize {
        self.primary
    }
    pub fn primary_range(&self) -> &SelectionRange {
        &self.ranges[self.primary]
    }

    /// Returns a mapped copy. The original is unchanged if any endpoint fails.
    pub fn mapped(&self, event: PositionMap) -> Result<Self, SelectionError> {
        event.validate()?;
        let mut candidate = self.clone();
        for range in &mut candidate.ranges {
            range.anchor = event.map_position(range.anchor)?;
            range.head = event.map_position(range.head)?;
        }
        Ok(candidate)
    }

    /// Applies a map atomically: publish only after every endpoint succeeds.
    pub fn transform(&mut self, event: PositionMap) -> Result<(), SelectionError> {
        let candidate = self.mapped(event)?;
        *self = candidate;
        Ok(())
    }
}

/// A concrete structural or text edit that can map selection positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionMap {
    /// Insert `byte_len` bytes at `at` in `node`.
    Insert {
        node: NodeId,
        at: usize,
        byte_len: usize,
    },
    /// Delete the half-open byte interval `[start, end)` from `node`.
    Delete {
        node: NodeId,
        start: usize,
        end: usize,
    },
    /// Split `source` at `at`, moving the suffix into `right`.
    Split {
        source: NodeId,
        right: NodeId,
        at: usize,
    },
    /// Merge all content from `right` onto the end of `left`.
    Merge {
        left: NodeId,
        right: NodeId,
        left_len: usize,
    },
    /// Remove a node and map its endpoints to the caller-provided fallback.
    Remove {
        node: NodeId,
        fallback: TextPosition,
    },
}

impl PositionMap {
    fn validate(self) -> Result<(), SelectionError> {
        match self {
            Self::Delete { start, end, .. } if start > end => {
                Err(SelectionError::InvalidInterval { start, end })
            }
            Self::Split { source, right, .. } if source == right => {
                Err(SelectionError::ConflictingNodes)
            }
            Self::Merge { left, right, .. } if left == right => {
                Err(SelectionError::ConflictingNodes)
            }
            Self::Remove { node, fallback } if fallback.node == node => {
                Err(SelectionError::InvalidFallback)
            }
            _ => Ok(()),
        }
    }

    fn map_position(self, position: TextPosition) -> Result<TextPosition, SelectionError> {
        let mut mapped = position;
        match self {
            Self::Insert { node, at, byte_len } if position.node == node => {
                if position.offset > at
                    || (position.offset == at && position.affinity == Affinity::After)
                {
                    mapped.offset = position
                        .offset
                        .checked_add(byte_len)
                        .ok_or(SelectionError::OffsetOverflow)?;
                }
            }
            Self::Delete { node, start, end } if position.node == node => {
                if position.offset >= start && position.offset <= end {
                    mapped.offset = start;
                } else if position.offset > end {
                    mapped.offset = position
                        .offset
                        .checked_sub(end - start)
                        .ok_or(SelectionError::OffsetOverflow)?;
                }
            }
            Self::Split { source, right, at } if position.node == source => {
                if position.offset > at
                    || (position.offset == at && position.affinity == Affinity::After)
                {
                    mapped.node = right;
                    mapped.offset = position
                        .offset
                        .checked_sub(at)
                        .ok_or(SelectionError::OffsetOverflow)?;
                }
            }
            Self::Merge {
                left,
                right,
                left_len,
            } if position.node == right => {
                mapped.node = left;
                mapped.offset = left_len
                    .checked_add(position.offset)
                    .ok_or(SelectionError::OffsetOverflow)?;
            }
            Self::Remove { node, fallback } if position.node == node => mapped = fallback,
            _ => {}
        }
        Ok(mapped)
    }
}

/// Invalid selection construction or position mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionError {
    Empty,
    TooManyRanges { limit: usize },
    PrimaryOutOfBounds { primary: usize, len: usize },
    InvalidInterval { start: usize, end: usize },
    ConflictingNodes,
    InvalidFallback,
    OffsetOverflow,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a selection must contain at least one range"),
            Self::TooManyRanges { limit } => write!(f, "selection exceeds the {limit}-range limit"),
            Self::PrimaryOutOfBounds { primary, len } => {
                write!(f, "primary range {primary} is outside {len} ranges")
            }
            Self::InvalidInterval { start, end } => {
                write!(f, "invalid half-open interval [{start}, {end})")
            }
            Self::ConflictingNodes => f.write_str("source and target node identities must differ"),
            Self::InvalidFallback => {
                f.write_str("fallback position must belong to a surviving node")
            }
            Self::OffsetOverflow => f.write_str("mapped text offset overflows usize"),
        }
    }
}

impl std::error::Error for SelectionError {}

#[cfg(test)]
mod tests {
    use super::{
        Affinity, MAX_SELECTION_RANGES, PositionMap, Selection, SelectionError, SelectionRange,
        TextPosition,
    };
    use crate::{
        CodecError, EditKind, Encoder, History, NodeId, Operation, Transaction, TransactionalState,
    };

    fn p(node: u64, offset: usize, affinity: Affinity) -> TextPosition {
        TextPosition::new(NodeId(node), offset, affinity)
    }
    fn range(a: TextPosition, h: TextPosition) -> SelectionRange {
        SelectionRange::new(a, h)
    }

    #[test]
    fn selection_validates_count_primary_and_preserves_order_and_direction() {
        assert_eq!(Selection::new(vec![], 0), Err(SelectionError::Empty));
        assert!(matches!(
            Selection::new(vec![SelectionRange::caret(NodeId(1), 0)], 1),
            Err(SelectionError::PrimaryOutOfBounds { .. })
        ));
        let ranges: Vec<_> = (0..MAX_SELECTION_RANGES + 1)
            .map(|_| SelectionRange::caret(NodeId(1), 0))
            .collect();
        assert!(matches!(
            Selection::new(ranges, 0),
            Err(SelectionError::TooManyRanges { .. })
        ));
        let original = Selection::new(
            vec![
                range(p(1, 8, Affinity::After), p(1, 2, Affinity::Before)),
                SelectionRange::caret(NodeId(2), 4),
            ],
            1,
        )
        .unwrap();
        let mapped = original
            .mapped(PositionMap::Insert {
                node: NodeId(1),
                at: 5,
                byte_len: 3,
            })
            .unwrap();
        assert_eq!(
            mapped.ranges()[0],
            range(p(1, 11, Affinity::After), p(1, 2, Affinity::Before))
        );
        assert_eq!(mapped.primary_index(), 1);
    }

    #[test]
    fn insert_respects_affinity_and_caret_defaults_after() {
        let selection = Selection::new(
            vec![range(p(1, 4, Affinity::Before), p(1, 4, Affinity::After))],
            0,
        )
        .unwrap();
        let mapped = selection
            .mapped(PositionMap::Insert {
                node: NodeId(1),
                at: 4,
                byte_len: 2,
            })
            .unwrap();
        assert_eq!(
            mapped.ranges()[0],
            range(p(1, 4, Affinity::Before), p(1, 6, Affinity::After))
        );
        assert_eq!(
            Selection::caret(NodeId(1), 4)
                .mapped(PositionMap::Insert {
                    node: NodeId(1),
                    at: 4,
                    byte_len: 2
                })
                .unwrap(),
            Selection::caret(NodeId(1), 6)
        );
        assert_eq!(selection.ranges()[0].head.offset, 4);
    }

    #[test]
    fn delete_collapses_inclusive_end_and_shifts_after() {
        let selection = Selection::new(
            vec![
                range(p(1, 4, Affinity::Before), p(1, 10, Affinity::After)),
                SelectionRange::caret(NodeId(1), 13),
            ],
            0,
        )
        .unwrap();
        let mapped = selection
            .mapped(PositionMap::Delete {
                node: NodeId(1),
                start: 5,
                end: 10,
            })
            .unwrap();
        assert_eq!(
            mapped.ranges()[0],
            range(p(1, 4, Affinity::Before), p(1, 5, Affinity::After))
        );
        assert_eq!(mapped.ranges()[1], SelectionRange::caret(NodeId(1), 8));
    }

    #[test]
    fn split_boundary_uses_affinity_and_merge_adds_left_length() {
        let selection = Selection::new(
            vec![
                range(p(1, 3, Affinity::Before), p(1, 3, Affinity::After)),
                SelectionRange::caret(NodeId(2), 7),
            ],
            1,
        )
        .unwrap();
        let split = selection
            .mapped(PositionMap::Split {
                source: NodeId(1),
                right: NodeId(2),
                at: 3,
            })
            .unwrap();
        assert_eq!(
            split.ranges()[0],
            range(p(1, 3, Affinity::Before), p(2, 0, Affinity::After))
        );
        let merged = split
            .mapped(PositionMap::Merge {
                left: NodeId(1),
                right: NodeId(2),
                left_len: 3,
            })
            .unwrap();
        assert_eq!(
            merged.ranges()[0],
            range(p(1, 3, Affinity::Before), p(1, 3, Affinity::After))
        );
        assert_eq!(merged.ranges()[1].anchor, p(1, 10, Affinity::After));
    }

    #[test]
    fn remove_uses_explicit_surviving_fallback() {
        let mut selection = Selection::new(
            vec![range(p(1, 2, Affinity::Before), p(2, 9, Affinity::After))],
            0,
        )
        .unwrap();
        selection
            .transform(PositionMap::Remove {
                node: NodeId(1),
                fallback: p(3, 6, Affinity::After),
            })
            .unwrap();
        assert_eq!(
            selection.ranges()[0],
            range(p(3, 6, Affinity::After), p(2, 9, Affinity::After))
        );
        assert_eq!(
            selection.transform(PositionMap::Remove {
                node: NodeId(3),
                fallback: p(3, 0, Affinity::After)
            }),
            Err(SelectionError::InvalidFallback)
        );
    }

    #[test]
    fn mapping_failure_is_atomic_for_every_endpoint() {
        let original = Selection::new(
            vec![range(
                p(1, 2, Affinity::After),
                p(1, usize::MAX, Affinity::After),
            )],
            0,
        )
        .unwrap();
        let mut candidate = original.clone();
        assert_eq!(
            candidate.transform(PositionMap::Insert {
                node: NodeId(1),
                at: 0,
                byte_len: 1
            }),
            Err(SelectionError::OffsetOverflow)
        );
        assert_eq!(candidate, original);
        assert_eq!(
            original.mapped(PositionMap::Delete {
                node: NodeId(1),
                start: 4,
                end: 3
            }),
            Err(SelectionError::InvalidInterval { start: 4, end: 3 })
        );
    }

    #[test]
    fn offsets_are_bytes_and_utf8_boundary_validation_is_deferred() {
        // In "é" (two UTF-8 bytes), byte offset 1 is intentionally representable
        // here; the text owner must reject it as a non-character boundary.
        let mid_codepoint = Selection::caret(NodeId(1), 1);
        assert_eq!(mid_codepoint.primary_range().head.offset, 1);
        assert_eq!(
            mid_codepoint
                .mapped(PositionMap::Insert {
                    node: NodeId(1),
                    at: 1,
                    byte_len: 2
                })
                .unwrap()
                .primary_range()
                .head
                .offset,
            3
        );
    }

    #[derive(Default)]
    struct State(i32);
    impl TransactionalState for State {
        type Checkpoint = i32;
        fn checkpoint(&self) -> i32 {
            self.0
        }
        fn restore(&mut self, checkpoint: i32) {
            self.0 = checkpoint;
        }
    }
    struct Add(i32);
    impl Operation<State> for Add {
        type Error = &'static str;
        fn apply(&self, state: &mut State) -> Result<(), Self::Error> {
            state.0 += self.0;
            Ok(())
        }
        fn invert(&self, _: &State) -> Result<Self, Self::Error> {
            Ok(Self(-self.0))
        }
        fn encode(&self, _: &mut Encoder) -> Result<(), CodecError> {
            Ok(())
        }
        fn decode(_: &mut crate::Decoder<'_>) -> Result<Self, CodecError> {
            unreachable!()
        }
    }

    #[test]
    fn concrete_selection_snapshots_restore_through_history_undo_and_redo() {
        let before = Selection::new(
            vec![
                range(p(1, 1, Affinity::Before), p(1, 3, Affinity::After)),
                SelectionRange::caret(NodeId(2), 5),
            ],
            1,
        )
        .unwrap();
        let after = Selection::new(vec![SelectionRange::caret(NodeId(1), 8)], 0).unwrap();
        let mut history = History::new(before.clone(), 10_000);
        let mut state = State::default();
        history
            .apply(
                &mut state,
                Transaction::new(vec![Add(7)]).unwrap(),
                after.clone(),
                1,
                EditKind::Other,
                100,
            )
            .unwrap();
        assert_eq!(history.selection(), &after);
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(history.selection(), &before);
        assert_eq!(state.0, 0);
        assert!(history.redo(&mut state).unwrap());
        assert_eq!(history.selection(), &after);
        assert_eq!(state.0, 7);
    }
}
