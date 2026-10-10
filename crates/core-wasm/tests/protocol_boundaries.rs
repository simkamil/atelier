// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_core_wasm::protocol::{
    Direction, HEADER_BYTES, MAX_MESSAGE_BYTES, Message, MessageType, ProtocolError, decode, encode,
};

#[test]
fn exact_transport_limit_round_trips_and_oversized_input_is_rejected() {
    let payload = vec![0x5a; MAX_MESSAGE_BYTES - HEADER_BYTES];
    let message = Message {
        kind: MessageType::ApplyOps,
        request_id: 1u64 << 63,
        payload,
    };
    let mut frame = encode(&message).unwrap();
    assert_eq!(frame.len(), MAX_MESSAGE_BYTES);
    let decoded = decode(&frame, Direction::Command).unwrap();
    assert_eq!(decoded.request_id, 1u64 << 63);
    assert_eq!(decoded.payload.len(), message.payload.len());
    assert!(decoded.payload.iter().all(|&v| v == 0x5a));
    drop(decoded);
    frame.push(0);
    assert_eq!(
        decode(&frame, Direction::Command),
        Err(ProtocolError::MessageTooLarge)
    );
}
