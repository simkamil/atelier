// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed errors of the rich document model.

use super::NodeId;
use core::fmt;

/// Why an operation or a validation was rejected. A rejected operation never
/// leaves a partially modified document behind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RichError {
    /// No node carries this identifier.
    NodeNotFound(NodeId),
    /// The node exists but is not of the kind the operation needs.
    WrongNodeKind { id: NodeId, expected: &'static str },
    /// An identifier is already used by another node or inline item.
    DuplicateId(NodeId),
    /// A child position lies outside the sequence.
    PositionOutOfBounds { position: usize, len: usize },
    /// A text range has `start > end`.
    InvalidRange { start: usize, end: usize },
    /// A byte offset exceeds the paragraph text.
    OffsetOutOfBounds { offset: usize, len: usize },
    /// A byte offset is not on an extended grapheme cluster boundary.
    NotGraphemeBoundary { offset: usize },
    /// A split would cut through a field result.
    SplitInsideField,
    /// Two bookmarks would carry the same name.
    DuplicateBookmarkName(String),
    /// A structural invariant is broken (static description).
    Invariant(&'static str),
    /// An attribute value is not acceptable.
    InvalidAttribute(&'static str),
    /// A table operation is not applicable or would break the grid.
    InvalidTable(&'static str),
    /// The node identifier space is exhausted.
    IdentifiersExhausted,
    /// The indexed tree cannot allocate another slot.
    CapacityExhausted,
    /// The binary journal codec is not provided for rich operations.
    CodecUnsupported,
}

impl fmt::Display for RichError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeNotFound(id) => write!(f, "node {id:?} does not exist"),
            Self::WrongNodeKind { id, expected } => {
                write!(f, "node {id:?} is not a {expected}")
            }
            Self::DuplicateId(id) => write!(f, "identifier {id:?} is already in use"),
            Self::PositionOutOfBounds { position, len } => {
                write!(f, "position {position} is outside a sequence of {len}")
            }
            Self::InvalidRange { start, end } => write!(f, "invalid range {start}..{end}"),
            Self::OffsetOutOfBounds { offset, len } => {
                write!(f, "offset {offset} exceeds text length {len}")
            }
            Self::NotGraphemeBoundary { offset } => {
                write!(f, "offset {offset} is not a grapheme boundary")
            }
            Self::SplitInsideField => f.write_str("a paragraph cannot be split inside a field"),
            Self::DuplicateBookmarkName(name) => write!(f, "bookmark name {name:?} already used"),
            Self::Invariant(what) => write!(f, "invariant violated: {what}"),
            Self::InvalidAttribute(what) => write!(f, "invalid attribute: {what}"),
            Self::InvalidTable(what) => write!(f, "invalid table operation: {what}"),
            Self::IdentifiersExhausted => f.write_str("node identifiers exhausted"),
            Self::CapacityExhausted => f.write_str("node storage exhausted"),
            Self::CodecUnsupported => f.write_str("rich operations have no binary codec yet"),
        }
    }
}

impl std::error::Error for RichError {}
