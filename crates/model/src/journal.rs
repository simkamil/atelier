// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, generation-scoped recovery snapshots and transaction journals.
//!
//! This module stores bytes only. It does not write files or make durability
//! claims; the caller owns publication and synchronization.

use core::fmt;

use atelier_compress::crc32;

use crate::{CodecError, Operation, Transaction, TransactionError, TransactionalState};

const SNAPSHOT_MAGIC: &[u8; 4] = b"ATJS";
const JOURNAL_MAGIC: &[u8; 4] = b"ATJR";
const FORMAT_VERSION: u16 = 1;
const SNAPSHOT_HEADER_BYTES: usize = 4 + 2 + 16 + 16 + 8 + 8 + 4;
const JOURNAL_HEADER_BYTES: usize = 4 + 2 + 16 + 16 + 8;
const ENTRY_PREFIX_BYTES: usize = 4; // frame length
const ENTRY_FIXED_BYTES: usize = 8 + 4 + 4; // sequence, ATOP size, CRC

/// Default maximum complete snapshot envelope size (64 MiB).
pub const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
/// Default maximum complete journal size (64 MiB).
pub const DEFAULT_MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
/// Default maximum number of journal entries.
pub const DEFAULT_MAX_ENTRIES: usize = 100_000;

/// Opaque 128-bit document or generation identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RecoveryId(pub [u8; 16]);

/// Limits applied before copying, reserving, or decoding caller-provided data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalLimits {
    pub max_snapshot_bytes: usize,
    pub max_journal_bytes: usize,
    pub max_entries: usize,
}

impl Default for JournalLimits {
    fn default() -> Self {
        Self {
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            max_journal_bytes: DEFAULT_MAX_JOURNAL_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }
}

/// Snapshot serialization belongs to the document codec. `restore` must
/// return a fully validated new state; it must not expose partial restoration.
pub trait SnapshotCodec<S> {
    type Error;
    fn encode(&self, state: &S) -> Result<Vec<u8>, Self::Error>;
    fn restore(&self, payload: &[u8]) -> Result<S, Self::Error>;
}

/// Errors that prevent a snapshot and journal from being associated safely.
#[derive(Debug, PartialEq, Eq)]
pub enum JournalError<E> {
    TooLarge,
    TooManyEntries,
    ArithmeticOverflow,
    AllocationFailed,
    InvalidHeader,
    UnknownVersion(u16),
    DocumentMismatch,
    GenerationMismatch,
    BaseSequenceMismatch { snapshot: u64, journal: u64 },
    SnapshotChecksumMismatch,
    BaseSequenceOverflow,
    SnapshotCodec(E),
    TransactionCodec(CodecError),
}

impl<E: fmt::Debug + fmt::Display> fmt::Display for JournalError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "recovery journal error: {self:?}")
    }
}
impl<E: fmt::Debug + fmt::Display> std::error::Error for JournalError<E> {}

/// Why replay stopped at the first unusable record.
#[derive(Debug, Eq, PartialEq)]
pub enum StopReason<E> {
    Truncated,
    MalformedRecord,
    ChecksumMismatch,
    UnexpectedSequence { expected: u64, actual: u64 },
    LimitExceeded,
    ArithmeticOverflow,
    InvalidTransaction(CodecError),
    ApplicationFailed(TransactionError<E>),
}

/// Result of replay. `last_sequence` is the last transaction successfully
/// applied, or the snapshot's base sequence when no entry was applied.
pub struct ReplayResult<S, E> {
    pub state: S,
    pub last_sequence: u64,
    pub stopped: Option<StopReason<E>>,
    pub applied_entries: usize,
}

