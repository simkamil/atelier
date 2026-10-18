// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small, manually managed ABI for the WebAssembly build of Atelier.
//!
//! The registry owns every live allocation. Callers can only read or release
//! an allocation when both its pointer and its original length match exactly.

#[cfg(any(target_arch = "wasm32", test))]
use std::collections::BTreeMap;
#[cfg(target_arch = "wasm32")]
use std::sync::{Mutex, OnceLock};

pub mod editor;
mod images;
/// Maximum size of one allocation or dispatch payload (64 MiB).
pub mod properties;
pub mod protocol;
pub mod rich_editor;
pub mod rich_layout;
pub mod rich_preview;
pub mod rich_protocol;
pub mod rich_session;
pub mod staged;

pub const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
/// Maximum number of bytes owned by the ABI at one time (128 MiB).
pub const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;
/// Maximum number of simultaneously active allocations.
pub const MAX_ALLOCATIONS: usize = 1024;

#[cfg(target_arch = "wasm32")]
const STATUS_OK: u32 = 0;
#[cfg(target_arch = "wasm32")]
const STATUS_INVALID_INPUT: u32 = 1;
#[cfg(target_arch = "wasm32")]
const STATUS_LIMIT: u32 = 2;
#[allow(dead_code)]
const STATUS_UNSUPPORTED: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(target_arch = "wasm32", test))]
enum RegistryError {
    Invalid,
    Limit,
}

#[cfg(any(target_arch = "wasm32", test))]
struct AllocationRegistry {
    active: BTreeMap<usize, Box<[u8]>>,
    total_bytes: usize,
    max_buffer_bytes: usize,
    max_total_bytes: usize,
    max_allocations: usize,
}

#[cfg(any(target_arch = "wasm32", test))]
impl AllocationRegistry {
    fn new(max_buffer_bytes: usize, max_total_bytes: usize, max_allocations: usize) -> Self {
        Self {
            active: BTreeMap::new(),
            total_bytes: 0,
            max_buffer_bytes,
            max_total_bytes,
            max_allocations,
        }
    }

