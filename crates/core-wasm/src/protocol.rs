// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict v1 binary message codec shared by the WASM boundary and native code.

use std::fmt;

pub const HEADER_BYTES: usize = 24;
/// Leaves four bytes in the ABI buffer for its status word.
pub const MAX_MESSAGE_BYTES: usize = super::MAX_BUFFER_BYTES - 4;
const MAGIC: [u8; 4] = *b"ATLM";
const VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Ping = 0x0001,
    LoadText = 0x0010,
    ApplyOps = 0x0011,
    Render = 0x0012,
    Cancel = 0x0013,
    Editor = 0x0020,
    Pong = 0x8001,
    Completed = 0x8010,
    Failed = 0x8011,
    Progress = 0x8012,
    Cancelled = 0x8013,
    EditorResult = 0x8020,
}

impl MessageType {
    pub fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            0x0001 => Self::Ping,
            0x0010 => Self::LoadText,
            0x0011 => Self::ApplyOps,
            0x0012 => Self::Render,
            0x0013 => Self::Cancel,
            0x0020 => Self::Editor,
            0x8001 => Self::Pong,
            0x8010 => Self::Completed,
            0x8011 => Self::Failed,
            0x8012 => Self::Progress,
            0x8013 => Self::Cancelled,
            0x8020 => Self::EditorResult,
            _ => return None,
        })
    }

    fn direction(self) -> Direction {
        match self {
            Self::Ping
            | Self::LoadText
            | Self::ApplyOps
            | Self::Render
            | Self::Cancel
            | Self::Editor => Direction::Command,
            Self::Pong
            | Self::Completed
            | Self::Failed
            | Self::Progress
            | Self::Cancelled
            | Self::EditorResult => Direction::Event,
        }
    }

    fn requires_empty_payload(self) -> bool {
        matches!(
            self,
            Self::Ping | Self::Pong | Self::Cancel | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Command,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: MessageType,
    pub request_id: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    MessageTooLarge,
    InvalidRequestId,
    InvalidType,
    WrongDirection,
    InvalidMagic,
    UnsupportedVersion,
    InvalidFlags,
    NonzeroReserved,
    Truncated,
    TrailingBytes,
    PayloadNotEmpty,
    LengthOverflow,
    AllocationFailed,
}

impl ProtocolError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MessageTooLarge => "MESSAGE_TOO_LARGE",
            Self::InvalidRequestId => "INVALID_REQUEST_ID",
            Self::InvalidType => "INVALID_TYPE",
            Self::WrongDirection => "WRONG_DIRECTION",
            Self::InvalidMagic => "INVALID_MAGIC",
            Self::UnsupportedVersion => "UNSUPPORTED_VERSION",
            Self::InvalidFlags => "INVALID_FLAGS",
            Self::NonzeroReserved => "NONZERO_RESERVED",
            Self::Truncated => "TRUNCATED",
            Self::TrailingBytes => "TRAILING_BYTES",
            Self::PayloadNotEmpty => "PAYLOAD_NOT_EMPTY",
            Self::LengthOverflow => "LENGTH_OVERFLOW",
            Self::AllocationFailed => "ALLOCATION_FAILED",
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for ProtocolError {}

pub fn encode(message: &Message) -> Result<Vec<u8>, ProtocolError> {
    if message.request_id == 0 {
        return Err(ProtocolError::InvalidRequestId);
    }
    if message.kind.requires_empty_payload() && !message.payload.is_empty() {
        return Err(ProtocolError::PayloadNotEmpty);
    }
    let total = HEADER_BYTES
        .checked_add(message.payload.len())
        .ok_or(ProtocolError::LengthOverflow)?;
    if total > MAX_MESSAGE_BYTES || message.payload.len() > u32::MAX as usize {
        return Err(ProtocolError::MessageTooLarge);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| ProtocolError::AllocationFailed)?;
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&(message.kind as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&message.request_id.to_le_bytes());
    bytes.extend_from_slice(&(message.payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&message.payload);
    Ok(bytes)
}

pub fn decode(bytes: &[u8], direction: Direction) -> Result<Message, ProtocolError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge);
    }
    if bytes.len() < HEADER_BYTES {
        return Err(ProtocolError::Truncated);
    }
    if bytes[..4] != MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    let kind = MessageType::from_u16(u16::from_le_bytes([bytes[6], bytes[7]]))
        .ok_or(ProtocolError::InvalidType)?;
    if kind.direction() != direction {
        return Err(ProtocolError::WrongDirection);
    }
    if u16::from_le_bytes([bytes[8], bytes[9]]) != 0 {
        return Err(ProtocolError::InvalidFlags);
    }
    if u16::from_le_bytes([bytes[10], bytes[11]]) != 0 {
        return Err(ProtocolError::NonzeroReserved);
    }
    let request_id = u64::from_le_bytes(bytes[12..20].try_into().expect("fixed header range"));
    if request_id == 0 {
        return Err(ProtocolError::InvalidRequestId);
    }
    let payload_len =
        u32::from_le_bytes(bytes[20..24].try_into().expect("fixed header range")) as usize;
    let expected = HEADER_BYTES
        .checked_add(payload_len)
        .ok_or(ProtocolError::LengthOverflow)?;
    if expected > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge);
    }
    if bytes.len() < expected {
        return Err(ProtocolError::Truncated);
    }
    if bytes.len() > expected {
        return Err(ProtocolError::TrailingBytes);
    }
    if kind.requires_empty_payload() && payload_len != 0 {
        return Err(ProtocolError::PayloadNotEmpty);
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_len)
        .map_err(|_| ProtocolError::AllocationFailed)?;
    payload.extend_from_slice(&bytes[HEADER_BYTES..]);
    Ok(Message {
        kind,
        request_id,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_types_round_trip_and_header_is_little_endian() {
        let cases = [
            (MessageType::Ping, Direction::Command),
            (MessageType::LoadText, Direction::Command),
            (MessageType::ApplyOps, Direction::Command),
            (MessageType::Render, Direction::Command),
            (MessageType::Cancel, Direction::Command),
            (MessageType::Editor, Direction::Command),
            (MessageType::Pong, Direction::Event),
            (MessageType::Completed, Direction::Event),
            (MessageType::Failed, Direction::Event),
            (MessageType::Progress, Direction::Event),
            (MessageType::Cancelled, Direction::Event),
            (MessageType::EditorResult, Direction::Event),
        ];
        for (kind, direction) in cases {
            let payload = if kind.requires_empty_payload() {
                Vec::new()
            } else {
                vec![0, 0xff, 0xc3, 0xa9]
            };
            let message = Message {
                kind,
                request_id: u64::MAX,
                payload,
            };
            let encoded = encode(&message).unwrap();
            assert_eq!(&encoded[..4], b"ATLM");
            assert_eq!(decode(&encoded, direction).unwrap(), message);
        }
    }

    #[test]
    fn rejects_every_truncated_prefix_and_trailing_bytes() {
        let encoded = encode(&Message {
            kind: MessageType::LoadText,
            request_id: 9,
            payload: vec![1, 2],
        })
        .unwrap();
        for end in 0..encoded.len() {
            assert!(
                decode(&encoded[..end], Direction::Command).is_err(),
                "prefix {end}"
            );
        }
        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            decode(&trailing, Direction::Command),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn rejects_invalid_fields_direction_ids_and_control_payloads() {
        let valid = encode(&Message {
            kind: MessageType::Ping,
            request_id: 1,
            payload: Vec::new(),
        })
        .unwrap();
        for (offset, value, error) in [
            (0, b'X', ProtocolError::InvalidMagic),
            (4, 2, ProtocolError::UnsupportedVersion),
            (6, 2, ProtocolError::InvalidType),
            (8, 1, ProtocolError::InvalidFlags),
            (10, 1, ProtocolError::NonzeroReserved),
            (12, 0, ProtocolError::InvalidRequestId),
        ] {
            let mut bytes = valid.clone();
            bytes[offset] = value;
            assert_eq!(decode(&bytes, Direction::Command), Err(error));
        }
        assert_eq!(
            decode(&valid, Direction::Event),
            Err(ProtocolError::WrongDirection)
        );
        let mut wrong_len = valid;
        wrong_len[20] = 1;
        wrong_len.push(0);
        assert_eq!(
            decode(&wrong_len, Direction::Command),
            Err(ProtocolError::PayloadNotEmpty)
        );
        assert_eq!(
            encode(&Message {
                kind: MessageType::Cancel,
                request_id: 1,
                payload: vec![1]
            }),
            Err(ProtocolError::PayloadNotEmpty)
        );
    }

    #[test]
    fn copies_payload_and_enforces_size_limit() {
        let payload = vec![42; 32];
        let message = Message {
            kind: MessageType::LoadText,
            request_id: 1,
            payload: payload.clone(),
        };
        let bytes = encode(&message).unwrap();
        let decoded = decode(&bytes, Direction::Command).unwrap();
        assert_eq!(decoded.payload, payload);
        assert_eq!(MAX_MESSAGE_BYTES, super::super::MAX_BUFFER_BYTES - 4);
    }

    #[test]
    fn rejects_oversized_encode_and_declared_payload_before_copying() {
        let payload = vec![0; MAX_MESSAGE_BYTES - HEADER_BYTES + 1];
        assert_eq!(
            encode(&Message {
                kind: MessageType::LoadText,
                request_id: 1,
                payload,
            }),
            Err(ProtocolError::MessageTooLarge)
        );

        let mut header = vec![0; HEADER_BYTES];
        header[..4].copy_from_slice(b"ATLM");
        header[4..6].copy_from_slice(&VERSION.to_le_bytes());
        header[6..8].copy_from_slice(&(MessageType::LoadText as u16).to_le_bytes());
        header[12..20].copy_from_slice(&1u64.to_le_bytes());
        header[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode(&header, Direction::Command),
            Err(ProtocolError::MessageTooLarge)
        );
    }
}