/// Build an autonomous snapshot envelope from an already encoded payload.
pub fn encode_snapshot<E>(
    document: RecoveryId,
    generation: RecoveryId,
    base_sequence: u64,
    payload: &[u8],
    limits: JournalLimits,
) -> Result<Vec<u8>, JournalError<E>> {
    let total = SNAPSHOT_HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(JournalError::ArithmeticOverflow)?;
    if total > limits.max_snapshot_bytes || payload.len() > u64::MAX as usize {
        return Err(JournalError::TooLarge);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| JournalError::AllocationFailed)?;
    bytes.extend_from_slice(SNAPSHOT_MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&document.0);
    bytes.extend_from_slice(&generation.0);
    bytes.extend_from_slice(&base_sequence.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(payload);
    let checksum = crc32(&bytes);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    Ok(bytes)
}

/// Encode state through the caller's snapshot codec, then wrap it in a bounded
/// generation envelope.
pub fn snapshot_state<S, C: SnapshotCodec<S>>(
    codec: &C,
    state: &S,
    document: RecoveryId,
    generation: RecoveryId,
    base_sequence: u64,
    limits: JournalLimits,
) -> Result<Vec<u8>, JournalError<C::Error>> {
    let payload = codec.encode(state).map_err(JournalError::SnapshotCodec)?;
    encode_snapshot(document, generation, base_sequence, &payload, limits)
}

/// Append-only in-memory representation of one generation's transaction log.
pub struct Journal {
    bytes: Vec<u8>,
    base_sequence: u64,
    entries: usize,
    limits: JournalLimits,
}

impl Journal {
    pub fn new(
        document: RecoveryId,
        generation: RecoveryId,
        base_sequence: u64,
        limits: JournalLimits,
    ) -> Result<Self, JournalError<core::convert::Infallible>> {
        if JOURNAL_HEADER_BYTES > limits.max_journal_bytes {
            return Err(JournalError::TooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(JOURNAL_HEADER_BYTES)
            .map_err(|_| JournalError::AllocationFailed)?;
        bytes.extend_from_slice(JOURNAL_MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&document.0);
        bytes.extend_from_slice(&generation.0);
        bytes.extend_from_slice(&base_sequence.to_le_bytes());
        Ok(Self {
            bytes,
            base_sequence,
            entries: 0,
            limits,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn entry_count(&self) -> usize {
        self.entries
    }
    pub fn base_sequence(&self) -> u64 {
        self.base_sequence
    }

    /// Append an encoded transaction. Failure leaves the journal byte-for-byte
    /// unchanged. The returned sequence is contiguous from `base_sequence + 1`.
    pub fn append<S, O>(
        &mut self,
        transaction: &Transaction<O>,
    ) -> Result<u64, JournalError<core::convert::Infallible>>
    where
        O: Operation<S>,
    {
        if self.entries >= self.limits.max_entries {
            return Err(JournalError::TooManyEntries);
        }
        let sequence = self
            .base_sequence
            .checked_add(self.entries as u64)
            .and_then(|n| n.checked_add(1))
            .ok_or(JournalError::BaseSequenceOverflow)?;
        let atop = transaction
            .encode::<S>()
            .map_err(JournalError::TransactionCodec)?;
        let frame_len = ENTRY_FIXED_BYTES
            .checked_add(atop.len())
            .ok_or(JournalError::ArithmeticOverflow)?;
        let frame_len_u32 = u32::try_from(frame_len).map_err(|_| JournalError::TooLarge)?;
        let record_bytes = ENTRY_PREFIX_BYTES
            .checked_add(frame_len)
            .ok_or(JournalError::ArithmeticOverflow)?;
        let new_len = self
            .bytes
            .len()
            .checked_add(record_bytes)
            .ok_or(JournalError::ArithmeticOverflow)?;
        if new_len > self.limits.max_journal_bytes {
            return Err(JournalError::TooLarge);
        }
        let mut record = Vec::new();
        record
            .try_reserve_exact(frame_len)
            .map_err(|_| JournalError::AllocationFailed)?;
        record.extend_from_slice(&sequence.to_le_bytes());
        record.extend_from_slice(&(atop.len() as u32).to_le_bytes());
        record.extend_from_slice(&atop);
        let checksum = crc32(&record);
        record.extend_from_slice(&checksum.to_le_bytes());
        self.bytes
            .try_reserve_exact(record_bytes)
            .map_err(|_| JournalError::AllocationFailed)?;
        self.bytes.extend_from_slice(&frame_len_u32.to_le_bytes());
        self.bytes.extend_from_slice(&record);
        self.entries += 1;
        Ok(sequence)
    }
}

/// Replay exactly one matching snapshot and journal. Header incompatibilities
/// are global errors and occur before snapshot restoration or transaction use.
pub fn replay<S, O, C>(
    codec: &C,
    snapshot: &[u8],
    journal: &[u8],
    limits: JournalLimits,
) -> Result<ReplayResult<S, O::Error>, JournalError<C::Error>>
where
    S: TransactionalState,
    O: Operation<S>,
    C: SnapshotCodec<S>,
{
    let (document, generation, snapshot_base, payload) =
        parse_snapshot::<C::Error>(snapshot, limits)?;
    let (journal_document, journal_generation, base_sequence, mut offset) =
        parse_journal_header::<C::Error>(journal, limits)?;
    if document != journal_document {
        return Err(JournalError::DocumentMismatch);
    }
    if generation != journal_generation {
        return Err(JournalError::GenerationMismatch);
    }
    if snapshot_base != base_sequence {
        return Err(JournalError::BaseSequenceMismatch {
            snapshot: snapshot_base,
            journal: base_sequence,
        });
    }

    // Snapshot codecs produce a new value, so failed decoding never leaks a
    // partially restored state to the caller.
    let mut state = codec
        .restore(payload)
        .map_err(JournalError::SnapshotCodec)?;
    let mut last_sequence = base_sequence;
    let mut applied_entries = 0usize;
    let mut stopped = None;
    while offset < journal.len() {
        if applied_entries >= limits.max_entries {
            stopped = Some(StopReason::LimitExceeded);
            break;
        }
        let Some(prefix_end) = offset.checked_add(ENTRY_PREFIX_BYTES) else {
            stopped = Some(StopReason::ArithmeticOverflow);
            break;
        };
        let Some(prefix) = journal.get(offset..prefix_end) else {
            stopped = Some(StopReason::Truncated);
            break;
        };
        let frame_len = u32::from_le_bytes(prefix.try_into().expect("fixed-width slice")) as usize;
        if frame_len < ENTRY_FIXED_BYTES {
            stopped = Some(StopReason::MalformedRecord);
            break;
        }
        let Some(end) = prefix_end.checked_add(frame_len) else {
            stopped = Some(StopReason::ArithmeticOverflow);
            break;
        };
        if end > journal.len() {
            stopped = Some(StopReason::Truncated);
            break;
        }
        // Frame length is bounded by the already-bounded input and ATOP ceiling.
        if frame_len > ENTRY_FIXED_BYTES + crate::MAX_TRANSACTION_BYTES {
            stopped = Some(StopReason::LimitExceeded);
            break;
        }
        let body = &journal[prefix_end..end];
        let seq = u64::from_le_bytes(body[..8].try_into().expect("checked fixed record size"));
        let atop_len =
            u32::from_le_bytes(body[8..12].try_into().expect("checked fixed record size")) as usize;
        let expected_frame = ENTRY_FIXED_BYTES.checked_add(atop_len);
        if expected_frame != Some(frame_len) || atop_len > crate::MAX_TRANSACTION_BYTES {
            stopped = Some(StopReason::MalformedRecord);
            break;
        }
        let crc_offset = 12 + atop_len;
        let stored_crc = u32::from_le_bytes(
            body[crc_offset..crc_offset + 4]
                .try_into()
                .expect("checked frame length"),
        );
        if crc32(&body[..crc_offset]) != stored_crc {
            stopped = Some(StopReason::ChecksumMismatch);
            break;
        }
        let expected_sequence = match last_sequence.checked_add(1) {
            Some(value) => value,
            None => {
                stopped = Some(StopReason::ArithmeticOverflow);
                break;
            }
        };
        if seq != expected_sequence {
            stopped = Some(StopReason::UnexpectedSequence {
                expected: expected_sequence,
                actual: seq,
            });
            break;
        }
        let transaction = match Transaction::<O>::decode::<S>(&body[12..crc_offset]) {
            Ok(value) => value,
            Err(error) => {
                stopped = Some(StopReason::InvalidTransaction(error));
                break;
            }
        };
        if let Err(error) = transaction.apply(&mut state) {
            stopped = Some(StopReason::ApplicationFailed(error));
            break;
        }
        last_sequence = seq;
        applied_entries += 1;
        offset = end;
    }
    Ok(ReplayResult {
        state,
        last_sequence,
        stopped,
        applied_entries,
    })
}

fn parse_snapshot<E>(
    bytes: &[u8],
    limits: JournalLimits,
) -> Result<(RecoveryId, RecoveryId, u64, &[u8]), JournalError<E>> {
    if bytes.len() > limits.max_snapshot_bytes {
        return Err(JournalError::TooLarge);
    }
    if bytes.len() < SNAPSHOT_HEADER_BYTES {
        return Err(JournalError::InvalidHeader);
    }
    if &bytes[..4] != SNAPSHOT_MAGIC {
        return Err(JournalError::InvalidHeader);
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().expect("fixed-width slice"));
    if version != FORMAT_VERSION {
        return Err(JournalError::UnknownVersion(version));
    }
    let document = RecoveryId(bytes[6..22].try_into().expect("fixed-width slice"));
    let generation = RecoveryId(bytes[22..38].try_into().expect("fixed-width slice"));
    let base_sequence = u64::from_le_bytes(bytes[38..46].try_into().expect("fixed-width slice"));
    let len = u64::from_le_bytes(bytes[46..54].try_into().expect("fixed-width slice"));
    let len = usize::try_from(len).map_err(|_| JournalError::TooLarge)?;
    let payload_end = SNAPSHOT_HEADER_BYTES
        .checked_sub(4)
        .and_then(|header| header.checked_add(len))
        .ok_or(JournalError::ArithmeticOverflow)?;
    let end = payload_end
        .checked_add(4)
        .ok_or(JournalError::ArithmeticOverflow)?;
    if end != bytes.len() {
        return Err(JournalError::InvalidHeader);
    }
    let stored_crc = u32::from_le_bytes(
        bytes[payload_end..end]
            .try_into()
            .expect("fixed-width checksum"),
    );
    if crc32(&bytes[..payload_end]) != stored_crc {
        return Err(JournalError::SnapshotChecksumMismatch);
    }
    Ok((
        document,
        generation,
        base_sequence,
        &bytes[SNAPSHOT_HEADER_BYTES - 4..payload_end],
    ))
}

fn parse_journal_header<E>(
    bytes: &[u8],
    limits: JournalLimits,
) -> Result<(RecoveryId, RecoveryId, u64, usize), JournalError<E>> {
    if bytes.len() > limits.max_journal_bytes {
        return Err(JournalError::TooLarge);
    }
    if bytes.len() < JOURNAL_HEADER_BYTES || &bytes[..4] != JOURNAL_MAGIC {
        return Err(JournalError::InvalidHeader);
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().expect("fixed-width slice"));
    if version != FORMAT_VERSION {
        return Err(JournalError::UnknownVersion(version));
    }
    let document = RecoveryId(bytes[6..22].try_into().expect("fixed-width slice"));
    let generation = RecoveryId(bytes[22..38].try_into().expect("fixed-width slice"));
    let base_sequence = u64::from_le_bytes(bytes[38..46].try_into().expect("fixed-width slice"));
    Ok((document, generation, base_sequence, JOURNAL_HEADER_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Decoder, Encoder, IndexedTree, IndexedTreeOperation as Op, Operation, TransactionalState,
    };

    struct TreeCodec;
    impl SnapshotCodec<IndexedTree<i32>> for TreeCodec {
        type Error = &'static str;
        fn encode(&self, tree: &IndexedTree<i32>) -> Result<Vec<u8>, Self::Error> {
            let mut out = Vec::new();
            out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
            for i in 0..tree.len() {
                out.extend_from_slice(&tree.get(i).unwrap().to_le_bytes());
            }
            Ok(out)
        }
        fn restore(&self, payload: &[u8]) -> Result<IndexedTree<i32>, Self::Error> {
            if payload.len() < 4 {
                return Err("short snapshot");
            }
            let count = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            if payload.len() != 4 + count * 4 {
                return Err("invalid snapshot");
            }
            let mut tree = IndexedTree::new();
            for chunk in payload[4..].chunks_exact(4) {
                tree.insert(tree.len(), i32::from_le_bytes(chunk.try_into().unwrap()))
                    .map_err(|_| "tree")?;
            }
            Ok(tree)
        }
    }
    fn id(byte: u8) -> RecoveryId {
        RecoveryId([byte; 16])
    }
    fn snapshot(tree: &IndexedTree<i32>, base_sequence: u64) -> Vec<u8> {
        snapshot_state(
            &TreeCodec,
            tree,
            id(1),
            id(2),
            base_sequence,
            JournalLimits::default(),
        )
        .unwrap()
    }
    fn insert(position: usize, value: i32) -> Transaction<Op<i32>> {
        Transaction::new(vec![Op::Insert { position, value }]).unwrap()
    }

    struct ResourceState {
        value: i32,
        resource: Vec<u8>,
    }

    impl TransactionalState for ResourceState {
        type Checkpoint = (i32, Vec<u8>);

        fn checkpoint(&self) -> Self::Checkpoint {
            (self.value, self.resource.clone())
        }

        fn restore(&mut self, checkpoint: Self::Checkpoint) {
            self.value = checkpoint.0;
            self.resource = checkpoint.1;
        }
    }

    struct AddValue(i32);

    impl Operation<ResourceState> for AddValue {
        type Error = core::convert::Infallible;

        fn apply(&self, state: &mut ResourceState) -> Result<(), Self::Error> {
            state.value += self.0;
            Ok(())
        }

        fn invert(&self, _: &ResourceState) -> Result<Self, Self::Error> {
            Ok(Self(-self.0))
        }

        fn encode(&self, encoder: &mut Encoder) -> Result<(), CodecError> {
            encoder.write_u32(self.0 as u32)
        }

        fn decode(decoder: &mut Decoder<'_>) -> Result<Self, CodecError> {
            Ok(Self(decoder.read_u32()? as i32))
        }
    }

    struct ResourceCodec;

    impl SnapshotCodec<ResourceState> for ResourceCodec {
        type Error = &'static str;

        fn encode(&self, state: &ResourceState) -> Result<Vec<u8>, Self::Error> {
            let resource_len =
                u32::try_from(state.resource.len()).map_err(|_| "resource too large")?;
            let mut payload = Vec::with_capacity(8 + state.resource.len());
            payload.extend_from_slice(&state.value.to_le_bytes());
            payload.extend_from_slice(&resource_len.to_le_bytes());
            payload.extend_from_slice(&state.resource);
            Ok(payload)
        }

        fn restore(&self, payload: &[u8]) -> Result<ResourceState, Self::Error> {
            if payload.len() < 8 {
                return Err("short resource snapshot");
            }
            let value = i32::from_le_bytes(payload[..4].try_into().map_err(|_| "bad value")?);
            let resource_len =
                u32::from_le_bytes(payload[4..8].try_into().map_err(|_| "bad resource size")?)
                    as usize;
            if payload.len() != 8 + resource_len {
                return Err("bad resource payload");
            }
            Ok(ResourceState {
                value,
                resource: payload[8..].to_vec(),
            })
        }
    }

    #[test]
    fn snapshot_and_transactions_replay_in_sequence() {
        let mut base = IndexedTree::new();
        base.insert(0, 4).unwrap();
        let mut log = Journal::new(id(1), id(2), 7, JournalLimits::default()).unwrap();
        assert_eq!(log.append::<IndexedTree<i32>, _>(&insert(1, 5)), Ok(8));
        assert_eq!(log.append::<IndexedTree<i32>, _>(&insert(2, 6)), Ok(9));
        let restored = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 7),
            log.as_bytes(),
            JournalLimits::default(),
        )
        .unwrap();
        assert_eq!(restored.applied_entries, 2);
        assert_eq!(restored.last_sequence, 9);
        assert_eq!(
            (
                restored.state.get(0),
                restored.state.get(1),
                restored.state.get(2)
            ),
            (Some(&4), Some(&5), Some(&6))
        );
        assert_eq!(restored.stopped, None);
    }

    #[test]
    fn snapshot_replay_restores_embedded_resource_after_source_replacement() {
        let original_resource = b"embedded-font-resource\0\xFF".to_vec();
        let mut source = ResourceState {
            value: 10,
            resource: original_resource.clone(),
        };
        let captured = snapshot_state(
            &ResourceCodec,
            &source,
            id(1),
            id(2),
            0,
            JournalLimits::default(),
        )
        .unwrap();
        let mut log = Journal::new(id(1), id(2), 0, JournalLimits::default()).unwrap();
        let transaction = Transaction::new(vec![AddValue(5)]).unwrap();
        log.append::<ResourceState, _>(&transaction).unwrap();

        source = ResourceState {
            value: -900,
            resource: b"replacement-source-resource".to_vec(),
        };
        let result = replay::<ResourceState, AddValue, _>(
            &ResourceCodec,
            &captured,
            log.as_bytes(),
            JournalLimits::default(),
        )
        .unwrap();

        assert_eq!(result.state.value, 15);
        assert_eq!(result.state.resource, original_resource);
        assert_eq!(result.last_sequence, 1);
        assert_eq!(result.stopped, None);
        assert_eq!(
            source.value, -900,
            "replay uses the captured snapshot bytes"
        );
    }

    #[test]
    fn empty_document_snapshot_and_empty_journal_restore_exact_base() {
        let base = IndexedTree::<i32>::new();
        let log = Journal::new(id(1), id(2), 19, JournalLimits::default()).unwrap();
        let result = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 19),
            log.as_bytes(),
            JournalLimits::default(),
        )
        .unwrap();
        assert!(result.state.is_empty());
        assert_eq!(result.last_sequence, 19);
        assert_eq!(result.applied_entries, 0);
        assert_eq!(result.stopped, None);
    }

    #[test]
    fn corrupt_tail_keeps_last_applied_state_and_stops() {
        let base = IndexedTree::new();
        let mut log = Journal::new(id(1), id(2), 0, JournalLimits::default()).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(0, 1)).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(1, 2)).unwrap();
        let mut bytes = log.as_bytes().to_vec();
        *bytes.last_mut().unwrap() ^= 1;
        let result = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 0),
            &bytes,
            JournalLimits::default(),
        )
        .unwrap();
        assert_eq!(result.state.len(), 1);
        assert_eq!(result.last_sequence, 1);
        assert_eq!(result.stopped, Some(StopReason::ChecksumMismatch));
    }

    #[test]
    fn every_truncated_tail_is_bounded_and_never_applied_partially() {
        let base = IndexedTree::new();
        let mut log = Journal::new(id(1), id(2), 0, JournalLimits::default()).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(0, 3)).unwrap();
        let bytes = log.as_bytes();
        for cut in 47..bytes.len() {
            let result = replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &snapshot(&base, 0),
                &bytes[..cut],
                JournalLimits::default(),
            )
            .unwrap();
            assert_eq!(result.state.len(), 0);
            assert_eq!(result.stopped, Some(StopReason::Truncated));
        }
    }

    #[test]
    fn mismatches_and_hostile_size_limits_are_rejected() {
        let base = IndexedTree::new();
        let log = Journal::new(id(9), id(2), 0, JournalLimits::default()).unwrap();
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &snapshot(&base, 0),
                log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::DocumentMismatch)
        ));
        let generation_log = Journal::new(id(1), id(3), 0, JournalLimits::default()).unwrap();
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &snapshot(&base, 0),
                generation_log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::GenerationMismatch)
        ));
        let base_mismatch_log = Journal::new(id(1), id(2), 4, JournalLimits::default()).unwrap();
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &snapshot(&base, 0),
                base_mismatch_log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::BaseSequenceMismatch {
                snapshot: 0,
                journal: 4
            })
        ));
        let mut unknown_snapshot_version = snapshot(&base, 0);
        unknown_snapshot_version[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &unknown_snapshot_version,
                log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::UnknownVersion(2))
        ));
        let mut unknown_journal_version = log.as_bytes().to_vec();
        unknown_journal_version[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &snapshot(&base, 0),
                &unknown_journal_version,
                JournalLimits::default()
            ),
            Err(JournalError::UnknownVersion(2))
        ));
        let tight = JournalLimits {
            max_journal_bytes: 46,
            ..JournalLimits::default()
        };
        let mut bounded = Journal::new(id(1), id(2), 0, tight).unwrap();
        let before = bounded.as_bytes().to_vec();
        assert_eq!(
            bounded.append::<IndexedTree<i32>, _>(&insert(0, 1)),
            Err(JournalError::TooLarge)
        );
        assert_eq!(bounded.as_bytes(), before);
        let mut huge = snapshot(&base, 0);
        huge[46..54].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &huge,
                log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::InvalidHeader
                | JournalError::TooLarge
                | JournalError::ArithmeticOverflow)
        ));
        let mut damaged_snapshot = snapshot(&base, 0);
        damaged_snapshot[38] ^= 1;
        assert!(matches!(
            replay::<IndexedTree<i32>, Op<i32>, _>(
                &TreeCodec,
                &damaged_snapshot,
                log.as_bytes(),
                JournalLimits::default()
            ),
            Err(JournalError::SnapshotChecksumMismatch)
        ));
    }

    #[test]
    fn sequence_gap_and_invalid_atop_stop_at_first_bad_record() {
        let base = IndexedTree::new();
        let mut log = Journal::new(id(1), id(2), 0, JournalLimits::default()).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(0, 1)).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(1, 2)).unwrap();
        let second_prefix =
            46 + 4 + (u32::from_le_bytes(log.as_bytes()[46..50].try_into().unwrap()) as usize);

        let mut gap = log.as_bytes().to_vec();
        gap[second_prefix + 4..second_prefix + 12].copy_from_slice(&3_u64.to_le_bytes());
        let gap_end = second_prefix
            + 4
            + u32::from_le_bytes(gap[second_prefix..second_prefix + 4].try_into().unwrap())
                as usize;
        let checksum = crc32(&gap[second_prefix + 4..gap_end - 4]);
        gap[gap_end - 4..gap_end].copy_from_slice(&checksum.to_le_bytes());
        let result = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 0),
            &gap,
            JournalLimits::default(),
        )
        .unwrap();
        assert_eq!(result.state.len(), 1);
        assert!(matches!(
            result.stopped,
            Some(StopReason::UnexpectedSequence {
                expected: 2,
                actual: 3
            })
        ));

        let mut invalid = log.as_bytes().to_vec();
        invalid[second_prefix + 4 + 12 + 4..second_prefix + 4 + 12 + 6]
            .copy_from_slice(&2_u16.to_le_bytes());
        let invalid_end = second_prefix
            + 4
            + u32::from_le_bytes(
                invalid[second_prefix..second_prefix + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
        let checksum = crc32(&invalid[second_prefix + 4..invalid_end - 4]);
        invalid[invalid_end - 4..invalid_end].copy_from_slice(&checksum.to_le_bytes());
        let result = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 0),
            &invalid,
            JournalLimits::default(),
        )
        .unwrap();
        assert_eq!(result.state.len(), 1);
        assert!(matches!(
            result.stopped,
            Some(StopReason::InvalidTransaction(CodecError::UnknownVersion(
                2
            )))
        ));
    }

    #[test]
    fn failed_application_keeps_previous_state() {
        let base = IndexedTree::new();
        let mut log = Journal::new(id(1), id(2), 0, JournalLimits::default()).unwrap();
        log.append::<IndexedTree<i32>, _>(&insert(0, 1)).unwrap();
        let partially_mutating = Transaction::new(vec![
            Op::Insert {
                position: 1,
                value: 2,
            },
            Op::Insert {
                position: 99,
                value: 3,
            },
        ])
        .unwrap();
        log.append::<IndexedTree<i32>, _>(&partially_mutating)
            .unwrap();
        let result = replay::<IndexedTree<i32>, Op<i32>, _>(
            &TreeCodec,
            &snapshot(&base, 0),
            log.as_bytes(),
            JournalLimits::default(),
        )
        .unwrap();
        assert_eq!(result.state.len(), 1);
        assert!(matches!(
            result.stopped,
            Some(StopReason::ApplicationFailed(_))
        ));
    }
}