    fn allocate_zeroed(&mut self, len: usize) -> Result<usize, RegistryError> {
        if len == 0 {
            return Ok(0);
        }
        self.check_capacity(len)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_| RegistryError::Limit)?;
        bytes.resize(len, 0);
        self.insert(bytes)
    }

    fn insert(&mut self, bytes: Vec<u8>) -> Result<usize, RegistryError> {
        let len = bytes.len();
        self.check_capacity(len)?;
        if len == 0 {
            return Ok(0);
        }
        let boxed = bytes.into_boxed_slice();
        let ptr = boxed.as_ptr() as usize;
        if ptr == 0 || self.active.contains_key(&ptr) {
            return Err(RegistryError::Limit);
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(len)
            .ok_or(RegistryError::Limit)?;
        self.active.insert(ptr, boxed);
        Ok(ptr)
    }

    fn check_capacity(&self, len: usize) -> Result<(), RegistryError> {
        if len > self.max_buffer_bytes
            || self.active.len() >= self.max_allocations
            || self
                .total_bytes
                .checked_add(len)
                .is_none_or(|sum| sum > self.max_total_bytes)
        {
            return Err(RegistryError::Limit);
        }
        Ok(())
    }

    fn take_exact(&mut self, ptr: usize, len: usize) -> Result<Box<[u8]>, RegistryError> {
        if ptr == 0 || len == 0 {
            return Err(RegistryError::Invalid);
        }
        let Some(bytes) = self.active.get(&ptr) else {
            return Err(RegistryError::Invalid);
        };
        if bytes.len() != len {
            return Err(RegistryError::Invalid);
        }
        let bytes = self.active.remove(&ptr).ok_or(RegistryError::Invalid)?;
        self.total_bytes -= bytes.len();
        Ok(bytes)
    }

    /// Remove an input from the active map while keeping its bytes charged
    /// until dispatch either frees it or replaces it with the response payload.
    fn take_for_dispatch(&mut self, ptr: usize, len: usize) -> Result<Box<[u8]>, RegistryError> {
        if ptr == 0 || len == 0 {
            return Err(RegistryError::Invalid);
        }
        let Some(bytes) = self.active.get(&ptr) else {
            return Err(RegistryError::Invalid);
        };
        if bytes.len() != len {
            return Err(RegistryError::Invalid);
        }
        self.active.remove(&ptr).ok_or(RegistryError::Invalid)
    }

    /// Replace the in-flight input charge with its response allocation charge.
    fn replace_in_flight(
        &mut self,
        input_len: usize,
        bytes: Vec<u8>,
    ) -> Result<usize, RegistryError> {
        let base_total = self
            .total_bytes
            .checked_sub(input_len)
            .ok_or(RegistryError::Invalid)?;
        let len = bytes.len();
        let Some(new_total) = base_total.checked_add(len) else {
            self.total_bytes = base_total;
            return Err(RegistryError::Limit);
        };
        if len == 0
            || len > self.max_buffer_bytes
            || self.active.len() >= self.max_allocations
            || new_total > self.max_total_bytes
        {
            self.total_bytes = base_total;
            return Err(RegistryError::Limit);
        }
        let boxed = bytes.into_boxed_slice();
        let ptr = boxed.as_ptr() as usize;
        if ptr == 0 || self.active.contains_key(&ptr) {
            self.total_bytes = base_total;
            return Err(RegistryError::Limit);
        }
        self.total_bytes = new_total;
        self.active.insert(ptr, boxed);
        Ok(ptr)
    }

    /// Release a consumed input after dispatch could not reuse it.
    fn release_in_flight(&mut self, input_len: usize) -> Result<(), RegistryError> {
        self.total_bytes = self
            .total_bytes
            .checked_sub(input_len)
            .ok_or(RegistryError::Invalid)?;
        Ok(())
    }

    fn release_exact(&mut self, ptr: usize, len: usize) -> Result<(), RegistryError> {
        if ptr == 0 && len == 0 {
            return Ok(());
        }
        drop(self.take_exact(ptr, len)?);
        Ok(())
    }

    #[cfg(test)]
    fn allocation_count(&self) -> usize {
        self.active.len()
    }

    #[cfg(test)]
    fn byte_count(&self) -> usize {
        self.total_bytes
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl Default for AllocationRegistry {
    fn default() -> Self {
        Self::new(MAX_BUFFER_BYTES, MAX_TOTAL_BYTES, MAX_ALLOCATIONS)
    }
}

#[cfg(target_arch = "wasm32")]
fn registry() -> &'static Mutex<AllocationRegistry> {
    static REGISTRY: OnceLock<Mutex<AllocationRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(AllocationRegistry::default()))
}

#[cfg(target_arch = "wasm32")]
fn staged_engine() -> &'static Mutex<staged::StagedEngine> {
    static STAGED: OnceLock<Mutex<staged::StagedEngine>> = OnceLock::new();
    STAGED.get_or_init(|| Mutex::new(staged::StagedEngine::default()))
}

/// The rich editor (ATED v2) of this instance: one active document, edited
/// in place.
#[cfg(target_arch = "wasm32")]
fn rich_editor_session() -> &'static Mutex<rich_editor::RichEditor> {
    static RICH: OnceLock<Mutex<rich_editor::RichEditor>> = OnceLock::new();
    RICH.get_or_init(|| Mutex::new(rich_editor::RichEditor::default()))
}

#[cfg(target_arch = "wasm32")]
fn prefix_status(mut body: Vec<u8>, status: u32) -> Result<Vec<u8>, RegistryError> {
    let old_len = body.len();
    let new_len = 4usize.checked_add(old_len).ok_or(RegistryError::Limit)?;
    if new_len > MAX_BUFFER_BYTES {
        return Err(RegistryError::Limit);
    }
    body.try_reserve_exact(4)
        .map_err(|_| RegistryError::Limit)?;
    body.resize(new_len, 0);
    body.copy_within(0..old_len, 4);
    body[..4].copy_from_slice(&status.to_le_bytes());
    Ok(body)
}

