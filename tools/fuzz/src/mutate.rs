// SPDX-License-Identifier: AGPL-3.0-or-later
//! Byte-level mutations. Every function draws all its randomness from the
//! supplied [`Rng`], so a (seed, input) pair fully determines the result.

use crate::rng::Rng;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mutation {
    BitFlip,
    Interesting,
    Insert,
    Delete,
    Duplicate,
    Splice,
    Dictionary,
    Truncate,
}

pub const ALL: [Mutation; 8] = [
    Mutation::BitFlip,
    Mutation::Interesting,
    Mutation::Insert,
    Mutation::Delete,
    Mutation::Duplicate,
    Mutation::Splice,
    Mutation::Dictionary,
    Mutation::Truncate,
];

/// Inputs shared by the mutations.
pub struct Context<'a> {
    /// Another corpus entry, used by splicing.
    pub other: Option<&'a [u8]>,
    pub dictionary: &'a [&'a [u8]],
    /// Mutations never grow an input beyond this size.
    pub max_len: usize,
}

/// Boundary values for integer widths of 1, 2, 4 and 8 bytes.
pub const INTERESTING: [(usize, [u64; 6]); 4] = [
    (1, [0x00, 0x01, 0x7F, 0x80, 0xFF, 0xFE]),
    (2, [0x0000, 0x0001, 0x7FFF, 0x8000, 0xFFFF, 0xFFFE]),
    (
        4,
        [0, 1, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, 0xFFFF_FFFE],
    ),
    (
        8,
        [
            0,
            1,
            0x7FFF_FFFF_FFFF_FFFF,
            0x8000_0000_0000_0000,
            u64::MAX,
            u64::MAX - 1,
        ],
    ),
];

/// Apply one mutation. Returns false when it was not applicable (the input is
/// then untouched).
pub fn apply(kind: Mutation, rng: &mut Rng, data: &mut Vec<u8>, ctx: &Context<'_>) -> bool {
    match kind {
        Mutation::BitFlip => bit_flip(rng, data),
        Mutation::Interesting => interesting(rng, data),
        Mutation::Insert => insert(rng, data, ctx.max_len),
        Mutation::Delete => delete(rng, data),
        Mutation::Duplicate => duplicate(rng, data, ctx.max_len),
        Mutation::Splice => ctx
            .other
            .is_some_and(|other| splice(rng, data, other, ctx.max_len)),
        Mutation::Dictionary => dictionary(rng, data, ctx.dictionary, ctx.max_len),
        Mutation::Truncate => truncate(rng, data),
    }
}

/// Stack one to five mutations. Returns the mutations actually applied.
pub fn mutate(rng: &mut Rng, data: &mut Vec<u8>, ctx: &Context<'_>) -> Vec<Mutation> {
    let wanted = 1 + rng.below(3) + usize::from(rng.chance(1, 4)) * (1 + rng.below(2));
    let mut applied = Vec::new();
    for _ in 0..wanted {
        for _ in 0..4 {
            let kind = ALL[rng.below(ALL.len())];
            if apply(kind, rng, data, ctx) {
                applied.push(kind);
                break;
            }
        }
    }
    data.truncate(ctx.max_len);
    applied
}

/// Flip one to three bits, anywhere in the input.
pub fn bit_flip(rng: &mut Rng, data: &mut [u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    for _ in 0..=rng.below(3) {
        let at = rng.below(data.len());
        data[at] ^= 1 << rng.below(8);
    }
    true
}

/// Overwrite a 1, 2, 4 or 8 byte field with a boundary value, in a random
/// byte order.
pub fn interesting(rng: &mut Rng, data: &mut [u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let (width, values) = INTERESTING[rng.below(INTERESTING.len())];
    if width > data.len() {
        let at = rng.below(data.len());
        data[at] = if rng.chance(1, 2) { 0 } else { 0xFF };
        return true;
    }
    let value = values[rng.below(values.len())];
    let at = rng.below(data.len() - width + 1);
    let field = &mut data[at..at + width];
    if rng.chance(1, 2) {
        field.copy_from_slice(&value.to_le_bytes()[..width]);
    } else {
        field.copy_from_slice(&value.to_be_bytes()[8 - width..]);
    }
    true
}

/// Insert one to sixteen random (or repeated) bytes.
pub fn insert(rng: &mut Rng, data: &mut Vec<u8>, max_len: usize) -> bool {
    let room = max_len.saturating_sub(data.len());
    if room == 0 {
        return false;
    }
    let count = (1 + rng.below(16)).min(room);
    let at = rng.below(data.len() + 1);
    let repeated = rng.chance(1, 3);
    let fill = rng.byte();
    let bytes: Vec<u8> = (0..count)
        .map(|_| if repeated { fill } else { rng.byte() })
        .collect();
    data.splice(at..at, bytes);
    true
}

/// Remove one to sixteen bytes.
pub fn delete(rng: &mut Rng, data: &mut Vec<u8>) -> bool {
    if data.is_empty() {
        return false;
    }
    let count = 1 + rng.below(data.len().min(16));
    let at = rng.below(data.len() - count + 1);
    data.drain(at..at + count);
    true
}

/// Copy a block of up to 32 bytes to another position.
pub fn duplicate(rng: &mut Rng, data: &mut Vec<u8>, max_len: usize) -> bool {
    let room = max_len.saturating_sub(data.len());
    if data.is_empty() || room == 0 {
        return false;
    }
    let count = (1 + rng.below(data.len().min(32))).min(room);
    let from = rng.below(data.len() - count + 1);
    let block = data[from..from + count].to_vec();
    let to = rng.below(data.len() + 1);
    data.splice(to..to, block);
    true
}

/// Keep a prefix of `data` and continue with a suffix of `other`.
pub fn splice(rng: &mut Rng, data: &mut Vec<u8>, other: &[u8], max_len: usize) -> bool {
    if other.is_empty() {
        return false;
    }
    let keep = rng.below(data.len() + 1);
    let from = rng.below(other.len());
    data.truncate(keep);
    data.extend_from_slice(&other[from..]);
    data.truncate(max_len);
    true
}

/// Insert a dictionary token, or overwrite the bytes at its position.
pub fn dictionary(rng: &mut Rng, data: &mut Vec<u8>, tokens: &[&[u8]], max_len: usize) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let token = tokens[rng.below(tokens.len())];
    if token.is_empty() || token.len() > max_len {
        return false;
    }
    let at = rng.below(data.len() + 1);
    if rng.chance(1, 2) {
        let end = (at + token.len()).min(data.len());
        data.splice(at..end, token.iter().copied());
    } else {
        data.splice(at..at, token.iter().copied());
    }
    data.truncate(max_len);
    true
}

/// Cut the input at a random shorter length.
pub fn truncate(rng: &mut Rng, data: &mut Vec<u8>) -> bool {
    if data.len() < 2 {
        return false;
    }
    let new_len = rng.below(data.len());
    data.truncate(new_len);
    true
}
