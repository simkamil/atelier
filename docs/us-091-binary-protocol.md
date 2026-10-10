# US-091 — Binary message protocol v1

This codec defines messages carried over the US-090 byte-echo ABI. It does not change that ABI or implement a worker, queue, handler, document operation, or execution order. The payload remains opaque bytes; application payload schemas belong to later stories.

## Wire format

Every integer is little-endian. A v1 message is exactly one 24-byte header followed by exactly `payload_len` bytes. The complete frame, including its header, must be at most `MAX_MESSAGE_BYTES = super::MAX_BUFFER_BYTES - 4` (64 MiB minus the four-byte ABI status word).

| Offset | Width | Field | Required value / meaning |
|---:|---:|---|---|
| 0 | 4 | Magic | ASCII `ATLM` |
| 4 | 2 | Version | `1` |
| 6 | 2 | Type | One of the values below |
| 8 | 2 | Flags | `0` |
| 10 | 2 | Reserved | `0` |
| 12 | 8 | Request ID | Unsigned, nonzero `u64`; JS uses `BigInt` only |
| 20 | 4 | Payload length | Number of following payload bytes |
| 24 | variable | Payload | Opaque bytes; no UTF-8 assumption |

The declared payload length must match the frame exactly. A short header or payload is truncated; bytes beyond the declared payload are trailing data. Rust checks size additions and bounds before allocation or copying. JavaScript first validates the argument and direction, then checks the received frame's minimum and maximum sizes. It copies that bounded frame into an owned `Uint8Array`, validates content, header fields, and declared length exclusively against the snapshot, then returns an independent payload copy. A `SharedArrayBuffer`-backed input is copied as well; mutations after that copy cannot affect the returned payload. The copy does not promise an atomic snapshot if shared memory is concurrently written while the copy itself is in progress.

## Message types and directions

| Type | Value | Direction | Payload |
|---|---:|---|---|
| Ping | `0x0001` | Command | Must be empty |
| LoadText | `0x0010` | Command | Opaque |
| ApplyOps | `0x0011` | Command | Opaque |
| Render | `0x0012` | Command | Opaque |
| Cancel | `0x0013` | Command | Must be empty; request ID identifies the operation being cancelled |
| Pong | `0x8001` | Event | Must be empty |
| Completed | `0x8010` | Event | Opaque |
| Failed | `0x8011` | Event | Opaque |
| Progress | `0x8012` | Event | Opaque |
| Cancelled | `0x8013` | Event | Must be empty |

Events carry the request ID of their request. IDs range from 1 through `2^64 - 1`; IDs are not converted to JavaScript `Number`. No codec behavior implies request scheduling or execution.

## APIs

Rust exposes `MessageType`, `Direction::{Command, Event}`, `Message { kind, request_id, payload }`, `encode(&Message) -> Result<Vec<u8>, ProtocolError>` and `decode(&[u8], Direction) -> Result<Message, ProtocolError>` from `crates/core-wasm/src/protocol.rs`.

JavaScript ES modules expose frozen `MessageType` constants, `encodeMessage({ type, requestId, payload })` and `decodeMessage(bytes, direction)`. `direction` is required and must be `'command'` or `'event'`. `payload` and `bytes` must be `Uint8Array`; views are honored at their `byteOffset`. Encoding copies its payload and decoding returns an independent payload copy. `requestId` must be a `BigInt`.

## Stable error codes

JavaScript throws `ProtocolError` with the following stable `code`. Rust `ProtocolError::code()` uses the corresponding wire-validation codes; allocation and API argument errors are only applicable where that API can produce them.

| Code | Meaning |
|---|---|
| `INVALID_ARGUMENT` | JS input is not a `Uint8Array` where required |
| `INVALID_DIRECTION` | JS direction is missing or not `command` / `event` |
| `INVALID_TYPE` | Type is unknown or not a valid v1 type |
| `WRONG_DIRECTION` | A command type was decoded as an event, or vice versa |
| `INVALID_MAGIC` | Magic is not `ATLM` |
| `UNSUPPORTED_VERSION` | Version is not 1 |
| `INVALID_FLAGS` | Flags are nonzero |
| `NONZERO_RESERVED` | Reserved field is nonzero |
| `INVALID_REQUEST_ID` | ID is zero, outside `u64`, or not a JS `BigInt` |
| `TRUNCATED` | Header or declared payload is incomplete |
| `TRAILING_BYTES` | Frame contains undeclared bytes |
| `PAYLOAD_NOT_EMPTY` | Ping, Pong, Cancel, or Cancelled has payload bytes |
| `MESSAGE_TOO_LARGE` | Complete frame exceeds the protocol limit |
| `LENGTH_OVERFLOW` | Rust checked size arithmetic overflowed |
| `ALLOCATION_FAILED` | Rust could not reserve memory for a validated frame |

The oracle example uses hexadecimal text solely as a native-test adapter; it is not a production protocol transport. The optional WASM test argument passes frames through the unchanged US-090 echo dispatch and decodes the returned bytes.