#[cfg(target_arch = "wasm32")]
fn dispatch_inner(ptr: u32, len: u32) -> u32 {
    let mut registry = registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let (input, input_charge) = if ptr == 0 && len == 0 {
        (Some(Vec::new()), 0)
    } else {
        match registry.take_for_dispatch(ptr as usize, len as usize) {
            Ok(bytes) => (Some(bytes.into_vec()), len as usize),
            Err(_) => (None, 0),
        }
    };

    if input.is_none() {
        let payload = match prefix_status(Vec::new(), STATUS_INVALID_INPUT) {
            Ok(payload) => payload,
            Err(_) => return 0,
        };
        return register_response(&mut registry, payload);
    }

    let input = input.expect("checked above");
    #[cfg(all(feature = "abi-test-hooks", target_arch = "wasm32"))]
    if input.as_slice() == b"__atelier_test_panic__" {
        panic!("US-090 test hook: deliberate WASM trap");
    }

    let editor_type = (protocol::MessageType::Editor as u16).to_le_bytes();
    let is_editor = input.starts_with(b"ATLM")
        && input
            .get(6..8)
            .is_some_and(|kind| kind == editor_type.as_slice());
    if is_editor {
        return match protocol::decode(&input, protocol::Direction::Command) {
            Ok(message) => dispatch_editor(&mut registry, input_charge, &message),
            Err(_) => {
                let request_id = input
                    .get(12..20)
                    .map(|bytes| u64::from_le_bytes(bytes.try_into().expect("fixed slice")))
                    .unwrap_or(0);
                dispatch_editor_failure(
                    &mut registry,
                    input_charge,
                    request_id,
                    "EDITOR_INVALID_ENVELOPE",
                )
            }
        };
    }

    let payload = match prefix_status(input, STATUS_OK) {
        Ok(payload) => payload,
        Err(_) => {
            if registry.release_in_flight(input_charge).is_err() {
                return 0;
            }
            return match prefix_status(Vec::new(), STATUS_LIMIT) {
                Ok(payload) => register_response(&mut registry, payload),
                Err(_) => 0,
            };
        }
    };

    if input_charge == 0 {
        return match registry.insert(payload) {
            Ok(payload_ptr) => register_descriptor(&mut registry, payload_ptr),
            Err(_) => 0,
        };
    }

    match registry.replace_in_flight(input_charge, payload) {
        Ok(payload_ptr) => register_descriptor(&mut registry, payload_ptr),
        Err(RegistryError::Limit) => match prefix_status(Vec::new(), STATUS_LIMIT) {
            Ok(payload) => register_response(&mut registry, payload),
            Err(_) => 0,
        },
        Err(RegistryError::Invalid) => 0,
    }
}

#[cfg(target_arch = "wasm32")]
fn dispatch_editor(
    registry: &mut AllocationRegistry,
    input_charge: usize,
    message: &protocol::Message,
) -> u32 {
    let was_rich = rich_editor::is_rich(&message.payload);
    let response = if rich_editor::is_rich(&message.payload) {
        let mut rich = rich_editor_session()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        rich.respond(message)
    } else if staged::is_staged(&message.payload) {
        let mut staged = staged_engine()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        staged.respond(message)
    } else {
        // The plain-text editor (v1) is gone: only rich and staged commands remain.
        protocol::Message {
            kind: protocol::MessageType::Failed,
            request_id: message.request_id,
            payload: b"EDITOR_INVALID_COMMAND".to_vec(),
        }
    };
    let frame = match protocol::encode(&response) {
        Ok(frame) => frame,
        Err(_) => {
            let failed = protocol::Message {
                kind: protocol::MessageType::Failed,
                request_id: message.request_id,
                payload: b"EDITOR_RESPONSE_LIMIT".to_vec(),
            };
            if was_rich {
                mark_rich_resync();
            }
            match protocol::encode(&failed) {
                Ok(bytes) => bytes,
                Err(_) => {
                    let _ = registry.release_in_flight(input_charge);
                    return 0;
                }
            }
        }
    };
    let payload = match prefix_status(frame, STATUS_OK) {
        Ok(payload) => payload,
        Err(_) => {
            let _ = registry.release_in_flight(input_charge);
            return 0;
        }
    };
    match registry.replace_in_flight(input_charge, payload) {
        Ok(payload_ptr) => {
            let descriptor = register_descriptor(registry, payload_ptr);
            descriptor
        }
        Err(_) => {
            // The client never saw the reply: it resynchronises.
            if was_rich {
                mark_rich_resync();
            }
            match prefix_status(Vec::new(), STATUS_LIMIT) {
                Ok(payload) => register_response(registry, payload),
                Err(_) => 0,
            }
        }
    }
}

/// Tells the rich editor that its last reply was lost.
#[cfg(target_arch = "wasm32")]
fn mark_rich_resync() {
    rich_editor_session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .mark_resync();
}

#[cfg(target_arch = "wasm32")]
fn dispatch_editor_failure(
    registry: &mut AllocationRegistry,
    input_charge: usize,
    request_id: u64,
    code: &'static str,
) -> u32 {
    if request_id == 0 {
        let _ = registry.release_in_flight(input_charge);
        return match prefix_status(Vec::new(), STATUS_INVALID_INPUT) {
            Ok(payload) => register_response(registry, payload),
            Err(_) => 0,
        };
    }
    let frame = match protocol::encode(&protocol::Message {
        kind: protocol::MessageType::Failed,
        request_id,
        payload: code.as_bytes().to_vec(),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = registry.release_in_flight(input_charge);
            return 0;
        }
    };
    let payload = match prefix_status(frame, STATUS_OK) {
        Ok(payload) => payload,
        Err(_) => {
            let _ = registry.release_in_flight(input_charge);
            return 0;
        }
    };
    match registry.replace_in_flight(input_charge, payload) {
        Ok(ptr) => register_descriptor(registry, ptr),
        Err(_) => match prefix_status(Vec::new(), STATUS_LIMIT) {
            Ok(payload) => register_response(registry, payload),
            Err(_) => 0,
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn register_response(registry: &mut AllocationRegistry, payload: Vec<u8>) -> u32 {
    let payload_ptr = match registry.insert(payload) {
        Ok(ptr) => ptr,
        Err(_) => return 0,
    };
    register_descriptor(registry, payload_ptr)
}

#[cfg(target_arch = "wasm32")]
fn register_descriptor(registry: &mut AllocationRegistry, payload_ptr: usize) -> u32 {
    let payload_size = match registry.active.get(&payload_ptr) {
        Some(payload) => payload.len(),
        None => return 0,
    };
    let payload_len = match u32::try_from(payload_size) {
        Ok(len) => len,
        Err(_) => {
            let _ = registry.release_exact(payload_ptr, payload_size);
            return 0;
        }
    };
    let payload_ptr_u32 = match u32::try_from(payload_ptr) {
        Ok(ptr) => ptr,
        Err(_) => {
            let _ = registry.release_exact(payload_ptr, payload_size);
            return 0;
        }
    };
    let mut descriptor = Vec::new();
    if descriptor.try_reserve_exact(8).is_err() {
        let _ = registry.release_exact(payload_ptr, payload_size);
        return 0;
    }
    descriptor.extend_from_slice(&payload_ptr_u32.to_le_bytes());
    descriptor.extend_from_slice(&payload_len.to_le_bytes());
    let descriptor_ptr = match registry.insert(descriptor) {
        Ok(ptr) => ptr,
        Err(_) => {
            let _ = registry.release_exact(payload_ptr, payload_size);
            return 0;
        }
    };
    u32::try_from(descriptor_ptr).unwrap_or_else(|_| {
        let _ = registry.release_exact(descriptor_ptr, 8);
        let _ = registry.release_exact(payload_ptr, payload_size);
        0
    })
}

/// Allocate a zero-filled region owned by the ABI registry. Zero means failure
/// or the specified successful zero-length allocation.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> u32 {
    match registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .allocate_zeroed(len as usize)
    {
        Ok(ptr) => u32::try_from(ptr).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Release an allocation only when pointer and size match its registry entry.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn free(ptr: u32, len: u32) -> i32 {
    match registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .release_exact(ptr as usize, len as usize)
    {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Echo the request into a separately owned payload and return its descriptor.
/// A zero result is a fatal ABI failure; callers should recreate the instance.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn dispatch(ptr: u32, len: u32) -> u32 {
    dispatch_inner(ptr, len)
}

#[cfg(test)]
mod tests {
    use super::{AllocationRegistry, RegistryError};

    #[test]
    fn exact_pointer_and_length_are_required_for_release() {
        let mut registry = AllocationRegistry::new(16, 32, 4);
        let ptr = registry.allocate_zeroed(7).expect("allocation");

        assert_eq!(registry.release_exact(ptr, 6), Err(RegistryError::Invalid));
        assert_eq!(
            registry.release_exact(ptr + 1, 7),
            Err(RegistryError::Invalid)
        );
        assert_eq!(registry.allocation_count(), 1);
        assert_eq!(registry.byte_count(), 7);
        assert_eq!(registry.release_exact(ptr, 7), Ok(()));
        assert_eq!(registry.release_exact(ptr, 7), Err(RegistryError::Invalid));
        assert_eq!(registry.byte_count(), 0);
    }

    #[test]
    fn zero_length_is_noop_and_never_enters_the_registry() {
        let mut registry = AllocationRegistry::new(16, 32, 4);
        assert_eq!(registry.allocate_zeroed(0), Ok(0));
        assert_eq!(registry.release_exact(0, 0), Ok(()));
        assert_eq!(registry.release_exact(0, 1), Err(RegistryError::Invalid));
        assert_eq!(registry.allocation_count(), 0);
    }

    #[test]
    fn allocation_limits_are_checked_before_allocation() {
        let mut registry = AllocationRegistry::new(8, 10, 2);
        let first = registry.allocate_zeroed(6).expect("first");
        assert_eq!(registry.allocate_zeroed(5), Err(RegistryError::Limit));
        let second = registry.allocate_zeroed(4).expect("second");
        assert_eq!(registry.allocate_zeroed(1), Err(RegistryError::Limit));
        assert_eq!(registry.allocation_count(), 2);
        assert_eq!(registry.release_exact(first, 6), Ok(()));
        assert_eq!(registry.allocate_zeroed(9), Err(RegistryError::Limit));
        assert_eq!(registry.release_exact(second, 4), Ok(()));
    }

    #[test]
    fn exact_take_transfers_ownership_and_updates_totals() {
        let mut registry = AllocationRegistry::new(16, 32, 4);
        let ptr = registry.insert(vec![1, 2, 3]).expect("insert");
        let owned = registry.take_exact(ptr, 3).expect("take");
        assert_eq!(&*owned, &[1, 2, 3]);
        assert_eq!(registry.allocation_count(), 0);
        assert_eq!(registry.byte_count(), 0);
    }

    #[test]
    fn in_flight_dispatch_input_remains_charged_until_replaced_or_released() {
        let mut registry = AllocationRegistry::new(16, 10, 4);
        let input_ptr = registry.insert(vec![1; 6]).expect("input");
        let input = registry
            .take_for_dispatch(input_ptr, 6)
            .expect("consume input");
        assert_eq!(registry.allocation_count(), 0);
        assert_eq!(registry.byte_count(), 6, "consumed input remains budgeted");
        assert_eq!(registry.allocate_zeroed(5), Err(RegistryError::Limit));

        let response_ptr = registry
            .replace_in_flight(6, [0, 0, 0, 0, 1, 2, 3, 4].to_vec())
            .expect("replace input with response");
        drop(input);
        assert_eq!(registry.allocation_count(), 1);
        assert_eq!(
            registry.byte_count(),
            8,
            "replacement adjusts charge, without double count"
        );
        registry
            .release_exact(response_ptr, 8)
            .expect("release response");

        let second_ptr = registry.insert(vec![9; 4]).expect("second input");
        let second = registry
            .take_for_dispatch(second_ptr, 4)
            .expect("consume second");
        drop(second);
        registry
            .release_in_flight(4)
            .expect("release consumed input");
        assert_eq!(registry.byte_count(), 0);
    }
}
